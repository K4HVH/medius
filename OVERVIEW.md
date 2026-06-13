# medius — library overview

The `medius` crate is the host control plane for the Medius transparent mouse-passthrough box. It is a
**faithful, minimal v1**: a 1:1 binding of the firmware's command primitives plus the infrastructure
to drive the box reliably — and **nothing that automates, paces, or composes mouse input** (no click,
drag, hold, smoothing, auto-velocity, or host-side pacer).

Validated end-to-end on real hardware (see "Validation"). Tracks the firmware in `medius-fw`
(`control-protocol.md` is the wire source of truth).

---

## 1. Scope — what it is and isn't

| In scope | Out of scope (deliberately) |
|---|---|
| 1:1 bindings of the firmware frames (MOVE/WHEEL/BUTTON/RESET/QUERY/REBOOT_DL/LOG) | `click`, `drag`, double-click, any composed multi-frame gesture |
| Infrastructure: connect/handshake, keepalive (holds caller-commanded state), reconnect+reapply, the reader, SEQ correlation | `set_velocity` / any auto-generated motion |
| Linux + Windows raw serial, async wrapper, mock, tracing | a host-side pacer / frame clock — the caller drives MOVE timing |
| | smoothing / humanization / trajectory synthesis |

**The test for "is this an extra":** a function that *generates/automates input the caller didn't
explicitly supply* — or clocks *when* their input emits — is out. A function that binds one firmware
frame, maintains caller-commanded state, or recovers the link → infrastructure → in.

---

## 2. Architecture

```
src/
  protocol/   internal, pure, no-I/O wire layer — crc16, frame codec, opcodes, command/response
  types/      PUBLIC value vocabulary — one file per concern (button, version, health, log, reboot,
              counters, port); pure value types + their wire-mapping helpers
  transport/  PRIVATE — Transport trait; linux (termios2/4M/DTR-RTS), windows (DCB), mock, VID/PID scan
  device/     Device core — connect/handshake, commands, queries, logs, reconcile (DesiredState),
              reboot/reconnect/keepalive, counters
  asyncv/     AsyncDevice — thin wrapper over the SAME core (async only on query, feature `async`)
  mock/       MockBox — public scriptable fake box (feature `mock`)
  flash/      esptool reboot+flash handoff (feature `flash`)
  error/      structured Error / Result
  trace/      feature-gated tracing shim (per-frame TX/RX at TRACE; feature `tracing`)
```

Every top-level concern is its own folder (`mod.rs` + submodules); `lib.rs` is the only loose file.

**Concurrency model.** `Device` is `&self`-only, `Send + Sync`, a cheap `Arc<Inner>` clone. Two
background threads:
- **reader** — sole transport reader: `read → FrameDecoder → route by TYPE` (RESP → fulfil the
  SEQ+selector-matched waiter; LOG → fan out; unknown → ignore). Observes a stop flag within one read
  timeout → deterministic shutdown.
- **keepalive** — sends a cheap `QUERY(HEALTH)` **only while desired-state is non-idle**, so a held
  button survives idle periods (the firmware honours any frame as activity). Silent when idle, so the
  firmware's silence auto-clear still fires on a real host crash.

