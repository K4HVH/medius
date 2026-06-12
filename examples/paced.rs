//! Drive the 1 kHz frame pacer: push deltas and set a constant velocity, then drop the session.
//!
//! THIS EXAMPLE NEEDS A CONNECTED BOX TO RUN (it opens a real device). It compiles without hardware.
//!
//! The pacer clocks *frame emission* on a precise absolute-deadline clock — it does NOT humanize,
//! smooth, ease, or invent intermediate motion. Each tick it drains the shared accumulator and emits
//! at most one MOVE; an oversized burst is split across ticks at the i16 wire-field limit (the total
//! is preserved exactly). The firmware owns the real motion semantics.
//!
//! Run:
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

    // Open a paced session at the default rate (1 kHz). This spawns the dedicated `medius-pacer`
    // real-time thread; dropping the session stops and joins it.
    let session = device.movement();
    println!("paced session running at {} Hz", session.rate());

    // Push relative deltas: they accumulate and are paced out one MOVE per tick. Many pushes inside a
    // single tick window coalesce into one MOVE of their sum.
    for _ in 0..200 {
        session.push(2, 0);
        sleep(Duration::from_millis(1));
    }

    // Velocity mode: a constant (vx, vy) emitted EVERY tick until changed or cleared. It combines
    // additively with pushes through the same wire-field carry.
    session.set_velocity(1, -1);
    sleep(Duration::from_millis(100));
    session.clear_velocity();

    // Drop stops the pacer thread (residual deltas are not force-flushed — fire-and-go).
    drop(session);
    device.reset()?;
    Ok(())
}
