//! Drive the 1 kHz frame pacer: push deltas, then drop the session.
//!
//! NEEDS A CONNECTED BOX TO RUN (compiles without hardware).
//!
//! The pacer clocks *frame emission* on an absolute-deadline clock; it does NOT humanize, smooth, or
//! invent intermediate motion. Each tick drains the accumulator and emits at most one MOVE (an
//! oversized burst splits across ticks at the i16 wire-field limit, total preserved exactly). The
//! firmware owns the real motion semantics.
//!
//!     cargo run --example paced
//!     cargo run --example paced -- /dev/ttyACM0

use std::thread::sleep;
use std::time::Duration;

use medius::Device;

fn main() -> medius::Result<()> {
    let device = match std::env::args().nth(1) {
        Some(path) => Device::open(path)?,
        None => Device::find()?,
    };

    // Spawns the `medius-pacer` real-time thread; dropping the session stops and joins it.
    let session = device.movement();
    println!("paced session running at {} Hz", session.rate());

    // Pushes accumulate and pace out one MOVE per tick; pushes within a tick coalesce into their sum.
    for _ in 0..200 {
        session.push(2, 0);
        sleep(Duration::from_millis(1));
    }

    // Sustained motion is just a push loop paced at ~1 kHz: feed (1, -1) each tick.
    for _ in 0..100 {
        session.push(1, -1);
        sleep(Duration::from_millis(1));
    }

    // Drop stops the thread; residual deltas are not force-flushed.
    drop(session);
    device.reset()?;
    Ok(())
}
