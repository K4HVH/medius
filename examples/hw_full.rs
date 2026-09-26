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
        Action, Axis, BearingMode, Blanket, Button, CatchClass, CatchFilter, Class, ClipAction,
        ClipBuilder, ClipFrame, ClipPacketTrigger, ClipState, ClipTrigger, Device, Direction, Edge,
        EmitPace, Input, Key, LedMode, LedTarget, MediaKey, Patch, PatchSection, RebootTarget,
        RenderMode, RewriteAction, RewriteClass, RewriteRule, Setup, Timeline, TrafficClass,
        TransferStatus, Transform, TransformOp,
    };
    use medius::{BEARING_WINDOW_DEFAULT, PROTO_VER};

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
            Button::LEFT => acc.btn_left.load(Ordering::Relaxed),
            Button::RIGHT => acc.btn_right.load(Ordering::Relaxed),
            Button::MIDDLE => acc.btn_middle.load(Ordering::Relaxed),
            Button::SIDE1 => acc.btn_side.load(Ordering::Relaxed),
            Button::SIDE2 => acc.btn_extra.load(Ordering::Relaxed),
            _ => 0,
        }
    }

    // Resolved through /dev/input/by-id: node indexes move on every add, remove or replug, and a
    // wrong node reads zero on every wire check, like broken firmware. By-id names carry the VID:PID.
    fn clone_event_nodes() -> Result<Vec<String>, String> {
        let dir = std::path::Path::new("/dev/input/by-id");
        let entries =
            std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        let mut found = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            // A composite clone's mouse and keyboard are separate nodes; both are grabbed, or injected
            // input leaks to the desktop unverified.
            if !name.starts_with("usb-")
                || !(name.ends_with("-event-mouse") || name.ends_with("-event-kbd"))
            {
                continue;
            }
            let Ok(target) = std::fs::canonicalize(entry.path()) else {
                continue;
            };
            found.push((name, target.to_string_lossy().into_owned()));
        }
        // The clone takes the attached device's VID:PID, so none is fixed.
        found.sort();
        if found.is_empty() {
            return Err("no usb event-mouse or event-kbd node in /dev/input/by-id; is USB1 cabled to this machine?".into());
        }
        let stem = |n: &str| {
            n.rsplit_once("-if").map_or_else(
                || {
                    n.trim_end_matches("-event-mouse")
                        .trim_end_matches("-event-kbd")
                        .to_string()
                },
                |(head, _)| head.to_string(),
            )
        };
        let mouse = found
            .iter()
            .find(|(n, _)| n.ends_with("-event-mouse"))
            .ok_or_else(|| "no usb event-mouse node in /dev/input/by-id".to_string())?;
        let key = stem(&mouse.0);
        Ok(found
            .iter()
            .filter(|(n, _)| stem(n) == key)
            .map(|(_, path)| path.clone())
            .collect())
    }

    pub fn run() -> ExitCode {
        let args: Vec<String> = std::env::args().collect();
        // args[1]: one or more comma-separated evdev nodes.
        let events: Vec<String> = match args.get(1) {
            Some(s) => s
                .split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect(),
            None => match clone_event_nodes() {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{e}");
                    return ExitCode::FAILURE;
                }
            },
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
        // One reader per grabbed node into one accumulator (mouse node: REL/buttons, keyboard node:
        // KEY_*), so a composite clone is verified in full.
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
            let ver_ok = ver
                .as_ref()
                .map(|v| v.proto_ver == PROTO_VER)
                .unwrap_or(false);
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
                format!("proto_ver=={PROTO_VER} ({fw})  health={health:?}"),
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
            // Link drop counters are cumulative since boot; nonzero means a report or injected delta
            // lost between the box's two chips.
            let stats_ok = stats
                .as_ref()
                .map(|s| s.tx_drops == 0 && s.link_rx_drops == 0 && s.host_rx_drops == 0)
                .unwrap_or(false);

            let hz = rate
                .as_ref()
                .ok()
                .and_then(|r| r.native_hz())
                .map(|hz| format!("{hz:.0}"))
                .unwrap_or_else(|| "?".into());
            let confident = rate.as_ref().map(|r| r.confident).unwrap_or(false);
            let (drops, wedges, link_drops, host_drops, relay_drops) = stats
                .as_ref()
                .map(|s| {
                    (
                        s.tx_drops,
                        s.tx_wedges,
                        s.link_rx_drops,
                        s.host_rx_drops,
                        s.relay_drops,
                    )
                })
                .unwrap_or((u16::MAX, u8::MAX, u32::MAX, u32::MAX, u32::MAX));
            let id = info
                .as_ref()
                .map(|i| i.to_string())
                .unwrap_or_else(|_| "?".into());
            check(
                "device info",
                caps_ok && info_ok && rate_ok && stats_ok,
                format!(
                    "mouse={id} caps={caps:?}  rate={hz}Hz confident={confident}  tx_drops={drops} tx_wedges={wedges}  link_rx_drops={link_drops} host_rx_drops={host_drops} relay_drops={relay_drops}"
                ),
            );
        }

        {
            // FIRMWARE: read only.
            let dev = device.as_ref().unwrap();
            let fw = dev.firmware_info();
            let both = fw.as_ref().map(|f| f.host.is_some()).unwrap_or(false);
            let slot_ok = fw
                .as_ref()
                .map(|f| f.slot_size == 0x000F_0000)
                .unwrap_or(false);
            let matched = fw
                .as_ref()
                .map(|f| {
                    f.host.is_none_or(|h| {
                        (h.major, h.minor, h.patch)
                            == (f.device.major, f.device.minor, f.device.patch)
                    })
                })
                .unwrap_or(false);
            let detail = fw
                .as_ref()
                .map(|f| match f.host {
                    Some(h) => format!("device {} | host {} | slot {}B", f.device, h, f.slot_size),
                    None => format!("device {} | host absent", f.device),
                })
                .unwrap_or_else(|e| format!("{e}"));
            check(
                "firmware slots",
                fw.is_ok() && both && slot_ok && matched,
                detail,
            );
        }

        {
            // IMPERFECT: a normal mouse fits the box's endpoints, so it is never over capacity and the
            // clone is faithful. The opt-in is only printed.
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
            // MOVE_RIDE wire round trip and NVS persistence (riding itself needs the rig). Leaves the
            // default (off).
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
            // Per-command override on the wire: with riding on and the mouse idle, a plain move never
            // reaches the PC, a NOW move does, FLUSH sends what was held and DISCARD drops it. Leaves
            // riding off.
            let dev = device.as_ref().unwrap();
            let window = Duration::from_millis(20);
            let _ = dev.set_movement_riding(Some(window));
            std::thread::sleep(Duration::from_millis(60));

            reset_motion(&acc);
            for _ in 0..5 {
                let _ = dev.move_rel(40, 0);
                std::thread::sleep(Duration::from_millis(30));
            }
            std::thread::sleep(Duration::from_millis(200));
            let held = acc.rel_x.load(Ordering::Relaxed);

            reset_motion(&acc);
            let _ = dev.discard_motion();
            for _ in 0..5 {
                let _ = dev.move_rel_now(40, 0);
                std::thread::sleep(Duration::from_millis(3));
            }
            std::thread::sleep(Duration::from_millis(300));
            let bypassed = acc.rel_x.load(Ordering::Relaxed);

            // Only a native cursor-motion report clears held motion, and the mouse stays still here, so
            // each step drops the previous step's motion first or FLUSH reads the running total.
            reset_motion(&acc);
            let _ = dev.discard_motion();
            let _ = dev.move_rel(70, 0);
            let _ = dev.flush_motion();
            std::thread::sleep(Duration::from_millis(300));
            let flushed = acc.rel_x.load(Ordering::Relaxed);

            reset_motion(&acc);
            let _ = dev.discard_motion();
            let _ = dev.move_rel(70, 0);
            let _ = dev.discard_motion();
            let _ = dev.flush_motion();
            std::thread::sleep(Duration::from_millis(300));
            let discarded = acc.rel_x.load(Ordering::Relaxed);

            let _ = dev.set_movement_riding(None);
            std::thread::sleep(Duration::from_millis(60));
            check(
                "movement riding override",
                held == 0 && bypassed == 200 && flushed == 70 && discarded == 0,
                format!(
                    "held={held} (want 0), now={bypassed} (want 200), \
                     flush={flushed} (want 70), discard={discarded} (want 0)"
                ),
            );
        }

        {
            // EMIT wire round trip and NVS persistence (pacing itself needs the rig). Restores LEARNED,
            // the default.
            let dev = device.as_ref().unwrap();
            let set_ok = dev.set_emit_pace(EmitPace::Fixed(500), None).is_ok();
            std::thread::sleep(Duration::from_millis(60));
            let read = dev.query_emit_pace();
            let matched = read
                .as_ref()
                .map(|s| s.mode == EmitPace::Fixed(500) && s.resolved_hz == 500)
                .unwrap_or(false);
            let off_ok = dev.set_emit_pace(EmitPace::Learned, None).is_ok();
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
            // OPTION(RENDER): the render texture, and whether native motion goes through it.
            let dev = device.as_ref().unwrap();
            let mut all_ok = true;
            let mut last = String::new();
            for mode in [
                RenderMode::Off,
                RenderMode::Stock,
                RenderMode::Despiked,
                RenderMode::Unsmoothed,
            ] {
                for full in [false, true] {
                    let set_ok = dev.set_render(mode, full).is_ok();
                    std::thread::sleep(Duration::from_millis(60));
                    let read = dev.query_render();
                    let matched = read
                        .as_ref()
                        .map(|s| s.mode == mode && s.full == full)
                        .unwrap_or(false);
                    all_ok &= set_ok && matched;
                    last = format!("{mode:?}/full={full} -> {read:?}");
                }
            }
            // The pace reports the rendered gate: on LEARNED a rendered stream self-paces at 1 kHz once
            // a profile arms, and stays at the learnt cap before.
            let _ = dev.set_render(RenderMode::Stock, false);
            let _ = dev.set_emit_pace(EmitPace::Learned, None);
            std::thread::sleep(Duration::from_millis(60));
            // `ready` is read before and after the pace query, since a profile arming in between would
            // fail the gate with no defect.
            let before = dev.query_render().map(|s| s.ready).unwrap_or(false);
            let paced = dev.query_emit_pace();
            let ready = dev.query_render().map(|s| s.ready).unwrap_or(false);
            let gate_ok = paced
                .as_ref()
                .map(|s| {
                    let want = if ready { 1000 } else { 0 };
                    s.resolved_hz == want || (before != ready && s.resolved_hz == 1000 - want)
                })
                .unwrap_or(false);
            check(
                "render option",
                all_ok && gate_ok,
                format!("{last}, ready={ready} pace -> {paced:?}"),
            );
            // The checks above leave the box on Stock, which is not where it boots.
            let _ = dev.set_render(RenderMode::Despiked, false);
        }

        {
            // OPTION(SPREAD): the percent round-trips, and the reported interval tracks this loop's
            // command rate.
            let dev = device.as_ref().unwrap();
            let mut all_ok = true;
            let mut last = String::new();
            for pct in [0u16, 50, 250, 100] {
                let set_ok = dev.set_spread(pct).is_ok();
                std::thread::sleep(Duration::from_millis(60));
                let read = dev.query_spread();
                let matched = read.as_ref().map(|s| s.percent == pct).unwrap_or(false);
                all_ok &= set_ok && matched;
                last = format!("{pct} -> {read:?}");
            }
            for _ in 0..140 {
                let _ = dev.move_rel(1, 0);
                std::thread::sleep(Duration::from_millis(4));
            }
            let learnt = dev.query_spread();
            // The estimator buckets to 250 us and takes the fastest cluster's centre, and the sleep only
            // sets a floor, so the window is wide on the slow side.
            let span_ok = learnt
                .as_ref()
                .map(|s| s.percent == 100 && (3500..=6000).contains(&s.span_us))
                .unwrap_or(false);
            check(
                "spread option",
                all_ok && span_ok,
                format!("{last}, 250 Hz loop -> {learnt:?}"),
            );
            let _ = dev.discard_motion();
        }

        {
            // With the opt-in on, a force re-clones and drops the control port mid-suite, so this runs
            // faithful-only, where the box stores the request inert: force_active stays 0 and
            // advertised_hz native, which an echo cannot fake. tools/validate_rate_force.py covers the
            // descriptor half, with the reboot.
            let dev = device.as_ref().unwrap();
            let allowed = dev.query_imperfect().map(|i| i.allowed).unwrap_or(true);
            if allowed {
                check(
                    "rate force",
                    true,
                    "skipped: imperfect clones are allowed, so a force would re-clone the box"
                        .into(),
                );
            } else {
                let native = dev.query_emit_pace().map(|s| s.advertised_hz).unwrap_or(0);
                let asked = if native == 1000 { 125 } else { 1000 };
                let set_ok = dev.set_emit_pace(EmitPace::Learned, Some(asked)).is_ok();
                std::thread::sleep(Duration::from_millis(60));
                let read = dev.query_emit_pace();
                let matched = read
                    .as_ref()
                    .map(|s| {
                        s.force_hz == Some(asked) && !s.force_active && s.advertised_hz == native
                    })
                    .unwrap_or(false);
                let off_ok = dev.set_emit_pace(EmitPace::Learned, None).is_ok();
                std::thread::sleep(Duration::from_millis(60));
                let read_off = dev.query_emit_pace();
                let off_matched = read_off
                    .as_ref()
                    .map(|s| s.force_hz.is_none() && s.advertised_hz == native)
                    .unwrap_or(false);
                check(
                    "rate force",
                    set_ok && matched && off_ok && off_matched,
                    format!(
                        "clone advertises {native} Hz, asked {asked} -> {read:?}, off -> {read_off:?}"
                    ),
                );
            }
        }

        {
            // The name is in RESP(VERSION) beside the MAC; clearing restores the "Medius-XXXX" default.
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
            // The clone cannot show the LED, so this is a smoke check: every mode is accepted, the box
            // stays healthy, and the LED returns to the box.
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
            // LOCK: a locked axis still moves under injection (only physical input is suppressed). The
            // 3 ms inject cadence doubles as the keepalive.
            let dev = device.as_ref().unwrap();
            let _ = dev.reset();
            let _ = dev.lock(Axis::X, Direction::Both);
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
            // LOCK: the LOCKS query reflects the set, is_locked() reads single directions, the reply
            // has one entry per weighed direction, and LOCK_ON is set.
            let dev = device.as_ref().unwrap();
            let _ = dev.reset();
            let _ = dev.lock(Axis::X, Direction::Positive);
            let _ = dev.lock(Button::LEFT, Direction::Positive);
            let locks = dev.query_locks();
            let lock_on = dev.query_health().map(|h| h.lock_on).unwrap_or(false);
            let n = locks.as_ref().map(|l| l.entries().len()).unwrap_or(0);
            let q_ok = locks
                .as_ref()
                .map(|l| {
                    l.is_locked(Axis::X, Direction::Positive)
                        && !l.is_locked(Axis::X, Direction::Negative)
                        && l.is_locked(Button::LEFT, Direction::Positive)
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
            // PAN + BUTTONS PAST FIVE: an AC Pan lock is a box-table operation the query reflects on
            // any clone, and pan injects like the wheel. The box drives a button past five only when
            // the clone declares it, so a button-8 lock in the query is the declared-count gate.
            let dev = device.as_ref().unwrap();
            let _ = dev.reset();
            let nbtn = dev.caps().map(|c| c.mouse.n_buttons).unwrap_or(0);
            let pan_inject_ok = dev.pan(3).is_ok() && dev.pan_now(-3).is_ok();
            let _ = dev.lock(Axis::Pan, Direction::Positive);
            let btn8 = Button::new(8);
            let btn8_inject_ok = dev.press(btn8).is_ok() && dev.force_release(btn8).is_ok();
            let _ = dev.lock(btn8, Direction::Positive);
            let locks = dev.query_locks();
            let pan_locked = locks
                .as_ref()
                .map(|l| l.is_locked(Axis::Pan, Direction::Positive))
                .unwrap_or(false);
            let btn8_locked = locks
                .as_ref()
                .map(|l| l.is_locked(btn8, Direction::Positive))
                .unwrap_or(false);
            let btn8_ok = btn8_locked == (nbtn > 8);
            check(
                "pan + button 8: pan lock/inject, button 8 gated by declared count",
                pan_inject_ok && pan_locked && btn8_inject_ok && btn8_ok,
                format!(
                    "nbtn={nbtn} pan_locked={pan_locked} btn8_locked={btn8_locked} (declared={})",
                    nbtn > 8
                ),
            );
            let _ = dev.reset();
        }

        {
            // SCALE: what the box stores is what it renders.
            let dev = device.as_ref().unwrap();
            let _ = dev.reset();
            let _ = dev.scale(Axis::X, Direction::Negative, 40);
            let _ = dev.scale(Axis::Y, Direction::With, 130);
            // A one-bit field truncates: below a full pass to a block, at or above to a pass, so 50% on
            // a button reads back as 0 and 150% not at all.
            let _ = dev.scale(Button::LEFT, Direction::Positive, 50);
            let _ = dev.scale(Button::RIGHT, Direction::Positive, 150);
            let locks = dev.query_locks();
            let s_ok = locks
                .as_ref()
                .map(|l| {
                    l.scale_of(Axis::X, Direction::Negative) == 40
                        && l.scale_of(Axis::Y, Direction::With) == 130
                        && l.scale_of(Axis::X, Direction::Positive) == medius::LOCK_SCALE_PASS
                        && !l.is_locked(Axis::X, Direction::Negative)
                        && l.scale_of(Button::LEFT, Direction::Positive) == medius::LOCK_SCALE_BLOCK
                        && l.scale_of(Button::RIGHT, Direction::Positive) == medius::LOCK_SCALE_PASS
                })
                .unwrap_or(false);
            let on = dev.query_health().map(|h| h.lock_on).unwrap_or(false);
            check(
                "scale: round trip + one-bit truncation",
                s_ok && on,
                format!("40%/130% kept, 50%/150% on a button truncated ok={s_ok} lock_on={on}"),
            );
            let _ = dev.reset();
        }

        {
            // SCALE: a negative percent inverts.
            let dev = device.as_ref().unwrap();
            let _ = dev.reset();
            let _ = dev.scale(Axis::X, Direction::Both, -100);
            let _ = dev.scale(Axis::Y, Direction::Positive, -50);
            let neg = dev.query_locks();
            let n_ok = neg
                .as_ref()
                .map(|l| {
                    l.scale_of(Axis::X, Direction::Positive) == -100
                        && l.scale_of(Axis::X, Direction::Negative) == -100
                        && l.scale_of(Axis::Y, Direction::Positive) == -50
                        && l.scale_of(Axis::Y, Direction::Negative) == medius::LOCK_SCALE_PASS
                        && !l.is_locked(Axis::X, Direction::Both)
                })
                .unwrap_or(false);
            // A momentary usage has nothing to reverse, and a magnitude past the bound is refused, not
            // clamped; both before the wire.
            let n_usage = matches!(
                dev.scale(Button::LEFT, Direction::Positive, -100),
                Err(medius::Error::LockScaleUsage { .. })
            );
            let n_range = matches!(
                dev.scale(Axis::X, Direction::Both, -1000),
                Err(medius::Error::LockScaleRange { .. })
            );
            check(
                "scale: negative percent reverses the axis",
                n_ok && n_usage && n_range,
                format!("stored={n_ok}, usage refused={n_usage}, range refused={n_range}"),
            );
            let _ = dev.reset();
        }

        {
            // SCALE: a Both scale means the same with or without a live bearing, so the box stores it
            // on the fixed pair and leaves the relative pair passing. Only the box can say which of
            // the four slots took the one number sent.
            let dev = device.as_ref().unwrap();
            let _ = dev.reset();
            let _ = dev.scale(Axis::X, Direction::Both, 50);
            let both = dev.query_locks();
            let b_ok = both
                .as_ref()
                .map(|l| {
                    l.scale_of(Axis::X, Direction::Positive) == 50
                        && l.scale_of(Axis::X, Direction::Negative) == 50
                        && l.scale_of(Axis::X, Direction::With) == medius::LOCK_SCALE_PASS
                        && l.scale_of(Axis::X, Direction::Against) == medius::LOCK_SCALE_PASS
                        && l.entries().len() == 2
                })
                .unwrap_or(false);
            // and an unlock is total: it reaches the relative pair too
            let _ = dev.scale(Axis::X, Direction::Against, medius::LOCK_SCALE_BLOCK);
            let _ = dev.unlock(Axis::X, Direction::Both);
            let cleared = dev.query_locks().map(|l| l.entries().len()).unwrap_or(99);
            check(
                "scale: Both weighs the fixed pair, unlock clears all four",
                b_ok && cleared == 0,
                format!("both_ok={b_ok} entries after unlock={cleared}"),
            );
            let _ = dev.reset();
        }

        {
            // BEARING: the mode changes what the box reports for the relative pair.
            let dev = device.as_ref().unwrap();
            let _ = dev.reset();
            let _ = dev.set_bearing(Some(BEARING_WINDOW_DEFAULT), BearingMode::PerAxis);
            let _ = dev.scale(Axis::X, Direction::With, 130);
            let _ = dev.scale(Axis::Y, Direction::With, 60);
            let per_axis = dev.query_locks();
            let _ = dev.set_bearing(Some(BEARING_WINDOW_DEFAULT), BearingMode::Vector);
            let vector = dev.query_locks();
            let m_ok = matches!(&per_axis, Ok(l)
                if l.scale_of(Axis::X, Direction::With) == 130
                    && l.scale_of(Axis::Y, Direction::With) == 60)
                && matches!(&vector, Ok(l)
                    if l.scale_of(Axis::X, Direction::With) == 60
                        && l.scale_of(Axis::Y, Direction::With) == 60);
            check(
                "bearing: vector mode reports the applied scale",
                m_ok,
                format!(
                    "per-axis X/Y = {:?}/{:?}, vector X/Y = {:?}/{:?} (want 130/60 then 60/60)",
                    per_axis
                        .as_ref()
                        .map(|l| l.scale_of(Axis::X, Direction::With)),
                    per_axis
                        .as_ref()
                        .map(|l| l.scale_of(Axis::Y, Direction::With)),
                    vector
                        .as_ref()
                        .map(|l| l.scale_of(Axis::X, Direction::With)),
                    vector
                        .as_ref()
                        .map(|l| l.scale_of(Axis::Y, Direction::With)),
                ),
            );
            let _ = dev.reset();
        }

        {
            // LOCK: the key blanket honours its direction, one blanket per edge, reported as the edges
            // it blocks, never as a Both the box does not hold.
            let dev = device.as_ref().unwrap();
            let _ = dev.reset();
            let _ = dev.lock_all(Blanket::Keys, Direction::Positive);
            let press_only = dev.query_locks();
            let _ = dev.lock_all(Blanket::Keys, Direction::Negative);
            let both_edges = dev.query_locks();
            let _ = dev.unlock_all(Blanket::Keys, Direction::Positive);
            let release_only = dev.query_locks();
            let dirs = |l: &Result<medius::Locks, medius::Error>| {
                l.as_ref()
                    .map(|l| l.entries().iter().map(|e| e.direction).collect::<Vec<_>>())
                    .unwrap_or_default()
            };
            let k_ok = dirs(&press_only) == vec![Direction::Positive]
                && dirs(&both_edges) == vec![Direction::Positive, Direction::Negative]
                && dirs(&release_only) == vec![Direction::Negative];
            check(
                "lock: key blanket reports the edges it blocks",
                k_ok,
                format!(
                    "press-only {:?}, both {:?}, release-only {:?}",
                    dirs(&press_only),
                    dirs(&both_edges),
                    dirs(&release_only)
                ),
            );
            let _ = dev.reset();
        }

        {
            // LOCK: a media usage has no edges; whatever edge is asked, the box suppresses it whole and
            // reports Both.
            let dev = device.as_ref().unwrap();
            let _ = dev.reset();
            let _ = dev.lock(MediaKey::MUTE, Direction::RELEASE);
            let l = dev.query_locks();
            let m_ok = matches!(&l, Ok(l)
                if l.entries().iter().any(|e| e.direction == Direction::Both)
                    && l.is_locked(MediaKey::MUTE, Direction::Both));
            // and a relative direction is refused by the crate rather than dropped by the box
            let refused = matches!(
                dev.lock(MediaKey::MUTE, Direction::Against),
                Err(medius::Error::RelativeDirection { .. })
            ) && matches!(
                dev.lock(Button::LEFT, Direction::With),
                Err(medius::Error::RelativeDirection { .. })
            );
            check(
                "lock: media has no edges, relative direction refused",
                m_ok && refused,
                format!("media reported Both ok={m_ok} relative refused={refused}"),
            );
            let _ = dev.reset();
        }

        {
            // BEARING: the option round-trips and persists in NVS like the other OPTION ids.
            let dev = device.as_ref().unwrap();
            let _ = dev.set_bearing(Some(Duration::from_millis(35)), BearingMode::Vector);
            let a = dev.query_bearing();
            let _ = dev.set_bearing(None, BearingMode::PerAxis);
            let b = dev.query_bearing();
            let _ = dev.set_bearing(Some(BEARING_WINDOW_DEFAULT), BearingMode::PerAxis);
            let ok = matches!(&a, Ok(x) if x.window == Some(Duration::from_millis(35))
                && x.mode == BearingMode::Vector && x.is_live())
                && matches!(&b, Ok(x) if x.window.is_none() && !x.is_live());
            check(
                "bearing: option round trip",
                ok,
                format!("set 35ms/vector -> {a:?}; off -> {b:?}"),
            );
        }

        {
            // LOCK: injection overrides a locked button (block-press, but a forced press wins).
            let dev = device.as_ref().unwrap();
            let _ = dev.reset();
            let _ = dev.lock(Button::LEFT, Direction::Positive);
            let _ = dev.press(Button::LEFT);
            std::thread::sleep(Duration::from_millis(200));
            let down = btn_val(&acc, Button::LEFT);
            check(
                "lock: inject overrides",
                down == 1,
                format!("Left press-locked, injected press -> BTN_LEFT={down}"),
            );
            let _ = dev.reset();
            std::thread::sleep(Duration::from_millis(150));
        }

        {
            // LOCK safety: RESET clears every lock; the keepalive keeps a lock while the client runs,
            // and the firmware clears it only on real control-PC silence (a crash). A Both lock is two
            // entries, one per fixed sign; the relative pair stays at a full pass, unreported.
            let dev = device.as_ref().unwrap();
            let _ = dev.lock(Axis::Y, Direction::Both);
            let _ = dev.reset();
            let after_reset = dev.query_locks().map(|l| l.entries().len()).unwrap_or(99);

            let _ = dev.lock(Axis::Y, Direction::Both);
            let before = dev.query_locks().map(|l| l.entries().len()).unwrap_or(0);
            std::thread::sleep(Duration::from_millis(1400)); // longer than the box silence window
            let after_hold = dev.query_locks().map(|l| l.entries().len()).unwrap_or(99);
            let _ = dev.reset();
            check(
                "lock: reset + keepalive holds",
                after_reset == 0 && before == 2 && after_hold == 2,
                format!(
                    "reset->{after_reset} locks; y-lock {before}, held across 1.4s {after_hold}"
                ),
            );
        }

        {
            // LOCK (keyboard/blanket): a key lock and the all-keys blanket set HEALTH lock_on and RESET
            // clears them; the physical block needs keyboard input (run `medius.py`).
            let dev = device.as_ref().unwrap();
            let has_kbd = dev.query_health().map(|h| h.kbd_attached).unwrap_or(false);
            if has_kbd {
                let _ = dev.lock(Key::A, Direction::Both);
                let on1 = dev.query_health().map(|h| h.lock_on).unwrap_or(false);
                let _ = dev.unlock(Key::A, Direction::Both);
                let _ = dev.lock_all(Blanket::Keys, Direction::Both);
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
            // CATCH: subscribe, confirm CATCH_ON and the table via query_catch; RESET clears catch and
            // disconnects the host stream.
            //
            // Input classes only: an idle mouse raises no input event, so "quiet while idle" holds,
            // but HID_IN and EMIT fire on every report of a streaming device and would fail a working
            // box.
            let dev = device.as_ref().unwrap();
            let stream = dev.catch_events([
                CatchFilter::watch_axes(),
                CatchFilter::watch_class(Class::Button),
            ]);
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
            // Drain first: recv() returns buffered events before reporting the disconnect, so an ended
            // stream runs out rather than erroring at once.
            let stream_ended = stream
                .as_ref()
                .map(|s| {
                    while s.try_recv().is_some() {}
                    s.recv().is_err()
                })
                .unwrap_or(false);
            check(
                "catch: subscribe + reset",
                on && entries == 2 && idle_quiet && off && cleared && stream_ended,
                format!(
                    "CATCH_ON={on} entries={entries} idle_quiet={idle_quiet}; reset->off={off} cleared={cleared} stream_ended={stream_ended}"
                ),
            );
        }

        {
            // Catch timestamps: every stamp in a short window advances by a plausible amount.
            let dev = device.as_ref().unwrap();
            let mut stamps: Vec<u32> = Vec::new();
            if let Ok(stream) = dev.catch_events([
                CatchFilter::watch_axes(),
                CatchFilter::watch_class(Class::Button),
            ]) {
                let deadline = std::time::Instant::now() + Duration::from_secs(2);
                while std::time::Instant::now() < deadline {
                    // Both classes are host-chip stamped, so the monotonicity check compares one
                    // domain.
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
            // HID_IN is host-stamped (the real device produced it) and EMIT device-stamped (the clone
            // did). A wrong domain gives plausible wrong deltas, not an error, so it is asserted.
            //
            // EMIT is driven by injecting: a change-driven mouse NAKs at rest (the Mamba Elite sends
            // nothing untouched), so waiting alone sees neither class. HID_IN needs the device to
            // report, and says so when it did not.
            let dev = device.as_ref().unwrap();
            let mut hid_in = 0usize;
            let mut emit = 0usize;
            let mut domains_right = true;
            if let Ok(stream) = dev.catch_events([
                CatchFilter::traffic_class(TrafficClass::HidIn),
                CatchFilter::traffic_class(TrafficClass::Emit),
            ]) {
                let injector = {
                    let d = dev.clone();
                    let stop = Arc::new(AtomicBool::new(false));
                    let flag = Arc::clone(&stop);
                    let h = std::thread::spawn(move || {
                        while !flag.load(Ordering::Relaxed) {
                            let _ = d.move_rel(1, 0);
                            let _ = d.move_rel(-1, 0);
                            std::thread::sleep(Duration::from_millis(4));
                        }
                    });
                    (stop, h)
                };
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
                injector.0.store(true, Ordering::Relaxed);
                let _ = injector.1.join();
            }
            let _ = dev.reset();
            // EMIT must have flowed: this drove it.
            let ok =
                emit > 0 && domains_right && (hid_in == 0 || hid_in.abs_diff(emit) <= emit / 4 + 2);
            check(
                "catch: traffic classes",
                ok,
                format!(
                    "hid_in={hid_in} emit={emit} clock_domains_correct={domains_right}{}",
                    if hid_in == 0 {
                        " (device silent at rest; move it to exercise HID_IN)"
                    } else {
                        ""
                    }
                ),
            );
        }

        {
            // An exact-id input subscription, on hardware.
            let dev = device.as_ref().unwrap();
            let mut wanted = 0usize;
            let mut unwanted = 0usize;
            if let Ok(stream) = dev.catch_events([CatchFilter::watch_axis(medius::Axis::X)]) {
                let deadline = std::time::Instant::now() + Duration::from_secs(3);
                while std::time::Instant::now() < deadline {
                    if let Some(medius::CatchEvent::Motion(m)) =
                        stream.recv_timeout(Duration::from_millis(100))
                    {
                        if m.dx != 0 {
                            wanted += 1;
                        } else {
                            unwanted += 1;
                        }
                    }
                }
            }
            let _ = dev.reset();
            // Every event received must have moved the named axis; no count, as the mouse may be still.
            check(
                "catch: exact-id input filter",
                unwanted == 0,
                format!("{wanted} moved X, {unwanted} did not (move the mouse if both are 0)"),
            );
        }

        {
            // Decoded input edges.
            let dev = device.as_ref().unwrap();
            let (mut presses, mut releases, mut motions) = (0usize, 0usize, 0usize);
            let mut consistent = true;
            let mut held: Vec<medius::Usage> = Vec::new();
            if let Ok(mut input) = dev.input_events(CatchFilter::all_input()) {
                let deadline = std::time::Instant::now() + Duration::from_secs(3);
                while std::time::Instant::now() < deadline {
                    let Some(ev) = input.recv_timeout(Duration::from_millis(100)) else {
                        continue;
                    };
                    match ev.input {
                        Input::Press(u) => {
                            presses += 1;
                            consistent &= !held.contains(&u);
                            held.push(u);
                        }
                        Input::Release(u) => {
                            releases += 1;
                            consistent &= held.contains(&u);
                            held.retain(|h| *h != u);
                        }
                        Input::Motion { dx, dy, dz, pan } => {
                            motions += 1;
                            consistent &= (dx, dy, dz, pan) != (0, 0, 0, 0);
                        }
                    }
                }
            }
            // The three refusals, on the shipped binary.
            let refuses = matches!(
                dev.input_events([CatchFilter::traffic_class(TrafficClass::VendorBulk)]),
                Err(medius::Error::NotAnInputFilter { .. })
            ) && matches!(
                dev.input_events([CatchFilter::everything()]),
                Err(medius::Error::WildcardNotInput)
            ) && matches!(
                dev.input_events([CatchFilter::watch(Button::LEFT).on_press()]),
                Err(medius::Error::HalfEdgeInputFilter)
            );
            let _ = dev.reset();
            check(
                "catch: decoded input edges",
                consistent && refuses,
                format!(
                    "{presses} press, {releases} release, {motions} motion; refusals_ok={refuses} \
                     (move and click the mouse if all three are 0)"
                ),
            );
        }

        {
            // Timeline: box microseconds unwrapped onto this machine's clock. EMIT is device-chip
            // stamped and injection drives it, so no input is needed.
            let dev = device.as_ref().unwrap();
            let mut time = Timeline::new();
            let mut n = 0usize;
            let mut monotonic = true;
            let mut last: Option<(u64, std::time::Instant)> = None;
            if let Ok(stream) = dev.catch_events([CatchFilter::traffic_class(TrafficClass::Emit)]) {
                let stop = Arc::new(AtomicBool::new(false));
                let injector = {
                    let d = dev.clone();
                    let flag = Arc::clone(&stop);
                    std::thread::spawn(move || {
                        while !flag.load(Ordering::Relaxed) {
                            let _ = d.move_rel(1, 0);
                            let _ = d.move_rel(-1, 0);
                            std::thread::sleep(Duration::from_millis(4));
                        }
                    })
                };
                let deadline = std::time::Instant::now() + Duration::from_secs(2);
                while std::time::Instant::now() < deadline {
                    if let Some(ev) = stream.recv_timeout(Duration::from_millis(100)) {
                        let st = time.observe(&ev);
                        if let Some((prev_box, prev_host)) = last {
                            monotonic &= st.box_us >= prev_box && st.host >= prev_host;
                        }
                        last = Some((st.box_us, st.host));
                        n += 1;
                    }
                }
                stop.store(true, Ordering::Relaxed);
                let _ = injector.join();
            }
            let _ = dev.reset();
            check(
                "catch: host timeline",
                n > 0 && monotonic,
                format!(
                    "{n} events mapped, monotonic={monotonic}, samples={}",
                    time.samples(medius::ClockDomain::DeviceChip)
                ),
            );
        }

        {
            // Inter-chip clock estimate; converges only with current firmware on both chips. An absent
            // estimate reads age=None, not a zero offset.
            let dev = device.as_ref().unwrap();
            let st = dev
                .catch_events([CatchFilter::everything()])
                .ok()
                .and_then(|_s| {
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
            // KEYBOARD + MEDIA: an injected KEY_A reaches the grabbed evdev (1 then 0), catching a key
            // on the wrong interface; the grabbed node must be the keyboard's.
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
            // Clip playback: 200 mouse moves and a KEY_A hold drive both interfaces; the grabbed node
            // reports its own. Also covers auto-lock and counters.
            let dev = device.as_ref().unwrap();
            let _ = dev.reset();
            reset_motion(&acc);
            acc.key_a.store(0, Ordering::Relaxed);
            let clip = dev.clip();
            let mut b = ClipBuilder::new();
            // Hold KEY_A across the whole motion run so the held-usage snapshot is sampled mid-hold.
            b.press(Key::A);
            for _ in 0..199 {
                b.move_by(10, 0);
            }
            // One frame with every motion field; without declared pan the box drops that field and
            // plays the rest.
            b.frame(ClipFrame::new().move_by(10, 0).pan(1));
            b.release(Key::A);
            let appended = clip.append(&b).is_ok();
            let loaded = clip.query_status().map(|s| s.total > 0).unwrap_or(false);
            // Selective auto-lock: cursor axes and buttons only, keyboard free (the clip still drives
            // KEY_A; only physical input is locked).
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

            // Trigger set and config readback (firing needs a physical press, in the physical suite):
            // bind two, read back, unbind, clear.
            let _ = clip.clear();
            let bound_key = clip
                .bind(ClipTrigger::new(Key::A, Edge::Press, ClipAction::Start))
                .is_ok();
            let bound_btn = clip
                .bind(ClipTrigger::new(Button::SIDE1, Edge::Release, ClipAction::Stop).consume())
                .is_ok();
            let loop_set = clip.set_loop(true).is_ok();
            let ride_set = clip.set_ride(true).is_ok();
            let cfg_ok = clip
                .query_config()
                .map(|c| {
                    c.loop_
                        && c.ride
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
            let _ = clip.set_ride(false);
            check(
                "clip trigger set + config readback",
                bound_key
                    && bound_btn
                    && loop_set
                    && ride_set
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
                Button::LEFT,
                Button::RIGHT,
                Button::MIDDLE,
                Button::SIDE1,
                Button::SIDE2,
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
                    if matches!(button, Button::SIDE1 | Button::SIDE2) && other >= 0 {
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
            let _ = dev.press(Button::LEFT);
            std::thread::sleep(Duration::from_millis(200));
            let down = acc.btn_left.load(Ordering::Relaxed);
            let _ = dev.force_release(Button::LEFT);
            std::thread::sleep(Duration::from_millis(200));
            let up = acc.btn_left.load(Ordering::Relaxed);
            let _ = dev.release(Button::LEFT);
            check(
                "force_release",
                down == 1 && up == 0,
                format!("press→{down}, force_release→{up}"),
            );
        }

        {
            let dev = device.as_ref().unwrap();
            let _ = dev.inject(Button::RIGHT, Action::Press);
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
            // Halving is a SUSTAINED rate fault, so this measures the sustained rate.
            let dev = device.as_ref().unwrap();
            let _ = dev.reset();
            std::thread::sleep(Duration::from_millis(100));
            let warm = Instant::now() + Duration::from_millis(250);
            let mut next = Instant::now();
            while Instant::now() < warm {
                let _ = dev.move_rel(1, 0);
                next += Duration::from_millis(1);
                let now = Instant::now();
                if next > now {
                    std::thread::sleep(next - now);
                }
            }
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
            let _ = dev.press(Button::RIGHT);
            std::thread::sleep(Duration::from_millis(200));
            let down = acc.btn_right.load(Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(1600));
            let still = acc.btn_right.load(Ordering::Relaxed);
            let _ = dev.release(Button::RIGHT);
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
            let _ = dev.press(Button::SIDE1);
            std::thread::sleep(Duration::from_millis(200));
            let rc = dev.reconnect();
            std::thread::sleep(Duration::from_millis(300));
            let ver_ok = dev.query_version().is_ok();
            reset_motion(&acc);
            let _ = dev.move_rel(10, 0);
            std::thread::sleep(Duration::from_millis(200));
            let moved = acc.rel_x.load(Ordering::Relaxed);
            let side_held = btn_val(&acc, Button::SIDE1) == 1;
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
            // A reconnect re-sends no clip and reads back what the box holds, so a retained clip
            // survives a blip shorter than the silence window.
            let dev = device.as_ref().unwrap();
            let clip = dev.clip();
            let _ = dev.reset();
            let _ = clip.clear();
            let loaded = clip.set_retain(true).is_ok() && {
                let mut one = ClipBuilder::new();
                one.move_by(1, 0);
                clip.append(&one).is_ok() && clip.finalize().is_ok()
            };
            std::thread::sleep(Duration::from_millis(100));
            let before = clip.query_status().map(|s| s.total).unwrap_or(0);
            let rc = dev.reconnect();
            std::thread::sleep(Duration::from_millis(1600));
            let after = clip.query_status().map(|s| s.total).unwrap_or(0);
            let retained = clip.query_config().map(|c| c.retain).unwrap_or(false);
            let _ = clip.clear();
            let _ = clip.set_retain(false);
            check(
                "clip: held across a reconnect",
                loaded && rc.is_ok() && before > 0 && after == before && retained,
                format!(
                    "loaded={loaded}, reconnect={:?}, ring {before} B before, {after} B 1.6 s after, retain={retained}",
                    rc.map(|_| "Ok")
                ),
            );
        }

        {
            // A held Y scale survives the host chip's reboot: the chip returns within the link watch's
            // timeout, so nothing is released; a counted release (a slower reboot) gets the scale
            // re-sent.
            let dev = device.as_ref().unwrap();
            let _ = dev.scale(Axis::Y, Direction::Both, 40);
            let session = dev.query_stats().map(|s| s.session).ok();
            let _ = dev.reboot(RebootTarget::HostRun);
            std::thread::sleep(Duration::from_secs(2));
            let mut recovered = matches!(dev.query_version(), Ok(v) if v.proto_ver == PROTO_VER);
            for _ in 0..10 {
                if recovered {
                    break;
                }
                let _ = dev.reconnect();
                std::thread::sleep(Duration::from_millis(500));
                recovered = matches!(dev.query_version(), Ok(v) if v.proto_ver == PROTO_VER);
            }
            let released = dev.query_stats().map(|s| s.session).ok() != session;
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut y_scale = -1;
            while Instant::now() < deadline {
                y_scale = dev
                    .query_locks()
                    .map(|l| l.scale_of(Axis::Y, Direction::Positive))
                    .unwrap_or(-1);
                if y_scale == 40 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            reset_motion(&acc);
            let _ = dev.move_rel(10, 0);
            std::thread::sleep(Duration::from_millis(200));
            let moved = acc.rel_x.load(Ordering::Relaxed);
            let _ = dev.reset();
            check(
                "reboot-to-run",
                recovered && moved == 10 && y_scale == 40,
                format!(
                    "reboot(HostRun) → responsive={recovered}, session moved={released}, Y scale \
                     {y_scale}, post-reboot move REL_X={moved}"
                ),
            );
        }

        // Advanced control layer (§3.14): raw injection, control transfers, rewrite rules,
        // descriptor patches.
        {
            let dev = device.as_ref().unwrap();
            let opt = dev.allow_imperfect_clones(true);

            // TRANSFER: read the real device's 18-byte device descriptor over EP0.
            let t = dev.transfer(0, Setup::new(0x80, 0x06, 0x0100, 0x0000, 18), &[]);
            let transfer_ok = matches!(&t, Ok(o)
                if o.status == TransferStatus::Ok && o.data().len() >= 2 && o.data()[1] == 0x01);
            check(
                "advanced control: transfer",
                opt.is_ok() && transfer_ok,
                format!(
                    "EP0 GET_DESCRIPTOR(Device) -> {:?} ({} B)",
                    t.as_ref().map(|o| o.status),
                    t.as_ref().map(|o| o.data().len()).unwrap_or(0)
                ),
            );

            // RAW: a null report on the clone's interrupt-IN endpoint. RAW skips the opt-in check, so
            // the gate is tested on set_rewrite.
            let raw_on = dev.raw(1, Direction::IN, &[0, 0, 0, 0]).is_ok();
            let _ = dev.allow_imperfect_clones(false);
            let raw_off = dev.raw(1, Direction::IN, &[0, 0, 0, 0]).is_ok();
            let gated = matches!(
                dev.set_rewrite(&RewriteRule::new(
                    RewriteClass::Emit,
                    1,
                    Direction::IN,
                    RewriteAction::Pass
                )),
                Err(medius::Error::ImperfectRequired)
            );
            let _ = dev.allow_imperfect_clones(true);
            check(
                "advanced control: raw",
                raw_on && raw_off && gated,
                format!(
                    "sent when allowed={raw_on}, sent with opt-in off={raw_off}, set_rewrite gated={gated}"
                ),
            );

            // REWRITE: a no-op PASS rule on the emit wire; read it back, then clear.
            let rule = RewriteRule::new(RewriteClass::Emit, 1, Direction::IN, RewriteAction::Pass);
            let set_ok = dev.set_rewrite(&rule).is_ok();
            let q = dev.query_rewrite();
            let present =
                matches!(&q, Ok(tab) if tab.entries.iter().any(|e| e.class == RewriteClass::Emit));
            let generation = q.as_ref().map(|tab| tab.generation).unwrap_or(0);
            let entry_ok = dev
                .query_rewrite_entry(0)
                .map(|r| r.action == RewriteAction::Pass)
                .unwrap_or(false);
            let health_on = dev.query_health().map(|h| h.rewrite_on).unwrap_or(false);
            let clear_ok = dev.clear_rewrite().is_ok();
            let cleared = matches!(dev.query_rewrite(), Ok(tab) if tab.entries.is_empty());
            check(
                "advanced control: rewrite",
                set_ok && present && entry_ok && health_on && clear_ok && cleared,
                format!(
                    "set={set_ok}, present={present} gen={generation}, entry={entry_ok}, \
                     health.rewrite_on={health_on}, clear={clear_ok}, cleared={cleared}"
                ),
            );

            // The next two checks need the exact frame a (3, 0) move produces, learnt from the EMIT
            // tap as the report seen once (a streaming mouse fills the rest with its idle report).
            // Spreading is off, so the move is one frame.
            let clip = dev.clip();
            let _ = clip.clear();
            let _ = dev.clear_rewrite();
            let spread_was = dev.query_spread().map(|s| s.percent).unwrap_or(100);
            let _ = dev.set_spread(0);
            std::thread::sleep(Duration::from_millis(60));
            let mut seen: Vec<(Vec<u8>, usize)> = Vec::new();
            if let Ok(stream) = dev.catch_events([CatchFilter::traffic(TrafficClass::Emit, 1)]) {
                let _ = dev.move_rel(3, 0);
                let until = Instant::now() + Duration::from_millis(250);
                while Instant::now() < until {
                    if let Some(medius::CatchEvent::Traffic(t)) =
                        stream.recv_timeout(Duration::from_millis(50))
                    {
                        match seen.iter_mut().find(|(b, _)| b == &t.bytes) {
                            Some((_, n)) => *n += 1,
                            None => seen.push((t.bytes.clone(), 1)),
                        }
                    }
                }
            }
            let once: Vec<&Vec<u8>> = seen
                .iter()
                .filter(|(_, n)| *n == 1)
                .map(|(b, _)| b)
                .collect();
            let frame: Option<Vec<u8>> = (once.len() == 1).then(|| once[0].clone());
            let learnt = format!(
                "frame learnt={} ({} distinct reports)",
                frame.is_some(),
                seen.len()
            );

            // Clip entries on this layer: a raw report and a control transfer on the clip's timeline.
            let get_device = Setup::new(0x80, 0x06, 0x0100, 0x0000, 18);
            let mut adv = ClipBuilder::new();
            adv.gap(2).frame(
                ClipFrame::new()
                    .raw(1, Direction::IN, frame.clone().unwrap_or_default())
                    .transfer(0, get_device, []),
            );
            let before = clip.query_status().unwrap_or_default();
            let _ = dev.allow_imperfect_clones(false);
            reset_motion(&acc);
            let gated_sent = clip.append(&adv).is_ok() && clip.start().is_ok();
            std::thread::sleep(Duration::from_millis(150));
            let _ = clip.stop();
            let off = clip.query_status().unwrap_or_default();
            let off_x = acc.rel_x.load(Ordering::Relaxed);
            let _ = dev.allow_imperfect_clones(true);
            reset_motion(&acc);
            let mut answer = None;
            if let Ok(stream) =
                dev.catch_events([CatchFilter::traffic_class(TrafficClass::ClipTransfer)])
            {
                let _ = clip.append(&adv).and_then(|_| clip.start());
                let until = Instant::now() + Duration::from_millis(800);
                while answer.is_none() && Instant::now() < until {
                    if let Some(medius::CatchEvent::Traffic(t)) =
                        stream.recv_timeout(Duration::from_millis(100))
                    {
                        answer = Some((t.transfer_status(), t.data().to_vec()));
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(100));
            let _ = clip.stop();
            let on = clip.query_status().unwrap_or_default();
            let on_x = acc.rel_x.load(Ordering::Relaxed);
            let gated = off.gated.wrapping_sub(before.gated);
            let ran = on.xfers.wrapping_sub(off.xfers);
            let answered = matches!(&answer, Some((Some(TransferStatus::Ok), d))
                if d.len() >= 2 && d[1] == 0x01);
            check(
                "clip: raw and transfer entries",
                frame.is_some()
                    && gated_sent
                    && gated == 2
                    && off_x == 0
                    && on.gated == off.gated
                    && on_x == 3
                    && ran == 1
                    && answered,
                format!(
                    "{learnt}; opt-in off: gated +{gated}, REL_X {off_x}; on: gated +{}, REL_X {on_x}, \
                     xfers +{ran}, answer {:?}",
                    on.gated.wrapping_sub(off.gated),
                    answer.as_ref().map(|(s, d)| (*s, d.len()))
                ),
            );

            // A packet trigger that runs a clip verb.
            let _ = clip.clear();
            let armed = frame.is_some() && clip.set_retain(true).is_ok() && {
                let mut one = ClipBuilder::new();
                one.move_by(7, 0);
                clip.append(&one).is_ok() && clip.finalize().is_ok()
            };
            let mut bound = false;
            let mut read_back = false;
            let mut trigger = None;
            if let (true, Some(frame)) = (armed, &frame) {
                let head = &frame[..frame.len().min(medius::PKT_MATCH_MAX)];
                let t =
                    ClipPacketTrigger::new(TrafficClass::Emit, 1, Direction::IN, ClipAction::Start)
                        .matching(head.to_vec(), vec![0xFF; head.len()]);
                bound = clip.bind_packet(&t).is_ok();
                read_back = clip
                    .query_config()
                    .map(|c| c.packet_triggers.iter().any(|e| e.trigger == t))
                    .unwrap_or(false);
                trigger = Some(t);
            }
            reset_motion(&acc);
            let _ = dev.move_rel(3, 0);
            std::thread::sleep(Duration::from_millis(300));
            let trig_x = acc.rel_x.load(Ordering::Relaxed);
            let hits = clip
                .query_config()
                .ok()
                .and_then(|c| {
                    c.packet_triggers
                        .iter()
                        .find(|e| Some(&e.trigger) == trigger.as_ref())
                        .map(|e| e.hits)
                })
                .unwrap_or(0);
            let unbound = trigger.is_some_and(|t| {
                clip.unbind_packet(&t).is_ok()
                    && clip
                        .query_config()
                        .map(|c| c.packet_triggers.is_empty())
                        .unwrap_or(false)
            });
            let _ = clip.clear();
            let _ = clip.set_retain(false);
            let _ = dev.set_spread(spread_was);
            check(
                "clip: packet trigger runs a clip verb",
                armed && bound && read_back && trig_x == 10 && hits == 1 && unbound,
                format!(
                    "{learnt}, bound={bound}, read back={read_back}, move 3 + clip 7 = REL_X {trig_x}, \
                     hits={hits}, unbound={unbound}"
                ),
            );

            // CATCH marks a packet a rule acted on: a PATCH at EMIT turning the (3, 0) frame's X into 5
            // reaches the PC as 5 on a marked frame; without the rule, neither.
            let _ = dev.set_spread(0);
            let x_at = frame.as_ref().and_then(|f| {
                let at: Vec<usize> = (0..f.len()).filter(|&i| f[i] == 3).collect();
                (at.len() == 1).then(|| at[0])
            });
            let emitted = || -> Option<(bool, i64)> {
                let stream = dev
                    .catch_events([CatchFilter::traffic(TrafficClass::Emit, 1)])
                    .ok()?;
                reset_motion(&acc);
                let _ = dev.move_rel(3, 0);
                let until = Instant::now() + Duration::from_millis(250);
                let mut acted = false;
                while Instant::now() < until {
                    if let Some(medius::CatchEvent::Traffic(t)) =
                        stream.recv_timeout(Duration::from_millis(50))
                    {
                        acted |= t.rule_acted();
                    }
                }
                Some((acted, acc.rel_x.load(Ordering::Relaxed)))
            };
            let (mut ruled, mut plain) = (None, None);
            if let (Some(f), Some(i)) = (&frame, x_at) {
                let head = &f[..f.len().min(medius::PKT_MATCH_MAX)];
                let rule =
                    RewriteRule::new(RewriteClass::Emit, 1, Direction::IN, RewriteAction::Patch)
                        .matching(head.to_vec(), vec![0xFF; head.len()])
                        .at_offset(i as u16)
                        .with_payload(vec![5]);
                if dev.set_rewrite(&rule).is_ok() {
                    ruled = emitted();
                }
                let _ = dev.clear_rewrite();
                plain = emitted();
            }
            let _ = dev.set_spread(spread_was);
            check(
                "catch: rule mark on the packet",
                ruled == Some((true, 5)) && plain == Some((false, 3)),
                format!(
                    "{learnt}, X byte at {x_at:?}; ruled (marked, REL_X) {ruled:?}, without the rule \
                     {plain:?}"
                ),
            );

            // PATCH: store a device-descriptor patch (unapplied), read it back, then clear.
            let patch = Patch::new(PatchSection::Device, 12, vec![0x00, 0x01]);
            let pset_ok = dev.set_patch(&patch).is_ok();
            let ps = dev.query_patches();
            let ppresent = matches!(&ps, Ok(s)
                if s.entries.iter().any(|e| e.section == PatchSection::Device) && s.pending);
            let pentry_ok = dev
                .query_patch_entry(0)
                .map(|p| p.section == PatchSection::Device && p.bytes == vec![0x00, 0x01])
                .unwrap_or(false);
            let pclear_ok = dev.clear_patch().is_ok(); // the set was only stored, so the clone stays
            check(
                "advanced control: patch",
                pset_ok && ppresent && pentry_ok && pclear_ok,
                format!(
                    "set={pset_ok}, present+pending={ppresent}, entry={pentry_ok}, clear={pclear_ok}"
                ),
            );

            // TRANSFORM (§3.15): swap X and Y (ungated), read back, clear.
            let tset_ok = dev.transform_swap(Axis::X, Axis::Y).is_ok();
            let tq = dev.query_transforms();
            let tpresent = matches!(&tq, Ok(t)
                if t.entries.iter().any(|e| e.op == TransformOp::Swap
                    && *e == Transform::swap(Axis::X, Axis::Y)));
            let thealth_on = dev.query_health().map(|h| h.transform_on).unwrap_or(false);
            let tclear_ok = dev.clear_transforms().is_ok();
            let tcleared = matches!(dev.query_transforms(), Ok(t) if t.entries.is_empty());
            check(
                "transforms: field transform",
                tset_ok && tpresent && thealth_on && tclear_ok && tcleared,
                format!(
                    "swap(X,Y) set={tset_ok}, present={tpresent}, health.transform_on={thealth_on}, \
                     clear={tclear_ok}, cleared={tcleared}"
                ),
            );

            // Remap and application order: two remaps writing one axis compose differently reversed,
            // so readback order is state. The second sorts below the first by wire key, so a table
            // held in key order would come back swapped.
            let _ = dev.clear_transforms();
            let ord_ok = dev.transform_remap(Axis::Wheel, Axis::Y).is_ok()
                && dev.transform_remap(Axis::X, Axis::Y).is_ok();
            let ord = dev.query_transforms();
            let ord_right = matches!(&ord, Ok(t) if t.entries.len() == 2
                && t.entries[0] == Transform::remap(Axis::Wheel, Axis::Y)
                && t.entries[1] == Transform::remap(Axis::X, Axis::Y));
            // An overwrite keeps its row: same key, new op, still first.
            let ow_ok = dev.transform_swap(Axis::Wheel, Axis::Y).is_ok();
            let ow_right = matches!(dev.query_transforms(), Ok(t) if t.entries.len() == 2
                && t.entries[0] == Transform::swap(Axis::Wheel, Axis::Y));
            let _ = dev.clear_transforms();
            check(
                "transforms: swap, remap, apply order and overwrite in place",
                ord_ok && ord_right && ow_ok && ow_right,
                format!(
                    "set={ord_ok}, readback in install order={ord_right}, overwrite={ow_ok}/{ow_right}"
                ),
            );

            // The crate's two pre-wire refusals; each must leave the table unchanged.
            let r_pair = dev
                .transform(&Transform::new(TransformOp::Swap, Axis::X, Button::SIDE1))
                .is_err();
            let r_self = dev.transform(&Transform::remap(Axis::X, Axis::X)).is_err();
            let r_clean = matches!(dev.query_transforms(), Ok(t) if t.entries.is_empty());
            check(
                "transforms: refusals before the wire",
                r_pair && r_self && r_clean,
                format!("pair={r_pair}, field-onto-itself={r_self}, table untouched={r_clean}"),
            );

            let _ = dev.allow_imperfect_clones(false);
            let _ = dev.reset();
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
                .map(|v| v.proto_ver == PROTO_VER)
                .unwrap_or(false);
            let ah_ok = block_on(adev.query_health())
                .map(|h| h.link_up)
                .unwrap_or(false);
            // async option queries on hardware (the sync ones run above)
            let aopt_ok = block_on(adev.query_movement_riding()).is_ok()
                && block_on(adev.query_imperfect()).is_ok()
                && block_on(adev.query_emit_pace()).is_ok()
                && block_on(adev.query_bearing()).is_ok()
                && block_on(adev.query_render()).is_ok();
            // async name: set then clear (back to the derived default)
            let aname_ok = adev.set_name("async box").is_ok() && adev.clear_name().is_ok();
            // async scale + bearing: the sync checks' behaviours from the async surface, read back
            // through the box.
            let _ = adev.reset();
            let _ = adev.scale(Axis::X, Direction::Both, 50);
            let _ = adev.scale_axis(Axis::Y, Direction::With, 60);
            let _ = adev.scale_all(Blanket::Wheel, Direction::Negative, 25);
            let _ = adev.set_bearing(Some(Duration::from_millis(35)), BearingMode::Vector);
            // Vector mode reports one relative scale, the lower, on both axes, so X's With reads Y's
            // 60, not its stored pass.
            let ascale_ok = matches!(block_on(adev.query_locks()), Ok(l)
                if l.scale_of(Axis::X, Direction::Positive) == 50
                    && l.scale_of(Axis::X, Direction::With) == 60
                    && l.scale_of(Axis::Y, Direction::With) == 60
                    && l.scale_of(Axis::Wheel, Direction::Negative) == 25);
            let abear_ok = matches!(block_on(adev.query_bearing()), Ok(b)
                if b.window == Some(Duration::from_millis(35)) && b.mode == BearingMode::Vector);
            let arel_ok = matches!(
                adev.lock(Button::LEFT, Direction::Against),
                Err(medius::Error::RelativeDirection { .. })
            );
            let _ = adev.set_bearing(Some(BEARING_WINDOW_DEFAULT), BearingMode::PerAxis);
            let _ = adev.reset();
            reset_motion(&acc);
            let _ = adev.move_rel(12, 0);
            std::thread::sleep(Duration::from_millis(200));
            let amoved = acc.rel_x.load(Ordering::Relaxed);
            let _ = adev.reset();
            // async observers and reconnect mirror the sync Device. Last, since reconnect() swaps the
            // serial transport and a reopen blip would disturb the checks above.
            let alog_n = adev.logs().try_iter().count();
            let arecon_base = adev.counters().reconnects;
            let arecon_ok = adev.reconnect().is_ok() && adev.counters().reconnects > arecon_base;
            check(
                "async",
                av_ok
                    && ah_ok
                    && aopt_ok
                    && aname_ok
                    && ascale_ok
                    && abear_ok
                    && arel_ok
                    && amoved == 12
                    && arecon_ok,
                format!(
                    "AsyncDevice: version_ok={av_ok}, health_ok={ah_ok}, option_queries_ok={aopt_ok}, name_ok={aname_ok}, scale_ok={ascale_ok}, bearing_ok={abear_ok}, relative_refused={arel_ok}, reconnect_ok={arecon_ok}, async_logs_drained={alog_n}, async move REL_X={amoved}"
                ),
            );
        }

        {
            let dev = device.as_ref().unwrap();
            let _ = dev.press(Button::MIDDLE);
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
                    let up0 = matches!(dev.query_version(), Ok(v) if v.proto_ver == PROTO_VER);
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
                            && matches!(dev.query_version(), Ok(v) if v.proto_ver == PROTO_VER)
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

        {
            // Device chip reboot under held state: the crate hears the hello, waits for the clone,
            // re-sends what it holds, and reports the clip ring gone.
            let reopened = match args.get(2) {
                Some(p) => Device::open(p),
                None => Device::find(),
            };
            match reopened {
                Ok(dev) => {
                    let clip = dev.clip();
                    let _ = dev.reset();
                    let trigger = ClipTrigger::new(Button::SIDE2, Edge::Press, ClipAction::Toggle);
                    let held = dev.scale(Axis::X, Direction::Both, 40).is_ok()
                        && clip.set_retain(true).is_ok()
                        && clip.bind(trigger).is_ok()
                        && {
                            let mut one = ClipBuilder::new();
                            one.move_by(1, 0);
                            clip.append(&one).is_ok()
                        };
                    let base = dev.counters().restarts;
                    let rebooted = dev.reboot(RebootTarget::DeviceRun).is_ok();
                    let deadline = Instant::now() + Duration::from_secs(20);
                    while dev.counters().restarts == base && Instant::now() < deadline {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    let recovered = dev.counters().restarts > base;
                    let scale = dev
                        .query_locks()
                        .map(|l| l.scale_of(Axis::X, Direction::Positive))
                        .unwrap_or(-1);
                    let config = clip.query_config().ok();
                    let retained = config.as_ref().is_some_and(|c| c.retain);
                    let bound = config.as_ref().is_some_and(|c| c.triggers == [trigger]);
                    let lost = clip.lost();
                    let _ = clip.clear_triggers();
                    let _ = clip.clear();
                    let _ = clip.set_retain(false);
                    let _ = dev.reset();
                    check(
                        "device-chip restart",
                        held && rebooted && recovered && scale == 40 && retained && bound && lost,
                        format!(
                            "recovered={recovered}, X scale back at {scale}, retain={retained}, \
                             trigger bound={bound}, clip reported lost={lost}"
                        ),
                    );
                }
                Err(e) => check("device-chip restart", false, format!("reopen failed: {e}")),
            }
        }

        {
            // Re-presenting the clone (a patch apply, then its clear) releases the session like a
            // replug, with no hello: the crate sees the PC enumerate the new clone and re-sends the
            // scale.
            let reopened = match args.get(2) {
                Some(p) => Device::open(p),
                None => Device::find(),
            };
            match reopened {
                Ok(dev) => {
                    let was = dev.query_imperfect().map(|i| i.allowed).unwrap_or(false);
                    let _ = dev.reset();
                    let held = dev.allow_imperfect_clones(true).is_ok()
                        && dev.scale(Axis::X, Direction::Both, 40).is_ok()
                        && dev
                            .set_patch(&Patch::new(PatchSection::Device, 12, vec![0x00, 0x01]))
                            .is_ok();
                    let settled = |applied: bool| {
                        let deadline = Instant::now() + Duration::from_secs(10);
                        while Instant::now() < deadline {
                            std::thread::sleep(Duration::from_millis(100));
                            let served = dev.query_patches().map(|p| p.applied).ok();
                            let scale = dev
                                .query_locks()
                                .map(|l| l.scale_of(Axis::X, Direction::Positive))
                                .unwrap_or(-1);
                            if served == Some(applied) && scale == 40 {
                                return true;
                            }
                        }
                        false
                    };
                    let applied = dev.apply_patch().is_ok() && settled(true);
                    let cleared = dev.clear_patch().is_ok() && settled(false);
                    let _ = dev.reset();
                    let _ = dev.allow_imperfect_clones(was);
                    check(
                        "re-presented clone",
                        held && applied && cleared,
                        format!(
                            "X scale back at 40 after the apply={applied}, after the clear={cleared}"
                        ),
                    );
                }
                Err(e) => check("re-presented clone", false, format!("reopen failed: {e}")),
            }
        }

        {
            // Erases the box's name, options and everything learned about every device seen, then
            // restarts it, so it runs only when named. It reopens its own handle (the crash-safe check
            // took the suite's). medius-fw's tools/validate_factory_reset.py covers the behaviour.
            if std::env::var("MEDIUS_HW_FACTORY_RESET").as_deref() == Ok("1") {
                let reopened = match args.get(2) {
                    Some(p) => Device::open(p),
                    None => Device::find(),
                };
                match reopened {
                    Ok(dev) => {
                        let named = dev.set_name("hw-full-factory").is_ok();
                        std::thread::sleep(Duration::from_millis(200));
                        let wiped = dev.factory_reset().is_ok();
                        // The CH343 stays enumerated while the chip reboots, so the box goes silent,
                        // not away: poll until a query replies.
                        let deadline = Instant::now() + Duration::from_secs(30);
                        let mut back = String::new();
                        while Instant::now() < deadline {
                            std::thread::sleep(Duration::from_millis(250));
                            if let Ok(v) = dev.query_version() {
                                back = v.name;
                                break;
                            }
                        }
                        check(
                            "factory reset",
                            named && wiped && !back.is_empty() && back != "hw-full-factory",
                            format!("name went to {back:?} and the box answered again"),
                        );
                    }
                    Err(e) => check("factory reset", false, format!("reopen failed: {e}")),
                }
            } else {
                check(
                    "factory reset",
                    true,
                    "skipped: erases this box's name, options and learned devices; \
                     set MEDIUS_HW_FACTORY_RESET=1 to run it"
                        .into(),
                );
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
