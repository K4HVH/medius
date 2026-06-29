"""Enumerations mirroring the medius_capi wire values."""

from __future__ import annotations

from enum import IntEnum, IntFlag


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


class RebootTarget(IntEnum):
    DEVICE_DOWNLOAD = 0
    HOST_DOWNLOAD = 1
    DEVICE_RUN = 2
    HOST_RUN = 3


class LedTarget(IntEnum):
    DEVICE = 0
    HOST = 1
    BOTH = 2


class LedMode(IntEnum):
    AUTO = 0
    OFF = 1
    SOLID = 2
    BLINK = 3


class LockDirection(IntEnum):
    BOTH = 0
    POSITIVE = 1
    NEGATIVE = 2


class LockTargetKind(IntEnum):
    X = 0
    Y = 1
    WHEEL = 2
    BUTTON = 3


class Blanket(IntEnum):
    KEYS = 0
    MEDIA = 1
    BUTTONS = 2


class LogLevel(IntEnum):
    ERROR = 0
    WARN = 1
    INFO = 2
    DEBUG = 3
    VERBOSE = 4


class CatchEventKind(IntEnum):
    MOUSE = 0
    KEYBOARD = 1
    MEDIA = 2


class MotionKind(IntEnum):
    CURSOR = 0
    WHEEL = 1


class InputKind(IntEnum):
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
    MOUSE_EVENT = 12
    KB_EVENT = 15
    CONS_EVENT = 16
    OPTION = 17


class CatchMask(IntFlag):
    MOTION = 1
    WHEEL = 2
    BUTTONS = 4
    KEYS = 8
    ALL = 15


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
