//! Media-key command vocabulary: a media key by 16-bit Consumer usage, and the media catch snapshot.

/// A media key, addressed by 16-bit HID Consumer Usage (§3.11, v2.0.0).
///
/// Construct from a raw usage with [`MediaKey::new`], or use a constant. Present-gated: a key the
/// cloned board does not declare is a silent no-op, so check [`KbdCaps::has_consumer`](crate::KbdCaps).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MediaKey(pub u16);

impl MediaKey {
    /// A media key from a raw 16-bit Consumer usage.
    pub const fn new(usage: u16) -> MediaKey {
        MediaKey(usage)
    }

    /// The Consumer usage value.
    pub const fn usage(self) -> u16 {
        self.0
    }

    pub const PLAY_PAUSE: MediaKey = MediaKey(0xCD);
    pub const STOP: MediaKey = MediaKey(0xB7);
    pub const NEXT_TRACK: MediaKey = MediaKey(0xB5);
    pub const PREV_TRACK: MediaKey = MediaKey(0xB6);
    pub const MUTE: MediaKey = MediaKey(0xE2);
    pub const VOLUME_UP: MediaKey = MediaKey(0xE9);
    pub const VOLUME_DOWN: MediaKey = MediaKey(0xEA);
    pub const PLAY: MediaKey = MediaKey(0xB0);
    pub const PAUSE: MediaKey = MediaKey(0xB1);
}

/// A media catch snapshot — a `CONS_EVENT` frame (§4.13, v2.0.0).
///
/// Carries the active Consumer usages (one at a time on a typical board). Self-correcting like the
/// keyboard snapshot: diff successive snapshots for press/release edges.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MediaEvent {
    /// The currently-active media usages, ascending.
    pub keys: Vec<MediaKey>,
}

impl MediaEvent {
    /// Decode a `CONS_EVENT` payload: `[n u8][usage u16 LE × n]`.
    pub(crate) fn from_payload(p: &[u8]) -> Option<MediaEvent> {
        let n = *p.first()? as usize;
        let mut keys = Vec::with_capacity(n);
        for i in 0..n {
            let lo = *p.get(1 + 2 * i)?;
            let hi = *p.get(2 + 2 * i)?;
            keys.push(MediaKey(u16::from_le_bytes([lo, hi])));
        }
        Some(MediaEvent { keys })
    }

    /// Whether `key` is active in this snapshot.
    pub fn is_pressed(&self, key: MediaKey) -> bool {
        self.keys.contains(&key)
    }
}
