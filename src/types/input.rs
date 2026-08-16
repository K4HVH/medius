//! `CATCH` vocabulary (§3.9): the subscription address space, catch events, decoded `RESP(CATCH)`.

use core::time::Duration;

use crate::protocol::opcode::{
    CATCH_CLS_ANY, CATCH_CLS_AXIS, CATCH_CLS_BTN, CATCH_CLS_BUS, CATCH_CLS_CONTROL, CATCH_CLS_EMIT,
    CATCH_CLS_HID_IN, CATCH_CLS_HID_OUT, CATCH_CLS_KEY, CATCH_CLS_MEDIA, CATCH_CLS_VEND_BULK,
    CATCH_CLS_VEND_INTR, CATCH_ID_ANY,
};
use crate::types::{Class, LockDirection, Usage};

/// Byte width of the header every catch event frame leads with: `ts_us` (u32) then the clock domain.
pub(crate) const EVENT_HDR: usize = 5;

/// What a [`CatchFilter`] addresses. Classes 0–3 are the same classes `LOCK` and `INJECT` address, so
/// one vocabulary spans the whole protocol; 4–10 are the traffic the box relays.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CatchClass {
    /// A mouse button; `id` is the button id.
    Button = CATCH_CLS_BTN,
    /// A keyboard key or modifier; `id` is the HID usage.
    Key = CATCH_CLS_KEY,
    /// A media usage; `id` is the 16-bit Consumer usage.
    Media = CATCH_CLS_MEDIA,
    /// A relative axis; `id` is X, Y or wheel.
    Axis = CATCH_CLS_AXIS,
    /// Raw HID input report bytes; `id` is the interface number. Covers interfaces the semantic model
    /// does not parse, such as a vendor-usage-page collection, which produce no other event.
    HidIn = CATCH_CLS_HID_IN,
    /// Interrupt-OUT report bytes the PC wrote; `id` is the endpoint address.
    HidOut = CATCH_CLS_HID_OUT,
    /// Interrupt traffic on a vendor interface; `id` is the endpoint address.
    VendorInterrupt = CATCH_CLS_VEND_INTR,
    /// Bulk traffic on a vendor interface; `id` is the endpoint address. The one class that can
    /// saturate the control link on its own, and the first dropped when it cannot keep up.
    VendorBulk = CATCH_CLS_VEND_BULK,
    /// A proxied control transaction; `id` is the endpoint number (0 = EP0).
    Control = CATCH_CLS_CONTROL,
    /// The bytes the clone actually put on the wire; `id` is the interface number.
    Emit = CATCH_CLS_EMIT,
    /// Bus lifecycle (reset, suspend, configuration and interface changes, attach and detach).
    Bus = CATCH_CLS_BUS,
}

impl CatchClass {
    /// The wire `class` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Map a wire `class` byte to a [`CatchClass`], or `None` for an unknown value.
    pub fn from_u8(v: u8) -> Option<CatchClass> {
        Some(match v {
            CATCH_CLS_BTN => CatchClass::Button,
            CATCH_CLS_KEY => CatchClass::Key,
            CATCH_CLS_MEDIA => CatchClass::Media,
            CATCH_CLS_AXIS => CatchClass::Axis,
            CATCH_CLS_HID_IN => CatchClass::HidIn,
            CATCH_CLS_HID_OUT => CatchClass::HidOut,
            CATCH_CLS_VEND_INTR => CatchClass::VendorInterrupt,
            CATCH_CLS_VEND_BULK => CatchClass::VendorBulk,
            CATCH_CLS_CONTROL => CatchClass::Control,
            CATCH_CLS_EMIT => CatchClass::Emit,
            CATCH_CLS_BUS => CatchClass::Bus,
            _ => return None,
        })
    }
}

/// One subscription entry: what to observe, in which direction, and how much of each packet to keep.
///
/// `class: None` matches every class and `id: None` every id within a class. Matching is
/// most-specific-first, so an exact `(class, id)` beats a class blanket, which beats the everything
/// filter — and the winning entry supplies `snaplen`. That is what lets a caller say "everything at
/// 16 bytes, except this endpoint in full" in two entries.
///
/// Addressing doubles as the filter, and that is load-bearing rather than tidy: the control link
/// cannot carry every class at once, so a subscription has to be able to name what it means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CatchFilter {
    /// The class to observe; `None` for every class.
    pub class: Option<CatchClass>,
    /// The class-specific id; `None` for every id in the class.
    pub id: Option<u16>,
    /// For the input classes, the press or release edge. For the traffic classes,
    /// [`LockDirection::Positive`] is IN (device to PC) and [`LockDirection::Negative`] is OUT.
    pub direction: LockDirection,
    /// Bytes kept per event; 0 keeps the whole packet.
    pub snaplen: u8,
}

