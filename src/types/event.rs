//! The three catch event frames (§4.10) and what they decode to.

use crate::protocol::opcode::{CATCH_CTRL_NAK, CATCH_CTRL_OK, CATCH_CTRL_STALL};
use crate::types::{Axis, CatchClass, Class, ClockDomain, Direction, Usage};

/// Byte width of the header every catch event frame leads with: `ts_us` (u32) then the clock domain.
pub(crate) const EVENT_HDR: usize = 5;

/// A relative-axis catch event, a `MOTION_EVENT` frame (§4.10).
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
}

impl MotionEvent {
    /// Decode a `MOTION_EVENT` payload (§4.10): `[ts u32][clk u8][dx i16][dy i16][dz i16]`.
    pub(crate) fn from_payload(p: &[u8]) -> Option<MotionEvent> {
        if p.len() < EVENT_HDR + 6 {
            return None;
        }
        Some(MotionEvent {
            ts_us: u32::from_le_bytes([p[0], p[1], p[2], p[3]]),
            clock: ClockDomain::from_u8(p[4]),
            dx: i16::from_le_bytes([p[5], p[6]]),
            dy: i16::from_le_bytes([p[7], p[8]]),
            dz: i16::from_le_bytes([p[9], p[10]]),
        })
    }

    /// Every axis and delta this report carries, moved or not.
    pub fn all_axes(&self) -> [(Axis, i16); 3] {
        [
            (Axis::X, self.dx),
            (Axis::Y, self.dy),
            (Axis::Wheel, self.dz),
        ]
    }

    /// The axes this report actually moved, with their deltas. Empty for a report that moved nothing,
    /// which the box does not emit but a mock can.
    pub fn axes(&self) -> impl Iterator<Item = (Axis, i16)> + use<> {
        self.all_axes().into_iter().filter(|(_, d)| *d != 0)
    }
}

/// A held-usage snapshot catch event, a `USAGE_EVENT` frame (§4.10).
///
/// A snapshot is the class's state, not one usage's: it lists what is held, so the release of usage U
/// is the snapshot that does not contain U.
///
/// It lists what the BOX's table matched, which is the union of every subscription in this process,
/// so a stream watching one key sees the others as soon as unrelated code widens the table.
/// [`Device::input_events`](crate::Device::input_events) filters against your own addresses and turns
/// these into press and release edges; decode them yourself only if you want the raw held set.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UsageSnapshot {
    /// When the real device's report arrived, in that chip's microseconds.
    pub ts_us: u32,
    /// Always [`ClockDomain::HostChip`].
    pub clock: ClockDomain,
    /// Which class this snapshot is of. Carried in the frame because the empty snapshot, the release
    /// of the last held usage, has no usages to read it from.
    pub class: Class,
    /// The edge that produced this snapshot: the subscribed set grew ([`Direction::PRESS`]) or shrank
    /// ([`Direction::RELEASE`]).
    pub direction: Direction,
    /// The currently-held usages, all of `class`.
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

/// What a [`CatchClass::Bus`] event describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BusEvent {
    /// A USB bus reset.
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
    /// A real device attached on the host chip.
    DeviceAttached,
    /// The real device detached.
    DeviceDetached,
    /// The clone started.
    CloneUp,
    /// The clone stopped.
    CloneDown,
}

/// What the real device answered a proxied control transaction with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControlStatus {
    /// The device answered.
    Ok,
    /// The device STALLed.
    Stalled,
    /// The device NAKed to timeout, or never answered.
    Naked,
    /// A status byte this build does not know. Distinct from the three, so a future firmware's new
    /// status is not reported as a device fault that never happened.
    Other(u8),
}

