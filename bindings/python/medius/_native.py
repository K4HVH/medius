"""ctypes layer over the medius_capi C ABI.

Loads the shared library and declares every function signature plus a `ctypes`
mirror of every `Medius*` type, field for field with the generated header. The
rest of the package builds the Pythonic API on top of this module; nothing here
imports the rest of the package.
"""

from __future__ import annotations

import ctypes
import os
import sys
from ctypes.util import find_library
from pathlib import Path

MEDIUS_MAX_KEYS = 256
MEDIUS_MAX_MEDIA_KEYS = 256
MEDIUS_MAX_LOG_TEXT = 512
MEDIUS_MAX_PATH = 512
MEDIUS_MAX_PRODUCT = 128
MEDIUS_MAX_SERIAL = 128

u8 = ctypes.c_uint8
u16 = ctypes.c_uint16
u32 = ctypes.c_uint32
u64 = ctypes.c_uint64
i16 = ctypes.c_int16
i32 = ctypes.c_int32
usize = ctypes.c_size_t
c_bool = ctypes.c_bool
HANDLE = ctypes.c_void_p
PHANDLE = ctypes.POINTER(ctypes.c_void_p)


# --- value types (field layouts mirror medius.h exactly) ---


class MediusPortInfo(ctypes.Structure):
    _fields_ = [
        ("path", ctypes.c_char * MEDIUS_MAX_PATH),
        ("vid", u16),
        ("pid", u16),
        ("serial", ctypes.c_char * MEDIUS_MAX_SERIAL),
        ("has_serial", u8),
    ]


class MediusMotion(ctypes.Structure):
    _fields_ = [("kind", u8), ("dx", i16), ("dy", i16), ("wheel", i16)]


class MediusInput(ctypes.Structure):
    _fields_ = [("kind", u8), ("value", u16)]


class MediusLockTarget(ctypes.Structure):
    _fields_ = [("kind", u8), ("button", u8)]


class MediusVersion(ctypes.Structure):
    _fields_ = [
        ("proto_ver", u8),
        ("fw_major", u8),
        ("fw_minor", u8),
        ("fw_patch", u8),
        ("mac", u8 * 6),
    ]


class MediusHealth(ctypes.Structure):
    _fields_ = [
        ("link_up", u8),
        ("mouse_attached", u8),
        ("clone_configured", u8),
        ("injection_active", u8),
        ("rate_confident", u8),
        ("lock_on", u8),
        ("catch_on", u8),
        ("kbd_attached", u8),
    ]


class MediusDeviceInfo(ctypes.Structure):
    _fields_ = [
        ("vid", u16),
        ("pid", u16),
        ("bcd_device", u16),
        ("bcd_usb", u16),
        ("has_serial", u8),
        ("has_bos", u8),
        ("kind", u8),
        ("product", ctypes.c_char * MEDIUS_MAX_PRODUCT),
    ]


class MediusBoxInfo(ctypes.Structure):
    _fields_ = [
        ("port", MediusPortInfo),
        ("version", MediusVersion),
        ("device", MediusDeviceInfo),
    ]


class MediusMouseCaps(ctypes.Structure):
    _fields_ = [
        ("n_buttons", u8),
        ("has_x", u8),
        ("has_y", u8),
        ("has_wheel", u8),
        ("has_report_id", u8),
        ("n_hid", u8),
    ]


class MediusKbdCaps(ctypes.Structure):
    _fields_ = [
        ("n_keys", u8),
        ("nkro", u8),
        ("has_consumer", u8),
        ("has_system", u8),
        ("has_report_id", u8),
    ]


class MediusCaps(ctypes.Structure):
    _fields_ = [
        ("mouse", MediusMouseCaps),
        ("keyboard", MediusKbdCaps),
        ("mouse_change_driven", u8),
        ("kbd_change_driven", u8),
    ]


class MediusRate(ctypes.Structure):
    _fields_ = [
        ("native_period_us", u16),
        ("poll_period_us", u16),
        ("confident", u8),
        ("change_driven", u8),
    ]


class MediusStats(ctypes.Structure):
    _fields_ = [
        ("inject_emits", u32),
        ("tx_drops", u16),
        ("tx_merges", u16),
        ("tx_maxdepth", u8),
        ("tx_wedges", u8),
        ("wakeups", u16),
        ("reset_count", u16),
        ("config_count", u16),
    ]


class MediusLocks(ctypes.Structure):
    _fields_ = [("mask", u16)]


