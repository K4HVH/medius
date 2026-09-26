# medius

[![Crates.io](https://img.shields.io/crates/v/medius)](https://crates.io/crates/medius)
[![Docs](https://img.shields.io/badge/docs-medius.k4tech.net-blue)](https://medius.k4tech.net)
[![CI](https://img.shields.io/github/actions/workflow/status/K4HVH/medius/ci.yml?label=CI)](https://github.com/K4HVH/medius/actions)
[![License](https://img.shields.io/crates/l/medius)](./LICENSE)
[![Discord](https://img.shields.io/badge/discord-firmware-5865F2?logo=discord&logoColor=white)](https://discord.gg/ArRqcA84pB)

Custom firmware for MAKCU mouse-passthrough boxes, and the Rust library that drives it.

A MAKCU box sits between a mouse and a PC: the mouse passes through while a control program injects movement, buttons and scroll over USB-serial. medius replaces the stock firmware with a binary protocol; this crate binds its commands 1:1 (one call, one frame) and adds handshake, keepalive and reconnect.

Flash and test a box in the browser, with no drivers or install: **[medius.k4tech.net/dashboard](https://medius.k4tech.net/dashboard)**. Docs: **[medius.k4tech.net](https://medius.k4tech.net)**.

## medius vs stock firmware

Same box, different firmware. Both clone the mouse's USB descriptor byte for byte, since that's the hardware. The differences:

| | medius | stock MAKCU |
|---|---|---|
| **Your motion** | Injection **adds** to physical motion; both arrive. | Injection **overwrites** it; at 1 kHz physical motion never arrives. |
| **Detection** | Timing, control values and USB conformance measured against the native mouse and matched. | Copies the descriptor; no published native-behaviour audit. |
| **Reliability** | Clears all injection after 1 s of host silence, so a crashed controller never leaves a button held. | No documented silence release; a forced button stays held until you clear it. |
| **Link** | Binary frames with CRC and request IDs at a fixed baud. | ASCII command prompt; replies matched by arrival order; a baud handshake lost on power cycle. |

## Quick start

```toml
[dependencies]
medius = "3.4"
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
    device.reset()?;                          // back to passthrough
    Ok(())
}
```

## Features

The base crate is the sync core. Optional features:

| Feature   | Description |
|-----------|-------------|
| `async`   | `AsyncDevice`: async queries over the same core, runtime-agnostic |
| `mock`    | `MockBox`: an in-process fake box for hardware-free tests |
| `tracing` | per-frame TX/RX `tracing` instrumentation |

```toml
medius = { version = "3.3", features = ["async", "mock"] }
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
    match &b.device {
        Some(d) => println!("{} {:?} {} {}", b.id(), b.name(), d.kind, d),  // MAC, name, kind, vid:pid + product
        None => println!("{} is on protocol {}", b.id(), b.version.proto_ver),  // needs a firmware update
    }
}
let m = Device::find_mouse_box()?;            // the box cloning a mouse
let k = Device::find_keyboard_box()?;         // the box cloning a keyboard
let d = Device::open_by_id("5a4e00111e28")?;  // by device MAC (or CH343 serial)
```

A box's identity is its device-chip MAC, so a reopen reaches the same unit after ports renumber.

A box on a protocol other than `PROTO_VER` lists with `device: None`; `open_by_id` and `find_*` return `Error::BadProtoVer` carrying the protocol it reported. Update it from the [dashboard](https://medius.k4tech.net/dashboard).

### Mouse control

```rust
device.move_rel(100, -50)?;          // relative move (+x right, +y down)
device.wheel(3)?;                    // scroll

device.press(Button::Left)?;         // force down
device.release(Button::Left)?;       // release our press (a physical hold stays)
device.force_release(Button::Left)?; // force up, masking a physical hold
device.inject(Button::Right, Action::Press)?; // the generic form

device.reset()?;                     // clear all injection → passthrough
```

Buttons are `Left`, `Right`, `Middle`, `Side1`, `Side2`. Move and wheel take a full `i16`; the firmware clamps to the descriptor with carry, so `move_rel(2000, 0)` lands exactly 2000.

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

Keys are HID keycodes (`Key::A`, `Key::ENTER`, the eight modifiers, F-keys, arrows…) or any usage via `Key::new(0x04)`; media keys are Consumer usages (`MediaKey::VOLUME_UP`, `PLAY_PAUSE`, `MUTE`…). Like buttons, they take the tri-state `Action` (press / soft-release / force-release) and stay held across a reconnect. Both are present-gated: a key the board can't report is a no-op with no error; see `caps()`.

### Sustained motion

Send one fire-and-forget `move_rel` per tick. The firmware merges additively, never halves, and carries the remainder, so a 1 kHz loop lands the full distance; the box paces emitted reports to the native report rate:

```rust
for _ in 0..1000 {
    device.move_rel(1, 0)?;
    std::thread::sleep(Duration::from_millis(1));
}
```

### Emit pacing

`set_emit_pace` sets two NVS-persisted settings in one `OPTION(EMIT)` frame: what paces injected motion, and the rate the clone advertises.

```rust
use medius::EmitPace;

device.set_emit_pace(EmitPace::Learned, None)?;
device.set_emit_pace(EmitPace::Fixed(500), Some(1000))?;
let s = device.query_emit_pace()?;  // mode, resolved_hz, force_hz, advertised_hz, force_active
```

`EmitPace::Learned` (default) paces to the learnt native report rate, `EmitPace::Interval` to the clone's `bInterval` poll rate, and `EmitPace::Fixed(hz)` to `hz`, snapped by the 1 ms frame clock to `1000/n` Hz and capped at `EMIT_MAX_HZ`.

A non-zero `force_hz` re-clones the box with a `bInterval` the device did not advertise, snapped to `1000/n` Hz; it needs `allow_imperfect_clones`. `None` keeps the native interval.

### Rendering

`set_render` sets the texture, and whether native motion goes through it, in one NVS-persisted
`OPTION(RENDER)` frame. The model is [ABCurves](https://github.com/optima-manent/ABCurves) (MIT).

```rust
use medius::RenderMode;

device.set_render(RenderMode::Despiked, false)?;
let s = device.query_render()?;  // mode, full, ready
```

| `RenderMode` | What reaches the wire |
|---|---|
| `Off` | the paced fill, renderer off |
| `Stock` | rendered with the bit-exact triangular smoother |
| `Despiked` | rendered with the smoother's onset ramped, not stepped (factory default) |
| `Unsmoothed` | rendered with no smoother; the model receives raw injection |

`full` extends the model to native motion, so one texture reaches the wire instead of an injected stream beside a relayed one. Rendering adds a little latency, which `full` puts on native motion too. `full` is off by default.

Nothing renders until the box learns a profile for the attached device (`RenderStatus::ready`); until then motion is relayed and injection takes the paced fill. The profile lives in RAM, so after a power cut every box is unready again until the mouse moves.

### Input scale

`lock` blocks physical input on one field while injection still drives it. `scale` is the same
command with the number exposed: the percent of the physical value kept, 0 (lock) to 100 (unlock)
and every value between, up to 255 to amplify. Down to -255, a negative scale weighs the physical
value and reverses it, so `-100` on an axis inverts it. Only an axis takes a negative.

```rust
use medius::{Axis, Blanket, Direction};

device.lock(Axis::X, Direction::Both)?;          // block horizontal motion (scale 0)
device.scale(Axis::Y, Direction::Negative, 60)?; // keep 60% of upward motion
device.unlock(Axis::X, Direction::Both)?;        // pass untouched (scale 100)
```

`Direction::With` and `Direction::Against` are measured against the **bearing** (the direction the
box is injecting), not a fixed sign, so motion along the injection and against it take separate
scales. The box resolves them at the merge point, where the pending injection and the arriving report
are both in hand. `set_bearing` sets how long a bearing is held; past that window the axis has no
bearing, relative directions stop applying, and the physical delta reaches the PC unweighed, with no
host command.

```rust
use medius::{Axis, BearingMode, Direction};
use std::time::Duration;

device.set_bearing(Some(Duration::from_millis(20)), BearingMode::PerAxis)?;
device.scale(Axis::X, Direction::Against, 40)?;  // motion against the bearing kept at 40%
device.scale(Axis::X, Direction::With, 130)?;    // motion along it amplified to 130%
```

A delta takes at most two scales, its fixed direction's and its relative direction's, and they
multiply, so a block in either zeroes the product. `Direction::Both` writes the scale to the two fixed
signs and a full pass to the relative pair, so a `Both` of 50 is 50% with or without a live bearing.
`BearingMode::Vector` instead projects the physical delta onto the injected vector, leaving motion
across it untouched; one relative scale, the lower of X's and Y's, then governs both axes, and
`query_locks` reports it on both. Each axis's absolute scale applies to what the projection left, not
the sign the report carried, so a block still covers motion the projection put on that axis.

Only an axis has a bearing, so `With`/`Against` on a button, key or media usage is
`Error::RelativeDirection` instead of a frame the box would drop. A button, key or media usage
carries one bit: a scale below 100 locks it, 100 or above unlocks it, and a negative is
`Error::LockScaleUsage`. A media usage has no edges (it is suppressed whole), so an edge on one is
sent as `Both`, which `query_locks` reports. `lock_all(Blanket::Keys, ...)` honours the edge:
`Positive` blocks presses only, `Negative` releases only.

### Clip playback

Preload per-frame input into a ring on the box, which drains one entry per native frame on its own clock, free of host scheduling jitter and the per-command send floor.

Motion is a per-frame delta, edges (buttons/keys/media) hold until changed, and a gap emits nothing for N frames. Pace top-ups by `query_status().free`.

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
let stats = device.query_stats()?;      // delivery counters; stats.tx_drops / stats.tx_wedges / stats.link_rx_drops / stats.relay_drops
let locks = device.query_locks()?;      // active input scales; locks.scale_of(...) / locks.is_locked(...)
let catch = device.query_catch()?;      // catch table, drop counts, inter-chip clock
```

### Catch

Subscribe to what the box carries: physical input, HID and vendor traffic in both directions,
proxied control transfers, the bytes the clone emits, and bus lifecycle. Input is reported *before*
lock suppression or injection, so one loop can lock an input and catch it to rebind it. Dropping the
stream unsubscribes.

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

`CatchFilter::watch` takes what `lock` takes. `CatchFilter::all_input()` covers every class;
`watch_class` / `watch_axis` narrow it.

For traffic, `catch_events` yields raw frames. A `Capture` caps the bytes kept per packet; a vendor
bulk pipe at whole packets saturates the 6 Mbaud control link by itself:

```rust
use medius::{Capture, CatchEvent, CatchFilter, TrafficClass};

let events = device.catch_events([
    CatchFilter::everything().with_capture(Capture::First(16)),
    CatchFilter::traffic(TrafficClass::VendorInterrupt, 3),   // this endpoint, whole packets
])?;
while let Ok(CatchEvent::Traffic(t)) = events.recv() {
    println!("{:?} ep {} {} bytes", t.class, t.id, t.true_len);
}
```

Both streams are bounded and drop under back-pressure (`dropped()`), stay open through the
keepalive, and are re-asserted across a reconnect. Under `async`, `recv_async().await`. `Timeline`
maps a box stamp onto this machine's clock, unwrapping the 32-bit rollover and both chips' domains.

### Box management

```rust
device.reboot(RebootTarget::DeviceRun)?;  // restart a chip (run / ROM-download × device / host)
device.reconnect()?;                      // rescan VID/PID, reopen, re-assert held state
device.reapply()?;                        // re-send held overrides now

let fw = device.firmware_info()?;         // both chips' firmware versions and booted app slots
device.update_firmware(UpdateTarget::Device, &image, &mut |p| println!("{}%", p.percent()))?;
```

`update_firmware` writes the image to the chip's spare app slot over the control port and boots it; the box reverts an image that will not run. It needs a box this build can open, so update a box on another control protocol from the dashboard.

The reader reconnects by itself if the link drops. When the device chip restarts under a live link, or the box releases the program's state without a restart (a patch apply or clear, an opt-in toggle, a detached device, the inter-chip link dropping), the crate re-sends what it holds once the clone is up; `ClipHandle::lost` reports a dropped clip.

### Observability

```rust
for line in device.logs() {       // device LOG stream
    println!("[{:?}] {}", line.level, line.text);
}

let c = device.counters();        // frames_tx / frames_rx / crc_drops / reconnects / restarts
```

### Async (feature = `async`)

The same core; only queries await, and fire-and-forget commands are unchanged:

```rust
let device = Device::find()?.into_async();
device.move_rel(10, 0)?;                // instant, not async
let v = device.query_version().await?;  // awaits the correlated reply
```

It uses `flume`'s async recv, so it has no runtime dependency and runs on any executor.

### Mock (feature = `mock`)

```rust
use medius::{Button, Device, FrameType, Health, MockBox, PROTO_VER, Rate, Version};

let mock = MockBox::new()
    .with_version(Version { proto_ver: PROTO_VER, fw_major: 1, fw_minor: 2, fw_patch: 3, mac: [0; 6], name: "my-box".into() })
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

Four layers, `protocol → transport → link → device`, each using only the one below.

| Layer | What |
|---|---|
| `protocol` | wire codec: framed binary (SOF, type, rolling SEQ, length, payload, CRC16), no I/O |
| `transport` | byte pipe (no `unsafe`) and VID/PID discovery over `serialport`; on Windows, `serial2`'s overlapped COM handle keeps a read and a write in flight at once |
| `link` | live connection: reader thread, SEQ-correlated queries, keepalive, reconnect |
| `device` | typed API; each command is one `link.send(...)` |

`Device` methods take `&self`; it is `Send + Sync` and clones cheaply. The link runs framed binary at a fixed 6 Mbaud, and queries correlate by SEQ. After ~1 s of host silence the firmware clears all injection, so a crash never leaves a button stuck; a keepalive thread keeps a deliberately held button held. Tested on Linux and Windows.

## Other languages

The `medius-capi` crate exports the whole API as a C ABI. Its generated header compiles as C and
C++, and a ctypes Python package wraps it. See [`bindings/`](bindings/).

## License

MIT, see [LICENSE](LICENSE).
