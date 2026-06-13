//! Diagnostics snapshot of the device's always-on counters.

/// A plain, copyable snapshot of the device's always-on counters, for display / JSON (serde-gated).
///
/// Produced by [`Device::counters`](crate::Device::counters); the source totals are the internal
/// atomic `Counters` in the device core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

#[cfg(all(test, feature = "serde"))]
mod tests {
    use super::*;

    #[test]
    fn snapshot_serde_round_trip() {
        let s = CountersSnapshot {
            frames_tx: 10,
            frames_rx: 7,
            crc_drops: 1,
            reconnects: 2,
        };
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<CountersSnapshot>(&j).unwrap(), s);
    }
}
