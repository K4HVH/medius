//! Hardware injection validation (Linux only).
//!
//! Drives injection through the `medius` library while exclusively grabbing the clone's mouse event
//! node (`EVIOCGRAB`) so injected motion is measured here and never reaches the desktop. Checks
//! observed-vs-injected for motion, wheel, button press/release, the 1 s silence auto-clear
//! (no-stuck), and sustained 1 kHz no-halving via the `MovementSession` pacer.
//!
//! ```text
//! cargo run --example hw_validate -- [event_node=/dev/input/event11] [port]
//! ```
//! Needs read access to the event node (uaccess ACL, else run as root). The port defaults to the
//! first medius box by VID/PID.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("hw_validate is Linux-only (uses evdev EVIOCGRAB).");
}

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    linux::run()
}

#[cfg(target_os = "linux")]
mod linux {
    use std::os::fd::RawFd;
    use std::process::ExitCode;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
    use std::time::Duration;

    use medius::{Button, Device};

    // evdev constants (Linux UAPI, arch-invariant for these).
    const EVIOCGRAB: libc::c_ulong = 0x4004_4590; // _IOW('E', 0x90, int)
    const EV_KEY: u16 = 0x01;
    const EV_REL: u16 = 0x02;
    const REL_X: u16 = 0x00;
    const REL_Y: u16 = 0x01;
    const REL_WHEEL: u16 = 0x08;
    const BTN_LEFT: u16 = 0x110;
    const BTN_RIGHT: u16 = 0x111;
    /// `struct input_event` on 64-bit Linux: `timeval{i64,i64}` + `u16 type` + `u16 code` + `i32 value`.
    const EVENT_SIZE: usize = 24;

    /// Accumulators populated by the reader thread from the grabbed event stream.
    #[derive(Default)]
    struct Acc {
        rel_x: AtomicI64,
        rel_y: AtomicI64,
        rel_wheel: AtomicI64,
        rel_x_events: AtomicI64,
        btn_left: AtomicI64,
        btn_right: AtomicI64,
    }

    /// Owns the grabbed fd; releases the grab and closes the fd on drop (even on panic), so injected
    /// motion can't leak to the desktop after we exit.
    struct EvdevGrab {
        fd: RawFd,
    }

    impl EvdevGrab {
        fn open(path: &str) -> std::io::Result<Self> {
            let cpath = std::ffi::CString::new(path).unwrap();
            // SAFETY: valid C string and flags. O_NONBLOCK so the reader polls `stop` rather than
            // blocking in read() when injection is idle (clean shutdown).
            let fd = unsafe {
                libc::open(
                    cpath.as_ptr(),
                    libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NONBLOCK,
                )
            };
            if fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            // SAFETY: fd is valid; EVIOCGRAB with arg 1 takes exclusive ownership of the device.
            let rc = unsafe { libc::ioctl(fd, EVIOCGRAB, 1) };
            if rc < 0 {
                let e = std::io::Error::last_os_error();
                // SAFETY: closing the fd we just opened.
                unsafe { libc::close(fd) };
                return Err(e);
            }
            Ok(EvdevGrab { fd })
        }
    }

    impl Drop for EvdevGrab {
        fn drop(&mut self) {
            // SAFETY: releasing the grab and closing our owned fd exactly once.
            unsafe {
                libc::ioctl(self.fd, EVIOCGRAB, 0);
                libc::close(self.fd);
            }
        }
    }

