use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::key_payload;
use crate::types::{Action, Key};

use super::Device;

impl Device {
    /// `KEY` — set an injection override for one keyboard key. A modifier (usage `0xE0..=0xE7`) folds
    /// into the report's modifier byte; every other key fills a keycode slot, merged with the user's
    /// real typing. Present-gated: a key the cloned board cannot report is a silent no-op.
    pub fn key(&self, key: Key, action: Action) -> Result<()> {
        self.link.desired().lock().apply_key(key, action);
        self.link
            .send(FrameType::Key, &key_payload(key.usage(), action.as_u8()))
    }

    /// Press (hold down) a key.
    pub fn key_down(&self, key: Key) -> Result<()> {
        self.key(key, Action::Press)
    }

    /// Soft-release a key — clears our injected press; a physical hold is left intact.
    pub fn key_up(&self, key: Key) -> Result<()> {
        self.key(key, Action::SoftRelease)
    }

    /// Force-release a key — masks a physical hold too.
    pub fn key_force_release(&self, key: Key) -> Result<()> {
        self.key(key, Action::ForceRelease)
    }
}
