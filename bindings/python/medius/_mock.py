"""The scriptable mock box (feature = mock); errors clearly if the library lacks it."""

from __future__ import annotations

import ctypes
from typing import Optional

from . import _native
from ._device import Device
from ._enums import FrameType, LogLevel
from ._types import (
    Caps,
    CatchState,
    ClipSettings,
    ClipStatus,
    EmitPace,
    Health,
    ImperfectStatus,
    DeviceInfo,
    KbdCaps,
    Locks,
    MotionEvent,
    MouseCaps,
    Rate,
    RecordedFrame,
    Stats,
    UsageSnapshot,
    Version,
    caps_to_c,
    catch_state_to_c,
    clip_settings_to_c,
    clip_status_to_c,
    device_info_to_c,
    health_to_c,
    imperfect_to_c,
    kbd_caps_to_c,
    locks_to_c,
    motion_event_to_c,
    mouse_caps_to_c,
    rate_to_c,
    stats_to_c,
    usage_snapshot_to_c,
    version_to_c,
)


class MockBox:
    """A scriptable in-process fake box for hardware-free testing."""

    def __init__(self):
        if not _native.HAS_MOCK:
            raise RuntimeError(
                "the loaded medius_capi library was built without the mock feature "
                "(rebuild with --features mock)"
            )
        self._handle = _native.lib.medius_mock_new()
        if not self._handle:
            raise RuntimeError("medius_mock_new returned null")

    def open(self) -> Device:
        """Open a `Device` over this mock and run the handshake."""
        return Device.open_mock(self)

    def with_device(self) -> Device:
        """Open a `Device` over this mock without a handshake."""
        return Device.with_mock(self)

    def clone(self) -> "MockBox":
        """Another handle sharing the same recorded state."""
        handle = _native.lib.medius_mock_clone(self._handle)
        if not handle:
            raise RuntimeError("medius_mock_clone failed")
        other = MockBox.__new__(MockBox)
        other._handle = handle
        return other

    def set_version(self, version: Version):
        _native.lib.medius_mock_set_version(self._handle, version_to_c(version))

    def set_health(self, health: Health):
        _native.lib.medius_mock_set_health(self._handle, health_to_c(health))

    def set_device_info(self, info: DeviceInfo):
        _native.lib.medius_mock_set_device_info(self._handle, device_info_to_c(info))

    def set_caps(self, caps: Caps):
        _native.lib.medius_mock_set_caps(self._handle, caps_to_c(caps))

    def set_mouse_caps(self, caps: MouseCaps):
        _native.lib.medius_mock_set_mouse_caps(self._handle, mouse_caps_to_c(caps))

    def set_kbd_caps(self, caps: KbdCaps):
        _native.lib.medius_mock_set_kbd_caps(self._handle, kbd_caps_to_c(caps))

    def set_rate(self, rate: Rate):
        _native.lib.medius_mock_set_rate(self._handle, rate_to_c(rate))

    def set_stats(self, stats: Stats):
        _native.lib.medius_mock_set_stats(self._handle, stats_to_c(stats))

    def set_locks(self, locks: Locks):
        _native.lib.medius_mock_set_locks(self._handle, locks_to_c(locks))

    def set_catch_state(self, state: CatchState):
        _native.lib.medius_mock_set_catch_state(self._handle, catch_state_to_c(state))

    def set_imperfect_status(self, status: ImperfectStatus):
        _native.lib.medius_mock_set_imperfect_status(self._handle, imperfect_to_c(status))

    def set_movement_riding(self, window_ms: Optional[int]):
        enabled = window_ms is not None
        _native.lib.medius_mock_set_movement_riding(
            self._handle, enabled, int(window_ms) if enabled else 0
        )

    def set_emit_pace(self, pace: EmitPace):
        _native.lib.medius_mock_set_emit_pace(self._handle, int(pace.mode), int(pace.hz))

    def set_clip_status(self, status: ClipStatus):
        """Set the `ClipStatus` the mock answers to `ClipHandle.query_status`."""
        _native.lib.medius_mock_set_clip_status(self._handle, clip_status_to_c(status))

    def set_clip_settings(self, settings: "ClipSettings"):
        """Set the `ClipSettings` the mock answers to `ClipHandle.query_config`."""
        _native.lib.medius_mock_set_clip_settings(self._handle, clip_settings_to_c(settings))

    def silent(self):
        """Make the mock stop answering queries (one-way, for timeout tests)."""
        _native.lib.medius_mock_silent(self._handle)

    def push_raw(self, data: bytes):
        if not data:
            return
        buf = (_native.u8 * len(data)).from_buffer_copy(bytes(data))
        _native.lib.medius_mock_push_raw(self._handle, buf, len(data))

    def push_log(self, level: LogLevel, text: str):
        _native.lib.medius_mock_push_log(self._handle, int(level), text.encode("utf-8"))

    def push_motion(self, seq: int, ts_us: int, event: MotionEvent):
        _native.lib.medius_mock_push_motion(self._handle, seq, ts_us, motion_event_to_c(event))

    def push_usages(self, seq: int, ts_us: int, event: UsageSnapshot):
        c = usage_snapshot_to_c(event)
        _native.lib.medius_mock_push_usages(self._handle, seq, ts_us, ctypes.byref(c))

    def recorded(self) -> int:
        return int(_native.lib.medius_mock_recorded(self._handle))

    def saw(self, frame_type: FrameType) -> bool:
        return bool(_native.lib.medius_mock_saw(self._handle, int(frame_type)))

    def clear_recorded(self):
        _native.lib.medius_mock_clear_recorded(self._handle)

    def recorded_frame(self, idx: int) -> Optional[RecordedFrame]:
        if idx < 0 or idx >= self.recorded():
            return None
        cap = 512
        out_ty = _native.u8()
        out_seq = _native.u8()
        buf = (_native.u8 * cap)()
        full = _native.lib.medius_mock_recorded_frame(
            self._handle, idx, ctypes.byref(out_ty), ctypes.byref(out_seq), buf, cap
        )
        payload = bytes(buf[: min(full, cap)])
        try:
            ty = FrameType(out_ty.value)
        except ValueError:
            ty = out_ty.value
        return RecordedFrame(ty, out_seq.value, payload)

    def close(self):
        if self._handle is not None:
            _native.lib.medius_mock_free(self._handle)
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
