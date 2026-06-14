# medius — architecture

Design notes for the `medius` crate. The user-facing guide is the [README](../README.md); the
byte-exact wire spec is `control-protocol.md` in the firmware repo. This is a temporary reference until
the hosted docs are published.

## Scope

A **faithful, minimal v1**: a 1:1 binding of the firmware's frames plus the infrastructure to run the
link, and nothing that automates, paces, or composes input.

| In | Out (deliberately) |
|---|---|
| 1:1 frame commands (MOVE / WHEEL / BUTTON / RESET / QUERY / REBOOT_DL / LOG) | click, drag, double-click, any composed gesture |
| connect/handshake, keepalive, reconnect+reapply, the reader, SEQ correlation | `set_velocity` / generated motion; a host-side pacer or frame clock |
| Linux + Windows serial, async wrapper, mock, tracing | smoothing / humanization / trajectory synthesis |

**The "is this an extra" test:** a function that *generates input the caller didn't supply*, or clocks
*when* their input emits, is out. A function that binds one frame, maintains caller-commanded state, or
recovers the link is infrastructure — in.

## Layers

`protocol → transport → link → device` (a clean dependency DAG; `types`/`error`/`trace` cross-cut).

```
protocol/   pure, no-I/O wire codec — crc16, frame codec, opcodes, command/response
transport/  byte pipe — Transport trait, serial (serialport crate), mock, VID/PID scan
link/       connection engine — Link handle + state, TX path, reader, correlation,
            keepalive, reconnect, plus counters/reconcile/log-push link state
device/     thin typed API over a Link — Device/AsyncDevice, connect, movement,
            buttons, admin, query, logs
```

`device/` is a skin: each command is one `self.link.send(...)`. The engine lives in `link/`, split one
responsibility per file. `Device` and `AsyncDevice` are both newtypes over the same `Link`.

## Wire protocol

`[SOF 0xA5][TYPE u8][SEQ u8][LEN u16 LE][PAYLOAD ≤512][CRC16-CCITT-FALSE LE]`, CRC over
`TYPE|SEQ|LEN|PAYLOAD`. Fixed **4 Mbaud, framed-only** from power-up — no baud dance, no ASCII REPL.
Connect = open → `QUERY(VERSION)` → check `proto_ver`. Fire-and-go hot path (no per-command ACK); the
only round-trip is `QUERY→RESP`, correlated by SEQ. Unknown opcodes are ignored (forward-compat). The
box emits an unsolicited `RESP(VERSION)` (SEQ=0) at boot and on first contact as a ready signal.

## Concurrency

`Device` is `&self`-only, `Send + Sync`, a cheap `Arc<LinkInner>` clone (via `Link`). Two threads:

- **reader** — sole transport reader: `read → FrameDecoder → route by TYPE` (RESP → fulfil the
  SEQ+selector-matched waiter; LOG → fan out; else ignore). On a read error it auto-reconnects in place
  with back-off — the same rescan/reopen/reapply path as manual `reconnect()`. Observes a stop flag
  within one read timeout for deterministic shutdown.
- **keepalive** — sends a cheap `QUERY(HEALTH)` **only while desired-state is non-idle**, so a held
  button survives idle periods. Silent when idle, so the firmware silence auto-clear still fires on a
  real crash.

Anti-cycle / anti-self-join: the threads capture individual `Arc`s, **never `Arc<LinkInner>`**, so
`LinkInner::drop` can join the reader without the reader ever owning the last ref. Writes are serialized
by one mutex held only around `transport.write_all` (never nested → deadlock-free). Queries correlate by
a generation-tagged, selector-aware SEQ map: a free SEQ, a monotonic gen (so a stale canceller can't
evict a newer waiter), and an expected QUERY selector (so an unsolicited RESP can't be cross-delivered).

## Infrastructure behaviors

- **Single-shot handshake** — one `QUERY(VERSION)` with a short bounded retry; firmware boot-hello +
  early RX make the first frame reliable. `QueryTimeout → NoReply`; wrong version → `BadProtoVer`.
- **Keepalive holds state** — while a button override is held, a sub-1 s `QUERY(HEALTH)` resets the
  firmware's silence timer so the hold survives. Idle = silent → a real crash still clears.
- **Reconnect + reapply** — recovers a re-enumerated port (VID/PID rescan) and re-asserts held state.
  Manual via `reconnect()`; automatic from the reader on a dropped link.
- **No-stuck safety** — on host silence > ~1 s (or RESET, or a real disconnect) the firmware clears
  injection → passthrough. The only crash detector the hardware can have, so it stays.

## Firmware coupling

To keep the host faithful (no host workarounds), three firmware changes were made + validated: (1) the
silence timer resets on any inbound frame, not just injection, so a QUERY keepalive holds state; (2) a
boot/first-contact `RESP(VERSION)` hello + earlier UART RX for the single-shot handshake; (3) the
silence constant extracted as a named value.

## Validation

- **Host-free** (`src/tests/`): decoder robustness (garbage/resync, bad-CRC drop+count, truncation),
  SEQ correlation under concurrent queries, async query success/timeout, keepalive fire/silent/stop,
  end-to-end behavior via `MockBox`, plus internal unit tests over crate-private seams. Green under
  `--all-features`; clippy clean on Linux and `x86_64-pc-windows-msvc`; doc link-clean.
- **Hardware** (`examples/hw_full.rs`, grabbed evdev): handshake, all moves/wheel/buttons/reset,
  1 kHz no-halving, a sustained soak, keepalive-holds, query-under-load, reconnect+reapply,
  reboot-to-run, the async gate, no-stuck safety — all PASS, `crc_drops=0`. An opt-in
  `MEDIUS_UNPLUG_TEST=1` phase additionally proves unattended auto-reconnect across a real link drop.
