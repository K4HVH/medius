use crate::error::{Error, Result};
use crate::protocol::FrameType;
use crate::protocol::command::lock_payload;
use crate::protocol::opcode::{
    LOCK_AXIS_WHEEL, LOCK_AXIS_X, LOCK_AXIS_Y, LOCK_CLS_AXIS, LOCK_CLS_BTN, LOCK_CLS_KEY,
    LOCK_CLS_MEDIA, LOCK_ID_ALL, LOCK_SCALE_BLOCK, LOCK_SCALE_MAX, LOCK_SCALE_MIN, LOCK_SCALE_PASS,
};
use crate::types::{Axis, Blanket, Direction, LockTarget};

use super::Device;

impl Device {
    fn send_lock(&self, class: u8, id: u16, direction: Direction, scale: i16) -> Result<()> {
        if !(LOCK_SCALE_MIN..=LOCK_SCALE_MAX).contains(&scale) {
            return Err(Error::LockScaleRange {
                scale,
                min: LOCK_SCALE_MIN,
                max: LOCK_SCALE_MAX,
            });
        }
        // One bit has nothing to reverse.
        if scale < 0 && class != LOCK_CLS_AXIS {
            return Err(Error::LockScaleUsage { scale, class });
        }
        let dir = lock_direction(class, direction)?.as_u8();
        // Serialised against recovery re-sends, or a stale lock can land after this one. Recorded
        // before the write so a racing reconnect replays it; rolled back if the frame never went out,
        // or the keepalive stays open for a lock nothing applies.
        let _serial = self.link.reassert_guard();
        let undo = self
            .link
            .desired()
            .lock()
            .apply_lock((class, id, dir), scale);
        let sent = self
            .link
            .send(FrameType::Lock, &lock_payload(class, id, dir, scale));
        if sent.is_err() {
            self.link.desired().lock().restore_lock(undo);
        }
        sent
    }

    /// `LOCK`: weigh physical input on a target while host injection still drives it; reverts on
    /// control-PC silence.
    ///
    /// `scale` is the percent of the physical value kept on that direction: [`LOCK_SCALE_BLOCK`]
    /// blocks, [`LOCK_SCALE_PASS`] passes untouched, and above that amplifies, up to
    /// [`LOCK_SCALE_MAX`](crate::LOCK_SCALE_MAX) = 2.55x. [`lock`](Self::lock) and
    /// [`unlock`](Self::unlock) are its two ends.
    ///
    /// Signed down to [`LOCK_SCALE_MIN`](crate::LOCK_SCALE_MIN): a negative scale weighs the physical
    /// value and reverses it, so `-100` inverts. The slot is picked by the delta's sign before the
    /// weigh, so a `Positive` of `-100` turns rightward motion leftward and leaves leftward motion
    /// alone. Axes only: [`Error::LockScaleUsage`](crate::Error::LockScaleUsage) otherwise, and
    /// [`Error::LockScaleRange`](crate::Error::LockScaleRange) outside the range.
    ///
    /// A delta takes at most two scales, its absolute direction's and its relative direction's, and
    /// they multiply: a `Negative` of 50 with an `Against` of 40 puts leftward motion, while injecting
    /// rightward, at 20%. A block in either zeroes the product.
    ///
    /// [`Direction::Both`] writes the scale to the two absolute directions and a full pass to the two
    /// relative ones; writing all four would square it (50% with no bearing, 25% with one). Name a
    /// relative direction to weigh it.
    ///
    /// [`Direction::With`] and [`Direction::Against`] are measured against the bearing and do nothing
    /// until one is live; see [`set_bearing`](Self::set_bearing). Only an axis has a bearing, so a
    /// relative direction on a button, key or media usage is
    /// [`Error::RelativeDirection`](crate::Error::RelativeDirection) instead of a frame the box
    /// discards. A momentary usage carries one bit: any scale below a full pass locks it, and a full
    /// pass or more unlocks it.
    ///
    /// A media usage has no edges (it is suppressed whole), so an edge direction on one is sent, and
    /// reported by `RESP(LOCKS)`, as [`Direction::Both`].
    ///
    /// ```no_run
    /// # use medius::{Axis, Device, Direction, Result};
    /// # fn main() -> Result<()> {
    /// let device = Device::find()?;
    /// device.scale(Axis::X, Direction::Against, 40)?;   // 40% of physical motion opposing the injection
    /// device.scale(Axis::X, Direction::With, 130)?;     // 130% of physical motion along it
    /// # Ok(()) }
    /// ```
    pub fn scale(
        &self,
        target: impl Into<LockTarget>,
        direction: Direction,
        scale: i16,
    ) -> Result<()> {
        let (class, id) = LockTarget::class_id(target.into());
        self.send_lock(class, id, direction, scale)
    }

