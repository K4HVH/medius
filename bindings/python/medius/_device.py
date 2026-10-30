"""The Device wrapper: commands, queries, and stream factories."""

from __future__ import annotations

import ctypes
from typing import Optional, Sequence, Union

from . import _native
from ._enums import (Action, Axis, BearingMode, Blanket, EmitMode, RenderMode, LedMode, LedTarget,
                     Direction, MoveTiming, PendingMotion, RebootTarget, Status, UpdateTarget)
from ._errors import MediusError, check
from ._handle import Handle
from ._clip import ClipHandle
from ._streams import EventStream, InputStream, LogStream
from ._types import (
    Bearing,
    FirmwareInfo,
    firmware_info_from_c,
    _as_lock_target,
    _bytes_buf,
    _enum,
    _i16,
    _u8,
    _u16,
    _u32,
    _window_ms,
    Caps,
    CatchFilter,
    CatchState,
    Counters,
    EmitPace,
    EmitPaceStatus,
    RenderStatus,
    SpreadStatus,
    Health,
    ImperfectStatus,
    Patch,
    PatchSet,
    RewriteRule,
    RewriteTable,
    Setup,
    TransferOutcome,
    Transform,
    Transforms,
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
    spread_status_from_c,
    health_from_c,
    imperfect_from_c,
    locks_from_c,
    patch_from_c,
    patch_set_from_c,
    patch_to_c,
    rate_from_c,
    rewrite_rule_from_c,
    rewrite_rule_to_c,
    rewrite_table_from_c,
    setup_to_c,
    stats_from_c,
    transfer_outcome_from_c,
    transform_to_c,
    transforms_from_c,
    version_from_c,
)


def _require_mock():
    if not _native.HAS_MOCK:
        raise RuntimeError(
            "the loaded medius_capi library was built without the mock feature "
            "(rebuild with --features mock)"
        )


