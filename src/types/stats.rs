//! Decoded `RESP(STATS)`: delivery and telemetry counters (§4.6).

/// Firmware delivery and telemetry counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Stats {
    /// Pure-injection reports emitted (the no-halving / 1 kHz path).
    pub inject_emits: u32,
    /// Input reports never emitted to the game PC; should stay 0.
    pub tx_drops: u16,
    /// Backed-up reports merged instead of queued.
    pub tx_merges: u16,
    /// Deepest the TX queue has reached.
    pub tx_maxdepth: u8,
    /// Wedged-endpoint recoveries by the watchdog.
    pub tx_wedges: u8,
    /// Remote wakeups signalled to the PC: for an injection, or one from the real device.
    pub wakeups: u16,
    /// USB bus resets seen.
    pub reset_count: u16,
    /// `SET_CONFIGURATION` events (re-enumerations).
    pub config_count: u16,
    /// Input-carrying frames the device chip could not take off the inter-chip link: each a mouse
    /// report or injected delta never emitted. Should stay 0.
    pub link_rx_drops: u32,
    /// Same count for the host chip, relayed over the link.
    pub host_rx_drops: u32,
    /// Relayed traffic and commands that went no further, never native input.
    pub relay_drops: u32,
    /// Times the box released host-set session state (locks, held input, subscriptions, rules,
    /// transforms, the clip, an LED override). 0 at boot; wraps, so compare for inequality. The crate
    /// watches it and re-sends all of that state except the LED override.
    pub session: u16,
    /// Restarts of the host chip since the device chip booted.
    pub host_restarts: u16,
}

impl Stats {
    /// Decodes a `RESP(STATS)` payload (§4.6).
    pub(crate) fn from_payload(p: &[u8]) -> Option<Self> {
        if p.len() < 33 {
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
            link_rx_drops: u32::from_le_bytes([p[17], p[18], p[19], p[20]]),
            host_rx_drops: u32::from_le_bytes([p[21], p[22], p[23], p[24]]),
            relay_drops: u32::from_le_bytes([p[25], p[26], p[27], p[28]]),
            session: u16::from_le_bytes([p[29], p[30]]),
            host_restarts: u16::from_le_bytes([p[31], p[32]]),
        })
    }
}
