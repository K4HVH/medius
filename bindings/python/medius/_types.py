"""Value types as dataclasses, the ctypes converters, and the parameter helpers."""

from __future__ import annotations

import ctypes
from dataclasses import dataclass, field
from typing import List, Optional, Union

from . import _native
from ._enums import (
    Axis,
    Blanket,
    BusEventKind,
    CatchClass,
    CatchEventKind,
    Class,
    ClipAction,
    ClipState,
    ClockDomain,
    ControlStatus,
    DeviceKind,
    Direction,
    Edge,
    EmitMode,
    InputKind,
    LockTargetKind,
    LogLevel,
    TrafficClass,
    Button,
    Key,
    MediaKey,
)


def _as_usage(usage) -> "Usage":
    """A `Usage`, a `Button` / `Key` / `MediaKey`, or a raw ctypes usage, as a `Usage`.

    The three key enums are accepted directly so an input is addressed the same way here as in
    `lock`: `CatchFilter.watch(Key.A)` rather than `CatchFilter.watch(Usage.key(Key.A))`. They are
    distinct types, so there is no ambiguity about which class a bare id belongs to.
    """
    if isinstance(usage, Usage):
        return usage
    if isinstance(usage, _native.MediusUsage):
        return Usage(usage)
    if isinstance(usage, Button):
        return Usage.button(usage)
    if isinstance(usage, Key):
        return Usage.key(usage)
    if isinstance(usage, MediaKey):
        return Usage.media(usage)
    raise TypeError(
        f"expected a Usage, Button, Key or MediaKey, got {type(usage).__name__}"
    )


def _cstr(buf) -> str:
    raw = bytes(buf)
    return raw.split(b"\x00", 1)[0].decode("utf-8", "replace")


@dataclass
class Version:
    proto_ver: int
    fw_major: int
    fw_minor: int
    fw_patch: int
    mac: bytes = b"\x00" * 6
    name: str = ""

    @property
    def mac_hex(self) -> str:
        """The base MAC as 12 lowercase hex digits, the canonical box id."""
        return self.mac.hex()


@dataclass
class Health:
    link_up: bool
    mouse_attached: bool
    clone_configured: bool
    injection_active: bool
    rate_confident: bool
    lock_on: bool
    catch_on: bool
    kbd_attached: bool


@dataclass
class DeviceInfo:
    vid: int
    pid: int
    bcd_device: int
    bcd_usb: int
    has_serial: bool
    has_bos: bool
    kind: DeviceKind
    product: str


@dataclass
class MouseCaps:
    n_buttons: int
    has_x: bool
    has_y: bool
    has_wheel: bool
    has_report_id: bool
    n_hid: int


@dataclass
class KbdCaps:
    n_keys: int
    nkro: bool
    has_consumer: bool
    has_system: bool
    has_report_id: bool


@dataclass
class Caps:
    mouse: MouseCaps
    keyboard: KbdCaps
    mouse_change_driven: bool
    kbd_change_driven: bool

    def has_mouse(self) -> bool:
        return bool(_native.lib.medius_caps_has_mouse(caps_to_c(self)))

    def has_keyboard(self) -> bool:
        return bool(_native.lib.medius_caps_has_keyboard(caps_to_c(self)))

    def is_composite(self) -> bool:
        return bool(_native.lib.medius_caps_is_composite(caps_to_c(self)))


@dataclass
class Rate:
    native_period_us: int
    poll_period_us: int
    confident: bool
    change_driven: bool

    def native_hz(self) -> Optional[float]:
        out = ctypes.c_float()
        if _native.lib.medius_rate_native_hz(rate_to_c(self), ctypes.byref(out)):
            return out.value
        return None


@dataclass
class Stats:
    inject_emits: int
    tx_drops: int
    tx_merges: int
    tx_maxdepth: int
    tx_wedges: int
    wakeups: int
    reset_count: int
    config_count: int


@dataclass
class LockEntry:
    """One active lock: what is locked and which edges."""

    target: "LockTarget"
    is_blanket: bool
    positive: bool
    negative: bool


@dataclass
class Locks:
    entries: List[LockEntry] = field(default_factory=list)

    def is_locked(self, target: "LockTarget", direction) -> bool:
        c = locks_to_c(self)
        return bool(
            _native.lib.medius_locks_is_locked(ctypes.byref(c), target._c, int(direction))
        )


