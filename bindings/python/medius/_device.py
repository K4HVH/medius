"""The Device wrapper: commands, queries, and stream factories."""

from __future__ import annotations

import ctypes
from typing import Optional

from . import _native
from ._enums import Action, Blanket, Button, CatchMask, LedMode, LedTarget, LockDirection, RebootTarget, Status
from ._errors import MediusError, check
from ._streams import EventStream, LogStream
from ._types import (
    Caps,
    CatchState,
    Counters,
    EmitPace,
    EmitPaceStatus,
    Health,
    ImperfectStatus,
    Input,
    Locks,
    LockTarget,
    Motion,
    MouseInfo,
    Rate,
    Stats,
    Version,
    caps_from_c,
    catch_state_from_c,
    counters_from_c,
    emit_pace_status_from_c,
    health_from_c,
    imperfect_from_c,
    mouse_info_from_c,
    rate_from_c,
    stats_from_c,
    version_from_c,
)


def _require_mock():
    if not _native.HAS_MOCK:
        raise RuntimeError(
            "the loaded medius_capi library was built without the mock feature "
            "(rebuild with --features mock)"
        )


class Device:
    """An open connection to one medius box."""

    def __init__(self, handle):
        self._handle = handle

    @classmethod
    def open(cls, path) -> "Device":
        out = ctypes.c_void_p()
        check(_native.lib.medius_device_open(path.encode("utf-8"), ctypes.byref(out)))
        return cls(out.value)

    @classmethod
    def find(cls) -> "Device":
        out = ctypes.c_void_p()
        check(_native.lib.medius_device_find(ctypes.byref(out)))
        return cls(out.value)

    @classmethod
    def open_mock(cls, mock) -> "Device":
        """Build a device over a `MockBox` and run the version handshake."""
        _require_mock()
        out = ctypes.c_void_p()
        check(_native.lib.medius_device_open_mock(mock._handle, ctypes.byref(out)))
        return cls(out.value)

    @classmethod
    def with_mock(cls, mock) -> "Device":
        """Build a device over a `MockBox` without a handshake."""
        _require_mock()
        out = ctypes.c_void_p()
        check(_native.lib.medius_device_with_mock(mock._handle, ctypes.byref(out)))
        return cls(out.value)

    def clone(self) -> "Device":
        """Another handle to the same connection; the link is shared."""
        handle = _native.lib.medius_device_clone(self._handle)
        if not handle:
            raise MediusError(Status.ERR_UNKNOWN, "device clone failed")
        return Device(handle)

    # --- movement ---

    def move_rel(self, dx, dy):
        check(_native.lib.medius_device_move_rel(self._handle, dx, dy))

    def wheel(self, delta):
        check(_native.lib.medius_device_wheel(self._handle, delta))

    def move_axis(self, motion: Motion):
        check(_native.lib.medius_device_move_axis(self._handle, motion._c))

    # --- injection ---

    def inject(self, input: Input, action: Action):
        check(_native.lib.medius_device_inject(self._handle, input._c, int(action)))

    def button(self, button: Button, action: Action):
        check(_native.lib.medius_device_button(self._handle, int(button), int(action)))

    def press(self, button: Button):
        check(_native.lib.medius_device_press(self._handle, int(button)))

    def soft_release(self, button: Button):
        check(_native.lib.medius_device_soft_release(self._handle, int(button)))

    def force_release(self, button: Button):
        check(_native.lib.medius_device_force_release(self._handle, int(button)))

    def key(self, key, action: Action):
        check(_native.lib.medius_device_key(self._handle, int(key), int(action)))

    def key_down(self, key):
        check(_native.lib.medius_device_key_down(self._handle, int(key)))

    def key_up(self, key):
        check(_native.lib.medius_device_key_up(self._handle, int(key)))

    def key_force_release(self, key):
        check(_native.lib.medius_device_key_force_release(self._handle, int(key)))

    def media(self, media, action: Action):
        check(_native.lib.medius_device_media(self._handle, int(media), int(action)))

    def media_down(self, media):
        check(_native.lib.medius_device_media_down(self._handle, int(media)))

    def media_up(self, media):
        check(_native.lib.medius_device_media_up(self._handle, int(media)))

    def media_force_release(self, media):
        check(_native.lib.medius_device_media_force_release(self._handle, int(media)))

    # --- locks ---

    def lock(self, target: LockTarget, direction: LockDirection):
        check(_native.lib.medius_device_lock(self._handle, target._c, int(direction)))

    def unlock(self, target: LockTarget, direction: LockDirection):
        check(_native.lib.medius_device_unlock(self._handle, target._c, int(direction)))

    def lock_key(self, key, direction: LockDirection):
        check(_native.lib.medius_device_lock_key(self._handle, int(key), int(direction)))

    def unlock_key(self, key, direction: LockDirection):
        check(_native.lib.medius_device_unlock_key(self._handle, int(key), int(direction)))

    def lock_media(self, media):
        check(_native.lib.medius_device_lock_media(self._handle, int(media)))

    def unlock_media(self, media):
        check(_native.lib.medius_device_unlock_media(self._handle, int(media)))

    def lock_all(self, what: Blanket):
        check(_native.lib.medius_device_lock_all(self._handle, int(what)))

    def unlock_all(self, what: Blanket):
        check(_native.lib.medius_device_unlock_all(self._handle, int(what)))

    # --- led / admin ---

    def led(self, target: LedTarget, mode: LedMode, level):
        check(_native.lib.medius_device_led(self._handle, int(target), int(mode), int(level)))

    def reset(self):
        check(_native.lib.medius_device_reset(self._handle))

    def reapply(self):
        check(_native.lib.medius_device_reapply(self._handle))

    def reconnect(self):
        check(_native.lib.medius_device_reconnect(self._handle))

    def reboot(self, target: RebootTarget):
        check(_native.lib.medius_device_reboot(self._handle, int(target)))

    def allow_imperfect_clones(self, allow: bool):
        check(_native.lib.medius_device_allow_imperfect_clones(self._handle, bool(allow)))

    def set_movement_riding(self, window_ms: Optional[int]):
        """Set the movement-riding window in ms, or `None` to turn it off."""
        enabled = window_ms is not None
        check(
            _native.lib.medius_device_set_movement_riding(
                self._handle, enabled, int(window_ms) if enabled else 0
            )
        )

    def set_emit_pace(self, pace: EmitPace):
        """Set what paces injected motion (`hz` matters only for `EmitPace.fixed`)."""
        check(_native.lib.medius_device_set_emit_pace(self._handle, int(pace.mode), int(pace.hz)))

    # --- queries ---

    def query_version(self) -> Version:
        out = _native.MediusVersion()
        check(_native.lib.medius_device_query_version(self._handle, ctypes.byref(out)))
        return version_from_c(out)

    def query_health(self) -> Health:
        out = _native.MediusHealth()
        check(_native.lib.medius_device_query_health(self._handle, ctypes.byref(out)))
        return health_from_c(out)

    def query_mouse_info(self) -> MouseInfo:
        out = _native.MediusMouseInfo()
        check(_native.lib.medius_device_query_mouse_info(self._handle, ctypes.byref(out)))
        return mouse_info_from_c(out)

    def caps(self) -> Caps:
        out = _native.MediusCaps()
        check(_native.lib.medius_device_caps(self._handle, ctypes.byref(out)))
        return caps_from_c(out)

    def query_rate(self) -> Rate:
        out = _native.MediusRate()
        check(_native.lib.medius_device_query_rate(self._handle, ctypes.byref(out)))
        return rate_from_c(out)

    def query_stats(self) -> Stats:
        out = _native.MediusStats()
        check(_native.lib.medius_device_query_stats(self._handle, ctypes.byref(out)))
        return stats_from_c(out)

    def query_locks(self) -> Locks:
        out = _native.MediusLocks()
        check(_native.lib.medius_device_query_locks(self._handle, ctypes.byref(out)))
        return Locks(out.mask)

    def query_catch(self) -> CatchState:
        out = _native.MediusCatchState()
        check(_native.lib.medius_device_query_catch(self._handle, ctypes.byref(out)))
        return catch_state_from_c(out)

    def query_imperfect(self) -> ImperfectStatus:
        out = _native.MediusImperfectStatus()
        check(_native.lib.medius_device_query_imperfect(self._handle, ctypes.byref(out)))
        return imperfect_from_c(out)

    def query_movement_riding(self) -> Optional[int]:
        """The movement-riding window in whole ms, or `None` when off."""
        enabled = _native.c_bool()
        window = _native.u32()
        check(
            _native.lib.medius_device_query_movement_riding(
                self._handle, ctypes.byref(enabled), ctypes.byref(window)
            )
        )
        return int(window.value) if enabled.value else None

    def query_emit_pace(self) -> EmitPaceStatus:
        out = _native.MediusEmitPaceStatus()
        check(_native.lib.medius_device_query_emit_pace(self._handle, ctypes.byref(out)))
        return emit_pace_status_from_c(out)

    def counters(self) -> Counters:
        out = _native.MediusCountersSnapshot()
        check(_native.lib.medius_device_counters(self._handle, ctypes.byref(out)))
        return counters_from_c(out)

    # --- streams ---

    def catch_events(self, mask: CatchMask = CatchMask.ALL) -> EventStream:
        out = ctypes.c_void_p()
        check(_native.lib.medius_device_catch_events(self._handle, int(mask), ctypes.byref(out)))
        return EventStream(out.value, self)

    def logs(self) -> LogStream:
        out = ctypes.c_void_p()
        check(_native.lib.medius_device_logs(self._handle, ctypes.byref(out)))
        return LogStream(out.value, self)

    # --- lifecycle ---

    def close(self):
        if self._handle is not None:
            _native.lib.medius_device_free(self._handle)
            self._handle = None

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()

    def __del__(self):
        try:
            self.close()
        except Exception:
            pass
