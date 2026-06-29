//! In-crate tests for the C ABI. `helpers` needs no hardware or features; `abi` drives the full
//! surface through the mock box and is gated on the `mock` feature.

mod helpers;

#[cfg(feature = "mock")]
mod abi;
