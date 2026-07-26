//! Serial transport for the control link.

use std::time::Duration;

pub(crate) const CTRL_BAUD: u32 = 4_000_000;

// Bounds a parked read so the reader thread stays responsive to stop and reconnect.
const IO_TIMEOUT: Duration = Duration::from_millis(100);

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::SerialTransport;

#[cfg(not(windows))]
mod portable;
#[cfg(not(windows))]
pub(crate) use portable::SerialTransport;
