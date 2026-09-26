//! `CATCH` (§3.9) subscription vocabulary: the address space, one table entry, decoded `RESP(CATCH)`.

use crate::protocol::opcode::{
    CATCH_CLS_ANY, CATCH_CLS_AXIS, CATCH_CLS_BTN, CATCH_CLS_BUS, CATCH_CLS_CLIP_XFER,
    CATCH_CLS_CONTROL, CATCH_CLS_EMIT, CATCH_CLS_HID_IN, CATCH_CLS_HID_OUT, CATCH_CLS_KEY,
    CATCH_CLS_MEDIA, CATCH_CLS_VEND_BULK, CATCH_CLS_VEND_INTR, CATCH_ID_ANY,
};
use crate::types::{Axis, Class, ClockEstimate, Direction, Usage};

/// Class a [`CatchFilter`] addresses.
///
/// Classes 0 to 3 are `LOCK`'s classes at the same byte values. Classes 4 to 10 are the
/// byte-oriented traffic the box relays; [`TrafficClass`] is that half alone.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CatchClass {
    /// Mouse button; `id` is the button id.
    Button = CATCH_CLS_BTN,
    /// Keyboard key or modifier; `id` is the HID usage.
    Key = CATCH_CLS_KEY,
    /// Media usage; `id` is the 16-bit Consumer usage.
    Media = CATCH_CLS_MEDIA,
    /// Relative axis; `id` is X, Y, wheel or pan.
    Axis = CATCH_CLS_AXIS,
    /// Raw HID input report bytes; `id` is the interface number. Covers interfaces the semantic model
    /// does not parse, which raise no other event.
    HidIn = CATCH_CLS_HID_IN,
    /// Interrupt-OUT report bytes the PC wrote; `id` is the endpoint number, `direction`
    /// [`OUT`](Direction::OUT).
    HidOut = CATCH_CLS_HID_OUT,
    /// Interrupt traffic on a vendor interface; `id` is the endpoint number, `direction`
    /// [`IN`](Direction::IN) or [`OUT`](Direction::OUT).
    VendorInterrupt = CATCH_CLS_VEND_INTR,
    /// Bulk traffic on a vendor interface; `id` is the endpoint number, `direction`
    /// [`IN`](Direction::IN) or [`OUT`](Direction::OUT). The only class that can saturate the
    /// control link by itself, and the first dropped when it cannot keep up.
    VendorBulk = CATCH_CLS_VEND_BULK,
    /// Control transaction the game PC received, on any control endpoint; `id` is the endpoint
    /// number (0 = EP0). On EP0 only class and vendor requests raise one.
    Control = CATCH_CLS_CONTROL,
    /// Bytes the clone put on the wire; `id` is the endpoint number, `direction`
    /// [`IN`](Direction::IN).
    Emit = CATCH_CLS_EMIT,
    /// Bus lifecycle; no id.
    Bus = CATCH_CLS_BUS,
    /// Control transfer a clip ran against the real device
    /// ([`ClipFrame::transfer`](crate::ClipFrame::transfer)); `id` is the endpoint number (0 = EP0).
    ClipTransfer = CATCH_CLS_CLIP_XFER,
}

impl CatchClass {
    /// Wire `class` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Decodes a wire `class` byte; `None` if unknown.
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
            CATCH_CLS_CLIP_XFER => CatchClass::ClipTransfer,
            _ => return None,
        })
    }

    /// Whether this is a parsed-input class (button, key, media, axis). These arrive decoded with no
    /// packet, so a [`Capture`] means nothing on them.
    pub fn is_input(self) -> bool {
        matches!(
            self,
            CatchClass::Button | CatchClass::Key | CatchClass::Media | CatchClass::Axis
        )
    }

    /// Whether this is one of the eight traffic classes.
    pub fn is_traffic(self) -> bool {
        !self.is_input()
    }

    /// How this class reads a [`Direction`].
    pub fn direction_meaning(self) -> DirectionMeaning {
        match self {
            CatchClass::Button | CatchClass::Key | CatchClass::Media => DirectionMeaning::Edge,
            CatchClass::Axis => DirectionMeaning::Sign,
            _ => DirectionMeaning::Flow,
        }
    }
}

/// [`Direction`] reading a class uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DirectionMeaning {
    /// [`Direction::PRESS`] or [`Direction::RELEASE`].
    Edge,
    /// Sign of a relative delta.
    Sign,
    /// [`Direction::IN`] or [`Direction::OUT`].
    Flow,
}

