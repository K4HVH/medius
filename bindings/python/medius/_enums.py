"""Enumerations mirroring the medius_capi wire values."""

from __future__ import annotations

from enum import IntEnum


class Status(IntEnum):
    OK = 0
    ERR_IO = 1
    ERR_NOT_FOUND = 2
    ERR_NO_REPLY = 3
    ERR_BAD_PROTO_VER = 4
    ERR_QUERY_TIMEOUT = 5
    ERR_DISCONNECTED = 6
    ERR_FRAME_TOO_LONG = 7
    ERR_FLASH_TOOL = 8
    ERR_INVALID_ARG = 9
    ERR_PANIC = 10
    ERR_UNKNOWN = 11
    ERR_CATCH_TABLE_FULL = 12
    ERR_EMPTY_SUBSCRIPTION = 13
    ERR_CAPTURE_NOT_APPLICABLE = 14
    ERR_NOT_AN_INPUT_FILTER = 15
    ERR_WILDCARD_NOT_INPUT = 16
    ERR_HALF_EDGE_INPUT_FILTER = 17
    ERR_RESERVED_ID = 18
    ERR_RELATIVE_DIRECTION = 19


class DeviceKind(IntEnum):
    """The cloned device's primary kind (its Boot-interface protocol)."""

    UNKNOWN = 0
    KEYBOARD = 1
    MOUSE = 2


class Button(IntEnum):
    LEFT = 0
    RIGHT = 1
    MIDDLE = 2
    SIDE1 = 3
    SIDE2 = 4


class Action(IntEnum):
    SOFT_RELEASE = 0
    PRESS = 1
    FORCE_RELEASE = 2


class MoveTiming(IntEnum):
    """When a delta reaches the game PC, against movement riding."""

    RIDE = 0
    NOW = 1


class PendingMotion(IntEnum):
    """What a move does to the motion already held for a ride."""

    KEEP = 0
    FLUSH = 1
    DISCARD = 2


class ClipState(IntEnum):
    """The device-side clip lifecycle state (`ClipStatus.state`)."""

    IDLE = 0
    PLAYING = 1
    PAUSED = 2
    FAULTED = 3


class Edge(IntEnum):
    """Which edge of a trigger usage fires its `ClipTrigger`."""

    BOTH = 0
    PRESS = 1
    RELEASE = 2


class ClipAction(IntEnum):
    """The engine action a `ClipTrigger` drives."""

    START = 0
    STOP = 1
    PAUSE = 2
    RESUME = 3
    RESTART = 4
    TOGGLE = 5


class RebootTarget(IntEnum):
    DEVICE_DOWNLOAD = 0
    HOST_DOWNLOAD = 1
    DEVICE_RUN = 2
    HOST_RUN = 3


class EmitMode(IntEnum):
    LEARNED = 0
    INTERVAL = 1
    FIXED = 2


class LedTarget(IntEnum):
    DEVICE = 0
    HOST = 1
    BOTH = 2


class LedMode(IntEnum):
    AUTO = 0
    OFF = 1
    SOLID = 2
    BLINK = 3


class Direction(IntEnum):
    """Which way, on the one byte LOCK, CLIP and CATCH all carry.

    The members are named for the axis reading; `PRESS`/`RELEASE` and `IN`/`OUT` are the same two
    values under names that read at the call site. Which applies is decided by the class.

    `WITH` and `AGAINST` name a sign relative to the bearing, the direction the box is currently
    injecting, so they follow the aim instead of the axis. Axes only, and inert until a bearing is
    live (see `Device.set_bearing`).
    """

    BOTH = 0
    POSITIVE = 1
    NEGATIVE = 2
    WITH = 3
    AGAINST = 4

    @property
    def is_relative(self) -> bool:
        """Whether this direction is measured against the bearing rather than a fixed sign."""
        return self in (Direction.WITH, Direction.AGAINST)


#: A momentary usage going down (`Direction.POSITIVE`).
Direction.PRESS = Direction.POSITIVE
#: A momentary usage coming up (`Direction.NEGATIVE`).
Direction.RELEASE = Direction.NEGATIVE
#: Traffic from the device to the PC (`Direction.POSITIVE`).
Direction.IN = Direction.POSITIVE
#: Traffic from the PC to the device (`Direction.NEGATIVE`).
Direction.OUT = Direction.NEGATIVE


class BearingMode(IntEnum):
    """How the box decides whether physical motion runs with or against its own injection."""

    #: Each axis compares its own sign against its own bearing, independently.
    PER_AXIS = 0
    #: The aim is projected onto the injected XY vector; motion across it passes untouched. One
    #: relative scale governs the whole aim, the lower of X's and Y's, and that is what reads back.
    #: Each axis's absolute scale then applies to what the projection left, not to the sign the hand
    #: moved: it is a statement about what reaches the PC.
    VECTOR = 1


#: LOCK scale: keep none of the physical value.
LOCK_SCALE_BLOCK = 0
#: LOCK scale: keep all of it, the unweighed default.
LOCK_SCALE_PASS = 100
#: LOCK scale ceiling: 2.55x.
LOCK_SCALE_MAX = 255
#: The bearing window the box holds before any host sets one, in ms.
BEARING_WINDOW_DEFAULT_MS = 20


class Axis(IntEnum):
    """A relative axis. Values match the wire axis id a CATCH or LOCK entry carries."""

    X = 0
    Y = 1
    WHEEL = 2


