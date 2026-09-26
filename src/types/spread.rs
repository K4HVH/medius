//! Spread of an injected delta across the host's command interval (§4.14).

/// Configured spread percent and the interval the box releases across (§4.14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SpreadStatus {
    /// Percent of the learnt command interval. 0 is off; above 100 overlaps.
    pub percent: u16,
    /// Release interval in microseconds. 0 until the box learns the host's command period, while
    /// `percent` is 0, and before the box settles where the motion is held; in each case the whole
    /// delta goes out on the next report.
    pub span_us: u32,
}

impl SpreadStatus {
    /// Decodes a `RESP(OPTIONS, SPREAD)` payload (§4.14).
    pub(crate) fn from_payload(p: &[u8]) -> Option<SpreadStatus> {
        if p.len() < 8 {
            return None;
        }
        Some(SpreadStatus {
            percent: u16::from_le_bytes([p[2], p[3]]),
            span_us: u32::from_le_bytes([p[4], p[5], p[6], p[7]]),
        })
    }
}
