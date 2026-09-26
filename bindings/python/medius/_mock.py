"""The scriptable mock box (feature = mock); raises if the library was built without it."""

from __future__ import annotations

import ctypes
from typing import Optional, Tuple

from . import _native
from ._device import Device
from ._enums import (BearingMode, ClipAction, ClipState, ClockDomain, DeviceKind, Direction, EmitMode,
                     FrameType, LogLevel, RenderMode, TrafficClass)
from ._types import (
    Bearing,
    Caps,
    _enum,
    _as_bytes,
    _u8,
    _u16,
    _window_ms,
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
    TrafficEvent,
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
    traffic_event_to_c,
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
        """Set the mock's `Device.query_imperfect` reply. With the opt-in off the mock drops its
        consuming clip packet triggers, as the box does."""
        _native.lib.medius_mock_set_imperfect_status(self._handle, imperfect_to_c(status))

    def set_transfer_reply(self, status, data: bytes = b""):
        """Set the canned (status, IN data) reply to a TRANSFER while the opt-in is on; with it off
        the mock replies REFUSED. `status` is a `TransferStatus` or a raw wire byte."""
        raw = _as_bytes(data, "data")
        buf = (_native.u8 * len(raw)).from_buffer_copy(raw)
        _native.lib.medius_mock_set_transfer_reply(
            self._handle, _u8(int(status), "status"), buf, len(raw)
        )

    def set_advertised_hz(self, hz: int):
        """The rate the mock's clone advertises unforced; 0 means no clone."""
        _native.lib.medius_mock_set_advertised_hz(self._handle, _u16(hz, "hz"))

    def set_movement_riding(self, window_ms: Optional[int]):
        enabled = window_ms is not None
        _native.lib.medius_mock_set_movement_riding(
            self._handle, enabled, int(window_ms) if enabled else 0
        )

    def set_bearing(self, bearing: Bearing):
        """Set the mock's `Device.query_bearing` reply."""
        _native.lib.medius_mock_set_bearing(
            self._handle,
            _window_ms(bearing.window_ms),
            int(_enum(bearing.mode, BearingMode, "mode")),
        )

    def set_emit_pace(self, pace: EmitPace, force_hz: Optional[int] = None):
        mode = _enum(pace.mode, EmitMode, "mode")
        _native.lib.medius_mock_set_emit_pace(
            self._handle, int(mode), _u16(pace.hz, "hz"), _u16(force_hz or 0, "force_hz")
        )

    def set_spread_learned(self, period_us: int):
        """The mock's learned command period, in microseconds. A real box learns it from MOVE
        arrivals, so a mock left at 0 replies with a span of 0 whatever the percent."""
        _native.lib.medius_mock_set_spread_learned(self._handle, int(period_us))

    def set_render(self, mode: RenderMode, full: bool, ready: bool):
        """The mock's `Device.query_render` reply. `ready` is whether a profile is armed, which
        gates rendering on a real box; an unarmed mock is every box's state after a power cut."""
        _native.lib.medius_mock_set_render(
            self._handle, int(_enum(mode, RenderMode, "mode")), bool(full), bool(ready)
        )

    def set_clip_status(self, status: ClipStatus):
        """Set the mock's `ClipHandle.query_status` reply."""
        _native.lib.medius_mock_set_clip_status(self._handle, clip_status_to_c(status))

    def set_clip_settings(self, settings: "ClipSettings"):
        """Set the mock's `ClipHandle.query_config` reply.

        Its packet triggers are bound in order, as `ClipHandle.bind_packet` binds them, under the
        opt-in the mock holds at scripting time. The mock keeps those the box would, each with its
        scripted ``hits``, and drops the rest as the box's reply would: a direction the class never
        carries, a match bit outside the mask, a run with no condition, ``consume`` on ``CONTROL``
        or with the opt-in off, and entries past the match pool. Script the opt-in with
        `set_imperfect_status` before a consuming trigger. `ClipHandle.bind_packet` adds to the held
        set and `clip_packet` runs packets through it."""
        _native.lib.medius_mock_set_clip_settings(self._handle, clip_settings_to_c(settings))

    def clip_packet(
        self, traffic_class: TrafficClass, id: int, direction: Direction, head: bytes
    ) -> Tuple[Optional[ClipAction], bool]:
        """Run one packet through the packet triggers, as the box does for a packet crossing
        `traffic_class` at `id` in `direction` with first bytes `head`. The most specific matching
        trigger counts it in ``hits``.

        Returns the action that trigger drives on this packet, and whether it consumes the packet.
        The action is `None` when no trigger matches, or when a ``once_per_run`` trigger's run
        continues.

        A packet travels ``IN`` or ``OUT`` across a surface carrying that flow: ``IN`` for
        ``HID_IN`` and ``EMIT``, ``OUT`` for ``HID_OUT``, either for the vendor classes and
        ``CONTROL``. Any other `traffic_class` and `direction` is no packet: it returns ``(None,
        False)``, counts in no ``hits`` and leaves every run unchanged."""
        raw = _as_bytes(head, "head")
        buf = (_native.u8 * len(raw)).from_buffer_copy(raw)
        action, consumed = _native.u8(), _native.c_bool()
        fired = _native.lib.medius_mock_clip_packet(
            self._handle,
            int(_enum(traffic_class, TrafficClass, "traffic_class")),
            _u16(id, "id"),
            int(_enum(direction, Direction, "direction")),
            buf,
            len(raw),
            ctypes.byref(action),
            ctypes.byref(consumed),
        )
        return (ClipAction(action.value) if fired else None, bool(consumed.value))

    def restart(self):
        """Simulate a device-chip restart: the mock drops its session state, keeps its stored state,
        sends its hello now and on the next frame it receives, and has its clone back 100 ms
        later."""
        _native.lib.medius_mock_restart(self._handle)

    def link_lost(self):
        """Simulate an inter-chip link drop and recovery: the mock releases host-set session state
        (counted in `Stats.session`); the clone stays up."""
        _native.lib.medius_mock_link_lost(self._handle)

    def detach(self, back_within_grace: bool = False):
        """Simulate the real device detaching: the mock releases host-set session state. With
        `back_within_grace` the same device re-attaches inside the 250 ms grace and the clone stays
        up; otherwise the clone is torn down when the grace ends (counted again only if a command
        arrived during it) and stays down until `attach`."""
        _native.lib.medius_mock_detach(self._handle, bool(back_within_grace))

    def attach(self):
        """Simulate the device attaching again: inside a detach's grace the clone stays as it is; after
        teardown a fresh clone starts with nothing to release."""
        _native.lib.medius_mock_attach(self._handle)

    def silent(self):
        """Make the mock stop replying to queries (one-way, for timeout tests)."""
        _native.lib.medius_mock_silent(self._handle)

    def push_raw(self, data: bytes):
        if not data:
            return
        buf = (_native.u8 * len(data)).from_buffer_copy(bytes(data))
        _native.lib.medius_mock_push_raw(self._handle, buf, len(data))

    def push_log(self, level: LogLevel, text: str):
        level = _enum(level, LogLevel, "level")
        _native.lib.medius_mock_push_log(self._handle, int(level), text.encode("utf-8"))

    def push_motion(self, seq: int, ts_us: int, event: MotionEvent):
        _native.lib.medius_mock_push_motion(self._handle, seq, ts_us, motion_event_to_c(event))

    def push_usages(self, seq: int, ts_us: int, event: UsageSnapshot):
        c = usage_snapshot_to_c(event)
        _native.lib.medius_mock_push_usages(self._handle, seq, ts_us, ctypes.byref(c))

    def push_traffic(self, seq: int, ts_us: int, clock: ClockDomain, event: TrafficEvent):
        """Push a TRAFFIC_EVENT; a `true_len` above the byte count makes a cut capture."""
        clock = _enum(clock, ClockDomain, "clock")
        c = traffic_event_to_c(event)
        _native.lib.medius_mock_push_traffic(self._handle, seq, ts_us, int(clock), ctypes.byref(c))

    def recorded(self) -> int:
        return int(_native.lib.medius_mock_recorded(self._handle))

    def saw(self, frame_type: FrameType) -> bool:
        frame_type = _enum(frame_type, FrameType, "frame_type")
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
