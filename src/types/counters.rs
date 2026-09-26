//! Snapshot of the always-on link counters.

/// Copyable snapshot of the always-on link counters, for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CountersSnapshot {
    /// Frames written to the transport.
    pub frames_tx: u64,
    /// Frames decoded from the transport.
    pub frames_rx: u64,
    /// Frames dropped for a failed CRC.
    pub crc_drops: u64,
    /// Successful reconnects.
    pub reconnects: u64,
    /// Device-chip restarts recovered by re-sending the held state.
    pub restarts: u64,
}
