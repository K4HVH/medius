"""The scriptable mock box (feature = mock). Degrades to a clear error if the
loaded library was built without the mock feature."""

from __future__ import annotations

import ctypes
from typing import Optional

from . import _native
from ._device import Device
from ._enums import FrameType, LogLevel
from ._types import (
    Caps,
    CatchState,
    EmitPace,
    Health,
    ImperfectStatus,
    KbdCaps,
    KeyboardEvent,
    MediaEvent,
    MouseCaps,
    MouseEvent,
    MouseInfo,
    Rate,
    RecordedFrame,
    Stats,
    Version,
    caps_to_c,
    catch_state_to_c,
    health_to_c,
    imperfect_to_c,
    kbd_caps_to_c,
    keyboard_event_to_c,
    media_event_to_c,
    mouse_caps_to_c,
    mouse_event_to_c,
    mouse_info_to_c,
    rate_to_c,
    stats_to_c,
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

    # --- open a device over this mock ---

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

    # --- query answers ---

    def set_version(self, version: Version):
        _native.lib.medius_mock_set_version(self._handle, version_to_c(version))

    def set_health(self, health: Health):
        _native.lib.medius_mock_set_health(self._handle, health_to_c(health))

    def set_mouse_info(self, info: MouseInfo):
        _native.lib.medius_mock_set_mouse_info(self._handle, mouse_info_to_c(info))

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

    def set_locks(self, mask: int):
        _native.lib.medius_mock_set_locks(self._handle, _native.MediusLocks(mask=mask))

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

    def silent(self):
        """Make the mock stop answering queries (one-way, for timeout tests)."""
        _native.lib.medius_mock_silent(self._handle)

    # --- push inbound traffic ---

    def push_raw(self, data: bytes):
        if not data:
            return
        buf = (_native.u8 * len(data)).from_buffer_copy(bytes(data))
        _native.lib.medius_mock_push_raw(self._handle, buf, len(data))

    def push_log(self, level: LogLevel, text: str):
        _native.lib.medius_mock_push_log(self._handle, int(level), text.encode("utf-8"))

    def push_event(self, seq: int, event: MouseEvent):
        _native.lib.medius_mock_push_event(self._handle, seq, mouse_event_to_c(event))

    def push_kb_event(self, seq: int, event: KeyboardEvent):
        c = keyboard_event_to_c(event)
        _native.lib.medius_mock_push_kb_event(self._handle, seq, ctypes.byref(c))

    def push_cons_event(self, seq: int, event: MediaEvent):
        c = media_event_to_c(event)
        _native.lib.medius_mock_push_cons_event(self._handle, seq, ctypes.byref(c))

    # --- recorded commands ---

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

    # --- lifecycle ---

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
