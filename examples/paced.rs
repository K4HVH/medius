//! Drive the 1 kHz frame pacer: push deltas and set a constant velocity, then drop the session.
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

    // Velocity mode: a constant (vx, vy) emitted every tick until cleared. Combines additively with
    // pushes through the same wire-field carry.
    session.set_velocity(1, -1);
    sleep(Duration::from_millis(100));
    session.clear_velocity();

    // Drop stops the thread; residual deltas are not force-flushed.
    drop(session);
    device.reset()?;
    Ok(())
}
