"""ctypes layer over the medius_capi C ABI."""

from __future__ import annotations

import ctypes
import os
import sys
from ctypes.util import find_library
from pathlib import Path

MEDIUS_MAX_USAGES = 256
MEDIUS_CLIP_TRIG_MAX = 8
MEDIUS_MAX_LOCKS = 256
MEDIUS_MAX_LOG_TEXT = 512
MEDIUS_MAX_PATH = 512
MEDIUS_MAX_PRODUCT = 128
MEDIUS_MAX_SERIAL = 128
MEDIUS_MAX_NAME = 33
MEDIUS_MAX_CATCH_ENTRIES = 32
MEDIUS_MAX_TRAFFIC_BYTES = 180

# The CATCH wildcards are sentinel values, not a separate flag byte.
MEDIUS_CATCH_CLASS_ANY = 0xFF
MEDIUS_CATCH_ID_ANY = 0xFFFF
MEDIUS_CLOCK_AGE_NONE = 0xFFFFFFFF
# MediusClockEstimate.rate_ppb when the box has fitted no drift rate. Different from a fitted 0, which
# says the two crystals are matched.
MEDIUS_CLOCK_RATE_NONE = -0x80000000

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


class MediusUsage(ctypes.Structure):
    _fields_ = [("kind", u8), ("id", u16)]


class MediusClipTrigger(ctypes.Structure):
    _fields_ = [("on", MediusUsage), ("edge", u8), ("action", u8), ("consume", u8)]


class MediusClipSettings(ctypes.Structure):
    _fields_ = [
        ("autolock_bits", u8),
        ("loop_", u8),
        ("retain", u8),
        ("finalized", u8),
        ("ride", u8),
        ("triggers", MediusClipTrigger * MEDIUS_CLIP_TRIG_MAX),
        ("n", u8),
    ]


class MediusLockTarget(ctypes.Structure):
    _fields_ = [("kind", u8), ("usage", MediusUsage)]


