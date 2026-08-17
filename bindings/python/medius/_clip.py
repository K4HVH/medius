"""Buffered clip playback: the clip-entry builder and clip handle (§3.11)."""

from __future__ import annotations

import ctypes
from typing import Optional, Sequence, Tuple

from . import _native
from ._enums import Action, Blanket, Edge
from ._errors import check
from ._types import (
    ClipSettings,
    ClipStatus,
    ClipTrigger,
    Usage,
    clip_settings_from_c,
    clip_status_from_c,
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

    def gap(self, frames: int) -> "ClipBuilder":
        """Emit nothing for `frames` native frames (a zero count is a no-op)."""
        check(_native.lib.medius_clip_builder_gap(self._ptr, frames))
        return self

    def move(self, dx: int, dy: int) -> "ClipBuilder":
        """A cursor-motion frame."""
        check(_native.lib.medius_clip_builder_move(self._ptr, dx, dy))
        return self

    def wheel(self, dz: int) -> "ClipBuilder":
        """A wheel frame."""
        check(_native.lib.medius_clip_builder_wheel(self._ptr, dz))
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
        check(_native.lib.medius_clip_builder_edge(self._ptr, usage._c, int(action)))
        return self

    def frame(
        self,
        dx: int = 0,
        dy: int = 0,
        wheel: int = 0,
        edges: Optional[Sequence[Tuple[Usage, Action]]] = None,
    ) -> "ClipBuilder":
        """A general content frame: a motion delta plus a list of `(Usage, Action)` edges on the same frame."""
        edges = edges or []
        n = len(edges)
        inputs = (_native.MediusUsage * n)()
        actions = (ctypes.c_uint8 * n)()
        for i, (inp, action) in enumerate(edges):
            inputs[i] = inp._c
            actions[i] = int(action)
        iptr = ctypes.cast(inputs, ctypes.POINTER(_native.MediusUsage)) if n else None
        aptr = ctypes.cast(actions, ctypes.POINTER(ctypes.c_uint8)) if n else None
        check(_native.lib.medius_clip_builder_frame(self._ptr, dx, dy, wheel, iptr, aptr, n))
        return self


class ClipHandle:
    """A handle to one box's buffered-clip playback, from `Device.clip`; keep one handle per clip session."""

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
        """Append the builder's entries to the ring (whole-entry frames, each with the next append seq)."""
        check(_native.lib.medius_clip_append(self._handle, builder._ptr))

    def set_autolock(self, scope: Optional[Sequence[Blanket]] = None):
        """Auto-lock these input groups while the clip plays (clip-owned, released on stop). Set before the first append."""
        groups = [int(b) for b in (scope or [])]
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
        """Make the clip's motion wait to ride a native report (False = the box's own clock, the default)."""
        check(_native.lib.medius_clip_set_ride(self._handle, 1 if on else 0))

    def bind(self, trigger: ClipTrigger):
        """Add or overwrite a trigger binding: `trigger.on`'s edge fires its action on the box, no host round-trip."""
        t = _native.MediusClipTrigger(
            trigger.on._c, int(trigger.edge), int(trigger.action), 1 if trigger.consume else 0
        )
        check(_native.lib.medius_clip_bind(self._handle, t))

    def unbind(self, usage: Usage, edge: Edge):
        """Remove the trigger binding on `usage`'s `edge`."""
        check(_native.lib.medius_clip_unbind(self._handle, usage._c, int(edge)))

    def clear_triggers(self):
        """Remove every trigger binding."""
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
        """The clip configuration: autolock, loop, retain, finalized, and the trigger set."""
        out = _native.MediusClipSettings()
        check(_native.lib.medius_clip_query_config(self._handle, ctypes.byref(out)))
        return clip_settings_from_c(out)
