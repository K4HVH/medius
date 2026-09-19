"""Buffered clip playback: the clip-entry builder and clip handle (§3.11)."""

from __future__ import annotations

import ctypes
from typing import Iterable, Optional, Sequence, Tuple, Union

from . import _native
from ._enums import Action, Blanket, ClipAction, Direction, Edge
from ._errors import check
from ._types import (
    ClipPacketTrigger,
    ClipSettings,
    ClipStatus,
    ClipTrigger,
    Setup,
    Usage,
    _bytes_buf,
    _enum,
    _i16,
    _u8,
    _u16,
    clip_packet_trigger_to_c,
    clip_settings_from_c,
    clip_status_from_c,
    setup_to_c,
)


class ClipBuilder:
    """Builds a buffered-clip entry stream; pass it to `ClipHandle.append`, then `clear()` to reuse it."""

    def __init__(self):
        self._ptr = _native.lib.medius_clip_builder_new()
        if not self._ptr:
            raise MemoryError("clip builder allocation failed")

    def close(self):
        if getattr(self, "_ptr", None):
            _native.lib.medius_clip_builder_free(self._ptr)
            self._ptr = None

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()

    def __del__(self):
        self.close()

    def clear(self) -> "ClipBuilder":
        """Clear the stream to reuse the builder after an append."""
        check(_native.lib.medius_clip_builder_clear(self._ptr))
        return self

    def byte_len(self) -> int:
        """The bytes the entries take in the ring: what to hold against `ClipStatus.free` before an append."""
        return int(_native.lib.medius_clip_builder_byte_len(self._ptr))

    def gap(self, frames: int) -> "ClipBuilder":
        """Emit nothing for `frames` native frames (a zero count is a no-op)."""
        check(_native.lib.medius_clip_builder_gap(self._ptr, _u16(frames, "frames")))
        return self

    def move(self, dx: int, dy: int) -> "ClipBuilder":
        """A cursor-motion frame."""
        check(_native.lib.medius_clip_builder_move(self._ptr, _i16(dx, "dx"), _i16(dy, "dy")))
        return self

    def wheel(self, dz: int) -> "ClipBuilder":
        """A wheel frame."""
        check(_native.lib.medius_clip_builder_wheel(self._ptr, _i16(dz, "dz")))
        return self

    def pan(self, dpan: int) -> "ClipBuilder":
        """A pan (horizontal scroll) frame."""
        check(_native.lib.medius_clip_builder_pan(self._ptr, _i16(dpan, "dpan")))
        return self

    def press(self, usage: Usage) -> "ClipBuilder":
        """A frame that presses a usage (a button, key, or media usage)."""
        check(_native.lib.medius_clip_builder_press(self._ptr, usage._c))
        return self

    def release(self, usage: Usage) -> "ClipBuilder":
        """A frame that soft-releases a usage (a physical hold is left intact)."""
        check(_native.lib.medius_clip_builder_release(self._ptr, usage._c))
        return self

    def force_release(self, usage: Usage) -> "ClipBuilder":
        """A frame that force-releases a usage (masks a physical hold too)."""
        check(_native.lib.medius_clip_builder_force_release(self._ptr, usage._c))
        return self

    def edge(self, usage: Usage, action: Action = Action.PRESS) -> "ClipBuilder":
        """A one-edge frame for any `Usage` (button, key, or media) with an explicit `Action`."""
        action = _enum(action, Action, "action")
        check(_native.lib.medius_clip_builder_edge(self._ptr, usage._c, int(action)))
        return self

    def raw(self, ep: int, direction: Direction, data: bytes) -> "ClipBuilder":
        """A frame carrying one raw report, as `Device.raw` sends one."""
        direction = _enum(direction, Direction, "direction")
        buf, n = _bytes_buf(data, "data")
        check(_native.lib.medius_clip_builder_raw(self._ptr, _u8(ep, "ep"), int(direction), buf, n))
        return self

    def transfer(self, ep: int, setup: Setup, out: bytes = b"") -> "ClipBuilder":
        """A frame carrying one control transfer, as `Device.transfer` runs one; the answer arrives as a `TrafficClass.CLIP_TRANSFER` event."""
        buf, n = _bytes_buf(out, "out")
        check(
            _native.lib.medius_clip_builder_transfer(self._ptr, _u8(ep, "ep"), setup_to_c(setup), buf, n)
        )
        return self

    def frame(
        self,
        dx: int = 0,
        dy: int = 0,
        wheel: int = 0,
        pan: int = 0,
        edges: Iterable[Tuple[Usage, Action]] = (),
        raw: Iterable[Tuple[int, Direction, bytes]] = (),
        transfers: Iterable[Union[Tuple[int, Setup], Tuple[int, Setup, bytes]]] = (),
    ) -> "ClipBuilder":
        """A general content frame: motion, wheel and pan deltas plus `(Usage, Action)` edges, `(ep, direction, bytes)` raw reports and `(ep, setup)` or `(ep, setup, out_bytes)` transfers on the same frame."""
        lib = _native.lib
        frame = lib.medius_clip_frame_new()
        if not frame:
            raise MemoryError("clip frame allocation failed")
        try:
            check(lib.medius_clip_frame_move(frame, _i16(dx, "dx"), _i16(dy, "dy")))
            check(lib.medius_clip_frame_wheel(frame, _i16(wheel, "wheel")))
            check(lib.medius_clip_frame_pan(frame, _i16(pan, "pan")))
            for i, (usage, action) in enumerate(edges or ()):
                action = _enum(action, Action, f"edges[{i}].action")
                check(lib.medius_clip_frame_edge(frame, usage._c, int(action)))
            for i, (ep, direction, data) in enumerate(raw or ()):
                direction = _enum(direction, Direction, f"raw[{i}].direction")
                buf, n = _bytes_buf(data, f"raw[{i}].bytes")
                ep = _u8(ep, f"raw[{i}].ep")
                check(lib.medius_clip_frame_raw(frame, ep, int(direction), buf, n))
            for i, item in enumerate(transfers or ()):
                if not isinstance(item, (tuple, list)) or len(item) not in (2, 3):
                    raise ValueError(
                        f"transfers[{i}] must be (ep, setup) or (ep, setup, out_bytes), got {item!r}"
                    )
                ep, setup = item[0], item[1]
                out = item[2] if len(item) == 3 else b""
                buf, n = _bytes_buf(out, f"transfers[{i}].out")
                ep = _u8(ep, f"transfers[{i}].ep")
                check(lib.medius_clip_frame_transfer(frame, ep, setup_to_c(setup), buf, n))
            check(lib.medius_clip_builder_frame(self._ptr, frame))
        finally:
            lib.medius_clip_frame_free(frame)
        return self