/// Byte-oriented half of the catch address space, so a traffic constructor cannot take an input
/// class.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TrafficClass {
    /// Raw HID input report bytes; `id` is the interface number.
    HidIn = CATCH_CLS_HID_IN,
    /// Interrupt-OUT report bytes the PC wrote; `id` is the endpoint number, `direction`
    /// [`OUT`](Direction::OUT).
    HidOut = CATCH_CLS_HID_OUT,
    /// Interrupt traffic on a vendor interface; `id` is the endpoint number, `direction`
    /// [`IN`](Direction::IN) or [`OUT`](Direction::OUT).
    VendorInterrupt = CATCH_CLS_VEND_INTR,
    /// Bulk traffic on a vendor interface; `id` is the endpoint number, `direction`
    /// [`IN`](Direction::IN) or [`OUT`](Direction::OUT).
    VendorBulk = CATCH_CLS_VEND_BULK,
    /// Control transaction the game PC received, on any control endpoint; `id` is the endpoint
    /// number (0 = EP0). On EP0 only class and vendor requests raise one.
    Control = CATCH_CLS_CONTROL,
    /// Bytes the clone put on the wire; `id` is the endpoint number, `direction`
    /// [`IN`](Direction::IN).
    Emit = CATCH_CLS_EMIT,
    /// Bus lifecycle; no id.
    Bus = CATCH_CLS_BUS,
    /// Control transfer a clip ran against the real device; `id` is the endpoint number (0 = EP0).
    ClipTransfer = CATCH_CLS_CLIP_XFER,
}

impl TrafficClass {
    /// Every traffic class, in wire order.
    pub const ALL: [TrafficClass; 8] = [
        TrafficClass::HidIn,
        TrafficClass::HidOut,
        TrafficClass::VendorInterrupt,
        TrafficClass::VendorBulk,
        TrafficClass::Control,
        TrafficClass::Emit,
        TrafficClass::Bus,
        TrafficClass::ClipTransfer,
    ];

    /// Wire `class` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

impl From<TrafficClass> for CatchClass {
    fn from(t: TrafficClass) -> CatchClass {
        match t {
            TrafficClass::HidIn => CatchClass::HidIn,
            TrafficClass::HidOut => CatchClass::HidOut,
            TrafficClass::VendorInterrupt => CatchClass::VendorInterrupt,
            TrafficClass::VendorBulk => CatchClass::VendorBulk,
            TrafficClass::Control => CatchClass::Control,
            TrafficClass::Emit => CatchClass::Emit,
            TrafficClass::Bus => CatchClass::Bus,
            TrafficClass::ClipTransfer => CatchClass::ClipTransfer,
        }
    }
}

impl TryFrom<CatchClass> for TrafficClass {
    type Error = CatchClass;

    /// The traffic class, or the input class back as the error.
    fn try_from(c: CatchClass) -> Result<TrafficClass, CatchClass> {
        Ok(match c {
            CatchClass::HidIn => TrafficClass::HidIn,
            CatchClass::HidOut => TrafficClass::HidOut,
            CatchClass::VendorInterrupt => TrafficClass::VendorInterrupt,
            CatchClass::VendorBulk => TrafficClass::VendorBulk,
            CatchClass::Control => TrafficClass::Control,
            CatchClass::Emit => TrafficClass::Emit,
            CatchClass::Bus => TrafficClass::Bus,
            CatchClass::ClipTransfer => TrafficClass::ClipTransfer,
            input => return Err(input),
        })
    }
}

impl From<Class> for CatchClass {
    fn from(c: Class) -> CatchClass {
        match c {
            Class::Button => CatchClass::Button,
            Class::Key => CatchClass::Key,
            Class::Media => CatchClass::Media,
        }
    }
}

/// How much of each packet to keep.
///
/// Traffic classes only: an input class carries no packet, so a capture on one is refused. The
/// control link runs at 6 Mbaud, and a vendor bulk pipe at whole packets saturates it by itself.
///
/// A ceiling request: the box holds one entry per address and cuts once, so another subscriber
/// naming the same address more widely raises yours too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Capture {
    /// Keep the whole packet.
    #[default]
    Whole,
    /// Keep the first `n` bytes. `First(0)` is [`Capture::Whole`].
    First(u8),
}

impl Capture {
    /// Bytes kept per event, or `None` for the whole packet.
    pub fn bytes(self) -> Option<u8> {
        match self {
            Capture::Whole | Capture::First(0) => None,
            Capture::First(n) => Some(n),
        }
    }

    /// The wider of two: whole if either is whole, else the longer length.
    pub fn widest(self, other: Capture) -> Capture {
        match (self.bytes(), other.bytes()) {
            (Some(a), Some(b)) => Capture::First(a.max(b)),
            _ => Capture::Whole,
        }
    }

    /// Wire byte: 0 for the whole packet.
    pub fn as_u8(self) -> u8 {
        self.bytes().unwrap_or(0)
    }

