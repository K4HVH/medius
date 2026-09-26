//! Lists every connected box, then opens one by device kind and by id.

fn main() -> medius::Result<()> {
    let boxes = medius::Device::list();
    if boxes.is_empty() {
        println!("no medius boxes found");
        return Ok(());
    }
    for b in &boxes {
        let clone = match &b.device {
            Some(d) => format!("{:<8} {d}", d.kind),
            None => format!(
                "protocol {}, this build speaks {}: update the box",
                b.version.proto_ver,
                medius::PROTO_VER
            ),
        };
        println!(
            "{}  name={:<16} {:<16} serial={:<12} {clone}  ({})",
            b.id(),
            b.name(),
            b.port.path,
            b.serial().unwrap_or("-"),
            b.version,
        );
    }

    match medius::Device::find_mouse_box() {
        Ok(m) => println!("find_mouse_box    -> {}", m.device_info()?),
        Err(e) => println!("find_mouse_box    -> {e}"),
    }
    match medius::Device::find_keyboard_box() {
        Ok(k) => println!("find_keyboard_box -> {}", k.device_info()?),
        Err(e) => println!("find_keyboard_box -> {e}"),
    }

    let id = boxes[0].id();
    let d = medius::Device::open_by_id(&id)?;
    println!("open_by_id({id}) -> {}", d.query_version()?);
    Ok(())
}
