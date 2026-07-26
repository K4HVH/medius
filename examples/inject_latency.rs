//! Measure how long `move_rel` blocks the calling thread, so a host-side write stall shows up as a
//! number instead of a feeling. Reports the percentiles, every call over the stall threshold, and
//! the share of wall clock spent inside the library.

use std::time::{Duration, Instant};

use medius::Device;

const BURST: usize = 400;
const PACED_SECS: u64 = 5;
const PACE: Duration = Duration::from_micros(1000);
const STALL: Duration = Duration::from_millis(5);

fn main() -> medius::Result<()> {
    let device = match std::env::args().nth(1) {
        Some(path) => Device::open(path)?,
        None => Device::find()?,
    };
    println!("connected: {}", device.query_version()?);

    let burst = {
        let start = Instant::now();
        let mut calls = Vec::with_capacity(BURST);
        for i in 0..BURST {
            let dx = if i % 2 == 0 { 1 } else { -1 };
            let t0 = Instant::now();
            device.move_rel(dx, 0)?;
            calls.push(t0.elapsed());
        }
        (calls, start.elapsed())
    };
    report("burst (unpaced)", &burst.0, burst.1);

    let paced = {
        let deadline = Duration::from_secs(PACED_SECS);
        let start = Instant::now();
        let mut calls = Vec::new();
        let mut next = start;
        let mut i = 0usize;
        while start.elapsed() < deadline {
            let dx = if i % 2 == 0 { 1 } else { -1 };
            let t0 = Instant::now();
            device.move_rel(dx, 0)?;
            calls.push(t0.elapsed());
            i += 1;
            next += PACE;
            if let Some(idle) = next.checked_duration_since(Instant::now()) {
                std::thread::sleep(idle);
            }
        }
        (calls, start.elapsed())
    };
    report("paced (1 kHz)", &paced.0, paced.1);

    println!("counters: {:?}", device.counters());
    Ok(())
}

fn report(label: &str, calls: &[Duration], wall: Duration) {
    let mut sorted = calls.to_vec();
    sorted.sort_unstable();
    let blocked: Duration = calls.iter().sum();
    let stalls: Vec<_> = calls
        .iter()
        .enumerate()
        .filter(|(_, d)| **d >= STALL)
        .collect();

    println!("\n== {label}: {} calls in {:.1?}", calls.len(), wall);
    println!(
        "   p50 {:.1?}  p99 {:.1?}  max {:.1?}",
        pct(&sorted, 50),
        pct(&sorted, 99),
        sorted.last().copied().unwrap_or_default(),
    );
    println!(
        "   blocked {:.1}% of wall clock, {} calls over {:.0?}",
        blocked.as_secs_f64() / wall.as_secs_f64() * 100.0,
        stalls.len(),
        STALL,
    );
    for (i, d) in stalls.iter().take(10) {
        println!("   stall at call {i}: {d:.1?}");
    }
    if stalls.len() > 10 {
        println!("   ... {} more", stalls.len() - 10);
    }
}

fn pct(sorted: &[Duration], p: usize) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    sorted[(sorted.len() - 1) * p / 100]
}
