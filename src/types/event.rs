//! The three catch event frames (§4.10), decoded.

use crate::protocol::opcode::{
    CATCH_CTRL_MASK, CATCH_CTRL_NAK, CATCH_CTRL_OK, CATCH_CTRL_STALL, CATCH_F_RULE,
};
use crate::types::{Axis, CatchClass, Class, ClockDomain, Direction, TransferStatus, Usage};

/// Catch event frame header width: `ts_us` (u32) then the clock domain.
pub(crate) const EVENT_HDR: usize = 5;

/// Relative-axis catch event, a `MOTION_EVENT` frame (§4.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MotionEvent {
    /// When the real device's report arrived, in that chip's microseconds.
    pub ts_us: u32,
    /// Always [`ClockDomain::HostChip`]: this event only exists for a report the real device sent.
    pub clock: ClockDomain,
    /// Relative X this report (right positive).
    pub dx: i16,
    /// Relative Y this report (down positive).
    pub dy: i16,
    /// Wheel delta this report (up positive).
    pub dz: i16,
    /// AC Pan (horizontal-scroll) delta this report (right positive).
    pub pan: i16,
}

impl MotionEvent {
    /// Decode a `MOTION_EVENT` payload (§4.10): `[ts u32][clk u8][dx i16][dy i16][dz i16][dpan i16]`.
    pub(crate) fn from_payload(p: &[u8]) -> Option<MotionEvent> {
        if p.len() < EVENT_HDR + 8 {
            return None;
        }
        Some(MotionEvent {
            ts_us: u32::from_le_bytes([p[0], p[1], p[2], p[3]]),
            clock: ClockDomain::from_u8(p[4]),
            dx: i16::from_le_bytes([p[5], p[6]]),
            dy: i16::from_le_bytes([p[7], p[8]]),
            dz: i16::from_le_bytes([p[9], p[10]]),
            pan: i16::from_le_bytes([p[11], p[12]]),
        })
    }

    /// Every axis and delta this report carries, moved or not.
    pub fn all_axes(&self) -> [(Axis, i16); 4] {
        [
            (Axis::X, self.dx),
            (Axis::Y, self.dy),
            (Axis::Wheel, self.dz),
            (Axis::Pan, self.pan),
        ]
    }

    /// Axes this report moved, with their deltas. Empty for a report that moved nothing, which the
    /// box never emits but a mock can.
    pub fn axes(&self) -> impl Iterator<Item = (Axis, i16)> + use<> {
        self.all_axes().into_iter().filter(|(_, d)| *d != 0)
    }
}

/// Held-usage snapshot catch event, a `USAGE_EVENT` frame (§4.10).
///
/// Lists everything held in the class, so the release of usage U is the snapshot without U.
///
/// It lists what the box's table matched: the union of every subscription in this process, so a
/// stream watching one key sees others once unrelated code widens the table.
/// [`Device::input_events`](crate::Device::input_events) filters to the requested addresses and
/// yields press and release edges; decode snapshots directly only for the raw held set.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UsageSnapshot {
    /// When the real device's report arrived, in that chip's microseconds.
    pub ts_us: u32,
    /// Always [`ClockDomain::HostChip`].
    pub clock: ClockDomain,
    /// Snapshot class. In the frame because the empty snapshot (release of the last held usage) has
    /// no usages to read it from.
    pub class: Class,
    /// Edge that produced this snapshot: the subscribed set grew ([`Direction::PRESS`]) or shrank
    /// ([`Direction::RELEASE`]).
    pub direction: Direction,
    /// Held usages, all of `class`.
    pub usages: Vec<Usage>,
}

impl UsageSnapshot {
    // Decode a `USAGE_EVENT` payload (§4.10): `[ts u32][clk u8][cls u8][dir u8][n u8]` then
    // `n × [class][id u16]`.
    pub(crate) fn from_payload(p: &[u8]) -> Option<UsageSnapshot> {
        if p.len() < EVENT_HDR + 2 {
            return None;
        }
        Some(UsageSnapshot {
            ts_us: u32::from_le_bytes([p[0], p[1], p[2], p[3]]),
            clock: ClockDomain::from_u8(p[4]),
            class: Class::from_u8(p[5])?,
            direction: Direction::from_u8(p[6])?,
            usages: Usage::decode_list(&p[EVENT_HDR + 2..])?,
        })
    }

    /// Whether `usage` is held in this snapshot.
    pub fn is_held(&self, usage: impl Into<Usage>) -> bool {
        let u = usage.into();
        self.usages.contains(&u)
    }
}

