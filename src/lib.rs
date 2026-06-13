//! Host control library for the medius transparent mouse passthrough box.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

#[macro_use]
mod trace;

mod device;
mod error;
mod link;
pub(crate) mod protocol;
mod transport;
pub mod types;

#[cfg(feature = "async")]
mod asyncv;
#[cfg(feature = "flash")]
pub mod flash;
#[cfg(feature = "mock")]
mod mock;

#[cfg(test)]
mod tests;

pub use device::Device;
pub use device::logs::LogStream;
pub use error::{Error, Result};
pub use link::{DEFAULT_KEEPALIVE_CADENCE, DEFAULT_QUERY_TIMEOUT};
pub use protocol::{DecodedFrame, FrameType};
pub use transport::scan::find_medius;
pub use types::{
    Button, ButtonAction, CountersSnapshot, Health, LogLevel, LogLine, PortInfo, RebootTarget,
    Version,
};

#[cfg(feature = "async")]
pub use asyncv::AsyncDevice;
#[cfg(feature = "mock")]
pub use mock::MockBox;
