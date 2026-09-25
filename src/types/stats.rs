//! Decoded `RESP(STATS)`: delivery and telemetry counters (§4.6).

/// Delivery and telemetry counters the firmware maintains (§4.6).
///
/// The narrowed fields saturate, so a maxed counter never wraps to a small value. The three drop
/// counters are full width and do not saturate: a count that stopped rising would stop saying that
/// the loss is still going on. [`session`](Self::session) wraps for the same reason.
///
/// The ones to act on are [`tx_drops`](Self::tx_drops), [`link_rx_drops`](Self::link_rx_drops) and
/// [`host_rx_drops`](Self::host_rx_drops): each of those is the player's own input going missing,
/// and each should read 0. [`relay_drops`](Self::relay_drops) is back-pressure on a relayed stream,
/// which carries no input and is expected under load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Stats {
    /// Pure-injection reports emitted (the no-halving / 1 kHz path).
    pub inject_emits: u32,
    /// Reports the clone's TX queue could not hold: the player's own input, which the game PC
    /// never saw; it should stay 0.
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
    /// Input-carrying frames the chip that serves the clone could not take off the inter-chip link.
    /// One lost there is a mouse report or an injected delta that never reached the wire; it should
    /// stay 0.
    pub link_rx_drops: u32,
    /// The same count for the chip that reads the real device, relayed over the link.
    pub host_rx_drops: u32,
    /// Back-pressure on a relayed stream, either direction: a vendor IN packet the PC is not
    /// draining, or an OUT packet past the relay's one-per-frame ceiling. Expected under load, and
    /// counted apart from `tx_drops` because nothing of the player's input goes missing with it.
    pub relay_drops: u32,
    /// The times the box released the session state a host set (locks, held input, subscriptions,
    /// rules, transforms, the clip, an LED override). It wraps, so compare it for inequality; 0 at
    /// boot. The crate watches it and re-sends what it holds, which is all of that but the LED.
    pub session: u16,
}

impl Stats {
    /// Decode a `RESP(STATS)` payload (§4.6).
    pub(crate) fn from_payload(p: &[u8]) -> Option<Self> {
        if p.len() < 31 {
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
        })
    }
}