class LockTargetKind(IntEnum):
    X = 0
    Y = 1
    WHEEL = 2
    USAGE = 3


class Blanket(IntEnum):
    # ABI-local ordinals matching the Rust MediusBlanket, not the CLIP_LOCK_* wire bits.
    AIM = 0
    WHEEL = 1
    BUTTONS = 2
    KEYS = 3
    MEDIA = 4


class LogLevel(IntEnum):
    ERROR = 0
    WARN = 1
    INFO = 2
    DEBUG = 3
    VERBOSE = 4


class CatchEventKind(IntEnum):
    MOTION = 0
    USAGES = 1
    TRAFFIC = 2


class CatchClass(IntEnum):
    """What a `CatchFilter` addresses. 0-3 are the classes LOCK and INJECT address; 4-10 are relayed traffic."""

    BUTTON = 0
    KEY = 1
    MEDIA = 2
    AXIS = 3
    HID_IN = 4
    HID_OUT = 5
    VENDOR_INTERRUPT = 6
    VENDOR_BULK = 7
    CONTROL = 8
    EMIT = 9
    BUS = 10

    def is_input(self) -> bool:
        """A parsed-input class: it arrives decoded and carries no packet, so a capture means nothing."""
        return self <= CatchClass.AXIS

    def is_traffic(self) -> bool:
        """One of the seven byte-oriented traffic classes."""
        return not self.is_input()


class TrafficClass(IntEnum):
    """The byte-oriented half of the catch address space, for the `traffic` constructors."""

    HID_IN = 4
    HID_OUT = 5
    VENDOR_INTERRUPT = 6
    VENDOR_BULK = 7
    CONTROL = 8
    EMIT = 9
    BUS = 10


class InputKind(IntEnum):
    """Which arm of an `InputEvent` is populated."""

    PRESS = 0
    RELEASE = 1
    MOTION = 2


class ClockDomain(IntEnum):
    """Which chip's clock stamped an event. The two boot independently, so never subtract across domains."""

    HOST_CHIP = 0
    DEVICE_CHIP = 1


class ControlStatus(IntEnum):
    """What the real device answered a proxied control transaction with."""

    OK = 0
    STALLED = 1
    NAKED = 2
    #: A status byte this build does not know; read `TrafficEvent.flags` for its value. Distinct from
    #: the three, so a future firmware's new status is not reported as a device fault that never
    #: happened -- and so decoding one does not raise.
    OTHER = 3


class BusEventKind(IntEnum):
    """What a `CatchClass.BUS` event describes."""

    RESET = 0
    SUSPEND = 1
    RESUME = 2
    CONFIGURED = 3
    DECONFIGURED = 4
    SET_INTERFACE = 5
    DEVICE_ATTACHED = 6
    DEVICE_DETACHED = 7
    CLONE_UP = 8
    CLONE_DOWN = 9


class MotionKind(IntEnum):
    CURSOR = 0
    WHEEL = 1


class Class(IntEnum):
    BUTTON = 0
    KEY = 1
    MEDIA = 2


class FrameType(IntEnum):
    MOVE = 1
    INJECT = 3
    RESET = 4
    QUERY = 5
    RESP = 6
    REBOOT_DL = 7
    LOG = 8
    LED = 9
    LOCK = 10
    CATCH = 11
    MOTION_EVENT = 12
    USAGE_EVENT = 15
    TRAFFIC_EVENT = 22
    OPTION = 17
    CLIP_APPEND = 18
    CLIP_CTRL = 19
    CLIP_SET = 20
    CLIP_TRIGGER = 21


class Key(IntEnum):
    A = 4
    B = 5
    C = 6
    D = 7
    E = 8
    F = 9
    G = 10
    H = 11
    I = 12
    J = 13
    K = 14
    L = 15
    M = 16
    N = 17
    O = 18
    P = 19
    Q = 20
    R = 21
    S = 22
    T = 23
    U = 24
    V = 25
    W = 26
    X = 27
    Y = 28
    Z = 29
    N1 = 30
    N2 = 31
    N3 = 32
    N4 = 33
    N5 = 34
    N6 = 35
    N7 = 36
    N8 = 37
    N9 = 38
    N0 = 39
    ENTER = 40
    ESCAPE = 41
    BACKSPACE = 42
    TAB = 43
    SPACE = 44
    CAPS_LOCK = 57
    F1 = 58
    F2 = 59
    F3 = 60
    F4 = 61
    F5 = 62
    F6 = 63
    F7 = 64
    F8 = 65
    F9 = 66
    F10 = 67
    F11 = 68
    F12 = 69
    INSERT = 73
    HOME = 74
    PAGE_UP = 75
    DELETE = 76
    END = 77
    PAGE_DOWN = 78
    RIGHT = 79
    LEFT = 80
    DOWN = 81
    UP = 82
    LEFT_CTRL = 224
    LEFT_SHIFT = 225
    LEFT_ALT = 226
    LEFT_GUI = 227
    RIGHT_CTRL = 228
    RIGHT_SHIFT = 229
    RIGHT_ALT = 230
    RIGHT_GUI = 231


class MediaKey(IntEnum):
    PLAY = 176
    PAUSE = 177
    NEXT_TRACK = 181
    PREV_TRACK = 182
    STOP = 183
    PLAY_PAUSE = 205
    MUTE = 226
    VOLUME_UP = 233
    VOLUME_DOWN = 234
