"""Error type and the status-to-exception check."""

from __future__ import annotations

import ctypes

from . import _native
from ._enums import Status


class MediusError(Exception):
    """A failed medius_capi call. Carries the `Status`, the box's last error text,
    and, for a bad-proto-version failure, the offending `proto_ver` byte."""

    def __init__(self, status, message="", proto_ver=0):
        self.status = status
        self.message = message
        self.proto_ver = proto_ver
        name = status.name if isinstance(status, Status) else str(status)
        super().__init__("{}: {}".format(name, message) if message else name)


class IoError(MediusError):
    pass


class NotFoundError(MediusError):
    pass


class NoReplyError(MediusError):
    pass


class BadProtoVerError(MediusError):
    pass


class QueryTimeoutError(MediusError):
    pass


class DisconnectedError(MediusError):
    pass


class FrameTooLongError(MediusError):
    pass


class FlashToolError(MediusError):
    pass


class InvalidArgError(MediusError):
    pass


class PanicError(MediusError):
    pass


_STATUS_EXC = {
    Status.ERR_IO: IoError,
    Status.ERR_NOT_FOUND: NotFoundError,
    Status.ERR_NO_REPLY: NoReplyError,
    Status.ERR_BAD_PROTO_VER: BadProtoVerError,
    Status.ERR_QUERY_TIMEOUT: QueryTimeoutError,
    Status.ERR_DISCONNECTED: DisconnectedError,
    Status.ERR_FRAME_TOO_LONG: FrameTooLongError,
    Status.ERR_FLASH_TOOL: FlashToolError,
    Status.ERR_INVALID_ARG: InvalidArgError,
    Status.ERR_PANIC: PanicError,
}


def last_error_message():
    cap = 256
    buf = ctypes.create_string_buffer(cap)
    full = _native.lib.medius_last_error_message(buf, cap)
    if full >= cap:
        cap = full + 1
        buf = ctypes.create_string_buffer(cap)
        _native.lib.medius_last_error_message(buf, cap)
    return buf.value.decode("utf-8", "replace")


def check(status):
    """Raise the matching `MediusError` subclass when `status` is not OK."""
    try:
        st = Status(status)
    except ValueError:
        st = Status.ERR_UNKNOWN
    if st == Status.OK:
        return
    message = last_error_message()
    proto = int(_native.lib.medius_last_error_proto_ver())
    raise _STATUS_EXC.get(st, MediusError)(st, message, proto)