    /// `LOCK`: block physical input on a target while host injection still drives it; reverts on
    /// control-PC silence. [`scale`](Self::scale) at [`LOCK_SCALE_BLOCK`].
    pub fn lock(&self, target: impl Into<LockTarget>, direction: Direction) -> Result<()> {
        self.scale(target, direction, LOCK_SCALE_BLOCK)
    }

    /// Release a lock, back to passing untouched: [`scale`](Self::scale) at [`LOCK_SCALE_PASS`].
    ///
    /// [`Direction::Both`] clears every direction of the target, the relative pair included, so no
    /// bearing scale stays behind weighing unseen. Only a full pass reaches the relative pair this
    /// way; a `Both` at any other scale does not.
    pub fn unlock(&self, target: impl Into<LockTarget>, direction: Direction) -> Result<()> {
        self.scale(target, direction, LOCK_SCALE_PASS)
    }

    /// `LOCK` a relative axis by sign; `lock(axis, direction)`.
    pub fn lock_axis(&self, axis: Axis, direction: Direction) -> Result<()> {
        self.lock(axis, direction)
    }

    /// Release an axis lock.
    pub fn unlock_axis(&self, axis: Axis, direction: Direction) -> Result<()> {
        self.unlock(axis, direction)
    }

    /// Weigh a relative axis by sign; `scale(axis, direction, scale)`.
    pub fn scale_axis(&self, axis: Axis, direction: Direction, scale: i16) -> Result<()> {
        self.scale(axis, direction, scale)
    }

    /// `LOCK` a [`Blanket`] group (X and Y, the wheel, or every button / key / media usage).
    ///
    /// [`Blanket::Keys`] honours the direction: `Positive` blocks press edges only, `Negative`
    /// release edges only.
    pub fn lock_all(&self, what: Blanket, direction: Direction) -> Result<()> {
        self.blanket(what, direction, LOCK_SCALE_BLOCK)
    }

    /// Release a blanket lock.
    pub fn unlock_all(&self, what: Blanket, direction: Direction) -> Result<()> {
        self.blanket(what, direction, LOCK_SCALE_PASS)
    }

    /// Weigh a [`Blanket`] group; see [`scale`](Self::scale) for the number.
    pub fn scale_all(&self, what: Blanket, direction: Direction, scale: i16) -> Result<()> {
        self.blanket(what, direction, scale)
    }

    fn blanket(&self, what: Blanket, direction: Direction, scale: i16) -> Result<()> {
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

// Only an axis has a bearing to be with or against; the box drops a relative direction on any other
// class with no reply.
fn lock_direction(class: u8, direction: Direction) -> Result<Direction> {
    if class == LOCK_CLS_AXIS {
        return Ok(direction);
    }
    if direction.is_relative() {
        return Err(Error::RelativeDirection {
            direction,
            what: match class {
                LOCK_CLS_BTN => "button lock",
                LOCK_CLS_KEY => "key lock",
                _ => "media lock",
            },
        });
    }
    Ok(if class == LOCK_CLS_MEDIA {
        Direction::Both
    } else {
        direction
    })
}