class Device(Handle):
    """An open connection to one medius box."""

    _free = "medius_device_free"

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
        """Open the box whose identity matches `box_id` (device-chip MAC hex or CH343 serial).

        Raises `BadProtoVerError` if that box speaks another control protocol."""
        out = ctypes.c_void_p()
        check(_native.lib.medius_device_open_by_id(box_id.encode("utf-8"), ctypes.byref(out)))
        return cls(out.value)

    @classmethod
    def find_mouse_box(cls) -> "Device":
        """Open the first box whose clone is a mouse.

        With no such box, a box on another control protocol (which reports no clone) raises
        `BadProtoVerError`."""
        out = ctypes.c_void_p()
        check(_native.lib.medius_device_find_mouse_box(ctypes.byref(out)))
        return cls(out.value)

    @classmethod
    def find_keyboard_box(cls) -> "Device":
        """Open the first box whose clone is a keyboard.

        With no such box, a box on another control protocol (which reports no clone) raises
        `BadProtoVerError`."""
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
        handle = self._call(_native.lib.medius_device_clone)
        if not handle:
            raise MediusError(Status.ERR_UNKNOWN, "device clone failed")
        return Device(handle)

    def move_rel(self, dx, dy):
        check(self._call(_native.lib.medius_device_move_rel, _i16(dx, "dx"), _i16(dy, "dy")))

    def wheel(self, delta):
        check(self._call(_native.lib.medius_device_wheel, _i16(delta, "delta")))

    def move_rel_now(self, dx, dy):
        """A cursor move that bypasses movement riding, emitted on the next mouse report."""
        check(self._call(_native.lib.medius_device_move_rel_now, _i16(dx, "dx"), _i16(dy, "dy")))

    def wheel_now(self, delta):
        """A wheel move that bypasses movement riding."""
        check(self._call(_native.lib.medius_device_wheel_now, _i16(delta, "delta")))

    def pan(self, delta):
        """An AC Pan (horizontal-scroll) move; full `i16`, no clamp."""
        check(self._call(_native.lib.medius_device_pan, _i16(delta, "delta")))

    def pan_now(self, delta):
        """An AC Pan move that bypasses movement riding."""
        check(self._call(_native.lib.medius_device_pan_now, _i16(delta, "delta")))

    def flush_motion(self):
        """Emit the motion held for a ride now, ignoring the ride window."""
        check(self._call(_native.lib.medius_device_flush_motion))

    def discard_motion(self):
        """Drop the motion held for a ride."""
        check(self._call(_native.lib.medius_device_discard_motion))

    def move_axis(self, motion: Motion, timing: MoveTiming = MoveTiming.RIDE,
                  pending: PendingMotion = PendingMotion.KEEP):
        timing = _enum(timing, MoveTiming, "timing")
        pending = _enum(pending, PendingMotion, "pending")
        check(self._call(_native.lib.medius_device_move_axis, motion._c, int(timing), int(pending)))

    def inject(self, input: Usage, action: Action):
        action = _enum(action, Action, "action")
        check(self._call(_native.lib.medius_device_inject, input._c, int(action)))

    def press(self, input: Usage):
        check(self._call(_native.lib.medius_device_press, input._c))

    def soft_release(self, input: Usage):
        check(self._call(_native.lib.medius_device_soft_release, input._c))

    def force_release(self, input: Usage):
        check(self._call(_native.lib.medius_device_force_release, input._c))

    def lock(self, target: LockTarget, direction: Direction):
        direction = _enum(direction, Direction, "direction")
        check(self._call(_native.lib.medius_device_lock, target._c, int(direction)))

    def unlock(self, target: LockTarget, direction: Direction):
        direction = _enum(direction, Direction, "direction")
        check(self._call(_native.lib.medius_device_unlock, target._c, int(direction)))

    def scale(self, target: LockTarget, direction: Direction, scale: int):
        """Weigh native input on a target and direction.

        `scale` is the percent of the native value kept: 0 blocks, 100 passes, and values above
        that amplify, up to 255 (2.55x). `lock` is a scale of 0, and `unlock` a scale of 100.

        A negative scale, down to `LOCK_SCALE_MIN`, weighs the native value and reverses it (-100
        inverts). The delta's sign picks the slot before the weigh, so -100 on
        `Direction.POSITIVE` turns rightward motion leftward and leaves leftward motion alone. A
        negative scale is for axes only: on a momentary usage, which carries one bit, it raises
        `LockScaleUsageError`. A scale outside the range raises `LockScaleRangeError`.

        A delta takes at most two scales, one for its absolute direction and one for its relative
        direction, and they multiply. A block in either zeroes the product. `Direction.BOTH` writes
        the scale to the two fixed signs and a full pass to the relative pair. Name a relative
        direction to weigh it.

        `Direction.WITH` and `Direction.AGAINST` do nothing until a bearing is live (`set_bearing`).
        Only an axis has a bearing, so on a button, key or media usage either raises
        `RelativeDirectionError`. A scale below full pass locks a momentary usage, and full pass or
        more unlocks it. A media usage has no edges, so an edge direction on one is sent as
        `Direction.BOTH`, and `query_locks` reports it as `BOTH`.
        """
        direction = _enum(direction, Direction, "direction")
        check(
            self._call(
                _native.lib.medius_device_scale, target._c, int(direction), _i16(scale, "scale")
            )
        )

    def scale_all(self, what: Blanket, direction: Direction, scale: int):
        """Weigh a whole class blanket; see `scale` for what the number means."""
        what = _enum(what, Blanket, "what")
        direction = _enum(direction, Direction, "direction")
        check(
            self._call(
                _native.lib.medius_device_scale_all, int(what), int(direction), _i16(scale, "scale")
            )
        )

    def lock_all(self, what: Blanket, direction: Direction):
        """Block a whole class blanket.

        `Blanket.KEYS` honours the direction: `POSITIVE` blocks press edges only, `NEGATIVE` release
        edges only.
        """
        what = _enum(what, Blanket, "what")
        direction = _enum(direction, Direction, "direction")
        check(self._call(_native.lib.medius_device_lock_all, int(what), int(direction)))

    def unlock_all(self, what: Blanket, direction: Direction):
        what = _enum(what, Blanket, "what")
        direction = _enum(direction, Direction, "direction")
        check(self._call(_native.lib.medius_device_unlock_all, int(what), int(direction)))

    def led(self, target: LedTarget, mode: LedMode, level):
        target = _enum(target, LedTarget, "target")
        mode = _enum(mode, LedMode, "mode")
        check(
            self._call(
                _native.lib.medius_device_led, int(target), int(mode), _u8(level, "level")
            )
        )

    def reset(self):
        check(self._call(_native.lib.medius_device_reset))

    def factory_reset(self):
        """RESET with the NVS flag: the ``reset`` release, then the box erases its persistent store
        and reboots with defaults and its MAC-derived name. Erases the box name, every option and
        all learned per-device data. The control port stays enumerated during the reboot, so
        queries time out with the link up: poll one until it replies."""
        check(self._call(_native.lib.medius_device_factory_reset))

    def reapply(self):
        check(self._call(_native.lib.medius_device_reapply))

    def reconnect(self):
        """Rescan, reopen this box, and re-apply held state.

        If the box replies on another control protocol, raises `BadProtoVerError` and stays
        disconnected."""
        check(self._call(_native.lib.medius_device_reconnect))

    def reboot(self, target: RebootTarget):
        target = _enum(target, RebootTarget, "target")
        check(self._call(_native.lib.medius_device_reboot, int(target)))

    def allow_imperfect_clones(self, allow: bool):
        """`OPTION(IMPERFECT)`: opt into cloning devices the box cannot clone faithfully, or back to
        faithful-only. If a toggle changes the served patch set, the clone is presented again. That
        releases the session like a device replug: once the new clone is up the library re-sends
        its held state, and `ClipHandle.lost` reports a dropped clip."""
        check(self._call(_native.lib.medius_device_allow_imperfect_clones, bool(allow)))

    def set_movement_riding(self, window_ms: Optional[int]):
        """Set the movement-riding window in ms, or `None` to turn it off."""
        enabled = window_ms is not None
        check(
            self._call(
                _native.lib.medius_device_set_movement_riding, enabled, _window_ms(window_ms)
            )
        )

    def set_emit_pace(self, pace: EmitPace, force_hz: Optional[int] = None):
        """Set injected-motion pacing (`hz` matters only for `EmitPace.fixed`) and the forced poll
        rate (`force_hz`; None = native). The clone advertises the forced rate, and the box polls
        the real device at it."""
        mode = _enum(pace.mode, EmitMode, "mode")
        check(
            self._call(
                _native.lib.medius_device_set_emit_pace,
                int(mode),
                _u16(pace.hz, "hz"),
                _u16(force_hz or 0, "force_hz"),
            )
        )

    def set_name(self, name: str):
        """Set the box's persistent human-readable name; an empty string clears it."""
        check(self._call(_native.lib.medius_device_set_name, name.encode("utf-8")))

    def clear_name(self):
        """Clear the custom name, reverting to the synthesised `Medius-XXXX` default."""
        check(self._call(_native.lib.medius_device_clear_name))

    def set_bearing(self, window_ms: Optional[int], mode: BearingMode):
        """Set what `Direction.WITH` and `Direction.AGAINST` are measured against.

        `window_ms` is how long the last injected delta's direction stays the bearing; `None` turns
        it off, leaving the relative directions inert whatever their scale. It saturates at 65535 ms,
        as the Rust API does.

        Both fields share one frame and persist together, so `mode` is required: a default would
        revert a `VECTOR` box on any window change.
        """
        mode = _enum(mode, BearingMode, "mode")
        check(
            self._call(
                _native.lib.medius_device_set_bearing, _window_ms(window_ms), int(mode)
            )
        )

    def set_render(self, mode: RenderMode, full: bool):
        """Set the render texture, and whether the model renders native motion instead of relaying
        it.

        Both share one command and persist, so `full` is required: omitting it would rewrite a
        setting the caller did not name. Rendering adds latency; `full` adds it to native motion
        too. `full` is off by default. Nothing renders until the box has learned the real device's
        profile (`RenderStatus.ready`).

        Motion asking for exact timing skips the model: `move_rel_now`, `flush_motion` and
        `discard_motion` take the paced path. With `full` on, the rendered stream ignores
        `set_movement_riding`."""
        mode = _enum(mode, RenderMode, "mode")
        check(self._call(_native.lib.medius_device_set_render, int(mode), bool(full)))

    def set_spread(self, percent: int):
        """Set the percent of the host's command interval an injected delta is released across. 0
        puts the whole delta on the next emitted report, 100 spreads it across one command interval,
        and >100 overlaps. A command loop at the native report rate keeps each command whole, on one
        report. Until the box learns the host's command period from MOVE arrivals
        (`SpreadStatus.span_us`), each delta is emitted whole."""
        check(self._call(_native.lib.medius_device_set_spread, _u16(percent, "percent")))

    def query_version(self) -> Version:
        out = _native.MediusVersion()
        check(self._call(_native.lib.medius_device_query_version, ctypes.byref(out)))
        return version_from_c(out)

    def firmware_info(self) -> FirmwareInfo:
        """Both chips' firmware versions and booted app slots."""
        out = _native.MediusFirmwareInfo()
        check(self._call(_native.lib.medius_device_firmware_info, ctypes.byref(out)))
        return firmware_info_from_c(out)

    def wait_firmware_confirmed(self) -> FirmwareInfo:
        """Block until neither chip is on probation (a chip on probation refuses an update).

        Keeps asking while the box reboots after an activate. After 55 s, raises `UpdateError` if a
        chip is still on probation, or `QueryTimeoutError` if the box hasn't replied.
        """
        out = _native.MediusFirmwareInfo()
        check(self._call(_native.lib.medius_device_wait_firmware_confirmed, ctypes.byref(out)))
        return firmware_info_from_c(out)

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
            self._call(
                _native.lib.medius_device_stage_firmware, int(target), buf, len(image), cb, None
            )
        )

    def abort_update(self, target: UpdateTarget) -> None:
        """Drop whatever is staged or in flight for one target."""
        check(self._call(_native.lib.medius_device_abort_update, int(target)))

    def activate_firmware(self) -> None:
        """Commit every staged image and reboot into it; the host chip goes first."""
        check(self._call(_native.lib.medius_device_activate_firmware))

    def update_firmware(self, target: UpdateTarget, image: bytes, progress=None) -> None:
        """Stage one image and activate it; a refused activate aborts the staged image."""
        if not image:
            raise ValueError("image is empty")
        buf = (ctypes.c_uint8 * len(image)).from_buffer_copy(image)

        def _cb(_user, sent, total):
            if progress is not None:
                progress(int(sent), int(total))

        cb = _native.UPDATE_PROGRESS_CB(_cb)
        check(
            self._call(
                _native.lib.medius_device_update_firmware, int(target), buf, len(image), cb, None
            )
        )

    def query_health(self) -> Health:
        out = _native.MediusHealth()
        check(self._call(_native.lib.medius_device_query_health, ctypes.byref(out)))
        return health_from_c(out)

    def device_info(self) -> DeviceInfo:
        out = _native.MediusDeviceInfo()
        check(self._call(_native.lib.medius_device_device_info, ctypes.byref(out)))
        return device_info_from_c(out)

    def caps(self) -> Caps:
        out = _native.MediusCaps()
        check(self._call(_native.lib.medius_device_caps, ctypes.byref(out)))
        return caps_from_c(out)

    def query_rate(self) -> Rate:
        out = _native.MediusRate()
        check(self._call(_native.lib.medius_device_query_rate, ctypes.byref(out)))
        return rate_from_c(out)

    def query_stats(self) -> Stats:
        out = _native.MediusStats()
        check(self._call(_native.lib.medius_device_query_stats, ctypes.byref(out)))
        return stats_from_c(out)

    def query_locks(self) -> Locks:
        out = _native.MediusLocks()
        check(self._call(_native.lib.medius_device_query_locks, ctypes.byref(out)))
        return locks_from_c(out)

    def query_catch(self) -> CatchState:
        out = _native.MediusCatchState()
        check(self._call(_native.lib.medius_device_query_catch, ctypes.byref(out)))
        return catch_state_from_c(out)

    def query_imperfect(self) -> ImperfectStatus:
        out = _native.MediusImperfectStatus()
        check(self._call(_native.lib.medius_device_query_imperfect, ctypes.byref(out)))
        return imperfect_from_c(out)

    def query_movement_riding(self) -> Optional[int]:
        """The movement-riding window in whole ms, or `None` when off."""
        enabled = _native.c_bool()
        window = _native.u32()
        check(
            self._call(
                _native.lib.medius_device_query_movement_riding, ctypes.byref(enabled), ctypes.byref(window)
            )
        )
        return int(window.value) if enabled.value else None

    def query_emit_pace(self) -> EmitPaceStatus:
        out = _native.MediusEmitPaceStatus()
        check(self._call(_native.lib.medius_device_query_emit_pace, ctypes.byref(out)))
        return emit_pace_status_from_c(out)

    def query_bearing(self) -> Bearing:
        """The configured bearing: its window and how it is read."""
        out = _native.MediusBearing()
        check(self._call(_native.lib.medius_device_query_bearing, ctypes.byref(out)))
        return bearing_from_c(out)

    def query_render(self) -> RenderStatus:
        out = _native.MediusRenderStatus()
        check(self._call(_native.lib.medius_device_query_render, ctypes.byref(out)))
        return render_status_from_c(out)

    def query_spread(self) -> SpreadStatus:
        """Spread percent and release interval."""
        out = _native.MediusSpreadStatus()
        check(self._call(_native.lib.medius_device_query_spread, ctypes.byref(out)))
        return spread_status_from_c(out)

    def counters(self) -> Counters:
        out = _native.MediusCountersSnapshot()
        check(self._call(_native.lib.medius_device_counters, ctypes.byref(out)))
        return counters_from_c(out)

    # Advanced control layer (§3.14): raw injection, control transfers, rewrite rules, descriptor
    # patches. Gated on `allow_imperfect_clones`.

    def raw(self, ep: int, direction: Direction, data: bytes) -> None:
        """`RAW` (§3.14): put `data` verbatim on cloned endpoint `ep` in `direction`,
        fire-and-forget.

        `ep` is the bare endpoint number (0-15). `Direction.IN` emits to the game PC;
        `Direction.OUT` relays to the real device. `Direction.BOTH` raises `RawDirectionError`, and
        the bearing-relative pair raises `RelativeDirectionError`. Needs the imperfect-clone
        opt-in: with it off the box drops the frame with no reply, and this still returns.
        `query_imperfect()` reports the state.
        """
        direction = _enum(direction, Direction, "direction")
        buf, n = _bytes_buf(data, "data")
        check(self._call(_native.lib.medius_device_raw, _u8(ep, "ep"), int(direction), buf, n))

    def transfer(
        self, ep: int, setup: Setup, out: bytes = b"", timeout_ms: Optional[int] = None
    ) -> TransferOutcome:
        """`TRANSFER` (§3.14): run one control transfer against the real device and return what came
        back.

        `ep` is 0 for EP0, or a control endpoint the real device declares. `out` is the OUT data
        stage: `setup.length` bytes for an OUT request, none for an IN one (`TransferDataError`),
        at most `TRANSFER_OUT_MAX` (`FrameTooLongError`). A non-OK `TransferOutcome.status` is a
        protocol outcome: it is returned, not raised. With the opt-in off the box replies
        `REFUSED`. `timeout_ms` (1-65535, else `TransferTimeoutRangeError`; a negative value or one
        past 32 bits raises `ValueError`) is how long the box waits on the real device, the time the
        game PC holds the pipe included; the default is `default_transfer_timeout_ms()`. The call
        waits up to `timeout_ms` plus 2 s for the reply (`QueryTimeoutError` past that), and a reply
        that lost a chunk between the box's chips or on the control link raises
        `TransferReplyIncompleteError`.
        """
        buf, n = _bytes_buf(out, "out")
        reply = (_native.u8 * _native.MEDIUS_MAX_TRANSFER_REPLY)()
        outcome = _native.MediusTransferOutcome()
        if timeout_ms is None:
            check(
                self._call(
                    _native.lib.medius_device_transfer,
                    _u8(ep, "ep"),
                    setup_to_c(setup),
                    buf,
                    n,
                    reply,
                    len(reply),
                    ctypes.byref(outcome),
                )
            )
        else:
            check(
                self._call(
                    _native.lib.medius_device_transfer_timeout,
                    _u8(ep, "ep"),
                    setup_to_c(setup),
                    buf,
                    n,
                    _u32(timeout_ms, "timeout_ms"),
                    reply,
                    len(reply),
                    ctypes.byref(outcome),
                )
            )
        return transfer_outcome_from_c(outcome, reply)

    def set_rewrite(self, rule: RewriteRule) -> None:
        """`REWRITE` (§3.14): add or overwrite one rewrite rule; needs the opt-in.

        `match_bytes` and `mask` must be one length (`RewriteMaskLengthError`) and <=16 bytes
        (`RewriteMatchTooLongError`). The action must suit the class (`RewriteActionClassError`),
        the direction must not be bearing-relative (`RelativeDirectionError`), and the payload must
        fit the box's head (`RewritePayloadTooLargeError`). `query_rewrite` confirms what the box
        holds.
        """
        c = rewrite_rule_to_c(rule)
        check(self._call(_native.lib.medius_device_set_rewrite, ctypes.byref(c)))

    def remove_rewrite(self, rule: RewriteRule) -> None:
        """`REWRITE` remove (§3.14): drop the rule keyed by `rule`'s
        ``(rewrite_class, id, direction, match_bytes, mask)``, ignoring its action and payload."""
        c = rewrite_rule_to_c(rule)
        check(self._call(_native.lib.medius_device_remove_rewrite, ctypes.byref(c)))

    def clear_rewrite(self) -> None:
        """`REWRITE` clear (§3.14): drop the whole rewrite table. Always clears the held rules."""
        check(self._call(_native.lib.medius_device_clear_rewrite))

    def query_rewrite(self) -> RewriteTable:
        """`QUERY(REWRITE)` (§4.17): the table summary, a row per rule without its bytes."""
        out = _native.MediusRewriteTable()
        check(self._call(_native.lib.medius_device_query_rewrite, ctypes.byref(out)))
        return rewrite_table_from_c(out)

    def query_rewrite_entry(self, index: int) -> RewriteRule:
        """`QUERY(REWRITE_ENTRY, index)` (§4.17): one rule in full, in the shape `set_rewrite` takes."""
        out = _native.MediusRewriteRule()
        check(
            self._call(
                _native.lib.medius_device_query_rewrite_entry, _u8(index, "index"), ctypes.byref(out)
            )
        )
        return rewrite_rule_from_c(out)

    def set_patch(self, patch: Patch) -> None:
        """`PATCH` (§3.14): store one descriptor patch; empty `bytes` removes the patch at its key.
        The box stores a patch with the opt-in on or off. The clone serves it only with the opt-in
        on, from its next presentation: `apply_patch`, the opt-in turning on, or the real device
        attaching."""
        c = patch_to_c(patch)
        check(self._call(_native.lib.medius_device_set_patch, ctypes.byref(c)))

    def apply_patch(self) -> None:
        """`PATCH` APPLY (§3.14): re-present the clone with the stored patch set; needs the opt-in.
        The box re-presents only if the stored set differs from the served one. Applying an emptied
        set serves the descriptors unpatched; an unchanged refused set stays as is. Re-presenting
        releases the session like a device replug: once the new clone is up the library re-sends
        its held state, and `ClipHandle.lost` reports a dropped clip."""
        check(self._call(_native.lib.medius_device_apply_patch))

    def clear_patch(self) -> None:
        """`PATCH` CLEAR (§3.14): erase the real device's stored set (with no device attached, the
        last attached device's). A clone serving patches re-presents unpatched. That releases the
        session as `apply_patch` does, and the library re-sends held state the same way."""
        check(self._call(_native.lib.medius_device_clear_patch))

    def query_patches(self) -> PatchSet:
        """`QUERY(PATCHES)` (§4.17): the stored patch set and its apply state, a row per patch."""
        out = _native.MediusPatchSet()
        check(self._call(_native.lib.medius_device_query_patches, ctypes.byref(out)))
        return patch_set_from_c(out)

    def query_patch_entry(self, index: int) -> Patch:
        """`QUERY(PATCH_ENTRY, index)` (§4.17): one patch in full, in the shape `set_patch` takes."""
        out = _native.MediusPatch()
        check(
            self._call(
                _native.lib.medius_device_query_patch_entry, _u8(index, "index"), ctypes.byref(out)
            )
        )
        return patch_from_c(out)

    # Field transforms (§3.15): faithful field operations on the semantic path, with no
    # imperfect-clone opt-in.

    def transform(self, t: Transform) -> None:
        """`TRANSFORM` (§3.15): add or overwrite one field transform.

        It swaps or remaps a field the clone already declares, so it is faithful and needs no
        `allow_imperfect_clones`. To weigh or reverse a field, use `scale`; it runs first and passes
        the transform what it kept. Entries are keyed by `(source, dest)` and apply in installation
        order. A combination the op cannot address raises `TransformOpFieldsError`. An entry beyond
        the table's capacity raises `TransformTableFullError`. `query_transforms` confirms what the
        box holds.
        """
        c = transform_to_c(t)
        check(self._call(_native.lib.medius_device_transform, ctypes.byref(c)))

    def untransform(self, t: Transform) -> None:
        """`TRANSFORM` remove (§3.15): drop the transform keyed by `t`'s `(source, dest)`, ignoring
        its op; a no-op on the box if no such entry is held."""
        c = transform_to_c(t)
        check(self._call(_native.lib.medius_device_untransform, ctypes.byref(c)))

    def clear_transforms(self) -> None:
        """`TRANSFORM` clear (§3.15): drop the whole transform table."""
        check(self._call(_native.lib.medius_device_clear_transforms))

    def transform_swap(self, a: Axis, b: Axis) -> None:
        """Exchange two axes: a `transform` of `Transform.swap`."""
        check(
            self._call(
                _native.lib.medius_device_transform_swap, int(_enum(a, Axis, "a")), int(_enum(b, Axis, "b"))
            )
        )

    def transform_remap(self, source, dest) -> None:
        """Move a source field into a destination: a `transform` of `Transform.remap`. `source` and
        `dest` are a `LockTarget`, an `Axis`, or a usage (`Usage`/`Button`/`Key`/`MediaKey`)."""
        s = _as_lock_target(source)
        d = _as_lock_target(dest)
        check(self._call(_native.lib.medius_device_transform_remap, s._c, d._c))

    def query_transforms(self) -> Transforms:
        """`QUERY(TRANSFORMS)` (§4.18): the transform table, a row per entry in the shape
        `transform` takes, so a read entry replays as a set."""
        out = _native.MediusTransforms()
        check(self._call(_native.lib.medius_device_query_transforms, ctypes.byref(out)))
        return transforms_from_c(out)

    def clip(self) -> ClipHandle:
        """A handle to this box's buffered-clip playback (§3.11)."""
        out = ctypes.c_void_p()
        check(self._call(_native.lib.medius_device_clip, ctypes.byref(out)))
        return ClipHandle(out.value, self)

    def catch_events(self, filters: Union[CatchFilter, Sequence[CatchFilter]]) -> EventStream:
        """Subscribe to the catch stream for one filter or a sequence.

        Overlapping subscriptions from different callers collapse into the box's one table; each
        consumer still receives only what it asked for.
        """
        seq = [filters] if isinstance(filters, CatchFilter) else list(filters)
        arr = (_native.MediusCatchFilter * len(seq))(*[f._c for f in seq])
        out = ctypes.c_void_p()
        check(
            self._call(
                _native.lib.medius_device_catch_events, arr, len(seq), ctypes.byref(out)
            )
        )
        return EventStream(out.value, self)

    def input_events(self, filters: Union[CatchFilter, Sequence[CatchFilter]]) -> InputStream:
        """Subscribe to decoded input: press and release edges, and motion.

        Every filter must name an input class and cover both edges; build them with
        `CatchFilter.watch*` or `CatchFilter.all_input()`. A traffic class, `everything()`, or a
        one-edge filter is refused.
        """
        seq = [filters] if isinstance(filters, CatchFilter) else list(filters)
        arr = (_native.MediusCatchFilter * len(seq))(*[f._c for f in seq])
        out = ctypes.c_void_p()
        check(
            self._call(
                _native.lib.medius_device_input_events, arr, len(seq), ctypes.byref(out)
            )
        )
        return InputStream(out.value, self)

    def logs(self) -> LogStream:
        out = ctypes.c_void_p()
        check(self._call(_native.lib.medius_device_logs, ctypes.byref(out)))
        return LogStream(out.value, self)
