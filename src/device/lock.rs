use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::lock_payload;
use crate::types::{LockDirection, LockTarget};

use super::Device;

impl Device {
    /// `LOCK` — lock an axis or button edge so physical input is masked. Fire-and-forget.
    pub fn lock(&self, target: LockTarget, direction: LockDirection) -> Result<()> {
        self.link.send(
            FrameType::Lock,
            &lock_payload(target.as_u8(), direction.as_u8(), 1),
        )
    }

    /// `LOCK` — release a previously locked axis or button edge. Fire-and-forget.
    pub fn unlock(&self, target: LockTarget, direction: LockDirection) -> Result<()> {
        self.link.send(
            FrameType::Lock,
            &lock_payload(target.as_u8(), direction.as_u8(), 0),
        )
    }
}