@dataclass
class ClockEstimate:
    """The measured difference between the two chips' clocks, from RESP(CATCH)."""

    offset_us: int = 0
    # None when the box has fitted no rate. Not the same as a fitted 0, which says the two crystals
    # are matched: on a link too busy for enough clean exchanges no fit is made at all, which is
    # exactly when assuming no drift costs the most.
    rate_ppb: int | None = 0
    delay_us: int = 0
    # `None` is the box saying it has never measured, which an offset of zero also looks like.
    age_ms: Optional[int] = None

    @property
    def error_bound_us(self) -> int:
        """Half the measured round trip: the bound on how wrong `offset_us` can be."""
        return self.delay_us // 2

    def to_host_domain(self, device_us: int) -> Optional[int]:
        """A device-chip stamp on the host chip's timeline, or `None` when there is no estimate to apply."""
        if self.age_ms is None:
            return None
        return int(device_us) + self.offset_us


@dataclass
class CatchEntry:
    """One row of the box's subscription table: a live subscription and what it has lost."""

    filter: "CatchFilter"
    dropped: int = 0


@dataclass
class CatchState:
    """Decoded RESP(CATCH): the live subscription table, its drop counts, and the inter-chip clock estimate."""

    table_full: bool = False
    dropped: int = 0
    clock: ClockEstimate = field(default_factory=ClockEstimate)
    entries: List[CatchEntry] = field(default_factory=list)


@dataclass
class ImperfectStatus:
    allowed: bool
    over_capacity: bool
    clone_imperfect: bool


@dataclass(frozen=True)
class EmitPace:
    """What paces injected motion. Build with `EmitPace.learned/interval/fixed`."""

    mode: EmitMode
    hz: int = 0

    @classmethod
    def learned(cls) -> "EmitPace":
        return cls(EmitMode.LEARNED)

    @classmethod
    def interval(cls) -> "EmitPace":
        return cls(EmitMode.INTERVAL)

    @classmethod
    def fixed(cls, hz: int) -> "EmitPace":
        return cls(EmitMode.FIXED, int(hz))


@dataclass
class EmitPaceStatus:
    mode: EmitPace
    resolved_hz: int


@dataclass
class Counters:
    frames_tx: int
    frames_rx: int
    crc_drops: int
    reconnects: int


@dataclass
class PortInfo:
    path: str
    vid: int
    pid: int
    serial: Optional[str] = None


@dataclass
class BoxInfo:
    port: PortInfo
    version: Version
    device: "DeviceInfo"

    @property
    def id(self) -> str:
        """The canonical, stable box id: the device MAC as hex."""
        return self.version.mac_hex

    @property
    def name(self) -> str:
        """The box's human-readable name (its readable partner to `id`); a synthesized default when unset."""
        return self.version.name

    @property
    def serial(self) -> Optional[str]:
        return self.port.serial


@dataclass
class MotionEvent:
    """A relative-axis catch event: the user's real motion at the merge point."""

    dx: int
    dy: int
    dz: int


@dataclass
class UsageSnapshot:
    """A held-usage snapshot for one class: every held usage (button, key, or media).

    `cls` and `direction` come from the frame header, not from the entries. The snapshot that most
    needs them is the EMPTY one -- releasing the last held usage is the edge a caller waits for, and
    it lists nothing to read a class or an edge from.
    """

    usages: List["Usage"] = field(default_factory=list)
    cls: Class = Class.BUTTON
    direction: Direction = Direction.BOTH

    def __post_init__(self) -> None:
        # A snapshot carries one class, so its entries are what that class IS. Taking it from them
        # keeps a hand-built snapshot from claiming a class its own usages contradict; the header
        # field exists for the EMPTY snapshot, which has no entry to read it from.
        if self.usages:
            self.cls = Class(self.usages[0].kind)

    def is_held(self, usage: "Usage") -> bool:
        return any(u == usage for u in self.usages)


@dataclass
class BusEvent:
    """A decoded bus lifecycle event; the payload fields are 0 for the kinds that carry none."""

    kind: BusEventKind
    configuration: int = 0
    interface: int = 0
    alt: int = 0