impl CatchFilter {
    /// Every class, every id, both directions, whole packets. One frame on the wire.
    pub const fn all() -> CatchFilter {
        CatchFilter {
            class: None,
            id: None,
            direction: LockDirection::Both,
            snaplen: 0,
        }
    }

    /// Every id within one class.
    pub const fn class(class: CatchClass) -> CatchFilter {
        CatchFilter {
            class: Some(class),
            id: None,
            direction: LockDirection::Both,
            snaplen: 0,
        }
    }

    /// One exact address: an endpoint, an interface, or a usage.
    pub const fn addr(class: CatchClass, id: u16) -> CatchFilter {
        CatchFilter {
            class: Some(class),
            id: Some(id),
            direction: LockDirection::Both,
            snaplen: 0,
        }
    }

    /// Restrict to one direction or edge.
    pub const fn direction(mut self, direction: LockDirection) -> CatchFilter {
        self.direction = direction;
        self
    }

    /// Keep only the first `n` bytes of each event; 0 keeps the whole packet.
    pub const fn snaplen(mut self, n: u8) -> CatchFilter {
        self.snaplen = n;
        self
    }

    /// The wire `(class, id)` pair, with wildcards resolved to their sentinels.
    pub(crate) fn wire(self) -> (u8, u16) {
        (
            self.class.map_or(CATCH_CLS_ANY, CatchClass::as_u8),
            self.id.unwrap_or(CATCH_ID_ANY),
        )
    }

    pub(crate) fn from_wire(class: u8, id: u16, direction: u8, snaplen: u8) -> Option<CatchFilter> {
        Some(CatchFilter {
            class: if class == CATCH_CLS_ANY {
                None
            } else {
                Some(CatchClass::from_u8(class)?)
            },
            id: if id == CATCH_ID_ANY { None } else { Some(id) },
            direction: LockDirection::from_u8(direction)?,
            snaplen,
        })
    }
}

/// Which chip's clock stamped an event.
///
/// The two chips boot independently, so nothing relates their timers. A stamp is only meaningful
/// against another from the same domain; to place both on one timeline, apply
/// [`ClockEstimate::offset_us`] and respect its error bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClockDomain {
    /// The device-facing chip, stamped in USB interrupt context when the real device's transfer
    /// completed. Everything the real device produced carries this.
    HostChip,
    /// The PC-facing chip, stamped at the tap. Everything the PC produced, and everything the clone
    /// emitted, carries this — the host chip never saw those bytes.
    DeviceChip,
}

impl ClockDomain {
    pub(crate) fn from_u8(v: u8) -> ClockDomain {
        if v == 0 {
            ClockDomain::HostChip
        } else {
            ClockDomain::DeviceChip
        }
    }
}

/// The measured difference between the two chips' clocks, from `RESP(CATCH)` (§4.9).
///
/// Measured with a four-timestamp exchange over the inter-chip link, stamped as each frame reaches
/// the wire rather than when it is queued — queueing is the largest and most variable delay on that
/// link, so stamping late removes it from the measurement instead of filtering around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClockEstimate {
    /// The host chip's clock minus the device chip's, in microseconds.
    pub offset_us: i32,
    /// Relative drift between the two crystals, parts per billion.
    pub rate_ppb: i32,
    /// Best measured round trip in the window. The offset is good to about half of this.
    pub delay_us: u16,
    /// Age of the estimate, or `None` if the box has no estimate yet — which is how a caller tells
    /// that apart from an offset that happens to be zero.
    pub age: Option<Duration>,
}

impl ClockEstimate {
    /// Half the measured round trip: the bound on how wrong [`Self::offset_us`] can be.
    pub fn error_bound_us(&self) -> u16 {
        self.delay_us / 2
    }

    /// Translate a device-chip stamp into the host chip's domain, drift-corrected. `None` when there
    /// is no estimate to apply.
    pub fn to_host_domain(&self, device_us: u32) -> Option<i64> {
        self.age?;
        Some(device_us as i64 + self.offset_us as i64)
    }
}

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
}

/// A held-usage snapshot catch event, a `USAGE_EVENT` frame (§4.10).
///
/// Only the held usages that actually match the subscription appear, and no event is emitted when
/// none do — so a subscription to one button stays sparse even against a 1 kHz mouse.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UsageSnapshot {
    /// When the real device's report arrived, in that chip's microseconds.
    pub ts_us: u32,
    /// Always [`ClockDomain::HostChip`].
    pub clock: ClockDomain,
    /// The currently-held usages (all of one class per event).
    pub usages: Vec<Usage>,
}

impl UsageSnapshot {
    /// Decode a `USAGE_EVENT` payload (§4.10): `[ts u32][clk u8][n u8]` then `n × [class][id u16]`.
    pub(crate) fn from_payload(p: &[u8]) -> Option<UsageSnapshot> {
        if p.len() < EVENT_HDR {
            return None;
        }
        Some(UsageSnapshot {
            ts_us: u32::from_le_bytes([p[0], p[1], p[2], p[3]]),
            clock: ClockDomain::from_u8(p[4]),
            usages: Usage::decode_list(&p[EVENT_HDR..])?,
        })
    }

