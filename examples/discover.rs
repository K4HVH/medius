//! Enumerate every connected box, then open one by device kind and by id.

fn main() -> medius::Result<()> {
    let boxes = medius::Device::list();
    if boxes.is_empty() {
        println!("no medius boxes found");
        return Ok(());
    }
    for b in &boxes {
        println!(
            "{}  {:<16} serial={:<12} {:<8} {}  ({})",
            b.id(),
            b.port.path,
            b.serial().unwrap_or("-"),
            b.device.kind,
            b.device,
            b.version,
        );
    }

    if let Ok(m) = medius::Device::find_mouse_box() {
        println!("find_mouse_box    -> {}", m.device_info()?);
    }
    if let Ok(k) = medius::Device::find_keyboard_box() {
        println!("find_keyboard_box -> {}", k.device_info()?);
    }

    let id = boxes[0].id();
    let d = medius::Device::open_by_id(&id)?;
    println!("open_by_id({id}) -> {}", d.query_version()?);
    Ok(())
}
