use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::lock_payload;
use crate::protocol::opcode::{
    LOCK_AXIS_WHEEL, LOCK_AXIS_X, LOCK_AXIS_Y, LOCK_CLS_AXIS, LOCK_CLS_BTN, LOCK_CLS_KEY,
    LOCK_CLS_MEDIA, LOCK_ID_ALL, LOCK_SCALE_BLOCK, LOCK_SCALE_PASS,
};
use crate::types::{Axis, Blanket, Direction, LockTarget};

use super::Device;

impl Device {
    fn send_lock(&self, class: u8, id: u16, direction: Direction, scale: u8) -> Result<()> {
        let dir = direction.as_u8();
        self.link
            .desired()
            .lock()
            .apply_lock((class, id, dir), scale);
        self.link
            .send(FrameType::Lock, &lock_payload(class, id, dir, scale))
    }

    /// `LOCK` weighs physical input on a target while host injection still drives it; reverts on
    /// control-PC silence.
    ///
    /// `scale` is the percent of the physical value the box keeps on that direction:
    /// [`LOCK_SCALE_BLOCK`] blocks it, [`LOCK_SCALE_PASS`] passes it untouched, and above that
    /// amplifies, to [`LOCK_SCALE_MAX`](crate::LOCK_SCALE_MAX) = 2.55x.
    /// [`lock`](Self::lock) and [`unlock`](Self::unlock) are the two ends of this one number.
    ///
    /// A delta picks up at most two scales, its absolute direction's and its relative direction's, and
    /// they multiply: a `Negative` of 50 with an `Against` of 40 lands leftward-while-injecting-right at
    /// 20%. A block anywhere therefore wins outright.
    ///
    /// [`Direction::Both`] addresses the whole target, writing the scale to the two absolute directions
    /// and a full pass to the two relative ones. Writing it to all four would square it, so a plain
    /// `Both` of 50 would mean 50% with no bearing live and 25% with one. Name a relative direction to
    /// weigh it.
    ///
    /// [`Direction::With`] and [`Direction::Against`] are measured against the bearing and do nothing
    /// until one is live; see [`set_bearing`](Self::set_bearing). A momentary usage carries one bit, so
    /// any scale below a full pass locks it and there is nothing in between.
    ///
    /// ```no_run
    /// # use medius::{Axis, Device, Direction, Result};
    /// # fn main() -> Result<()> {
    /// let device = Device::find()?;
    /// device.scale(Axis::X, Direction::Against, 40)?;   // 40% of physical opposing the aim
    /// device.scale(Axis::X, Direction::With, 130)?;     // 130% of physical helping it
    /// # Ok(()) }
    /// ```
    pub fn scale(
        &self,
        target: impl Into<LockTarget>,
        direction: Direction,
        scale: u8,
    ) -> Result<()> {
        let (class, id) = target_class_id(target.into());
        self.send_lock(class, id, direction, scale)
    }

    /// `LOCK` blocks physical input on a target while host injection still drives it; reverts on
    /// control-PC silence. The same as [`scale`](Self::scale) at [`LOCK_SCALE_BLOCK`].
    pub fn lock(&self, target: impl Into<LockTarget>, direction: Direction) -> Result<()> {
        self.scale(target, direction, LOCK_SCALE_BLOCK)
    }

    /// Release a lock on the given target/direction, back to passing untouched. The same as
    /// [`scale`](Self::scale) at [`LOCK_SCALE_PASS`].
    ///
    /// [`Direction::Both`] clears every direction of the target, the relative pair included, so an
    /// unlock never walks away from a bearing scale that would go on weighing unseen. It is total in a
    /// way a `Both` at any other scale is not: only a full pass reaches the relative pair.
    pub fn unlock(&self, target: impl Into<LockTarget>, direction: Direction) -> Result<()> {
        self.scale(target, direction, LOCK_SCALE_PASS)
    }

    /// `LOCK` a relative axis by sign; convenience for `lock(axis, direction)`.
    pub fn lock_axis(&self, axis: Axis, direction: Direction) -> Result<()> {
        self.lock(axis, direction)
    }

    /// Release an axis lock.
    pub fn unlock_axis(&self, axis: Axis, direction: Direction) -> Result<()> {
        self.unlock(axis, direction)
    }

    /// Weigh a relative axis by sign; convenience for `scale(axis, direction, scale)`.
    pub fn scale_axis(&self, axis: Axis, direction: Direction, scale: u8) -> Result<()> {
        self.scale(axis, direction, scale)
    }

    /// `LOCK` a whole [`Blanket`] group (the aim, the wheel, or every button / key / media usage).
    pub fn lock_all(&self, what: Blanket, direction: Direction) -> Result<()> {
        self.blanket(what, direction, LOCK_SCALE_BLOCK)
    }

    /// Release a blanket lock.
    pub fn unlock_all(&self, what: Blanket, direction: Direction) -> Result<()> {
        self.blanket(what, direction, LOCK_SCALE_PASS)
    }

    /// Weigh a whole [`Blanket`] group; see [`scale`](Self::scale) for what the number means.
    pub fn scale_all(&self, what: Blanket, direction: Direction, scale: u8) -> Result<()> {
        self.blanket(what, direction, scale)
    }

    fn blanket(&self, what: Blanket, direction: Direction, scale: u8) -> Result<()> {
        match what {
            Blanket::Aim => {
                self.send_lock(LOCK_CLS_AXIS, LOCK_AXIS_X, direction, scale)?;
                self.send_lock(LOCK_CLS_AXIS, LOCK_AXIS_Y, direction, scale)
            }
            Blanket::Wheel => self.send_lock(LOCK_CLS_AXIS, LOCK_AXIS_WHEEL, direction, scale),
            Blanket::Buttons => self.send_lock(LOCK_CLS_BTN, LOCK_ID_ALL, direction, scale),
            Blanket::Keys => self.send_lock(LOCK_CLS_KEY, LOCK_ID_ALL, direction, scale),
            Blanket::Media => self.send_lock(LOCK_CLS_MEDIA, LOCK_ID_ALL, direction, scale),
        }
    }
}

fn target_class_id(target: LockTarget) -> (u8, u16) {
    match target {
        LockTarget::Axis(a) => (LOCK_CLS_AXIS, a.as_u16()),
        LockTarget::Usage(u) => u.class_id(),
    }
}
