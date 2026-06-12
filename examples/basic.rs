//! Open a real medius box, read its version/health, and exercise the core control surface.
//!
//! THIS EXAMPLE NEEDS A CONNECTED BOX TO RUN. It compiles without hardware (`cargo build
//! --examples`), but actually running it requires a medius box plugged in over its CH343 USB-serial
//! link. With no box present, `Device::find()` returns `Error::NotFound` and the program exits early.
//!
//! Run with an auto-discovered box:
//!     cargo run --example basic
//! Or point it at an explicit serial port:
//!     cargo run --example basic -- /dev/ttyACM0      # Linux
//!     cargo run --example basic -- COM7              # Windows

use std::time::Duration;

use medius::{Button, Device};

fn main() -> medius::Result<()> {
    // Either open the port given on the command line, or auto-discover the first box by VID/PID.
    // Both run the version handshake (QUERY(VERSION) + proto-version check) before returning.
    let device = match std::env::args().nth(1) {
        Some(path) => Device::open(path)?,
        None => Device::find()?,
    };

    // Queries are the only request/response exchange; everything else is fire-and-go.
    let version = device.query_version()?;
    let health = device.query_health()?;
    println!("connected: {version}");
    println!(
        "health: link_up={} mouse_attached={} clone_configured={} injection_active={}",
        health.link_up, health.mouse_attached, health.clone_configured, health.injection_active,
    );

    // Relative move: +dx right, +dy down. Fire-and-go (no ACK, returns once the bytes are flushed).
    device.move_rel(40, 0)?;

    // A host-composed click: press, hold, soft-release. Blocks the calling thread for `hold`.
    device.click(Button::Left, Duration::from_millis(40))?;

    // Return to pure passthrough (clear every injection override).
    device.reset()?;

    // A snapshot of the always-on counters (frames sent, reconnects, etc.).
    println!("counters: {:?}", device.counters());
    Ok(())
}