class ClipHandle:
    """A handle to one box's buffered-clip playback, from `Device.clip`; keep one handle per clip session.

    A trigger runs a clip verb on the box, with no host round trip. There are two kinds in one set: an
    input trigger (`bind`) fires on a button, key or media edge, and a packet trigger (`bind_packet`)
    fires on a packet crossing a traffic surface. `clear_triggers` removes both and `query_config`
    reads both back.
    """

    def __init__(self, handle, device=None):
        self._handle = handle
        self._device = device  # keep the device alive while the handle is open

    def close(self):
        if getattr(self, "_handle", None):
            _native.lib.medius_clip_free(self._handle)
            self._handle = None

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()

    def __del__(self):
        self.close()

    def append(self, builder: ClipBuilder):
        """Append the builder's entries to the ring (whole-entry frames, each with the next append seq).

        Every entry is checked before the first frame goes out, so a refusal sends nothing: a frame
        past `CLIP_EDGES_MAX` edges or `CLIP_RAW_MAX` raw reports (`ClipFrameCountError`), one that
        encodes past `CLIP_ENTRY_MAX` bytes (`ClipFrameTooLongError`), a raw report whose direction is
        neither `Direction.IN` nor `Direction.OUT` (`RawDirectionError`, or `RelativeDirectionError`
        for the bearing-relative pair), or a transfer whose data does not match its setup packet
        (`ClipTransferDataError`).
        """
        check(_native.lib.medius_clip_append(self._handle, builder._ptr))

    def set_autolock(self, scope: Optional[Sequence[Blanket]] = None):
        """Auto-lock these input groups while the clip plays (clip-owned, released on stop). Set before the first append."""
        groups = [int(_enum(b, Blanket, "scope")) for b in (scope or [])]
        arr = (_native.u8 * len(groups))(*groups)
        ptr = ctypes.cast(arr, ctypes.POINTER(_native.u8)) if groups else ctypes.POINTER(_native.u8)()
        check(_native.lib.medius_clip_set_autolock(self._handle, ptr, len(groups)))

    def set_loop(self, on: bool):
        """Loop playback at the clip end (retained mode only)."""
        check(_native.lib.medius_clip_set_loop(self._handle, 1 if on else 0))

    def set_retain(self, on: bool):
        """Retain the loaded clip so it can rewind and replay (False = streaming, the default). Set before the first append."""
        check(_native.lib.medius_clip_set_retain(self._handle, 1 if on else 0))

    def set_ride(self, on: bool):
        """Make the clip's motion wait to ride a native report (False = the box's own clock, the default); only its wheel and pan while rendering is on with a profile armed."""
        check(_native.lib.medius_clip_set_ride(self._handle, 1 if on else 0))

    def bind(self, trigger: ClipTrigger):
        """Add or overwrite an input trigger: `trigger.on`'s edge fires its action on the box, no host round-trip."""
        t = _native.MediusClipTrigger(
            trigger.on._c,
            int(_enum(trigger.edge, Edge, "edge")),
            int(_enum(trigger.action, ClipAction, "action")),
            1 if trigger.consume else 0,
        )
        check(_native.lib.medius_clip_bind(self._handle, t))

    def unbind(self, usage: Usage, edge: Edge):
        """Remove the input trigger on `usage`'s `edge`."""
        edge = _enum(edge, Edge, "edge")
        check(_native.lib.medius_clip_unbind(self._handle, usage._c, int(edge)))

    def bind_packet(self, trigger: ClipPacketTrigger):
        """Add or overwrite a packet trigger: a packet `trigger` matches fires its action on the box's
        next tick, no host round trip. Binding a key the box holds overwrites it.

        What the box would refuse is `ClipPacketTriggerError` before anything is sent, and the message
        says which:

        - a `traffic_class` that is ``BUS`` or ``CLIP_TRANSFER``;
        - a match past `PKT_MATCH_MAX` bytes, or unlike its mask in length;
        - a direction the class never carries: ``OUT`` on ``HID_IN`` or ``EMIT``, ``IN`` on
          ``HID_OUT``;
        - a match bit outside its mask, which no packet can equal;
        - ``consume`` on ``CONTROL``;
        - a ``selector_len`` without ``once_per_run``;
        - ``once_per_run`` without one stream (a class other than ``CONTROL``, a concrete ``id``, and
          ``IN`` or ``OUT``), without match bytes past its selector, or with no masked bit in them.

        A bearing-relative direction is `RelativeDirectionError`.

        The box makes three checks this call cannot. A consuming trigger needs
        `Device.allow_imperfect_clones`, the set holds `CLIP_PKT_TRIG_MAX` triggers, and their match
        bytes share a pool of `CLIP_PKT_MATCH_POOL`. A bind the box refuses leaves its set as it
        was: a new key is not held, and a key the box holds keeps the trigger that was there, with its
        own action and flags. To confirm a bind, compare the fields `query_config` reads back with the
        ones bound.
        """
        c = clip_packet_trigger_to_c(trigger)
        check(_native.lib.medius_clip_bind_packet(self._handle, ctypes.byref(c)))

    def unbind_packet(self, trigger: ClipPacketTrigger):
        """Remove the packet trigger keyed by `trigger`'s ``(traffic_class, id, direction,
        match_bytes, mask)``; its other fields are ignored. A key the box cannot hold is refused as
        `bind_packet` refuses it: the class, the lengths, the direction, and a match bit outside the
        mask."""
        c = clip_packet_trigger_to_c(trigger, key_only=True)
        check(_native.lib.medius_clip_unbind_packet(self._handle, ctypes.byref(c)))

    def clear_triggers(self):
        """Remove every trigger of both kinds: the input triggers and the packet triggers."""
        check(_native.lib.medius_clip_clear_triggers(self._handle))

    def start(self):
        """Rewind and play (resume from a pause)."""
        check(_native.lib.medius_clip_start(self._handle))

    def stop(self):
        """Stop, flush a streaming clip (rewind a retained one), release held input and the clip auto-lock."""
        check(_native.lib.medius_clip_stop(self._handle))

    def pause(self):
        """Halt mid-clip, retaining the cursor and any held input."""
        check(_native.lib.medius_clip_pause(self._handle))

    def resume(self):
        """Continue from the paused cursor."""
        check(_native.lib.medius_clip_resume(self._handle))

    def restart(self):
        """Force a rewind and play, even mid-playback."""
        check(_native.lib.medius_clip_restart(self._handle))

    def toggle(self):
        """Toggle: play if idle/paused, stop if playing."""
        check(_native.lib.medius_clip_toggle(self._handle))

    def clear(self):
        """Discard the loaded clip, free the ring, and clear a fault."""
        check(_native.lib.medius_clip_clear(self._handle))

    def finalize(self):
        """Finalize a retained clip: fix its end so it can replay and loop."""
        check(_native.lib.medius_clip_finalize(self._handle))

    def query_status(self) -> ClipStatus:
        """The ring depth, progress, and playback counters. A `FAULTED` state means recover with `clear`."""
        out = _native.MediusClipStatus()
        check(_native.lib.medius_clip_query_status(self._handle, ctypes.byref(out)))
        return clip_status_from_c(out)

    def query_config(self) -> ClipSettings:
        """The clip configuration: autolock, loop, retain, finalized, and both kinds of trigger."""
        out = _native.MediusClipSettings()
        check(_native.lib.medius_clip_query_config(self._handle, ctypes.byref(out)))
        return clip_settings_from_c(out)
