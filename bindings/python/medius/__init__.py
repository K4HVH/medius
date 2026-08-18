"""Python bindings for the medius transparent mouse passthrough box."""

from __future__ import annotations

from typing import List

from . import _native
from ._enums import (
    Axis,
    Action,
    BEARING_WINDOW_DEFAULT_MS,
    BearingMode,
    Blanket,
    LOCK_SCALE_BLOCK,
    LOCK_SCALE_MAX,
    LOCK_SCALE_PASS,
    BusEventKind,
    Button,
    CatchClass,
    CatchEventKind,
    ClipAction,
    ClipState,
    ClockDomain,
    ControlStatus,
    DeviceKind,
    Edge,
    EmitMode,
    FrameType,
    Class,
    Key,
    LedMode,
    LedTarget,
    Direction,
    LockTargetKind,
    LogLevel,
    MediaKey,
    MotionKind,
    MoveTiming,
    PendingMotion,
    RebootTarget,
    Status,
    InputKind,
    TrafficClass,
)
from ._errors import (
    BadProtoVerError,
    CaptureNotApplicableError,
    CatchTableFullError,
    DisconnectedError,
    EmptySubscriptionError,
    FlashToolError,
    FrameTooLongError,
    HalfEdgeInputFilterError,
    InvalidArgError,
    IoError,
    MediusError,
    NoReplyError,
    NotAnInputFilterError,
    NotFoundError,
    PanicError,
    QueryTimeoutError,
    RelativeDirectionError,
    ReservedIdError,
    WildcardNotInputError,
)
from ._device import Device
from ._clip import ClipBuilder, ClipHandle
from ._streams import EventStream, InputStream, LogStream, Timeline
from ._mock import MockBox
from ._types import (
    Bearing,
    BoxInfo,
    BusEvent,
    Caps,
    CatchEntry,
    CatchEvent,
    CatchFilter,
    CatchState,
    ClipSettings,
    ClipStatus,
    ClipTrigger,
    ClockEstimate,
    Counters,
    DeviceInfo,
    EmitPace,
    EmitPaceStatus,
    Health,
    ImperfectStatus,
    Usage,
    KbdCaps,
    Locks,
    LockEntry,
    LockTarget,
    LogLine,
    Motion,
    MotionEvent,
    MouseCaps,
    PortInfo,
    Rate,
    RecordedFrame,
    Stats,
    TrafficEvent,
    UsageSnapshot,
    Version,
    box_from_c,
    Capture,
    InputEvent,
    Stamped,
)

HAS_MOCK = _native.HAS_MOCK
HAS_FLASH = _native.HAS_FLASH


def find_ports(cap: int = 16) -> List[PortInfo]:
    """Enumerate the medius serial ports currently present."""
    import ctypes

    from ._types import port_from_c

    arr = (_native.MediusPortInfo * cap)()
    total = _native.usize(0)
    n = _native.lib.medius_find_ports(arr, cap, ctypes.byref(total))
    return [port_from_c(arr[i]) for i in range(min(int(n), cap))]


def list_boxes(cap: int = 16) -> List[BoxInfo]:
    """Enumerate every connected box: opens each, handshakes, and reads its version + device info."""
    import ctypes

    arr = (_native.MediusBoxInfo * cap)()
    total = _native.usize(0)
    n = _native.lib.medius_list(arr, cap, ctypes.byref(total))
    return [box_from_c(arr[i]) for i in range(min(int(n), cap))]


def default_query_timeout_ms() -> int:
    return int(_native.lib.medius_default_query_timeout_ms())


def default_keepalive_cadence_ms() -> int:
    return int(_native.lib.medius_default_keepalive_cadence_ms())


def abi_version() -> int:
    return int(_native.lib.medius_abi_version())


def version_string() -> str:
    return _native.lib.medius_version_string().decode("utf-8", "replace")


def flash(port: str, bin_path: str, host: bool = False) -> None:
    """Reboot a chip to ROM download and flash a firmware binary via esptool (Linux/Windows only)."""
    if not HAS_FLASH:
        raise RuntimeError(
            "the loaded medius_capi library was built without the flash feature "
            "(rebuild with --features flash)"
        )
    from ._errors import check

    check(_native.lib.medius_flash(port.encode("utf-8"), bin_path.encode("utf-8"), bool(host)))


__all__ = [
    "Action",
    "BEARING_WINDOW_DEFAULT_MS",
    "BearingMode",
    "Blanket",
    "LOCK_SCALE_BLOCK",
    "LOCK_SCALE_MAX",
    "LOCK_SCALE_PASS",
    "BusEventKind",
    "Button",
    "CatchClass",
    "CatchEventKind",
    "ClipAction",
    "ClipState",
    "ClockDomain",
    "ControlStatus",
    "DeviceKind",
    "Edge",
    "EmitMode",
    "FrameType",
    "Class",
    "Key",
    "LedMode",
    "LedTarget",
    "Direction",
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
    "ClipBuilder",
    "ClipHandle",
    "EventStream",
    "LogStream",
    "MockBox",
    "BoxInfo",
    "BusEvent",
    "Caps",
    "CatchEntry",
    "CatchEvent",
    "Axis",
    "Capture",
    "InputEvent",
    "InputKind",
    "InputStream",
    "Stamped",
    "Timeline",
    "TrafficClass",
    "CatchTableFullError",
    "EmptySubscriptionError",
    "CaptureNotApplicableError",
    "NotAnInputFilterError",
    "WildcardNotInputError",
    "HalfEdgeInputFilterError",
    "ReservedIdError",
    "RelativeDirectionError",
    "CatchFilter",
    "CatchState",
    "Bearing",
    "ClipSettings",
    "ClipStatus",
    "ClipTrigger",
    "ClockEstimate",
    "Counters",
    "DeviceInfo",
    "EmitPace",
    "EmitPaceStatus",
    "Health",
    "ImperfectStatus",
    "Usage",
    "KbdCaps",
    "Locks",
    "LockEntry",
    "LockTarget",
    "LogLine",
    "Motion",
    "MotionEvent",
    "MoveTiming",
    "PendingMotion",
    "MouseCaps",
    "PortInfo",
    "Rate",
    "RecordedFrame",
    "Stats",
    "TrafficEvent",
    "UsageSnapshot",
    "Version",
    "find_ports",
    "list_boxes",
    "default_query_timeout_ms",
    "default_keepalive_cadence_ms",
    "abi_version",
    "version_string",
    "flash",
    "HAS_MOCK",
    "HAS_FLASH",
]