    /// The class of this snapshot's usages (from the first entry), or `None` if empty.
    pub fn class(&self) -> Option<Class> {
        self.usages.first().map(|u| u.class)
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
    /// [`LockDirection::Positive`] is IN (device to PC), [`LockDirection::Negative`] is OUT.
    pub direction: LockDirection,
    /// Class-specific; read it through [`Self::bus_event`] or [`Self::control_status`].
    pub flags: u8,
    /// The packet's length before `snaplen` truncation.
    pub true_len: u16,
    /// As much of the packet as the subscription's `snaplen` kept.
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
            direction: LockDirection::from_u8(p[8])?,
            flags: p[9],
            true_len: u16::from_le_bytes([p[10], p[11]]),
            bytes: p[12..].to_vec(),
        })
    }

    /// Whether `snaplen` cut this packet short. Without checking, a truncated capture and a genuinely
    /// short packet are indistinguishable.
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
    pub fn data(&self) -> &[u8] {
        if self.class == CatchClass::Control && self.bytes.len() >= 8 {
            &self.bytes[8..]
        } else {
            &self.bytes
        }
    }

    /// What the real device answered, for a [`CatchClass::Control`] event.
    pub fn control_status(&self) -> Option<ControlStatus> {
        if self.class != CatchClass::Control {
            return None;
        }
        Some(match self.flags {
            0x00 => ControlStatus::Ok,
            0xFD => ControlStatus::Stalled,
            _ => ControlStatus::Naked,
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
/// Every variant carries `ts_us` and the [`ClockDomain`] that stamped it. Both clocks are box-local,
/// unrelated to any clock on this machine, so values are only meaningful compared against each other
/// — and only within one domain unless you apply [`CatchState::clock`]. Each wraps every ~71.6
/// minutes and restarts at zero if that chip reboots, so a value below the previous one is a wrap, a
/// reboot, or a domain change.
///
/// Idle polls are never reported. A device that reports at every poll interval even at rest still
/// only produces events when something subscribed actually changes.
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
}

/// One row of [`CatchState::entries`]: a live subscription and what it has lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CatchEntry {
    /// The subscription as the box holds it.
    pub filter: CatchFilter,
    /// Events this entry could not queue. Per entry, because under a saturating trace the box-wide
    /// count says you are losing events but not which ones, and those are different problems.
    pub dropped: u16,
}

/// Decoded `RESP(CATCH)` (§4.9): the live subscription table, its drop counts, and the measured
/// inter-chip clock estimate.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CatchState {
    /// The box refused an entry because its table is full.
    pub table_full: bool,
    /// Box-wide events dropped under back-pressure.
    pub dropped: u32,
    /// The measured difference between the two chips' clocks.
    pub clock: ClockEstimate,
    /// The live subscription table.
    pub entries: Vec<CatchEntry>,
}

impl CatchState {
    pub(crate) const HDR: usize = 19;
    pub(crate) const ENTRY: usize = 7;

    /// Decode a `RESP(CATCH)` payload (§4.9).
    pub(crate) fn from_payload(p: &[u8]) -> Option<CatchState> {
        if p.len() < Self::HDR {
            return None;
        }
        let age_ms = u16::from_le_bytes([p[16], p[17]]);
        let n = p[18] as usize;
        let mut entries = Vec::with_capacity(n);
        for i in 0..n {
            let o = Self::HDR + Self::ENTRY * i;
            if o + Self::ENTRY > p.len() {
                break;
            }
            let filter = CatchFilter::from_wire(
                p[o],
                u16::from_le_bytes([p[o + 1], p[o + 2]]),
                p[o + 3],
                p[o + 4],
            )?;
            entries.push(CatchEntry {
                filter,
                dropped: u16::from_le_bytes([p[o + 5], p[o + 6]]),
            });
        }
        Some(CatchState {
            table_full: p[1] & 0x01 != 0,
            dropped: u32::from_le_bytes([p[2], p[3], p[4], p[5]]),
            clock: ClockEstimate {
                offset_us: i32::from_le_bytes([p[6], p[7], p[8], p[9]]),
                rate_ppb: i32::from_le_bytes([p[10], p[11], p[12], p[13]]),
                delay_us: u16::from_le_bytes([p[14], p[15]]),
                // 0xFFFF is the box saying it has no estimate, which is not the same as an estimate
                // that happens to be zero microseconds old.
                age: if age_ms == u16::MAX {
                    None
                } else {
                    Some(Duration::from_millis(age_ms as u64))
                },
            },
            entries,
        })
    }

    /// Whether anything is subscribed.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