@dataclass
class TrafficEvent:
    """One byte-oriented catch event: HID reports, vendor endpoints, control transactions, the bytes
    the clone emitted, or bus lifecycle.

    `bytes` is as much of the packet as the subscription's capture kept; `true_len` is its length
    before that truncation, so set both when building one by hand.
    """

    catch_class: CatchClass
    id: int
    direction: Direction
    flags: int = 0
    true_len: int = 0
    bytes: bytes = b""

    def truncated(self) -> bool:
        """Whether the capture cut this packet short; without it a cut capture reads as a short packet."""
        c = traffic_event_to_c(self)
        return bool(_native.lib.medius_traffic_event_truncated(ctypes.byref(c)))

    def setup(self) -> Optional[bytes]:
        """The 8-byte setup packet of a CONTROL event; `None` for another class or a shorter capture."""
        c = traffic_event_to_c(self)
        p = _native.lib.medius_traffic_event_setup(ctypes.byref(c))
        return bytes(p[:8]) if p else None

    def data(self) -> bytes:
        """The data stage of a CONTROL event, the whole packet for any other class."""
        c = traffic_event_to_c(self)
        n = _native.usize()
        p = _native.lib.medius_traffic_event_data(ctypes.byref(c), ctypes.byref(n))
        return bytes(p[: int(n.value)]) if p else b""

    def control_status(self) -> Optional[ControlStatus]:
        """What the real device answered; `None` for any class but CONTROL."""
        c = traffic_event_to_c(self)
        out = _native.u8()
        if _native.lib.medius_traffic_event_control_status(ctypes.byref(c), ctypes.byref(out)):
            return ControlStatus(out.value)
        return None

    def bus_event(self) -> Optional[BusEvent]:
        """The lifecycle event; `None` for any class but BUS or an unknown kind."""
        c = traffic_event_to_c(self)
        out = _native.MediusBusEvent()
        if _native.lib.medius_traffic_event_bus_event(ctypes.byref(c), ctypes.byref(out)):
            return BusEvent(BusEventKind(out.kind), out.configuration, out.interface, out.alt)
        return None

    def bulk_end_of_transfer(self) -> bool:
        """Whether this VENDOR_BULK event carries end-of-transfer."""
        c = traffic_event_to_c(self)
        return bool(_native.lib.medius_traffic_event_bulk_end_of_transfer(ctypes.byref(c)))

    def bulk_zlp(self) -> bool:
        """Whether this VENDOR_BULK event is a zero-length packet, which terminates a transfer."""
        c = traffic_event_to_c(self)
        return bool(_native.lib.medius_traffic_event_bulk_zlp(ctypes.byref(c)))


@dataclass
class CatchEvent:
    """One catch-stream event.

    `ts_us` is in the `clock` chip's microseconds. Both chips boot independently, so a stamp is a
    box-local value unrelated to any clock on this machine and only meaningful compared against
    another from the same domain; to cross domains apply `CatchState.clock`. Each wraps every ~71.6
    minutes and restarts at zero if that chip reboots, so a value below the previous one is a wrap, a
    reboot, or a domain change, and the delta across it is meaningless.
    """

    kind: CatchEventKind
    payload: Union[MotionEvent, UsageSnapshot, TrafficEvent]
    ts_us: int = 0
    clock: ClockDomain = ClockDomain.HOST_CHIP

    @property
    def motion(self) -> Optional[MotionEvent]:
        return self.payload if self.kind == CatchEventKind.MOTION else None

    @property
    def usages(self) -> Optional[UsageSnapshot]:
        return self.payload if self.kind == CatchEventKind.USAGES else None

    @property
    def traffic(self) -> Optional[TrafficEvent]:
        return self.payload if self.kind == CatchEventKind.TRAFFIC else None


@dataclass
class LogLine:
    level: LogLevel
    text: str


@dataclass
class RecordedFrame:
    type: int
    seq: int
    payload: bytes


class Usage:
    """A momentary usage (button, key, or media), all one shape. Build with `Usage.button`/`key`/`media`."""

    def __init__(self, c):
        self._c = c

    @classmethod
    def button(cls, button) -> "Usage":
        return cls(_native.lib.medius_usage_button(int(button)))

    @classmethod
    def key(cls, key) -> "Usage":
        return cls(_native.lib.medius_usage_key(int(key)))

    @classmethod
    def media(cls, media) -> "Usage":
        return cls(_native.lib.medius_usage_media(int(media)))

    @property
    def kind(self) -> Class:
        return Class(self._c.kind)

    @property
    def id(self) -> int:
        """The class-specific id: button id, HID keycode, or 16-bit Consumer usage."""
        return int(self._c.id)

    def __eq__(self, other) -> bool:
        return (
            isinstance(other, Usage)
            and self._c.kind == other._c.kind
            and self._c.id == other._c.id
        )

    def __hash__(self) -> int:
        return hash((int(self._c.kind), int(self._c.id)))

    def __repr__(self) -> str:
        return f"Usage(kind={self.kind.name}, id={self.id})"


class Motion:
    """A relative axis drive. Build with `Motion.cursor` / `Motion.wheel`."""

    def __init__(self, c):
        self._c = c

    @classmethod
    def cursor(cls, dx, dy) -> "Motion":
        return cls(_native.lib.medius_motion_cursor(int(dx), int(dy)))

    @classmethod
    def wheel(cls, delta) -> "Motion":
        return cls(_native.lib.medius_motion_wheel(int(delta)))


