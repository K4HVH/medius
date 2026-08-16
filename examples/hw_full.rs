//! Hardware validation (Linux only).

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
        Action, Axis, Blanket, Button, CatchClass, CatchFilter, ClipAction, ClipBuilder, ClipState,
        ClipTrigger, Device, Edge, EmitPace, Key, LedMode, LedTarget, LockDirection, MediaKey,
        RebootTarget,
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
    const KEY_A: u16 = 30; // evdev keycode for the 'A' key (HID usage 0x04)
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
        key_a: AtomicI64,
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
                    KEY_A => acc.key_a.store(val, Ordering::Relaxed),
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
        // args[1]: one or more comma-separated evdev nodes. A cloned mouse is composite (mouse and keyboard
        // interfaces on separate event nodes), so grab BOTH by default; injected input on an ungrabbed node
        // would otherwise leak to the desktop and escape verification here.
        let events: Vec<String> = match args.get(1) {
            Some(s) => s
                .split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect(),
            None => vec![
                "/dev/input/event11".to_string(),
                "/dev/input/event12".to_string(),
            ],
        };
        let soak_secs: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(20);

        let mut grabs = Vec::new();
        for ev in &events {
            match EvdevGrab::open(ev) {
                Ok(g) => grabs.push(g),
                Err(e) => {
                    eprintln!("cannot grab {ev}: {e} (try a different event node, or run as root)");
                    return ExitCode::FAILURE;
                }
            }
        }
        let acc = Arc::new(Acc::new());
        let stop = Arc::new(AtomicBool::new(false));
        // One reader per grabbed node, all feeding the same accumulator (mouse node reports REL/buttons,
        // keyboard node reports KEY_*), so a composite clone is verified in full.
        let readers: Vec<_> = grabs
            .iter()
            .map(|g| {
                let rfd = g.fd;
                let racc = Arc::clone(&acc);
                let rstop = Arc::clone(&stop);
                std::thread::spawn(move || reader(rfd, racc, rstop))
            })
            .collect();
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
                for r in readers {
                    let _ = r.join();
                }
                return ExitCode::FAILURE;
            }
        };
        println!(
            "grabbed {}: injected input is captured here, NOT sent to the desktop\n",
            events.join(", ")
        );

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
            let ver_ok = ver.as_ref().map(|v| v.proto_ver == 3).unwrap_or(false);
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
                format!("proto_ver==3 ({fw})  health={health:?}"),
            );
        }

        {
            let dev = device.as_ref().unwrap();
            let attached = dev
                .query_health()
                .map(|h| h.mouse_attached)
                .unwrap_or(false);
            let caps = dev.caps();
            let info = dev.device_info();
            let rate = dev.query_rate();
            let stats = dev.query_stats();

            let caps_ok = caps
                .as_ref()
                .map(|c| c.mouse.has_x && c.mouse.has_y && c.mouse.n_buttons > 0)
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
            // IMPERFECT: a normal mouse fits the box's endpoints, so it's never over-capacity and the
            // live clone is faithful. The opt-in toggle is informational here (just printed).
            let dev = device.as_ref().unwrap();
            let imp = dev.query_imperfect();
            let faithful = imp
                .as_ref()
                .map(|i| !i.over_capacity && !i.clone_imperfect)
                .unwrap_or(false);
            let allowed = imp.as_ref().map(|i| i.allowed).unwrap_or(false);
            check(
                "imperfect clone",
                imp.is_ok() && faithful,
                format!("allowed={allowed} status={imp:?}"),
            );
        }

        {
            // Wire round-trip + NVS-persistence check for the MOVE_RIDE option; the riding behaviour
            // itself needs the rig. Leaves the box back at the default (off).
            let dev = device.as_ref().unwrap();
            let want = Duration::from_millis(5);
            let set_ok = dev.set_movement_riding(Some(want)).is_ok();
            std::thread::sleep(Duration::from_millis(60));
            let read = dev.query_movement_riding();
            let matched = read.as_ref().map(|w| *w == Some(want)).unwrap_or(false);
            let off_ok = dev.set_movement_riding(None).is_ok();
            std::thread::sleep(Duration::from_millis(60));
            let read_off = dev.query_movement_riding();
            let off_matched = read_off.as_ref().map(|w| w.is_none()).unwrap_or(false);
            check(
                "movement riding",
                set_ok && matched && off_ok && off_matched,
                format!("set 5ms -> {read:?}, off -> {read_off:?}"),
            );
        }

        {
            // Wire round-trip + NVS-persistence check for the EMIT option; the pacing behaviour itself
            // needs the rig. Restores LEARNED (the default) afterward.
            let dev = device.as_ref().unwrap();
            let set_ok = dev.set_emit_pace(EmitPace::Fixed(500)).is_ok();
            std::thread::sleep(Duration::from_millis(60));
            let read = dev.query_emit_pace();
            let matched = read
                .as_ref()
                .map(|s| s.mode == EmitPace::Fixed(500) && s.resolved_hz == 500)
                .unwrap_or(false);
            let off_ok = dev.set_emit_pace(EmitPace::Learned).is_ok();
            std::thread::sleep(Duration::from_millis(60));
            let read_off = dev.query_emit_pace();
            let off_matched = read_off
                .as_ref()
                .map(|s| s.mode == EmitPace::Learned)
                .unwrap_or(false);
            check(
                "emit pace",
                set_ok && matched && off_ok && off_matched,
                format!("set Fixed(500) -> {read:?}, off -> {read_off:?}"),
            );
        }

        {
            // The name rides RESP(VERSION) like the MAC; clearing reverts to the synthesized
            // "Medius-XXXX" default.
            let dev = device.as_ref().unwrap();
            let set_ok = dev.set_name("hw-full box").is_ok();
            std::thread::sleep(Duration::from_millis(250)); // the name is a persisted OPTION write
            let named = matches!(dev.query_version(), Ok(v) if v.name == "hw-full box");
            let clear_ok = dev.clear_name().is_ok();
            std::thread::sleep(Duration::from_millis(250));
            let after = dev.query_version().map(|v| v.name).unwrap_or_default();
            let reverted = after.starts_with("Medius-");
            check(
                "box name",
                set_ok && named && clear_ok && reverted,
                format!("set 'hw-full box' -> read back, clear -> {after:?}"),
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
            let _ = dev.lock(Axis::X, LockDirection::Both);
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
            let _ = dev.lock(Axis::X, LockDirection::Positive);
            let _ = dev.lock(Button::Left, LockDirection::Positive);
            let locks = dev.query_locks();
            let lock_on = dev.query_health().map(|h| h.lock_on).unwrap_or(false);
            let n = locks.as_ref().map(|l| l.entries().len()).unwrap_or(0);
            let q_ok = locks
                .as_ref()
                .map(|l| {
                    l.is_locked(Axis::X, LockDirection::Positive)
                        && !l.is_locked(Axis::X, LockDirection::Negative)
                        && l.is_locked(Button::Left, LockDirection::Positive)
                        && l.entries().len() == 2
                })
                .unwrap_or(false);
            check(
                "lock: query + health",
                q_ok && lock_on,
                format!("{n} locks q_ok={q_ok} lock_on={lock_on}"),
            );
            let _ = dev.reset();
        }

        {
            // LOCK: injection overrides a hand-locked button (block-press, but a forced press wins).
            let dev = device.as_ref().unwrap();
            let _ = dev.reset();
            let _ = dev.lock(Button::Left, LockDirection::Positive);
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
            // LOCK safety: RESET clears every lock; the keepalive holds a lock alive while the client
            // runs, and the firmware self-clears only on true control-PC silence (a crash stops it).
            let dev = device.as_ref().unwrap();
            let _ = dev.lock(Axis::Y, LockDirection::Both);
            let _ = dev.reset();
            let after_reset = dev.query_locks().map(|l| l.entries().len()).unwrap_or(99);

            let _ = dev.lock(Axis::Y, LockDirection::Both);
            let before = dev.query_locks().map(|l| l.entries().len()).unwrap_or(0);
            std::thread::sleep(Duration::from_millis(1400)); // longer than the box silence window
            let after_hold = dev.query_locks().map(|l| l.entries().len()).unwrap_or(99);
            let _ = dev.reset();
            check(
                "lock: reset + keepalive holds",
                after_reset == 0 && before == 1 && after_hold == 1,
                format!(
                    "reset->{after_reset} locks; y-lock {before}, held across 1.4s {after_hold}"
                ),
            );
        }

        {
            // LOCK (keyboard/blanket): key-lock and all-keys blanket both register on HEALTH lock_on and
            // RESET clears them; the physical block needs a hand on the keyboard (run `medius.py`).
            let dev = device.as_ref().unwrap();
            let has_kbd = dev.query_health().map(|h| h.kbd_attached).unwrap_or(false);
            if has_kbd {
                let _ = dev.lock(Key::A, LockDirection::Both);
                let on1 = dev.query_health().map(|h| h.lock_on).unwrap_or(false);
                let _ = dev.unlock(Key::A, LockDirection::Both);
                let _ = dev.lock_all(Blanket::Keys, LockDirection::Both);
                let on2 = dev.query_health().map(|h| h.lock_on).unwrap_or(false);
                let _ = dev.reset();
                let off = dev.query_health().map(|h| !h.lock_on).unwrap_or(false);
                check(
                    "lock: keyboard + blanket",
                    on1 && on2 && off,
                    format!("key-lock lock_on={on1}, all-keys lock_on={on2}, reset-cleared={off}"),
                );
            } else {
                check(
                    "lock: keyboard + blanket",
                    true,
                    "skipped (no keyboard attached)".into(),
                );
            }
        }

        {
            // CATCH: subscribe, confirm CATCH_ON + mask via query_catch, idle stays quiet, and RESET
            // clears catch AND disconnects the host stream (recv -> Err). Live delivery needs a hand.
            let dev = device.as_ref().unwrap();
            let stream = dev.catch_events([CatchFilter::all()]);
            std::thread::sleep(Duration::from_millis(100));
            let on = dev.query_health().map(|h| h.catch_on).unwrap_or(false);
            let entries = dev.query_catch().map(|c| c.entries.len()).unwrap_or(0);
            let idle_quiet = stream
                .as_ref()
                .map(|s| s.try_recv().is_none())
                .unwrap_or(false);
            let _ = dev.reset();
            std::thread::sleep(Duration::from_millis(100));
            let off = dev.query_health().map(|h| !h.catch_on).unwrap_or(false);
            let cleared = dev.query_catch().map(|c| c.is_empty()).unwrap_or(false);
            let stream_ended = stream.as_ref().map(|s| s.recv().is_err()).unwrap_or(false);
            check(
                "catch: subscribe + reset",
                on && entries == 1 && idle_quiet && off && cleared && stream_ended,
                format!(
                    "CATCH_ON={on} entries={entries} idle_quiet={idle_quiet}; reset->off={off} cleared={cleared} stream_ended={stream_ended}"
                ),
            );
        }

        {
            // Catch timestamps: drain a short window and confirm every stamp advances by a plausible
            // amount. Passes vacuously when the mouse is still, since there is nothing to check then;
            // the event count in the message says whether it actually got exercised.
            let dev = device.as_ref().unwrap();
            let mut stamps: Vec<u32> = Vec::new();
            if let Ok(stream) = dev.catch_events([
                CatchFilter::class(CatchClass::Axis),
                CatchFilter::class(CatchClass::Button),
            ]) {
                let deadline = std::time::Instant::now() + Duration::from_secs(2);
                while std::time::Instant::now() < deadline {
                    // One domain only: these two classes are always host-chip stamped, so the
                    // monotonicity check below is comparing like with like.
                    if let Some(ev) = stream.recv_timeout(Duration::from_millis(100)) {
                        stamps.push(ev.ts_us());
                    }
                }
            }
            let sane = stamps
                .windows(2)
                .all(|w| w[1] > w[0] && w[1] - w[0] < 1_000_000);
            let _ = dev.reset();
            check(
                "catch: event timestamps advance",
                sane,
                format!(
                    "{} events; {}",
                    stamps.len(),
                    if stamps.len() < 2 {
                        "idle, nothing to compare (move the mouse to exercise this)".to_string()
                    } else {
                        format!(
                            "span {} us",
                            stamps.last().unwrap() - stamps.first().unwrap()
                        )
                    }
                ),
            );
        }

        {
            // The traffic classes: HID_IN is host-stamped (the real device produced it) and EMIT is
            // device-stamped (the clone produced it), and the two must agree on the report count.
            // A class tagged with the wrong clock domain yields plausible wrong deltas rather than an
            // error, so the domain is asserted rather than eyeballed.
            let dev = device.as_ref().unwrap();
            let mut hid_in = 0usize;
            let mut emit = 0usize;
            let mut domains_right = true;
            if let Ok(stream) = dev.catch_events([
                CatchFilter::class(CatchClass::HidIn),
                CatchFilter::class(CatchClass::Emit),
            ]) {
                let deadline = std::time::Instant::now() + Duration::from_secs(2);
                while std::time::Instant::now() < deadline {
                    if let Some(medius::CatchEvent::Traffic(t)) =
                        stream.recv_timeout(Duration::from_millis(100))
                    {
                        match t.class {
                            CatchClass::HidIn => {
                                hid_in += 1;
                                domains_right &= t.clock == medius::ClockDomain::HostChip;
                            }
                            CatchClass::Emit => {
                                emit += 1;
                                domains_right &= t.clock == medius::ClockDomain::DeviceChip;
                            }
                            _ => domains_right = false,
                        }
                    }
                }
            }
            let _ = dev.reset();
            // The two streams track the same reports, so they differ by at most whatever was in
            // flight when the window closed.
            let paired = hid_in > 0 && emit > 0 && hid_in.abs_diff(emit) <= 2;
            check(
                "catch: traffic classes",
                paired && domains_right,
                format!("hid_in={hid_in} emit={emit} clock_domains_correct={domains_right}"),
            );
        }

        {
            // The measured inter-chip clock estimate. Both chips must be running current firmware for
            // this to converge; an absent estimate reads as age=None rather than a zero offset.
            let dev = device.as_ref().unwrap();
            let st = dev.catch_events([CatchFilter::all()]).ok().and_then(|_s| {
                std::thread::sleep(Duration::from_millis(300));
                dev.query_catch().ok()
            });
            let (converged, detail) = match st {
                Some(c) => match c.clock.age {
                    Some(age) => (
                        c.clock.delay_us > 0 && c.clock.delay_us < 5_000,
                        format!(
                            "offset={}us rate={} delay={}us (+/-{}us) age={}ms",
                            c.clock.offset_us,
                            c.clock
                                .rate_ppb
                                .map_or("unfitted".into(), |r| format!("{r}ppb")),
                            c.clock.delay_us,
                            c.clock.error_bound_us(),
                            age.as_millis()
                        ),
                    ),
                    None => (
                        false,
                        "no estimate: is the host chip on current firmware?".into(),
                    ),
                },
                None => (false, "query failed".into()),
            };
            let _ = dev.reset();
            check("catch: inter-chip clock", converged, detail);
        }

        {
            // KEYBOARD + MEDIA: verify an injected KEY_A really reaches the grabbed evdev (key_a 1 then 0),
            // which catches a key landing on the wrong interface; the grabbed node must be the keyboard's.
            let dev = device.as_ref().unwrap();
            let caps = dev.caps().map(|c| c.keyboard);
            let attached = dev.query_health().map(|h| h.kbd_attached).unwrap_or(false);
            let mut inject_ok = true;
            let mut detail = format!("kbd_caps={caps:?} attached={attached}");
            if attached {
                acc.key_a.store(0, Ordering::Relaxed);
                let _ = dev.press(Key::A);
                let key_on = dev
                    .query_health()
                    .map(|h| h.injection_active)
                    .unwrap_or(false);
                std::thread::sleep(Duration::from_millis(200));
                let evdev_down = acc.key_a.load(Ordering::Relaxed) == 1;
                let _ = dev.release(Key::A);
                std::thread::sleep(Duration::from_millis(200));
                let evdev_up = acc.key_a.load(Ordering::Relaxed) == 0;
                let _ = dev.reset();
                let key_off = dev
                    .query_health()
                    .map(|h| !h.injection_active)
                    .unwrap_or(false);
                inject_ok = key_on && key_off && evdev_down && evdev_up;
                detail = format!(
                    "{detail} key[on={key_on} off={key_off} evdev_down={evdev_down} evdev_up={evdev_up}]"
                );
                if caps.as_ref().map(|c| c.has_consumer).unwrap_or(false) {
                    let _ = dev.press(MediaKey::VOLUME_UP);
                    let med_on = dev
                        .query_health()
                        .map(|h| h.injection_active)
                        .unwrap_or(false);
                    let _ = dev.release(MediaKey::VOLUME_UP);
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
            // Buffered clip playback: a clip of 200 mouse moves plus a KEY_A hold drives both the mouse and
            // keyboard interfaces; the grabbed node reports whichever it is. Also covers auto-lock and counters.
            let dev = device.as_ref().unwrap();
            let _ = dev.reset();
            reset_motion(&acc);
            acc.key_a.store(0, Ordering::Relaxed);
            let clip = dev.clip();
            let mut b = ClipBuilder::new();
            // Hold KEY_A across the whole motion run so the held-usage snapshot is sampled mid-hold.
            b.press(Key::A);
            for _ in 0..200 {
                b.move_by(10, 0);
            }
            b.release(Key::A);
            let appended = clip.append(&b).is_ok();
            let loaded = clip.query_status().map(|s| s.total > 0).unwrap_or(false);
            // Selective auto-lock: only the aim axes and buttons, leaving the keyboard free (the clip still
            // drives KEY_A; only physical input is scoped-locked).
            let scoped = clip.set_autolock(&[Blanket::Aim, Blanket::Buttons]).is_ok();
            let started = scoped && clip.start().is_ok();
            std::thread::sleep(Duration::from_millis(150));
            let key_down = acc.key_a.load(Ordering::Relaxed) == 1; // set if the grabbed node is the keyboard
            // While the clip still holds KEY_A (before its release entry), status reports it as held.
            let keys_held = clip
                .query_status()
                .map(|s| s.is_held(medius::Key::A))
                .unwrap_or(false);
            std::thread::sleep(Duration::from_millis(500));
            let x = acc.rel_x.load(Ordering::Relaxed); // set if the grabbed node is the mouse
            let played = clip.query_status().map(|s| s.ticks >= 200).unwrap_or(false);
            let stopped = clip.stop().is_ok();
            std::thread::sleep(Duration::from_millis(60));
            let idle = matches!(clip.query_status(), Ok(s) if s.state == ClipState::Idle);
            let drove_grabbed = x == 2000 || key_down;
            check(
                "clip playback (field-generic)",
                appended
                    && loaded
                    && started
                    && drove_grabbed
                    && keys_held
                    && played
                    && stopped
                    && idle,
                format!(
                    "appended={appended} loaded={loaded} started={started} REL_X={x} key_down={key_down} keys_held={keys_held} played={played} stopped={stopped} idle={idle}"
                ),
            );

            // Trigger set + config readback (the actual firing needs a physical press, exercised in the
            // physical-hand suite): bind two bindings, read them back, unbind, and clear.
            let _ = clip.clear();
            let bound_key = clip
                .bind(ClipTrigger::new(Key::A, Edge::Press, ClipAction::Start))
                .is_ok();
            let bound_btn = clip
                .bind(ClipTrigger::new(Button::Side1, Edge::Release, ClipAction::Stop).consume())
                .is_ok();
            let loop_set = clip.set_loop(true).is_ok();
            let cfg_ok = clip
                .query_config()
                .map(|c| {
                    c.loop_
                        && c.triggers.len() == 2
                        && c.triggers.iter().any(|t| {
                            t.on == Key::A.into()
                                && t.edge == Edge::Press
                                && t.action == ClipAction::Start
                        })
                        && c.triggers
                            .iter()
                            .any(|t| t.action == ClipAction::Stop && t.consume)
                })
                .unwrap_or(false);
            let unbound = clip.unbind(Key::A, Edge::Press).is_ok();
            let after_unbind = clip
                .query_config()
                .map(|c| c.triggers.len() == 1)
                .unwrap_or(false);
            let cleared = clip.clear_triggers().is_ok();
            let no_triggers = clip
                .query_config()
                .map(|c| c.triggers.is_empty())
                .unwrap_or(false);
            let _ = clip.set_loop(false);
            check(
                "clip trigger set + config readback",
                bound_key
                    && bound_btn
                    && loop_set
                    && cfg_ok
                    && unbound
                    && after_unbind
                    && cleared
                    && no_triggers,
                format!(
                    "bound_key={bound_key} bound_btn={bound_btn} loop_set={loop_set} cfg_ok={cfg_ok} unbound={unbound} after_unbind={after_unbind} cleared={cleared} no_triggers={no_triggers}"
                ),
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
                let _ = dev.release(button);
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
            let _ = dev.release(Button::Left);
            check(
                "force_release",
                down == 1 && up == 0,
                format!("press→{down}, force_release→{up}"),
            );
        }

        {
            let dev = device.as_ref().unwrap();
            let _ = dev.inject(Button::Right, Action::Press);
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
            let _ = dev.release(Button::Right);
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
            let mut recovered = matches!(dev.query_version(), Ok(v) if v.proto_ver == 3);
            for _ in 0..10 {
                if recovered {
                    break;
                }
                let _ = dev.reconnect();
                std::thread::sleep(Duration::from_millis(500));
                recovered = matches!(dev.query_version(), Ok(v) if v.proto_ver == 3);
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
                .map(|v| v.proto_ver == 3)
                .unwrap_or(false);
            let ah_ok = block_on(adev.query_health())
                .map(|h| h.link_up)
                .unwrap_or(false);
            // exercise the async option-query paths against real hardware (the sync ones run above)
            let aopt_ok = block_on(adev.query_movement_riding()).is_ok()
                && block_on(adev.query_imperfect()).is_ok()
                && block_on(adev.query_emit_pace()).is_ok();
            // async name setter parity: set then clear (leaves the box on its synth default)
            let aname_ok = adev.set_name("async box").is_ok() && adev.clear_name().is_ok();
            reset_motion(&acc);
            let _ = adev.move_rel(12, 0);
            std::thread::sleep(Duration::from_millis(200));
            let amoved = acc.rel_x.load(Ordering::Relaxed);
            let _ = adev.reset();
            // async parity: observers + reconnect mirror the sync Device. Run LAST because reconnect()
            // swaps the serial transport, so a reopen blip can't pollute the checks above.
            let alog_n = adev.logs().try_iter().count();
            let arecon_base = adev.counters().reconnects;
            let arecon_ok = adev.reconnect().is_ok() && adev.counters().reconnects > arecon_base;
            check(
                "async",
                av_ok && ah_ok && aopt_ok && aname_ok && amoved == 12 && arecon_ok,
                format!(
                    "AsyncDevice: version_ok={av_ok}, health_ok={ah_ok}, option_queries_ok={aopt_ok}, name_ok={aname_ok}, reconnect_ok={arecon_ok}, async_logs_drained={alog_n}, async move REL_X={amoved}"
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
        for r in readers {
            let _ = r.join();
        }
        drop(grabs);

        if std::env::var_os("MEDIUS_UNPLUG_TEST").is_some() {
            let reopened = match args.get(2) {
                Some(p) => Device::open(p),
                None => Device::find(),
            };
            match reopened {
                Ok(dev) => {
                    let base = dev.counters().reconnects;
                    let up0 = matches!(dev.query_version(), Ok(v) if v.proto_ver == 3);
                    println!(
                        "\n>>> AUTO-RECONNECT: physically UNPLUG the box's control USB, wait ~2s, then \
                         replug.\n    Waiting up to 60s for the reader to self-heal; NO reconnect() is \
                         called by this test."
                    );
                    let deadline = Instant::now() + Duration::from_secs(60);
                    let mut healed = false;
                    while Instant::now() < deadline {
                        std::thread::sleep(Duration::from_millis(500));
                        if dev.counters().reconnects > base
                            && matches!(dev.query_version(), Ok(v) if v.proto_ver == 3)
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
