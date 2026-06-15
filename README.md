# medius

[![Crates.io](https://img.shields.io/crates/v/medius)](https://crates.io/crates/medius)
[![Docs](https://img.shields.io/badge/docs-medius.k4tech.net-blue)](https://medius.k4tech.net)
[![CI](https://img.shields.io/github/actions/workflow/status/K4HVH/medius/ci.yml?label=CI)](https://github.com/K4HVH/medius/actions)
[![License](https://img.shields.io/crates/l/medius)](./LICENSE)
[![Discord](https://img.shields.io/badge/discord-firmware-5865F2?logo=discord&logoColor=white)](https://discord.gg/ArRqcA84pB)

Custom firmware for MAKCU mouse-passthrough boxes, and the Rust library that drives it.

A MAKCU box sits inline between a mouse and a PC: the real mouse passes through to the PC while a control program injects movement, buttons, and scroll over USB-serial. medius replaces the stock firmware with a clean binary protocol; this crate binds its commands 1:1 and adds what you need to run the box reliably (handshake, keepalive, reconnect). Each call sends one firmware frame.

Flash and test a box from your browser at **[medius.k4tech.net/dashboard](https://medius.k4tech.net/dashboard)**: no drivers, nothing to install. Full documentation is at **[medius.k4tech.net](https://medius.k4tech.net)**.

## Why medius vs stock firmware

Same MAKCU box, different firmware. Both clone your mouse's USB descriptor byte for byte, since that's the hardware. What changes is how the firmware behaves:

| | medius | stock MAKCU |
|---|---|---|
| **Your motion** | Injection **adds** to your real movement. Both go through, nothing lost. | Injection **overwrites** it. At 1 kHz your real motion never arrives. |
| **Detection** | Measured against the native mouse and matched: timing, control values, USB conformance. | Copies the descriptor; no published native-behaviour audit. |
| **Reliability** | Clears all injection after 1 s of host silence, so a crashed controller never leaves a button held. | No silence release documented; a forced button stays held until you clear it. |
| **Link** | Binary frames with CRC and request IDs, at a fixed baud. | An ASCII command prompt, replies matched by arrival order, behind a baud handshake that doesn't persist a power cycle. |

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
| `async`   | `AsyncDevice`, async queries over the same core, runtime-agnostic (no tokio) |
| `mock`    | `MockBox`, an in-process fake box for tests without hardware |
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

Buttons are `Left`, `Right`, `Middle`, `Side1`, `Side2`. Move and wheel take a full `i16`; the firmware clamps to the mouse's descriptor with carry, so `move_rel(2000, 0)` lands as exactly 2000.

### Sustained motion

You drive sustained motion yourself, one fire-and-forget `move_rel` per tick. The firmware merges additively with no halving, so a tight 1 kHz loop lands at full rate:

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

The async wrapper is the same core. Only queries await; the fire-and-forget commands are identical:

```rust
let device = Device::find()?.into_async();
device.move_rel(10, 0)?;                // instant, not async
let v = device.query_version().await?;  // awaits the correlated reply
```

It uses `flume`'s async recv, so there's no runtime dependency and it runs on any executor.

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

Four layers, `protocol → transport → link → device`, each depending only on the one below it. `protocol` is the wire codec: framed binary (SOF, type, rolling SEQ, length, payload, CRC16), no I/O. `transport` is the byte pipe over the `serialport` crate (cross-platform, no `unsafe`) plus VID/PID discovery. `link` runs the live connection: the reader thread, SEQ-correlated queries, keepalive, and reconnect. `device` is the typed API on top, where each command is one `link.send(...)`.

`Device` takes `&self`, is `Send + Sync`, and clones cheaply. The link runs at a fixed 4 Mbaud in framed binary (no baud dance, no ASCII REPL), and queries correlate by SEQ rather than arrival order. If the host goes quiet for ~1 s the firmware clears all injection, so a crash never leaves a button stuck; a keepalive thread keeps an intentionally-held button alive. Tested on Linux and Windows.

See [`docs/architecture.md`](docs/architecture.md) for the deeper design notes.

## License

MIT, see [LICENSE](LICENSE).