class MediusCatchState(ctypes.Structure):
    _fields_ = [("mask", u8), ("dropped", u32)]


class MediusImperfectStatus(ctypes.Structure):
    _fields_ = [("allowed", u8), ("over_capacity", u8), ("clone_imperfect", u8)]


class MediusEmitPaceStatus(ctypes.Structure):
    _fields_ = [("mode", u8), ("fixed_hz", u16), ("resolved_hz", u16)]


class MediusCountersSnapshot(ctypes.Structure):
    _fields_ = [("frames_tx", u64), ("frames_rx", u64), ("crc_drops", u64), ("reconnects", u64)]


class MediusMouseEvent(ctypes.Structure):
    _fields_ = [("buttons", u8), ("dx", i16), ("dy", i16), ("wheel", i16)]


class MediusKeyboardEvent(ctypes.Structure):
    _fields_ = [("modifiers", u8), ("n_keys", u8), ("keys", u8 * MEDIUS_MAX_KEYS)]


class MediusMediaEvent(ctypes.Structure):
    _fields_ = [("n_keys", u8), ("keys", u16 * MEDIUS_MAX_MEDIA_KEYS)]


class MediusCatchEventData(ctypes.Union):
    _fields_ = [
        ("mouse", MediusMouseEvent),
        ("keyboard", MediusKeyboardEvent),
        ("media", MediusMediaEvent),
    ]


class MediusCatchEvent(ctypes.Structure):
    _fields_ = [("kind", u8), ("data", MediusCatchEventData)]


class MediusLogLine(ctypes.Structure):
    _fields_ = [("level", u8), ("text", ctypes.c_char * MEDIUS_MAX_LOG_TEXT)]


# --- library loading ---


def _candidate_names():
    if sys.platform == "darwin":
        return ["libmedius_capi.dylib"]
    if os.name == "nt":
        return ["medius_capi.dll", "libmedius_capi.dll"]
    return ["libmedius_capi.so"]


def _load_library():
    # MEDIUS_LIB wins so dev/test runs can point at target/debug; then the
    # bundled binary next to this file; then the system loader.
    override = os.environ.get("MEDIUS_LIB")
    if override:
        return ctypes.CDLL(override)

    here = Path(__file__).resolve().parent
    for name in _candidate_names():
        bundled = here / name
        if bundled.exists():
            return ctypes.CDLL(str(bundled))

    for name in _candidate_names():
        try:
            return ctypes.CDLL(name)
        except OSError:
            continue
    found = find_library("medius_capi")
    if found:
        return ctypes.CDLL(found)
    raise OSError(
        "could not locate the medius_capi shared library; build it "
        "(cargo build -p medius-capi) and set MEDIUS_LIB to its path"
    )


lib = _load_library()


def _decl(name, restype, argtypes, optional=False):
    try:
        fn = getattr(lib, name)
    except AttributeError:
        if optional:
            return None
        raise
    fn.restype = restype
    fn.argtypes = argtypes
    return fn


# --- lifecycle ---
_decl("medius_device_open", i32, [ctypes.c_char_p, PHANDLE])
_decl("medius_device_find", i32, [PHANDLE])
_decl("medius_device_open_by_id", i32, [ctypes.c_char_p, PHANDLE])
_decl("medius_device_find_mouse_box", i32, [PHANDLE])
_decl("medius_device_find_keyboard_box", i32, [PHANDLE])
_decl("medius_device_clone", HANDLE, [HANDLE])
_decl("medius_device_free", None, [HANDLE])
_decl("medius_find_ports", usize, [ctypes.POINTER(MediusPortInfo), usize, ctypes.POINTER(usize)])
_decl("medius_list", usize, [ctypes.POINTER(MediusBoxInfo), usize, ctypes.POINTER(usize)])

