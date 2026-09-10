use std::time::Duration;

use crate::error::{Error, Result};
use crate::protocol::FrameType;
use crate::protocol::command::raw_payload;
use crate::types::{Direction, Setup, TransferOutcome, TransferStatus};

use super::Device;

/// How long a [`transfer`](Device::transfer) waits for the device's answer before
/// [`Error::QueryTimeout`](crate::Error::QueryTimeout). A control transfer to a real device can be
/// slower than a box-local query, so this is longer than [`DEFAULT_QUERY_TIMEOUT`](crate::DEFAULT_QUERY_TIMEOUT).
pub const DEFAULT_TRANSFER_TIMEOUT: Duration = Duration::from_millis(1500);

impl Device {
    /// Return [`Error::ImperfectRequired`] unless the box reports the imperfect-clone opt-in on. The
    /// developer layer (§3.14) is admitted by that opt-in and nothing else; a frame sent with it off is
    /// silently dropped box-side, so the crate reads the state first and turns that into a real error.
    pub(crate) fn require_imperfect(&self) -> Result<()> {
        if self.query_imperfect()?.allowed {
            Ok(())
        } else {
            Err(Error::ImperfectRequired)
        }
    }

    /// `RAW` (§3.14): put `bytes` verbatim on cloned endpoint number `ep` in `direction`, fire-and-forget.
    ///
    /// `ep` is the bare endpoint number (0 to 15); `direction` names the flow. [`Direction::IN`] emits
    /// toward the game PC; [`Direction::OUT`] relays to the real device. Only those two are addressable:
    /// [`Direction::Both`] returns [`Error::RawDirection`] and the bearing-relative pair returns
    /// [`Error::RelativeDirection`], both before any frame goes out. The write is stateless and one-shot:
    /// the next native report on that endpoint carries the device's own state, not the raw one, and
    /// `RAW` bypasses the [rewrite rules](Device::set_rewrite). An interrupt payload past the endpoint's
    /// `wMaxPacketSize` is dropped box-side; a bulk transfer splits at the packet size and terminates
    /// with a short packet.
    ///
    /// Gated on [`allow_imperfect_clones`](Device::allow_imperfect_clones): with the opt-in off this
    /// returns [`Error::ImperfectRequired`] rather than sending a frame the box would silently drop.
    ///
    /// ```no_run
    /// # use medius::{Device, Direction, Result};
    /// # fn main() -> Result<()> {
    /// let device = Device::find()?;
    /// device.allow_imperfect_clones(true)?;
    /// device.raw(1, Direction::IN, &[0x00, 0x01, 0x00, 0x00])?;   // one report on interrupt-IN endpoint 1
    /// # Ok(()) }
    /// ```
    pub fn raw(&self, ep: u8, direction: Direction, bytes: &[u8]) -> Result<()> {
        validate_raw_direction(direction)?;
        self.require_imperfect()?;
        self.raw_frame(ep, direction, bytes)
    }

    /// The `RAW` send with no opt-in pre-check, so the async wrapper can gate on the async query path.
    pub(crate) fn raw_frame(&self, ep: u8, direction: Direction, bytes: &[u8]) -> Result<()> {
        self.link
            .send(FrameType::Raw, &raw_payload(ep, direction, bytes))
    }

    /// `TRANSFER` (§3.14): run one control transfer against the real device and return its answer.
    ///
    /// `ep` is 0 for EP0 or a control endpoint the device declares. `setup` is the eight-byte USB setup
    /// packet; `out` is the OUT data stage (empty for an IN transfer). The returned [`TransferOutcome`]
    /// carries the [`status`](TransferOutcome::status) and any IN data: a status other than
    /// [`Ok`](TransferStatus::Ok) is a real protocol result, not a link error, so it is returned rather
    /// than raised, and the surrounding `Ok` means the box answered at all.
    ///
    /// This is admitted by [`allow_imperfect_clones`](Device::allow_imperfect_clones): with the opt-in
    /// off the box answers [`Refused`](TransferStatus::Refused) rather than reaching the device. It
    /// rides its own inter-chip link pair, never the game PC's EP0 proxy, and is single-outstanding.
    /// Uses [`DEFAULT_TRANSFER_TIMEOUT`]; see [`transfer_timeout`](Device::transfer_timeout) to choose.
    ///
    /// ```no_run
    /// # use medius::{Device, Result, Setup, TransferStatus};
    /// # fn main() -> Result<()> {
    /// let device = Device::find()?;
    /// device.allow_imperfect_clones(true)?;
    /// // GET_DESCRIPTOR(Device): standard device-to-host request for the 18-byte device descriptor.
    /// let reply = device.transfer(0, Setup::new(0x80, 0x06, 0x0100, 0x0000, 18), &[])?;
    /// if reply.status == TransferStatus::Ok {
    ///     println!("device descriptor: {:02x?}", reply.data());
    /// }
    /// # Ok(()) }
    /// ```
    pub fn transfer(&self, ep: u8, setup: Setup, out: &[u8]) -> Result<TransferOutcome> {
        self.transfer_timeout(ep, setup, out, DEFAULT_TRANSFER_TIMEOUT)
    }

    /// [`transfer`](Device::transfer) with an explicit reply timeout.
    ///
    /// The box gives up on a control transfer after its own ~800 ms window, so a `timeout` shorter than
    /// that abandons the wait before a slow device would answer and, only if 256 further transfers to the
    /// same `ep` then wrap the sequence number inside that window, could let a late answer correlate to a
    /// later transfer. Keep it at or above the box window; [`DEFAULT_TRANSFER_TIMEOUT`] does.
    pub fn transfer_timeout(
        &self,
        ep: u8,
        setup: Setup,
        out: &[u8],
        timeout: Duration,
    ) -> Result<TransferOutcome> {
        let (status, data) = self.link.transfer(ep, setup, out, timeout)?;
        Ok(TransferOutcome {
            status: TransferStatus::from_u8(status),
            data,
        })
    }
}

/// A raw injection goes on one endpoint flow, so only [`Direction::IN`] and [`Direction::OUT`] address
/// one. The bearing-relative pair is measured at emit time, which a raw write has none of, and
/// [`Direction::Both`] names two flows at once; both are refused before the wire rather than sent as a
/// frame the box would resolve to OUT.
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
