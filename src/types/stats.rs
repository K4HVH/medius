//! Decoded `RESP(STATS)`: delivery and telemetry counters (§4.6).

/// Firmware delivery and telemetry counters (§4.6).
///
/// Narrowed fields saturate, so a maxed counter never wraps to a small value. The three drop
/// counters are full width and never saturate: a count that stopped rising would hide ongoing loss.
/// [`session`](Self::session) wraps for the same reason.
///
/// Act on [`tx_drops`](Self::tx_drops), [`link_rx_drops`](Self::link_rx_drops) and
/// [`host_rx_drops`](Self::host_rx_drops): each counts lost input and should read 0.
/// [`relay_drops`](Self::relay_drops) counts relayed traffic and commands that went no further, never
/// native input. A rise across a burst of commands can mean some never reached the box. A lost
/// release can leave a key held. [`reapply`](crate::Device::reapply) re-sends only what is held, so
/// send the releases again or call [`reset`](crate::Device::reset).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Stats {
    /// Pure-injection reports emitted (the no-halving / 1 kHz path).
    pub inject_emits: u32,
    /// Reports the clone's TX queue could not hold, never emitted to the game PC. Should stay 0.
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
    /// Relayed traffic and commands that went no further. Counted apart from `tx_drops` because no
    /// native input goes missing with it. Each of these counts:
    ///
    /// - a vendor IN packet that found its endpoint's queue full of the control PC's `RAW` packets
    /// - a native zero-length packet answering a poll the PC did not make (the one poll the box runs
    ///   ahead of a suspended PC)
    /// - an OUT packet the box could not queue, that failed 3 times on the real device's bus, or that
    ///   a bus reset or `SET_INTERFACE` overtook
    /// - a `RAW` OUT packet with no room to wait
    /// - a relayed request or a flagged `MOVE` the box could not queue
    /// - any frame other than native input that either chip's link receive ring dropped
    /// - a frame from the control PC the box's receive ring had no room for (1 per frame)
    /// - an overrun of the control port's UART (1 per overrun, however many frames it cost)
    pub relay_drops: u32,
    /// Times the box released host-set session state (locks, held input, subscriptions, rules,
    /// transforms, the clip, an LED override). 0 at boot; wraps, so compare for inequality. The crate
    /// watches it and re-sends all of that state except the LED override.
    pub session: u16,
}

impl Stats {
    /// Decodes a `RESP(STATS)` payload (§4.6).
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
