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
