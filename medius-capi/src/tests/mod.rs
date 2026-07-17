//! In-crate tests for the C ABI: `helpers` needs no features; `abi` drives the full surface through the mock box, gated on `mock`.

mod helpers;

#[cfg(feature = "mock")]
mod abi;