class LockTarget:
    """A lock target: an axis (`LockTarget.x/y/wheel`) or a momentary usage (`LockTarget.usage`)."""

    def __init__(self, c):
        self._c = c

    @classmethod
    def x(cls) -> "LockTarget":
        return cls(_native.lib.medius_lock_target_axis(int(LockTargetKind.X)))

    @classmethod
    def y(cls) -> "LockTarget":
        return cls(_native.lib.medius_lock_target_axis(int(LockTargetKind.Y)))

    @classmethod
    def wheel(cls) -> "LockTarget":
        return cls(_native.lib.medius_lock_target_axis(int(LockTargetKind.WHEEL)))

    @classmethod
    def usage(cls, usage: "Usage") -> "LockTarget":
        return cls(_native.lib.medius_lock_target_usage(usage._c))

    @classmethod
    def button(cls, button) -> "LockTarget":
        return cls.usage(Usage.button(button))

    @classmethod
    def key(cls, key) -> "LockTarget":
        return cls.usage(Usage.key(key))

    @classmethod
    def media(cls, media) -> "LockTarget":
        return cls.usage(Usage.media(media))

    @property
    def kind(self) -> LockTargetKind:
        return LockTargetKind(self._c.kind)

    @property
    def input(self) -> Optional["Usage"]:
        """The locked usage, when `kind` is `USAGE`; `None` for an axis."""
        if self._c.kind == int(LockTargetKind.USAGE):
            return Usage(_native.MediusUsage(kind=self._c.usage.kind, id=self._c.usage.id))
        return None


class Capture:
    """How much of each packet to keep. Traffic classes only.

    An input class carries no packet, so naming one together with a capture is refused rather than
    ignored. It exists because the control link runs at 4 Mbaud and a vendor bulk pipe at whole
    packets saturates it on its own. A ceiling request, not a guarantee: the box holds one entry per
    address and cuts once, so another subscriber naming it more widely raises yours too.
    """

    #: Keep the whole packet.
    WHOLE = 0

    @staticmethod
    def first(n: int) -> int:
        """Keep the first `n` bytes; `first(0)` is `WHOLE`."""
        return int(n)


class CatchFilter:
    """One CATCH subscription: what to observe, in which direction, and how much of each packet to keep.

    The input constructors take what `Device.lock` takes, so hiding an input from the game and
    watching it are written alike::

        CatchFilter.watch(Key.A)                      # one key, both edges
        CatchFilter.watch_class(Class.KEY)            # every key and modifier
        CatchFilter.watch_axis(Axis.X)                # one axis
        CatchFilter.all_input()                       # buttons, keys, media, axes

        CatchFilter.traffic_class(TrafficClass.HID_IN)
        CatchFilter.traffic(TrafficClass.VENDOR_BULK, 0x83).with_capture(16)
        CatchFilter.everything().with_capture(16)

    The box resolves each event to its most specific matching entry -- an exact `(class, id)` beats a
    class blanket, which beats `everything()`, and a named direction beats `BOTH` -- and that entry
    supplies the capture.
    """

    def __init__(self, c):
        self._c = c

    @classmethod
    def watch(cls, usage) -> "CatchFilter":
        """One momentary usage: a button, a key, or a media usage."""
        return cls(_native.lib.medius_catch_filter_watch(_as_usage(usage)._c))

    @classmethod
    def watch_axis(cls, axis: Axis) -> "CatchFilter":
        """One relative axis."""
        return cls(_native.lib.medius_catch_filter_watch_axis(int(axis)))

    @classmethod
    def watch_class(cls, input_class: Class) -> "CatchFilter":
        """Every usage in one momentary class."""
        return cls(_native.lib.medius_catch_filter_watch_class(int(input_class)))

    @classmethod
    def watch_axes(cls) -> "CatchFilter":
        """Every relative axis: X, Y and the wheel."""
        return cls(_native.lib.medius_catch_filter_watch_axes())

    @classmethod
    def all_input(cls) -> List["CatchFilter"]:
        """All four input classes, and the whole of what `Device.input_events` can report."""
        buf = (_native.MediusCatchFilter * 4)()
        _native.lib.medius_catch_filter_all_input(buf)
        return [cls(_native.MediusCatchFilter(f.class_, f.id, f.direction, f.capture)) for f in buf]

    @classmethod
    def traffic(cls, traffic_class: TrafficClass, id: int) -> "CatchFilter":
        """One traffic address: an endpoint, an interface, or a control endpoint number."""
        return cls(_native.lib.medius_catch_filter_traffic(int(traffic_class), int(id)))

    @classmethod
    def traffic_class(cls, traffic_class: TrafficClass) -> "CatchFilter":
        """Every id within one traffic class."""
        return cls(_native.lib.medius_catch_filter_traffic_class(int(traffic_class)))

    @classmethod
    def everything(cls) -> "CatchFilter":
        """Every class, every id, both directions, whole packets. One table entry, not an expansion.

        This includes `TrafficClass.VENDOR_BULK`, which can saturate the control link by itself.
        Pair it with `with_capture` unless you mean to trace bulk in full.
        """
        return cls(_native.lib.medius_catch_filter_everything())

    def with_direction(self, direction: Direction) -> "CatchFilter":
        """A copy restricted to one direction, sign or edge."""
        return CatchFilter(
            _native.lib.medius_catch_filter_with_direction(self._c, int(direction))
        )

    def with_capture(self, n: int) -> "CatchFilter":
        """A copy keeping only the first `n` bytes of each packet; 0 keeps the whole packet."""
        return CatchFilter(_native.lib.medius_catch_filter_with_capture(self._c, int(n)))

    def on_press(self) -> "CatchFilter":
        """A copy restricted to the press edge."""
        return CatchFilter(_native.lib.medius_catch_filter_on_press(self._c))

    def on_release(self) -> "CatchFilter":
        """A copy restricted to the release edge."""
        return CatchFilter(_native.lib.medius_catch_filter_on_release(self._c))

    def inbound(self) -> "CatchFilter":
        """A copy restricted to traffic from the device to the PC."""
        return CatchFilter(_native.lib.medius_catch_filter_inbound(self._c))

    def outbound(self) -> "CatchFilter":
        """A copy restricted to traffic from the PC to the device."""
        return CatchFilter(_native.lib.medius_catch_filter_outbound(self._c))

    def same_address(self, other: "CatchFilter") -> bool:
        """Whether both name the same box table entry, whatever their captures."""
        return bool(_native.lib.medius_catch_filter_same_address(self._c, other._c))

    @property
    def catch_class(self) -> Optional[CatchClass]:
        """The class observed, or `None` for the every-class wildcard."""
        if self._c.class_ == _native.MEDIUS_CATCH_CLASS_ANY:
            return None
        return CatchClass(self._c.class_)

    @property
    def id(self) -> Optional[int]:
        """The class-specific id, or `None` for the every-id wildcard."""
        if self._c.id == _native.MEDIUS_CATCH_ID_ANY:
            return None
        return int(self._c.id)

    @property
    def direction(self) -> Direction:
        """The edge, sign or flow this covers."""
        return Direction(self._c.direction)

    @property
    def capture(self) -> int:
        """Bytes kept per event; 0 keeps the whole packet."""
        return int(self._c.capture)

    def _key(self):
        return (int(self._c.class_), int(self._c.id), int(self._c.direction), int(self._c.capture))

    def __eq__(self, other) -> bool:
        return isinstance(other, CatchFilter) and self._key() == other._key()

    def __hash__(self) -> int:
        return hash(self._key())

    def __repr__(self) -> str:
        name = "ANY" if self.catch_class is None else self.catch_class.name
        ident = "ANY" if self.id is None else self.id
        return (
            f"CatchFilter(class={name}, id={ident}, "
            f"direction={self.direction.name}, capture={self.capture})"
        )


