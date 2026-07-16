use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::lock_payload;
use crate::types::{Blanket, Key, LockClass, LockDirection, LockTarget, MediaKey};

use super::Device;

impl Device {
    fn send_lock(&self, class: LockClass, usage: u16, direction: u8, on: bool) -> Result<()> {
        self.link
            .desired()
            .lock()
            .apply_lock((class.as_u8(), usage, direction), on);
        self.link.send(
            FrameType::Lock,
            &lock_payload(class.as_u8(), usage, direction, on as u8),
        )
    }

    /// `LOCK` — block a mouse axis or button edge so physical input is masked. Injection still drives it.
    pub fn lock(&self, target: LockTarget, direction: LockDirection) -> Result<()> {
        self.send_lock(
            LockClass::Mouse,
            target.as_u8() as u16,
            direction.as_u8(),
            true,
        )
    }

    /// `LOCK` — release a previously locked mouse axis or button edge.
    pub fn unlock(&self, target: LockTarget, direction: LockDirection) -> Result<()> {
        self.send_lock(
            LockClass::Mouse,
            target.as_u8() as u16,
            direction.as_u8(),
            false,
        )
    }

    /// `LOCK` — block a physical keyboard key or modifier. A press lock stops new hand presses; a
    /// release lock latches a held key down. Injection still drives it.
    pub fn lock_key(&self, key: Key, direction: LockDirection) -> Result<()> {
        self.send_lock(LockClass::Key, key.usage() as u16, direction.as_u8(), true)
    }

    /// `LOCK` — release a previously locked keyboard key or modifier.
    pub fn unlock_key(&self, key: Key, direction: LockDirection) -> Result<()> {
        self.send_lock(LockClass::Key, key.usage() as u16, direction.as_u8(), false)
    }

    /// `LOCK` — block a physical media usage.
    pub fn lock_media(&self, key: MediaKey) -> Result<()> {
        self.send_lock(LockClass::Media, key.usage(), 0, true)
    }

    /// `LOCK` — release a previously locked media usage.
    pub fn unlock_media(&self, key: MediaKey) -> Result<()> {
        self.send_lock(LockClass::Media, key.usage(), 0, false)
    }

    /// `LOCK` — blanket-block a whole input group: the cursor aim (X+Y), the wheel, every mouse button,
    /// every key, or every media usage.
    pub fn lock_all(&self, what: Blanket) -> Result<()> {
        self.set_blanket(what, true)
    }

    /// `LOCK` — release a blanket whole-group lock.
    pub fn unlock_all(&self, what: Blanket) -> Result<()> {
        self.set_blanket(what, false)
    }

    fn set_blanket(&self, what: Blanket, on: bool) -> Result<()> {
        let both = LockDirection::Both.as_u8();
        match what.class() {
            Some(class) => self.send_lock(class, 0, 0, on),
            None => match what {
                // The axis groups have no whole-class lock: block their targets directly, both directions.
                Blanket::Aim => {
                    self.send_lock(LockClass::Mouse, LockTarget::X.as_u8() as u16, both, on)?;
                    self.send_lock(LockClass::Mouse, LockTarget::Y.as_u8() as u16, both, on)
                }
                Blanket::Wheel => {
                    self.send_lock(LockClass::Mouse, LockTarget::Wheel.as_u8() as u16, both, on)
                }
                _ => unreachable!("class() is Some for non-axis groups"),
            },
        }
    }
}
