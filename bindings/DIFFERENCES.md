# How the bindings deviate from the native Rust API

The C ABI and the C++/Python wrappers aim to be faithful, but a C boundary can't be byte-identical
to idiomatic Rust. This lists every deviation, worst first, so you know exactly what changes when you
cross the boundary. Capability is preserved everywhere — these are ergonomic or model differences, not
missing features. (The one real capability gap, the missing `Clone`, has been fixed: every handle has
`clone`.)

## 1. Errors: a thread-local last-error instead of a value (raw C ABI only)

The biggest model difference, and it only bites the raw C ABI. In Rust the error is a value that
carries its data and that you match exhaustively:

```rust
match dev.query_version() {
    Ok(v) => ...,
    Err(Error::BadProtoVer { got }) => eprintln!("box speaks v{got}"),
    Err(e) => ...,
}
```

In C the call returns a status code and stashes the detail in thread-local state you read separately:

```c
MediusVersion v;
if (medius_device_query_version(dev, &v) != MEDIUS_STATUS_OK) {
    char buf[256];
    medius_last_error_message(buf, sizeof buf);   // valid until the next call on this thread
    uint8_t got = medius_last_error_proto_ver();   // the BadProtoVer byte, else 0
}
```

The footgun: make another call before reading and the message is overwritten. The C++ and Python
wrappers remove it — they capture the status, message, and `proto_ver` at the moment of failure:
`medius::Error` (a `std::runtime_error`) and `medius.MediusError` (with per-status subclasses) behave
like Rust's value error.

## 2. `inject` loses its generic argument

Rust's `inject` takes `impl Into<Input>`, so you pass the target directly:

```rust
dev.inject(Button::Left, Action::Press)?;
dev.inject(Key::A, Action::Press)?;
```

The bindings can't express that, so you build an `Input` first:

```python
dev.inject(Input.button(Button.LEFT), Action.PRESS)   # python
```
```cpp
dev.inject(Input::button(Button::Left), Action::Press);  // c++
```

In practice you rarely call `inject` directly — `press`, `key_down`, `media_down`, etc. take the
target directly in every language, so this only shows up on the field-generic path.

## 3. No async (deliberate)

Rust's `async` feature offers `dev.query_version().await` and `events.recv_async().await`. The bindings
are synchronous; async Rust futures have no C representation. Nothing is lost — the sync calls plus the
non-blocking `try_recv` / `recv_timeout` let each language build its own async (asyncio executor,
`std::async`, a thread). This was an explicit scope decision.

## 4. Enums are plain integers at the C boundary

Rust can't construct an invalid `Button`. The C enums are `uint8_t`-backed; passing an out-of-range
value is the caller's contract to avoid (undefined behavior if you don't), the same as any C enum. The
C++ `enum class` and Python `IntEnum` make this hard to do by accident; only hand-written C is exposed.

## 5. `Duration` becomes whole milliseconds

`set_movement_riding(Some(Duration::from_micros(1500)))` in Rust can express sub-millisecond windows;
the bindings take a `u32` (or `chrono::milliseconds` / `int`) in whole ms. No capability is lost — the
firmware rounds to whole ms regardless — but the type is coarser.

## 6. Smaller ergonomic trims

- `MockBox` builders chain in Rust (`MockBox::new().with_version(v).with_health(h)`); the bindings use
  separate `set_*` calls (the mock is a test-only surface).
- `Display` strings (`"fw 2.1.0"`, `"1532:006c"`) aren't bound; the fields are there, format them
  yourself.
- `find` returns a fixed-capacity array at the C level (paths are capped at 511 bytes, longer ones are
  omitted — no real serial path is near that). The C++ `find_ports()` and Python `find_ports()` rebuild
  a normal vector/list, so only the raw C ABI sees the caller-array shape.
- A few helpers aren't bound where the language has a better idiom: `Health::from_flags`/`to_flags`
  (read the fields), some `CatchMask` set operations (C++ has `operator|`, Python `CatchMask` is an
  `IntFlag`).

## What is fully faithful

Every command, query, and stream; the `mock` and `flash` features; the value-type helpers
(`is_pressed`, `is_locked`, `native_hz`, the caps predicates); the key/media constants; and `clone`
on `Device`, `EventStream`, `LogStream`, and `MockBox`. The C ABI emits byte-for-byte identical wire
frames to the native API (verified by parity tests through the mock), so on real hardware a binding
drives the box exactly as the Rust crate does.
