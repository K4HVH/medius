"""CATCH and LOG stream wrappers."""

from __future__ import annotations

import ctypes
from typing import Optional

from . import _native
from ._enums import Status
from ._errors import MediusError, check
from ._types import CatchEvent, LogLine, decode_catch_event, decode_log_line


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
