//! C ABI for the [`medius`] host control library.

#![allow(clippy::missing_safety_doc)]

mod clip;
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

pub use clip::*;
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