@dataclass(frozen=True)
class InputEvent:
    """One decoded input event: a press or release edge, or a motion report."""

    kind: InputKind
    ts_us: int
    clock: ClockDomain
    #: The usage this is an edge on; `None` for `MOTION`.
    usage: Optional["Usage"] = None
    #: Relative X this report (right positive); 0 unless `kind` is `MOTION`.
    dx: int = 0
    #: Relative Y this report (down positive); 0 unless `kind` is `MOTION`.
    dy: int = 0
    #: Wheel delta this report (up positive); 0 unless `kind` is `MOTION`.
    dz: int = 0

    @property
    def is_press(self) -> bool:
        return self.kind == InputKind.PRESS

    @property
    def is_release(self) -> bool:
        return self.kind == InputKind.RELEASE


@dataclass(frozen=True)
class Stamped:
    """One event placed on the caller's clock by a `Timeline`."""

    #: When the event happened, on the same monotonic scale passed as `now_ns`.
    host_ns: int
    #: The event's own stamp, unwrapped past the 32-bit rollover.
    box_us: int
    #: How much later than the measured floor this event arrived. Jitter, not latency.
    excess_ns: int


def input_event_from_c(c) -> InputEvent:
    kind = InputKind(c.kind)
    usage = None
    if kind != InputKind.MOTION:
        usage = Usage(_native.MediusUsage(kind=c.usage.kind, id=c.usage.id))
    return InputEvent(kind, int(c.ts_us), ClockDomain(c.clock), usage, int(c.dx), int(c.dy), int(c.dz))


def version_from_c(c) -> Version:
    return Version(
        c.proto_ver, c.fw_major, c.fw_minor, c.fw_patch, bytes(c.mac), _cstr(c.name)
    )


