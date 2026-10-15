"""keeps a native handle alive while calls on it run"""

from __future__ import annotations

import threading

from . import _native


class Handle:
    # ctypes lets go of the GIL during a call, so the last call still running frees the handle
    _free = ""
    # medius-capi takes these mutably, so their calls also run one at a time
    _exclusive = False

    def __init__(self, handle):
        self._handle = handle
        self._lock = threading.Lock()
        self._users = 0
        self._closing = False
        self._turn = threading.Lock() if self._exclusive else None

    def _call(self, fn, *args):
        with self._lock:
            if self._closing:
                handle = None
            else:
                handle = self._handle
                self._users += 1
        if handle is None:
            return fn(None, *args)
        try:
            if self._turn is None:
                return fn(handle, *args)
            with self._turn:
                return fn(handle, *args)
        finally:
            with self._lock:
                self._users -= 1
                last = self._closing and self._users == 0
            if last:
                self._release()

    def close(self):
        with self._lock:
            if self._closing:
                return
            self._closing = True
            last = self._users == 0
        if last:
            self._release()

    def _release(self):
        handle, self._handle = self._handle, None
        if handle:
            getattr(_native.lib, self._free)(handle)

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()

    def __del__(self):
        try:
            self.close()
        except Exception:
            pass
