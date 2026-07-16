"""Buffered clip playback: the clip-entry builder and clip handle (§3.11)."""

from __future__ import annotations

import ctypes
from typing import Optional, Sequence, Tuple

from . import _native
from ._enums import Action, Blanket, Button
from ._errors import check
from ._types import Input, ClipStatus, clip_status_from_c


class ClipConfig:
    """Playback options for a clip `start` or catch trigger. The single place clip settings live; extensible
    as more are added. `autolock` is the list of `Blanket` groups to lock while playing (None/[] = none;
    `list(Blanket)` for every class)."""

    def __init__(self, autolock: Optional[Sequence[Blanket]] = None):
        self.autolock = list(autolock) if autolock is not None else []

    def _c(self):
        """Build a (MediusClipConfig, backing array) pair; keep the array alive for the call's duration."""
        groups = [int(b) for b in self.autolock]
        arr = (_native.u8 * len(groups))(*groups)
        ptr = ctypes.cast(arr, ctypes.POINTER(_native.u8)) if groups else ctypes.POINTER(_native.u8)()
        return _native.MediusClipConfig(ptr, len(groups)), arr


class ClipBuilder:
    """Builds a buffered-clip entry stream. Each call appends one per-frame entry: motion is a relative
    delta, edges are actions that stick until a later frame changes them (like `inject`), and a `gap` run
    emits nothing for N frames. Pass it to `ClipHandle.append`, then `clear()` to reuse it."""

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

    def press(self, button: Button) -> "ClipBuilder":
        """A frame that presses a button."""
        check(_native.lib.medius_clip_builder_press(self._ptr, int(button)))
        return self

    def release(self, button: Button) -> "ClipBuilder":
        """A frame that soft-releases a button (a physical hold is left intact)."""
        check(_native.lib.medius_clip_builder_release(self._ptr, int(button)))
        return self

    def force_release(self, button: Button) -> "ClipBuilder":
        """A frame that force-releases a button (masks a physical hold too)."""
        check(_native.lib.medius_clip_builder_force_release(self._ptr, int(button)))
        return self

    def key(self, usage, action: Action = Action.PRESS) -> "ClipBuilder":
        """A frame carrying one key edge."""
        check(_native.lib.medius_clip_builder_key(self._ptr, int(usage), int(action)))
        return self

    def media(self, usage, action: Action = Action.PRESS) -> "ClipBuilder":
        """A frame carrying one media edge."""
        check(_native.lib.medius_clip_builder_media(self._ptr, int(usage), int(action)))
        return self

    def edge(self, input: Input, action: Action = Action.PRESS) -> "ClipBuilder":
        """A one-edge frame for any `Input` (button/key/media) with an `Action` — the field-generic form the
        press/release/key/media helpers wrap."""
        check(_native.lib.medius_clip_builder_edge(self._ptr, input._c, int(action)))
        return self

    def frame(
        self,
        dx: int = 0,
        dy: int = 0,
        wheel: int = 0,
        edges: Optional[Sequence[Tuple[Input, Action]]] = None,
    ) -> "ClipBuilder":
        """A general content frame: a motion delta plus a list of `(Input, Action)` edges on the same frame."""
        edges = edges or []
        n = len(edges)
        inputs = (_native.MediusInput * n)()
        actions = (ctypes.c_uint8 * n)()
        for i, (inp, action) in enumerate(edges):
            inputs[i] = inp._c
            actions[i] = int(action)
        iptr = ctypes.cast(inputs, ctypes.POINTER(_native.MediusInput)) if n else None
        aptr = ctypes.cast(actions, ctypes.POINTER(ctypes.c_uint8)) if n else None
        check(_native.lib.medius_clip_builder_frame(self._ptr, dx, dy, wheel, iptr, aptr, n))
        return self


class ClipHandle:
    """A handle to one box's buffered-clip playback, from `Device.clip`. Owns the append-sequence counter,
    so keep one handle for a clip session and top it up with `append`."""

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

    def start(self, config: Optional[ClipConfig] = None):
        """Begin playback from the ring head with a `ClipConfig` (its `autolock` scope, extensible). With no
        config, plays with no auto-lock."""
        cfg, _arr = (config or ClipConfig())._c()
        check(_native.lib.medius_clip_start(self._handle, cfg))

    def stop(self):
        """Stop playback, flush the ring, release any clip-owned auto-lock."""
        check(_native.lib.medius_clip_stop(self._handle))

    def arm_catch(self, trigger: Input, config: Optional[ClipConfig] = None):
        """Arm an on-device trigger: playback starts on a physical press of `trigger`, any `Input` (a button,
        key, or media usage) built with `Input.button` / `Input.key` / `Input.media`, with `config` when it
        fires. For any input, use `arm_catch_any`."""
        cfg, _arr = (config or ClipConfig())._c()
        check(_native.lib.medius_clip_arm_catch(self._handle, trigger._c, cfg))

    def arm_catch_any(self, config: Optional[ClipConfig] = None):
        """Arm a trigger on any physical input (button, key, or media), with `config` when it fires."""
        cfg, _arr = (config or ClipConfig())._c()
        check(_native.lib.medius_clip_arm_catch_any(self._handle, cfg))

    def disarm(self):
        """Clear a pending catch-arm."""
        check(_native.lib.medius_clip_disarm(self._handle))

    def status(self) -> ClipStatus:
        """The ring depth (`free`/`used`) and playback counters. A `FAULTED` state means re-sync."""
        out = _native.MediusClipStatus()
        check(_native.lib.medius_clip_status(self._handle, ctypes.byref(out)))
        return clip_status_from_c(out)
