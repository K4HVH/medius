"""Keeps a native handle alive while calls on it run."""

from __future__ import annotations

import threading

from . import _native


class Handle:
    # ctypes releases the GIL during a call, so the handle is freed when the last running call on it
    # returns, never under one.
    _free = ""
    # A stream's native close: ends the stream and wakes a receive blocked on it.
    _end = ""
    # medius-capi takes these mutably, so their calls also run one at a time.
    _exclusive = False

    def __init__(self, handle):
        self._handle = handle
        self._lock = threading.Lock()
        self._users = 0
        self._closing = False
        self._turn = threading.Lock() if self._exclusive else None

    def _call(self, fn, *args):
        counted = False
        try:
            with self._lock:
                if not self._closing:
                    self._users += 1
                    counted = True
            handle = self._handle if counted else None
            if self._turn is None or handle is None:
                return fn(handle, *args)
            with self._turn:
                return fn(handle, *args)
        finally:
            if counted:
                self._leave()

    def _leave(self):
        with self._lock:
            self._users -= 1
            last = self._closing and self._users == 0
        if last:
            self._release()

    def _shut(self, end):
        with self._lock:
            if self._closing:
                return
            self._closing = True
            # Held through the native close, so a receive it wakes cannot free the handle under it.
            self._users += 1
        try:
            if end and self._end and self._handle:
                getattr(_native.lib, self._end)(self._handle)
        finally:
            self._leave()

    def close(self):
        """Free the handle once no call on it is running; a call in another thread ends first."""
        self._shut(end=True)

    def _release(self):
        handle, self._handle = self._handle, None
        if handle:
            getattr(_native.lib, self._free)(handle)

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()

    def __del__(self):
        # When garbage-collected, a stream's clone frees its handle and leaves the stream open for
        # the others.
        try:
            self._shut(end=False)
        except Exception:
            pass
