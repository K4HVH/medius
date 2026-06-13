# medius

Host control library for the **medius** transparent mouse passthrough box — the compiled control
plane for a box whose firmware the project owns. It speaks a clean framed binary protocol over the
device-chip USB-serial link, binds the firmware's command primitives 1:1 — a small fire-and-go
control surface (`move`, `wheel`, button press/release, `reset`) plus the two SEQ-correlated queries
(`version`, `health`) — and adds the infrastructure to drive the box reliably (handshake, keepalive,
reconnect). It is the production replacement for the C and Python reference clients.

## Transparent control + injection — NOT a humanizer or smoother

**`medius` does not smooth, humanize, ease, interpolate, pace, or synthesize any fake mouse
behaviour.** It is a transparent, precise control + injection layer: each method binds exactly one
firmware frame. The firmware guarantees additive "no-halving" merge and descriptor-faithful
carry-remainder clamping (a `MOVE 2000` lands as exactly 2000), and the firmware sustains 1 kHz
injection — the host just emits `MOVE` frames as fast as the caller drives them. The library never
invents trajectory and never clocks motion for you. If you want pacing or humanization, that is the
host application's job — build it on top, not in here.

## Feature flags

| Flag       | Default | What it adds                                                              |
| ---------- | :-----: | ------------------------------------------------------------------------- |
| (none)     |   ✓     | Sync `Device`: 1:1 frame commands, queries, logs, keepalive, reconnect.   |
| `async`    |         | `AsyncDevice` — a thin wrapper over the **same** sync core; `async` queries, runtime-agnostic timeout, no tokio dep. |
| `mock`     |         | `MockBox` — a public scriptable fake box for hardware-free tests.         |
| `flash`    |         | `esptool` reboot-to-download + flash handoff.                             |
| `tracing`  |         | Library-side `tracing` instrumentation (per-frame TX/RX at TRACE only).   |
| `serde`    |         | `Serialize`/`Deserialize` derives on the public value types + `ConnectOptions`. |

## Quick start

Add it to `Cargo.toml`:

```toml
[dependencies]
medius = "0.1"
```

### Sync: open, move, query

```rust,no_run
use medius::{Button, Device};

fn main() -> medius::Result<()> {
    // Auto-discover the first box by VID/PID (or `Device::open("/dev/ttyACM0")`).
    let device = Device::find()?;

    println!("{}", device.query_version()?);     // e.g. "fw 1.2.3"
    let health = device.query_health()?;
    println!("injection active: {}", health.injection_active);

    device.move_rel(40, 0)?;                       // +dx right, +dy down (fire-and-go)
    device.press(Button::Left)?;                   // primitive press …
    device.soft_release(Button::Left)?;            // … then soft-release
    device.reset()?;                               // back to pure passthrough
    Ok(())
}
```

### Sustained 1 kHz motion (caller-driven)

```rust,no_run
use std::{thread::sleep, time::Duration};
use medius::Device;

fn main() -> medius::Result<()> {
    let device = Device::find()?;

    // Sustained motion is just a caller-driven loop: one fire-and-go MOVE per tick. The firmware
    // merges additively with no halving, so a tight 1 kHz loop lands as full-rate motion. The library
    // does NOT pace, smooth, or invent motion — you own the timing (use your own real-time loop).
    for _ in 0..200 {
        device.move_rel(2, 0)?;
        sleep(Duration::from_millis(1));
    }
    Ok(())
}
```

### Hardware-free test with the `mock` feature

```rust
use medius::mock::MockBox;
use medius::protocol::FrameType;
use medius::{Button, Device, Health, Version};

let mock = MockBox::new()
    .with_version(Version { proto_ver: 1, fw_major: 2, fw_minor: 3, fw_patch: 4 })
    .with_health(Health::from_flags(0x0F));

let device = Device::with_mock(mock.clone());       // the real device stack over a fake box

let v = device.query_version().unwrap();
assert_eq!((v.fw_major, v.fw_minor, v.fw_patch), (2, 3, 4));

device.press(Button::Left).unwrap();
assert!(mock.saw(FrameType::Button));               // the command was recorded
```

## Examples

Runnable examples live in [`examples/`](examples/):

- `basic` — compiles everywhere, but needs a **connected box** to run
  (`cargo run --example basic`).
- `mock` — fully **hardware-free** (`cargo run --example mock --features mock`).
- `async` — hardware-free over the mock box on any executor
  (`cargo run --example async --features async,mock`).

## How it differs from the makcu library

We **own the firmware** on this box, so the host library is clean by construction rather than working
around a black-box device:

- **A clean framed binary protocol** — length-delimited frames with a type, a rolling `SEQ`, and a
  payload. No ASCII REPL, no text parsing.
- **No baud dance** — one fixed raw serial config (4 Mbaud); no magic-baud command channel to enter a
  control mode.
- **No positional response queue** — queries are correlated by `SEQ` (each `RESP` echoes its request's
  `SEQ`), so replies are matched explicitly and two in-flight queries never cross-deliver. makcu
  relied on response ordering.
- **No artificial wheel cap** — `wheel` takes a full `i16`; the firmware clamps to the clone's native
  descriptor field with carry. The library imposes no arbitrary limits (shaping is the host app's job).
- **Fire-and-go hot path** — commands return the instant their bytes are flushed; no per-command ACK,
  no `spawn_blocking`-per-command thread. The `async` wrapper is the *same* core, not a second
  transport.
- **1 kHz injection lives in firmware** — the firmware does the additive no-halving merge, so the host
  stays a thin 1:1 frame binding. There is no host-side pacer/smoother; the caller drives MOVE timing.

## Platform notes

Supported on **Linux** and **Windows**, talking raw serial to the box's CH343 (WCH) USB-serial bridge
at **4 Mbaud**. Port discovery is by USB VID/PID (`0x1A86` / `0x55D3`); reconnect rescans by VID/PID
rather than a fixed path, so a re-enumerated device is found again. No `serialport` crate dependency —
the transport is a thin platform FFI layer (`libc` termios2 on Linux, `windows-sys` on Windows).

## Design references

- Library design spec: `docs/superpowers/specs/2026-06-13-medius-rust-library-design.md` (firmware repo).
- Byte-exact wire reference: `docs/protocol/control-protocol.md` (firmware repo).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.
