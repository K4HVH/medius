//! Diagnostics snapshot of the device's always-on counters.

/// A plain, copyable snapshot of the device's always-on counters, for display / diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CountersSnapshot {
    /// Total frames written to the transport.
    pub frames_tx: u64,
    /// Total frames decoded from the transport.
    pub frames_rx: u64,
    /// Total frames dropped for a failed CRC.
    pub crc_drops: u64,
    /// Total successful reconnects.
    pub reconnects: u64,
}