# --- commands ---
_decl("medius_device_move_rel", i32, [HANDLE, i16, i16])
_decl("medius_device_wheel", i32, [HANDLE, i16])
_decl("medius_device_move_axis", i32, [HANDLE, MediusMotion])
_decl("medius_device_inject", i32, [HANDLE, MediusInput, u8])
_decl("medius_device_button", i32, [HANDLE, u8, u8])
_decl("medius_device_press", i32, [HANDLE, u8])
_decl("medius_device_soft_release", i32, [HANDLE, u8])
_decl("medius_device_force_release", i32, [HANDLE, u8])
_decl("medius_device_key", i32, [HANDLE, u8, u8])
_decl("medius_device_key_down", i32, [HANDLE, u8])
_decl("medius_device_key_up", i32, [HANDLE, u8])
_decl("medius_device_key_force_release", i32, [HANDLE, u8])
_decl("medius_device_media", i32, [HANDLE, u16, u8])
_decl("medius_device_media_down", i32, [HANDLE, u16])
_decl("medius_device_media_up", i32, [HANDLE, u16])
_decl("medius_device_media_force_release", i32, [HANDLE, u16])
_decl("medius_device_lock", i32, [HANDLE, MediusLockTarget, u8])
_decl("medius_device_unlock", i32, [HANDLE, MediusLockTarget, u8])
_decl("medius_device_lock_key", i32, [HANDLE, u8, u8])
_decl("medius_device_unlock_key", i32, [HANDLE, u8, u8])
_decl("medius_device_lock_media", i32, [HANDLE, u16])
_decl("medius_device_unlock_media", i32, [HANDLE, u16])
_decl("medius_device_lock_all", i32, [HANDLE, u8])
_decl("medius_device_unlock_all", i32, [HANDLE, u8])
_decl("medius_device_led", i32, [HANDLE, u8, u8, u8])
_decl("medius_device_reset", i32, [HANDLE])
_decl("medius_device_reapply", i32, [HANDLE])
_decl("medius_device_reconnect", i32, [HANDLE])
_decl("medius_device_reboot", i32, [HANDLE, u8])
_decl("medius_device_allow_imperfect_clones", i32, [HANDLE, c_bool])
_decl("medius_device_set_movement_riding", i32, [HANDLE, c_bool, u32])
_decl("medius_device_set_emit_pace", i32, [HANDLE, u8, u16])

# --- queries ---
_decl("medius_device_query_version", i32, [HANDLE, ctypes.POINTER(MediusVersion)])
_decl("medius_device_query_health", i32, [HANDLE, ctypes.POINTER(MediusHealth)])
_decl("medius_device_device_info", i32, [HANDLE, ctypes.POINTER(MediusDeviceInfo)])
_decl("medius_device_caps", i32, [HANDLE, ctypes.POINTER(MediusCaps)])
_decl("medius_device_query_rate", i32, [HANDLE, ctypes.POINTER(MediusRate)])
_decl("medius_device_query_stats", i32, [HANDLE, ctypes.POINTER(MediusStats)])
_decl("medius_device_query_locks", i32, [HANDLE, ctypes.POINTER(MediusLocks)])
_decl("medius_device_query_catch", i32, [HANDLE, ctypes.POINTER(MediusCatchState)])
_decl("medius_device_query_imperfect", i32, [HANDLE, ctypes.POINTER(MediusImperfectStatus)])
_decl(
    "medius_device_query_movement_riding",
    i32,
    [HANDLE, ctypes.POINTER(c_bool), ctypes.POINTER(u32)],
)
_decl("medius_device_query_emit_pace", i32, [HANDLE, ctypes.POINTER(MediusEmitPaceStatus)])
_decl("medius_device_counters", i32, [HANDLE, ctypes.POINTER(MediusCountersSnapshot)])

# --- meta ---
_decl("medius_default_query_timeout_ms", u32, [])
_decl("medius_default_keepalive_cadence_ms", u32, [])
_decl("medius_abi_version", u32, [])
_decl("medius_version_string", ctypes.c_char_p, [])
_decl("medius_last_error_message", usize, [ctypes.c_char_p, usize])
_decl("medius_last_error_proto_ver", u8, [])

# --- pure helpers ---
_decl("medius_input_button", MediusInput, [u8])
_decl("medius_input_key", MediusInput, [u8])
_decl("medius_input_media", MediusInput, [u16])
_decl("medius_motion_cursor", MediusMotion, [i16, i16])
_decl("medius_motion_wheel", MediusMotion, [i16])
_decl("medius_locks_is_locked", c_bool, [MediusLocks, MediusLockTarget, u8])
_decl("medius_rate_native_hz", c_bool, [MediusRate, ctypes.POINTER(ctypes.c_float)])
_decl("medius_mouse_event_is_pressed", c_bool, [ctypes.POINTER(MediusMouseEvent), u8])
_decl("medius_keyboard_event_is_pressed", c_bool, [ctypes.POINTER(MediusKeyboardEvent), u8])
_decl("medius_media_event_is_pressed", c_bool, [ctypes.POINTER(MediusMediaEvent), u16])
_decl("medius_caps_has_mouse", c_bool, [MediusCaps])
_decl("medius_caps_has_keyboard", c_bool, [MediusCaps])
_decl("medius_caps_is_composite", c_bool, [MediusCaps])