def version_to_c(v) -> "_native.MediusVersion":
    mac = bytes(v.mac).ljust(6, b"\x00")[:6]
    return _native.MediusVersion(
        v.proto_ver,
        v.fw_major,
        v.fw_minor,
        v.fw_patch,
        (_native.u8 * 6)(*mac),
        v.name.encode("utf-8")[: _native.MEDIUS_MAX_NAME - 1],
    )


def health_from_c(c) -> Health:
    return Health(
        bool(c.link_up),
        bool(c.mouse_attached),
        bool(c.clone_configured),
        bool(c.injection_active),
        bool(c.rate_confident),
        bool(c.lock_on),
        bool(c.catch_on),
        bool(c.kbd_attached),
    )


def health_to_c(h) -> "_native.MediusHealth":
    return _native.MediusHealth(
        int(h.link_up),
        int(h.mouse_attached),
        int(h.clone_configured),
        int(h.injection_active),
        int(h.rate_confident),
        int(h.lock_on),
        int(h.catch_on),
        int(h.kbd_attached),
    )


def _device_kind(v: int) -> DeviceKind:
    try:
        return DeviceKind(v)
    except ValueError:
        return DeviceKind.UNKNOWN


def device_info_from_c(c) -> DeviceInfo:
    return DeviceInfo(
        c.vid,
        c.pid,
        c.bcd_device,
        c.bcd_usb,
        bool(c.has_serial),
        bool(c.has_bos),
        _device_kind(c.kind),
        _cstr(c.product),
    )


def device_info_to_c(m) -> "_native.MediusDeviceInfo":
    return _native.MediusDeviceInfo(
        m.vid,
        m.pid,
        m.bcd_device,
        m.bcd_usb,
        int(m.has_serial),
        int(m.has_bos),
        int(m.kind),
        m.product.encode("utf-8")[: _native.MEDIUS_MAX_PRODUCT - 1],
    )


def port_from_c(c) -> PortInfo:
    serial = _cstr(c.serial) if c.has_serial else None
    return PortInfo(_cstr(c.path), c.vid, c.pid, serial)


def box_from_c(c) -> BoxInfo:
    return BoxInfo(port_from_c(c.port), version_from_c(c.version), device_info_from_c(c.device))


def mouse_caps_from_c(c) -> MouseCaps:
    return MouseCaps(
        c.n_buttons, bool(c.has_x), bool(c.has_y), bool(c.has_wheel), bool(c.has_report_id), c.n_hid
    )


def mouse_caps_to_c(m) -> "_native.MediusMouseCaps":
    return _native.MediusMouseCaps(
        m.n_buttons, int(m.has_x), int(m.has_y), int(m.has_wheel), int(m.has_report_id), m.n_hid
    )


def kbd_caps_from_c(c) -> KbdCaps:
    return KbdCaps(
        c.n_keys, bool(c.nkro), bool(c.has_consumer), bool(c.has_system), bool(c.has_report_id)
    )


def kbd_caps_to_c(k) -> "_native.MediusKbdCaps":
    return _native.MediusKbdCaps(
        k.n_keys, int(k.nkro), int(k.has_consumer), int(k.has_system), int(k.has_report_id)
    )


def caps_from_c(c) -> Caps:
    return Caps(
        mouse_caps_from_c(c.mouse),
        kbd_caps_from_c(c.keyboard),
        bool(c.mouse_change_driven),
        bool(c.kbd_change_driven),
    )


def caps_to_c(c) -> "_native.MediusCaps":
    return _native.MediusCaps(
        mouse_caps_to_c(c.mouse),
        kbd_caps_to_c(c.keyboard),
        int(c.mouse_change_driven),
        int(c.kbd_change_driven),
    )


def rate_from_c(c) -> Rate:
    return Rate(c.native_period_us, c.poll_period_us, bool(c.confident), bool(c.change_driven))


def rate_to_c(r) -> "_native.MediusRate":
    return _native.MediusRate(
        r.native_period_us, r.poll_period_us, int(r.confident), int(r.change_driven)
    )


def stats_from_c(c) -> Stats:
    return Stats(
        c.inject_emits,
        c.tx_drops,
        c.tx_merges,
        c.tx_maxdepth,
        c.tx_wedges,
        c.wakeups,
        c.reset_count,
        c.config_count,
    )


def stats_to_c(s) -> "_native.MediusStats":
    return _native.MediusStats(
        s.inject_emits,
        s.tx_drops,
        s.tx_merges,
        s.tx_maxdepth,
        s.tx_wedges,
        s.wakeups,
        s.reset_count,
        s.config_count,
    )


def catch_filter_from_c(c) -> CatchFilter:
    # Copy: a filter read out of a state buffer must outlive the buffer it was read from.
    return CatchFilter(_native.MediusCatchFilter(c.class_, c.id, c.direction, c.capture))


