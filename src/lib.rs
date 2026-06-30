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

#[cfg(feature = "flash")]
pub mod flash;
#[cfg(feature = "mock")]
mod mock;

#[cfg(test)]
mod tests;

pub use device::Device;
pub use device::catch::EventStream;
pub use device::logs::LogStream;
pub use error::{Error, Result};
pub use link::{DEFAULT_KEEPALIVE_CADENCE, DEFAULT_QUERY_TIMEOUT};
pub use protocol::{DecodedFrame, FrameType};
pub use transport::scan::find_medius;
pub use types::{
    Action, Blanket, Button, Caps, CatchEvent, CatchMask, CatchState, CountersSnapshot, EmitPace,
    EmitPaceStatus, Health, ImperfectStatus, Input, KbdCaps, Key, KeyboardEvent, LedMode, LedTarget,
    LockClass, LockDirection, LockTarget, Locks, LogLevel, LogLine, MediaEvent, MediaKey, Motion,
    MouseCaps, MouseEvent, MouseInfo, PortInfo, Rate, RebootTarget, Stats, Version,
};

#[cfg(feature = "async")]
pub use device::asyncv::AsyncDevice;
#[cfg(feature = "mock")]
pub use mock::MockBox;
