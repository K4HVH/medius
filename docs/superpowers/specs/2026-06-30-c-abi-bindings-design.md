# C ABI + C++/Python bindings — design

Date: 2026-06-30. Status: approved, ready to build.

## Goal

Give `medius` a stable C ABI so any language can drive a box, and ship thin idiomatic
wrappers for the two immediate targets, C++ and Python. The binding must be faithful: every
capability the Rust crate exposes stays reachable, with no behaviour change. "Everything needs
to be supported."

## Non-goals

No new box behaviour, no host-side automation or pacing (the crate's own scope rules still
hold). The C ABI is a transport for the existing API, not a place to add features.

## Approach and why

One stable C ABI is the common substrate; cbindgen generates the header; the C++ and Python
wrappers ride on top of it. Rejected alternatives: PyO3 for Python plus cxx for C++ means two
separate FFI mechanisms and no single ABI other languages can reuse; SWIG generates clunky
bindings and drags in a heavy toolchain. A curated C ABI is the standard for cross-language
libraries and is what was asked for.

The main crate is `#![forbid(unsafe_code)]`, so the FFI can't live inside it. A new
`medius-capi` crate holds the `unsafe extern "C"` surface and its own `#[repr(C)]` mirror
types, converting to and from the real `medius` types at the boundary. No FFI concern leaks
into the idiomatic crate, and the crate stays publishable to crates.io untouched.

## Repository layout (all in the `medius` repo, as a Cargo workspace)

```
medius/
  Cargo.toml                 # gains [workspace]; stays a publishable [package]
  src/ ...                   # unchanged
  medius-capi/
    Cargo.toml               # crate-type = ["cdylib","staticlib"]; [lib] name = "medius"; publish = false
    cbindgen.toml
    src/{lib,error,ctypes,convert,device,stream,mock,flash,keys}.rs
    include/medius.h          # committed, cbindgen-generated
    tests/                    # Rust integration tests over the mock feature
  bindings/
    cpp/
      include/medius/medius.hpp   # header-only, RAII, exceptions
      CMakeLists.txt
      examples/  tests/
    python/
      medius/                 # ctypes package; bundles the cdylib
      pyproject.toml
      tests/
  tools/gen-header.sh         # runs cbindgen → include/medius.h
  .github/workflows/          # new jobs for capi/cpp/python + cbindgen drift; publish fix
```

## The C ABI contract

### Conventions

Functions are prefixed `medius_`, grouped by receiver (`medius_device_*`,
`medius_event_stream_*`, `medius_log_stream_*`, `medius_mock_*`). Types are `Medius*`
(`#[repr(C)]`), enums use `#[repr(u8)]` (or `i32` for `MediusStatus`) with wire-correct
discriminants, and macro constants are `MEDIUS_*`. The cdylib's lib name is `medius`, so the
artifact is `libmedius.so` / `medius.dll` / `libmedius.dylib` and the header is `medius.h`.

### Error model

Every fallible function returns `MediusStatus` (`int32`, `MEDIUS_OK = 0`); results go to
out-params. The status enum mirrors `medius::Error` one to one and adds shim-only codes:

| code | meaning |
|---|---|
| `MEDIUS_OK` (0) | success |
| `MEDIUS_ERR_IO` | `Error::Io` (OS/serial) |
| `MEDIUS_ERR_NOT_FOUND` | `Error::NotFound` |
| `MEDIUS_ERR_NO_REPLY` | `Error::NoReply` |
| `MEDIUS_ERR_BAD_PROTO_VER` | `Error::BadProtoVer { got }` |
| `MEDIUS_ERR_QUERY_TIMEOUT` | `Error::QueryTimeout` |
| `MEDIUS_ERR_DISCONNECTED` | `Error::Disconnected` |
| `MEDIUS_ERR_FRAME_TOO_LONG` | `Error::FrameTooLong` |
| `MEDIUS_ERR_FLASH_TOOL` | `Error::FlashTool` (flash feature) |
| `MEDIUS_ERR_INVALID_ARG` | shim: null pointer, bad enum value, non-UTF8 path |
| `MEDIUS_ERR_PANIC` | shim: a Rust panic was caught at the boundary |
| `MEDIUS_ERR_UNKNOWN` | the `#[non_exhaustive]` tail |

A thread-local holds the last error's `Display` text and the `BadProtoVer.got` byte:

```c
size_t  medius_last_error_message(char* buf, size_t cap); // copies, returns full length
uint8_t medius_last_error_proto_ver(void);                // got byte, else 0
```

The thread-local is set on every error return and cleared on the next successful call on that
thread.

### Panic and thread safety