def clock_estimate_from_c(c) -> ClockEstimate:
    age = None if c.age_ms == _native.MEDIUS_CLOCK_AGE_NONE else int(c.age_ms)
    rate = None if c.rate_ppb == _native.MEDIUS_CLOCK_RATE_NONE else int(c.rate_ppb)
    return ClockEstimate(int(c.offset_us), rate, int(c.delay_us), age)


def clock_estimate_to_c(e) -> "_native.MediusClockEstimate":
    age = _native.MEDIUS_CLOCK_AGE_NONE if e.age_ms is None else int(e.age_ms)
    rate = _native.MEDIUS_CLOCK_RATE_NONE if e.rate_ppb is None else int(e.rate_ppb)
    return _native.MediusClockEstimate(e.offset_us, rate, e.delay_us, age)


def catch_state_from_c(c) -> CatchState:
    n = min(int(c.n), _native.MEDIUS_MAX_CATCH_ENTRIES)
    entries = [
        CatchEntry(catch_filter_from_c(c.entries[i].filter), int(c.entries[i].dropped))
        for i in range(n)
    ]
    return CatchState(bool(c.table_full), int(c.dropped), clock_estimate_from_c(c.clock), entries)


def catch_state_to_c(s) -> "_native.MediusCatchState":
    c = _native.MediusCatchState()
    c.table_full = 1 if s.table_full else 0
    c.dropped = s.dropped
    c.clock = clock_estimate_to_c(s.clock)
    n = min(len(s.entries), _native.MEDIUS_MAX_CATCH_ENTRIES)
    c.n = n
    for i in range(n):
        c.entries[i] = _native.MediusCatchEntry(s.entries[i].filter._c, s.entries[i].dropped)
    return c


def imperfect_from_c(c) -> ImperfectStatus:
    return ImperfectStatus(bool(c.allowed), bool(c.over_capacity), bool(c.clone_imperfect))


def imperfect_to_c(i) -> "_native.MediusImperfectStatus":
    return _native.MediusImperfectStatus(
        int(i.allowed), int(i.over_capacity), int(i.clone_imperfect)
    )


def emit_pace_status_from_c(c) -> EmitPaceStatus:
    mode = EmitMode(c.mode)
    return EmitPaceStatus(EmitPace(mode, c.fixed_hz), c.resolved_hz)


@dataclass
class ClipStatus:
    """The device-side clip ring and playback status (the runtime view of RESP(CLIP))."""

    state: ClipState
    free: int
    total: int
    played: int
    ticks: int
    underruns: int
    overruns: int
    seq_gaps: int
    held: List["Usage"] = field(default_factory=list)

    def is_held(self, usage: "Usage") -> bool:
        return any(u == usage for u in self.held)


def clip_status_from_c(c) -> ClipStatus:
    n = min(int(c.held_n), _native.MEDIUS_MAX_USAGES)
    held = [_input_copy(c.held[i]) for i in range(n)]
    return ClipStatus(
        ClipState(c.state),
        c.free,
        c.total,
        c.played,
        c.ticks,
        c.underruns,
        c.overruns,
        c.seq_gaps,
        held,
    )


def clip_status_to_c(s) -> "_native.MediusClipStatus":
    c = _native.MediusClipStatus()
    c.state = int(s.state)
    c.free = s.free
    c.total = s.total
    c.played = s.played
    c.ticks = s.ticks
    c.underruns = s.underruns
    c.overruns = s.overruns
    c.seq_gaps = s.seq_gaps
    n = min(len(s.held), _native.MEDIUS_MAX_USAGES)
    c.held_n = n
    for i in range(n):
        c.held[i] = s.held[i]._c
    return c


@dataclass
class ClipTrigger:
    """One clip trigger binding: `on`'s `edge` drives `action`; `consume` suppresses the input from the game."""

    on: "Usage"
    edge: Edge
    action: ClipAction
    consume: bool = False


@dataclass
class ClipSettings:
    """The clip configuration read back from RESP(CLIP): autolock, loop/retain, finalized, and the trigger set."""

    autolock: List[Blanket] = field(default_factory=list)
    loop: bool = False
    retain: bool = False
    finalized: bool = False
    triggers: List[ClipTrigger] = field(default_factory=list)


_BLANKET_BITS = [
    (0x01, Blanket.AIM),
    (0x02, Blanket.WHEEL),
    (0x04, Blanket.BUTTONS),
    (0x08, Blanket.KEYS),
    (0x10, Blanket.MEDIA),
]


