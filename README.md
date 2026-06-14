# medius

A Rust library for controlling **medius** mouse-passthrough boxes.

A medius box sits inline between a mouse and a PC: the real mouse passes through transparently while a control PC injects movement, buttons, and scroll over USB-serial. This crate is the host-side control plane — a 1:1 binding of the firmware's commands plus the infrastructure to drive the box reliably (handshake, keepalive, reconnect). It does not smooth, pace, or synthesize input; each method sends exactly one firmware frame.

## Quick start

```toml
[dependencies]
medius = "0.1"
```

```rust
use medius::{Button, Device, Result};

fn main() -> Result<()> {
    let device = Device::find()?;             // auto-detect by USB VID/PID

    println!("{}", device.query_version()?);  // firmware version
    device.move_rel(100, -50)?;               // relative move
    device.press(Button::Left)?;
    device.soft_release(Button::Left)?;
    device.wheel(-3)?;
    device.reset()?;                          // back to pure passthrough
    Ok(())
}
```

## Features

The base crate is the lean sync core. Optional features:

| Feature   | Description |
|-----------|-------------|
| `async`   | `AsyncDevice` — async queries over the same core, runtime-agnostic (no tokio) |
| `mock`    | `MockBox` — in-process fake box for tests without hardware |
| `flash`   | `esptool` reboot-to-download + firmware flash handoff |
| `tracing` | per-frame TX/RX `tracing` instrumentation |

```toml
medius = { version = "0.1", features = ["async", "mock"] }
```

## API

### Connect

```rust
let device = Device::find()?;                 // first box by VID/PID (0x1A86:0x55D3)
let device = Device::open("/dev/ttyACM0")?;   // a specific port
```

`open`/`find` run a version handshake and reject a mismatched protocol.

### Mouse control

```rust
device.move_rel(100, -50)?;          // relative move (+x right, +y down)
device.wheel(3)?;                    // scroll

device.press(Button::Left)?;         // force down
device.soft_release(Button::Left)?;  // release our press (a physical hold stays)
device.force_release(Button::Left)?; // force up, masking a physical hold
device.button(Button::Right, ButtonAction::Press)?; // the generic form

device.reset()?;                     // clear all injection → passthrough
```

Buttons are `Left`, `Right`, `Middle`, `Side1`, `Side2`. Move/wheel take a full `i16` with no artificial caps — the firmware clamps to the mouse's descriptor with carry, so `move_rel(2000, 0)` lands as exactly 2000.

### Sustained motion

There is no host-side pacer. Sustained motion is a caller-driven loop — one fire-and-go `move_rel` per tick. The firmware merges additively with no halving, so a tight 1 kHz loop lands at full rate:

```rust
for _ in 0..1000 {
    device.move_rel(1, 0)?;
    std::thread::sleep(Duration::from_millis(1));
}
```

### Queries

```rust
let v = device.query_version()?;  // proto_ver + fw_major / fw_minor / fw_patch
let h = device.query_health()?;   // link_up, mouse_attached, clone_configured, injection_active
```

### Box management

```rust
device.reboot(RebootTarget::DeviceRun)?;  // restart a chip (run / ROM-download × device / host)
device.reconnect()?;                      // rescan VID/PID, reopen, re-assert held state
device.reapply()?;                        // re-send currently-held overrides on demand
```

The reader also reconnects on its own if the link drops.

### Observability

```rust
for line in device.logs() {       // device LOG stream
    println!("[{:?}] {}", line.level, line.text);
}

let c = device.counters();        // frames_tx / frames_rx / crc_drops / reconnects
```

### Async (feature = `async`)

The async wrapper is the same core — only queries await; fire-and-go commands are identical:

```rust
let device = Device::find()?.into_async();
device.move_rel(10, 0)?;                // instant, not async
let v = device.query_version().await?;  // awaits the correlated reply
```

It uses `flume`'s async recv, so there is no runtime dependency — it runs on any executor.

### Mock (feature = `mock`)

```rust
use medius::{Button, Device, FrameType, Health, MockBox, Version};

let mock = MockBox::new()
    .with_version(Version { proto_ver: 1, fw_major: 1, fw_minor: 2, fw_patch: 3 })
    .with_health(Health::from_flags(0x0F));

let device = Device::with_mock(mock.clone());  // the real stack over a fake box

assert_eq!(device.query_version()?.fw_minor, 2);
device.press(Button::Left)?;
assert!(mock.saw(FrameType::Button));          // commands are recorded
```

## Examples

```bash
cargo run --example basic                   # minimal usage (needs a connected box)
cargo run --example hw_full --all-features   # on-hardware validation suite (Linux)
```

## Architecture

Four layers, `protocol → transport → link → device`:

- **`protocol`** — pure, no-I/O wire codec (framed binary: SOF, type, rolling SEQ, length, payload, CRC16).
- **`transport`** — the byte pipe (`serialport` crate, cross-platform, no `unsafe`) and VID/PID discovery.
- **`link`** — the connection engine: a reader thread, SEQ-correlated queries, keepalive, reconnect.
- **`device`** — the thin typed API (`Device` / `AsyncDevice`), one `link.send(...)` per command.

`Device` is `&self`-only, `Send + Sync`, and cheap to clone. The link is a fixed 4 Mbaud framed-binary connection (no baud dance, no ASCII REPL); queries correlate by SEQ, not response order. The firmware clears all injection after ~1 s of host silence, so a crash never leaves a button stuck — the keepalive thread keeps an intentionally-held button alive. Tested on **Linux** and **Windows**.

See [`docs/architecture.md`](docs/architecture.md) for the deeper design notes.

## License

MIT — see [LICENSE](LICENSE).
