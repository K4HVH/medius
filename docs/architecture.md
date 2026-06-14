# medius architecture

Design notes for the `medius` crate. The user-facing guide is the [README](../README.md); the
byte-exact wire spec is `control-protocol.md` in the firmware repo. This is a temporary reference until
the hosted docs are published.

## Scope

A faithful, minimal v1: a 1:1 binding of the firmware's frames plus the infrastructure to run the link,
and nothing that automates, paces, or composes input.

| In | Out (deliberately) |
|---|---|
| 1:1 frame commands (MOVE / WHEEL / BUTTON / RESET / QUERY / REBOOT_DL / LOG) | click, drag, double-click, any composed gesture |
| connect/handshake, keepalive, reconnect+reapply, the reader, SEQ correlation | `set_velocity` / generated motion; a host-side pacer or frame clock |
| Linux + Windows serial, async wrapper, mock, tracing | smoothing / humanization / trajectory synthesis |

The test for whether something is an extra: a function that generates input the caller didn't supply,
or clocks when their input emits, is out. A function that binds one frame, holds caller-commanded state,
or recovers the link is infrastructure, so it stays.

## Layers

`protocol → transport → link → device`, a clean dependency DAG, with `types`/`error`/`trace` crossing
all of them.

```
protocol/   pure wire codec, no I/O: crc16, frame codec, opcodes, command/response
transport/  byte pipe: Transport trait, serial (serialport crate), mock, VID/PID scan
link/       connection engine: Link handle + state, TX path, reader, correlation,
            keepalive, reconnect, plus the counters/reconcile/log-push link state
device/     thin typed API over a Link: Device/AsyncDevice, connect, movement,
            buttons, admin, query, logs
```

`device/` is a skin; each command is one `self.link.send(...)`. The engine lives in `link/`, one
responsibility per file. `Device` and `AsyncDevice` are both newtypes over the same `Link`.

## Wire protocol

`[SOF 0xA5][TYPE u8][SEQ u8][LEN u16 LE][PAYLOAD ≤512][CRC16-CCITT-FALSE LE]`, CRC over
`TYPE|SEQ|LEN|PAYLOAD`. Fixed 4 Mbaud, framed-only from power-up, with no baud dance and no ASCII REPL.
Connect is open, then `QUERY(VERSION)`, then a `proto_ver` check. The hot path is fire-and-forget with no
per-command ACK; the only round-trip is `QUERY→RESP`, correlated by SEQ. Unknown opcodes are ignored
for forward compatibility. The box emits an unsolicited `RESP(VERSION)` (SEQ=0) at boot and on first
contact as a ready signal.

## Concurrency

`Device` takes `&self`, is `Send + Sync`, and clones cheaply as an `Arc<LinkInner>` behind `Link`. Two
threads do the work. The reader is the only thing that reads the transport: it loops
`read → FrameDecoder → route by TYPE`, fulfilling the SEQ-and-selector-matched waiter for a RESP,
fanning out a LOG, and ignoring anything else. On a read error it reconnects in place with back-off, the
same rescan/reopen/reapply path as a manual `reconnect()`, and it checks a stop flag once per read
timeout so shutdown is deterministic. The keepalive thread sends a `QUERY(HEALTH)` only while
desired-state is non-idle, which keeps a held button alive through idle periods; while idle it sends
nothing, so the firmware's silence auto-clear still fires on a real crash.

Both threads capture individual `Arc`s, never `Arc<LinkInner>`, so `LinkInner::drop` can join the reader
without the reader ever holding the last reference. Writes take one mutex, held only around
`transport.write_all` and never nested, so the lock layer can't deadlock. Queries correlate through a
generation-tagged, selector-aware SEQ map: each gets a free SEQ, a monotonic generation (so a stale
canceller can't evict a newer waiter), and the QUERY selector it expects back (so an unsolicited RESP
can't be cross-delivered).

## Infrastructure behaviors

The handshake is a single `QUERY(VERSION)` with a short bounded retry; the firmware's boot hello and
early RX make the first frame reliable. A timeout becomes `NoReply`, a wrong version becomes
`BadProtoVer`. While a button override is held, the keepalive's sub-1 s `QUERY(HEALTH)` resets the
firmware's silence timer so the hold survives; idle stays silent, so a real crash still clears.
Reconnect recovers a re-enumerated port by VID/PID rescan and re-asserts held state, either manually via
`reconnect()` or automatically from the reader on a dropped link. The no-stuck safety lives in firmware:
after ~1 s of host silence (or a RESET, or a real disconnect) it clears injection back to passthrough.
That's the only crash detector the hardware can have, so it stays.

## Firmware coupling

To keep the host faithful and avoid host-side workarounds, three firmware changes were made and
validated: (1) the silence timer resets on any inbound frame, not just injection, so a QUERY keepalive
holds state; (2) a boot/first-contact `RESP(VERSION)` hello plus earlier UART RX for the single-shot
handshake; (3) the silence constant extracted as a named value.

## Validation

The host-free tests in `src/tests/` cover decoder behaviour under garbage, resync, bad CRC, and
truncation; SEQ correlation under concurrent queries; async query success and timeout; keepalive
fire/silent/stop; end-to-end behaviour through `MockBox`; and internal unit tests over crate-private
seams. They pass under `--all-features`, clippy is clean on Linux and `x86_64-pc-windows-msvc`, and the
docs link-check.

The hardware suite in `examples/hw_full.rs` (grabbed evdev) runs handshake, every move/wheel/button/
reset, 1 kHz no-halving, a sustained soak, keepalive-holds, query-under-load, reconnect and reapply,
reboot-to-run, the async gate, and the no-stuck safety. All pass, `crc_drops=0`. An opt-in
`MEDIUS_UNPLUG_TEST=1` phase also proves unattended auto-reconnect across a real link drop.
