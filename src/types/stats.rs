//! Decoded `RESP(STATS)`: delivery and telemetry counters (§4.6).

/// Delivery and telemetry counters the firmware maintains (§4.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Stats {
    /// Pure-injection reports emitted (the no-halving / 1 kHz path).
    pub inject_emits: u32,
    /// Reports dropped on TX-queue overflow (should stay 0).
    pub tx_drops: u16,
    /// Backed-up reports merged instead of queued.
    pub tx_merges: u16,
    /// Deepest the TX queue has reached.
    pub tx_maxdepth: u8,
    /// Wedged-endpoint recoveries by the watchdog.
    pub tx_wedges: u8,
    /// Remote-wakeups issued.
    pub wakeups: u16,
    /// USB bus resets seen.
    pub reset_count: u16,
    /// `SET_CONFIGURATION` events (re-enumerations).
    pub config_count: u16,
}

impl Stats {
    /// Decode a `RESP(STATS)` payload (§4.6).
    pub(crate) fn from_payload(p: &[u8]) -> Option<Self> {
        if p.len() < 17 {
            return None;
        }
        Some(Stats {
            inject_emits: u32::from_le_bytes([p[1], p[2], p[3], p[4]]),
            tx_drops: u16::from_le_bytes([p[5], p[6]]),
            tx_merges: u16::from_le_bytes([p[7], p[8]]),
            tx_maxdepth: p[9],
            tx_wedges: p[10],
            wakeups: u16::from_le_bytes([p[11], p[12]]),
            reset_count: u16::from_le_bytes([p[13], p[14]]),
            config_count: u16::from_le_bytes([p[15], p[16]]),
        })
    }
}