    /// Read 24-byte `input_event` records from `fd`, folding REL_*/BTN_* into `acc`, until `stop`.
    fn reader(fd: RawFd, acc: Arc<Acc>, stop: Arc<AtomicBool>) {
        let mut buf = [0u8; EVENT_SIZE];
        while !stop.load(Ordering::Relaxed) {
            // SAFETY: fd is valid; we read into a buffer of exactly EVENT_SIZE bytes.
            let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, EVENT_SIZE) };
            if n != EVENT_SIZE as isize {
                // EAGAIN / partial: nap, then re-check `stop`. Buffered events drain back-to-back.
                std::thread::sleep(std::time::Duration::from_millis(1));
                continue;
            }
            let typ = u16::from_ne_bytes([buf[16], buf[17]]);
            let code = u16::from_ne_bytes([buf[18], buf[19]]);
            let val = i32::from_ne_bytes([buf[20], buf[21], buf[22], buf[23]]) as i64;
            match typ {
                EV_REL => match code {
                    REL_X => {
                        acc.rel_x.fetch_add(val, Ordering::Relaxed);
                        acc.rel_x_events.fetch_add(1, Ordering::Relaxed);
                    }
                    REL_Y => {
                        acc.rel_y.fetch_add(val, Ordering::Relaxed);
                    }
                    REL_WHEEL => {
                        acc.rel_wheel.fetch_add(val, Ordering::Relaxed);
                    }
                    _ => {}
                },
                EV_KEY => match code {
                    BTN_LEFT => {
                        acc.btn_left.store(val, Ordering::Relaxed);
                    }
                    BTN_RIGHT => {
                        acc.btn_right.store(val, Ordering::Relaxed);
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    pub fn run() -> ExitCode {
        let args: Vec<String> = std::env::args().collect();
        let event = args
            .get(1)
            .cloned()
            .unwrap_or_else(|| "/dev/input/event11".to_string());

        // Grab FIRST so nothing leaks to the desktop, then start reading.
        let grab = match EvdevGrab::open(&event) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("cannot grab {event}: {e} (try a different event node, or run as root)");
                return ExitCode::FAILURE;
            }
        };
        let acc = Arc::new(Acc::default());
        let stop = Arc::new(AtomicBool::new(false));
        let rfd = grab.fd;
        let racc = Arc::clone(&acc);
        let rstop = Arc::clone(&stop);
        let reader = std::thread::spawn(move || reader(rfd, racc, rstop));
        std::thread::sleep(Duration::from_millis(300));

        let device = match args.get(2) {
            Some(p) => Device::open(p),
            None => Device::find(),
        };
        let device = match device {
            Ok(d) => d,
            Err(e) => {
                eprintln!("cannot open medius box: {e}");
                stop.store(true, Ordering::Relaxed);
                return ExitCode::FAILURE;
            }
        };
        println!("connected: {:?}", device.query_version().ok());
        println!("grabbed {event} — injected motion is captured here, NOT sent to the desktop\n");

        let mut ok = true;

        // 1) MOTION — 50 moves of +40 via the frame clock (physical mouse idle).
        acc.rel_x.store(0, Ordering::Relaxed);
        acc.rel_y.store(0, Ordering::Relaxed);
        for _ in 0..50 {
            let _ = device.move_rel(40, 0);
            std::thread::sleep(Duration::from_millis(3));
        }
        std::thread::sleep(Duration::from_millis(400));
        let x = acc.rel_x.load(Ordering::Relaxed);
        let y = acc.rel_y.load(Ordering::Relaxed);
        let motion_ok = x == 2000;
        ok &= motion_ok;
        println!(
            "[motion ] injected X=2000  observed REL_X={x}  (Y drift={y})  {}",
            pf(motion_ok)
        );

        // 2) WHEEL — +5.
        acc.rel_wheel.store(0, Ordering::Relaxed);
        for _ in 0..5 {
            let _ = device.wheel(1);
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(300));
        let w = acc.rel_wheel.load(Ordering::Relaxed);
        let wheel_ok = w == 5;
        ok &= wheel_ok;
        println!(
            "[wheel  ] injected +5     observed REL_WHEEL={w}  {}",
            pf(wheel_ok)
        );

        // 3) BUTTON — press left, then soft-release.
        let _ = device.press(Button::Left);
        std::thread::sleep(Duration::from_millis(250));
        let pressed = acc.btn_left.load(Ordering::Relaxed);
        let inj = device
            .query_health()
            .map(|h| h.injection_active)
            .unwrap_or(false);
        let _ = device.release(Button::Left);
        std::thread::sleep(Duration::from_millis(250));
        let released = acc.btn_left.load(Ordering::Relaxed);
        let button_ok = pressed == 1 && released == 0 && inj;
        ok &= button_ok;
        println!(
            "[button ] left pressed={pressed} (inj_active={inj}) -> released={released}  {}",
            pf(button_ok)
        );

        // 4) NO-STUCK — force a press, go silent, the 1 s firmware auto-clear must release it.
        let _ = device.button(Button::Right, medius::ButtonAction::Press);
        std::thread::sleep(Duration::from_millis(250));
        let rp = acc.btn_right.load(Ordering::Relaxed);
        println!("[nostuck] right pressed={rp}; going silent 1.4 s ...");
        std::thread::sleep(Duration::from_millis(1400));
        let rr = acc.btn_right.load(Ordering::Relaxed);
        let inj2 = device
            .query_health()
            .map(|h| h.injection_active)
            .unwrap_or(true);
        let nostuck_ok = rp == 1 && rr == 0 && !inj2;
        ok &= nostuck_ok;
        println!(
            "[nostuck] after silence: right={rr}  inj_active={inj2}  {}",
            pf(nostuck_ok)
        );

        // 5) 1 kHz NO-HALVING — the headline: a push loop feeding (1,0) at ~1 kHz for 1 s should
        //    deliver ~1000 reports (halving would show ~500).
        let _ = device.reset();
        std::thread::sleep(Duration::from_millis(100));
        acc.rel_x.store(0, Ordering::Relaxed);
        acc.rel_x_events.store(0, Ordering::Relaxed);
        let session = device.movement();
        // Operator-driven push loop: feed one delta per ~1 ms; the pacer emits one MOVE per tick.
        let deadline = std::time::Instant::now() + Duration::from_millis(1000);
        while std::time::Instant::now() < deadline {
            session.push(1, 0);
            std::thread::sleep(Duration::from_millis(1));
        }
        drop(session);
        std::thread::sleep(Duration::from_millis(100));
        let events = acc.rel_x_events.load(Ordering::Relaxed);
        let sum = acc.rel_x.load(Ordering::Relaxed);
        // 998 Hz measured; accept ≥950 reports/s as "full rate, no halving" (push timing isn't exact,
        // so judge on report count, with sum ≥ reports to confirm motion was delivered).
        let pace_ok = events >= 950 && sum >= events;
        ok &= pace_ok;
        println!(
            "[1kHz   ] push(1,0) loop for 1s -> {events} reports, sum REL_X={sum}  (>=950 = no-halving)  {}",
            pf(pace_ok)
        );
        let _ = device.reset();

        stop.store(true, Ordering::Relaxed);
        let _ = reader.join(); // non-blocking reader observes `stop` within ~1 ms
        drop(grab);

        println!("\nRESULT: {}", if ok { "PASS ✓" } else { "FAIL ✗" });
        if ok {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }

    fn pf(b: bool) -> &'static str {
        if b { "✓" } else { "✗ FAIL" }
    }
}
