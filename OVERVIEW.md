# medius — library overview

The `medius` crate is the host control plane for the Medius transparent mouse-passthrough box. It is a
**faithful, minimal v1**: a 1:1 binding of the firmware's command primitives, plus the host-side 1 kHz
pacer and the infrastructure to drive the box reliably — and **nothing that automates or composes
mouse input** (no click, drag, hold, smoothing, or auto-velocity).

Validated end-to-end on real hardware (see "Validation"). Tracks the firmware in `medius-fw`
(`control-protocol.md` is the wire source of truth).

---

## 1. Scope — what it is and isn't

| In scope | Out of scope (deliberately) |
|---|---|
| 1:1 bindings of the firmware frames (MOVE/WHEEL/BUTTON/RESET/QUERY/REBOOT_DL/LOG) | `click`, `drag`, double-click, any composed multi-frame gesture |
| The 1 kHz `MovementSession` pacer (caller supplies every delta; the library only paces *when* frames emit — the firmware offloads pacing to the host) | `set_velocity` / any auto-generated motion |
| Infrastructure: connect/handshake, keepalive (holds caller-commanded state), reconnect+reapply, the reader, SEQ correlation | smoothing / humanization / trajectory synthesis |
| Linux + Windows raw serial, async wrapper, mock, metrics, CLI, tracing, serde | firmware-side features not yet in `control-protocol.md` |

**The test for "is this an extra":** a function that *generates/automates input the caller didn't
explicitly supply* is a gesture → out. A function that binds one firmware frame, maintains
caller-commanded state, recovers the link, or paces caller-supplied data → infrastructure → in.

---

## 2. Architecture

```
src/
  protocol/   internal, pure, no-I/O wire layer — crc16, frame codec, opcodes, command/response, types
  transport/  PRIVATE — Transport trait; linux (termios2/4M/DTR-RTS), windows (DCB), mock, VID/PID scan
  device/     Device core — connect/handshake, commands, queries, logs, reconcile (DesiredState),
              reboot/reconnect/keepalive, counters
  pacer/      MovementSession — push→1kHz emission, precise clock, metrics
  asyncv/     AsyncDevice — thin wrapper over the SAME core (async only on query)
  mock/       MockBox — public scriptable fake box (feature `mock`)
  flash/      esptool reboot+flash handoff (feature `flash`)
  bin/medius  CLI (feature `cli`)
  config/error/trace
```

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
- `Device::open(path)` / `open_with(path, &ConnectOptions)` — open + single-shot version handshake.
- `Device::find()` / `find_with(&ConnectOptions)` — scan by CH343 VID/PID (`0x1A86:0x55D3`), open first.
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

### Pacer — `MovementSession` (the host's 1 kHz job)
- `device.movement()` / `movement_at(hz)` / `movement_with(&ConnectOptions)` → spawns the real-time
  pacer thread.
- `push(dx, dy)` — accumulate a caller delta; the pacer emits ≤1 MOVE per tick (drift-free absolute
  clock; carry-remainder for >i16 bursts; idle ticks emit nothing).
- `set_rate(hz)` / `rate()` · `stats() -> PacerStats` (feature `metrics`: tick-jitter + write-latency
  histograms).

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
`Device`, `AsyncDevice`, `MovementSession`, `Button`, `ButtonAction`, `RebootTarget`, `Version`,
`Health`, `LogLine`, `LogLevel`, `ConnectOptions`, `CountersSnapshot`, `PacerStats`,
`HistogramSnapshot`, `PortInfo`, `find_medius`, `MockBox`, `LogStream`, `Error`, `Result`,
`DEFAULT_RATE_HZ`, plus `FrameType`/`DecodedFrame` (frame-inspection for `MockBox`). The wire codec
(`protocol`) and low-level discovery (`find_ports`/`WCH_VID`/`CH343_PID`) are now crate-internal.

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

`default` = lean sync core. `async` (AsyncDevice) · `mock` (MockBox) · `metrics` (PacerStats) ·
`flash` (esptool handoff) · `cli` (the `medius` binary) · `tracing` (library spans, off the pacer hot
path) · `serde` (derives on value types, snake_case).

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

- **Host-free:** every feature flag + `--all-features` (164 unit + 4 bin + 10 doctests) pass; clippy
  clean on Linux **and** `x86_64-pc-windows-msvc`; `cargo doc` link-clean; examples build.
- **Hardware (`examples/hw_full.rs`, grabbed):** handshake, move (exact/neg/zero/diagonal/carry),
  wheel, all 5 buttons × actions, force-release, reset, 1 kHz pacer (no-halving), keepalive-holds,
  query-under-1kHz-load (SEQ correlation), reconnect+reapply, async query, no-stuck/crash safety — **all
  PASS**, `crc_drops=0`. Re-validated after every fix.
- **Adversarial review:** 4-lens (firmware, concurrency, cleanup-regression, protocol-integration);
  all confirmed findings fixed (selector-aware correlation, CLI `--rate`, firmware linger decoupling).

## 10. Tests & examples

Unit + doctests across all features; a public scriptable `mock` for hardware-free downstream tests.
Examples: `basic`, `paced`, `mock`, `async`, `hw_validate`, `hw_soak`, `hw_full`. CI workflow checks
fmt/clippy(both targets)/test/doc.
