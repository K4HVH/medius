# medius-capi

The C ABI for [medius](https://crates.io/crates/medius), the Rust library for Medius, replacement firmware for the
MAKCU box. It builds as a shared and a static library with one header, `include/medius.h`, and is what the Python
binding loads.

A Rust library that exports a C API can depend on it and hand a `medius::Device` it already opened to C code
with `MediusDevice::into_raw`; the handle is freed with `medius_device_free`.

Build, link and API reference: **[medius.k4tech.net/bindings/c](https://medius.k4tech.net/bindings/c)**.
