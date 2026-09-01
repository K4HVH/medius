"""The Device wrapper: commands, queries, and stream factories."""

from __future__ import annotations

import ctypes
import time
from typing import Optional, Sequence, Union

from . import _native
from ._enums import (Action, BearingMode, Blanket, EmitMode, RenderMode, LedMode, LedTarget, Direction,
                     MoveTiming, PendingMotion, RebootTarget, Status, UpdateTarget)
from ._errors import InvalidArgError, MediusError, check
from ._clip import ClipHandle
from ._streams import EventStream, InputStream, LogStream
from ._types import (
    Bearing,
    FirmwareInfo,
    firmware_info_from_c,
    _enum,
    _i16,
    _u8,
    _u16,
    _window_ms,
    Caps,
    CatchFilter,
    CatchState,
    Counters,
    EmitPace,
    EmitPaceStatus,
    RenderStatus,
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
    bearing_from_c,
    caps_from_c,
    catch_state_from_c,
    counters_from_c,
    device_info_from_c,
    emit_pace_status_from_c,
    render_status_from_c,
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
        check(_native.lib.medius_device_move_rel(self._handle, _i16(dx, "dx"), _i16(dy, "dy")))

    def wheel(self, delta):
        check(_native.lib.medius_device_wheel(self._handle, _i16(delta, "delta")))

    def move_rel_now(self, dx, dy):
        """A cursor move that bypasses movement riding: it emits on the box's own clock."""
        check(_native.lib.medius_device_move_rel_now(self._handle, _i16(dx, "dx"), _i16(dy, "dy")))

    def wheel_now(self, delta):
        """A wheel move that bypasses movement riding."""
        check(_native.lib.medius_device_wheel_now(self._handle, _i16(delta, "delta")))

    def flush_motion(self):
        """Emit the motion held for a ride now, ignoring the ride window."""
        check(_native.lib.medius_device_flush_motion(self._handle))

    def discard_motion(self):
        """Drop the motion held for a ride."""
        check(_native.lib.medius_device_discard_motion(self._handle))

    def move_axis(self, motion: Motion, timing: MoveTiming = MoveTiming.RIDE,
                  pending: PendingMotion = PendingMotion.KEEP):
        timing = _enum(timing, MoveTiming, "timing")
        pending = _enum(pending, PendingMotion, "pending")
        check(_native.lib.medius_device_move_axis(self._handle, motion._c, int(timing), int(pending)))

    def inject(self, input: Usage, action: Action):
        action = _enum(action, Action, "action")
        check(_native.lib.medius_device_inject(self._handle, input._c, int(action)))

    def press(self, input: Usage):
        check(_native.lib.medius_device_press(self._handle, input._c))

    def soft_release(self, input: Usage):
        check(_native.lib.medius_device_soft_release(self._handle, input._c))

    def force_release(self, input: Usage):
        check(_native.lib.medius_device_force_release(self._handle, input._c))

    def lock(self, target: LockTarget, direction: Direction):
        direction = _enum(direction, Direction, "direction")
        check(_native.lib.medius_device_lock(self._handle, target._c, int(direction)))

    def unlock(self, target: LockTarget, direction: Direction):
        direction = _enum(direction, Direction, "direction")
        check(_native.lib.medius_device_unlock(self._handle, target._c, int(direction)))

    def scale(self, target: LockTarget, direction: Direction, scale: int):
        """Weigh physical input on a target and direction.

        `scale` is the percent of the physical value the box keeps: 0 blocks, 100 passes it
        untouched, above that amplifies to 255 (2.55x). `lock` and `unlock` are its two ends.

        A delta picks up at most two scales, its absolute direction's and its relative direction's,
        and they multiply, so a block anywhere wins. `Direction.BOTH` is the exception: it writes the
        scale to the two fixed signs and a full pass to the relative pair, so a `BOTH` of 50 is 50%
        with or without a bearing rather than 25% with one. Name a relative direction to weigh it.

        `Direction.WITH` and `Direction.AGAINST` need a live bearing (see `set_bearing`) and only an
        axis has one, so either on a button, key or media usage raises `RelativeDirectionError`. A
        momentary usage carries one bit, so any scale below a full pass locks it and any scale at or
        above one unlocks it. A media usage has no edges and is sent as `Direction.BOTH` whatever
        edge is named, which is what `query_locks` reports it as.
        """
        direction = _enum(direction, Direction, "direction")
        check(
            _native.lib.medius_device_scale(
                self._handle, target._c, int(direction), _u8(scale, "scale")
            )
        )

    def scale_all(self, what: Blanket, direction: Direction, scale: int):
        """Weigh a whole class blanket; see `scale` for what the number means."""
        what = _enum(what, Blanket, "what")
        direction = _enum(direction, Direction, "direction")
        check(
            _native.lib.medius_device_scale_all(
                self._handle, int(what), int(direction), _u8(scale, "scale")
            )
        )

    def lock_all(self, what: Blanket, direction: Direction):
        """Block a whole class blanket.

        `Blanket.KEYS` honours the direction: `POSITIVE` blocks press edges only, `NEGATIVE` release
        edges only.
        """
        what = _enum(what, Blanket, "what")
        direction = _enum(direction, Direction, "direction")
        check(_native.lib.medius_device_lock_all(self._handle, int(what), int(direction)))

    def unlock_all(self, what: Blanket, direction: Direction):
        what = _enum(what, Blanket, "what")
        direction = _enum(direction, Direction, "direction")
        check(_native.lib.medius_device_unlock_all(self._handle, int(what), int(direction)))

    def led(self, target: LedTarget, mode: LedMode, level):
        target = _enum(target, LedTarget, "target")
        mode = _enum(mode, LedMode, "mode")
        check(
            _native.lib.medius_device_led(
                self._handle, int(target), int(mode), _u8(level, "level")
            )
        )

    def reset(self):
        check(_native.lib.medius_device_reset(self._handle))

    def reapply(self):
        check(_native.lib.medius_device_reapply(self._handle))

    def reconnect(self):
        check(_native.lib.medius_device_reconnect(self._handle))

    def reboot(self, target: RebootTarget):
        target = _enum(target, RebootTarget, "target")
        check(_native.lib.medius_device_reboot(self._handle, int(target)))

    def allow_imperfect_clones(self, allow: bool):
        check(_native.lib.medius_device_allow_imperfect_clones(self._handle, bool(allow)))

    def set_movement_riding(self, window_ms: Optional[int]):
        """Set the movement-riding window in ms, or `None` to turn it off."""
        enabled = window_ms is not None
        check(
            _native.lib.medius_device_set_movement_riding(
                self._handle, enabled, _window_ms(window_ms)
            )
        )

    def set_bearing(self, window_ms: Optional[int], mode: BearingMode):
        """Set what `Direction.WITH` and `Direction.AGAINST` are measured against.

        `window_ms` is how long the last injected delta's direction stays the bearing; `None` turns
        it off, leaving the relative directions inert whatever their scale. It saturates at 65535 ms,
        as the Rust API does.

        Both fields ride one frame and the box persists them together, so `mode` is required: a
        default here would revert a box configured for `VECTOR` on any window change.
        """
        mode = _enum(mode, BearingMode, "mode")
        check(
            _native.lib.medius_device_set_bearing(
                self._handle, _window_ms(window_ms), int(mode)
            )
        )

    def set_emit_pace(self, pace: EmitPace, force_hz: Optional[int] = None):
        """Set what paces injected motion (`hz` matters only for `EmitPace.fixed`) and what rate the
        clone advertises and the box polls the device at (`force_hz`, None = the device's own)."""
        mode = _enum(pace.mode, EmitMode, "mode")
        check(
            _native.lib.medius_device_set_emit_pace(
                self._handle, int(mode), _u16(pace.hz, "hz"), _u16(force_hz or 0, "force_hz"),
            )
        )

    def set_render(self, mode: RenderMode, full: bool):
        """Set the texture the box draws motion with, and whether the device's own motion is drawn by
        the model rather than relayed.

        Both ride one command and both persist, so `full` is required: an omitted one would silently
        rewrite a setting you did not name. `full` costs roughly 3 ms of latency on physical mouse
        movement and is off by default. Nothing is drawn until the box has learned a profile for the
        attached device (`RenderStatus.ready`)."""
        mode = _enum(mode, RenderMode, "mode")
        check(_native.lib.medius_device_set_render(self._handle, int(mode), bool(full)))

    def set_name(self, name: str):
        """Set the box's persistent human-readable name; an empty string clears it."""
        check(_native.lib.medius_device_set_name(self._handle, name.encode("utf-8")))

    def clear_name(self):
        """Clear the custom name, reverting the box to its synthesised `Medius-XXXX` default."""
        check(_native.lib.medius_device_clear_name(self._handle))

    def query_version(self) -> Version:
        out = _native.MediusVersion()
        check(_native.lib.medius_device_query_version(self._handle, ctypes.byref(out)))
        return version_from_c(out)

    def firmware_info(self) -> FirmwareInfo:
        """Both chips' firmware versions and which app slot each booted."""
        out = _native.MediusFirmwareInfo()
        check(_native.lib.medius_device_firmware_info(self._handle, ctypes.byref(out)))
        return firmware_info_from_c(out)

    def wait_firmware_confirmed(self, timeout: float = 45.0) -> FirmwareInfo:
        """Block until neither chip is still on probation; a chip on probation refuses an update."""
        deadline = time.monotonic() + timeout
        while True:
            info = self.firmware_info()
            if not info.any_pending():
                return info
            if time.monotonic() >= deadline:
                raise RuntimeError(f"still on probation after {timeout}s")
            time.sleep(0.5)

    def stage_firmware(self, target: UpdateTarget, image: bytes, progress=None) -> None:
        """Write one image into that chip's spare slot; it stays inert until activate_firmware()."""
        if not image:
            raise ValueError("image is empty")
        buf = (ctypes.c_uint8 * len(image)).from_buffer_copy(image)

        def _cb(_user, sent, total):
            if progress is not None:
                progress(int(sent), int(total))

        cb = _native.UPDATE_PROGRESS_CB(_cb)
        check(
            _native.lib.medius_device_stage_firmware(
                self._handle, int(target), buf, len(image), cb, None
            )
        )

    def abort_update(self, target: UpdateTarget) -> None:
        """Drop whatever is staged or in flight for one target."""
        check(_native.lib.medius_device_abort_update(self._handle, int(target)))

    def activate_firmware(self) -> None:
        """Commit every staged image and reboot into it; the host chip goes first."""
        check(_native.lib.medius_device_activate_firmware(self._handle))

    def update_firmware(self, target: UpdateTarget, image: bytes, progress=None) -> None:
        """Stage one image and activate it."""
        self.stage_firmware(target, image, progress)
        self.activate_firmware()

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

    def query_bearing(self) -> Bearing:
        """The configured bearing: its window and how it is read."""
        out = _native.MediusBearing()
        check(_native.lib.medius_device_query_bearing(self._handle, ctypes.byref(out)))
        return bearing_from_c(out)

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

    def query_render(self) -> RenderStatus:
        out = _native.MediusRenderStatus()
        check(_native.lib.medius_device_query_render(self._handle, ctypes.byref(out)))
        return render_status_from_c(out)

    def counters(self) -> Counters:
        out = _native.MediusCountersSnapshot()
        check(_native.lib.medius_device_counters(self._handle, ctypes.byref(out)))
        return counters_from_c(out)

    def clip(self) -> ClipHandle:
        """A handle to this box's buffered-clip playback (§3.11)."""
        out = ctypes.c_void_p()
        check(_native.lib.medius_device_clip(self._handle, ctypes.byref(out)))
        return ClipHandle(out.value, self)

    def catch_events(self, filters: Union[CatchFilter, Sequence[CatchFilter]]) -> EventStream:
        """Subscribe to the catch stream for one filter or a sequence of them.

        Overlapping subscriptions from different callers collapse into the one table the box holds,
        and each consumer still receives everything it asked for.
        """
        seq = [filters] if isinstance(filters, CatchFilter) else list(filters)
        if not seq:
            raise InvalidArgError(Status.ERR_INVALID_ARG, "catch_events needs at least one filter")
        arr = (_native.MediusCatchFilter * len(seq))(*[f._c for f in seq])
        out = ctypes.c_void_p()
        check(
            _native.lib.medius_device_catch_events(
                self._handle, arr, len(seq), ctypes.byref(out)
            )
        )
        return EventStream(out.value, self)

    def input_events(self, filters: Union[CatchFilter, Sequence[CatchFilter]]) -> InputStream:
        """Subscribe to decoded input: press and release edges, and motion.

        Every filter must name an input class and cover both edges; build them with
        `CatchFilter.watch*` or `CatchFilter.all_input()`. A traffic class, `everything()`, or a
        filter narrowed to one edge is refused rather than silently yielding nothing.
        """
        seq = [filters] if isinstance(filters, CatchFilter) else list(filters)
        if not seq:
            raise InvalidArgError(Status.ERR_INVALID_ARG, "input_events needs at least one filter")
        arr = (_native.MediusCatchFilter * len(seq))(*[f._c for f in seq])
        out = ctypes.c_void_p()
        check(
            _native.lib.medius_device_input_events(
                self._handle, arr, len(seq), ctypes.byref(out)
            )
        )
        return InputStream(out.value, self)

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
