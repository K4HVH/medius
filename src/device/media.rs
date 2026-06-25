use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::consumer_payload;
use crate::types::{Action, MediaKey};

use super::Device;

impl Device {
    /// `CONSUMER` — set an injection override for one media key by 16-bit Consumer usage, merged with
    /// the user's real media keys. Present-gated: a media key on a board with no Consumer collection
    /// is a silent no-op (check [`KbdCaps::has_consumer`](crate::KbdCaps::has_consumer)).
    pub fn media(&self, key: MediaKey, action: Action) -> Result<()> {
        self.link.desired().lock().apply_media(key, action);
        self.link.send(
            FrameType::Consumer,
            &consumer_payload(key.usage(), action.as_u8()),
        )
    }

    /// Press (hold down) a media key.
    pub fn media_down(&self, key: MediaKey) -> Result<()> {
        self.media(key, Action::Press)
    }

    /// Soft-release a media key — clears our injected press; a physical hold is left intact.
    pub fn media_up(&self, key: MediaKey) -> Result<()> {
        self.media(key, Action::SoftRelease)
    }

    /// Force-release a media key — masks a physical hold too.
    pub fn media_force_release(&self, key: MediaKey) -> Result<()> {
        self.media(key, Action::ForceRelease)
    }
}
