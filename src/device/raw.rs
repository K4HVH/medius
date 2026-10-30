use std::time::Duration;

use crate::error::{Error, Result};
use crate::protocol::FrameType;
use crate::protocol::command::raw_payload;
use crate::types::{Direction, Setup, TransferOutcome, TransferStatus};

use super::Device;

/// How long a [`transfer`](Device::transfer) waits for the native reply before
/// [`Error::QueryTimeout`](crate::Error::QueryTimeout). Longer than
/// [`DEFAULT_QUERY_TIMEOUT`](crate::DEFAULT_QUERY_TIMEOUT), as a real device can be slower than a
/// box-local query.
pub const DEFAULT_TRANSFER_TIMEOUT: Duration = Duration::from_millis(1500);

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

    /// `RAW` (§3.14): put `bytes` verbatim on cloned endpoint `ep` in `direction`; fire-and-forget.
    ///
    /// `ep` is the bare endpoint number (0-15). [`Direction::IN`] emits to the game PC;
    /// [`Direction::OUT`] relays to the real device. [`Direction::Both`] gives
    /// [`Error::RawDirection`], and the bearing-relative pair gives [`Error::RelativeDirection`],
    /// before any frame is sent. The write is stateless and one-shot: the next native report on
    /// that endpoint carries native state. `RAW` bypasses the [rewrite rules](Device::set_rewrite).
    /// A raw IN report never rides a native one: on an endpoint the real device reports on every
    /// poll, it takes a separate poll, within two native reports.
    ///
    /// `bytes` is <=510 long, else [`Error::FrameTooLong`]. The box drops an interrupt payload
    /// longer than the endpoint's `wMaxPacketSize`, in either direction. On a HID IN endpoint whose
    /// largest Input report is longer than `wMaxPacketSize`, a payload of exactly `wMaxPacketSize`
    /// ends with a zero-length packet, as a native report of that length does. A bulk payload
    /// splits at `wMaxPacketSize` on the wire and ends with a short packet, or a zero-length one on
    /// an exact multiple. Raw OUT packets and the game PC's writes queue together behind the real
    /// device's two-packet hold. Up to 16 wait across all endpoints; one past that is dropped and
    /// counted in [`relay_drops`](crate::Stats::relay_drops). The box queues 8 packets per vendor
    /// IN endpoint. When that queue is full, an interrupt endpoint loses its oldest packet and a
    /// bulk endpoint drops the raw packet sent into it; either counts in `relay_drops`.
    ///
    /// Needs [`allow_imperfect_clones`](Device::allow_imperfect_clones): with the opt-in off the
    /// box drops the frame with no reply, and this still returns `Ok`.
    /// [`query_imperfect`](Device::query_imperfect) reports the state; read it once at setup, not
    /// per call (a round trip each).
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
        self.link
            .send(FrameType::Raw, &raw_payload(ep, direction, bytes))
    }

    /// `TRANSFER` (§3.14): run one control transfer against the real device and return its reply.
    ///
    /// `ep` is 0 for EP0, or a control endpoint the real device declares. `setup` is the 8-byte
    /// setup packet; `out` is the OUT data stage (empty for an IN transfer). The
    /// [`TransferOutcome`] carries the [`status`](TransferOutcome::status) and the real device's
    /// reply ([`data`](TransferOutcome::data)). A status other than [`Ok`](TransferStatus::Ok) is a
    /// protocol result, not a link error, so it is returned, not raised; the outer `Ok` means the
    /// box replied.
    ///
    /// Needs [`allow_imperfect_clones`](Device::allow_imperfect_clones): with the opt-in off the
    /// box replies [`Refused`](TransferStatus::Refused) without reaching the real device. While the
    /// game PC has the clone suspended, the real device is suspended too and the box replies
    /// [`Nak`](TransferStatus::Nak). Transfers cross the inter-chip link one at a time, as a
    /// request and reply separate from the game PC's EP0 proxy. Uses [`DEFAULT_TRANSFER_TIMEOUT`];
    /// [`transfer_timeout`](Device::transfer_timeout) sets another.
    ///
    /// ```no_run
    /// # use medius::{Device, Result, Setup, TransferStatus};
    /// # fn main() -> Result<()> {
    /// let device = Device::find()?;
    /// device.allow_imperfect_clones(true)?;
    /// // GET_DESCRIPTOR(Device): the 18-byte device descriptor.
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
    /// The box gives up on a control transfer after ~800 ms. A shorter `timeout` can abandon the
    /// wait before a slow device replies. If 256 more transfers to the same `ep` then wrap the
    /// sequence number within those ~800 ms, the late reply can be matched to a later transfer.
    /// Keep `timeout` >=800 ms, as [`DEFAULT_TRANSFER_TIMEOUT`] is.
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