/// [`CatchClass::Bus`] event kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BusEvent {
    /// USB bus reset.
    Reset,
    /// The host suspended the bus.
    Suspend,
    /// The host resumed the bus.
    Resume,
    /// `SET_CONFIGURATION` selected this configuration index.
    Configured(u8),
    /// The clone left the configured state.
    Deconfigured,
    /// `SET_INTERFACE` selected this alternate setting on this interface.
    SetInterface { interface: u8, alt: u8 },
    /// Real device attached on the host chip.
    DeviceAttached,
    /// The real device detached.
    DeviceDetached,
    /// The clone started.
    CloneUp,
    /// The clone stopped.
    CloneDown,
}

/// Handshake the game PC received for a control transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControlStatus {
    /// Transaction completed.
    Ok,
    /// STALL to the PC: from the device, from a rule refusing the request, or, above endpoint 0,
    /// for a failed request.
    Stalled,
    /// NAKed until the host gave up, on endpoint 0 only: the device never answered, or a `Nak` rule.
    Naked,
    /// Handshake value unknown to this build, kept distinct so a newer firmware's value does not
    /// read as a device fault.
    Other(u8),
}

/// Byte-oriented catch event, a `TRAFFIC_EVENT` frame (§4.10): everything the box relays besides
/// parsed input.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TrafficEvent {
    /// When the tap fired, in the stamping chip's microseconds.
    pub ts_us: u32,
    /// Which chip's clock stamped it.
    pub clock: ClockDomain,
    /// Event class.
    pub class: CatchClass,
    /// Endpoint or interface number, per the class.
    pub id: u16,
    /// [`Direction::IN`] is device to PC, [`Direction::OUT`] is PC to device.
    pub direction: Direction,
    /// Class-specific; read through [`Self::control_status`], [`Self::rule_acted`],
    /// [`Self::transfer_status`], [`Self::bus_event`] or the bulk accessors.
    pub flags: u8,
    /// Packet length before the subscription's [`Capture`](crate::Capture) truncated it.
    pub true_len: u16,
    /// Packet bytes the subscription's [`Capture`](crate::Capture) kept.
    pub bytes: Vec<u8>,
}

impl TrafficEvent {
    pub(crate) fn from_payload(p: &[u8]) -> Option<TrafficEvent> {
        if p.len() < 12 {
            return None;
        }
        Some(TrafficEvent {
            ts_us: u32::from_le_bytes([p[0], p[1], p[2], p[3]]),
            clock: ClockDomain::from_u8(p[4]),
            class: CatchClass::from_u8(p[5])?,
            id: u16::from_le_bytes([p[6], p[7]]),
            direction: Direction::from_u8(p[8])?,
            flags: p[9],
            true_len: u16::from_le_bytes([p[10], p[11]]),
            bytes: p[12..].to_vec(),
        })
    }

    /// Whether the capture cut this packet short; the bytes alone cannot tell that from a short
    /// packet.
    pub fn truncated(&self) -> bool {
        (self.bytes.len() as u16) < self.true_len
    }

    // Classes whose bytes are `[setup 8][data]`.
    fn is_control_shaped(&self) -> bool {
        matches!(self.class, CatchClass::Control | CatchClass::ClipTransfer)
    }

    /// 8-byte setup packet of a [`CatchClass::Control`] or [`CatchClass::ClipTransfer`] event.
    pub fn setup(&self) -> Option<&[u8]> {
        if self.is_control_shaped() && self.bytes.len() >= 8 {
            Some(&self.bytes[..8])
        } else {
            None
        }
    }

    /// Data stage of a [`CatchClass::Control`] or [`CatchClass::ClipTransfer`] event; the whole
    /// packet for any other class.
    ///
    /// Empty when the capture cut the setup packet itself short: the surviving bytes are the
    /// request, and returning them would label a GET_DESCRIPTOR request as the descriptor.
    pub fn data(&self) -> &[u8] {
        if !self.is_control_shaped() {
            return &self.bytes;
        }
        if self.bytes.len() >= 8 {
            &self.bytes[8..]
        } else {
            &[]
        }
    }

    /// Handshake the game PC received, for a [`CatchClass::Control`] event.
    pub fn control_status(&self) -> Option<ControlStatus> {
        if self.class != CatchClass::Control {
            return None;
        }
        Some(match self.flags & CATCH_CTRL_MASK {
            CATCH_CTRL_OK => ControlStatus::Ok,
            CATCH_CTRL_STALL => ControlStatus::Stalled,
            CATCH_CTRL_NAK => ControlStatus::Naked,
            v => ControlStatus::Other(v),
        })
    }

    /// Whether a rewrite rule at this event's class changed, dropped, answered or refused the packet.
    /// Carried only by classes a rule acts at; never set by a `Pass` rule or a `Patch` that changed
    /// nothing.
    pub fn rule_acted(&self) -> bool {
        let ruled = matches!(
            self.class,
            CatchClass::HidIn
                | CatchClass::HidOut
                | CatchClass::VendorInterrupt
                | CatchClass::VendorBulk
                | CatchClass::Control
                | CatchClass::Emit
        );
        ruled && self.flags & CATCH_F_RULE != 0
    }