    /// Decodes a wire byte.
    pub fn from_u8(v: u8) -> Capture {
        match v {
            0 => Capture::Whole,
            n => Capture::First(n),
        }
    }
}

// The box dedups its table on (class, id, direction), so the host collapses on exactly that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct FilterKey {
    class: Option<CatchClass>,
    id: Option<u16>,
    direction: Direction,
}

/// Subscription entry: what to observe, in which direction, and how much of each packet to keep.
///
/// The input constructors take what [`Device::lock`](crate::Device::lock) takes.
///
/// ```no_run
/// # use medius::{Axis, Capture, CatchFilter, Class, Key, TrafficClass};
/// CatchFilter::watch(Key::A);                    // one key, both edges
/// CatchFilter::watch(Key::A).on_press();         // one key, the press edge
/// CatchFilter::watch_class(Class::Key);          // every key and modifier
/// CatchFilter::watch_axis(Axis::X);              // one axis
/// CatchFilter::all_input();                      // buttons, keys, media, axes
///
/// CatchFilter::traffic_class(TrafficClass::HidIn);
/// CatchFilter::traffic(TrafficClass::VendorBulk, 3).with_capture(Capture::First(16));
/// CatchFilter::everything().with_capture(Capture::First(16));
/// ```
///
/// The box resolves each event to its most specific matching entry, which supplies the [`Capture`]:
/// an exact `(class, id)` outranks a class blanket, which outranks [`CatchFilter::everything`], and a
/// named direction outranks [`Direction::Both`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CatchFilter {
    class: Option<CatchClass>,
    id: Option<u16>,
    direction: Direction,
    capture: Capture,
}

impl CatchFilter {
    /// One button, key or media usage.
    pub fn watch(usage: impl Into<Usage>) -> CatchFilter {
        let u = usage.into();
        CatchFilter::exact(CatchClass::from(u.class), u.id)
    }

    /// One relative axis.
    pub fn watch_axis(axis: Axis) -> CatchFilter {
        CatchFilter::exact(CatchClass::Axis, axis.as_u16())
    }

    /// Every usage in one momentary class.
    pub fn watch_class(class: Class) -> CatchFilter {
        CatchFilter::blanket(CatchClass::from(class))
    }

    /// Every relative axis: X, Y, the wheel and AC Pan.
    pub fn watch_axes() -> CatchFilter {
        CatchFilter::blanket(CatchClass::Axis)
    }

    /// All four input classes: everything
    /// [`Device::input_events`](crate::Device::input_events) can report.
    pub fn all_input() -> [CatchFilter; 4] {
        [
            CatchFilter::watch_class(Class::Button),
            CatchFilter::watch_class(Class::Key),
            CatchFilter::watch_class(Class::Media),
            CatchFilter::watch_axes(),
        ]
    }

    /// One traffic address: an endpoint, interface or control endpoint number.
    pub fn traffic(class: TrafficClass, id: u16) -> CatchFilter {
        CatchFilter::exact(class.into(), id)
    }

    /// Every id within one traffic class.
    pub fn traffic_class(class: TrafficClass) -> CatchFilter {
        CatchFilter::blanket(class.into())
    }

    /// Every class, id and direction, whole packets, as one table entry.
    ///
    /// Includes [`TrafficClass::VendorBulk`], which can saturate the control link by itself; pair it
    /// with a [`Capture`] unless tracing bulk in full.
    pub fn everything() -> CatchFilter {
        CatchFilter {
            class: None,
            id: None,
            direction: Direction::Both,
            capture: Capture::Whole,
        }
    }

    fn exact(class: CatchClass, id: u16) -> CatchFilter {
        CatchFilter {
            class: Some(class),
            id: Some(id),
            direction: Direction::Both,
            capture: Capture::Whole,
        }
    }

    fn blanket(class: CatchClass) -> CatchFilter {
        CatchFilter {
            class: Some(class),
            id: None,
            direction: Direction::Both,
            capture: Capture::Whole,
        }
    }

    /// Restrict to one direction, sign or edge.
    pub fn with_direction(mut self, direction: Direction) -> CatchFilter {
        self.direction = direction;
        self
    }

    /// Only the press edge.
    pub fn on_press(self) -> CatchFilter {
        self.with_direction(Direction::PRESS)
    }

    /// Only the release edge.
    pub fn on_release(self) -> CatchFilter {
        self.with_direction(Direction::RELEASE)
    }

    /// Only device-to-PC traffic.
    pub fn inbound(self) -> CatchFilter {
        self.with_direction(Direction::IN)
    }

    /// Only PC-to-device traffic.
    pub fn outbound(self) -> CatchFilter {
        self.with_direction(Direction::OUT)
    }

