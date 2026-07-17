"""The Device wrapper: commands, queries, and stream factories."""

from __future__ import annotations

import ctypes
from typing import Optional

from . import _native
from ._enums import Action, Blanket, CatchMask, LedMode, LedTarget, LockDirection, RebootTarget, Status
from ._errors import MediusError, check
from ._clip import ClipHandle
from ._streams import EventStream, LogStream
from ._types import (
    Caps,
    CatchState,
    Counters,
    EmitPace,
    EmitPaceStatus,
    Health,
    ImperfectStatus,
    Usage,
    Locks,
    LockTarget,
    DeviceInfo,
    Motion,
    Rate,
    Stats,
    Version,
    caps_from_c,
    catch_state_from_c,
    counters_from_c,
    device_info_from_c,
    emit_pace_status_from_c,
    health_from_c,
    imperfect_from_c,
    locks_from_c,
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
    def open_by_id(cls, box_id: str) -> "Device":
        """Open the box whose identity matches `box_id` (device MAC hex or CH343 serial)."""
        out = ctypes.c_void_p()
        check(_native.lib.medius_device_open_by_id(box_id.encode("utf-8"), ctypes.byref(out)))
        return cls(out.value)

    @classmethod
    def find_mouse_box(cls) -> "Device":
        """Open the first box whose clone is a mouse."""
        out = ctypes.c_void_p()
        check(_native.lib.medius_device_find_mouse_box(ctypes.byref(out)))
        return cls(out.value)

    @classmethod
    def find_keyboard_box(cls) -> "Device":
        """Open the first box whose clone is a keyboard."""
        out = ctypes.c_void_p()
        check(_native.lib.medius_device_find_keyboard_box(ctypes.byref(out)))
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

    def move_rel(self, dx, dy):
        check(_native.lib.medius_device_move_rel(self._handle, dx, dy))

    def wheel(self, delta):
        check(_native.lib.medius_device_wheel(self._handle, delta))

    def move_axis(self, motion: Motion):
        check(_native.lib.medius_device_move_axis(self._handle, motion._c))

    def inject(self, input: Usage, action: Action):
        check(_native.lib.medius_device_inject(self._handle, input._c, int(action)))

    def press(self, input: Usage):
        check(_native.lib.medius_device_press(self._handle, input._c))

    def soft_release(self, input: Usage):
        check(_native.lib.medius_device_soft_release(self._handle, input._c))

    def force_release(self, input: Usage):
        check(_native.lib.medius_device_force_release(self._handle, input._c))

    def lock(self, target: LockTarget, direction: LockDirection):
        check(_native.lib.medius_device_lock(self._handle, target._c, int(direction)))

    def unlock(self, target: LockTarget, direction: LockDirection):
        check(_native.lib.medius_device_unlock(self._handle, target._c, int(direction)))

    def lock_all(self, what: Blanket, direction: LockDirection):
        check(_native.lib.medius_device_lock_all(self._handle, int(what), int(direction)))

    def unlock_all(self, what: Blanket, direction: LockDirection):
        check(_native.lib.medius_device_unlock_all(self._handle, int(what), int(direction)))

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

    def set_name(self, name: str):
        """Set the box's persistent human-readable name; an empty string clears it."""
        check(_native.lib.medius_device_set_name(self._handle, name.encode("utf-8")))

    def clear_name(self):
        """Clear the custom name, reverting the box to its synthesized `Medius-XXXX` default."""
        check(_native.lib.medius_device_clear_name(self._handle))

    def query_version(self) -> Version:
        out = _native.MediusVersion()
        check(_native.lib.medius_device_query_version(self._handle, ctypes.byref(out)))
        return version_from_c(out)

    def query_health(self) -> Health:
        out = _native.MediusHealth()
        check(_native.lib.medius_device_query_health(self._handle, ctypes.byref(out)))
        return health_from_c(out)

    def device_info(self) -> DeviceInfo:
        out = _native.MediusDeviceInfo()
        check(_native.lib.medius_device_device_info(self._handle, ctypes.byref(out)))
        return device_info_from_c(out)

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
        return locks_from_c(out)

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

    def clip(self) -> ClipHandle:
        """A handle to this box's buffered-clip playback (§3.11)."""
        out = ctypes.c_void_p()
        check(_native.lib.medius_device_clip(self._handle, ctypes.byref(out)))
        return ClipHandle(out.value, self)

    def catch_events(self, mask: CatchMask = CatchMask.ALL) -> EventStream:
        out = ctypes.c_void_p()
        check(_native.lib.medius_device_catch_events(self._handle, int(mask), ctypes.byref(out)))
        return EventStream(out.value, self)

    def logs(self) -> LogStream:
        out = ctypes.c_void_p()
        check(_native.lib.medius_device_logs(self._handle, ctypes.byref(out)))
        return LogStream(out.value, self)

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