def clip_settings_from_c(c) -> ClipSettings:
    n = min(int(c.n), _native.MEDIUS_CLIP_TRIG_MAX)
    triggers = [
        ClipTrigger(
            _input_copy(c.triggers[i].on),
            Edge(c.triggers[i].edge),
            ClipAction(c.triggers[i].action),
            bool(c.triggers[i].consume),
        )
        for i in range(n)
    ]
    autolock = [b for (m, b) in _BLANKET_BITS if c.autolock_bits & m]
    return ClipSettings(
        autolock,
        bool(c.loop_),
        bool(c.retain),
        bool(c.finalized),
        triggers,
    )


def clip_settings_to_c(s) -> "_native.MediusClipSettings":
    c = _native.MediusClipSettings()
    bit = {b: m for (m, b) in _BLANKET_BITS}
    c.autolock_bits = sum(bit[b] for b in s.autolock)
    c.loop_ = 1 if s.loop else 0
    c.retain = 1 if s.retain else 0
    c.finalized = 1 if s.finalized else 0
    n = min(len(s.triggers), _native.MEDIUS_CLIP_TRIG_MAX)
    c.n = n
    for i in range(n):
        t = s.triggers[i]
        c.triggers[i] = _native.MediusClipTrigger(
            t.on._c, int(t.edge), int(t.action), 1 if t.consume else 0
        )
    return c


def counters_from_c(c) -> Counters:
    return Counters(c.frames_tx, c.frames_rx, c.crc_drops, c.reconnects)


def _input_copy(c) -> Usage:
    return Usage(_native.MediusUsage(kind=c.kind, id=c.id))


def lock_target_from_c(c) -> LockTarget:
    return LockTarget(
        _native.MediusLockTarget(
            kind=c.kind, usage=_native.MediusUsage(kind=c.usage.kind, id=c.usage.id)
        )
    )


def lock_entry_from_c(c) -> LockEntry:
    return LockEntry(
        lock_target_from_c(c.target), bool(c.is_blanket), bool(c.positive), bool(c.negative)
    )


def locks_from_c(c) -> Locks:
    n = min(int(c.n), _native.MEDIUS_MAX_LOCKS)
    return Locks([lock_entry_from_c(c.entries[i]) for i in range(n)])


def locks_to_c(locks) -> "_native.MediusLocks":
    c = _native.MediusLocks()
    n = min(len(locks.entries), _native.MEDIUS_MAX_LOCKS)
    c.n = n
    for i in range(n):
        e = locks.entries[i]
        c.entries[i] = _native.MediusLockEntry(
            target=e.target._c,
            is_blanket=bool(e.is_blanket),
            positive=bool(e.positive),
            negative=bool(e.negative),
        )
    return c


def motion_event_to_c(e) -> "_native.MediusMotionEvent":
    return _native.MediusMotionEvent(e.dx, e.dy, e.dz)


def usage_snapshot_to_c(s) -> "_native.MediusUsageEvent":
    c = _native.MediusUsageEvent()
    c.class_ = int(s.cls)
    c.direction = int(s.direction)
    n = min(len(s.usages), _native.MEDIUS_MAX_USAGES)
    c.n = n
    for idx in range(n):
        c.usages[idx] = s.usages[idx]._c
    return c


def traffic_event_to_c(t) -> "_native.MediusTrafficEvent":
    c = _native.MediusTrafficEvent()
    c.class_ = int(t.catch_class)
    c.id = int(t.id)
    c.direction = int(t.direction)
    c.flags = int(t.flags)
    c.true_len = int(t.true_len)
    raw = bytes(t.bytes)[: _native.MEDIUS_MAX_TRAFFIC_BYTES]
    c.len = len(raw)
    for i, b in enumerate(raw):
        c.bytes[i] = b
    return c


def traffic_event_from_c(c) -> TrafficEvent:
    n = min(int(c.len), _native.MEDIUS_MAX_TRAFFIC_BYTES)
    return TrafficEvent(
        CatchClass(c.class_),
        int(c.id),
        Direction(c.direction),
        int(c.flags),
        int(c.true_len),
        bytes(c.bytes[:n]),
    )


def decode_catch_event(c) -> CatchEvent:
    kind = CatchEventKind(c.kind)
    clock = ClockDomain(c.clock)
    if kind == CatchEventKind.MOTION:
        m = c.data.motion
        return CatchEvent(kind, MotionEvent(m.dx, m.dy, m.dz), c.ts_us, clock)
    if kind == CatchEventKind.TRAFFIC:
        return CatchEvent(kind, traffic_event_from_c(c.data.traffic), c.ts_us, clock)
    u = c.data.usages
    n = min(int(u.n), _native.MEDIUS_MAX_USAGES)
    usages = [_input_copy(u.usages[i]) for i in range(n)]
    snap = UsageSnapshot(usages, Class(u.class_), Direction(u.direction))
    return CatchEvent(kind, snap, c.ts_us, clock)


def decode_log_line(c) -> LogLine:
    return LogLine(LogLevel(c.level), _cstr(c.text))