Every `extern "C"` body runs inside `std::panic::catch_unwind` because unwinding across the
FFI boundary is undefined behaviour; a caught panic becomes `MEDIUS_ERR_PANIC`. `Device` is
`Send + Sync` with `&self` methods, so a handle is safe to share and call across threads (the
link serialises I/O); the stream handles wrap `flume` receivers that are `Send + Sync` too.
The header documents this.

### Handles and lifecycle

Opaque pointers via `Box::into_raw`: `MediusDevice`, `MediusEventStream`, `MediusLogStream`,
and (mock feature) `MediusMockBox`. Each has a `*_free`. `medius_device_free` drops the
`Device`, which joins the reader and keepalive threads. Dropping an `EventStream` unsubscribes
CATCH, exactly as in Rust. Freeing a null pointer is a no-op. Passing a handle to the wrong
function family is the caller's contract to keep (documented, not checked).

### Alloc-free data, by construction

The boundary needs no `malloc`/`free` except the handle destructors. The wire protocol bounds
every payload: `MAX_PAYLOAD` is 512, and every variable-length list (`KeyboardEvent.keys`,
`MediaEvent.keys`) is length-prefixed by a `u8`, so it holds at most 255 entries. Fixed-cap
arrays sized to those ceilings therefore never truncate:

- `MEDIUS_MAX_KEYS = 256`, `MEDIUS_MAX_MEDIA_KEYS = 256` (u8 count ceiling).
- `MEDIUS_MAX_LOG_TEXT = 512` (payload ceiling; log text ≤ 511 bytes + NUL).
- `MEDIUS_MAX_PATH = 512` for `PortInfo.path`. This is the one field with a theoretical cap;
  no real serial path (`/dev/ttyACM0`, `COM3`) approaches it. The shim skips any port whose
  path would overflow and counts it in the total, so the cap can never produce a half-written
  string. Documented.

The catch event is a tagged union, reused across a polling loop with zero per-event
allocation:

```c
typedef struct { uint8_t buttons; int16_t dx, dy, wheel; } MediusMouseEvent;
typedef struct { uint8_t modifiers; uint8_t n_keys; uint8_t keys[256]; } MediusKeyboardEvent;
typedef struct { uint8_t n_keys; uint16_t keys[256]; } MediusMediaEvent;

typedef enum { MEDIUS_CATCH_MOUSE = 0, MEDIUS_CATCH_KEYBOARD = 1, MEDIUS_CATCH_MEDIA = 2 } MediusCatchEventKind;
typedef struct {
  MediusCatchEventKind kind;
  union { MediusMouseEvent mouse; MediusKeyboardEvent keyboard; MediusMediaEvent media; } data;
} MediusCatchEvent;

typedef struct { MediusLogLevel level; char text[512]; } MediusLogLine;
```

### Value types (`#[repr(C)]`, returned by value into an out-param)

Flat mirrors of the crate's `Copy` types. `bool` fields become `uint8_t` (0/1).

`MediusVersion`, `MediusHealth`, `MediusMouseCaps`, `MediusKbdCaps`, `MediusCaps` (nests the
two caps + two change-driven flags), `MediusMouseInfo`, `MediusRate`, `MediusStats`,
`MediusLocks { uint16_t mask }`, `MediusCatchState { uint8_t mask; uint32_t dropped }`,
`MediusImperfectStatus`, `MediusCountersSnapshot`, `MediusPortInfo`.

### Parameter enums and tagged params

`#[repr(u8)]` enums with wire-correct values: `MediusButton` (Left=0..Side2=4, matching
`as_id`, since the Rust `Button` is not `repr(u8)`), `MediusAction` (0/1/2),
`MediusRebootTarget` (0..3), `MediusLedTarget` (0..2), `MediusLedMode` (0..3),
`MediusLockDirection` (0..2), `MediusBlanket` (Keys/Media/Buttons), `MediusLogLevel` (0..4),
`MediusFrameType` (the non-contiguous wire bytes). `MediusKey` is `uint8_t`,
`MediusMediaKey` is `uint16_t`. `MediusCatchMask` is `uint8_t` with `MEDIUS_CATCH_MASK_*`
defines (MOTION 0x01, WHEEL 0x02, BUTTONS 0x04, KEYS 0x08, ALL 0x0F).

Data-carrying params become small `#[repr(C)]` structs plus constructor helpers:

```c
typedef enum { MEDIUS_INPUT_BUTTON, MEDIUS_INPUT_KEY, MEDIUS_INPUT_MEDIA } MediusInputKind;
typedef struct { MediusInputKind kind; uint16_t value; } MediusInput; // value = button id / key / media usage
MediusInput medius_input_button(MediusButton b);
MediusInput medius_input_key(MediusKey k);
MediusInput medius_input_media(MediusMediaKey m);

typedef enum { MEDIUS_MOTION_CURSOR, MEDIUS_MOTION_WHEEL } MediusMotionKind;
typedef struct { MediusMotionKind kind; int16_t dx, dy, wheel; } MediusMotion;
MediusMotion medius_motion_cursor(int16_t dx, int16_t dy);
MediusMotion medius_motion_wheel(int16_t delta);

typedef enum { MEDIUS_LOCK_X, MEDIUS_LOCK_Y, MEDIUS_LOCK_WHEEL, MEDIUS_LOCK_BUTTON } MediusLockTargetKind;
typedef struct { MediusLockTargetKind kind; MediusButton button; } MediusLockTarget; // button only when kind == BUTTON
```

The common `Key` and `MediaKey` constants are emitted as `MEDIUS_KEY_*` / `MEDIUS_MEDIA_*`
macros so C users have the same vocabulary; any raw usage is still valid.

### Full function surface

Connection and lifecycle:

```c
MediusStatus medius_device_open(const char* path, MediusDevice** out);
MediusStatus medius_device_find(MediusDevice** out);
void         medius_device_free(MediusDevice* dev);
size_t       medius_find_ports(MediusPortInfo* out, size_t cap, size_t* out_total);
```

Commands (return `MediusStatus`):

```c
medius_device_move_rel(dev, int16 dx, int16 dy);
medius_device_wheel(dev, int16 delta);
medius_device_move_axis(dev, MediusMotion motion);
medius_device_inject(dev, MediusInput input, MediusAction action);
medius_device_button(dev, MediusButton b, MediusAction action);
medius_device_press / soft_release / force_release(dev, MediusButton b);
medius_device_key(dev, MediusKey k, MediusAction action);
medius_device_key_down / key_up / key_force_release(dev, MediusKey k);
medius_device_media(dev, MediusMediaKey m, MediusAction action);
medius_device_media_down / media_up / media_force_release(dev, MediusMediaKey m);
medius_device_lock / unlock(dev, MediusLockTarget t, MediusLockDirection d);
medius_device_lock_key / unlock_key(dev, MediusKey k, MediusLockDirection d);
medius_device_lock_media / unlock_media(dev, MediusMediaKey m);
medius_device_lock_all / unlock_all(dev, MediusBlanket what);
medius_device_led(dev, MediusLedTarget t, MediusLedMode mode, uint8 level);
medius_device_reset / reapply / reconnect(dev);
medius_device_reboot(dev, MediusRebootTarget target);
medius_device_allow_imperfect_clones(dev, bool allow);
medius_device_set_movement_riding(dev, bool enabled, uint32 window_ms); // !enabled => off
```

Queries (return `MediusStatus`, fill out-param):

```c
medius_device_query_version(dev, MediusVersion* out);
medius_device_query_health(dev, MediusHealth* out);
medius_device_query_mouse_info(dev, MediusMouseInfo* out);
medius_device_caps(dev, MediusCaps* out);
medius_device_query_rate(dev, MediusRate* out);
medius_device_query_stats(dev, MediusStats* out);
medius_device_query_locks(dev, MediusLocks* out);
medius_device_query_catch(dev, MediusCatchState* out);
medius_device_query_imperfect(dev, MediusImperfectStatus* out);
medius_device_query_movement_riding(dev, bool* out_enabled, uint32* out_window_ms);
medius_device_counters(dev, MediusCountersSnapshot* out);
```

Pure helpers (no device, faithful re-expressions of crate methods):

```c
bool  medius_locks_is_locked(MediusLocks locks, MediusLockTarget t, MediusLockDirection d);
bool  medius_rate_native_hz(MediusRate rate, float* out_hz);          // false when no cadence
bool  medius_mouse_event_is_pressed(const MediusMouseEvent* e, MediusButton b);
bool  medius_keyboard_event_is_pressed(const MediusKeyboardEvent* e, MediusKey k);
bool  medius_media_event_is_pressed(const MediusMediaEvent* e, MediusMediaKey m);
bool  medius_caps_has_mouse / has_keyboard / is_composite(MediusCaps caps);
```

Streams:

```c
MediusStatus medius_device_catch_events(dev, uint8 mask, MediusEventStream** out);
void         medius_event_stream_free(MediusEventStream* s);
MediusStatus medius_event_stream_recv(MediusEventStream* s, MediusCatchEvent* out);          // blocks; DISCONNECTED on close
bool         medius_event_stream_try_recv(MediusEventStream* s, MediusCatchEvent* out);      // true if filled
bool         medius_event_stream_recv_timeout(MediusEventStream* s, uint64 ms, MediusCatchEvent* out);
uint64       medius_event_stream_dropped(MediusEventStream* s);

MediusStatus medius_device_logs(dev, MediusLogStream** out);
void         medius_log_stream_free(MediusLogStream* s);
MediusStatus medius_log_stream_recv(MediusLogStream* s, MediusLogLine* out);                 // blocks; DISCONNECTED on close
bool         medius_log_stream_try_recv(MediusLogStream* s, MediusLogLine* out);
bool         medius_log_stream_recv_timeout(MediusLogStream* s, uint64 ms, MediusLogLine* out);
```

Meta and constants:

```c
uint32      medius_default_query_timeout_ms(void);     // 1000
uint32      medius_default_keepalive_cadence_ms(void); // 500
uint32      medius_abi_version(void);                  // capi ABI version (bumped on breaking change)
const char* medius_version_string(void);               // crate/capi version string
uint8       medius_proto_version(void);                // expected wire PROTO_VER
```

### What is deliberately not bound, and why nothing is lost

- `AsyncDevice` (chosen): async Rust futures have no C representation; its query_* are identical
  to the sync queries and stream `recv_async` equals `recv`. The sync surface plus
  `try_recv`/`recv_timeout` lets each language build its own async. No capability is lost.
- Stream `try_iter` / `IntoIterator` / `recv_async`: borrowed iterators and futures don't cross
  C. A `try_recv` loop is identical; the wrappers offer iteration on top.
- `flash_with` / `CommandRunner` / `esptool_args`: generic + closure + trait. The concrete
  `flash()` is the bound path (it uses `SystemRunner` internally).
- Rust-side `as_u8`/`from_u8`/`from_id` helpers: the C enums already carry wire values, so they
  round-trip by construction. `native_hz`, `is_pressed`, `is_locked`, the caps predicates are
  re-exposed as pure helpers above.

## mock feature (`--features mock`)

`MediusMockBox` with the full surface, all setters in place (the Rust builders consume `self`
but mutate shared `Arc` state, so they collapse to setters in C):

```c
MediusMockBox* medius_mock_new(void);
void           medius_mock_free(MediusMockBox* m);
void medius_mock_set_version / set_health / set_mouse_info / set_caps / set_mouse_caps /
     set_kbd_caps / set_rate / set_stats / set_locks / set_catch_state /
     set_imperfect_status(MediusMockBox* m, <value>);
void medius_mock_set_movement_riding(MediusMockBox* m, bool enabled, uint32 window_ms);
void medius_mock_set_silent(MediusMockBox* m, bool silent);
void medius_mock_push_raw(MediusMockBox* m, const uint8* bytes, size_t len);
void medius_mock_push_log(MediusMockBox* m, MediusLogLevel level, const char* text);
void medius_mock_push_event(MediusMockBox* m, uint8 seq, MediusMouseEvent report);
void medius_mock_push_kb_event(MediusMockBox* m, uint8 seq, const MediusKeyboardEvent* e);
void medius_mock_push_cons_event(MediusMockBox* m, uint8 seq, const MediusMediaEvent* e);
size_t medius_mock_recorded(MediusMockBox* m);
bool   medius_mock_saw(MediusMockBox* m, MediusFrameType ty);
void   medius_mock_clear_recorded(MediusMockBox* m);
size_t medius_mock_recorded_frame(MediusMockBox* m, size_t idx, MediusFrameType* out_ty,
                                  uint8* out_seq, uint8* payload_buf, size_t cap); // returns payload len
MediusStatus medius_device_with_mock(const MediusMockBox* m, MediusDevice** out);  // no handshake
MediusStatus medius_device_open_mock(const MediusMockBox* m, MediusDevice** out);  // runs handshake
```

The `silent` builder becomes a `set_silent(bool)` toggle.

## flash feature (`--features flash`)

```c
MediusStatus medius_flash(const char* port, const char* bin_path, bool host); // Linux/Windows only
```

Platform-gated to match the crate (`#[cfg(any(target_os="linux", windows))]`). On macOS the
function compiles to an immediate `MEDIUS_ERR_UNKNOWN`/unsupported status (documented), so the
header stays stable across platforms.

## C++ wrapper (`bindings/cpp/include/medius/medius.hpp`)