class MediusVersion(ctypes.Structure):
    _fields_ = [
        ("proto_ver", u8),
        ("fw_major", u8),
        ("fw_minor", u8),
        ("fw_patch", u8),
        ("mac", u8 * 6),
        ("name", ctypes.c_char * MEDIUS_MAX_NAME),
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


class MediusLockEntry(ctypes.Structure):
    _fields_ = [
        ("target", MediusLockTarget),
        ("is_blanket", c_bool),
        ("direction", u8),
        ("scale", u8),
    ]


class MediusLocks(ctypes.Structure):
    _fields_ = [("n", u16), ("entries", MediusLockEntry * MEDIUS_MAX_LOCKS)]


class MediusCatchFilter(ctypes.Structure):
    _fields_ = [("class_", u8), ("id", u16), ("direction", u8), ("capture", u8)]


class MediusCatchEntry(ctypes.Structure):
    _fields_ = [("filter", MediusCatchFilter), ("dropped", u16)]


class MediusClockEstimate(ctypes.Structure):
    _fields_ = [("offset_us", i32), ("rate_ppb", i32), ("delay_us", u16), ("age_ms", u32)]


class MediusCatchState(ctypes.Structure):
    _fields_ = [
        ("table_full", u8),
        ("dropped", u32),
        ("clock", MediusClockEstimate),
        ("n", u16),
        ("entries", MediusCatchEntry * MEDIUS_MAX_CATCH_ENTRIES),
    ]


class MediusImperfectStatus(ctypes.Structure):
    _fields_ = [("allowed", u8), ("over_capacity", u8), ("clone_imperfect", u8)]


class MediusBearing(ctypes.Structure):
    _fields_ = [("window_ms", u16), ("mode", u8)]


class MediusEmitPaceStatus(ctypes.Structure):
    _fields_ = [("mode", u8), ("fixed_hz", u16), ("resolved_hz", u16)]


class MediusClipStatus(ctypes.Structure):
    _fields_ = [
        ("state", u8),
        ("free", u32),
        ("total", u32),
        ("played", u32),
        ("ticks", u32),
        ("underruns", u16),
        ("overruns", u16),
        ("seq_gaps", u16),
        ("held_n", u16),
        ("held", MediusUsage * MEDIUS_MAX_USAGES),
    ]




class MediusCountersSnapshot(ctypes.Structure):
    _fields_ = [("frames_tx", u64), ("frames_rx", u64), ("crc_drops", u64), ("reconnects", u64)]


class MediusMotionEvent(ctypes.Structure):
    _fields_ = [("dx", i16), ("dy", i16), ("dz", i16)]


class MediusUsageEvent(ctypes.Structure):
    # Mirrors MediusUsageEvent in medius.h. A missing field here is not a decode bug, it is a buffer
    # overrun: the library writes sizeof(MediusCatchEvent) bytes into whatever this allocates, so a
    # struct short by one field lets every catch event write past the end of it.
    _fields_ = [
        ("class_", u8),
        ("direction", u8),
        ("n", u16),
        ("usages", MediusUsage * MEDIUS_MAX_USAGES),
    ]


class MediusTrafficEvent(ctypes.Structure):
    _fields_ = [
        ("class_", u8),
        ("id", u16),
        ("direction", u8),
        ("flags", u8),
        ("true_len", u16),
        ("len", u16),
        ("bytes", u8 * MEDIUS_MAX_TRAFFIC_BYTES),
    ]


class MediusBusEvent(ctypes.Structure):
    _fields_ = [("kind", u8), ("configuration", u8), ("interface", u8), ("alt", u8)]


class MediusCatchEventData(ctypes.Union):
    _fields_ = [
        ("motion", MediusMotionEvent),
        ("usages", MediusUsageEvent),
        ("traffic", MediusTrafficEvent),
    ]


class MediusCatchEvent(ctypes.Structure):
    _fields_ = [("kind", u8), ("ts_us", u32), ("clock", u8), ("data", MediusCatchEventData)]


class MediusInputEvent(ctypes.Structure):
    _fields_ = [
        ("kind", u8),
        ("ts_us", u32),
        ("clock", u8),
        ("usage", MediusUsage),
        ("dx", i16),
        ("dy", i16),
        ("dz", i16),
    ]


class MediusStamped(ctypes.Structure):
    _fields_ = [("host_ns", u64), ("box_us", u64), ("excess_ns", u64)]


class MediusLogLine(ctypes.Structure):
    _fields_ = [("level", u8), ("text", ctypes.c_char * MEDIUS_MAX_LOG_TEXT)]


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


_decl("medius_device_open", i32, [ctypes.c_char_p, PHANDLE])
_decl("medius_device_find", i32, [PHANDLE])
_decl("medius_device_open_by_id", i32, [ctypes.c_char_p, PHANDLE])
_decl("medius_device_find_mouse_box", i32, [PHANDLE])
_decl("medius_device_find_keyboard_box", i32, [PHANDLE])
_decl("medius_device_clone", HANDLE, [HANDLE])
_decl("medius_device_free", None, [HANDLE])
_decl("medius_find_ports", usize, [ctypes.POINTER(MediusPortInfo), usize, ctypes.POINTER(usize)])
_decl("medius_list", usize, [ctypes.POINTER(MediusBoxInfo), usize, ctypes.POINTER(usize)])

_decl("medius_device_move_rel", i32, [HANDLE, i16, i16])
_decl("medius_device_wheel", i32, [HANDLE, i16])
_decl("medius_device_move_rel_now", i32, [HANDLE, i16, i16])
_decl("medius_device_wheel_now", i32, [HANDLE, i16])
_decl("medius_device_flush_motion", i32, [HANDLE])
_decl("medius_device_discard_motion", i32, [HANDLE])
_decl("medius_device_move_axis", i32, [HANDLE, MediusMotion, u8, u8])
_decl("medius_device_inject", i32, [HANDLE, MediusUsage, u8])
_decl("medius_device_press", i32, [HANDLE, MediusUsage])
_decl("medius_device_soft_release", i32, [HANDLE, MediusUsage])
_decl("medius_device_force_release", i32, [HANDLE, MediusUsage])
_decl("medius_device_lock", i32, [HANDLE, MediusLockTarget, u8])
_decl("medius_device_unlock", i32, [HANDLE, MediusLockTarget, u8])
_decl("medius_device_lock_all", i32, [HANDLE, u8, u8])
_decl("medius_device_unlock_all", i32, [HANDLE, u8, u8])
_decl("medius_device_scale", i32, [HANDLE, MediusLockTarget, u8, u8])
_decl("medius_device_scale_all", i32, [HANDLE, u8, u8, u8])
_decl("medius_device_led", i32, [HANDLE, u8, u8, u8])
_decl("medius_device_reset", i32, [HANDLE])
_decl("medius_device_reapply", i32, [HANDLE])
_decl("medius_device_reconnect", i32, [HANDLE])
_decl("medius_device_reboot", i32, [HANDLE, u8])
_decl("medius_device_allow_imperfect_clones", i32, [HANDLE, c_bool])
_decl("medius_device_set_movement_riding", i32, [HANDLE, c_bool, u32])
_decl("medius_device_set_bearing", i32, [HANDLE, u16, u8])
_decl("medius_device_set_emit_pace", i32, [HANDLE, u8, u16])
_decl("medius_device_set_name", i32, [HANDLE, ctypes.c_char_p])
_decl("medius_device_clear_name", i32, [HANDLE])

_decl("medius_device_query_version", i32, [HANDLE, ctypes.POINTER(MediusVersion)])
_decl("medius_device_query_health", i32, [HANDLE, ctypes.POINTER(MediusHealth)])
_decl("medius_device_device_info", i32, [HANDLE, ctypes.POINTER(MediusDeviceInfo)])
_decl("medius_device_caps", i32, [HANDLE, ctypes.POINTER(MediusCaps)])
_decl("medius_device_query_rate", i32, [HANDLE, ctypes.POINTER(MediusRate)])
_decl("medius_device_query_stats", i32, [HANDLE, ctypes.POINTER(MediusStats)])
_decl("medius_device_query_locks", i32, [HANDLE, ctypes.POINTER(MediusLocks)])
_decl("medius_device_query_bearing", i32, [HANDLE, ctypes.POINTER(MediusBearing)])
_decl("medius_device_query_catch", i32, [HANDLE, ctypes.POINTER(MediusCatchState)])
_decl("medius_device_query_imperfect", i32, [HANDLE, ctypes.POINTER(MediusImperfectStatus)])
_decl(
    "medius_device_query_movement_riding",
    i32,
    [HANDLE, ctypes.POINTER(c_bool), ctypes.POINTER(u32)],
)
_decl("medius_device_query_emit_pace", i32, [HANDLE, ctypes.POINTER(MediusEmitPaceStatus)])
_decl("medius_device_counters", i32, [HANDLE, ctypes.POINTER(MediusCountersSnapshot)])

_decl("medius_default_query_timeout_ms", u32, [])
_decl("medius_default_keepalive_cadence_ms", u32, [])
_decl("medius_abi_version", u32, [])
_decl("medius_version_string", ctypes.c_char_p, [])
_decl("medius_last_error_message", usize, [ctypes.c_char_p, usize])
_decl("medius_last_error_proto_ver", u8, [])

_decl("medius_usage_button", MediusUsage, [u8])
_decl("medius_usage_key", MediusUsage, [u8])
_decl("medius_usage_media", MediusUsage, [u16])
_decl("medius_motion_cursor", MediusMotion, [i16, i16])
_decl("medius_motion_wheel", MediusMotion, [i16])
_decl("medius_lock_target_axis", MediusLockTarget, [u8])
_decl("medius_lock_target_usage", MediusLockTarget, [MediusUsage])
_decl("medius_locks_is_locked", c_bool, [ctypes.POINTER(MediusLocks), MediusLockTarget, u8])
_decl("medius_locks_scale_of", u8, [ctypes.POINTER(MediusLocks), MediusLockTarget, u8])
_decl("medius_rate_native_hz", c_bool, [MediusRate, ctypes.POINTER(ctypes.c_float)])
_decl("medius_usage_event_is_held", c_bool, [ctypes.POINTER(MediusUsageEvent), MediusUsage])
_decl("medius_catch_filter_watch", MediusCatchFilter, [MediusUsage])
_decl("medius_catch_filter_watch_axis", MediusCatchFilter, [u8])
_decl("medius_catch_filter_watch_class", MediusCatchFilter, [u8])
_decl("medius_catch_filter_watch_axes", MediusCatchFilter, [])
_decl("medius_catch_filter_all_input", None, [ctypes.POINTER(MediusCatchFilter)])
_decl("medius_catch_filter_traffic", MediusCatchFilter, [u8, u16])
_decl("medius_catch_filter_traffic_class", MediusCatchFilter, [u8])
_decl("medius_catch_filter_everything", MediusCatchFilter, [])
_decl("medius_catch_filter_with_direction", MediusCatchFilter, [MediusCatchFilter, u8])
_decl("medius_catch_filter_with_capture", MediusCatchFilter, [MediusCatchFilter, u8])
_decl("medius_catch_filter_on_press", MediusCatchFilter, [MediusCatchFilter])
_decl("medius_catch_filter_on_release", MediusCatchFilter, [MediusCatchFilter])
_decl("medius_catch_filter_inbound", MediusCatchFilter, [MediusCatchFilter])
_decl("medius_catch_filter_outbound", MediusCatchFilter, [MediusCatchFilter])
_decl("medius_catch_filter_same_address", c_bool, [MediusCatchFilter, MediusCatchFilter])
_decl("medius_catch_class_is_input", c_bool, [u8])
_decl("medius_catch_class_is_traffic", c_bool, [u8])
_decl("medius_traffic_event_truncated", c_bool, [ctypes.POINTER(MediusTrafficEvent)])
_decl("medius_traffic_event_setup", ctypes.POINTER(u8), [ctypes.POINTER(MediusTrafficEvent)])
_decl(
    "medius_traffic_event_data",
    ctypes.POINTER(u8),
    [ctypes.POINTER(MediusTrafficEvent), ctypes.POINTER(usize)],
)
_decl(
    "medius_traffic_event_control_status",
    c_bool,
    [ctypes.POINTER(MediusTrafficEvent), ctypes.POINTER(u8)],
)
_decl(
    "medius_traffic_event_bus_event",
    c_bool,
    [ctypes.POINTER(MediusTrafficEvent), ctypes.POINTER(MediusBusEvent)],
)
_decl("medius_traffic_event_bulk_end_of_transfer", c_bool, [ctypes.POINTER(MediusTrafficEvent)])
_decl("medius_traffic_event_bulk_zlp", c_bool, [ctypes.POINTER(MediusTrafficEvent)])
_decl("medius_clip_status_is_held", c_bool, [ctypes.POINTER(MediusClipStatus), MediusUsage])
_decl("medius_caps_has_mouse", c_bool, [MediusCaps])
_decl("medius_caps_has_keyboard", c_bool, [MediusCaps])
_decl("medius_caps_is_composite", c_bool, [MediusCaps])

_decl(
    "medius_device_catch_events",
    i32,
    [HANDLE, ctypes.POINTER(MediusCatchFilter), usize, PHANDLE],
)
_decl("medius_event_stream_clone", HANDLE, [HANDLE])
_decl("medius_event_stream_free", None, [HANDLE])
_decl("medius_event_stream_recv", i32, [HANDLE, ctypes.POINTER(MediusCatchEvent)])
_decl("medius_event_stream_try_recv", c_bool, [HANDLE, ctypes.POINTER(MediusCatchEvent)])
_decl("medius_event_stream_recv_timeout", c_bool, [HANDLE, u64, ctypes.POINTER(MediusCatchEvent)])
_decl("medius_event_stream_dropped", u64, [HANDLE])
_decl("medius_event_stream_is_connected", c_bool, [HANDLE])
_decl(
    "medius_device_input_events",
    i32,
    [HANDLE, ctypes.POINTER(MediusCatchFilter), usize, PHANDLE],
)
_decl("medius_input_stream_free", None, [HANDLE])
_decl("medius_input_stream_recv", i32, [HANDLE, ctypes.POINTER(MediusInputEvent)])
_decl("medius_input_stream_try_recv", c_bool, [HANDLE, ctypes.POINTER(MediusInputEvent)])
_decl("medius_input_stream_recv_timeout", c_bool, [HANDLE, u64, ctypes.POINTER(MediusInputEvent)])
_decl("medius_input_stream_dropped", u64, [HANDLE])
_decl("medius_input_stream_is_connected", c_bool, [HANDLE])
_decl(
    "medius_input_stream_held",
    usize,
    [HANDLE, u8, ctypes.POINTER(MediusUsage), usize],
)
_decl("medius_timeline_new", HANDLE, [])
_decl("medius_timeline_free", None, [HANDLE])
_decl(
    "medius_timeline_observe",
    c_bool,
    [HANDLE, ctypes.POINTER(MediusCatchEvent), u64, ctypes.POINTER(MediusStamped)],
)
_decl(
    "medius_timeline_observe_input",
    c_bool,
    [HANDLE, ctypes.POINTER(MediusInputEvent), u64, ctypes.POINTER(MediusStamped)],
)
_decl("medius_timeline_reset", None, [HANDLE, u8])
_decl("medius_timeline_samples", u64, [HANDLE, u8])
_decl("medius_device_logs", i32, [HANDLE, PHANDLE])
_decl("medius_log_stream_clone", HANDLE, [HANDLE])
_decl("medius_log_stream_free", None, [HANDLE])
_decl("medius_log_stream_recv", i32, [HANDLE, ctypes.POINTER(MediusLogLine)])
_decl("medius_log_stream_try_recv", c_bool, [HANDLE, ctypes.POINTER(MediusLogLine)])
_decl("medius_log_stream_recv_timeout", c_bool, [HANDLE, u64, ctypes.POINTER(MediusLogLine)])

_decl("medius_clip_builder_new", HANDLE, [])
_decl("medius_clip_builder_free", None, [HANDLE])
_decl("medius_clip_builder_clear", i32, [HANDLE])
_decl("medius_clip_builder_gap", i32, [HANDLE, u16])
_decl("medius_clip_builder_move", i32, [HANDLE, i16, i16])
_decl("medius_clip_builder_wheel", i32, [HANDLE, i16])
_decl("medius_clip_builder_press", i32, [HANDLE, MediusUsage])
_decl("medius_clip_builder_release", i32, [HANDLE, MediusUsage])
_decl("medius_clip_builder_force_release", i32, [HANDLE, MediusUsage])
_decl("medius_clip_builder_edge", i32, [HANDLE, MediusUsage, u8])
_decl("medius_clip_builder_frame", i32, [HANDLE, i16, i16, i16, ctypes.POINTER(MediusUsage), ctypes.POINTER(u8), usize])
_decl("medius_device_clip", i32, [HANDLE, PHANDLE])
_decl("medius_clip_free", None, [HANDLE])
_decl("medius_clip_append", i32, [HANDLE, HANDLE])
_decl("medius_clip_set_autolock", i32, [HANDLE, ctypes.POINTER(u8), usize])
_decl("medius_clip_set_loop", i32, [HANDLE, u8])
_decl("medius_clip_set_retain", i32, [HANDLE, u8])
_decl("medius_clip_set_ride", i32, [HANDLE, u8])
_decl("medius_clip_bind", i32, [HANDLE, MediusClipTrigger])
_decl("medius_clip_unbind", i32, [HANDLE, MediusUsage, u8])
_decl("medius_clip_clear_triggers", i32, [HANDLE])
_decl("medius_clip_start", i32, [HANDLE])
_decl("medius_clip_stop", i32, [HANDLE])
_decl("medius_clip_pause", i32, [HANDLE])
_decl("medius_clip_resume", i32, [HANDLE])
_decl("medius_clip_restart", i32, [HANDLE])
_decl("medius_clip_toggle", i32, [HANDLE])
_decl("medius_clip_clear", i32, [HANDLE])
_decl("medius_clip_finalize", i32, [HANDLE])
_decl("medius_clip_query_status", i32, [HANDLE, ctypes.POINTER(MediusClipStatus)])
_decl("medius_clip_query_config", i32, [HANDLE, ctypes.POINTER(MediusClipSettings)])

HAS_FLASH = _decl("medius_flash", i32, [ctypes.c_char_p, ctypes.c_char_p, c_bool], optional=True) is not None

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
    _decl("medius_mock_set_bearing", None, [HANDLE, u16, u8])
    _decl("medius_mock_set_emit_pace", None, [HANDLE, u8, u16])
    _decl("medius_mock_set_clip_status", None, [HANDLE, MediusClipStatus])
    _decl("medius_mock_set_clip_settings", None, [HANDLE, MediusClipSettings])
    _decl("medius_mock_silent", None, [HANDLE])
    _decl("medius_mock_push_raw", None, [HANDLE, ctypes.POINTER(u8), usize])
    _decl("medius_mock_push_log", None, [HANDLE, u8, ctypes.c_char_p])
    _decl("medius_mock_push_motion", None, [HANDLE, u8, u32, MediusMotionEvent])
    _decl("medius_mock_push_usages", None, [HANDLE, u8, u32, ctypes.POINTER(MediusUsageEvent)])
    _decl(
        "medius_mock_push_traffic",
        None,
        [HANDLE, u8, u32, u8, ctypes.POINTER(MediusTrafficEvent)],
    )
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
