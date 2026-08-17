"""CATCH and LOG stream wrappers."""

from __future__ import annotations

import ctypes
import time
from typing import List, Optional

from . import _native
from ._enums import Class, ClockDomain, Status
from ._errors import MediusError, check
from ._types import (
    CatchEvent,
    InputEvent,
    LogLine,
    Stamped,
    Usage,
    decode_catch_event,
    decode_log_line,
    input_event_from_c,
)


class EventStream:
    """A live CATCH event stream. Iterate it to consume events until the link drops."""

    def __init__(self, handle, device=None):
        self._handle = handle
        self._device = device  # keep the device alive while the stream is open

    def recv(self) -> CatchEvent:
        """Block for the next event. Raises `DisconnectedError` when the stream closes."""
        ev = _native.MediusCatchEvent()
        check(_native.lib.medius_event_stream_recv(self._handle, ctypes.byref(ev)))
        return decode_catch_event(ev)

    def try_recv(self) -> Optional[CatchEvent]:
        ev = _native.MediusCatchEvent()
        if _native.lib.medius_event_stream_try_recv(self._handle, ctypes.byref(ev)):
            return decode_catch_event(ev)
        return None

    def recv_timeout(self, ms) -> Optional[CatchEvent]:
        ev = _native.MediusCatchEvent()
        if _native.lib.medius_event_stream_recv_timeout(self._handle, int(ms), ctypes.byref(ev)):
            return decode_catch_event(ev)
        return None

    @property
    def dropped(self) -> int:
        return int(_native.lib.medius_event_stream_dropped(self._handle))

    @property
    def is_connected(self) -> bool:
        """Whether the box is still delivering.

        `try_recv` and `recv_timeout` both answer `None` for "nothing yet" and for "nothing ever
        again". This separates them: one means wait longer, the other means stop.
        """
        return bool(_native.lib.medius_event_stream_is_connected(self._handle))

    def clone(self) -> "EventStream":
        """Another handle to the same subscription; the queue is shared."""
        handle = _native.lib.medius_event_stream_clone(self._handle)
        if not handle:
            raise MediusError(Status.ERR_UNKNOWN, "event stream clone failed")
        return EventStream(handle, self._device)

    def __iter__(self):
        while True:
            try:
                yield self.recv()
            except MediusError as e:
                if e.status == Status.ERR_DISCONNECTED:
                    return
                raise

    def close(self):
        if self._handle is not None:
            _native.lib.medius_event_stream_free(self._handle)
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


class InputStream:
    """A live stream of decoded input: press and release edges, and motion.

    The box sends held-usage snapshots; this diffs them into the edges they represent. Iterate it to
    consume events until the link drops.
    """

    def __init__(self, handle, device=None):
        self._handle = handle
        self._device = device  # keep the device alive while the stream is open

    def recv(self) -> InputEvent:
        """Block for the next input event. Raises `DisconnectedError` when the stream closes."""
        ev = _native.MediusInputEvent()
        check(_native.lib.medius_input_stream_recv(self._handle, ctypes.byref(ev)))
        return input_event_from_c(ev)

    def try_recv(self) -> Optional[InputEvent]:
        ev = _native.MediusInputEvent()
        if _native.lib.medius_input_stream_try_recv(self._handle, ctypes.byref(ev)):
            return input_event_from_c(ev)
        return None

    def recv_timeout(self, ms) -> Optional[InputEvent]:
        ev = _native.MediusInputEvent()
        if _native.lib.medius_input_stream_recv_timeout(self._handle, int(ms), ctypes.byref(ev)):
            return input_event_from_c(ev)
        return None

    def held(self, input_class: Class) -> List[Usage]:
        """Which usages of `input_class` this stream currently holds."""
        cap = 16
        while True:
            buf = (_native.MediusUsage * cap)()
            n = int(_native.lib.medius_input_stream_held(self._handle, int(input_class), buf, cap))
            if n <= cap:
                return [Usage(_native.MediusUsage(kind=u.kind, id=u.id)) for u in buf[:n]]
            cap = n

    @property
    def dropped(self) -> int:
        return int(_native.lib.medius_input_stream_dropped(self._handle))

    @property
    def is_connected(self) -> bool:
        """Whether the box is still delivering. See `EventStream.is_connected`."""
        return bool(_native.lib.medius_input_stream_is_connected(self._handle))

    def __iter__(self):
        while True:
            try:
                yield self.recv()
            except MediusError as e:
                if e.status == Status.ERR_DISCONNECTED:
                    return
                raise

    def close(self):
        if self._handle is not None:
            _native.lib.medius_input_stream_free(self._handle)
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