    /// How much of each packet to keep. Traffic classes only; an input class with a capture is
    /// refused when the subscription is sent.
    pub fn with_capture(mut self, capture: Capture) -> CatchFilter {
        self.capture = capture.bytes().map_or(Capture::Whole, Capture::First);
        self
    }

    /// Addressed class; `None` for every class.
    pub fn class(self) -> Option<CatchClass> {
        self.class
    }

    /// Class-specific id; `None` for every id in the class.
    pub fn id(self) -> Option<u16> {
        self.id
    }

    /// Direction, sign or edge covered.
    pub fn direction(self) -> Direction {
        self.direction
    }

    /// How much of each packet is kept.
    pub fn capture(self) -> Capture {
        self.capture
    }

    /// Whether two filters name the same box table entry, whatever their captures.
    pub fn same_address(self, other: CatchFilter) -> bool {
        self.key() == other.key()
    }

    pub(crate) fn capture_is_meaningful(self) -> bool {
        self.capture == Capture::Whole || !self.class.is_some_and(CatchClass::is_input)
    }

    pub(crate) fn key(self) -> FilterKey {
        FilterKey {
            class: self.class,
            id: self.id,
            direction: self.direction,
        }
    }

    // A held-usage snapshot is the class's state, so it routes on class alone: a usage's release is
    // the snapshot that no longer lists it.
    pub(crate) fn matches_class_only(self, class: CatchClass) -> bool {
        self.class.is_none_or(|c| c == class)
    }

    pub(crate) fn matches(self, class: CatchClass, id: u16, direction: Direction) -> bool {
        if let Some(c) = self.class {
            if c != class {
                return false;
            }
            if self.id.is_some_and(|i| i != id) {
                return false;
            }
        }
        self.direction.admits(direction)
    }

    /// Wire `(class, id)`, wildcards as their sentinels (`0xFF` and `0xFFFF`).
    pub fn wire(self) -> (u8, u16) {
        (
            self.class.map_or(CATCH_CLS_ANY, CatchClass::as_u8),
            self.id.unwrap_or(CATCH_ID_ANY),
        )
    }

    /// Decodes the wire form; `None` if the four bytes address nothing the box would accept, such as
    /// the wildcard class with a real id (`id` means something different in every class).
    pub fn from_wire(class: u8, id: u16, direction: u8, capture: u8) -> Option<CatchFilter> {
        let class = if class == CATCH_CLS_ANY {
            None
        } else {
            Some(CatchClass::from_u8(class)?)
        };
        let id = if id == CATCH_ID_ANY { None } else { Some(id) };
        if class.is_none() && id.is_some() {
            return None;
        }
        Some(CatchFilter {
            class,
            id,
            direction: Direction::from_u8(direction)?,
            capture: Capture::from_u8(capture),
        })
    }
}

/// Row of [`CatchState::entries`]: a live subscription and what it has lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CatchEntry {
    /// Subscription as the box holds it.
    pub filter: CatchFilter,
    /// Events this entry could not queue.
    pub dropped: u16,
}

/// Decoded `RESP(CATCH)` (§4.9): live subscription table, drop counts, inter-chip clock estimate.
///
/// The box holds one table, so it is the union of every subscription in this process, not what any
/// single [`EventStream`](crate::EventStream) asked for.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CatchState {
    /// The box refused an entry because its table is full.
    pub table_full: bool,
    /// Box-wide events dropped under back-pressure.
    pub dropped: u32,
    /// Measured offset between the two chips' clocks.
    pub clock: ClockEstimate,
    /// Live subscription table.
    pub entries: Vec<CatchEntry>,
}

impl CatchState {
    pub(crate) const HDR: usize = 19;
    pub(crate) const ENTRY: usize = 7;

    pub(crate) fn from_payload(p: &[u8]) -> Option<CatchState> {
        if p.len() < Self::HDR {
            return None;
        }
        let n = p[18] as usize;
        let mut entries = Vec::with_capacity(n);
        for i in 0..n {
            let o = Self::HDR + Self::ENTRY * i;
            if o + Self::ENTRY > p.len() {
                break;
            }
            // An entry this build cannot name is skipped; the rest of the reply still decodes.
            let Some(filter) = CatchFilter::from_wire(
                p[o],
                u16::from_le_bytes([p[o + 1], p[o + 2]]),
                p[o + 3],
                p[o + 4],
            ) else {
                continue;
            };
            entries.push(CatchEntry {
                filter,
                dropped: u16::from_le_bytes([p[o + 5], p[o + 6]]),
            });
        }
        Some(CatchState {
            table_full: p[1] & 0x01 != 0,
            dropped: u32::from_le_bytes([p[2], p[3], p[4], p[5]]),
            clock: ClockEstimate::from_payload(&p[6..18])?,
            entries,
        })
    }

    /// Whether anything is subscribed.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
