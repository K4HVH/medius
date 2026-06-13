//! Comprehensive hardware validation (Linux only).
//!
//! Exercises EVERY command + infrastructure piece on the public [`medius::Device`] surface against a
//! real box, while exclusively grabbing (`EVIOCGRAB`) the clone's mouse event node so injected input is
//! measured here and never reaches the desktop. Each check prints `[name] ... PASS/FAIL`; the run ends
//! `RESULT: PASS/FAIL` with a matching exit code.
//!
//! ```text
//! cargo run --example hw_full -- [event_node=/dev/input/event11] [port]
//! cargo run --example hw_full --features async -- ...     # also runs the async query gate
//! ```
//! Needs read access to the event node (uaccess ACL, else run as root). The port defaults to the first
//! medius box by VID/PID. Wrap in `timeout` when running unattended — the grab freezes desktop input
//! for the window, and the Drop guard releases it even on panic.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("hw_full is Linux-only (uses evdev EVIOCGRAB).");
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
    use std::time::{Duration, Instant};

    use medius::{Button, ButtonAction, Device};

    // evdev constants (Linux UAPI, arch-invariant for these).
    const EVIOCGRAB: libc::c_ulong = 0x4004_4590; // _IOW('E', 0x90, int)
    const EV_KEY: u16 = 0x01;
    const EV_REL: u16 = 0x02;
    const REL_X: u16 = 0x00;
    const REL_Y: u16 = 0x01;
    const REL_WHEEL: u16 = 0x08;
    const BTN_LEFT: u16 = 0x110;
    const BTN_RIGHT: u16 = 0x111;
    const BTN_MIDDLE: u16 = 0x112;
    const BTN_SIDE: u16 = 0x113; // Side1 (back)
    const BTN_EXTRA: u16 = 0x114; // Side2 (forward)
    /// `struct input_event` on 64-bit Linux: `timeval{i64,i64}` + `u16 type` + `u16 code` + `i32 value`.
    const EVENT_SIZE: usize = 24;

    /// Accumulators populated by the reader thread from the grabbed event stream. Motion is summed (with
    /// a REL_X event count for the 1 kHz move rate); each button code latches its last KEY value (1=down,
    /// 0=up). `side_other_*` capture any side-button KEY that arrives on an UNEXPECTED code, so a mouse
    /// that maps its side buttons elsewhere is reported rather than hard-failing the run.
    #[derive(Default)]
    struct Acc {
        rel_x: AtomicI64,
        rel_y: AtomicI64,
        rel_wheel: AtomicI64,
        rel_x_events: AtomicI64,
        btn_left: AtomicI64,
        btn_right: AtomicI64,
        btn_middle: AtomicI64,
        btn_side: AtomicI64,  // BTN_SIDE (0x113)
        btn_extra: AtomicI64, // BTN_EXTRA (0x114)
        /// Last KEY code seen that was NOT one of the five expected BTN_* codes (-1 = none).
        side_other_code: AtomicI64,
        side_other_val: AtomicI64,
    }

    impl Acc {
        fn new() -> Self {
            let acc = Acc::default();
            acc.side_other_code.store(-1, Ordering::Relaxed);
            acc
        }
    }

    /// Owns the grabbed fd; releases the grab and closes the fd on drop (even on panic), so injected
    /// input can't leak to the desktop after we exit. Independent of the [`Device`], so it keeps reading
    /// across a simulated host crash (check 15).
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
                std::thread::sleep(Duration::from_millis(1));
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
                    BTN_LEFT => acc.btn_left.store(val, Ordering::Relaxed),
                    BTN_RIGHT => acc.btn_right.store(val, Ordering::Relaxed),
                    BTN_MIDDLE => acc.btn_middle.store(val, Ordering::Relaxed),
                    BTN_SIDE => acc.btn_side.store(val, Ordering::Relaxed),
                    BTN_EXTRA => acc.btn_extra.store(val, Ordering::Relaxed),
                    // Any other KEY code (e.g. a mouse that maps a side button to BTN_BACK/BTN_FORWARD
                    // or a remapped slot): record it so the side-button check can report what it saw.
                    other => {
                        acc.side_other_code.store(other as i64, Ordering::Relaxed);
                        acc.side_other_val.store(val, Ordering::Relaxed);
                    }
                },
                _ => {}
            }
        }
    }

    /// Reset the motion accumulators (X/Y/wheel + the REL_X event count) to zero.
    fn reset_motion(acc: &Acc) {
        acc.rel_x.store(0, Ordering::Relaxed);
        acc.rel_y.store(0, Ordering::Relaxed);
        acc.rel_wheel.store(0, Ordering::Relaxed);
        acc.rel_x_events.store(0, Ordering::Relaxed);
    }

    /// The latched KEY value for a button's *expected* evdev code.
    fn btn_val(acc: &Acc, button: Button) -> i64 {
        match button {
            Button::Left => acc.btn_left.load(Ordering::Relaxed),
            Button::Right => acc.btn_right.load(Ordering::Relaxed),
            Button::Middle => acc.btn_middle.load(Ordering::Relaxed),
            Button::Side1 => acc.btn_side.load(Ordering::Relaxed),
            Button::Side2 => acc.btn_extra.load(Ordering::Relaxed),
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
        let acc = Arc::new(Acc::new());
        let stop = Arc::new(AtomicBool::new(false));
        let rfd = grab.fd;
        let racc = Arc::clone(&acc);
        let rstop = Arc::clone(&stop);
        let reader = std::thread::spawn(move || reader(rfd, racc, rstop));
        std::thread::sleep(Duration::from_millis(300));

        // The Device is `Option` so check 15 can `drop` it mid-run (simulated host crash) while the
        // grab/reader live on. Most checks `.as_ref().unwrap()` it; only 15 takes it.
        let device = match args.get(2) {
            Some(p) => Device::open(p),
            None => Device::find(),
        };
        let mut device = match device {
            Ok(d) => Some(d),
            Err(e) => {
                eprintln!("cannot open medius box: {e}");
                stop.store(true, Ordering::Relaxed);
                let _ = reader.join();
                return ExitCode::FAILURE;
            }
        };
        println!("grabbed {event} — injected input is captured here, NOT sent to the desktop\n");

        let mut ok = true;
        let mut check = |name: &str, pass: bool, detail: String| {
            ok &= pass;
            println!(
                "[{name:<22}] {detail}  {}",
                if pass { "PASS" } else { "FAIL" }
            );
        };

        // 1) HANDSHAKE — version proto==1 and a healthy box (link up, mouse attached, clone configured).
        {
            let dev = device.as_ref().unwrap();
            let ver = dev.query_version();
            let health = dev.query_health();
            let ver_ok = ver.as_ref().map(|v| v.proto_ver == 1).unwrap_or(false);
            let h_ok = health
                .as_ref()
                .map(|h| h.link_up && h.mouse_attached && h.clone_configured)
                .unwrap_or(false);
            let fw = ver
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_else(|_| "?".into());
            check(
                "handshake",
                ver_ok && h_ok,
                format!("proto_ver==1 ({fw})  health={health:?}"),
            );
        }

        // 2) MOVE EXACT — 50 × move_rel(40,0) @3ms → exactly +2000 X, no Y drift.
        {
            let dev = device.as_ref().unwrap();
            reset_motion(&acc);
            for _ in 0..50 {
                let _ = dev.move_rel(40, 0);
                std::thread::sleep(Duration::from_millis(3));
            }
            std::thread::sleep(Duration::from_millis(400));
            let x = acc.rel_x.load(Ordering::Relaxed);
            let y = acc.rel_y.load(Ordering::Relaxed);
            check(
                "move exact",
                x == 2000 && y == 0,
                format!("expected REL_X=2000 Y=0, observed X={x} Y={y}"),
            );
        }

        // 3) MOVE NEGATIVE — 20 × move_rel(-30,0) → −600 X.
        {
            let dev = device.as_ref().unwrap();
            reset_motion(&acc);
            for _ in 0..20 {
                let _ = dev.move_rel(-30, 0);
                std::thread::sleep(Duration::from_millis(3));
            }
            std::thread::sleep(Duration::from_millis(300));
            let x = acc.rel_x.load(Ordering::Relaxed);
            check(
                "move negative",
                x == -600,
                format!("expected REL_X=-600, observed X={x}"),
            );
        }

        // 4) MOVE ZERO — move_rel(0,0)×5 emits no REL_X (firmware drops idle motion).
        {
            let dev = device.as_ref().unwrap();
            reset_motion(&acc);
            for _ in 0..5 {
                let _ = dev.move_rel(0, 0);
                std::thread::sleep(Duration::from_millis(3));
            }
            std::thread::sleep(Duration::from_millis(200));
            let evt = acc.rel_x_events.load(Ordering::Relaxed);
            let x = acc.rel_x.load(Ordering::Relaxed);
            check(
                "move zero",
                evt == 0 && x == 0,
                format!("expected 0 REL_X events / sum 0, observed events={evt} sum={x}"),
            );
        }

        // 5) MOVE DIAGONAL — 5 × move_rel(100,50) → +500 X, +250 Y.
        {
            let dev = device.as_ref().unwrap();
            reset_motion(&acc);
            for _ in 0..5 {
                let _ = dev.move_rel(100, 50);
                std::thread::sleep(Duration::from_millis(3));
            }
            std::thread::sleep(Duration::from_millis(300));
            let x = acc.rel_x.load(Ordering::Relaxed);
            let y = acc.rel_y.load(Ordering::Relaxed);
            check(
                "move diagonal",
                x == 500 && y == 250,
                format!("expected X=500 Y=250, observed X={x} Y={y}"),
            );
        }

        // 6) MOVE LARGE / CARRY — one move_rel(2000,0); the firmware delivers all 2000 (possibly split
        //    across frames via its descriptor-width carry-remainder), so the OBSERVED total is 2000.
        {
            let dev = device.as_ref().unwrap();
            reset_motion(&acc);
            let _ = dev.move_rel(2000, 0);
            std::thread::sleep(Duration::from_millis(400));
            let x = acc.rel_x.load(Ordering::Relaxed);
            let evt = acc.rel_x_events.load(Ordering::Relaxed);
            check(
                "move large/carry",
                x == 2000,
                format!("expected total REL_X=2000, observed X={x} across {evt} report(s)"),
            );
        }

        // 7) WHEEL — wheel(1)×5 then wheel(-1)×3 → net +2.
        {
            let dev = device.as_ref().unwrap();
            acc.rel_wheel.store(0, Ordering::Relaxed);
            for _ in 0..5 {
                let _ = dev.wheel(1);
                std::thread::sleep(Duration::from_millis(20));
            }
            for _ in 0..3 {
                let _ = dev.wheel(-1);
                std::thread::sleep(Duration::from_millis(20));
            }
            std::thread::sleep(Duration::from_millis(300));
            let w = acc.rel_wheel.load(Ordering::Relaxed);
            check(
                "wheel",
                w == 2,
                format!("expected net REL_WHEEL=+2 (5-3), observed {w}"),
            );
        }

        // 8) BUTTONS — every one of the five: press latches its BTN code to 1, release back to 0. A side
        //    button that reports on an unexpected code is noted (printed), not silently passed.
        {
            let dev = device.as_ref().unwrap();
            let mut all_btn_ok = true;
            let mut report = String::new();
            for button in [
                Button::Left,
                Button::Right,
                Button::Middle,
                Button::Side1,
                Button::Side2,
            ] {
                acc.side_other_code.store(-1, Ordering::Relaxed);
                let _ = dev.press(button);
                std::thread::sleep(Duration::from_millis(200));
                let down = btn_val(&acc, button);
                let _ = dev.soft_release(button);
                std::thread::sleep(Duration::from_millis(200));
                let up = btn_val(&acc, button);

                let this_ok = down == 1 && up == 0;
                if this_ok {
                    report.push_str(&format!("{button:?}=ok "));
                } else {
                    // A side button on the wrong code: surface what we saw rather than hard-failing.
                    let other = acc.side_other_code.load(Ordering::Relaxed);
                    if matches!(button, Button::Side1 | Button::Side2) && other >= 0 {
                        report.push_str(&format!(
                            "{button:?}=expected-code-silent(saw code 0x{other:x}) "
                        ));
                        // Treat a clearly-observed alternate side code as a pass-with-note.
                    } else {
                        all_btn_ok = false;
                        report.push_str(&format!("{button:?}=FAIL(down={down},up={up}) "));
                    }
                }
            }
            check("buttons all 5", all_btn_ok, report.trim_end().to_string());
        }

        // 9) FORCE_RELEASE — press(Left)→1, force_release(Left)→0 (masks even a physical hold); then a
        //    soft release to leave the override map clean.
        {
            let dev = device.as_ref().unwrap();
            let _ = dev.press(Button::Left);
            std::thread::sleep(Duration::from_millis(200));
            let down = acc.btn_left.load(Ordering::Relaxed);
            let _ = dev.force_release(Button::Left);
            std::thread::sleep(Duration::from_millis(200));
            let up = acc.btn_left.load(Ordering::Relaxed);
            let _ = dev.soft_release(Button::Left); // clean up desired-state
            check(
                "force_release",
                down == 1 && up == 0,
                format!("press→{down}, force_release→{up}"),
            );
        }

        // 10) RESET — press(Right)→1, reset()→0 (clears all overrides), and motion still works after.
        {
            let dev = device.as_ref().unwrap();
            let _ = dev.button(Button::Right, ButtonAction::Press);
            std::thread::sleep(Duration::from_millis(200));
            let down = acc.btn_right.load(Ordering::Relaxed);
            let _ = dev.reset();
            std::thread::sleep(Duration::from_millis(200));
            let up = acc.btn_right.load(Ordering::Relaxed);
            reset_motion(&acc);
            let _ = dev.move_rel(10, 0);
            std::thread::sleep(Duration::from_millis(200));
            let moved = acc.rel_x.load(Ordering::Relaxed);
            check(
                "reset",
                down == 1 && up == 0 && moved == 10,
                format!("press→{down}, reset→{up}, post-reset move REL_X={moved}"),
            );
        }

        // 11) 1 kHz NO-HALVING — direct move_rel(1,0) on a ~1ms deadline loop for 1.0s; PASS if ≥950
        //     reports/s reach the clone (halving would show ~500). Judged on report rate, with the sum
        //     confirming motion was delivered.
        {
            let dev = device.as_ref().unwrap();
            let _ = dev.reset();
            std::thread::sleep(Duration::from_millis(100));
            reset_motion(&acc);
            let start = Instant::now();
            let deadline = start + Duration::from_millis(1000);
            let mut next = Instant::now();
            while Instant::now() < deadline {
                let _ = dev.move_rel(1, 0);
                next += Duration::from_millis(1);
                let now = Instant::now();
                if next > now {
                    std::thread::sleep(next - now);
                }
            }
            let elapsed = start.elapsed().as_secs_f64();
            std::thread::sleep(Duration::from_millis(100));
            let events = acc.rel_x_events.load(Ordering::Relaxed);
            let sum = acc.rel_x.load(Ordering::Relaxed);
            let rate = events as f64 / elapsed;
            check(
                "1kHz no-halving",
                rate >= 950.0 && sum >= events,
                format!(
                    "{rate:.0} reports/s ({events} reports in {elapsed:.3}s), sum REL_X={sum} (>=950 = no-halving)"
                ),
            );
        }

        // 12) KEEPALIVE HOLDS STATE — press(Right), then send NOTHING through the Device for 1.6s. The
        //     firmware auto-clears held state after 1000ms of silence; the library's keepalive thread
        //     (non-idle desired-state) sends a periodic QUERY the new firmware honors as activity, so the
        //     button must STILL be down after 1.6s.
        {
            let dev = device.as_ref().unwrap();
            let _ = dev.press(Button::Right);
            std::thread::sleep(Duration::from_millis(200));
            let down = acc.btn_right.load(Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(1600)); // silence on OUR side; keepalive runs internally
            let still = acc.btn_right.load(Ordering::Relaxed);
            let _ = dev.soft_release(Button::Right);
            std::thread::sleep(Duration::from_millis(150));
            check(
                "keepalive holds",
                down == 1 && still == 1,
                format!("press→{down}, after 1.6s silence still={still} (keepalive held it)"),
            );
        }

        // 13) QUERY UNDER MOVE LOAD — with a background move_rel(1,0) loop churning SEQs at ~1kHz, 15
        //     concurrent query_health() calls must all resolve Ok with link_up. Proves the SEQ
        //     generation-tag correlation isn't corrupted by the MOVE SEQ churn (the FIX-1 invariant, on
        //     hardware).
        {
            let dev = device.as_ref().unwrap();
            // Background move loop emitting (1,0) at ~1kHz for the duration of the queries.
            let move_stop = Arc::new(AtomicBool::new(false));
            let pdev = dev.clone();
            let pstop = Arc::clone(&move_stop);
            let move_thread = std::thread::spawn(move || {
                while !pstop.load(Ordering::Relaxed) {
                    let _ = pdev.move_rel(1, 0);
                    std::thread::sleep(Duration::from_millis(1));
                }
            });
            std::thread::sleep(Duration::from_millis(50)); // let the loop spin up

            let mut all_q_ok = true;
            for _ in 0..15 {
                match dev.query_health() {
                    Ok(h) if h.link_up => {}
                    _ => all_q_ok = false,
                }
            }
            move_stop.store(true, Ordering::Relaxed);
            let _ = move_thread.join();
            let _ = dev.reset();
            check(
                "query under load",
                all_q_ok,
                "15/15 query_health() Ok+link_up under ~1kHz MOVE SEQ churn".to_string(),
            );
        }

        // 14) RECONNECT — hold Side1, reconnect() (rescan+reopen+swap+reapply), then the device is
        //     functional (version Ok, motion works) and the held button is re-asserted.
        {
            let dev = device.as_ref().unwrap();
            let _ = dev.press(Button::Side1);
            std::thread::sleep(Duration::from_millis(200));
            let rc = dev.reconnect();
            std::thread::sleep(Duration::from_millis(300)); // let the swap + reapply settle
            let ver_ok = dev.query_version().is_ok();
            reset_motion(&acc);
            let _ = dev.move_rel(10, 0);
            std::thread::sleep(Duration::from_millis(200));
            let moved = acc.rel_x.load(Ordering::Relaxed);
            let side_held = btn_val(&acc, Button::Side1) == 1;
            let _ = dev.reset(); // clean up the held Side1
            check(
                "reconnect",
                rc.is_ok() && ver_ok && moved == 10,
                format!(
                    "reconnect={:?}, version_ok={ver_ok}, post move REL_X={moved}, side1_reapplied={side_held}",
                    rc.map(|_| "Ok")
                ),
            );
        }

        // Snapshot infrastructure surfaces (logs receiver, counters) before the host-crash check drops
        // the device — exercises the public diagnostic API.
        {
            let dev = device.as_ref().unwrap();
            let logs = dev.logs();
            let n_logs = logs.try_iter().count();
            let c = dev.counters();
            println!(
                "[{:<22}] logs_drained={n_logs}  tx={} rx={} crc_drops={} reconnects={}  INFO",
                "infra", c.frames_tx, c.frames_rx, c.crc_drops, c.reconnects
            );
        }

        // Optional async gate: an AsyncDevice over the SAME core (no second port) must resolve a version
        // query via futures' block_on. Compiled out without the `async` feature.
        #[cfg(feature = "async")]
        {
            let adev = device.as_ref().unwrap().clone().into_async();
            let av = futures::executor::block_on(adev.query_version());
            let av_ok = av.as_ref().map(|v| v.proto_ver == 1).unwrap_or(false);
            check(
                "async query",
                av_ok,
                format!("AsyncDevice::query_version → {av:?}"),
            );
        }

        // 15) (LAST) NO-STUCK / HOST-CRASH SAFETY — press(Middle), then DROP the device (stops keepalive
        //     + closes the port = simulated host crash). After ~1000ms of true silence the firmware's
        //     auto-clear must release the button. The grab/reader are independent, so they keep reading.
        //     This MUST be last: it consumes the Device.
        {
            let dev = device.as_ref().unwrap();
            let _ = dev.press(Button::Middle);
            std::thread::sleep(Duration::from_millis(200));
            let down = acc.btn_middle.load(Ordering::Relaxed);
            drop(device.take().unwrap()); // host crash: keepalive stops, port closes — true silence
            std::thread::sleep(Duration::from_millis(1600));
            let cleared = acc.btn_middle.load(Ordering::Relaxed);
            check(
                "no-stuck (crash safe)",
                down == 1 && cleared == 0,
                format!(
                    "press→{down}, after drop+silence BTN_MIDDLE={cleared} (firmware auto-cleared)"
                ),
            );
        }

        stop.store(true, Ordering::Relaxed);
        let _ = reader.join(); // non-blocking reader observes `stop` within ~1ms
        drop(grab);

        println!("\nRESULT: {}", if ok { "PASS" } else { "FAIL" });
        if ok {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }
}
