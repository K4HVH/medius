use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::lock_payload;
use crate::protocol::opcode::{
    LOCK_AXIS_WHEEL, LOCK_AXIS_X, LOCK_AXIS_Y, LOCK_CLS_AXIS, LOCK_CLS_BTN, LOCK_CLS_KEY,
    LOCK_CLS_MEDIA, LOCK_ID_ALL,
};
use crate::types::{Axis, Blanket, Direction, LockTarget};

use super::Device;

impl Device {
    fn send_lock(&self, class: u8, id: u16, direction: Direction, on: bool) -> Result<()> {
        let dir = direction.as_u8();
        self.link.desired().lock().apply_lock((class, id, dir), on);
        self.link
            .send(FrameType::Lock, &lock_payload(class, id, dir, u8::from(on)))
    }

    /// `LOCK` blocks physical input on a target while host injection still drives it; reverts on control-PC silence.
    pub fn lock(&self, target: impl Into<LockTarget>, direction: Direction) -> Result<()> {
        let (class, id) = target_class_id(target.into());
        self.send_lock(class, id, direction, true)
    }

    /// Release a lock on the given target/direction.
    pub fn unlock(&self, target: impl Into<LockTarget>, direction: Direction) -> Result<()> {
        let (class, id) = target_class_id(target.into());
        self.send_lock(class, id, direction, false)
    }

    /// `LOCK` a relative axis by sign; convenience for `lock(axis, direction)`.
    pub fn lock_axis(&self, axis: Axis, direction: Direction) -> Result<()> {
        self.lock(axis, direction)
    }

    /// Release an axis lock.
    pub fn unlock_axis(&self, axis: Axis, direction: Direction) -> Result<()> {
        self.unlock(axis, direction)
    }

    /// `LOCK` a whole [`Blanket`] group (the aim, the wheel, or every button / key / media usage).
    pub fn lock_all(&self, what: Blanket, direction: Direction) -> Result<()> {
        self.blanket(what, direction, true)
    }

    /// Release a blanket lock.
    pub fn unlock_all(&self, what: Blanket, direction: Direction) -> Result<()> {
        self.blanket(what, direction, false)
    }

    fn blanket(&self, what: Blanket, direction: Direction, on: bool) -> Result<()> {
        match what {
            Blanket::Aim => {
                self.send_lock(LOCK_CLS_AXIS, LOCK_AXIS_X, direction, on)?;
                self.send_lock(LOCK_CLS_AXIS, LOCK_AXIS_Y, direction, on)
            }
            Blanket::Wheel => self.send_lock(LOCK_CLS_AXIS, LOCK_AXIS_WHEEL, direction, on),
            Blanket::Buttons => self.send_lock(LOCK_CLS_BTN, LOCK_ID_ALL, direction, on),
            Blanket::Keys => self.send_lock(LOCK_CLS_KEY, LOCK_ID_ALL, direction, on),
            Blanket::Media => self.send_lock(LOCK_CLS_MEDIA, LOCK_ID_ALL, direction, on),
        }
    }
}

fn target_class_id(target: LockTarget) -> (u8, u16) {
    match target {
        LockTarget::Axis(a) => (LOCK_CLS_AXIS, a.as_u16()),
        LockTarget::Usage(u) => u.class_id(),
    }
}