class Timeline:
    """Puts box stamps on this machine's clock.

    A catch stamp is microseconds on a chip that booted before this process did: it wraps every ~71.6
    minutes and has no relation to any clock here. Feed every event in as it arrives, in order::

        with dev.catch_events(CatchFilter.all_input()) as events:
            time = Timeline()
            for ev in events:
                print(ev, time.observe(ev).host_ns)

    Each domain is tracked separately. The mapping keeps a per-domain minimum of (elapsed here minus
    elapsed on the box) rather than an average, because the error is one-sided -- an event can arrive
    late but never early -- so it improves as it runs and never degrades.
    """

    def __init__(self):
        self._handle = _native.lib.medius_timeline_new()
        if not self._handle:
            raise MediusError(Status.ERR_UNKNOWN, "timeline allocation failed")

    def observe(self, event, now_ns: Optional[int] = None) -> Stamped:
        """Place `event` on this machine's clock. `now_ns` defaults to `time.monotonic_ns()`.

        Takes a `CatchEvent` or an `InputEvent`; both share one timeline, so a caller reading the
        decoded and the raw stream together gets one comparable ordering.
        """
        if now_ns is None:
            now_ns = time.monotonic_ns()
        out = _native.MediusStamped()
        if isinstance(event, InputEvent):
            c = _native.MediusInputEvent(
                kind=int(event.kind), ts_us=int(event.ts_us), clock=int(event.clock)
            )
            if not _native.lib.medius_timeline_observe_input(
                self._handle, ctypes.byref(c), int(now_ns), ctypes.byref(out)
            ):
                raise MediusError(Status.ERR_INVALID_ARG, "timeline observe failed")
            return Stamped(int(out.host_ns), int(out.box_us), int(out.excess_ns))
        # Only the stamp and the domain are read; rebuilding those two is cheaper and safer than
        # carrying the whole union back across the boundary.
        c = _native.MediusCatchEvent(
            kind=int(event.kind), ts_us=int(event.ts_us), clock=int(event.clock)
        )
        if not _native.lib.medius_timeline_observe(
            self._handle, ctypes.byref(c), int(now_ns), ctypes.byref(out)
        ):
            raise MediusError(Status.ERR_INVALID_ARG, "timeline observe failed")
        return Stamped(int(out.host_ns), int(out.box_us), int(out.excess_ns))

    def reset(self, domain: ClockDomain) -> None:
        """Forget one domain's rollover count and measured floor, for a chip that rebooted."""
        _native.lib.medius_timeline_reset(self._handle, int(domain))

    def samples(self, domain: ClockDomain) -> int:
        """Events observed for a domain; the floor is a minimum over these."""
        return int(_native.lib.medius_timeline_samples(self._handle, int(domain)))

    def close(self):
        if self._handle is not None:
            _native.lib.medius_timeline_free(self._handle)
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


class LogStream:
    """A device LOG stream. Iterate it to consume lines until the link drops."""

    def __init__(self, handle, device=None):
        self._handle = handle
        self._device = device

    def recv(self) -> LogLine:
        """Block for the next log line. Raises `DisconnectedError` when the stream closes."""
        line = _native.MediusLogLine()
        check(_native.lib.medius_log_stream_recv(self._handle, ctypes.byref(line)))
        return decode_log_line(line)

    def try_recv(self) -> Optional[LogLine]:
        line = _native.MediusLogLine()
        if _native.lib.medius_log_stream_try_recv(self._handle, ctypes.byref(line)):
            return decode_log_line(line)
        return None

    def recv_timeout(self, ms) -> Optional[LogLine]:
        line = _native.MediusLogLine()
        if _native.lib.medius_log_stream_recv_timeout(self._handle, int(ms), ctypes.byref(line)):
            return decode_log_line(line)
        return None

    def clone(self) -> "LogStream":
        """Another handle to the same LOG channel."""
        handle = _native.lib.medius_log_stream_clone(self._handle)
        if not handle:
            raise MediusError(Status.ERR_UNKNOWN, "log stream clone failed")
        return LogStream(handle, self._device)

    def __iter__(self):
        while True:
            try:
                yield self.recv()
            except MediusError as e:
                if e.status == Status.ERR_DISCONNECTED:
                    return
                raise

    def close(self):
        if self._handle is not None:
            _native.lib.medius_log_stream_free(self._handle)
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
