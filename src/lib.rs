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
pub use device::clip::ClipHandle;
pub use device::discover::BoxInfo;
pub use device::logs::LogStream;
pub use device::options::{EMIT_MAX_HZ, NAME_MAX};
pub use error::{Error, Result};
pub use link::{DEFAULT_KEEPALIVE_CADENCE, DEFAULT_QUERY_TIMEOUT};
/// The control-protocol version this build speaks. A box reporting anything else is refused at the
/// handshake; exposing it lets a caller say so in its own words before connecting.
pub use protocol::PROTO_VER;
pub use protocol::{DecodedFrame, FrameType};
pub use transport::scan::find_medius;
pub use types::{
    Action, Axis, Blanket, BusEvent, Button, CLIP_EDGES_MAX, Caps, CatchClass, CatchEntry,
    CatchEvent, CatchFilter, CatchState, Class, ClipAction, ClipBuilder, ClipSettings, ClipState,
    ClipStatus, ClipTrigger, ClockDomain, ClockEstimate, ControlStatus, CountersSnapshot,
    DeviceInfo, DeviceKind, Edge, EmitPace, EmitPaceStatus, Health, ImperfectStatus, KbdCaps, Key,
    LedMode, LedTarget, LockDirection, LockEntry, LockScope, LockTarget, Locks, LogLevel, LogLine,
    MediaKey, Motion, MotionEvent, MouseCaps, PortInfo, Rate, RebootTarget, Stats, TrafficEvent,
    Usage, UsageSnapshot, Version,
};

#[cfg(feature = "async")]
pub use device::asyncv::{AsyncClipHandle, AsyncDevice};
#[cfg(feature = "mock")]
pub use mock::MockBox;
