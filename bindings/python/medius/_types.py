"""Value types as dataclasses, the ctypes converters, and the parameter helpers."""

from __future__ import annotations

import ctypes
from dataclasses import dataclass, field
from typing import List, Optional, Union

from . import _native
from ._enums import (
    Button,
    CatchEventKind,
    EmitMode,
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
class MouseInfo:
    vid: int
    pid: int
    bcd_device: int
    bcd_usb: int
    has_serial: bool
    has_bos: bool


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
class Locks:
    mask: int

    def is_locked(self, target: "LockTarget", direction) -> bool:
        return bool(
            _native.lib.medius_locks_is_locked(
                _native.MediusLocks(mask=self.mask), target._c, int(direction)
            )
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


# --- catch / log payloads ---


@dataclass
class MouseEvent:
    buttons: int
    dx: int
    dy: int
    wheel: int

    def is_pressed(self, button) -> bool:
        return self.buttons & (1 << int(button)) != 0


@dataclass
class KeyboardEvent:
    modifiers: int = 0
    keys: List[int] = field(default_factory=list)

    def is_pressed(self, key) -> bool:
        key = int(key)
        if 0xE0 <= key <= 0xE7:
            return self.modifiers & (1 << (key - 0xE0)) != 0
        return key in self.keys


@dataclass
class MediaEvent:
    keys: List[int] = field(default_factory=list)

    def is_pressed(self, media) -> bool:
        return int(media) in self.keys


@dataclass
class CatchEvent:
    kind: CatchEventKind
    payload: Union[MouseEvent, KeyboardEvent, MediaEvent]

    @property
    def mouse(self) -> Optional[MouseEvent]:
        return self.payload if self.kind == CatchEventKind.MOUSE else None

    @property
    def keyboard(self) -> Optional[KeyboardEvent]:
        return self.payload if self.kind == CatchEventKind.KEYBOARD else None

    @property
    def media(self) -> Optional[MediaEvent]:
        return self.payload if self.kind == CatchEventKind.MEDIA else None

    def is_pressed(self, target) -> bool:
        return self.payload.is_pressed(target)


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
    """An injection target. Build with `Input.button` / `key` / `media`."""

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
    """A lock target. Build with `LockTarget.x/y/wheel/button`."""

    def __init__(self, c):
        self._c = c

    @classmethod
    def x(cls) -> "LockTarget":
        return cls(_native.MediusLockTarget(kind=int(LockTargetKind.X), button=int(Button.LEFT)))

    @classmethod
    def y(cls) -> "LockTarget":
        return cls(_native.MediusLockTarget(kind=int(LockTargetKind.Y), button=int(Button.LEFT)))

    @classmethod
    def wheel(cls) -> "LockTarget":
        return cls(
            _native.MediusLockTarget(kind=int(LockTargetKind.WHEEL), button=int(Button.LEFT))
        )

    @classmethod
    def button(cls, button) -> "LockTarget":
        return cls(_native.MediusLockTarget(kind=int(LockTargetKind.BUTTON), button=int(button)))


# --- ctypes <-> dataclass conversion ---


def version_from_c(c) -> Version:
    return Version(c.proto_ver, c.fw_major, c.fw_minor, c.fw_patch)


def version_to_c(v) -> "_native.MediusVersion":
    return _native.MediusVersion(v.proto_ver, v.fw_major, v.fw_minor, v.fw_patch)


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


def mouse_info_from_c(c) -> MouseInfo:
    return MouseInfo(c.vid, c.pid, c.bcd_device, c.bcd_usb, bool(c.has_serial), bool(c.has_bos))


def mouse_info_to_c(m) -> "_native.MediusMouseInfo":
    return _native.MediusMouseInfo(
        m.vid, m.pid, m.bcd_device, m.bcd_usb, int(m.has_serial), int(m.has_bos)
    )


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


def counters_from_c(c) -> Counters:
    return Counters(c.frames_tx, c.frames_rx, c.crc_drops, c.reconnects)


def mouse_event_to_c(e) -> "_native.MediusMouseEvent":
    return _native.MediusMouseEvent(e.buttons, e.dx, e.dy, e.wheel)


def keyboard_event_to_c(e) -> "_native.MediusKeyboardEvent":
    c = _native.MediusKeyboardEvent()
    c.modifiers = e.modifiers
    n = min(len(e.keys), 0xFF)  # the count is a u8, so cap at 255
    c.n_keys = n
    for idx in range(n):
        c.keys[idx] = int(e.keys[idx]) & 0xFF
    return c


def media_event_to_c(e) -> "_native.MediusMediaEvent":
    c = _native.MediusMediaEvent()
    n = min(len(e.keys), 0xFF)
    c.n_keys = n
    for idx in range(n):
        c.keys[idx] = int(e.keys[idx]) & 0xFFFF
    return c


def decode_catch_event(c) -> CatchEvent:
    kind = CatchEventKind(c.kind)
    if kind == CatchEventKind.MOUSE:
        m = c.data.mouse
        return CatchEvent(kind, MouseEvent(m.buttons, m.dx, m.dy, m.wheel))
    if kind == CatchEventKind.KEYBOARD:
        k = c.data.keyboard
        n = min(k.n_keys, _native.MEDIUS_MAX_KEYS)
        return CatchEvent(kind, KeyboardEvent(k.modifiers, list(k.keys[:n])))
    md = c.data.media
    n = min(md.n_keys, _native.MEDIUS_MAX_MEDIA_KEYS)
    return CatchEvent(kind, MediaEvent(list(md.keys[:n])))


def decode_log_line(c) -> LogLine:
    return LogLine(LogLevel(c.level), _cstr(c.text))
