//! Open a real medius box, read its version/health, and exercise the core control surface.
//!
//! NEEDS A CONNECTED BOX TO RUN (compiles without hardware). With no box, `Device::find()` returns
//! `Error::NotFound` and the program exits early.
//!
//!     cargo run --example basic                      # auto-discover
//!     cargo run --example basic -- /dev/ttyACM0      # Linux
//!     cargo run --example basic -- COM7              # Windows

use std::time::Duration;

use medius::{Button, Device};

fn main() -> medius::Result<()> {
    // Open the given port, or auto-discover. Both run the version handshake before returning.
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

    // +dx right, +dy down. Fire-and-go.
    device.move_rel(40, 0)?;

    // Host-composed press/hold/soft-release; blocks for `hold`.
    device.click(Button::Left, Duration::from_millis(40))?;

    // Back to pure passthrough.
    device.reset()?;

    println!("counters: {:?}", device.counters());
    Ok(())
}
