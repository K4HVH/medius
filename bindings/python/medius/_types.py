"""Value types as dataclasses, the ctypes converters, and the parameter helpers."""

from __future__ import annotations

import ctypes
from dataclasses import dataclass, field
from typing import List, Optional, Union

from . import _native
from ._enums import (
    CatchEventKind,
    ClipState,
    DeviceKind,
    EmitMode,
    InputKind,
    LockTargetKind,
    LogLevel,
)


def _cstr(buf) -> str:
    raw = bytes(buf)
    return raw.split(b"\x00", 1)[0].decode("utf-8", "replace")


# --- query results ---


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
        """The base MAC as 12 lowercase hex digits — the canonical box id."""
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
    """One active lock: what is locked and which edges. `is_blanket` marks a whole-class lock (every
    button / key / media usage), where `target` names only the class."""

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
class CatchState:
    mask: int
    dropped: int


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


# --- catch / log payloads ---


@dataclass
class MotionEvent:
    """A relative-axis catch event: the user's real motion at the merge point, before any lock
    suppression or injection."""

    dx: int
    dy: int
    dz: int


@dataclass
class UsageSnapshot:
    """A held-usage snapshot for one class: every held usage (button / key / media; modifiers are key
    usages 0xE0..0xE7). A button press and a key press have the same shape."""

    usages: List["Input"] = field(default_factory=list)

    def is_held(self, usage: "Input") -> bool:
        return any(u == usage for u in self.usages)


@dataclass
class CatchEvent:
    kind: CatchEventKind
    payload: Union[MotionEvent, UsageSnapshot]

    @property
    def motion(self) -> Optional[MotionEvent]:
        return self.payload if self.kind == CatchEventKind.MOTION else None

    @property
    def usages(self) -> Optional[UsageSnapshot]:
        return self.payload if self.kind == CatchEventKind.USAGES else None


@dataclass
class LogLine:
    level: LogLevel
    text: str


@dataclass
class RecordedFrame:
    type: int
    seq: int
    payload: bytes


# --- parameter helpers (wrap a ctypes struct built by the C constructors) ---


class Input:
    """A momentary usage (a button, key, or media usage — all one shape). Build with `Input.button` /
    `key` / `media`. The same value drives `inject`/`press`/`lock` and appears in a `UsageSnapshot`."""

    def __init__(self, c):
        self._c = c

    @classmethod
    def button(cls, button) -> "Input":
        return cls(_native.lib.medius_input_button(int(button)))

    @classmethod
    def key(cls, key) -> "Input":
        return cls(_native.lib.medius_input_key(int(key)))

    @classmethod
    def media(cls, media) -> "Input":
        return cls(_native.lib.medius_input_media(int(media)))

    @property
    def kind(self) -> InputKind:
        return InputKind(self._c.kind)

    @property
    def value(self) -> int:
        """The class-specific id: button id, HID keycode, or 16-bit Consumer usage."""
        return int(self._c.value)

    def __eq__(self, other) -> bool:
        return (
            isinstance(other, Input)
            and self._c.kind == other._c.kind
            and self._c.value == other._c.value
        )

    def __hash__(self) -> int:
        return hash((int(self._c.kind), int(self._c.value)))

    def __repr__(self) -> str:
        return f"Input(kind={self.kind.name}, value={self.value})"


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
    """A lock target: an axis (`LockTarget.x/y/wheel`) or a momentary usage (`LockTarget.usage`, or the
    `button`/`key`/`media` shortcuts). A button, key, and media usage all lock the same way."""

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
    def usage(cls, usage: "Input") -> "LockTarget":
        return cls(_native.lib.medius_lock_target_usage(usage._c))

    @classmethod
    def button(cls, button) -> "LockTarget":
        return cls.usage(Input.button(button))

    @classmethod
    def key(cls, key) -> "LockTarget":
        return cls.usage(Input.key(key))

    @classmethod
    def media(cls, media) -> "LockTarget":
        return cls.usage(Input.media(media))

    @property
    def kind(self) -> LockTargetKind:
        return LockTargetKind(self._c.kind)

    @property
    def input(self) -> Optional["Input"]:
        """The locked usage, when `kind` is `USAGE`; `None` for an axis."""
        if self._c.kind == int(LockTargetKind.USAGE):
            return Input(_native.MediusInput(kind=self._c.usage.kind, value=self._c.usage.value))
        return None


# --- ctypes <-> dataclass conversion ---


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


def catch_state_from_c(c) -> CatchState:
    return CatchState(c.mask, c.dropped)


def catch_state_to_c(c) -> "_native.MediusCatchState":
    return _native.MediusCatchState(mask=c.mask, dropped=c.dropped)


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
    """The device-side clip ring and playback status. `free`/`used` pace top-ups; `state == FAULTED`
    means re-sync (stop + rebuild). `held` is the held-usage snapshot: the buttons, keys, and media the
    clip is holding down, keyed like a `UsageSnapshot`."""

    state: ClipState
    free: int
    used: int
    ticks: int
    underruns: int
    overruns: int
    seq_gaps: int
    held: List["Input"] = field(default_factory=list)

    def is_held(self, usage: "Input") -> bool:
        return any(u == usage for u in self.held)


def clip_status_from_c(c) -> ClipStatus:
    n = min(int(c.held_n), _native.MEDIUS_MAX_USAGES)
    held = [_input_copy(c.held[i]) for i in range(n)]
    return ClipStatus(
        ClipState(c.state),
        c.free,
        c.used,
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
    c.used = s.used
    c.ticks = s.ticks
    c.underruns = s.underruns
    c.overruns = s.overruns
    c.seq_gaps = s.seq_gaps
    n = min(len(s.held), _native.MEDIUS_MAX_USAGES)
    c.held_n = n
    for i in range(n):
        c.held[i] = s.held[i]._c
    return c


def counters_from_c(c) -> Counters:
    return Counters(c.frames_tx, c.frames_rx, c.crc_drops, c.reconnects)


def _input_copy(c) -> Input:
    return Input(_native.MediusInput(kind=c.kind, value=c.value))


def lock_target_from_c(c) -> LockTarget:
    return LockTarget(
        _native.MediusLockTarget(
            kind=c.kind, usage=_native.MediusInput(kind=c.usage.kind, value=c.usage.value)
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
    n = min(len(s.usages), _native.MEDIUS_MAX_USAGES)
    c.n = n
    for idx in range(n):
        c.usages[idx] = s.usages[idx]._c
    return c


def decode_catch_event(c) -> CatchEvent:
    kind = CatchEventKind(c.kind)
    if kind == CatchEventKind.MOTION:
        m = c.data.motion
        return CatchEvent(kind, MotionEvent(m.dx, m.dy, m.dz))
    u = c.data.usages
    n = min(int(u.n), _native.MEDIUS_MAX_USAGES)
    usages = [_input_copy(u.usages[i]) for i in range(n)]
    return CatchEvent(kind, UsageSnapshot(usages))


def decode_log_line(c) -> LogLine:
    return LogLine(LogLevel(c.level), _cstr(c.text))