/// A byte-oriented catch event, a `TRAFFIC_EVENT` frame (§4.10). Everything the box relays that is
/// not parsed input arrives here.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TrafficEvent {
    /// When the tap fired, in the stamping chip's microseconds.
    pub ts_us: u32,
    /// Which chip's clock stamped it.
    pub clock: ClockDomain,
    /// What this event is.
    pub class: CatchClass,
    /// Endpoint address, interface number, or endpoint number, per the class.
    pub id: u16,
    /// [`Direction::IN`] is device to PC, [`Direction::OUT`] is PC to device.
    pub direction: Direction,
    /// Class-specific; read it through [`Self::bus_event`] or [`Self::control_status`].
    pub flags: u8,
    /// The packet's length before the subscription's [`Capture`](crate::Capture) truncated it.
    pub true_len: u16,
    /// As much of the packet as the subscription's [`Capture`](crate::Capture) kept.
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

    /// Whether the capture cut this packet short. Without checking, a truncated capture and a
    /// genuinely short packet are indistinguishable.
    pub fn truncated(&self) -> bool {
        (self.bytes.len() as u16) < self.true_len
    }

    /// The 8-byte setup packet, for a [`CatchClass::Control`] event.
    pub fn setup(&self) -> Option<&[u8]> {
        if self.class == CatchClass::Control && self.bytes.len() >= 8 {
            Some(&self.bytes[..8])
        } else {
            None
        }
    }

    /// The data stage, for a [`CatchClass::Control`] event; the whole packet for any other class.
    ///
    /// Empty when the capture cut the setup packet itself short. The surviving bytes are the request,
    /// and returning them would label a GET_DESCRIPTOR request as the descriptor.
    pub fn data(&self) -> &[u8] {
        if self.class != CatchClass::Control {
            return &self.bytes;
        }
        if self.bytes.len() >= 8 {
            &self.bytes[8..]
        } else {
            &[]
        }
    }

    /// What the real device answered, for a [`CatchClass::Control`] event.
    pub fn control_status(&self) -> Option<ControlStatus> {
        if self.class != CatchClass::Control {
            return None;
        }
        Some(match self.flags {
            CATCH_CTRL_OK => ControlStatus::Ok,
            CATCH_CTRL_STALL => ControlStatus::Stalled,
            CATCH_CTRL_NAK => ControlStatus::Naked,
            v => ControlStatus::Other(v),
        })
    }

    /// The lifecycle event, for a [`CatchClass::Bus`] event.
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

    /// Whether this event carries end-of-transfer, for a [`CatchClass::VendorBulk`] event.
    pub fn bulk_end_of_transfer(&self) -> bool {
        self.class == CatchClass::VendorBulk && self.flags & 0x01 != 0
    }

    /// Whether this event is a zero-length packet, for a [`CatchClass::VendorBulk`] event. A ZLP
    /// terminates a transfer whose length is an exact multiple of the packet size, so it carries no
    /// bytes and still matters.
    pub fn bulk_zlp(&self) -> bool {
        self.class == CatchClass::VendorBulk && self.flags & 0x02 != 0
    }
}

/// One event from the catch stream.
///
/// Every variant carries `ts_us` and the [`ClockDomain`] that stamped it; both clocks are box-local
/// and wrap every ~71.6 minutes. [`Timeline`](crate::Timeline) turns one into an
/// [`Instant`](std::time::Instant).
///
/// Idle polls are never reported: a device that reports at every poll interval still only produces
/// events when something subscribed changes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CatchEvent {
    /// A relative-axis event: cursor motion and/or wheel.
    Motion(MotionEvent),
    /// A held-usage snapshot for one class (buttons, keys, or media).
    Usages(UsageSnapshot),
    /// Byte-oriented traffic: HID, vendor endpoints, control transactions, emitted bytes, or bus.
    Traffic(TrafficEvent),
}

impl CatchEvent {
    /// The stamping chip's microsecond stamp, whichever variant this is.
    pub fn ts_us(&self) -> u32 {
        match self {
            CatchEvent::Motion(m) => m.ts_us,
            CatchEvent::Usages(u) => u.ts_us,
            CatchEvent::Traffic(t) => t.ts_us,
        }
    }

    /// Which chip's clock stamped it. Do not subtract two stamps from different domains.
    pub fn clock(&self) -> ClockDomain {
        match self {
            CatchEvent::Motion(m) => m.clock,
            CatchEvent::Usages(u) => u.clock,
            CatchEvent::Traffic(t) => t.clock,
        }
    }

    /// The class this event belongs to, whichever variant it is.
    pub fn class(&self) -> CatchClass {
        match self {
            CatchEvent::Motion(_) => CatchClass::Axis,
            CatchEvent::Usages(u) => u.class.into(),
            CatchEvent::Traffic(t) => t.class,
        }
    }

    /// The address within the class, when the event names one.
    ///
    /// `None` for both input variants: a motion report can move three axes at once and a snapshot is
    /// the class's whole held set. See [`MotionEvent::axes`] and [`UsageSnapshot::usages`].
    pub fn id(&self) -> Option<u16> {
        match self {
            CatchEvent::Motion(_) | CatchEvent::Usages(_) => None,
            CatchEvent::Traffic(t) => Some(t.id),
        }
    }

    /// The edge, sign or flow this event arrived on. [`Direction::Both`] for motion, where one report
    /// can move X positive and Y negative; [`MotionEvent::axes`] gives the per-axis signs.
    pub fn direction(&self) -> Direction {
        match self {
            CatchEvent::Motion(_) => Direction::Both,
            CatchEvent::Usages(u) => u.direction,
            CatchEvent::Traffic(t) => t.direction,
        }
    }

    /// The captured packet bytes; empty for the two decoded input variants, which carry no packet.
    pub fn bytes(&self) -> &[u8] {
        match self {
            CatchEvent::Motion(_) | CatchEvent::Usages(_) => &[],
            CatchEvent::Traffic(t) => &t.bytes,
        }
    }
}
