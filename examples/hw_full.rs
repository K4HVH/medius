//! Comprehensive hardware validation (Linux only).

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

    use medius::{
        Action, Button, CatchMask, Device, Key, LedMode, LedTarget, LockDirection, LockTarget,
        MediaKey, RebootTarget,
    };

    const EVIOCGRAB: libc::c_ulong = 0x4004_4590;
    const EV_KEY: u16 = 0x01;
    const EV_REL: u16 = 0x02;
    const REL_X: u16 = 0x00;
    const REL_Y: u16 = 0x01;
    const REL_WHEEL: u16 = 0x08;
    const BTN_LEFT: u16 = 0x110;
    const BTN_RIGHT: u16 = 0x111;
    const BTN_MIDDLE: u16 = 0x112;
    const BTN_SIDE: u16 = 0x113;
    const BTN_EXTRA: u16 = 0x114;
    const EVENT_SIZE: usize = 24;

    #[derive(Default)]
    struct Acc {
        rel_x: AtomicI64,
        rel_y: AtomicI64,
        rel_wheel: AtomicI64,
        rel_x_events: AtomicI64,
        btn_left: AtomicI64,
        btn_right: AtomicI64,
        btn_middle: AtomicI64,
        btn_side: AtomicI64,
        btn_extra: AtomicI64,
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

    struct EvdevGrab {
        fd: RawFd,
    }

    impl EvdevGrab {
        fn open(path: &str) -> std::io::Result<Self> {
            let cpath = std::ffi::CString::new(path).unwrap();
            // SAFETY: valid C string and flags; O_NONBLOCK so the reader polls `stop` instead of blocking in read().
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

    fn reader(fd: RawFd, acc: Arc<Acc>, stop: Arc<AtomicBool>) {
        let mut buf = [0u8; EVENT_SIZE];
        while !stop.load(Ordering::Relaxed) {
            // SAFETY: fd is valid; we read into a buffer of exactly EVENT_SIZE bytes.
            let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, EVENT_SIZE) };
            if n != EVENT_SIZE as isize {
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
                    other => {
                        acc.side_other_code.store(other as i64, Ordering::Relaxed);
                        acc.side_other_val.store(val, Ordering::Relaxed);
                    }
                },
                _ => {}
            }
        }
    }

    fn reset_motion(acc: &Acc) {
        acc.rel_x.store(0, Ordering::Relaxed);
        acc.rel_y.store(0, Ordering::Relaxed);
        acc.rel_wheel.store(0, Ordering::Relaxed);
        acc.rel_x_events.store(0, Ordering::Relaxed);
    }

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
        let soak_secs: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(20);

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

        {
            let dev = device.as_ref().unwrap();
            let attached = dev
                .query_health()
                .map(|h| h.mouse_attached)
                .unwrap_or(false);
            let caps = dev.query_mouse_caps();
            let info = dev.query_mouse_info();
            let rate = dev.query_rate();
            let stats = dev.query_stats();

            let caps_ok = caps
                .as_ref()
                .map(|c| c.has_x && c.has_y && c.n_buttons > 0)
                .unwrap_or(false);
            // vid != 0 once a mouse is cloned; zero is allowed when none is attached.
            let info_ok = info
                .as_ref()
                .map(|i| if attached { i.vid != 0 } else { true })
                .unwrap_or(false);
            // native_hz lands in a sane band once learned; None (not yet learned) is allowed.
            let rate_ok = rate
                .as_ref()
                .map(|r| {
                    r.native_hz()
                        .is_none_or(|hz| (100.0..=8000.0).contains(&hz))
                })
                .unwrap_or(false);
            let stats_ok = stats.as_ref().map(|s| s.tx_drops == 0).unwrap_or(false);

            let hz = rate
                .as_ref()
                .ok()
                .and_then(|r| r.native_hz())
                .map(|hz| format!("{hz:.0}"))
                .unwrap_or_else(|| "?".into());
            let confident = rate.as_ref().map(|r| r.confident).unwrap_or(false);
            let (drops, wedges) = stats
                .as_ref()
                .map(|s| (s.tx_drops, s.tx_wedges))
                .unwrap_or((u16::MAX, u8::MAX));
            let id = info
                .as_ref()
                .map(|i| i.to_string())
                .unwrap_or_else(|_| "?".into());
            check(
                "device info",
                caps_ok && info_ok && rate_ok && stats_ok,
                format!(
                    "mouse={id} caps={caps:?}  rate={hz}Hz confident={confident}  tx_drops={drops} tx_wedges={wedges}"
                ),
            );
        }

        {
            // LED override is not visible on the clone, so this is a smoke check: every mode is
            // accepted, the box stays healthy, and the LED is handed back to its status display.
            let dev = device.as_ref().unwrap();
            let mut accepted = true;
            for (mode, level) in [
                (LedMode::Off, 0u8),
                (LedMode::Solid, 200),
                (LedMode::Blink, 200),
                (LedMode::Auto, 0),
            ] {
                accepted &= dev.led(LedTarget::Both, mode, level).is_ok();
                std::thread::sleep(Duration::from_millis(60));
            }
            let healthy = dev.query_health().map(|h| h.link_up).unwrap_or(false);
            check(
                "led override",
                accepted && healthy,
                format!("off/solid/blink/auto accepted={accepted}, healthy after={healthy}"),
            );
        }

        {
            // LOCK: a locked axis still moves under injection (the lock suppresses the physical
            // mouse only). The 3 ms inject cadence doubles as the keepalive that holds the lock.
            let dev = device.as_ref().unwrap();
            let _ = dev.reset();
            let _ = dev.lock(LockTarget::X, LockDirection::Both);
            reset_motion(&acc);
            for _ in 0..50 {
                let _ = dev.move_rel(40, 0);
                std::thread::sleep(Duration::from_millis(3));
            }
            std::thread::sleep(Duration::from_millis(400));
            let x = acc.rel_x.load(Ordering::Relaxed);
            check(
                "lock: inject passes",
                x == 2000,
                format!("X locked, injected +2000 still emitted X={x}"),
            );
            let _ = dev.reset();
        }

        {
            // LOCK: the LOCKS query reflects the set, is_locked() reads individual edges, and the
            // mask matches the wire layout (X+ = bit0, Left press = bit6 => 0x0041). LOCK_ON is set.
            let dev = device.as_ref().unwrap();
            let _ = dev.reset();
            let _ = dev.lock(LockTarget::X, LockDirection::Positive);
            let _ = dev.lock(LockTarget::Button(Button::Left), LockDirection::Positive);
            let locks = dev.query_locks();
            let lock_on = dev.query_health().map(|h| h.lock_on).unwrap_or(false);
            let mask = locks.as_ref().map(|l| l.mask()).unwrap_or(0);
            let q_ok = locks
                .as_ref()
                .map(|l| {
                    l.is_locked(LockTarget::X, LockDirection::Positive)
                        && !l.is_locked(LockTarget::X, LockDirection::Negative)
                        && l.is_locked(LockTarget::Button(Button::Left), LockDirection::Positive)
                        && l.mask() == 0x0041
                })
                .unwrap_or(false);
            check(
                "lock: query + health",
                q_ok && lock_on,
                format!("mask=0x{mask:04X} lock_on={lock_on}"),
            );
            let _ = dev.reset();
        }

        {
            // LOCK: injection overrides a hand-locked button (block-press, but a forced press wins).
            let dev = device.as_ref().unwrap();
            let _ = dev.reset();
            let _ = dev.lock(LockTarget::Button(Button::Left), LockDirection::Positive);
            let _ = dev.press(Button::Left);
            std::thread::sleep(Duration::from_millis(200));
            let down = btn_val(&acc, Button::Left);
            check(
                "lock: inject overrides",
                down == 1,
                format!("Left press-locked, injected press -> BTN_LEFT={down}"),
            );
            let _ = dev.reset();
            std::thread::sleep(Duration::from_millis(150));
        }

        {
            // LOCK safety: RESET clears every lock, and a lock-only state self-clears after ~1 s of
            // control-PC silence so a locked mouse is never stranded.
            let dev = device.as_ref().unwrap();
            let _ = dev.lock(LockTarget::Y, LockDirection::Both);
            let _ = dev.reset();
            let after_reset = dev.query_locks().map(|l| l.mask()).unwrap_or(0xFFFF);

            let _ = dev.lock(LockTarget::Y, LockDirection::Both);
            let before = dev.query_locks().map(|l| l.mask()).unwrap_or(0);
            std::thread::sleep(Duration::from_millis(1400)); // silent: no frames sent
            let after_silence = dev.query_locks().map(|l| l.mask()).unwrap_or(0xFFFF);
            check(
                "lock: safety clear",
                after_reset == 0 && before == 0x000C && after_silence == 0,
                format!(
                    "reset->0x{after_reset:04X}; y-lock 0x{before:04X} after 1.4s silence 0x{after_silence:04X}"
                ),
            );
        }

        {
            // CATCH: subscribe and confirm the box reports it (CATCH_ON + the mask via query_catch),
            // no events while the mouse is idle, and that a RESET clears catch like injection AND
            // disconnects the host stream (recv -> Err, not a silent hang). Live physical-input delivery
            // needs a hand on the mouse — watch it with `medius.py watch`.
            let dev = device.as_ref().unwrap();
            let stream = dev.catch_events(CatchMask::all());
            std::thread::sleep(Duration::from_millis(100));
            let on = dev.query_health().map(|h| h.catch_on).unwrap_or(false);
            let mask = dev
                .query_catch()
                .map(|c| c.mask)
                .unwrap_or(CatchMask::empty());
            let idle_quiet = stream
                .as_ref()
                .map(|s| s.try_recv().is_none())
                .unwrap_or(false);
            let _ = dev.reset(); // clears catch like injection + disconnects the host stream
            std::thread::sleep(Duration::from_millis(100));
            let off = dev.query_health().map(|h| !h.catch_on).unwrap_or(false);
            let cleared = dev
                .query_catch()
                .map(|c| c.mask == CatchMask::empty())
                .unwrap_or(false);
            let stream_ended = stream.as_ref().map(|s| s.recv().is_err()).unwrap_or(false);
            check(
                "catch: subscribe + reset",
                on && mask == CatchMask::all() && idle_quiet && off && cleared && stream_ended,
                format!(
                    "CATCH_ON={on} mask={mask:?} idle_quiet={idle_quiet}; reset->off={off} cleared={cleared} stream_ended={stream_ended}"
                ),
            );
        }

        {
            // KEYBOARD + MEDIA (v1.7.0): query KBD_CAPS; if a keyboard is bound, inject a key (and a
            // media key when the board has a Consumer collection) and confirm injection_active toggles.
            // Real keystroke/media delivery needs a keyboard on the box + a grabbed evdev — watch it
            // with `medius.py watch keys`. With a mouse-only clone, this just confirms KBD_CAPS replies.
            let dev = device.as_ref().unwrap();
            let caps = dev.query_kbd_caps();
            let attached = dev.query_health().map(|h| h.kbd_attached).unwrap_or(false);
            let mut inject_ok = true;
            let mut detail = format!("kbd_caps={caps:?} attached={attached}");
            if attached {
                let _ = dev.key_down(Key::A);
                let key_on = dev
                    .query_health()
                    .map(|h| h.injection_active)
                    .unwrap_or(false);
                let _ = dev.key_up(Key::A);
                let _ = dev.reset();
                let key_off = dev
                    .query_health()
                    .map(|h| !h.injection_active)
                    .unwrap_or(false);
                inject_ok = key_on && key_off;
                detail = format!("{detail} key[on={key_on} off={key_off}]");
                if caps.as_ref().map(|c| c.has_consumer).unwrap_or(false) {
                    let _ = dev.media_down(MediaKey::VOLUME_UP);
                    let med_on = dev
                        .query_health()
                        .map(|h| h.injection_active)
                        .unwrap_or(false);
                    let _ = dev.media_up(MediaKey::VOLUME_UP);
                    let _ = dev.reset();
                    inject_ok = inject_ok && med_on;
                    detail = format!("{detail} media[on={med_on}]");
                }
            }
            check("keyboard + media", caps.is_ok() && inject_ok, detail);
        }

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
                    let other = acc.side_other_code.load(Ordering::Relaxed);
                    if matches!(button, Button::Side1 | Button::Side2) && other >= 0 {
                        report.push_str(&format!(
                            "{button:?}=expected-code-silent(saw code 0x{other:x}) "
                        ));
                    } else {
                        all_btn_ok = false;
                        report.push_str(&format!("{button:?}=FAIL(down={down},up={up}) "));
                    }
                }
            }
            check("buttons all 5", all_btn_ok, report.trim_end().to_string());
        }

        {
            let dev = device.as_ref().unwrap();
            let _ = dev.press(Button::Left);
            std::thread::sleep(Duration::from_millis(200));
            let down = acc.btn_left.load(Ordering::Relaxed);
            let _ = dev.force_release(Button::Left);
            std::thread::sleep(Duration::from_millis(200));
            let up = acc.btn_left.load(Ordering::Relaxed);
            let _ = dev.soft_release(Button::Left);
            check(
                "force_release",
                down == 1 && up == 0,
                format!("press→{down}, force_release→{up}"),
            );
        }

        {
            let dev = device.as_ref().unwrap();
            let _ = dev.button(Button::Right, Action::Press);
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

        if soak_secs > 0 {
            let dev = device.as_ref().unwrap();
            let _ = dev.reset();
            std::thread::sleep(Duration::from_millis(100));
            reset_motion(&acc);
            println!(
                "[{:<22}] soaking the 1 kHz loop for {soak_secs}s ...",
                "soak"
            );
            let start = Instant::now();
            let deadline = start + Duration::from_secs(soak_secs);
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
                "soak",
                rate >= 950.0 && sum >= events,
                format!(
                    "{rate:.0} reports/s sustained over {elapsed:.1}s ({events} reports, sum REL_X={sum})"
                ),
            );
        }

        {
            let dev = device.as_ref().unwrap();
            let _ = dev.press(Button::Right);
            std::thread::sleep(Duration::from_millis(200));
            let down = acc.btn_right.load(Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(1600));
            let still = acc.btn_right.load(Ordering::Relaxed);
            let _ = dev.soft_release(Button::Right);
            std::thread::sleep(Duration::from_millis(150));
            check(
                "keepalive holds",
                down == 1 && still == 1,
                format!("press→{down}, after 1.6s silence still={still} (keepalive held it)"),
            );
        }

        {
            let dev = device.as_ref().unwrap();
            let move_stop = Arc::new(AtomicBool::new(false));
            let pdev = dev.clone();
            let pstop = Arc::clone(&move_stop);
            let move_thread = std::thread::spawn(move || {
                while !pstop.load(Ordering::Relaxed) {
                    let _ = pdev.move_rel(1, 0);
                    std::thread::sleep(Duration::from_millis(1));
                }
            });
            std::thread::sleep(Duration::from_millis(50));

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

        {
            let dev = device.as_ref().unwrap();
            let _ = dev.press(Button::Side1);
            std::thread::sleep(Duration::from_millis(200));
            let rc = dev.reconnect();
            std::thread::sleep(Duration::from_millis(300));
            let ver_ok = dev.query_version().is_ok();
            reset_motion(&acc);
            let _ = dev.move_rel(10, 0);
            std::thread::sleep(Duration::from_millis(200));
            let moved = acc.rel_x.load(Ordering::Relaxed);
            let side_held = btn_val(&acc, Button::Side1) == 1;
            let _ = dev.reset();
            check(
                "reconnect",
                rc.is_ok() && ver_ok && moved == 10,
                format!(
                    "reconnect={:?}, version_ok={ver_ok}, post move REL_X={moved}, side1_reapplied={side_held}",
                    rc.map(|_| "Ok")
                ),
            );
        }

        {
            let dev = device.as_ref().unwrap();
            let _ = dev.reboot(RebootTarget::HostRun);
            std::thread::sleep(Duration::from_secs(2));
            let mut recovered = matches!(dev.query_version(), Ok(v) if v.proto_ver == 1);
            for _ in 0..10 {
                if recovered {
                    break;
                }
                let _ = dev.reconnect();
                std::thread::sleep(Duration::from_millis(500));
                recovered = matches!(dev.query_version(), Ok(v) if v.proto_ver == 1);
            }
            reset_motion(&acc);
            let _ = dev.move_rel(10, 0);
            std::thread::sleep(Duration::from_millis(200));
            let moved = acc.rel_x.load(Ordering::Relaxed);
            let _ = dev.reset();
            check(
                "reboot-to-run",
                recovered && moved == 10,
                format!("reboot(HostRun) → responsive={recovered}, post-reboot move REL_X={moved}"),
            );
        }

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

        #[cfg(feature = "async")]
        {
            use futures::executor::block_on;
            let adev = device.as_ref().unwrap().clone().into_async();
            let av_ok = block_on(adev.query_version())
                .map(|v| v.proto_ver == 1)
                .unwrap_or(false);
            let ah_ok = block_on(adev.query_health())
                .map(|h| h.link_up)
                .unwrap_or(false);
            reset_motion(&acc);
            let _ = adev.move_rel(12, 0);
            std::thread::sleep(Duration::from_millis(200));
            let amoved = acc.rel_x.load(Ordering::Relaxed);
            let _ = adev.reset();
            check(
                "async",
                av_ok && ah_ok && amoved == 12,
                format!(
                    "AsyncDevice: version_ok={av_ok}, health_ok={ah_ok}, async move REL_X={amoved}"
                ),
            );
        }

        {
            let dev = device.as_ref().unwrap();
            let _ = dev.press(Button::Middle);
            std::thread::sleep(Duration::from_millis(200));
            let down = acc.btn_middle.load(Ordering::Relaxed);
            drop(device.take().unwrap());
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
        let _ = reader.join();
        drop(grab);

        if std::env::var_os("MEDIUS_UNPLUG_TEST").is_some() {
            let reopened = match args.get(2) {
                Some(p) => Device::open(p),
                None => Device::find(),
            };
            match reopened {
                Ok(dev) => {
                    let base = dev.counters().reconnects;
                    let up0 = matches!(dev.query_version(), Ok(v) if v.proto_ver == 1);
                    println!(
                        "\n>>> AUTO-RECONNECT: physically UNPLUG the box's control USB, wait ~2s, then \
                         replug.\n    Waiting up to 60s for the reader to self-heal — NO reconnect() is \
                         called by this test."
                    );
                    let deadline = Instant::now() + Duration::from_secs(60);
                    let mut healed = false;
                    while Instant::now() < deadline {
                        std::thread::sleep(Duration::from_millis(500));
                        if dev.counters().reconnects > base
                            && matches!(dev.query_version(), Ok(v) if v.proto_ver == 1)
                        {
                            healed = true;
                            break;
                        }
                    }
                    let now = dev.counters().reconnects;
                    check(
                        "auto-reconnect",
                        up0 && healed,
                        format!(
                            "unattended self-heal after unplug: reconnects {base}→{now}, version recovered={healed}"
                        ),
                    );
                }
                Err(e) => check("auto-reconnect", false, format!("reopen failed: {e}")),
            }
        }

        println!("\nRESULT: {}", if ok { "PASS" } else { "FAIL" });
        if ok {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }
}
