use std::time::Duration;

use crate::error::{Error, Result};
use crate::protocol::FrameType;
use crate::protocol::command::raw_payload;
use crate::types::{Direction, Setup, TransferOutcome};

use super::Device;

/// Default wait on the real device for a [`transfer`](Device::transfer).
pub const DEFAULT_TRANSFER_TIMEOUT: Duration = Duration::from_millis(1000);

impl Device {
    // The box drops an advanced-layer (§3.14) frame with no reply while the opt-in is off, so the
    // config-rate setters check first. `raw` does not: it runs per report, and a query costs a round
    // trip.
    pub(crate) fn require_imperfect(&self) -> Result<()> {
        if self.query_imperfect()?.allowed {
            Ok(())
        } else {
            Err(Error::ImperfectRequired)
        }
    }

    /// `RAW`: put `bytes` verbatim on cloned endpoint `ep` in `direction`; fire-and-forget.
    pub fn raw(&self, ep: u8, direction: Direction, bytes: &[u8]) -> Result<()> {
        validate_raw_direction(direction)?;
        self.link
            .send(FrameType::Raw, &raw_payload(ep, direction, bytes))
    }

    /// `TRANSFER`: run one control transfer against the real device.
    pub fn transfer(&self, ep: u8, setup: Setup, out: &[u8]) -> Result<TransferOutcome> {
        self.transfer_timeout(ep, setup, out, DEFAULT_TRANSFER_TIMEOUT)
    }

    /// [`transfer`](Device::transfer) with the box's wait on the real device set to `timeout`.
    pub fn transfer_timeout(
        &self,
        ep: u8,
        setup: Setup,
        out: &[u8],
        timeout: Duration,
    ) -> Result<TransferOutcome> {
        let timeout_ms = transfer_timeout_ms(timeout)?;
        validate_transfer_data(setup, out)?;
        self.link.transfer(ep, timeout_ms, setup, out)
    }
}

pub(crate) fn transfer_timeout_ms(timeout: Duration) -> Result<u16> {
    match u16::try_from(timeout.as_millis()) {
        Ok(ms) if ms > 0 => Ok(ms),
        _ => Err(Error::TransferTimeoutRange { timeout }),
    }
}

pub(crate) fn validate_transfer_data(setup: Setup, out: &[u8]) -> Result<()> {
    let want = if setup.is_in() {
        0
    } else {
        setup.length as usize
    };
    if out.len() == want {
        Ok(())
    } else {
        Err(Error::TransferData {
            want,
            got: out.len(),
        })
    }
}

// Only IN and OUT name one endpoint flow. The bearing-relative pair needs an emit time, which a raw
// write lacks, and `Both` names two flows; the box would resolve either to OUT.
pub(crate) fn validate_raw_direction(direction: Direction) -> Result<()> {
    match direction {
        Direction::Positive | Direction::Negative => Ok(()),
        d if d.is_relative() => Err(Error::RelativeDirection {
            direction: d,
            what: "raw endpoint",
        }),
        d => Err(Error::RawDirection { direction: d }),
    }
}
