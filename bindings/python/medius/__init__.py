"""Python bindings for the medius transparent mouse passthrough box.

A thin ctypes wrapper over the medius_capi C ABI. Open a box with
`Device.find()` or `Device.open(path)`, drive it with the command methods, read
state with the `query_*` methods, and consume physical input with
`catch_events()`.
"""

from __future__ import annotations

from typing import List, Optional

from . import _native
from ._enums import (
    Action,
    Blanket,
    Button,
    CatchEventKind,
    CatchMask,
    FrameType,
    InputKind,
    Key,
    LedMode,
    LedTarget,
    LockDirection,
    LockTargetKind,
    LogLevel,
    MediaKey,
    MotionKind,
    RebootTarget,
    Status,
)
from ._errors import (
    BadProtoVerError,
    DisconnectedError,
    FlashToolError,
    FrameTooLongError,
    InvalidArgError,
    IoError,
    MediusError,
    NoReplyError,
    NotFoundError,
    PanicError,
    QueryTimeoutError,
)
from ._device import Device
from ._streams import EventStream, LogStream
from ._mock import MockBox
from ._types import (
    Caps,
    CatchEvent,
    CatchState,
    Counters,
    Health,
    ImperfectStatus,
    Input,
    KbdCaps,
    KeyboardEvent,
    Locks,
    LockTarget,
    LogLine,
    MediaEvent,
    Motion,
    MouseCaps,
    MouseEvent,
    MouseInfo,
    PortInfo,
    Rate,
    RecordedFrame,
    Stats,
    Version,
)

HAS_MOCK = _native.HAS_MOCK
HAS_FLASH = _native.HAS_FLASH


def find_ports(cap: int = 16) -> List[PortInfo]:
    """Enumerate the medius serial ports currently present."""
    import ctypes

    arr = (_native.MediusPortInfo * cap)()
    total = _native.usize(0)
    n = _native.lib.medius_find_ports(arr, cap, ctypes.byref(total))
    out = []
    for i in range(min(int(n), cap)):
        pi = arr[i]
        out.append(PortInfo(pi.path.split(b"\x00", 1)[0].decode("utf-8", "replace"), pi.vid, pi.pid))
    return out


def default_query_timeout_ms() -> int:
    return int(_native.lib.medius_default_query_timeout_ms())


def default_keepalive_cadence_ms() -> int:
    return int(_native.lib.medius_default_keepalive_cadence_ms())


def abi_version() -> int:
    return int(_native.lib.medius_abi_version())


def version_string() -> str:
    return _native.lib.medius_version_string().decode("utf-8", "replace")


def flash(port: str, bin_path: str, host: bool = False) -> None:
    """Reboot a chip to ROM download and flash a firmware binary via esptool.
    Linux and Windows only. Requires a library built with the flash feature."""
    if not HAS_FLASH:
        raise RuntimeError(
            "the loaded medius_capi library was built without the flash feature "
            "(rebuild with --features flash)"
        )
    from ._errors import check

    check(_native.lib.medius_flash(port.encode("utf-8"), bin_path.encode("utf-8"), bool(host)))


__all__ = [
    "Action",
    "Blanket",
    "Button",
    "CatchEventKind",
    "CatchMask",
    "FrameType",
    "InputKind",
    "Key",
    "LedMode",
    "LedTarget",
    "LockDirection",
    "LockTargetKind",
    "LogLevel",
    "MediaKey",
    "MotionKind",
    "RebootTarget",
    "Status",
    "MediusError",
    "IoError",
    "NotFoundError",
    "NoReplyError",
    "BadProtoVerError",
    "QueryTimeoutError",
    "DisconnectedError",
    "FrameTooLongError",
    "FlashToolError",
    "InvalidArgError",
    "PanicError",
    "Device",
    "EventStream",
    "LogStream",
    "MockBox",
    "Caps",
    "CatchEvent",
    "CatchState",
    "Counters",
    "Health",
    "ImperfectStatus",
    "Input",
    "KbdCaps",
    "KeyboardEvent",
    "Locks",
    "LockTarget",
    "LogLine",
    "MediaEvent",
    "Motion",
    "MouseCaps",
    "MouseEvent",
    "MouseInfo",
    "PortInfo",
    "Rate",
    "RecordedFrame",
    "Stats",
    "Version",
    "find_ports",
    "default_query_timeout_ms",
    "default_keepalive_cadence_ms",
    "abi_version",
    "version_string",
    "flash",
    "HAS_MOCK",
    "HAS_FLASH",
]
