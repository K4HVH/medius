"""CATCH and LOG stream wrappers."""

from __future__ import annotations

import ctypes
import math
import time
from typing import List, Optional

from . import _native
from ._enums import Class, ClockDomain, Status
from ._errors import DisconnectedError, MediusError, check
from ._handle import Handle
from ._types import (
    CatchEvent,
    InputEvent,
    LogLine,
    Stamped,
    Usage,
    _enum,
    decode_catch_event,
    decode_log_line,
    input_event_from_c,
)

class _Stream(Handle):
    def __init__(self, handle, device=None):
        super().__init__(handle)
        self._device = device  # keep the device alive while the stream is open

    def close(self):
        """End the stream for every handle on it and free this one. A receive blocked on it in
        another thread returns at once, raising `DisconnectedError`, and frees the handle as it
        ends.
        """
        super().close()

    def _recv(self, fn, out) -> None:
        status = self._call(fn, ctypes.byref(out))
        if status == Status.OK:
            return
        if self._closing or status == Status.ERR_DISCONNECTED:
            raise self._ended()
        check(status)

    def _recv_timeout(self, fn, out, ms) -> bool:
        return bool(self._call(fn, max(0, math.ceil(ms)), ctypes.byref(out)))

    def _ended(self) -> DisconnectedError:
        text = "stream closed" if self._closing else "device disconnected"
        return DisconnectedError(Status.ERR_DISCONNECTED, text)

    def __iter__(self):
        while True:
            try:
                yield self.recv()
            except MediusError as e:
                if e.status == Status.ERR_DISCONNECTED:
                    return
                raise


class EventStream(_Stream):
    """A live CATCH event stream. Iterate it to consume events until the link drops."""

    _free = "medius_event_stream_free"
    _end = "medius_event_stream_close"

    def recv(self) -> CatchEvent:
        """Block for the next event. Raises `DisconnectedError` when the stream closes."""
        ev = _native.MediusCatchEvent()
        self._recv(_native.lib.medius_event_stream_recv, ev)
        return decode_catch_event(ev)

    def try_recv(self) -> Optional[CatchEvent]:
        ev = _native.MediusCatchEvent()
        if self._call(_native.lib.medius_event_stream_try_recv, ctypes.byref(ev)):
            return decode_catch_event(ev)
        return None

    def recv_timeout(self, ms) -> Optional[CatchEvent]:
        ev = _native.MediusCatchEvent()
        if self._recv_timeout(_native.lib.medius_event_stream_recv_timeout, ev, ms):
            return decode_catch_event(ev)
        return None

    @property
    def dropped(self) -> int:
        return int(self._call(_native.lib.medius_event_stream_dropped))

    @property
    def is_connected(self) -> bool:
        """Whether the box is still delivering.

        `try_recv` and `recv_timeout` return `None` both for "nothing yet" and for "nothing ever
        again"; this tells them apart.
        """
        return bool(self._call(_native.lib.medius_event_stream_is_connected))

    def clone(self) -> "EventStream":
        """Another handle to the same subscription; the queue is shared."""
        handle = self._call(_native.lib.medius_event_stream_clone)
        if not handle:
            if self._closing:
                raise self._ended()
            raise MediusError(Status.ERR_UNKNOWN, "event stream clone failed")
        return EventStream(handle, self._device)


class InputStream(_Stream):
    """A live stream of decoded input: press and release edges, and motion.

    The box sends held-usage snapshots; this diffs them into edges. Iterate it to consume events
    until the link drops.
    """

    _free = "medius_input_stream_free"
    _end = "medius_input_stream_close"

    def recv(self) -> InputEvent:
        """Block for the next input event. Raises `DisconnectedError` when the stream closes."""
        ev = _native.MediusInputEvent()
        self._recv(_native.lib.medius_input_stream_recv, ev)
        return input_event_from_c(ev)

    def try_recv(self) -> Optional[InputEvent]:
        ev = _native.MediusInputEvent()
        if self._call(_native.lib.medius_input_stream_try_recv, ctypes.byref(ev)):
            return input_event_from_c(ev)
        return None

    def recv_timeout(self, ms) -> Optional[InputEvent]:
        ev = _native.MediusInputEvent()
        if self._recv_timeout(_native.lib.medius_input_stream_recv_timeout, ev, ms):
            return input_event_from_c(ev)
        return None

    def held(self, input_class: Class) -> List[Usage]:
        """The usages of `input_class` this stream holds."""
        input_class = int(_enum(input_class, Class, "input_class"))
        cap = 16
        while True:
            buf = (_native.MediusUsage * cap)()
            n = int(self._call(_native.lib.medius_input_stream_held, input_class, buf, cap))
            if n <= cap:
                return [Usage(_native.MediusUsage(kind=u.kind, id=u.id)) for u in buf[:n]]
            cap = n

    @property
    def dropped(self) -> int:
        return int(self._call(_native.lib.medius_input_stream_dropped))

    @property
    def is_connected(self) -> bool:
        """Whether the box is still delivering. See `EventStream.is_connected`."""
        return bool(self._call(_native.lib.medius_input_stream_is_connected))


