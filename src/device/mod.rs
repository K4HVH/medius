//! The public [`Device`] surface — the concurrency heart of the crate (§5 of the design spec).
//!
//! (Task 3.1 lands the counters; the [`Device`] core, reader thread, commands, queries, logs, and
//! reconcile/keepalive arrive in the following tasks.)

pub(crate) mod counters;

pub use counters::CountersSnapshot;