Header-only, C++17 floor, exceptions. RAII move-only classes own the opaque handles and free on
scope exit: `medius::Device`, `EventStream`, `LogStream`, and (mock feature) `MockBox`. Static
factories `Device::find()` / `Device::open(path)` return a `Device` or throw `medius::Error`
(which carries the `MediusStatus` and the last-error message). Fallible methods return the value
directly and throw on failure; the hot fire-and-forget path only throws on a real link error.
Enums are scoped (`enum class medius::Button`); value structs are thin C++ structs with the
helper methods (`MouseEvent::is_pressed`, `Caps::is_composite`, `Rate::native_hz` returning
`std::optional<float>`). `CatchEvent` exposes the kind plus typed accessors. A CMake package
config (`find_package(medius)`) links the cdylib or staticlib and adds the include dir. Examples
and a mock-backed test (one TU including the header twice to prove no ODR issues) ship alongside.

## Python package (`bindings/python/medius/`)

Pure-stdlib ctypes. `_native.py` declares every signature against the bundled
`libmedius.{so,dll,dylib}`, located next to the package via `importlib.resources`. Pythonic
classes wrap the handles: `Device`, `EventStream`, `LogStream`, `MockBox`, each a context
manager with `__del__` cleanup, raising `medius.MediusError` (subclasses per status) on failure.
Enums are `enum.IntEnum`; value structs are `ctypes.Structure` surfaced as small dataclasses or
named tuples; `CatchEvent` is a dataclass with `kind` and the relevant payload. The stream
classes are iterable (a `try_recv`/`recv` loop). The build backend (setuptools or hatchling with
a small build hook) runs `cargo build --release -p medius-capi` and copies the cdylib into the
package; `cibuildwheel` in CI produces wheels for manylinux (x86_64, aarch64), macOS (x86_64,
arm64), and Windows (amd64), each bundling its binary, so `pip install medius` works with no Rust
toolchain and no system deps.

## CI changes

New jobs in `.github/workflows/`:

- capi build on the three OSes (`cargo build -p medius-capi --features mock,flash`), plus a
  default-feature build.
- cbindgen drift check: run `tools/gen-header.sh`, `git diff --exit-code include/medius.h`.
- C++ build + ctest against the mock feature.
- Python build + pytest against the mock feature.
- cibuildwheel matrix producing the wheels (artifact upload; publish to PyPI deferred to a later
  productization step).

Fix the existing publish pipeline for the workspace, since `cargo metadata --no-deps |
jq '.packages[0].version'` is no longer guaranteed to pick `medius`:

- version check uses `jq -r '.packages[] | select(.name=="medius") | .version'`.
- `cargo publish` / `--dry-run` get an explicit `-p medius`.

The existing per-feature `check`/`test`/`clippy` jobs keep targeting the root `medius` package
(workspace members are linted by their own jobs).

## Testing strategy

- Rust: integration tests in `medius-capi/tests/` drive the C ABI through `MediusMockBox`,
  asserting recorded frames match the equivalent native crate calls byte for byte, that errors
  map to the right status, that streams deliver pushed events, and that handles free cleanly. Run
  under Miri where feasible for the unsafe paths.
- C++: ctest builds the header against the mock feature and repeats the parity assertions through
  the C++ API; an ASan/UBSan build catches lifetime and union bugs.
- Python: pytest drives the same parity assertions through the ctypes layer.
- Adversarial self-review (a workflow): one pass hunting FFI memory-safety bugs (use-after-free,
  null deref, union read of the wrong arm, missing catch_unwind, leak on error path), one pass
  auditing faithfulness (every public crate method has a binding in all three layers, semantics
  preserved).
- On-hardware: a parity check mirroring `examples/hw_full.rs` driven through the C ABI / C++ /
  Python against the real box, the final gate. Autonomous where the grabbed-evdev pattern allows;
  ping the user for any physical-motion check that needs eyes.

## Build phases

1. Workspace + `medius-capi` core (errors, handles, repr(C) types, all commands/queries/streams,
   find, counters, helpers, constants, key tables) + cbindgen header + mock-backed Rust tests.
2. mock + flash feature surfaces + their tests.
3. C++ header + CMake + examples + tests.
4. Python ctypes package + build backend + cibuildwheel CI.
5. CI wiring + publish fix + docs + adversarial review + on-hardware parity gate.

## Risks

- Workspace conversion breaking the crates.io publish job (mitigated by the jq fix + `-p medius`).
- cbindgen output drift between versions (pin cbindgen; commit the header; CI drift check).
- Union/lifetime bugs in the unsafe layer (ASan/UBSan + Miri + adversarial review).
- `PortInfo.path` cap (mitigated: skip-and-count, never a half-written string; real paths are
  far under 512).
- Python wheel build needs the Rust toolchain in CI and the right cibuildwheel before-build step
  per platform (standard, but the macOS arm64 + manylinux aarch64 cross cases need care).
