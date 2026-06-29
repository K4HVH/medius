//! C ABI for the [`medius`] host control library.
//!
//! This crate is the only `unsafe` layer in the stack. It defines `#[repr(C)]` mirror types and
//! `extern "C"` functions over the safe [`medius`] crate, converting at the boundary. The generated
//! header is `include/medius.h` (cbindgen). See `docs/superpowers/specs` in the crate for the design.

#![allow(clippy::missing_safety_doc)]

mod convert;
mod ctypes;
mod device;
mod error;
mod helpers;
mod keys;
mod stream;

#[cfg(feature = "flash")]
mod flash;
#[cfg(feature = "mock")]
mod mock;

#[cfg(test)]
mod tests;

pub use ctypes::*;
pub use device::*;
pub use error::*;
pub use helpers::*;
pub use keys::*;
pub use stream::*;

#[cfg(feature = "flash")]
pub use flash::*;
#[cfg(feature = "mock")]
pub use mock::*;
