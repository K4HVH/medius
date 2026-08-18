"""Error type and the status-to-exception check."""

from __future__ import annotations

import ctypes

from . import _native
from ._enums import Status


class MediusError(Exception):
    """A failed medius_capi call carrying the Status, last error text, and proto_ver byte."""

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


class CatchTableFullError(MediusError):
    """The subscription needs more entries than the box's table holds."""


class EmptySubscriptionError(MediusError):
    """A catch subscription with no filters, which would never yield an event."""


class CaptureNotApplicableError(MediusError):
    """A capture on an input class, which arrives decoded and carries no packet."""


class NotAnInputFilterError(MediusError):
    """A traffic class passed to `input_events`, which cannot decode one."""


class WildcardNotInputError(MediusError):
    """`CatchFilter.everything()` passed to `input_events`; it covers traffic too."""


class HalfEdgeInputFilterError(MediusError):
    """An input filter narrowed to one edge, which cannot be decoded into press and release."""


class ReservedIdError(MediusError):
    """An exact id equal to the blanket sentinel, which would address the whole class."""


class RelativeDirectionError(MediusError):
    """`Direction.WITH` / `AGAINST` on something with no bearing to measure them against."""


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
    Status.ERR_CATCH_TABLE_FULL: CatchTableFullError,
    Status.ERR_EMPTY_SUBSCRIPTION: EmptySubscriptionError,
    Status.ERR_CAPTURE_NOT_APPLICABLE: CaptureNotApplicableError,
    Status.ERR_NOT_AN_INPUT_FILTER: NotAnInputFilterError,
    Status.ERR_WILDCARD_NOT_INPUT: WildcardNotInputError,
    Status.ERR_HALF_EDGE_INPUT_FILTER: HalfEdgeInputFilterError,
    Status.ERR_RESERVED_ID: ReservedIdError,
    Status.ERR_RELATIVE_DIRECTION: RelativeDirectionError,
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
