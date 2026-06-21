//! Open a real medius box, read its version/health, and exercise the core control surface.

use medius::{Button, Device};

fn main() -> medius::Result<()> {
    let device = match std::env::args().nth(1) {
        Some(path) => Device::open(path)?,
        None => Device::find()?,
    };

    let version = device.query_version()?;
    let health = device.query_health()?;
    println!("connected: {version}");
    println!(
        "health: link_up={} mouse_attached={} clone_configured={} injection_active={} rate_confident={}",
        health.link_up,
        health.mouse_attached,
        health.clone_configured,
        health.injection_active,
        health.rate_confident,
    );

    let info = device.query_mouse_info()?;
    let caps = device.query_caps()?;
    let rate = device.query_rate()?;
    println!("mouse: {info} (composite={})", caps.is_composite());
    println!(
        "caps: {} buttons, x={} y={} wheel={}",
        caps.n_buttons, caps.has_x, caps.has_y, caps.has_wheel,
    );
    println!(
        "rate: {:?} Hz (confident={})",
        rate.native_hz(),
        rate.confident
    );

    device.move_rel(40, 0)?;

    device.press(Button::Left)?;
    device.soft_release(Button::Left)?;

    device.reset()?;

    println!("counters: {:?}", device.counters());
    Ok(())
}
