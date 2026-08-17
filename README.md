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
medius = "1.6"
```

```rust
use medius::{Button, Device, Result};

fn main() -> Result<()> {
    let device = Device::find()?;             // auto-detect by USB VID/PID

    println!("{}", device.query_version()?);  // firmware version
    device.move_rel(100, -50)?;               // relative move
    device.press(Button::Left)?;
    device.release(Button::Left)?;
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
medius = { version = "3.0", features = ["async", "mock"] }
```

## API

### Connect

```rust
let device = Device::find()?;                 // first box by VID/PID (0x1A86:0x55D3)
let device = Device::open("/dev/ttyACM0")?;   // a specific port
```

`open`/`find` run a version handshake and reject a mismatched protocol.

### Multiple boxes

```rust
for b in Device::list() {                     // every connected box
    println!("{} {:?} {} {}", b.id(), b.name(), b.device.kind, b.device);  // MAC, name, kind, vid:pid + product
}
let m = Device::find_mouse_box()?;            // the box cloning a mouse
let k = Device::find_keyboard_box()?;         // the box cloning a keyboard
let d = Device::open_by_id("5a4e00111e28")?;  // by device MAC (or CH343 serial)
```

Each box's identity is its device-chip MAC; a reopened box reconnects to the same physical unit even after ports renumber.

### Mouse control

```rust
device.move_rel(100, -50)?;          // relative move (+x right, +y down)
device.wheel(3)?;                    // scroll

device.press(Button::Left)?;         // force down
device.release(Button::Left)?;  // release our press (a physical hold stays)
device.force_release(Button::Left)?; // force up, masking a physical hold
device.inject(Button::Right, Action::Press)?; // the generic form

device.reset()?;                     // clear all injection → passthrough
```

Buttons are `Left`, `Right`, `Middle`, `Side1`, `Side2`. Move and wheel take a full `i16`; the firmware clamps to the mouse's descriptor with carry, so `move_rel(2000, 0)` lands as exactly 2000.

### Keyboard & media

```rust
use medius::{Action, Key, MediaKey};

device.press(Key::A)?;              // hold a key (a modifier like Key::LEFT_SHIFT folds in)
device.release(Key::A)?;            // release our press (a physical hold stays)
device.force_release(Key::A)?;      // force up, masking a physical hold
device.inject(Key::ENTER, Action::Press)?; // the generic form

device.press(MediaKey::VOLUME_UP)?; // a media key by 16-bit Consumer usage
device.release(MediaKey::VOLUME_UP)?;
```

Keys are HID keycodes (`Key::A`, `Key::ENTER`, the eight modifiers, F-keys, arrows…) or any usage via `Key::new(0x04)`; media keys are Consumer usages (`MediaKey::VOLUME_UP`, `PLAY_PAUSE`, `MUTE`…). The tri-state `Action` (press / soft-release / force-release) is shared with buttons. Held keys and media survive a reconnect, like buttons. Both are present-gated: a key the board can't report is a silent no-op; see `caps()`.

### Sustained motion

You drive sustained motion yourself, one fire-and-forget `move_rel` per tick. The firmware merges additively with no halving and carries the remainder, so a tight 1 kHz loop lands the full distance (the box paces the emitted reports to the mouse's native report rate):

```rust
for _ in 0..1000 {
    device.move_rel(1, 0)?;
    std::thread::sleep(Duration::from_millis(1));
}
```

### Buffered clip playback

For jitter-free playback, preload per-frame input into a device-side ring and let the box drain one entry per native frame, box-clocked, so it carries none of the host's scheduling jitter and none of the per-command send floor. Motion is a per-frame delta, edges (buttons/keys/media) are sticky until changed, and a gap run emits nothing for N frames. Pace top-ups off `query_status().free`.

```rust
use medius::{ClipBuilder, Button};

let mut b = ClipBuilder::new();
for _ in 0..1000 { b.move_by(1, 0); }  // 1000 frames of +1 dx, box-timed
b.press(Button::Left).gap(20).release(Button::Left);

let clip = device.clip();
clip.append(&b)?;
clip.start()?;                          // or a trigger: clip.bind(ClipTrigger::new(Button::Side1, Edge::Press, ClipAction::Start))?
```

### Queries

```rust
let v = device.query_version()?;  // proto_ver + fw_major / fw_minor / fw_patch
let h = device.query_health()?;   // link_up, mouse_attached, clone_configured, injection_active, rate_confident, lock_on, catch_on, kbd_attached

let info = device.device_info()?;       // cloned device identity: vid:pid, bcd, flags, kind, product
let caps = device.caps()?;              // unified caps; caps.is_composite(), caps.mouse.n_buttons, caps.keyboard.nkro, caps.keyboard.has_consumer, caps.keyboard.n_keys
let rate = device.query_rate()?;        // live native report rate; rate.native_hz()
let stats = device.query_stats()?;      // delivery counters; stats.tx_drops / stats.tx_wedges
let locks = device.query_locks()?;      // active input locks; locks.is_locked(target, direction)
let catch = device.query_catch()?;      // the live catch table, its drop counts, the inter-chip clock
```

### Catch (observing what passes through the box)

Subscribe to what the box carries: the user's real input, the raw HID and vendor traffic either way,
proxied control transfers, the bytes the clone emits, and bus lifecycle. Input is reported *before*
any lock suppression or injection, so intercepting an input (lock it) and rebinding it (catch it) is
one loop. Dropping the stream unsubscribes.

For input, `input_events` decodes the box's held-usage snapshots into edges:

```rust
use medius::{CatchFilter, Input, Key};

for ev in device.input_events([CatchFilter::watch(Key::ESCAPE)])? {
    match ev.input {
        Input::Press(u) => println!("down {u:?}"),
        Input::Release(u) => println!("up {u:?}"),
        Input::Motion { dx, dy, dz } => println!("moved {dx},{dy},{dz}"),
    }
}
```

`CatchFilter::watch` takes what `lock` takes, so an input is addressed the same way in both. Use
`CatchFilter::all_input()` for every class, or `watch_class` / `watch_axis` to narrow.

For traffic, `catch_events` yields the raw frames. A `Capture` caps how much of each packet comes
back, which matters because a vendor bulk pipe at whole packets saturates the 4 Mbaud control link on
its own:

```rust
use medius::{Capture, CatchEvent, CatchFilter, TrafficClass};

let events = device.catch_events([
    CatchFilter::everything().with_capture(Capture::First(16)),
    CatchFilter::traffic(TrafficClass::VendorInterrupt, 0x83),   // this endpoint, whole packets
])?;
while let Ok(CatchEvent::Traffic(t)) = events.recv() {
    println!("{:?} 0x{:02X} {} bytes", t.class, t.id, t.true_len);
}
```

Both streams are bounded and lossy under back-pressure (`dropped()`), held alive by the keepalive,
and re-asserted across a reconnect. Under `async`, `recv_async().await`. `Timeline` puts a box stamp
on this machine's clock, unwrapping the 32-bit rollover and both chips' domains.

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
use medius::{Button, Device, FrameType, Health, MockBox, Rate, Version};

let mock = MockBox::new()
    .with_version(Version { proto_ver: 3, fw_major: 1, fw_minor: 2, fw_patch: 3, mac: [0; 6], name: "my-box".into() })
    .with_health(Health::from_flags(0x0F))
    .with_rate(Rate { native_period_us: 1000, poll_period_us: 1000, confident: true, change_driven: false });

let device = Device::with_mock(mock.clone());  // the real stack over a fake box

assert_eq!(device.query_version()?.fw_minor, 2);
assert_eq!(device.query_rate()?.native_hz(), Some(1000.0));
device.press(Button::Left)?;
assert!(mock.saw(FrameType::Inject));          // commands are recorded
```

## Examples

```bash
cargo run --example basic                   # minimal usage (needs a connected box)
cargo run --example hw_full --all-features   # on-hardware validation suite (Linux)
```

## Architecture

Four layers, `protocol → transport → link → device`, each depending only on the one below it. `protocol` is the wire codec: framed binary (SOF, type, rolling SEQ, length, payload, CRC16), no I/O. `transport` is the byte pipe (no `unsafe`) plus VID/PID discovery, over `serialport` everywhere except Windows, where `serial2`'s overlapped COM handle keeps a read and a write in flight at once. `link` runs the live connection: the reader thread, SEQ-correlated queries, keepalive, and reconnect. `device` is the typed API on top, where each command is one `link.send(...)`.

`Device` takes `&self`, is `Send + Sync`, and clones cheaply. The link runs at a fixed 4 Mbaud in framed binary (no baud dance, no ASCII REPL), and queries correlate by SEQ rather than arrival order. If the host goes quiet for ~1 s the firmware clears all injection, so a crash never leaves a button stuck; a keepalive thread keeps an intentionally-held button alive. Tested on Linux and Windows.

## Other languages

A C ABI (the `medius-capi` crate) exports the whole API for other languages. The
generated header compiles as C and C++, and a ctypes Python package rides on top.
See [`bindings/`](bindings/).

## License

MIT, see [LICENSE](LICENSE).
