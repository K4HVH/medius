//! All crate tests, collected here — out of the implementation files, in one place.
//!
//! Two flavors share this location. **Integration-style** tests drive the device through the public
//! API + the scriptable `MockBox` (feature `mock`); they read like a downstream consumer. **Internal**
//! unit tests reach crate-private seams (`MockTransport`, `DesiredState`, the SEQ correlation) to pin
//! invariants the public surface can't express — these need no feature and run on a plain `cargo test`.
//! The on-hardware suite lives separately in `examples/hw_full.rs`.

// Integration-style (public API + MockBox); each file gates itself on its features.
mod async_query;
mod behavior;
mod concurrency;
mod keepalive;
mod robustness;

// Internal unit tests — crate-private seams, no feature needed.
mod correlation;
mod reconcile;
mod reconnect;
