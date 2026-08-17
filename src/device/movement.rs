use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::{move_cursor_payload, move_wheel_payload};
use crate::types::{Motion, MoveTiming, PendingMotion};

use super::Device;

impl Device {
    /// `MOVE` (cursor): relative cursor movement; full `i16`, no clamp.
    pub fn move_rel(&self, dx: i16, dy: i16) -> Result<()> {
        self.move_axis(
            Motion::Cursor { dx, dy },
            MoveTiming::Ride,
            PendingMotion::Keep,
        )
    }

    /// `MOVE` (wheel): vertical scroll; full `i16`, no clamp.
    pub fn wheel(&self, delta: i16) -> Result<()> {
        self.move_axis(Motion::Wheel(delta), MoveTiming::Ride, PendingMotion::Keep)
    }

    /// `MOVE` (cursor) that bypasses movement riding: emits on the box's own clock even while riding is
    /// on, and leaves motion already held for a ride held.
    pub fn move_rel_now(&self, dx: i16, dy: i16) -> Result<()> {
        self.move_axis(
            Motion::Cursor { dx, dy },
            MoveTiming::Now,
            PendingMotion::Keep,
        )
    }

    /// `MOVE` (wheel) that bypasses movement riding.
    pub fn wheel_now(&self, delta: i16) -> Result<()> {
        self.move_axis(Motion::Wheel(delta), MoveTiming::Now, PendingMotion::Keep)
    }

    /// `MOVE` (zero delta, `FLUSH`): emit the motion held for a ride now, ignoring the ride window.
    pub fn flush_motion(&self) -> Result<()> {
        self.move_axis(
            Motion::Cursor { dx: 0, dy: 0 },
            MoveTiming::Ride,
            PendingMotion::Flush,
        )
    }

    /// `MOVE` (zero delta, `DISCARD`): drop the motion held for a ride. Motion sent with
    /// [`move_rel_now`](Self::move_rel_now) is untouched.
    pub fn discard_motion(&self) -> Result<()> {
        self.move_axis(
            Motion::Cursor { dx: 0, dy: 0 },
            MoveTiming::Ride,
            PendingMotion::Discard,
        )
    }

    /// `MOVE`: drive a relative axis, choosing when this delta reaches the game PC and what happens to
    /// motion already held for a ride.
    pub fn move_axis(
        &self,
        motion: Motion,
        timing: MoveTiming,
        pending: PendingMotion,
    ) -> Result<()> {
        let flags = timing.as_u8() | pending.as_u8();
        match motion {
            Motion::Cursor { dx, dy } => self
                .link
                .send(FrameType::Move, &move_cursor_payload(dx, dy, flags)),
            Motion::Wheel(dz) => self
                .link
                .send(FrameType::Move, &move_wheel_payload(dz, flags)),
        }
    }
}