    /// How a [`CatchClass::ClipTransfer`] event's transfer ended: the status
    /// [`transfer`](crate::Device::transfer) returns, or [`Nak`](TransferStatus::Nak) with no reply.
    pub fn transfer_status(&self) -> Option<TransferStatus> {
        (self.class == CatchClass::ClipTransfer).then(|| TransferStatus::from_u8(self.flags))
    }

    /// Lifecycle event of a [`CatchClass::Bus`] event.
    pub fn bus_event(&self) -> Option<BusEvent> {
        if self.class != CatchClass::Bus {
            return None;
        }
        let a = self.bytes.first().copied().unwrap_or(0);
        let b = self.bytes.get(1).copied().unwrap_or(0);
        Some(match self.flags {
            0 => BusEvent::Reset,
            1 => BusEvent::Suspend,
            2 => BusEvent::Resume,
            3 => BusEvent::Configured(a),
            4 => BusEvent::Deconfigured,
            5 => BusEvent::SetInterface {
                interface: a,
                alt: b,
            },
            6 => BusEvent::DeviceAttached,
            7 => BusEvent::DeviceDetached,
            8 => BusEvent::CloneUp,
            9 => BusEvent::CloneDown,
            _ => return None,
        })
    }

    /// Whether a [`CatchClass::VendorBulk`] event carries end-of-transfer.
    pub fn bulk_end_of_transfer(&self) -> bool {
        self.class == CatchClass::VendorBulk && self.flags & 0x01 != 0
    }

    /// Whether a [`CatchClass::VendorBulk`] event is a zero-length packet, which ends a transfer
    /// whose length is an exact multiple of the packet size.
    pub fn bulk_zlp(&self) -> bool {
        self.class == CatchClass::VendorBulk && self.flags & 0x02 != 0
    }
}

/// Catch stream event.
///
/// Every variant carries `ts_us` and the stamping [`ClockDomain`]; both clocks are box-local and
/// wrap every ~71.6 minutes. [`Timeline`](crate::Timeline) turns a stamp into an
/// [`Instant`](std::time::Instant).
///
/// Idle polls are never reported: a device reporting at every poll interval produces events only
/// when something subscribed changes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CatchEvent {
    /// Relative-axis event: cursor motion and/or wheel.
    Motion(MotionEvent),
    /// Held-usage snapshot for one class (buttons, keys, or media).
    Usages(UsageSnapshot),
    /// Byte-oriented traffic: HID, vendor endpoints, control transactions, emitted bytes, or bus.
    Traffic(TrafficEvent),
}

impl CatchEvent {
    /// Stamping chip's microsecond stamp, for any variant.
    pub fn ts_us(&self) -> u32 {
        match self {
            CatchEvent::Motion(m) => m.ts_us,
            CatchEvent::Usages(u) => u.ts_us,
            CatchEvent::Traffic(t) => t.ts_us,
        }
    }

    /// Chip whose clock stamped it; never subtract stamps from different domains.
    pub fn clock(&self) -> ClockDomain {
        match self {
            CatchEvent::Motion(m) => m.clock,
            CatchEvent::Usages(u) => u.clock,
            CatchEvent::Traffic(t) => t.clock,
        }
    }

    /// Event class, for any variant.
    pub fn class(&self) -> CatchClass {
        match self {
            CatchEvent::Motion(_) => CatchClass::Axis,
            CatchEvent::Usages(u) => u.class.into(),
            CatchEvent::Traffic(t) => t.class,
        }
    }

    /// Address within the class, when the event names one.
    ///
    /// `None` for both input variants: a motion report can move three axes at once and a snapshot is
    /// the class's whole held set. See [`MotionEvent::axes`] and [`UsageSnapshot::usages`].
    pub fn id(&self) -> Option<u16> {
        match self {
            CatchEvent::Motion(_) | CatchEvent::Usages(_) => None,
            CatchEvent::Traffic(t) => Some(t.id),
        }
    }

    /// Edge, sign or flow this event arrived on. [`Direction::Both`] for motion, where one report can
    /// move X positive and Y negative; [`MotionEvent::axes`] gives per-axis signs.
    pub fn direction(&self) -> Direction {
        match self {
            CatchEvent::Motion(_) => Direction::Both,
            CatchEvent::Usages(u) => u.direction,
            CatchEvent::Traffic(t) => t.direction,
        }
    }

    /// Captured packet bytes; empty for the two decoded input variants, which carry no packet.
    pub fn bytes(&self) -> &[u8] {
        match self {
            CatchEvent::Motion(_) | CatchEvent::Usages(_) => &[],
            CatchEvent::Traffic(t) => &t.bytes,
        }
    }
}