class Timeline(Handle):
    """Puts box stamps on this machine's clock.

    A catch stamp is microseconds on a chip that booted before this process: it wraps every ~71.6
    minutes and is unrelated to any clock here. Feed every event in as it arrives, in order::

        with dev.catch_events(CatchFilter.all_input()) as events:
            time = Timeline()
            for ev in events:
                print(ev, time.observe(ev).host_ns)

    Each domain is tracked separately, by a minimum of (elapsed here minus elapsed on the box): the
    error is one-sided (an event can arrive late, never early), so the mapping improves as it runs
    and never degrades.
    """

    _free = "medius_timeline_free"
    _exclusive = True

    def __init__(self):
        super().__init__(_native.lib.medius_timeline_new())
        if not self._handle:
            raise MediusError(Status.ERR_UNKNOWN, "timeline allocation failed")

    def observe(self, event, now_ns: Optional[int] = None) -> Stamped:
        """Place `event` on this machine's clock. `now_ns` defaults to `time.monotonic_ns()`.

        Takes a `CatchEvent` or an `InputEvent`; both share one timeline, so events from the decoded
        and raw streams order together.
        """
        if now_ns is None:
            now_ns = time.monotonic_ns()
        out = _native.MediusStamped()
        if isinstance(event, InputEvent):
            c = _native.MediusInputEvent(
                kind=int(event.kind),
                ts_us=int(event.ts_us),
                clock=int(_enum(event.clock, ClockDomain, "clock")),
            )
            if not self._call(
                _native.lib.medius_timeline_observe_input, ctypes.byref(c), int(now_ns), ctypes.byref(out)
            ):
                raise MediusError(Status.ERR_INVALID_ARG, "timeline observe failed")
            return Stamped(int(out.host_ns), int(out.box_us), int(out.excess_ns))
        # Only stamp and domain are read, so rebuild those two instead of carrying the whole union
        # back across the boundary.
        c = _native.MediusCatchEvent(
            kind=int(event.kind),
            ts_us=int(event.ts_us),
            clock=int(_enum(event.clock, ClockDomain, "clock")),
        )
        if not self._call(
            _native.lib.medius_timeline_observe, ctypes.byref(c), int(now_ns), ctypes.byref(out)
        ):
            raise MediusError(Status.ERR_INVALID_ARG, "timeline observe failed")
        return Stamped(int(out.host_ns), int(out.box_us), int(out.excess_ns))

    def reset(self, domain: ClockDomain) -> None:
        """Forget one domain's rollover count and measured floor, for a chip that rebooted."""
        self._call(_native.lib.medius_timeline_reset, int(_enum(domain, ClockDomain, "domain")))

    def samples(self, domain: ClockDomain) -> int:
        """Events observed for a domain; the floor is a minimum over these."""
        domain = _enum(domain, ClockDomain, "domain")
        return int(self._call(_native.lib.medius_timeline_samples, int(domain)))


class LogStream(_Stream):
    """A device LOG stream. Iterate it to consume lines until the link drops."""

    _free = "medius_log_stream_free"
    _end = "medius_log_stream_close"

    def recv(self) -> LogLine:
        """Block for the next log line. Raises `DisconnectedError` when the stream closes."""
        line = _native.MediusLogLine()
        self._recv(_native.lib.medius_log_stream_recv, line)
        return decode_log_line(line)

    def try_recv(self) -> Optional[LogLine]:
        line = _native.MediusLogLine()
        if self._call(_native.lib.medius_log_stream_try_recv, ctypes.byref(line)):
            return decode_log_line(line)
        return None

    def recv_timeout(self, ms) -> Optional[LogLine]:
        line = _native.MediusLogLine()
        if self._recv_timeout(_native.lib.medius_log_stream_recv_timeout, line, ms):
            return decode_log_line(line)
        return None

    @property
    def is_connected(self) -> bool:
        """Whether the box is still delivering. See `EventStream.is_connected`."""
        return bool(self._call(_native.lib.medius_log_stream_is_connected))

    def clone(self) -> "LogStream":
        """Another handle to the same LOG channel."""
        handle = self._call(_native.lib.medius_log_stream_clone)
        if not handle:
            if self._closing:
                raise self._ended()
            raise MediusError(Status.ERR_UNKNOWN, "log stream clone failed")
        return LogStream(handle, self._device)