Writes are serialized by one mutex held *only* around `transport.write_all` (never nested with other
locks → deadlock-free). Queries correlate by a **generation-tagged, selector-aware** SEQ map: each
in-flight query gets a free SEQ, a monotonic gen (so a stale canceller can't evict a newer waiter),
and an expected QUERY selector (so an unsolicited `RESP` — e.g. the firmware boot hello — can never be
cross-delivered to a query awaiting a different selector).

---

## 3. Public API (everything that touches the box)

### Connect
- `Device::open(path)` — open + single-shot version handshake.
- `Device::find()` — scan by CH343 VID/PID (`0x1A86:0x55D3`), open the first match.

No connection config: the box's behavior is fixed, so the two timing values are `pub const`s
(`DEFAULT_QUERY_TIMEOUT` = 1 s, `DEFAULT_KEEPALIVE_CADENCE` = 500 ms), not per-connection knobs.
- `AsyncDevice::open(...)` / `Device::into_async()` — async wrapper over the same core.

### Commands — 1:1 firmware frames (fire-and-go)
| Method | Frame | Notes |
|---|---|---|
| `move_rel(dx: i16, dy: i16)` | MOVE | relative; firmware carries/clamps to the clone's descriptor field |
| `wheel(delta: i16)` | WHEEL | no artificial cap |
| `button(Button, ButtonAction)` | BUTTON | id × {soft-release, press, force-release} |
| `press(Button)` / `soft_release(Button)` / `force_release(Button)` | BUTTON | named aliases for the 3 actions (one fixed-action frame each) |
| `reset()` | RESET | clear all injection → passthrough |

### Queries — QUERY→RESP (the only round-trip)
- `query_version() -> Version` · `query_health() -> Health` (and async equivalents).

Sustained 1 kHz motion is a caller-driven loop of `move_rel` — the firmware merges additively with no
halving. There is no host-side pacer; the caller owns MOVE timing (e.g. its own real-time loop).

### Box management
- `reboot(RebootTarget)` → REBOOT_DL — the only software reboot (no DTR/RTS auto-reset on this board).
  The target encodes run/download × device/host, so one method covers all four cases.
- `reconnect()` — rescan VID/PID, reopen the port in place, **re-apply held desired state**.
- `reapply()` — re-send currently-held overrides (used by reconnect; available on demand).

### Observability
- `logs() -> LogStream` — device LOG frames (level + text); a small newtype (`recv`/`try_recv`/
  `recv_timeout`/`try_iter`/`IntoIterator`) so the channel impl (`flume`) isn't leaked.
- `counters() -> CountersSnapshot` — frames_tx/rx, crc_drops, reconnects (always-on, cheap atomics).

### Public types
The value vocabulary lives in one place — the `types/` module (`Button`, `ButtonAction`,
`RebootTarget`, `Version`, `Health`, `LogLine`, `LogLevel`, `CountersSnapshot`, `PortInfo`) — each
also re-exported at the crate root, so both `medius::Button` and `medius::types::Button` resolve.
Handles and the rest: `Device`, `AsyncDevice`, `MockBox`, `LogStream`, `Error`, `Result`,
`find_medius`, the `DEFAULT_QUERY_TIMEOUT`/`DEFAULT_KEEPALIVE_CADENCE` consts, plus
`FrameType`/`DecodedFrame` (frame-inspection for `MockBox`). The wire codec (`protocol`) and low-level
discovery (`find_ports`/`WCH_VID`/`CH343_PID`) are crate-internal.

---

## 4. Wire protocol

`[SOF 0xA5][TYPE u8][SEQ u8][LEN u16 LE][PAYLOAD ≤512][CRC16-CCITT-FALSE LE]`, CRC over
`TYPE|SEQ|LEN|PAYLOAD`. Fixed **4,000,000 baud, framed-only** from power-up; connect = open (DTR/RTS
deasserted) → `QUERY(VERSION)` → `proto_ver == 1`. Fire-and-go hot path (no per-command ACK); the only
round-trip is `QUERY→RESP` (SEQ-correlated). Unknown opcodes ignored (forward-compat). The box emits an
unsolicited `RESP(VERSION)` (SEQ=0) at boot + on first contact as a presence/ready signal.

---

## 5. Infrastructure behaviors

- **Single-shot handshake.** One `QUERY(VERSION)`; the firmware boot-hello + early-RX make the first
  frame reliable (hardware: 30/30 steady, 80/80 through a reboot). A `QueryTimeout` → `NoReply`; wrong
  version → `BadProtoVer`.
- **Keepalive holds state.** While a button override is held, a sub-1 s `QUERY(HEALTH)` keeps the
  firmware from auto-clearing it (the firmware resets its silence timer on any frame). Idle = silent,
  so a real crash still clears (the no-stuck safety).
- **Reconnect + reapply.** Recovers a re-enumerated port (VID/PID rescan) and re-asserts the held
  desired state.
- **No-stuck safety.** On host silence > 1 s (or `RESET`, or a real disconnect), the firmware clears
  injection → passthrough. This is the *only* crash detector the hardware can have (no DTR/RTS / no
  port-close signal), so it stays.

---

## 6. Feature flags

`default` = lean sync core. `async` (AsyncDevice) · `mock` (MockBox) · `flash` (esptool handoff) ·
`tracing` (library spans; per-frame TX/RX at TRACE only).

---

## 7. Error model

`enum Error { Io, NotFound, NoReply, BadProtoVer{got}, QueryTimeout, Disconnected, FrameTooLong, ... }`
(`#[non_exhaustive]`, no stringly-typed catch-all). CRC failures are dropped at the decoder (counted),
never surfaced per-frame.

---

## 8. Coupled firmware changes (in `medius-fw`)

To keep the host faithful (no host workarounds), three firmware changes were made + hardware-validated:
1. **Silence timer resets on any inbound frame** (not just injection) — so a QUERY keepalive holds
   state; real silence still clears. (`g_last_inject_us` decoupled from `g_last_cmd_us` so a fast
   keepalive can't pin the idle frame clock.)
2. **Boot/first-contact `RESP(VERSION)` hello + earlier UART RX** — single-shot handshake (no host
   retry).
3. Silence constant extracted (`INJECT_SILENCE_US`).

---

## 9. Validation status

- **Host-free (`tests/`, via the public API + `MockBox`):** decoder robustness (garbage/resync,
  bad-CRC drop+count, truncation), SEQ correlation under concurrent queries, async query
  success/timeout, and end-to-end behavior (query/record/log, handshake accept/reject/silent, thread
  lifecycle). All pass under `--all-features`; clippy clean on Linux **and** `x86_64-pc-windows-msvc`;
  `cargo doc` link-clean; both examples build.
- **Hardware (`examples/hw_full.rs`, grabbed):** handshake, move (exact/neg/zero/diagonal/carry),
  wheel, all 5 buttons × actions, force-release, reset, 1 kHz no-halving, a sustained soak,
  keepalive-holds, query-under-1kHz-load (SEQ correlation), reconnect+reapply, reboot-to-run recovery,
  the async gate (queries + fire-and-go), no-stuck/crash safety — **all PASS**, `crc_drops=0`. The soak
  holds **1000 reports/s** sustained and the no-halving check measures **1000 reports/s, sum 1000**
  (full rate, no halving).

## 10. Tests & examples

Tests live **outside** the implementation: a small, high-value integration suite in `tests/` driven
entirely through the public API + the scriptable `MockBox` (so impl files carry zero test code), plus
the on-hardware `hw_full` suite for everything a mock can't prove. Two examples: `basic` (minimal
usage) and `hw_full` (the grabbed hardware validation). CI checks fmt/clippy(both targets)/test/doc.
