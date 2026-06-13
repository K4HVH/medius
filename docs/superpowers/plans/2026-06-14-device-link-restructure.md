# Device → Link/Device Restructure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (inline) to implement this plan stage-by-stage. This is a pure reorganization — the existing test suite is the safety net. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Split the fused `device/` module into a clean 3-layer stack — `transport/` (byte pipe) → `link/` (connection engine) → `device/` (thin typed API) — so concerns are separated by layer, not jumbled in one folder.

**Architecture:** A new `link/` module owns the live-connection engine (threads, correlation, keepalive, reconnect, link state). `Device` and `AsyncDevice` become thin newtypes over a shared `Link = Arc<LinkInner>`. No behavior changes; every existing test must stay green at every stage.

**Tech Stack:** Rust, parking_lot, flume, serialport. Dependency DAG after: `protocol` (pure) ← `link` → `transport`; `device` → `link`; `types`/`error`/`trace` cross-cut.

**Comment policy:** every new/moved file follows `code-comment-policy` — no inline `//` slop, no private-item docs, public items get ≤1-line `///`.

---

## Target file map

```
protocol/   [unchanged]
transport/  [unchanged]   Transport trait, serial, mock, scan, Disconnected
link/        ★ NEW
  mod.rs          Link(Arc<LinkInner>) + LinkInner state + construction + Drop + TX path (write_frame/send/next_seq)
  slot.rs         TransportSlot
  reader.rs       spawn_reader, reader_loop, route_frame
  correlation.rs  PendingEntry, register_pending, cancel_query, register_query, query (sync), query_async
  keepalive.rs    KeepaliveCtx, spawn_keepalive, keepalive_loop
  reconnect.rs    ReconnectCtx, reconnect, reapply_held, auto_reconnect
  reconcile.rs    DesiredState, Override
  counters.rs     Counters
  logs.rs         push() ring-buffer + LOGS_CAPACITY
device/     [slimmed]
  mod.rs          Device { link: Link } handle, Clone/Debug, counters()/logs() pass-throughs, into_async
  connect.rs      open(), find(), handshake()
  movement.rs     move_rel, wheel
  buttons.rs      button, press, soft_release, force_release, reset
  admin.rs        reboot, reconnect(), reapply()
  query.rs        query_version, query_health (typed)
  logs.rs         LogStream + logs()
  asyncv.rs       AsyncDevice { link: Link }
   types/ · error/ · trace/ · mock/ · flash/   [unchanged]
```

Per-stage gate (the "verify" step everywhere below) = all must pass:
`cargo fmt && cargo build --all-features && cargo test --all-features && cargo clippy --all-features --all-targets`.
Windows clippy + `cargo doc -D warnings` run at the final stage.

---

### Task 1: Scaffold `link/` and move the self-contained leaf types

**Files:**
- Create: `src/link/mod.rs`, `src/link/counters.rs`, `src/link/slot.rs`, `src/link/reconcile.rs`, `src/link/logs.rs`
- Modify: `src/lib.rs` (add `mod link;`), `src/device/mod.rs` (import the moved types from `link`)
- Delete: `src/device/counters.rs`, `src/device/reconcile.rs`

- [ ] **Step 1:** Add `mod link;` to `lib.rs` and create `link/mod.rs` declaring `pub(crate) mod counters; pub(crate) mod slot; pub(crate) mod reconcile; pub(crate) mod logs;`.
- [ ] **Step 2:** Move `Counters` verbatim → `link/counters.rs`. Move `TransportSlot` (struct + impl) out of `device/mod.rs` → `link/slot.rs`. Move `DesiredState` + `Override` → `link/reconcile.rs`. Move log `push()` + `LOGS_CAPACITY` → `link/logs.rs` (leave `LogStream` + `Device::logs()` in `device/logs.rs`).
- [ ] **Step 3:** Delete `device/counters.rs` and `device/reconcile.rs`; update all `use super::counters::Counters` / `reconcile::DesiredState` / `TransportSlot` references (in `device/mod.rs`, `device/reboot.rs`, `device/logs.rs`, `transport/*`, tests) to `crate::link::…`.
- [ ] **Step 4:** Verify (gate). Commit: `refactor(link): scaffold link/ and move counters, slot, reconcile, log-push`.

---

### Task 2a: Introduce `Link`, re-base `Device` in place (logic move, files still in device/)

**Files:**
- Modify: `src/device/mod.rs` (rename `Inner`→`LinkInner`, add `Link(Arc<LinkInner>)`, change `Device` to `{ link: Link }`, move engine `impl Device` methods → `impl Link`), `src/device/{commands,query,connect,reboot,logs}.rs` + `src/mock/mod.rs` + `src/tests/*` (delegate through `self.link`)

- [ ] **Step 1:** In `device/mod.rs`: rename `Inner` → `LinkInner`; add `#[derive(Clone)] pub(crate) struct Link { inner: Arc<LinkInner> }`. Move `Drop`, `cancel_query`, construction (`from_transport*`), TX path (`write_frame`/`send`/`send_with_seq`/`next_seq`), and the accessors (`desired`/`transport_slot`/`counters_inner`/`query_timeout_default`/`weak_inner`/`logs_rx`) from `impl Device`/`Inner` to `impl Link`/`LinkInner`.
- [ ] **Step 2:** Change `Device` to `pub struct Device { link: Link }`. Construction returns `Device { link: Link::from_transport(...) }`. Every remaining `Device` method delegates: `self.link.send(...)`, `self.link.query(...)`, etc.
- [ ] **Step 3:** Update callers of the moved seams — `mock/mod.rs` (`with_mock`/cadence ctor), `src/tests/*` (`transport_slot()`, `pending_len()`, `from_transport_with_cadence`, `with_mock`) — to reach them via `Device`'s delegating methods or `device.link` as needed. Keep the public test API identical where possible.
- [ ] **Step 4:** Verify (gate). Commit: `refactor(link): introduce Link engine handle; Device is a thin wrapper`.

