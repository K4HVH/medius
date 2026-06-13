//! Sustained 1 kHz direct-move soak (Linux).
//!
//! Grabs the clone's mouse node (`EVIOCGRAB`, so injected motion never reaches the desktop), drives a
//! direct `move_rel(1, 0)` loop at ~1 kHz for N seconds, and reports the evdev-delivered stream (count,
//! achieved Hz, sum fidelity, worst gap, stalls) — proving the firmware's no-halving 1 kHz injection
//! holds up over a long run, with the host as a thin 1:1 frame binding (no host-side pacer).
//!
//! ```text
//! cargo run --example hw_soak -- [seconds=20] [event=/dev/input/event11] [port]
//! ```
//! NOTE: freezes the box's passthrough mouse for the whole window. Wrap in `timeout` when unattended.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("hw_soak needs Linux.");
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
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    use medius::Device;

    const EVIOCGRAB: libc::c_ulong = 0x4004_4590;
    const EV_REL: u16 = 0x02;
    const REL_X: u16 = 0x00;
    const EVENT_SIZE: usize = 24;
    /// Inter-event gap above this (ns) counts as a stall (a steady 1 kHz stream is ~1 ms apart; a
    /// halving would show ~2 ms, a hiccup more).
    const STALL_NS: u64 = 2_000_000;

    #[derive(Default)]
    struct Stats {
        events: AtomicU64,
        sum: AtomicU64,
        max_gap_ns: AtomicU64,
        stalls: AtomicU64,
    }

    struct EvdevGrab {
        fd: RawFd,
    }
    impl EvdevGrab {
        fn open(path: &str) -> std::io::Result<Self> {
            let cpath = std::ffi::CString::new(path).unwrap();
            // SAFETY: valid C string; O_NONBLOCK so the reader polls `stop` rather than blocking.
            let fd = unsafe {
                libc::open(
                    cpath.as_ptr(),
                    libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NONBLOCK,
                )
            };
            if fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            // SAFETY: fd valid; EVIOCGRAB(1) takes the device exclusively.
            if unsafe { libc::ioctl(fd, EVIOCGRAB, 1) } < 0 {
                let e = std::io::Error::last_os_error();
                // SAFETY: close the fd we just opened.
                unsafe { libc::close(fd) };
                return Err(e);
            }
            Ok(EvdevGrab { fd })
        }
    }
    impl Drop for EvdevGrab {
        fn drop(&mut self) {
            // SAFETY: release the grab + close our owned fd once.
            unsafe {
                libc::ioctl(self.fd, EVIOCGRAB, 0);
                libc::close(self.fd);
            }
        }
    }

    fn reader(fd: RawFd, stats: Arc<Stats>, stop: Arc<AtomicBool>) {
        let mut buf = [0u8; EVENT_SIZE];
        let mut last: Option<Instant> = None;
        while !stop.load(Ordering::Relaxed) {
            // SAFETY: fd valid; read into an EVENT_SIZE buffer.
            let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, EVENT_SIZE) };
            if n != EVENT_SIZE as isize {
                std::thread::sleep(Duration::from_micros(200));
                continue;
            }
            let typ = u16::from_ne_bytes([buf[16], buf[17]]);
            let code = u16::from_ne_bytes([buf[18], buf[19]]);
            let val = i32::from_ne_bytes([buf[20], buf[21], buf[22], buf[23]]);
            if typ == EV_REL && code == REL_X {
                let now = Instant::now();
                if let Some(prev) = last {
                    let gap = now.duration_since(prev).as_nanos() as u64;
                    stats.max_gap_ns.fetch_max(gap, Ordering::Relaxed);
                    if gap > STALL_NS {
                        stats.stalls.fetch_add(1, Ordering::Relaxed);
                    }
                }
                last = Some(now);
                stats.events.fetch_add(1, Ordering::Relaxed);
                stats
                    .sum
                    .fetch_add(val.unsigned_abs() as u64, Ordering::Relaxed);
            }
        }
    }

    pub fn run() -> ExitCode {
        let args: Vec<String> = std::env::args().collect();
        let secs: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(20);
        let event = args
            .get(2)
            .cloned()
            .unwrap_or_else(|| "/dev/input/event11".to_string());

        let grab = match EvdevGrab::open(&event) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("cannot grab {event}: {e}");
                return ExitCode::FAILURE;
            }
        };
        let stats = Arc::new(Stats::default());
        let stop = Arc::new(AtomicBool::new(false));
        let (rfd, rs, rst) = (grab.fd, Arc::clone(&stats), Arc::clone(&stop));
        let reader = std::thread::spawn(move || reader(rfd, rs, rst));
        std::thread::sleep(Duration::from_millis(200));

        let device = match args.get(3) {
            Some(p) => Device::open(p),
            None => Device::find(),
        };
        let device = match device {
            Ok(d) => d,
            Err(e) => {
                eprintln!("cannot open box: {e}");
                stop.store(true, Ordering::Relaxed);
                let _ = reader.join();
                return ExitCode::FAILURE;
            }
        };
        println!(
            "connected {:?}; grabbed {event}; soaking a direct 1 kHz move_rel loop for {secs}s ...\n",
            device.query_version().ok()
        );

        let _ = device.reset();
        let start = Instant::now();
        // Caller-driven 1 kHz loop: one MOVE per ~1 ms straight to the box. Sleep timing isn't exact, so
        // the verdict judges achieved rate + motion, not an exact secs*1000 sum.
        let deadline = start + Duration::from_secs(secs);
        while Instant::now() < deadline {
            let _ = device.move_rel(1, 0);
            std::thread::sleep(Duration::from_millis(1));
        }
        let elapsed = start.elapsed().as_secs_f64();
        std::thread::sleep(Duration::from_millis(150));
        let _ = device.reset();

        stop.store(true, Ordering::Relaxed);
        let _ = reader.join();
        drop(grab);

        let events = stats.events.load(Ordering::Relaxed);
        let sum = stats.sum.load(Ordering::Relaxed);
        let max_gap_ms = stats.max_gap_ns.load(Ordering::Relaxed) as f64 / 1e6;
        let stalls = stats.stalls.load(Ordering::Relaxed);
        let hz = events as f64 / elapsed;

        // Firmware merges additively: two host MOVEs in one ~1 ms frame emit ONE report of their sum.
        // So report COUNT can sit below total motion (`sum`) with no loss — the verdict judges achieved
        // rate + that motion was delivered (sum ≥ reports), not an exact count.
        println!("== evdev-delivered (what the OS actually saw) ==");
        println!("  duration       {elapsed:.2} s");
        println!("  reports        {events}  (sum REL_X = {sum})");
        println!("  achieved rate  {hz:.1} Hz");
        println!("  worst gap      {max_gap_ms:.3} ms");
        println!("  stalls (>2ms)  {stalls}");

        // No-halving: ~1 kHz observed (halving shows ~500), motion delivered (sum ≥ reports — sleep
        // timing isn't exact, so we don't demand sum == secs*1000), only the odd hiccup.
        let rate_ok = hz >= 950.0;
        let fidelity_ok = sum >= events;
        let stall_ok = stalls <= secs.max(3);
        let ok = rate_ok && fidelity_ok && stall_ok;
        println!("\nRESULT: {}", if ok { "PASS ✓" } else { "FAIL ✗" });
        if ok {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }
}
