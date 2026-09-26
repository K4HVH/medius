"""The Device wrapper: commands, queries, and stream factories."""

from __future__ import annotations

import ctypes
import time
from typing import Optional, Sequence, Union

from . import _native
from ._enums import (Action, Axis, BearingMode, Blanket, EmitMode, RenderMode, LedMode, LedTarget,
                     Direction, MoveTiming, PendingMotion, RebootTarget, Status, UpdateTarget)
from ._errors import InvalidArgError, MediusError, check
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
        """Open the box whose identity matches `box_id` (device MAC hex or CH343 serial).

        Raises `BadProtoVerError` when that box speaks another control protocol."""
        out = ctypes.c_void_p()
        check(_native.lib.medius_device_open_by_id(box_id.encode("utf-8"), ctypes.byref(out)))
        return cls(out.value)

    @classmethod
    def find_mouse_box(cls) -> "Device":
        """Open the first box whose clone is a mouse.

        Raises `BadProtoVerError` when no other box clones a mouse and a box on another control
        protocol (clone unread) is connected."""
        out = ctypes.c_void_p()
        check(_native.lib.medius_device_find_mouse_box(ctypes.byref(out)))
        return cls(out.value)

    @classmethod
    def find_keyboard_box(cls) -> "Device":
        """Open the first box whose clone is a keyboard.

        Raises `BadProtoVerError` when no other box clones a keyboard and a box on another control
        protocol (clone unread) is connected."""
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
        """A cursor move that bypasses movement riding, sent on the box's next mouse report."""
        check(_native.lib.medius_device_move_rel_now(self._handle, _i16(dx, "dx"), _i16(dy, "dy")))

    def wheel_now(self, delta):
        """A wheel move that bypasses movement riding."""
        check(_native.lib.medius_device_wheel_now(self._handle, _i16(delta, "delta")))

    def pan(self, delta):
        """An AC Pan (horizontal-scroll) move; full `i16`, no clamp."""
        check(_native.lib.medius_device_pan(self._handle, _i16(delta, "delta")))

    def pan_now(self, delta):
        """An AC Pan move that bypasses movement riding."""
        check(_native.lib.medius_device_pan_now(self._handle, _i16(delta, "delta")))

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

        `scale` is the percent of the physical value kept: 0 blocks, 100 passes, above that
        amplifies up to 255 (2.55x). `lock` and `unlock` are its two ends.

        A negative percent, down to `LOCK_SCALE_MIN`, weighs and reverses: -100 inverts. The slot
        comes from the delta's sign before the weigh, so -100 on `Direction.POSITIVE` turns
        rightward input leftward and leaves leftward input alone. Axes only: on a momentary usage,
        which carries one bit, it raises `LockScaleUsageError`. A magnitude out of range raises
        `LockScaleRangeError`.

        A delta picks up at most two scales, its absolute direction's and its relative direction's,
        and they multiply, so a block anywhere zeroes the product. `Direction.BOTH` writes the scale
        to the two fixed signs and a full pass to the relative pair, so a `BOTH` of 50 is 50% with
        or without a bearing. Name a relative direction to weigh it.

        `Direction.WITH` and `Direction.AGAINST` need a live bearing (`set_bearing`), which only an
        axis has; on a button, key or media usage either raises `RelativeDirectionError`. Any scale
        below a full pass locks a momentary usage; at or above one unlocks it. A media usage has no
        edges: it is sent, and `query_locks` reports it, as `Direction.BOTH` whatever edge is named.
        """
        direction = _enum(direction, Direction, "direction")
        check(
            _native.lib.medius_device_scale(
                self._handle, target._c, int(direction), _i16(scale, "scale")
            )
        )

    def scale_all(self, what: Blanket, direction: Direction, scale: int):
        """Weigh a whole class blanket; see `scale` for what the number means."""
        what = _enum(what, Blanket, "what")
        direction = _enum(direction, Direction, "direction")
        check(
            _native.lib.medius_device_scale_all(
                self._handle, int(what), int(direction), _i16(scale, "scale")
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

    def factory_reset(self):
        """RESET with the NVS flag: the ``reset`` release, then the box erases its persistent store
        and reboots to its defaults under its MAC-derived name. Erases the box name, every option,
        and all learned per-device data. While it reboots the control port stays enumerated but
        silent, so queries time out; the link does not drop."""
        check(_native.lib.medius_device_factory_reset(self._handle))

    def reapply(self):
        check(_native.lib.medius_device_reapply(self._handle))

    def reconnect(self):
        """Rescan, reopen this box, and re-apply held state.

        Raises `BadProtoVerError` when the box replies on another control protocol; it stays
        disconnected."""
        check(_native.lib.medius_device_reconnect(self._handle))

    def reboot(self, target: RebootTarget):
        target = _enum(target, RebootTarget, "target")
        check(_native.lib.medius_device_reboot(self._handle, int(target)))

    def allow_imperfect_clones(self, allow: bool):
        """`OPTION(IMPERFECT)`: opt into cloning devices the box cannot clone faithfully, or back to
        faithful-only. A toggle that changes the served patch set re-presents the clone, releasing
        the session like a device replug; the library re-sends its held state once the new clone is
        up, and `ClipHandle.lost` reports a dropped clip."""
        check(_native.lib.medius_device_allow_imperfect_clones(self._handle, bool(allow)))

    def set_movement_riding(self, window_ms: Optional[int]):
        """Set the movement-riding window in ms, or `None` to turn it off."""
        enabled = window_ms is not None
        check(
            _native.lib.medius_device_set_movement_riding(
                self._handle, enabled, _window_ms(window_ms)
            )
        )

    def set_emit_pace(self, pace: EmitPace, force_hz: Optional[int] = None):
        """Set injected-motion pacing (`hz` matters only for `EmitPace.fixed`) and the rate the
        clone advertises and the box polls the device at (`force_hz`, None = native)."""
        mode = _enum(pace.mode, EmitMode, "mode")
        check(
            _native.lib.medius_device_set_emit_pace(
                self._handle, int(mode), _u16(pace.hz, "hz"), _u16(force_hz or 0, "force_hz"),
            )
        )

    def set_name(self, name: str):
        """Set the box's persistent human-readable name; an empty string clears it."""
        check(_native.lib.medius_device_set_name(self._handle, name.encode("utf-8")))

    def clear_name(self):
        """Clear the custom name, reverting to the synthesised `Medius-XXXX` default."""
        check(_native.lib.medius_device_clear_name(self._handle))

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
            _native.lib.medius_device_set_bearing(
                self._handle, _window_ms(window_ms), int(mode)
            )
        )

    def set_render(self, mode: RenderMode, full: bool):
        """Set the render texture, and whether the model renders native motion instead of relaying
        it.

        Both share one command and persist, so `full` is required: omitting it would rewrite a
        setting you did not name. Rendering adds a small latency, which reaches native motion when
        `full` is on. `full` is off by default. Nothing renders until the box has learned the
        attached device's profile (`RenderStatus.ready`).

        Motion asking for exact timing skips the model: `move_rel_now`, `flush_motion` and
        `discard_motion` take the paced path, and with `full` on the rendered stream ignores
        `set_movement_riding`."""
        mode = _enum(mode, RenderMode, "mode")
        check(_native.lib.medius_device_set_render(self._handle, int(mode), bool(full)))

    def set_spread(self, percent: int):
        """Set the percent of the host's command interval an injected delta is released across: 0
        puts it all on the box's next report, 100 spreads it across one command interval, above 100
        overlaps. A loop at the native report rate keeps each command whole on its own report. Until
        the box learns the host's command period from MOVE arrivals (`SpreadStatus.span_us`), each
        delta goes out whole."""
        check(_native.lib.medius_device_set_spread(self._handle, int(percent)))

    def query_version(self) -> Version:
        out = _native.MediusVersion()
        check(_native.lib.medius_device_query_version(self._handle, ctypes.byref(out)))
        return version_from_c(out)

    def firmware_info(self) -> FirmwareInfo:
        """Both chips' firmware versions and booted app slots."""
        out = _native.MediusFirmwareInfo()
        check(_native.lib.medius_device_firmware_info(self._handle, ctypes.byref(out)))
        return firmware_info_from_c(out)

    def wait_firmware_confirmed(self, timeout: float = 55.0) -> FirmwareInfo:
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

    def query_bearing(self) -> Bearing:
        """The configured bearing: its window and how it is read."""
        out = _native.MediusBearing()
        check(_native.lib.medius_device_query_bearing(self._handle, ctypes.byref(out)))
        return bearing_from_c(out)

    def query_render(self) -> RenderStatus:
        out = _native.MediusRenderStatus()
        check(_native.lib.medius_device_query_render(self._handle, ctypes.byref(out)))
        return render_status_from_c(out)

    def query_spread(self) -> SpreadStatus:
        """Injection spread percent and the span the box releases across."""
        out = _native.MediusSpreadStatus()
        check(_native.lib.medius_device_query_spread(self._handle, ctypes.byref(out)))
        return spread_status_from_c(out)

    def counters(self) -> Counters:
        out = _native.MediusCountersSnapshot()
        check(_native.lib.medius_device_counters(self._handle, ctypes.byref(out)))
        return counters_from_c(out)

    # Advanced control layer (§3.14): raw injection, control transfers, rewrite rules, descriptor
    # patches. Gated on `allow_imperfect_clones`.

    def raw(self, ep: int, direction: Direction, data: bytes) -> None:
        """`RAW` (§3.14): put `data` verbatim on cloned endpoint `ep` in `direction`,
        fire-and-forget.

        `ep` is the bare endpoint number (0 to 15). `Direction.IN` emits to the game PC;
        `Direction.OUT` relays to the real device. `Direction.BOTH` raises `RawDirectionError`, the
        bearing-relative pair `RelativeDirectionError`. Gated on the imperfect-clone opt-in: with it
        off the box drops the frame with no reply, and this still returns. `query_imperfect()`
        reports the state.
        """
        direction = _enum(direction, Direction, "direction")
        buf, n = _bytes_buf(data, "data")
        check(_native.lib.medius_device_raw(self._handle, _u8(ep, "ep"), int(direction), buf, n))

    def transfer(
        self, ep: int, setup: Setup, out: bytes = b"", timeout_ms: Optional[int] = None
    ) -> TransferOutcome:
        """`TRANSFER` (§3.14): run one control transfer against the real device and return its
        reply.

        `ep` is 0 for EP0 or a control endpoint the device declares; `out` is the OUT data stage
        (empty for IN). A non-OK `TransferOutcome.status` is a protocol outcome, returned, not
        raised; the box replies `REFUSED` while the opt-in is off. `timeout_ms` is the reply wait,
        default `default_transfer_timeout_ms()`; the box abandons a transfer after its own ~800 ms,
        so a shorter wait gives up before a slow device replies.
        """
        buf, n = _bytes_buf(out, "out")
        outcome = _native.MediusTransferOutcome()
        if timeout_ms is None:
            check(
                _native.lib.medius_device_transfer(
                    self._handle, _u8(ep, "ep"), setup_to_c(setup), buf, n, ctypes.byref(outcome)
                )
            )
        else:
            check(
                _native.lib.medius_device_transfer_timeout(
                    self._handle,
                    _u8(ep, "ep"),
                    setup_to_c(setup),
                    buf,
                    n,
                    _u32(timeout_ms, "timeout_ms"),
                    ctypes.byref(outcome),
                )
            )
        return transfer_outcome_from_c(outcome)

    def set_rewrite(self, rule: RewriteRule) -> None:
        """`REWRITE` (§3.14): add or overwrite one rewrite rule; needs the opt-in.

        `match_bytes` and `mask` must be equal in length (`RewriteMaskLengthError`) and at most
        16 bytes (`RewriteMatchTooLongError`), the action must suit the class
        (`RewriteActionClassError`), the direction must not be bearing-relative
        (`RelativeDirectionError`), and the payload must fit the box's head
        (`RewritePayloadTooLargeError`). `query_rewrite` confirms what the box holds.
        """
        c = rewrite_rule_to_c(rule)
        check(_native.lib.medius_device_set_rewrite(self._handle, ctypes.byref(c)))

    def remove_rewrite(self, rule: RewriteRule) -> None:
        """`REWRITE` remove (§3.14): drop the rule keyed by `rule`'s
        ``(rewrite_class, id, direction, match_bytes, mask)``, ignoring its action and payload."""
        c = rewrite_rule_to_c(rule)
        check(_native.lib.medius_device_remove_rewrite(self._handle, ctypes.byref(c)))

    def clear_rewrite(self) -> None:
        """`REWRITE` clear (§3.14): drop the whole rewrite table. Always clears the held rules."""
        check(_native.lib.medius_device_clear_rewrite(self._handle))

    def query_rewrite(self) -> RewriteTable:
        """`QUERY(REWRITE)` (§4.17): the table summary, a row per rule without its bytes."""
        out = _native.MediusRewriteTable()
        check(_native.lib.medius_device_query_rewrite(self._handle, ctypes.byref(out)))
        return rewrite_table_from_c(out)

    def query_rewrite_entry(self, index: int) -> RewriteRule:
        """`QUERY(REWRITE_ENTRY, index)` (§4.17): one rule in full, in the shape `set_rewrite` takes."""
        out = _native.MediusRewriteRule()
        check(
            _native.lib.medius_device_query_rewrite_entry(
                self._handle, _u8(index, "index"), ctypes.byref(out)
            )
        )
        return rewrite_rule_from_c(out)

    def set_patch(self, patch: Patch) -> None:
        """`PATCH` (§3.14): store one descriptor patch; empty `bytes` removes the patch at its key.
        The box stores it whatever the opt-in; the set reaches the game PC when the clone is next
        presented under the opt-in: `apply_patch`, the opt-in turning on, or the device
        attaching."""
        c = patch_to_c(patch)
        check(_native.lib.medius_device_set_patch(self._handle, ctypes.byref(c)))

    def apply_patch(self) -> None:
        """`PATCH` APPLY (§3.14): re-present the clone with the stored patch set; needs the opt-in.
        The box re-presents only while the stored set differs from the served one: applying an
        emptied set serves the device unpatched, and a refused set unchanged since is left alone.
        Re-presenting releases the session like a device replug; the library re-sends its held state
        once the new clone is up, and `ClipHandle.lost` reports a dropped clip."""
        check(_native.lib.medius_device_apply_patch(self._handle))

    def clear_patch(self) -> None:
        """`PATCH` CLEAR (§3.14): erase this device's stored set (the last attached one's when
        unplugged). A clone serving patches re-presents unpatched, releasing the session and
        re-sending held state as `apply_patch` does."""
        check(_native.lib.medius_device_clear_patch(self._handle))

    def query_patches(self) -> PatchSet:
        """`QUERY(PATCHES)` (§4.17): the stored patch set and its apply state, a row per patch."""
        out = _native.MediusPatchSet()
        check(_native.lib.medius_device_query_patches(self._handle, ctypes.byref(out)))
        return patch_set_from_c(out)

    def query_patch_entry(self, index: int) -> Patch:
        """`QUERY(PATCH_ENTRY, index)` (§4.17): one patch in full, in the shape `set_patch` takes."""
        out = _native.MediusPatch()
        check(
            _native.lib.medius_device_query_patch_entry(
                self._handle, _u8(index, "index"), ctypes.byref(out)
            )
        )
        return patch_from_c(out)

    # Field transforms (§3.15): faithful field operations on the semantic path, with no
    # imperfect-clone opt-in.

    def transform(self, t: Transform) -> None:
        """`TRANSFORM` (§3.15): add or overwrite one field transform.

        It swaps or remaps a field the clone already declares, so it is faithful and needs no
        `allow_imperfect_clones`. How much of a field is kept is `scale`'s, which runs first.
        Entries are keyed by `(source, dest)` and apply in installation order. A combination the op
        cannot address raises `TransformOpFieldsError`; an entry past the table's capacity raises
        `TransformTableFullError`. `query_transforms` confirms what the box holds.
        """
        c = transform_to_c(t)
        check(_native.lib.medius_device_transform(self._handle, ctypes.byref(c)))

    def untransform(self, t: Transform) -> None:
        """`TRANSFORM` remove (§3.15): drop the transform keyed by `t`'s `(source, dest)`, ignoring
        its op; a no-op on the box if no such entry is held."""
        c = transform_to_c(t)
        check(_native.lib.medius_device_untransform(self._handle, ctypes.byref(c)))

    def clear_transforms(self) -> None:
        """`TRANSFORM` clear (§3.15): drop the whole transform table."""
        check(_native.lib.medius_device_clear_transforms(self._handle))

    def transform_swap(self, a: Axis, b: Axis) -> None:
        """Exchange two axes on the wire: a `transform` of `Transform.swap`."""
        check(
            _native.lib.medius_device_transform_swap(
                self._handle, int(_enum(a, Axis, "a")), int(_enum(b, Axis, "b"))
            )
        )

    def transform_remap(self, source, dest) -> None:
        """Move a source field into a destination: a `transform` of `Transform.remap`. `source` and
        `dest` are a `LockTarget`, an `Axis`, or a usage (`Usage`/`Button`/`Key`/`MediaKey`)."""
        s = _as_lock_target(source)
        d = _as_lock_target(dest)
        check(_native.lib.medius_device_transform_remap(self._handle, s._c, d._c))

    def query_transforms(self) -> Transforms:
        """`QUERY(TRANSFORMS)` (§4.18): the transform table, a row per entry in the shape
        `transform` takes, so a read entry replays as a set."""
        out = _native.MediusTransforms()
        check(_native.lib.medius_device_query_transforms(self._handle, ctypes.byref(out)))
        return transforms_from_c(out)

    def clip(self) -> ClipHandle:
        """A handle to this box's buffered-clip playback (§3.11)."""
        out = ctypes.c_void_p()
        check(_native.lib.medius_device_clip(self._handle, ctypes.byref(out)))
        return ClipHandle(out.value, self)

    def catch_events(self, filters: Union[CatchFilter, Sequence[CatchFilter]]) -> EventStream:
        """Subscribe to the catch stream for one filter or a sequence.

        Overlapping subscriptions from different callers merge into the box's one table; each
        consumer still receives everything it asked for.
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
        one-edge filter is refused.
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