# --- streams ---
_decl("medius_device_catch_events", i32, [HANDLE, u8, PHANDLE])
_decl("medius_event_stream_clone", HANDLE, [HANDLE])
_decl("medius_event_stream_free", None, [HANDLE])
_decl("medius_event_stream_recv", i32, [HANDLE, ctypes.POINTER(MediusCatchEvent)])
_decl("medius_event_stream_try_recv", c_bool, [HANDLE, ctypes.POINTER(MediusCatchEvent)])
_decl("medius_event_stream_recv_timeout", c_bool, [HANDLE, u64, ctypes.POINTER(MediusCatchEvent)])
_decl("medius_event_stream_dropped", u64, [HANDLE])
_decl("medius_device_logs", i32, [HANDLE, PHANDLE])
_decl("medius_log_stream_clone", HANDLE, [HANDLE])
_decl("medius_log_stream_free", None, [HANDLE])
_decl("medius_log_stream_recv", i32, [HANDLE, ctypes.POINTER(MediusLogLine)])
_decl("medius_log_stream_try_recv", c_bool, [HANDLE, ctypes.POINTER(MediusLogLine)])
_decl("medius_log_stream_recv_timeout", c_bool, [HANDLE, u64, ctypes.POINTER(MediusLogLine)])

# --- flash (feature-gated, optional) ---
HAS_FLASH = _decl("medius_flash", i32, [ctypes.c_char_p, ctypes.c_char_p, c_bool], optional=True) is not None

# --- mock (feature-gated, optional) ---
HAS_MOCK = _decl("medius_mock_new", HANDLE, [], optional=True) is not None
if HAS_MOCK:
    _decl("medius_mock_clone", HANDLE, [HANDLE])
    _decl("medius_mock_free", None, [HANDLE])
    _decl("medius_mock_set_version", None, [HANDLE, MediusVersion])
    _decl("medius_mock_set_health", None, [HANDLE, MediusHealth])
    _decl("medius_mock_set_device_info", None, [HANDLE, MediusDeviceInfo])
    _decl("medius_mock_set_caps", None, [HANDLE, MediusCaps])
    _decl("medius_mock_set_mouse_caps", None, [HANDLE, MediusMouseCaps])
    _decl("medius_mock_set_kbd_caps", None, [HANDLE, MediusKbdCaps])
    _decl("medius_mock_set_rate", None, [HANDLE, MediusRate])
    _decl("medius_mock_set_stats", None, [HANDLE, MediusStats])
    _decl("medius_mock_set_locks", None, [HANDLE, MediusLocks])
    _decl("medius_mock_set_catch_state", None, [HANDLE, MediusCatchState])
    _decl("medius_mock_set_imperfect_status", None, [HANDLE, MediusImperfectStatus])
    _decl("medius_mock_set_movement_riding", None, [HANDLE, c_bool, u32])
    _decl("medius_mock_set_emit_pace", None, [HANDLE, u8, u16])
    _decl("medius_mock_silent", None, [HANDLE])
    _decl("medius_mock_push_raw", None, [HANDLE, ctypes.POINTER(u8), usize])
    _decl("medius_mock_push_log", None, [HANDLE, u8, ctypes.c_char_p])
    _decl("medius_mock_push_event", None, [HANDLE, u8, MediusMouseEvent])
    _decl("medius_mock_push_kb_event", None, [HANDLE, u8, ctypes.POINTER(MediusKeyboardEvent)])
    _decl("medius_mock_push_cons_event", None, [HANDLE, u8, ctypes.POINTER(MediusMediaEvent)])
    _decl("medius_mock_recorded", usize, [HANDLE])
    _decl("medius_mock_saw", c_bool, [HANDLE, u8])
    _decl("medius_mock_clear_recorded", None, [HANDLE])
    _decl(
        "medius_mock_recorded_frame",
        usize,
        [HANDLE, usize, ctypes.POINTER(u8), ctypes.POINTER(u8), ctypes.POINTER(u8), usize],
    )
    _decl("medius_device_with_mock", i32, [HANDLE, PHANDLE])
    _decl("medius_device_open_mock", i32, [HANDLE, PHANDLE])