---

### Task 2b: Relocate the engine files from `device/` into `link/`

**Files:**
- Create: `src/link/reader.rs`, `src/link/correlation.rs`, `src/link/keepalive.rs`, `src/link/reconnect.rs`; fill in `src/link/mod.rs` (LinkInner + Link + construction + TX path)
- Modify: `src/device/mod.rs` (drops to the thin handle), `src/link/mod.rs` (module decls)
- Delete: `src/device/reboot.rs` (its command half already extracted in Task 3)

- [ ] **Step 1:** Move `LinkInner` + `Link` + construction + TX path from `device/mod.rs` → `link/mod.rs`. Move `spawn_reader`/`reader_loop`/`route_frame` → `link/reader.rs`. Move `PendingEntry`/`register_pending`/`cancel_query`/`register_query`/`query`/`query_timeout`/`pending_len` → `link/correlation.rs`.
- [ ] **Step 2:** Move keepalive (`KeepaliveCtx`/`spawn_keepalive`/`keepalive_loop`) → `link/keepalive.rs`; move reconnect (`ReconnectCtx`/`reconnect`/`reapply_held`/`auto_reconnect` + `reconnect_ctx()`) → `link/reconnect.rs`. (The `reboot` *command* and public `reconnect()`/`reapply()` stay on `Device` — handled in Task 3.)
- [ ] **Step 3:** `device/mod.rs` now contains only `Device { link }`, `Clone`/`Debug`, `counters()`/`logs()` pass-throughs, `into_async`. Fix all `use` paths.
- [ ] **Step 4:** Verify (gate). Commit: `refactor(link): relocate reader, correlation, keepalive, reconnect into link/`.

---

### Task 3: Split the device API by concern

**Files:**
- Create: `src/device/movement.rs`, `src/device/buttons.rs`, `src/device/admin.rs`
- Modify: `src/device/mod.rs` (module decls), `src/device/query.rs` (typed only)
- Delete: `src/device/commands.rs`

- [ ] **Step 1:** Split `commands.rs`: `move_rel`/`wheel` → `movement.rs`; `button`/`press`/`soft_release`/`force_release`/`reset` → `buttons.rs`. Delete `commands.rs`.
- [ ] **Step 2:** Create `admin.rs` with the public box-management surface: `reboot(target)` (calls `self.link.send(RebootDl, …)`), `reconnect()` and `reapply()` (call `self.link.reconnect()/reapply_held()`).
- [ ] **Step 3:** Trim `device/query.rs` to the typed `query_version`/`query_health` (mechanism now lives in `link/correlation.rs`); they call `self.link.query(...)`.
- [ ] **Step 4:** Verify (gate). Commit: `refactor(device): split commands into movement/buttons/admin`.

---

### Task 4: Collapse `asyncv/` → `device/asyncv.rs`, re-base on `Link`

**Files:**
- Create: `src/device/asyncv.rs`
- Modify: `src/link/correlation.rs` (add `query_async`), `src/lib.rs` (`mod asyncv` → `device::asyncv` re-export), `src/device/mod.rs`
- Delete: `src/asyncv/mod.rs` (+ the `asyncv/` dir)

- [ ] **Step 1:** Move the async query mechanism (`recv_async` + the detached timeout-timer thread) from `asyncv/mod.rs` into `link/correlation.rs` as `Link::query_async(what, timeout)`.
- [ ] **Step 2:** Create `device/asyncv.rs`: `pub struct AsyncDevice { link: Link }`. Fire-and-go methods delegate verbatim; `query_version`/`query_health` call `self.link.query_async(...).await`. Keep `Device::into_async`, `AsyncDevice::open`, `from`, `into_inner`.
- [ ] **Step 3:** Update `lib.rs`: replace `#[cfg(feature="async")] mod asyncv;` with `device::asyncv`; keep `pub use … AsyncDevice;`. Delete the `asyncv/` folder.
- [ ] **Step 4:** Verify (gate, including `--features async` tests). Commit: `refactor(device): fold asyncv into device/, re-base AsyncDevice on Link`.

---

### Task 5: Docs + final full gate

**Files:**
- Modify: `OVERVIEW.md` (§2 architecture: the new `protocol→transport→link→device` layering)

- [ ] **Step 1:** Update `OVERVIEW.md` §2 module map + concurrency description to the 3-layer structure.
- [ ] **Step 2:** Final full gate: `cargo fmt --check`, `cargo build --all-features`, `cargo test --all-features`, `cargo clippy --all-features --all-targets`, `cargo clippy --all-features --target x86_64-pc-windows-msvc`, `RUSTDOCFLAGS=-D warnings cargo doc --all-features --no-deps`, `cargo build --all-features --examples`.
- [ ] **Step 3:** Commit: `docs: record the link/device layering`.
- [ ] **Step 4 (optional, user-gated):** Re-run `hw_full` to confirm the reorganization is behavior-neutral on hardware.

---

## Notes / risks
- Tasks 2a/2b are the high-churn pivot; everything else is mechanical. Keep each task a single green commit — if a task can't go green as a unit, stop and re-slice rather than commit broken.
- The existing test suite (30 tests + async) is the behavioral oracle; no new tests needed (pure refactor). Hardware behavior is unchanged by construction.
- Watch `src/tests/*` and `mock/mod.rs` — they poke crate-internal seams (`transport_slot`, `pending_len`, `from_transport_with_cadence`, `with_mock`) that move onto `Link`; keep those seams reachable.
