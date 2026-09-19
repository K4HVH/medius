"""Mock-backed tests for the Python bindings."""

import gc
import pathlib
import subprocess
import tempfile
import time

import pytest

import medius
from medius import (
    RenderMode,
    RenderStatus,
    SpreadStatus,
    Axis,
    BadProtoVerError,
    BEARING_WINDOW_DEFAULT_MS,
    Bearing,
    BearingMode,
    BusEventKind,
    Button,
    Caps,
    Capture,
    CatchClass,
    CatchEntry,
    Class,
    CatchEventKind,
    CatchFilter,
    CatchState,
    Action,
    Blanket,
    CLIP_EDGES_MAX,
    CLIP_ENTRY_MAX,
    CLIP_PKT_MATCH_POOL,
    CLIP_PKT_TRIG_MAX,
    CLIP_RAW_MAX,
    ClipAction,
    ClipBuilder,
    ClipFrameCountError,
    ClipFrameTooLongError,
    ClipPacketTrigger,
    ClipPacketTriggerError,
    ClipSettings,
    ClipState,
    ClipStatus,
    ClipTransferDataError,
    ClipTrigger,
    ClockDomain,
    ClockEstimate,
    ControlStatus,
    Edge,
    LedMode,
    LedTarget,
    LOCK_SCALE_BLOCK,
    LOCK_SCALE_MAX,
    LOCK_SCALE_MIN,
    LOCK_SCALE_PASS,
    RebootTarget,
    Timeline,
    TrafficClass,
    Usage,
    Device,
    EmitPace,
    FrameType,
    Health,
    ImperfectStatus,
    InputKind,
    InvalidArgError,
    KbdCaps,
    Key,
    Direction,
    LockEntry,
    Locks,
    LockTarget,
    LockTargetKind,
    DeviceInfo,
    DeviceKind,
    LogLevel,
    MediaKey,
    MediusError,
    MockBox,
    Motion,
    MotionEvent,
    MoveTiming,
    PendingMotion,
    MouseCaps,
    Rate,
    Stats,
    Status,
    TrafficEvent,
    UsageSnapshot,
    Version,
    Patch,
    PatchEntry,
    PatchSet,
    PatchSection,
    PKT_MATCH_MAX,
    RewriteRule,
    RewriteEntry,
    RewriteTable,
    RewriteClass,
    RewriteAction,
    RewriteMatchTooLongError,
    Setup,
    TransferOutcome,
    TransferStatus,
    Transform,
    Transforms,
    TransformOp,
    ImperfectRequiredError,
    RawDirectionError,
    RelativeDirectionError,
    RewriteMaskLengthError,
    RewriteActionClassError,
    RewritePayloadTooLargeError,
    TransformOpFieldsError,
    TransformTableFullError,
)


def test_mock_feature_present():
    assert medius.HAS_MOCK, "tests need a mock-enabled libmedius_capi"


def test_meta_functions():
    # These are a hand-written mirror of the C structs, so a bumped ABI means they are stale until
    # someone re-reads the header. Pin it rather than accept anything newer.
    assert medius.abi_version() == 7
    assert medius.version_string()
    assert medius.default_query_timeout_ms() > 0
    assert medius.default_keepalive_cadence_ms() > 0


def test_configure_version_then_open_mock_matches():
    mock = MockBox()
    # The handshake checks proto_ver, so reuse the default proto and only change
    # the firmware triple.
    with Device.with_mock(mock) as d:
        proto = d.query_version().proto_ver
    mac = bytes([0x5A, 0x4E, 0x00, 0x11, 0x1E, 0x28])
    mock.set_version(Version(proto, 9, 8, 7, mac, "Left PC"))
    with mock.open() as d:
        v = d.query_version()
    assert v == Version(proto, 9, 8, 7, mac, "Left PC")
    assert v.mac_hex == "5a4e00111e28"
    assert v.name == "Left PC"  # the name rides the version readback beside the MAC
    mock.close()


def test_set_name_and_clear_name():
    mock = MockBox()
    with mock.open() as d:
        # No host-side validation (like the other setters): the box sanitises. These all just send.
        d.set_name("Left PC")
        d.set_name("x" * 40)  # over-length: the firmware caps it, the host does not error
        d.clear_name()
    mock.close()


def test_silent_mock_raises_on_open():
    mock = MockBox()
    mock.silent()
    with pytest.raises(MediusError):
        Device.open_mock(mock)
    mock.close()


def test_bad_proto_version_reports_status_and_proto_ver():
    mock = MockBox()
    mock.set_version(Version(99, 1, 0, 0))
    with pytest.raises(BadProtoVerError) as ei:
        Device.open_mock(mock)
    assert ei.value.status == Status.ERR_BAD_PROTO_VER
    assert ei.value.proto_ver == 99
    mock.close()


def test_recorded_frame_payload_readable():
    with MockBox() as mock, Device.with_mock(mock) as d:
        d.move_rel(1, 2)
        frame = mock.recorded_frame(0)
        assert frame is not None
        assert frame.type == FrameType.MOVE
        assert len(frame.payload) > 0
        assert mock.recorded_frame(99) is None


def test_move_riding_override_frames_carry_their_flags():
    with MockBox() as mock, Device.with_mock(mock) as d:
        d.move_rel(7, -2)
        d.move_rel_now(7, -2)
        d.wheel_now(3)
        d.flush_motion()
        d.discard_motion()
        d.move_axis(Motion.cursor(5, 5), MoveTiming.NOW, PendingMotion.FLUSH)
        sent = [mock.recorded_frame(i) for i in range(6)]
    payloads = [bytes(f.payload) for f in sent]
    assert all(f.type == FrameType.MOVE for f in sent)
    assert payloads == [
        bytes([0, 7, 0, 0xFE, 0xFF, 0x00]),
        bytes([0, 7, 0, 0xFE, 0xFF, 0x01]),
        bytes([1, 3, 0, 0x01]),
        bytes([0, 0, 0, 0, 0, 0x02]),
        bytes([0, 0, 0, 0, 0, 0x04]),
        bytes([0, 5, 0, 5, 0, 0x03]),
    ]


def test_caps_roundtrip():
    caps = Caps(
        mouse=MouseCaps(n_buttons=5, has_x=True, has_y=True, has_wheel=True, pan=True, has_report_id=False, n_hid=2),
        keyboard=KbdCaps(n_keys=6, nkro=False, has_consumer=True, has_system=False, has_report_id=True),
        mouse_change_driven=False,
        kbd_change_driven=True,
    )
    with MockBox() as mock:
        mock.set_caps(caps)
        with Device.with_mock(mock) as d:
            got = d.caps()
    assert got == caps
    assert got.has_mouse()
    assert got.has_keyboard()
    assert got.is_composite()


def test_rate_roundtrip_and_native_hz():
    # change_driven is not on the RATE wire payload, so it is excluded.
    rate = Rate(native_period_us=1000, poll_period_us=1000, confident=True, change_driven=False)
    with MockBox() as mock:
        mock.set_rate(rate)
        with Device.with_mock(mock) as d:
            got = d.query_rate()
    assert got.native_period_us == 1000
    assert got.poll_period_us == 1000
    assert got.confident is True
    assert abs(got.native_hz() - 1000.0) < 0.5

    off = Rate(native_period_us=0, poll_period_us=1000, confident=False, change_driven=True)
    assert off.native_hz() is None


def test_locks_roundtrip_and_is_locked():
    x = LockTarget.x()
    with MockBox() as mock:
        mock.set_locks(
            Locks(
                [
                    LockEntry(x, is_blanket=False, direction=Direction.POSITIVE, scale=0),
                    LockEntry(x, is_blanket=False, direction=Direction.NEGATIVE, scale=0),
                ]
            )
        )
        with Device.with_mock(mock) as d:
            locks = d.query_locks()
    assert len(locks.entries) == 2
    assert locks.is_locked(x, Direction.BOTH)
    assert not locks.is_locked(LockTarget.y(), Direction.BOTH)


def test_locks_carry_a_scale_not_just_a_lock():
    x = LockTarget.x()
    with MockBox() as mock:
        mock.set_locks(
            Locks(
                [
                    LockEntry(x, is_blanket=False, direction=Direction.AGAINST, scale=40),
                    LockEntry(x, is_blanket=False, direction=Direction.WITH, scale=130),
                ]
            )
        )
        with Device.with_mock(mock) as d:
            locks = d.query_locks()
    assert locks.scale_of(x, Direction.AGAINST) == 40
    assert locks.scale_of(x, Direction.WITH) == 130
    # A direction nothing covers passes untouched, and a weighed one is not a locked one.
    assert locks.scale_of(x, Direction.POSITIVE) == LOCK_SCALE_PASS
    assert not locks.is_locked(x, Direction.AGAINST)
    assert not locks.entries[0].is_block


def test_scale_and_bearing_reach_the_device():
    x = LockTarget.x()
    with MockBox() as mock, Device.with_mock(mock) as d:
        d.scale(x, Direction.AGAINST, 40)
        d.scale_all(Blanket.AIM, Direction.WITH, 130)
        d.lock(x, Direction.POSITIVE)
        d.unlock(x, Direction.POSITIVE)
        d.set_bearing(35, BearingMode.VECTOR)
        d.set_bearing(None, BearingMode.PER_AXIS)
        locks = _clip_frames(d, mock, FrameType.LOCK)
        options = _clip_frames(d, mock, FrameType.OPTION)
    # The payload bytes, from ctrl_proto.h: LOCK is [class][id u16 LE][direction][scale i16 LE] and
    # OPTION(BEARING) is [4][window u16 LE][mode]. A status of OK proves only that the call returned.
    assert locks == [
        bytes([3, 0, 0, 4, 40, 0]),    # AXIS X, AGAINST, 40%
        bytes([3, 0, 0, 3, 130, 0]),   # AXIS X, WITH, 130%  (Blanket.AIM is X then Y)
        bytes([3, 1, 0, 3, 130, 0]),   # AXIS Y, WITH, 130%
        bytes([3, 0, 0, 1, 0, 0]),     # AXIS X, POSITIVE, block
        bytes([3, 0, 0, 1, 100, 0]),   # AXIS X, POSITIVE, pass
    ]
    assert options == [bytes([4, 35, 0, 1]), bytes([4, 0, 0, 0])]


def test_the_box_lock_table_answers_query_locks():
    x, y = LockTarget.x(), LockTarget.y()
    with MockBox() as mock, Device.with_mock(mock) as d:
        d.scale(x, Direction.BOTH, 50)
        locks = d.query_locks()
        # BOTH is the fixed pair only: the relative pair keeps passing, so a BOTH of 50 means 50%
        # whether or not a bearing is live.
        assert len(locks.entries) == 2
        assert locks.scale_of(x, Direction.POSITIVE) == 50
        assert locks.scale_of(x, Direction.NEGATIVE) == 50
        assert locks.scale_of(x, Direction.WITH) == LOCK_SCALE_PASS
        assert locks.scale_of(x, Direction.AGAINST) == LOCK_SCALE_PASS

        # In vector mode one relative scale governs both axes, the lower of the two, and the box
        # reports that number on both axes rather than each axis's stored byte.
        d.unlock(x, Direction.BOTH)
        d.scale(x, Direction.WITH, 130)
        d.scale(y, Direction.WITH, 60)
        d.set_bearing(20, BearingMode.PER_AXIS)
        locks = d.query_locks()
        assert (locks.scale_of(x, Direction.WITH), locks.scale_of(y, Direction.WITH)) == (130, 60)
        d.set_bearing(20, BearingMode.VECTOR)
        locks = d.query_locks()
        assert (locks.scale_of(x, Direction.WITH), locks.scale_of(y, Direction.WITH)) == (60, 60)


def test_a_key_blanket_reports_the_edges_it_blocks():
    with MockBox() as mock, Device.with_mock(mock) as d:
        d.lock_all(Blanket.KEYS, Direction.POSITIVE)
        entries = d.query_locks().entries
        assert [(e.is_blanket, e.direction, e.scale) for e in entries] == [
            (True, Direction.POSITIVE, LOCK_SCALE_BLOCK)
        ]
        # Both edges are two entries, never one BOTH the box is not holding.
        d.lock_all(Blanket.KEYS, Direction.NEGATIVE)
        assert [e.direction for e in d.query_locks().entries] == [
            Direction.POSITIVE,
            Direction.NEGATIVE,
        ]


def test_a_one_bit_class_stores_the_block_or_pass_it_amounts_to():
    left = LockTarget.usage(Usage.button(Button.LEFT))
    with MockBox() as mock, Device.with_mock(mock) as d:
        d.scale(left, Direction.POSITIVE, 50)
        # Under a full pass a button truncates to a block, so that is what reads back.
        assert d.query_locks().scale_of(left, Direction.POSITIVE) == LOCK_SCALE_BLOCK
        # 150% is an amplification one bit cannot carry, so the box truncates it to a pass.
        d.scale(left, Direction.POSITIVE, 150)
        assert d.query_locks().entries == []


def test_a_media_lock_is_sent_and_reported_as_both():
    mute = LockTarget.usage(Usage.media(0xE2))
    with MockBox() as mock, Device.with_mock(mock) as d:
        d.lock(mute, Direction.PRESS)
        sent = _clip_frames(d, mock, FrameType.LOCK)
        entries = d.query_locks().entries
    # A media usage has no edges: it is suppressed whole, so the frame carries BOTH rather than an
    # edge the readback would disagree with.
    assert sent == [bytes([2, 0xE2, 0x00, 0, 0, 0])]
    assert [e.direction for e in entries] == [Direction.BOTH]


def test_a_relative_direction_needs_a_bearing_and_only_an_axis_has_one():
    with MockBox() as mock, Device.with_mock(mock) as d:
        for direction in (Direction.WITH, Direction.AGAINST):
            with pytest.raises(medius.RelativeDirectionError):
                d.lock(LockTarget.usage(Usage.button(Button.LEFT)), direction)
            with pytest.raises(medius.RelativeDirectionError):
                d.scale(LockTarget.usage(Usage.key(Key.A)), direction, 40)
            with pytest.raises(medius.RelativeDirectionError):
                d.lock_all(Blanket.KEYS, direction)
            with pytest.raises(medius.RelativeDirectionError):
                d.lock_all(Blanket.MEDIA, direction)
            d.scale(LockTarget.x(), direction, 130)
        # Nothing refused reached the wire; only the two axis scales did.
        assert len(_clip_frames(d, mock, FrameType.LOCK)) == 2


def test_scalars_are_checked_before_ctypes_truncates_them():
    x = LockTarget.x()
    with MockBox() as mock, Device.with_mock(mock) as d:
        # The scale is an i16, so 40000 would arrive as -25536 and wrap the reversal the sign means.
        for bad in (40_000, -40_000, 32_768):
            with pytest.raises(ValueError):
                d.scale(x, Direction.AGAINST, bad)
            with pytest.raises(ValueError):
                d.scale_all(Blanket.AIM, Direction.AGAINST, bad)
        # A direction byte outside the enum reaches an exhaustive match on the Rust side.
        with pytest.raises(ValueError):
            d.scale(x, 40, 50)
        with pytest.raises(ValueError):
            d.lock_all(99, Direction.BOTH)
        with pytest.raises(ValueError):
            d.set_bearing(20, 7)
        # The render mode crosses the ABI as a byte, so an out-of-range one must be refused here
        # rather than reaching the Rust enum.
        with pytest.raises(ValueError):
            d.set_render(4, False)
        with pytest.raises(ValueError):
            d.set_bearing(-1, BearingMode.PER_AXIS)
        # The window saturates rather than wrapping, as the Rust API does: 70000 ms would arrive as
        # 4464 through a u16.
        d.set_bearing(70_000, BearingMode.VECTOR)
        assert _clip_frames(d, mock, FrameType.OPTION) == [bytes([4, 0xFF, 0xFF, 1])]
        # Nothing that raised reached the wire.
        assert _clip_frames(d, mock, FrameType.LOCK) == []


def test_mock_set_locks_checks_each_entry_before_ctypes_truncates_it():
    x = LockTarget.x()
    with MockBox() as mock:
        with pytest.raises(ValueError):
            mock.set_locks(Locks([LockEntry(x, is_blanket=False, direction=40, scale=0)]))
        for bad in (40_000, -40_000):
            with pytest.raises(ValueError):
                mock.set_locks(
                    Locks([LockEntry(x, is_blanket=False, direction=Direction.POSITIVE, scale=bad)])
                )
        # The same direction byte reaches the two readers, which take it as a plain u8 as well.
        good = Locks([LockEntry(x, is_blanket=False, direction=Direction.POSITIVE, scale=0)])
        with pytest.raises(ValueError):
            good.is_locked(x, 40)
        with pytest.raises(ValueError):
            good.scale_of(x, 40)
        # and the named values still work
        mock.set_locks(good)
        with Device.with_mock(mock) as d:
            assert d.query_locks().is_locked(x, Direction.POSITIVE)


def test_catch_filter_scalars_are_checked_before_ctypes_truncates_them():
    f = CatchFilter.watch(Usage.button(Button.LEFT))
    # A direction byte no constant names would ride the filter to the box, where the subscription is
    # refused: a stream that never yields, reported nowhere near the mistake.
    with pytest.raises(ValueError):
        f.with_direction(40)
    with pytest.raises(ValueError):
        f.with_direction(-1)
    for bad in (300, -1, 256):
        with pytest.raises(ValueError):
            f.with_capture(bad)
    with pytest.raises(ValueError):
        CatchFilter.watch_axis(9)
    with pytest.raises(ValueError):
        CatchFilter.watch_class(9)
    with pytest.raises(ValueError):
        CatchFilter.traffic_class(99)
    with pytest.raises(ValueError):
        CatchFilter.traffic(TrafficClass.HID_IN, 0x1_0000)
    with pytest.raises(ValueError):
        CatchFilter.traffic(TrafficClass.HID_IN, -1)
    # and the named values still build the filter they always did
    assert f.with_direction(Direction.PRESS).direction == Direction.POSITIVE
    assert f.with_capture(16).capture == 16


def test_an_unnamed_direction_byte_never_reaches_a_subscription():
    # The C struct field is a plain byte, so a filter built past the Python guard still has to be
    # refused at subscribe time rather than narrowing the stream to nothing.
    with MockBox() as mock, Device.with_mock(mock) as d:
        f = CatchFilter.watch(Usage.button(Button.LEFT))
        f._c.direction = 40
        with pytest.raises(InvalidArgError):
            d.catch_events(f)
        with pytest.raises(InvalidArgError):
            d.input_events(f)
        # A named direction on the same filter subscribes.
        with d.catch_events(f.with_direction(Direction.PRESS)) as s:
            assert s is not None


def test_set_bearing_requires_the_mode():
    # Both fields ride one frame and the box persists them together, so a Python-only default would
    # revert a box configured for VECTOR on any window change.
    with MockBox() as mock, Device.with_mock(mock) as d:
        with pytest.raises(TypeError):
            d.set_bearing(50)


def test_mock_set_bearing_answers_a_vector_bearing():
    with MockBox() as mock, Device.with_mock(mock) as d:
        # The default is what a real box boots holding.
        assert d.query_bearing() == Bearing(BEARING_WINDOW_DEFAULT_MS, BearingMode.PER_AXIS)
        mock.set_bearing(Bearing(250, BearingMode.VECTOR))
        assert d.query_bearing() == Bearing(250, BearingMode.VECTOR)
        mock.set_bearing(Bearing(None, BearingMode.PER_AXIS))
        got = d.query_bearing()
        assert got.window_ms is None and not got.is_live


def test_relative_directions_report_themselves():
    assert Direction.WITH.is_relative and Direction.AGAINST.is_relative
    assert not Direction.BOTH.is_relative
    assert not Direction.POSITIVE.is_relative and not Direction.NEGATIVE.is_relative
    assert (int(Direction.WITH), int(Direction.AGAINST)) == (3, 4)
    assert (LOCK_SCALE_BLOCK, LOCK_SCALE_PASS, LOCK_SCALE_MAX, LOCK_SCALE_MIN) == (
        0,
        100,
        255,
        -255,
    )


def test_health_roundtrip():
    health = Health(
        link_up=True,
        mouse_attached=False,
        clone_configured=True,
        injection_active=False,
        rate_confident=True,
        lock_on=False,
        catch_on=True,
        kbd_attached=False,
    )
    with MockBox() as mock:
        mock.set_health(health)
        with Device.with_mock(mock) as d:
            got = d.query_health()
    assert got == health


def test_device_info_roundtrip():
    info = DeviceInfo(
        vid=0x046D,
        pid=0xC08B,
        bcd_device=0x0111,
        bcd_usb=0x0200,
        has_serial=True,
        has_bos=False,
        kind=DeviceKind.MOUSE,
        product="Logitech G502",
    )
    with MockBox() as mock:
        mock.set_device_info(info)
        with Device.with_mock(mock) as d:
            got = d.device_info()
    assert got == info
    assert got.kind == DeviceKind.MOUSE
    assert got.product == "Logitech G502"


def test_stats_roundtrip():
    stats = Stats(
        inject_emits=123456,
        tx_drops=12,
        tx_merges=34,
        tx_maxdepth=7,
        tx_wedges=2,
        wakeups=900,
        reset_count=3,
        config_count=4,
    )
    with MockBox() as mock:
        mock.set_stats(stats)
        with Device.with_mock(mock) as d:
            got = d.query_stats()
    assert got == stats


def _query_catch(state: CatchState) -> CatchState:
    with MockBox() as mock:
        mock.set_catch_state(state)
        with Device.with_mock(mock) as d:
            return d.query_catch()


def test_catch_state_roundtrip():
    state = CatchState(
        table_full=True,
        dropped=42,
        clock=ClockEstimate(offset_us=-1234, rate_ppb=57, delay_us=90, age_ms=1500),
        entries=[
            CatchEntry(CatchFilter.everything().with_capture(16), dropped=3),
            CatchEntry(
                CatchFilter.traffic(TrafficClass.VENDOR_BULK, 3).with_direction(
                    Direction.POSITIVE
                ),
                dropped=7,
            ),
        ],
    )
    got = _query_catch(state)
    assert got == state
    assert got.clock.error_bound_us == 45
    assert [e.dropped for e in got.entries] == [3, 7]
    assert got.entries[1].filter.catch_class == CatchClass.VENDOR_BULK
    assert got.entries[1].filter.id == 3


def test_catch_state_clock_age_none_is_not_a_zero_age():
    # An offset that was never measured also reads as zero, so the sentinel has to survive the
    # round trip: applying an unmeasured offset would silently shift every cross-domain stamp.
    never = _query_catch(CatchState(clock=ClockEstimate(offset_us=500, age_ms=None)))
    fresh = _query_catch(CatchState(clock=ClockEstimate(offset_us=500, age_ms=0)))
    assert never.clock.age_ms is None
    assert fresh.clock.age_ms == 0
    assert never.clock != fresh.clock
    assert never.clock.to_host_domain(1_000) is None
    assert fresh.clock.to_host_domain(1_000) == 1_500


def test_catch_filter_wildcards_survive_the_roundtrip():
    every = CatchFilter.everything()
    blanket = CatchFilter.traffic_class(TrafficClass.HID_IN)
    exact = CatchFilter.traffic(TrafficClass.CONTROL, 0)
    assert (every.catch_class, every.id) == (None, None)
    assert (blanket.catch_class, blanket.id) == (CatchClass.HID_IN, None)
    assert (exact.catch_class, exact.id) == (CatchClass.CONTROL, 0)

    got = _query_catch(CatchState(entries=[CatchEntry(f) for f in (every, blanket, exact)]))
    assert [e.filter for e in got.entries] == [every, blanket, exact]
    # An id of 0 is a real address, not the every-id wildcard.
    assert got.entries[2].filter.id == 0
    assert got.entries[0].filter.id is None


def test_imperfect_roundtrip():
    status = ImperfectStatus(allowed=True, over_capacity=True, clone_imperfect=False)
    with MockBox() as mock:
        mock.set_imperfect_status(status)
        with Device.with_mock(mock) as d:
            got = d.query_imperfect()
    assert got == status


def test_movement_riding_roundtrip():
    with MockBox() as mock:
        mock.set_movement_riding(8)
        with Device.with_mock(mock) as d:
            assert d.query_movement_riding() == 8
        mock.set_movement_riding(None)
        with Device.with_mock(mock) as d:
            assert d.query_movement_riding() is None


def test_emit_pace_roundtrip():
    with MockBox() as mock:
        # The box's factory texture is De-spiked, but nothing is rendered until a profile arms, so a
        # Learned pace reports the learnt cap rather than the 1 ms tick.
        mock.set_emit_pace(EmitPace.fixed(500))
        with Device.with_mock(mock) as d:
            status = d.query_emit_pace()
        assert status.mode == EmitPace.fixed(500)
        assert status.resolved_hz == 500  # Fixed clamps to its hz
        mock.set_emit_pace(EmitPace.learned())
        with Device.with_mock(mock) as d:
            status = d.query_emit_pace()
        assert status.mode == EmitPace.learned()
        assert status.resolved_hz == 0
        # Armed, the rendered stream self-paces every millisecond and says so.
        mock.set_render(RenderMode.DESPIKED, False, True)
        with Device.with_mock(mock) as d:
            assert d.query_emit_pace().resolved_hz == 1000
        # And disarming puts it back: without reading it again, `ready` could be latched on and the
        # assertion above would pass on a mock that ignored the flag.
        mock.set_render(RenderMode.DESPIKED, False, False)
        with Device.with_mock(mock) as d:
            assert d.query_emit_pace().resolved_hz == 0
        assert status.force_hz is None
        assert status.force_active is False
        assert status.advertised_hz == 0  # 0 = no clone, the documented sentinel


def test_spread_roundtrip():
    with MockBox() as mock, Device.with_mock(mock) as d:
        # A fresh box boots at the full percent with no command period learned, so it spreads nothing.
        assert d.query_spread() == SpreadStatus(100, 0)
        # The period is the box's own state, learned off MOVE arrivals, not something the host sets.
        mock.set_spread_learned(8000)
        assert d.query_spread() == SpreadStatus(100, 8000)
        d.set_spread(50)
        assert d.query_spread() == SpreadStatus(50, 4000)
        # Above 100 overlaps rather than being clamped, and past a byte, so a u8 anywhere in the
        # ctypes chain fails here rather than round-tripping.
        d.set_spread(1000)
        assert d.query_spread() == SpreadStatus(1000, 80000)
        # Off answers no interval even with a period learned.
        d.set_spread(0)
        assert d.query_spread() == SpreadStatus(0, 0)


def test_render_roundtrip():
    # The texture is its own command; the pace still reports the gate it implies.
    with MockBox() as mock, Device.with_mock(mock) as d:
        # The box's factory setting: de-spiked, native motion relayed, nothing learned yet.
        status = d.query_render()
        assert status.mode == RenderMode.DESPIKED
        assert status.full is False
        assert status.ready is False
        for mode in RenderMode:
            for full in (False, True):
                d.set_render(mode, full)
                status = d.query_render()
                assert (status.mode, status.full) == (mode, full)
        # The box gates the 1 ms tick on a profile having ARMED, not on the mode being set: a box told
        # to render but still waiting for one runs the paced fill and reports the learnt cap.
        d.set_render(RenderMode.STOCK, False)
        d.set_emit_pace(EmitPace.learned())
        assert d.query_emit_pace().resolved_hz == 0
        # Onto a Fixed rate the snapped value stands whatever the texture is doing.
        d.set_emit_pace(EmitPace.fixed(250))
        assert d.query_emit_pace().resolved_hz == 250
        d.set_render(RenderMode.OFF, False)
        d.set_emit_pace(EmitPace.learned())
        assert d.query_emit_pace().resolved_hz == 0

    # Armed, a rendered stream on a learnt pace self-paces every millisecond. This is the half that
    # discriminates: without it the reply reads the same whether the renderer is emitting or the box
    # is still on the fill.
    with MockBox() as mock, Device.with_mock(mock) as d:
        mock.set_render(RenderMode.STOCK, True, True)
        assert d.query_render() == RenderStatus(RenderMode.STOCK, True, True)
        d.set_emit_pace(EmitPace.learned())
        assert d.query_emit_pace().resolved_hz == 1000
        # full has no default: OPTION(RENDER) persists both fields, so an omitted one would silently
        # rewrite a setting the caller never named.
        with pytest.raises(TypeError):
            d.set_render(RenderMode.STOCK)


def test_rate_force_roundtrip():
    # The five numbers occupy five ctypes fields in one struct, so a layout slip shows as a swap.
    with MockBox() as mock:
        mock.set_advertised_hz(125)
        mock.set_imperfect_status(ImperfectStatus(allowed=True, over_capacity=False, clone_imperfect=False))
        # 400 resolves to bInterval 2, the interval a host would actually keep.
        mock.set_emit_pace(EmitPace.fixed(500), 400)
        with Device.with_mock(mock) as d:
            status = d.query_emit_pace()
        assert status.mode == EmitPace.fixed(500)
        assert status.resolved_hz == 500
        assert status.force_hz == 400
        assert status.advertised_hz == 500
        assert status.force_active is True
        mock.set_emit_pace(EmitPace.fixed(500), None)
        with Device.with_mock(mock) as d:
            status = d.query_emit_pace()
        assert status.force_hz is None
        assert status.force_active is False
        assert status.advertised_hz == 125  # back to what the clone declares


def test_rate_force_needs_the_imperfect_opt_in():
    # The box leaves a force inert without the opt-in, so a mock that applied it regardless would green
    # -light host code that disagrees with every real box.
    with MockBox() as mock:
        mock.set_advertised_hz(125)
        with Device.with_mock(mock) as d:
            d.set_emit_pace(EmitPace.learned(), force_hz=1000)
            status = d.query_emit_pace()
            assert status.force_hz == 1000
            assert status.force_active is False
            assert status.advertised_hz == 125
            d.allow_imperfect_clones(True)
            status = d.query_emit_pace()
            assert status.force_active is True
            assert status.advertised_hz == 1000


def test_rate_force_through_the_setter():
    # Drives OPTION(EMIT) out of the setter and reads the box's own state back, so a setter that drops
    # the force on the wire fails here rather than passing on a mock nobody sent anything to.
    with MockBox() as mock, Device.with_mock(mock) as d:
        d.allow_imperfect_clones(True)
        d.set_emit_pace(EmitPace.fixed(250), force_hz=125)
        status = d.query_emit_pace()
        assert status.mode == EmitPace.fixed(250)
        assert status.force_hz == 125
        assert status.advertised_hz == 125
        assert status.force_active is True
        d.set_emit_pace(EmitPace.fixed(250), force_hz=None)
        assert d.query_emit_pace().force_hz is None


def test_counters_readable():
    with MockBox() as mock, Device.with_mock(mock) as d:
        d.move_rel(1, 0)
        c = d.counters()
        assert c.frames_tx >= 1


def test_catch_delivers_motion_event():
    with MockBox() as mock, Device.with_mock(mock) as d:
        with d.catch_events(CatchFilter.everything()) as stream:
            mock.push_motion(1, 7_000, MotionEvent(dx=12, dy=-34, dz=1, pan=2))
            ev = stream.recv_timeout(2000)
            assert ev is not None
            assert ev.kind == CatchEventKind.MOTION
            assert ev.ts_us == 7_000
            assert ev.motion.dx == 12
            assert ev.motion.dy == -34
            assert ev.motion.dz == 1
            assert ev.motion.pan == 2


def test_catch_delivers_usage_event_for_a_key():
    with MockBox() as mock, Device.with_mock(mock) as d:
        with d.catch_events(CatchFilter.watch_class(Class.KEY)) as stream:
            mock.push_usages(1, 7_000, UsageSnapshot([Usage.key(Key.ESCAPE)]))
            ev = stream.recv_timeout(2000)
            assert ev is not None
            assert ev.kind == CatchEventKind.USAGES
            assert ev.usages.is_held(Usage.key(Key.ESCAPE))
            assert not ev.usages.is_held(Usage.key(Key.A))


def test_catch_delivers_usage_event_for_media():
    with MockBox() as mock, Device.with_mock(mock) as d:
        with d.catch_events(CatchFilter.everything()) as stream:
            mock.push_usages(1, 7_000, UsageSnapshot([Usage.media(MediaKey.VOLUME_UP)]))
            ev = stream.recv_timeout(2000)
            assert ev is not None
            assert ev.kind == CatchEventKind.USAGES
            assert ev.usages.is_held(Usage.media(MediaKey.VOLUME_UP))


def _push_and_recv(mock, stream, event: TrafficEvent, clock=ClockDomain.DEVICE_CHIP):
    mock.push_traffic(1, 7_000, clock, event)
    ev = stream.recv_timeout(2000)
    assert ev is not None and ev.kind == CatchEventKind.TRAFFIC
    return ev


def test_catch_delivers_traffic_event():
    sent = TrafficEvent(
        catch_class=CatchClass.HID_IN,
        id=2,
        direction=Direction.POSITIVE,
        flags=0,
        true_len=6,
        bytes=bytes([0x01, 0x02, 0x03, 0x04, 0x05, 0x06]),
    )
    with MockBox() as mock, Device.with_mock(mock) as d:
        with d.catch_events(CatchFilter.traffic_class(TrafficClass.HID_IN)) as stream:
            ev = _push_and_recv(mock, stream, sent, ClockDomain.HOST_CHIP)
    assert ev.ts_us == 7_000
    assert ev.clock == ClockDomain.HOST_CHIP  # the real device's bytes carry the host chip's stamp
    assert ev.motion is None and ev.usages is None
    assert ev.traffic == sent
    assert not ev.traffic.truncated()
    assert ev.traffic.data() == sent.bytes  # no setup stage outside CONTROL


def test_traffic_event_true_len_above_the_capture_is_truncation():
    # A cut capture and a genuinely short packet are the same bytes; only true_len separates
    # them, so it has to survive the wire.
    cut = TrafficEvent(
        catch_class=CatchClass.VENDOR_BULK,
        id=3,
        direction=Direction.POSITIVE,
        flags=0x03,
        true_len=512,
        bytes=bytes(range(16)),
    )
    with MockBox() as mock, Device.with_mock(mock) as d:
        with d.catch_events(CatchFilter.traffic(TrafficClass.VENDOR_BULK, 3).with_capture(16)) as s:
            ev = _push_and_recv(mock, s, cut)
    assert ev.traffic.true_len == 512
    assert len(ev.traffic.bytes) == 16
    assert ev.traffic.truncated()
    assert ev.traffic.bulk_end_of_transfer()
    assert ev.traffic.bulk_zlp()

    whole = TrafficEvent(CatchClass.VENDOR_BULK, 3, Direction.POSITIVE, 0, 16, bytes(16))
    assert not whole.truncated()


def test_traffic_event_control_accessors():
    setup = bytes([0x80, 0x06, 0x00, 0x01, 0x00, 0x00, 0x12, 0x00])
    stalled = TrafficEvent(
        catch_class=CatchClass.CONTROL,
        id=0,
        direction=Direction.POSITIVE,
        flags=0xFD,
        true_len=8,
        bytes=setup,
    )
    with MockBox() as mock, Device.with_mock(mock) as d:
        with d.catch_events(CatchFilter.traffic_class(TrafficClass.CONTROL)) as stream:
            ev = _push_and_recv(mock, stream, stalled)
    assert ev.traffic.setup() == setup
    assert ev.traffic.data() == b""  # a STALL answers with no data stage
    assert ev.traffic.control_status() == ControlStatus.STALLED
    assert ev.traffic.bus_event() is None

    answered = TrafficEvent(
        CatchClass.CONTROL, 0, Direction.POSITIVE, 0x00, 10, setup + b"\x12\x01"
    )
    assert answered.control_status() == ControlStatus.OK
    assert answered.data() == b"\x12\x01"


def test_clip_transfer_event_accessors():
    setup = bytes([0xA1, 0x01, 0x00, 0x03, 0x00, 0x00, 0x02, 0x00])
    answered = TrafficEvent(
        catch_class=CatchClass.CLIP_TRANSFER,
        id=0,
        direction=Direction.IN,
        flags=TransferStatus.OK,
        true_len=10,
        bytes=setup + b"\x04\x01",
    )
    with MockBox() as mock, Device.with_mock(mock) as d:
        with d.catch_events(CatchFilter.traffic_class(TrafficClass.CLIP_TRANSFER)) as stream:
            ev = _push_and_recv(mock, stream, answered)
    assert ev.traffic == answered
    assert ev.traffic.catch_class.is_traffic()
    assert ev.traffic.setup() == setup
    assert ev.traffic.data() == b"\x04\x01"
    assert ev.traffic.transfer_status() == TransferStatus.OK
    assert ev.traffic.transfer_status().is_ok
    # The flags byte is a transfer status on this class, so the control reading does not apply.
    assert ev.traffic.control_status() is None

    stalled = TrafficEvent(CatchClass.CLIP_TRANSFER, 0, Direction.IN, 0xFD, 8, setup)
    assert stalled.transfer_status() == TransferStatus.STALL
    assert stalled.data() == b""
    unanswered = TrafficEvent(CatchClass.CLIP_TRANSFER, 0, Direction.IN, 0xFE, 8, setup)
    assert unanswered.transfer_status() == TransferStatus.NAK
    # A status no member names is kept as its byte.
    unknown = TrafficEvent(CatchClass.CLIP_TRANSFER, 0, Direction.IN, 0x42, 8, setup)
    assert unknown.transfer_status() == 0x42
    # Any other class reads as None.
    control = TrafficEvent(CatchClass.CONTROL, 0, Direction.IN, 0x00, 8, setup)
    assert control.transfer_status() is None


def test_traffic_event_bus_event():
    with MockBox() as mock, Device.with_mock(mock) as d:
        with d.catch_events(CatchFilter.traffic_class(TrafficClass.BUS)) as stream:
            ev = _push_and_recv(
                mock,
                stream,
                TrafficEvent(CatchClass.BUS, 0, Direction.BOTH, 5, 2, bytes([3, 1])),
            )
    bus = ev.traffic.bus_event()
    assert bus.kind == BusEventKind.SET_INTERFACE
    assert (bus.interface, bus.alt) == (3, 1)
    assert ev.traffic.control_status() is None


def test_traffic_event_surfaces_on_every_receive_path():
    ev = TrafficEvent(CatchClass.EMIT, 1, Direction.POSITIVE, 0, 3, b"\x01\x02\x03")
    with MockBox() as mock, Device.with_mock(mock) as d:
        with d.catch_events(CatchFilter.traffic_class(TrafficClass.EMIT)) as stream:
            mock.push_traffic(1, 10, ClockDomain.DEVICE_CHIP, ev)
            blocking = stream.recv()

            mock.push_traffic(2, 20, ClockDomain.DEVICE_CHIP, ev)
            deadline = time.monotonic() + 2.0
            polled = None
            while polled is None and time.monotonic() < deadline:
                polled = stream.try_recv()

            mock.push_traffic(3, 30, ClockDomain.DEVICE_CHIP, ev)
            timed = stream.recv_timeout(2000)
    assert polled is not None and timed is not None
    for got in (blocking, polled, timed):
        assert got.kind == CatchEventKind.TRAFFIC
        assert got.traffic == ev


def test_catch_events_needs_at_least_one_filter():
    with MockBox() as mock, Device.with_mock(mock) as d:
        with pytest.raises(InvalidArgError):
            d.catch_events([])


def test_try_recv_returns_none_when_empty():
    with MockBox() as mock, Device.with_mock(mock) as d:
        with d.catch_events(CatchFilter.everything()) as stream:
            assert stream.try_recv() is None
            assert stream.dropped == 0


def test_log_stream_delivers_line():
    with MockBox() as mock, Device.with_mock(mock) as d:
        with d.logs() as logs:
            mock.push_log(LogLevel.WARN, "hello world")
            line = logs.recv_timeout(2000)
            assert line is not None
            assert line.level == LogLevel.WARN
            assert line.text == "hello world"


def test_clone_shares_state():
    with MockBox() as mock:
        d = Device.with_mock(mock)
        d2 = d.clone()
        d.move_rel(1, 0)
        d2.move_rel(2, 0)
        mock2 = mock.clone()
        assert mock2.recorded() == 2
        d.close()
        d2.close()
        mock2.close()


def test_event_stream_clone_shares_subscription():
    with MockBox() as mock, Device.with_mock(mock) as d:
        with d.catch_events(CatchFilter.everything()) as stream:
            stream2 = stream.clone()
            mock.push_motion(1, 7_000, MotionEvent(dx=9, dy=0, dz=0))
            ev = stream2.recv_timeout(2000)
            assert ev is not None and ev.kind == CatchEventKind.MOTION and ev.motion.dx == 9
            stream2.close()


def test_double_close_is_safe():
    mock = MockBox()
    d = Device.with_mock(mock)
    d.close()
    d.close()
    mock.close()
    mock.close()


def test_gc_frees_cleanly():
    mock = MockBox()
    d = Device.with_mock(mock)
    stream = d.catch_events(CatchFilter.everything())
    del stream
    del d
    del mock
    gc.collect()


def test_usage_snapshot_is_held_matches_any_class():
    # Buttons, keys, and modifiers live in one snapshot, keyed the same way.
    snap = UsageSnapshot(
        [Usage.button(Button.RIGHT), Usage.key(Key.LEFT_CTRL), Usage.key(Key.A)]
    )
    assert snap.is_held(Usage.button(Button.RIGHT))
    assert snap.is_held(Usage.key(Key.LEFT_CTRL))
    assert snap.is_held(Usage.key(Key.A))
    assert not snap.is_held(Usage.button(Button.LEFT))
    assert not snap.is_held(Usage.key(Key.B))


def _clip_frames(d, mock, ty):
    """The payloads of the recorded frames of a given FrameType, in order."""
    return [
        mock.recorded_frame(i).payload
        for i in range(mock.recorded())
        if mock.recorded_frame(i).type == ty
    ]


def test_clip_control_frames():
    with MockBox() as mock, Device.with_mock(mock) as d:
        clip = d.clip()
        clip.set_retain(True)
        clip.set_autolock([Blanket.AIM, Blanket.BUTTONS])
        clip.set_loop(True)
        clip.start()
        clip.pause()
        clip.resume()
        clip.restart()
        clip.toggle()
        clip.stop()
        clip.clear()
        clip.finalize()
        clip.bind(ClipTrigger(Usage.key(0x3A), Edge.PRESS, ClipAction.START))
        clip.bind(ClipTrigger(Usage.button(Button.RIGHT), Edge.RELEASE, ClipAction.TOGGLE, consume=True))
        clip.unbind(Usage.key(0x3A), Edge.PRESS)
        clip.clear_triggers()
        clip.close()
        clip_set = _clip_frames(d, mock, FrameType.CLIP_SET)
        ctrl = _clip_frames(d, mock, FrameType.CLIP_CTRL)
        trig = _clip_frames(d, mock, FrameType.CLIP_TRIGGER)
    assert clip_set == [bytes([2, 1]), bytes([0, 0x05]), bytes([1, 1])]
    assert ctrl == [bytes([n]) for n in (0, 2, 3, 4, 5, 1, 6, 7)]
    assert trig == [
        bytes([1, 0x3A, 0x00, 1, 0, 1]),       # bind KEY 0x3A Press Start (present)
        bytes([0, 0x01, 0x00, 2, 5, 3]),       # bind Button Right Release Toggle (present|consume)
        bytes([1, 0x3A, 0x00, 1, 0, 0]),       # unbind KEY 0x3A Press (present=0)
        bytes([0xFF, 0xFF, 0xFF, 0, 0, 0]),    # clear-all sentinel
    ]


def test_clip_append_encodes_and_chunks():
    with MockBox() as mock, Device.with_mock(mock) as d:
        b = ClipBuilder()
        for _ in range(150):
            b.move(3, -2)  # 150 * 5 = 750 bytes > 512: must split
        left = Usage.button(Button.LEFT)
        b.press(left).gap(4).release(left)
        clip = d.clip()
        clip.append(b)
        b.close()
        clip.close()
        appends = _clip_frames(d, mock, FrameType.CLIP_APPEND)
    assert len(appends) >= 2, "a >512-byte clip must chunk"
    joined = b"".join(appends)
    # 150 move(3,-2): flags=0x01, dx=3 LE, dy=-2 LE = 01 03 00 FE FF
    assert joined[:5] == bytes([0x01, 0x03, 0x00, 0xFE, 0xFF])
    # ... then press left (04 01 00 00 00 01), gap 4 (00 04 00), release left (04 01 00 00 00 00)
    assert joined.endswith(
        bytes([0x04, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x04, 0x00, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00])
    )
    assert all(len(p) <= 512 for p in appends)


def test_clip_builder_frame_edges():
    with MockBox() as mock, Device.with_mock(mock) as d:
        b = ClipBuilder()
        b.frame(1, 2, -1, edges=[(Usage.button(Button.LEFT), Action.PRESS), (Usage.key(0x04), Action.PRESS)])
        d.clip().append(b)
        b.close()
        appends = _clip_frames(d, mock, FrameType.CLIP_APPEND)
    # flags XY|WHEEL|EDGES=0x07, dx=1 dy=2, wheel=-1, n=2, [btn left press][key 0x04 press]
    assert appends[0] == bytes(
        [0x07, 0x01, 0x00, 0x02, 0x00, 0xFF, 0xFF, 0x02, 0x00, 0x00, 0x00, 0x01, 0x01, 0x04, 0x00, 0x01]
    )


def test_clip_frame_carries_pan_raw_reports_and_transfers():
    set_report = Setup(0x21, 0x09, 0x0300, 0, 2)
    get_report = Setup(0xA1, 0x01, 0x0300, 0, 8)
    with MockBox() as mock, Device.with_mock(mock) as d:
        with ClipBuilder() as b:
            b.frame(
                dx=4,
                dy=-2,
                wheel=1,
                pan=-3,
                edges=[(Usage.button(Button.LEFT), Action.PRESS)],
                raw=[(2, Direction.OUT, b"\x10\xFF\x05"), (1, Direction.IN, b"")],
                transfers=[(3, set_report, b"\x04\x01"), (4, get_report)],
            )
            d.clip().append(b)
        appends = _clip_frames(d, mock, FrameType.CLIP_APPEND)
    assert appends == [
        bytes(
            [0x3F, 0x04, 0x00, 0xFE, 0xFF]              # flags XY|WHEEL|EDGES|PAN|RAW|XFER, dx=4 dy=-2
            + [0x01, 0x00]                              # wheel=1
            + [0xFD, 0xFF]                              # pan=-3
            + [0x01, 0x00, 0x00, 0x00, 0x01]            # n=1, [btn left press]
            + [0x02]                                    # 2 raw reports
            + [0x02, 0x02, 0x03, 0x00, 0x10, 0xFF, 0x05]  # ep 2 OUT, 3 bytes
            + [0x01, 0x01, 0x00, 0x00]                  # ep 1 IN, 0 bytes
            + [0x02]                                    # 2 transfers
            + [0x03, 0x21, 0x09, 0x00, 0x03, 0x00, 0x00, 0x02, 0x00, 0x04, 0x01]  # ep 3, setup, 2 OUT bytes
            + [0x04, 0xA1, 0x01, 0x00, 0x03, 0x00, 0x00, 0x08, 0x00]              # ep 4, setup, no data
        )
    ]


def test_clip_builder_one_field_pan_raw_and_transfer_frames():
    with MockBox() as mock, Device.with_mock(mock) as d:
        with ClipBuilder() as b:
            b.pan(7).raw(2, Direction.OUT, b"\xAA").transfer(5, Setup(0x21, 0x09, 0x0300, 0, 1), b"\x05")
            b.transfer(6, Setup(0xA1, 0x01, 0x0300, 0, 8))
            assert b.byte_len() == 3 + 7 + 12 + 11
            d.clip().append(b)
        joined = b"".join(_clip_frames(d, mock, FrameType.CLIP_APPEND))
    assert joined == bytes(
        [0x08, 0x07, 0x00]                                                        # PAN, dpan=7
        + [0x10, 0x01, 0x02, 0x02, 0x01, 0x00, 0xAA]                              # RAW, n=1, ep 2 OUT, 1 byte
        + [0x20, 0x01, 0x05, 0x21, 0x09, 0x00, 0x03, 0x00, 0x00, 0x01, 0x00, 0x05]  # XFER, n=1, ep 5, setup, data
        + [0x20, 0x01, 0x06, 0xA1, 0x01, 0x00, 0x03, 0x00, 0x00, 0x08, 0x00]      # XFER, n=1, ep 6, setup
    )


def test_clip_builder_byte_len_is_what_an_append_puts_in_the_ring():
    left = Usage.button(Button.LEFT)
    with MockBox() as mock, Device.with_mock(mock) as d:
        with ClipBuilder() as b:
            assert b.byte_len() == 0
            b.frame()
            assert b.byte_len() == 5  # a frame carrying nothing is a zero XY tick
            b.gap(4).wheel(1).press(left)
            assert b.byte_len() == 5 + 3 + 3 + 6
            for _ in range(150):
                b.move(3, -2)
            b.frame(dx=1, raw=[(2, Direction.OUT, b"\x10\xFF")], transfers=[(3, Setup(0xA1, 0x01, 0x0300, 0, 8))])
            want = b.byte_len()
            d.clip().append(b)
            b.clear()
            assert b.byte_len() == 0
        appends = _clip_frames(d, mock, FrameType.CLIP_APPEND)
    assert len(appends) >= 2
    assert want == sum(len(p) for p in appends)


def test_clip_frame_transfers_take_a_pair_or_a_triple():
    get_report = Setup(0xA1, 0x01, 0x0300, 0, 8)
    set_report = Setup(0x21, 0x09, 0x0300, 0, 1)
    with MockBox() as mock, Device.with_mock(mock) as d:
        with ClipBuilder() as b:
            b.frame(transfers=[(3, get_report), [4, set_report, b"\x05"]])
            for bad in ([(3,)], [(3, get_report, b"", 0)], [get_report], [3]):
                with pytest.raises(ValueError, match=r"\(ep, setup\) or \(ep, setup, out_bytes\)"):
                    b.frame(transfers=bad)
            d.clip().append(b)
        appends = _clip_frames(d, mock, FrameType.CLIP_APPEND)
    assert appends == [
        bytes(
            [0x20, 0x02]
            + [0x03, 0xA1, 0x01, 0x00, 0x03, 0x00, 0x00, 0x08, 0x00]
            + [0x04, 0x21, 0x09, 0x00, 0x03, 0x00, 0x00, 0x01, 0x00, 0x05]
        )
    ]


@pytest.mark.parametrize("bad", [3, 0, True, "abc", None, 1.5, ["a"]])
def test_a_byte_argument_that_is_not_bytes_is_refused(bad):
    # bytes(3) is three zero bytes, which would go out as a report nobody wrote.
    setup = Setup(0x21, 0x09, 0x0300, 0, 3)
    with MockBox() as mock:
        mock.set_imperfect_status(_allowed())
        with Device.with_mock(mock) as d, ClipBuilder() as b:
            calls = [
                lambda: d.raw(1, Direction.OUT, bad),
                lambda: d.transfer(0, setup, bad),
                lambda: d.transfer(0, setup, bad, timeout_ms=1000),
                lambda: b.raw(1, Direction.OUT, bad),
                lambda: b.transfer(0, setup, bad),
                lambda: b.frame(raw=[(1, Direction.OUT, bad)]),
                lambda: b.frame(transfers=[(0, setup, bad)]),
                lambda: d.set_rewrite(RewriteRule(RewriteClass.EMIT, 1, Direction.IN, RewriteAction.REPLACE, payload=bad)),
                lambda: d.set_rewrite(RewriteRule(RewriteClass.EMIT, 1, Direction.IN, RewriteAction.PASS, match_bytes=bad, mask=b"")),
                lambda: d.set_rewrite(RewriteRule(RewriteClass.EMIT, 1, Direction.IN, RewriteAction.PASS, match_bytes=b"", mask=bad)),
                lambda: d.clip().bind_packet(ClipPacketTrigger(*_HELD[:4], match_bytes=bad, mask=b"")),
                lambda: d.clip().bind_packet(ClipPacketTrigger(*_HELD[:4], match_bytes=b"", mask=bad)),
                lambda: d.clip().unbind_packet(ClipPacketTrigger(*_HELD[:4], match_bytes=bad, mask=b"")),
                lambda: d.clip().unbind_packet(ClipPacketTrigger(*_HELD[:4], match_bytes=b"", mask=bad)),
                lambda: mock.set_clip_settings(ClipSettings(packet_triggers=[ClipPacketTrigger(*_HELD[:4], match_bytes=bad)])),
                lambda: mock.clip_packet(TrafficClass.HID_IN, 2, Direction.IN, bad),
                lambda: d.set_patch(Patch(PatchSection.DEVICE, 0, 0, 0, bad)),
                lambda: mock.set_transfer_reply(TransferStatus.OK, bad),
                lambda: mock.push_traffic(0, 0, ClockDomain.DEVICE_CHIP, TrafficEvent(CatchClass.HID_IN, 1, Direction.IN, 0, 3, bad)),
            ]
            for i, call in enumerate(calls):
                with pytest.raises(TypeError):
                    call()
                    pytest.fail(f"call {i} took {bad!r}")
            assert b.byte_len() == 0
            sent = {mock.recorded_frame(i).type for i in range(mock.recorded())}
    assert sent <= {FrameType.QUERY}


def test_a_byte_argument_takes_every_bytes_like_and_an_iterable_of_ints():
    with MockBox() as mock:
        mock.set_imperfect_status(_allowed())
        with Device.with_mock(mock) as d:
            for data in (b"\x01\x02", bytearray(b"\x01\x02"), memoryview(b"\x01\x02"), [1, 2], (1, 2), range(1, 3)):
                d.raw(1, Direction.OUT, data)
            raws = [
                mock.recorded_frame(i).payload
                for i in range(mock.recorded())
                if mock.recorded_frame(i).type == FrameType.RAW
            ]
            with pytest.raises(ValueError):
                d.raw(1, Direction.OUT, [1, 300])
    assert len(raws) == 6
    assert all(r.endswith(b"\x01\x02") for r in raws)
    assert len(set(raws)) == 1


def test_a_refused_clip_frame_raises_its_own_exception_and_sends_nothing():
    left = Usage.button(Button.LEFT)
    set_report = Setup(0x21, 0x09, 0x0300, 0, 2)
    refusals = [
        (ClipFrameCountError, dict(edges=[(left, Action.PRESS)] * (CLIP_EDGES_MAX + 1))),
        (ClipFrameCountError, dict(raw=[(1, Direction.OUT, b"\x00")] * (CLIP_RAW_MAX + 1))),
        (ClipFrameTooLongError, dict(raw=[(1, Direction.OUT, bytes(CLIP_ENTRY_MAX))])),
        (ClipTransferDataError, dict(transfers=[(0, set_report, b"\x04")])),
        (ClipTransferDataError, dict(transfers=[(0, Setup(0xA1, 0x01, 0x0300, 0, 8), b"\x04")])),
        (RawDirectionError, dict(raw=[(1, Direction.BOTH, b"\x00")])),
        (RelativeDirectionError, dict(raw=[(1, Direction.WITH, b"\x00")])),
    ]
    with MockBox() as mock, Device.with_mock(mock) as d:
        clip = d.clip()
        for exc, fields in refusals:
            with ClipBuilder() as b:
                b.move(1, 1).frame(**fields)  # a good entry ahead of the bad one
                with pytest.raises(exc):
                    clip.append(b)
        assert _clip_frames(d, mock, FrameType.CLIP_APPEND) == []
        # The limits themselves are admitted.
        with ClipBuilder() as b:
            b.frame(
                edges=[(left, Action.PRESS)] * CLIP_EDGES_MAX,
                raw=[(1, Direction.OUT, b"\x00")] * CLIP_RAW_MAX,
            )
            clip.append(b)
        assert len(_clip_frames(d, mock, FrameType.CLIP_APPEND)) == 1


def test_a_bad_clip_frame_field_leaves_the_builder_as_it_was():
    with MockBox() as mock, Device.with_mock(mock) as d:
        with ClipBuilder() as b:
            b.move(1, 1)
            with pytest.raises(ValueError):
                b.frame(dx=2, raw=[(1, 200, b"\x00")])
            with pytest.raises(ValueError):
                b.frame(dx=2, raw=[(300, Direction.OUT, b"\x00")])
            with pytest.raises(ValueError):
                b.frame(dx=2, transfers=[(0, Setup(0x21, 0x09, 0x0300, 0, 70_000), b"")])
            with pytest.raises(ValueError):
                b.frame(pan=70_000)
            with pytest.raises(ValueError):
                b.raw(1, 200, b"\x00")
            with pytest.raises(ValueError):
                b.pan(70_000)
            d.clip().append(b)
        assert _clip_frames(d, mock, FrameType.CLIP_APPEND) == [bytes([0x01, 0x01, 0x00, 0x01, 0x00])]


def test_the_clip_limits_are_the_header_s():
    import re

    header = pathlib.Path(__file__).resolve().parents[3] / "medius-capi" / "include" / "medius.h"
    if not header.exists():
        pytest.skip(f"{header} not present")
    defines = dict(re.findall(r"^#define (MEDIUS_\w+) (\d+)$", header.read_text(), re.M))
    assert int(defines["MEDIUS_CLIP_EDGES_MAX"]) == CLIP_EDGES_MAX
    assert int(defines["MEDIUS_CLIP_RAW_MAX"]) == CLIP_RAW_MAX
    assert int(defines["MEDIUS_CLIP_ENTRY_MAX"]) == CLIP_ENTRY_MAX
    assert int(defines["MEDIUS_CLIP_PKT_TRIG_MAX"]) == CLIP_PKT_TRIG_MAX
    assert int(defines["MEDIUS_CLIP_PKT_MATCH_POOL"]) == CLIP_PKT_MATCH_POOL
    assert int(defines["MEDIUS_MAX_PKT_MATCH"]) == PKT_MATCH_MAX
    assert int(defines["MEDIUS_CATCH_CLASS_CLIP_TRANSFER"]) == CatchClass.CLIP_TRANSFER == TrafficClass.CLIP_TRANSFER


def test_clip_status_and_config_roundtrip():
    status = ClipStatus(
        ClipState.PLAYING, free=512, total=40, played=8, ticks=99, underruns=2, overruns=0,
        seq_gaps=1, xfers=7, xfer_errs=3, gated=5,
        held=[Usage.button(Button.SIDE1), Usage.key(Key.A)],
    )
    settings = ClipSettings(
        autolock=[Blanket.AIM, Blanket.KEYS],
        loop=True,
        retain=True,
        finalized=False,
        ride=True,
        triggers=[
            ClipTrigger(Usage.button(Button.RIGHT), Edge.BOTH, ClipAction.TOGGLE),
            ClipTrigger(Usage.key(0x3A), Edge.RELEASE, ClipAction.STOP, consume=True),
        ],
        packet_triggers=_packet_rows()[:3],
    )
    with MockBox() as mock:
        # Packet rows 0 and 2 consume, which the box holds only under the opt-in.
        mock.set_imperfect_status(_allowed())
        mock.set_clip_status(status)
        mock.set_clip_settings(settings)
        with Device.with_mock(mock) as d:
            got = d.clip().query_status()
            cfg = d.clip().query_config()
    assert got == status
    assert (got.xfers, got.xfer_errs, got.gated) == (7, 3, 5)
    assert got.state == ClipState.PLAYING
    assert got.is_held(Usage.button(Button.SIDE1))
    assert got.is_held(Usage.key(Key.A))
    assert not got.is_held(Usage.button(Button.LEFT))
    assert cfg == settings


# The protocol's own example: HID_IN interface 2, IN, START, consume and once per run, selector 1,
# match 07 20 under mask FF 20.
_HELD = (TrafficClass.HID_IN, 2, Direction.IN, ClipAction.START, b"\x07\x20", b"\xFF\x20", True, True, 1)


def _packet_rows():
    """Eight packet triggers that differ in every field. Row 0 consumes only, row 1 is once per run
    only, rows 2 and 6 are both, and row 7 fills the match array. Each is a trigger a packet can
    match: a direction its class carries, every match bit under its mask, and a masked bit past a
    once-per-run selector. The vendor classes take IN, OUT and both."""
    masks = [0xFF, 0xF0, 0x0F, 0x20, 0x81, 0x7E, 0xC3, 0x01]
    rows = [
        (TrafficClass.HID_IN, 0x0102, Direction.IN, ClipAction.START, 1, True, None, 0),
        (TrafficClass.HID_OUT, 0x0001, Direction.OUT, ClipAction.STOP, 2, False, 1, 1),
        (TrafficClass.VENDOR_INTERRUPT, 0x0083, Direction.IN, ClipAction.PAUSE, 3, True, 2, 0xFFFF),
        (TrafficClass.VENDOR_BULK, ClipPacketTrigger.ANY_ID, Direction.BOTH, ClipAction.RESUME, 4, False, None, 0x1234),
        (TrafficClass.CONTROL, 0, Direction.BOTH, ClipAction.RESTART, 5, False, None, 0x00FF),
        (TrafficClass.EMIT, 0x0081, Direction.IN, ClipAction.TOGGLE, 0, False, None, 0xFF00),
        (TrafficClass.VENDOR_BULK, 0x0003, Direction.OUT, ClipAction.START, 7, True, 3, 65534),
        (TrafficClass.EMIT, 0x0082, Direction.IN, ClipAction.STOP, PKT_MATCH_MAX, False, None, 9),
    ]
    return [
        ClipPacketTrigger(
            traffic_class,
            id,
            direction,
            action,
            match_bytes=bytes((0x35 + 0x1B * i + 0x47 * j) & masks[(i + j) % 8] for j in range(n)),
            mask=bytes(masks[(i + j) % 8] for j in range(n)),
            consume=consume,
            once_per_run=selector is not None,
            selector_len=selector or 0,
            hits=hits,
        )
        for i, (traffic_class, id, direction, action, n, consume, selector, hits) in enumerate(rows)
    ]


def test_a_packet_trigger_goes_out_as_the_bytes_the_protocol_names():
    held = ClipPacketTrigger(*_HELD)
    # A removal sends the key alone, whatever the other fields hold.
    stale = ClipPacketTrigger(*_HELD[:3], 200, *_HELD[4:6], consume=True, once_per_run=True, selector_len=9, hits=77)
    with MockBox() as mock, Device.with_mock(mock) as d:
        clip = d.clip()
        clip.bind_packet(held)
        clip.unbind_packet(stale)
        clip.bind_packet(ClipPacketTrigger(TrafficClass.VENDOR_BULK, ClipPacketTrigger.ANY_ID, Direction.BOTH, ClipAction.PAUSE, consume=True))
        clip.bind_packet(
            ClipPacketTrigger(
                TrafficClass.HID_OUT, 1, Direction.OUT, ClipAction.RESUME,
                b"\x01\x02\x03", b"\xFF\xFF\x0F", once_per_run=True, selector_len=2,
            )
        )
        clip.bind_packet(
            ClipPacketTrigger(TrafficClass.CONTROL, 0, Direction.BOTH, ClipAction.TOGGLE, bytes(range(16)), bytes([0xFF] * 16), hits=500)
        )
        clip.bind_packet(ClipPacketTrigger(TrafficClass.EMIT, 0x0181, Direction.IN, ClipAction.STOP, [0x80], (0xF0,)))
        clip.clear_triggers()
        trig = _clip_frames(d, mock, FrameType.CLIP_TRIGGER)
    # [class][id u16][dir][action][flags: 1 present, 2 consume, 4 once per run][slen][mlen][match][mask]
    assert trig == [
        bytes.fromhex("04 02 00 01 00 07 01 02 07 20 FF 20"),
        bytes.fromhex("04 02 00 01 00 00 00 02 07 20 FF 20"),
        bytes.fromhex("07 FF FF 00 02 03 00 00"),
        bytes.fromhex("05 01 00 02 03 05 02 03 01 02 03 FF FF 0F"),
        bytes.fromhex("08 00 00 00 05 01 00 10") + bytes(range(16)) + bytes([0xFF] * 16),
        bytes.fromhex("09 81 01 01 01 01 00 01 80 F0"),
        bytes.fromhex("FF FF FF 00 00 00"),
    ]


def test_packet_triggers_read_back_field_for_field():
    rows = _packet_rows()
    assert rows[2].hits == 0xFFFF, "one row reads back a saturated count"
    inputs = [ClipTrigger(Usage.button(Button.RIGHT), Edge.PRESS, ClipAction.START)]
    for n in (0, 1, 8):
        with MockBox() as mock:
            # Rows 0, 2 and 6 consume, which the box holds only under the opt-in.
            mock.set_imperfect_status(_allowed())
            mock.set_clip_settings(ClipSettings(triggers=inputs, packet_triggers=rows[:n]))
            with Device.with_mock(mock) as d:
                cfg = d.clip().query_config()
        assert cfg.packet_triggers == rows[:n]
        assert cfg.triggers == inputs, "the input triggers keep their own count"

    # The protocol's read-back example: HID_IN id 0x0102, IN, TOGGLE, consume and once per run,
    # selector 1, hits saturated.
    spec = ClipPacketTrigger(TrafficClass.HID_IN, 0x0102, Direction.IN, ClipAction.TOGGLE, b"\x07\x20", b"\xFF\x20", True, True, 1, 0xFFFF)
    with MockBox() as mock:
        mock.set_imperfect_status(_allowed())
        mock.set_clip_settings(ClipSettings(packet_triggers=[spec]))
        with Device.with_mock(mock) as d:
            clip = d.clip()
            (got,) = clip.query_config().packet_triggers
            assert got == spec
            assert (got.consume, got.once_per_run, got.selector_len, got.hits) == (True, True, 1, 0xFFFF)
            assert (got.match_bytes, got.mask) == (b"\x07\x20", b"\xFF\x20")
            # A read trigger replays as a bind, its hits left behind.
            clip.bind_packet(got)
            assert _clip_frames(d, mock, FrameType.CLIP_TRIGGER) == [
                bytes.fromhex("04 02 01 01 05 07 01 02 07 20 FF 20")
            ]


def test_a_read_back_packet_trigger_carries_each_length_in_its_own_field():
    # The box holds a match and a mask of one length, so only a hand-built struct tells the two apart.
    from medius import _native
    from medius._types import clip_packet_trigger_from_c

    c = _native.MediusClipPacketTrigger()
    c.class_, c.id, c.direction, c.action = TrafficClass.HID_IN, 2, Direction.IN, ClipAction.START
    for i in range(PKT_MATCH_MAX):
        c.match_bytes[i] = i + 1
        c.mask[i] = 0xF0 + i
    c.match_len, c.mask_len = 3, 1
    got = clip_packet_trigger_from_c(c)
    assert (got.match_bytes, got.mask) == (b"\x01\x02\x03", b"\xF0")
    # A length past the array reads the array.
    c.match_len, c.mask_len = 1, 40
    got = clip_packet_trigger_from_c(c)
    assert (got.match_bytes, got.mask) == (b"\x01", bytes(range(0xF0, 0x100)))


def test_a_consuming_trigger_scripted_with_the_opt_in_off_is_left_out():
    consuming = ClipPacketTrigger(*_HELD, hits=7)
    watching = ClipPacketTrigger(TrafficClass.HID_IN, 3, Direction.IN, ClipAction.START, b"\x07\x20", b"\xFF\x20", False, True, 1, 5)
    inputs = [ClipTrigger(Usage.button(Button.RIGHT), Edge.PRESS, ClipAction.START, consume=True)]
    settings = ClipSettings(triggers=inputs, packet_triggers=[consuming, watching])
    with MockBox() as mock, Device.with_mock(mock) as d:
        clip = d.clip()
        mock.set_clip_settings(settings)
        cfg = clip.query_config()
        assert cfg.triggers == inputs, "an input trigger that consumes is held whatever the opt-in"
        assert cfg.packet_triggers == [watching]

        # Scripted under the opt-in, both are held, and turning it off takes the consuming one away.
        mock.set_imperfect_status(_allowed())
        mock.set_clip_settings(settings)
        assert clip.query_config().packet_triggers == [consuming, watching]
        mock.set_imperfect_status(ImperfectStatus(allowed=False, over_capacity=False, clone_imperfect=False))
        assert clip.query_config().packet_triggers == [watching]


def test_a_bound_packet_trigger_reads_back_as_it_was_bound():
    rows = _packet_rows()
    with MockBox() as mock:
        mock.set_imperfect_status(_allowed())
        with Device.with_mock(mock) as d:
            clip = d.clip()
            for t in rows:
                clip.bind_packet(t)
            cfg = clip.query_config()
            # Each flag reads back on its own: row 0 consumes, row 1 is once per run.
            assert [(t.consume, t.once_per_run) for t in cfg.packet_triggers[:2]] == [(True, False), (False, True)]
            # A bind starts its count at zero, whatever hits the trigger held.
            assert [t.hits for t in cfg.packet_triggers] == [0] * 8
            for t in rows:
                t.hits = 0
            assert cfg.packet_triggers == rows

            clip.unbind_packet(rows[2])
            clip.unbind_packet(rows[7])
            assert clip.query_config().packet_triggers == rows[:2] + rows[3:7]


def test_settings_past_the_arrays_are_clamped_to_them():
    # The C struct holds eight of each kind, as the box does.
    inputs = [ClipTrigger(Usage.key(0x04 + i), Edge.PRESS, ClipAction.START) for i in range(CLIP_PKT_TRIG_MAX + 1)]
    packets = [
        ClipPacketTrigger(TrafficClass.HID_IN, i, Direction.IN, ClipAction.START, bytes([i]), b"\xFF", hits=i)
        for i in range(CLIP_PKT_TRIG_MAX + 1)
    ]
    with MockBox() as mock:
        mock.set_clip_settings(ClipSettings(triggers=inputs, packet_triggers=packets))
        with Device.with_mock(mock) as d:
            cfg = d.clip().query_config()
    assert cfg.triggers == inputs[:8]
    assert cfg.packet_triggers == packets[:CLIP_PKT_TRIG_MAX]


def test_the_public_names_are_the_packet_trigger_ones():
    added = {
        "ClipPacketTrigger",
        "ClipPacketTriggerError",
        "CLIP_PKT_TRIG_MAX",
        "CLIP_PKT_MATCH_POOL",
        "PKT_MATCH_MAX",
    }
    removed = {"ClipVerb", "RewriteClipRuleError", "REWRITE_CLIP_DROP", "REWRITE_CLIP_EDGE"}
    assert added <= set(medius.__all__)
    assert all(hasattr(medius, name) for name in added)
    assert removed.isdisjoint(medius.__all__)
    assert not [name for name in removed if hasattr(medius, name)]
    assert len(medius.__all__) == len(set(medius.__all__))
    assert not [name for name in medius.__all__ if not hasattr(medius, name)]
    assert "CLIP" not in RewriteAction.__members__
    assert not hasattr(RewriteRule, "clip") and not hasattr(RewriteRule, "clip_verb")
    assert "ERR_REWRITE_CLIP_RULE" not in Status.__members__
    assert {"bind_packet", "unbind_packet"} <= set(dir(medius.ClipHandle))
    assert "clip_packet" in dir(MockBox)


def test_clear_triggers_removes_both_kinds():
    rows = _packet_rows()
    inputs = [ClipTrigger(Usage.button(Button.RIGHT), Edge.PRESS, ClipAction.START)]
    with MockBox() as mock:
        mock.set_clip_settings(ClipSettings(triggers=inputs, packet_triggers=[rows[1]]))
        with Device.with_mock(mock) as d:
            clip = d.clip()
            clip.bind_packet(rows[5])
            held = clip.query_config()
            assert (len(held.triggers), len(held.packet_triggers)) == (1, 2)
            clip.clear_triggers()
            cleared = clip.query_config()
            assert (cleared.triggers, cleared.packet_triggers) == ([], [])


def test_a_packet_trigger_the_box_would_refuse_has_its_own_exception():
    def base(**changes):
        fields = dict(
            traffic_class=TrafficClass.HID_IN, id=2, direction=Direction.IN, action=ClipAction.START,
            match_bytes=b"\x07\x20", mask=b"\xFF\x20",
        )
        fields.update(changes)
        return ClipPacketTrigger(**fields)

    # (trigger, the reason the message carries, whether the key is at fault)
    refused = [
        (base(traffic_class=TrafficClass.BUS), "names a surface packets cross", True),
        (base(traffic_class=TrafficClass.CLIP_TRANSFER), "names a surface packets cross", True),
        (base(mask=b"\xFF"), "a match and a mask of one length", True),
        (base(match_bytes=b"\x07"), "a match and a mask of one length", True),
        (base(match_bytes=bytes(17), mask=bytes(17)), "at most 16 match bytes", True),
        (base(match_bytes=bytes(0xFFFF), mask=bytes(0xFFFF)), "at most 16 match bytes", True),
        (base(traffic_class=TrafficClass.CONTROL, consume=True), "takes consume on a report or vendor class", False),
        (base(selector_len=1), "a selector length only with once_per_run", False),
        (base(traffic_class=TrafficClass.CONTROL, once_per_run=True), "needs one stream", False),
        (base(id=ClipPacketTrigger.ANY_ID, once_per_run=True), "needs one stream", False),
        (base(direction=Direction.BOTH, once_per_run=True), "needs one stream", False),
        (base(once_per_run=True, selector_len=2), "needs match bytes past its selector", False),
        (base(match_bytes=b"", mask=b"", once_per_run=True), "needs match bytes past its selector", False),
    ]
    with MockBox() as mock, Device.with_mock(mock) as d:
        clip = d.clip()
        for trigger, reason, key_fault in refused:
            with pytest.raises(ClipPacketTriggerError) as e:
                clip.bind_packet(trigger)
            assert e.value.status == Status.ERR_CLIP_PACKET_TRIGGER == 33
            assert e.value.message.startswith("a clip packet trigger ") and reason in e.value.message, trigger
            # A removal reads the key alone, so it refuses a bad key and takes anything else.
            if key_fault:
                with pytest.raises(ClipPacketTriggerError) as e:
                    clip.unbind_packet(trigger)
                assert reason in e.value.message
            else:
                clip.unbind_packet(trigger)
        for relative in (Direction.WITH, Direction.AGAINST):
            with pytest.raises(RelativeDirectionError):
                clip.bind_packet(base(direction=relative))
            with pytest.raises(RelativeDirectionError):
                clip.unbind_packet(base(direction=relative))
        # The removals of the seven sound keys are all that went out.
        sent = _clip_frames(d, mock, FrameType.CLIP_TRIGGER)
        assert len(sent) == sum(1 for r in refused if not r[2])
        assert all(p[4:7] == bytes(3) for p in sent)
        assert clip.query_config().packet_triggers == []

        # The longest match the box compares is bound and read back whole.
        full = base(match_bytes=bytes([0x11] * PKT_MATCH_MAX), mask=bytes([0xFF] * PKT_MATCH_MAX))
        clip.bind_packet(full)
        assert clip.query_config().packet_triggers == [full]


def _refused_for(call, reason):
    """`call` raises the packet trigger refusal and its message carries `reason`."""
    with pytest.raises(ClipPacketTriggerError) as e:
        call()
    assert e.value.status == Status.ERR_CLIP_PACKET_TRIGGER
    assert e.value.message.startswith("a clip packet trigger ") and reason in e.value.message, e.value.message


def test_a_packet_trigger_on_a_direction_its_class_never_carries_is_refused():
    reason = "names a direction its class never carries: HidIn and Emit flow IN, HidOut flows OUT"

    def on(traffic_class, direction):
        return ClipPacketTrigger(traffic_class, 1, direction, ClipAction.START, b"\x07", b"\xFF")

    with MockBox() as mock, Device.with_mock(mock) as d:
        clip = d.clip()
        for traffic_class, direction in (
            (TrafficClass.HID_IN, Direction.OUT),
            (TrafficClass.EMIT, Direction.OUT),
            (TrafficClass.HID_OUT, Direction.IN),
        ):
            _refused_for(lambda: clip.bind_packet(on(traffic_class, direction)), reason)
            _refused_for(lambda: clip.unbind_packet(on(traffic_class, direction)), reason)
        assert _clip_frames(d, mock, FrameType.CLIP_TRIGGER) == []

        # Every flow a class carries is bound, and every class takes both.
        carried = [
            on(TrafficClass.HID_IN, Direction.IN),
            on(TrafficClass.HID_IN, Direction.BOTH),
            on(TrafficClass.HID_OUT, Direction.OUT),
            on(TrafficClass.HID_OUT, Direction.BOTH),
            on(TrafficClass.VENDOR_INTERRUPT, Direction.IN),
            on(TrafficClass.VENDOR_INTERRUPT, Direction.OUT),
            on(TrafficClass.VENDOR_BULK, Direction.BOTH),
            on(TrafficClass.EMIT, Direction.IN),
        ]
        for t in carried:
            clip.bind_packet(t)
        assert clip.query_config().packet_triggers == carried


def test_a_packet_trigger_with_a_match_bit_outside_its_mask_is_refused():
    reason = "has a match bit outside its mask, which no packet can equal"

    def hid_in(match_bytes, mask):
        return ClipPacketTrigger(TrafficClass.HID_IN, 2, Direction.IN, ClipAction.START, match_bytes, mask)

    with MockBox() as mock, Device.with_mock(mock) as d:
        clip = d.clip()
        for t in (
            hid_in(b"\x07\x21", b"\xFF\x20"),
            hid_in(b"\x01", b"\x00"),
            # The last byte the box compares.
            hid_in(bytes(PKT_MATCH_MAX - 1) + b"\x80", bytes([0xFF] * (PKT_MATCH_MAX - 1)) + b"\x7F"),
        ):
            _refused_for(lambda: clip.bind_packet(t), reason)
            _refused_for(lambda: clip.unbind_packet(t), reason)
        assert _clip_frames(d, mock, FrameType.CLIP_TRIGGER) == []

        # Every match bit under its mask is a key the box holds, as given.
        held = hid_in(b"\x07\x20", b"\xFF\x20")
        clip.bind_packet(held)
        assert _clip_frames(d, mock, FrameType.CLIP_TRIGGER) == [bytes.fromhex("04 02 00 01 00 01 00 02 07 20 FF 20")]
        assert clip.query_config().packet_triggers == [held]


def test_a_once_per_run_trigger_with_no_masked_bit_past_its_selector_is_refused():
    reason = "needs a masked bit past its selector"

    def run(selector_len, match_bytes, mask, once_per_run=True):
        return ClipPacketTrigger(
            TrafficClass.HID_IN, 2, Direction.IN, ClipAction.START, match_bytes, mask,
            once_per_run=once_per_run, selector_len=selector_len if once_per_run else 0,
        )

    refused = [
        (1, b"\x07\x00", b"\xFF\x00"),
        (0, b"\x00\x00", b"\x00\x00"),
        (2, b"\x07\x01\x00\x00", b"\xFF\xFF\x00\x00"),
    ]
    with MockBox() as mock, Device.with_mock(mock) as d:
        clip = d.clip()
        for shape in refused:
            _refused_for(lambda: clip.bind_packet(run(*shape)), reason)
        assert _clip_frames(d, mock, FrameType.CLIP_TRIGGER) == []
        # The key is sound, so a removal goes out, and so does the same trigger bound on each packet.
        for shape in refused:
            clip.unbind_packet(run(*shape))
            clip.bind_packet(run(*shape, once_per_run=False))
        # One masked bit past the selector is a condition.
        one_bit = run(1, b"\x07\x00\x00", b"\xFF\x00\x01")
        clip.bind_packet(one_bit)
        held = clip.query_config().packet_triggers
        assert held == [run(*shape, once_per_run=False) for shape in refused] + [one_bit]


def test_a_packet_that_cannot_exist_fires_nothing_in_the_mock():
    surfaces = [
        TrafficClass.HID_IN,
        TrafficClass.HID_OUT,
        TrafficClass.VENDOR_INTERRUPT,
        TrafficClass.VENDOR_BULK,
        TrafficClass.CONTROL,
        TrafficClass.EMIT,
    ]
    with MockBox() as mock, Device.with_mock(mock) as d:
        clip = d.clip()
        # One trigger on every packet of each surface, so any packet the mock takes fires.
        for traffic_class in surfaces:
            clip.bind_packet(ClipPacketTrigger(traffic_class, ClipPacketTrigger.ANY_ID, Direction.BOTH, ClipAction.TOGGLE))
        no_packet = [
            (TrafficClass.HID_IN, Direction.OUT),
            (TrafficClass.EMIT, Direction.OUT),
            (TrafficClass.HID_OUT, Direction.IN),
            (TrafficClass.BUS, Direction.IN),
            (TrafficClass.CLIP_TRANSFER, Direction.IN),
        ] + [(c, direction) for c in surfaces for direction in (Direction.BOTH, Direction.WITH, Direction.AGAINST)]
        for traffic_class, direction in no_packet:
            assert mock.clip_packet(traffic_class, 1, direction, b"\x07") == (None, False), (traffic_class, direction)
        assert [t.hits for t in clip.query_config().packet_triggers] == [0] * 6

        # Each flow a surface carries is a packet, and its trigger counts it.
        for traffic_class, direction in (
            (TrafficClass.HID_IN, Direction.IN),
            (TrafficClass.HID_OUT, Direction.OUT),
            (TrafficClass.VENDOR_INTERRUPT, Direction.IN),
            (TrafficClass.VENDOR_INTERRUPT, Direction.OUT),
            (TrafficClass.VENDOR_BULK, Direction.IN),
            (TrafficClass.VENDOR_BULK, Direction.OUT),
            (TrafficClass.CONTROL, Direction.IN),
            (TrafficClass.CONTROL, Direction.OUT),
            (TrafficClass.EMIT, Direction.IN),
        ):
            assert mock.clip_packet(traffic_class, 1, direction, b"\x07") == (ClipAction.TOGGLE, False)
        assert [t.hits for t in clip.query_config().packet_triggers] == [1, 1, 2, 2, 2, 1]


def test_a_consuming_packet_trigger_needs_the_imperfect_opt_in_and_goes_when_it_is_turned_off():
    consuming = ClipPacketTrigger(*_HELD)
    watching = ClipPacketTrigger(TrafficClass.HID_IN, 3, Direction.IN, ClipAction.START, b"\x07\x20", b"\xFF\x20")
    with MockBox() as mock, Device.with_mock(mock) as d:
        clip = d.clip()
        # The bind goes out either way; the box holds the consuming one only under the opt-in.
        clip.bind_packet(consuming)
        clip.bind_packet(watching)
        assert clip.query_config().packet_triggers == [watching]
        d.allow_imperfect_clones(True)
        clip.bind_packet(consuming)
        assert clip.query_config().packet_triggers == [watching, consuming]
        d.allow_imperfect_clones(False)
        assert clip.query_config().packet_triggers == [watching]


def test_the_mock_runs_a_packet_through_its_triggers():
    hid_in = (TrafficClass.HID_IN, 2, Direction.IN)
    held = ClipPacketTrigger(*_HELD)
    let_go = ClipPacketTrigger(*hid_in, ClipAction.STOP, b"\x07\x00", b"\xFF\x20", once_per_run=True, selector_len=1)
    wide = ClipPacketTrigger(TrafficClass.HID_IN, ClipPacketTrigger.ANY_ID, Direction.BOTH, ClipAction.TOGGLE)
    with MockBox() as mock:
        mock.set_imperfect_status(_allowed())
        with Device.with_mock(mock) as d:
            clip = d.clip()
            for t in (wide, held, let_go):
                clip.bind_packet(t)
            # The held report starts the clip once, and every packet of the hold is consumed.
            assert mock.clip_packet(*hid_in, b"\x07\x20\x55") == (ClipAction.START, True)
            assert mock.clip_packet(*hid_in, b"\x07\x20\x56") == (None, True)
            # Another report ID leaves the run as it was.
            assert mock.clip_packet(*hid_in, b"\x09\x20") == (ClipAction.TOGGLE, False)
            assert mock.clip_packet(*hid_in, [0x07, 0x20]) == (None, True)
            # The release ends it, and the next hold starts it again.
            assert mock.clip_packet(*hid_in, b"\x07\x00") == (ClipAction.STOP, False)
            assert mock.clip_packet(*hid_in, b"\x07\x20") == (ClipAction.START, True)
            # The id wildcard takes what the exact triggers leave, and an empty head matches it.
            assert mock.clip_packet(TrafficClass.HID_IN, 5, Direction.IN, b"\x07\x20") == (ClipAction.TOGGLE, False)
            assert mock.clip_packet(TrafficClass.HID_IN, 5, Direction.IN, b"") == (ClipAction.TOGGLE, False)
            # Nothing wins on another class.
            assert mock.clip_packet(TrafficClass.HID_OUT, 2, Direction.OUT, b"\x07\x20") == (None, False)
            assert [t.hits for t in clip.query_config().packet_triggers] == [3, 4, 1]
            with pytest.raises(ValueError):
                mock.clip_packet(200, 2, Direction.IN, b"")
            with pytest.raises(ValueError):
                mock.clip_packet(CatchClass.KEY, 2, Direction.IN, b"")
            with pytest.raises(ValueError):
                mock.clip_packet(TrafficClass.HID_IN, 2, 200, b"")
            with pytest.raises(ValueError):
                mock.clip_packet(TrafficClass.HID_IN, 70_000, Direction.IN, b"")


def test_clip_packet_trigger_parameters_are_checked():
    hid_in = (TrafficClass.HID_IN, 2, Direction.IN, ClipAction.START)
    bad = [
        ClipPacketTrigger(200, 2, Direction.IN, ClipAction.START),
        ClipPacketTrigger(CatchClass.KEY, 2, Direction.IN, ClipAction.START),
        ClipPacketTrigger(TrafficClass.HID_IN, 70_000, Direction.IN, ClipAction.START),
        ClipPacketTrigger(TrafficClass.HID_IN, 2, 200, ClipAction.START),
        ClipPacketTrigger(*hid_in, match_bytes=[1, 300], mask=b"\xFF\xFF"),
    ]
    with MockBox() as mock, Device.with_mock(mock) as d:
        clip = d.clip()
        for t in bad:
            with pytest.raises(ValueError):
                clip.bind_packet(t)
            with pytest.raises(ValueError):
                clip.unbind_packet(t)
            with pytest.raises(ValueError):
                mock.set_clip_settings(ClipSettings(packet_triggers=[t]))
        for t in (
            ClipPacketTrigger(TrafficClass.HID_IN, 2, Direction.IN, 200),
            ClipPacketTrigger(*hid_in, selector_len=300),
            ClipPacketTrigger(*hid_in, hits=70_000),
        ):
            with pytest.raises(ValueError):
                clip.bind_packet(t)
        assert _clip_frames(d, mock, FrameType.CLIP_TRIGGER) == []


def test_the_status_codes_are_the_header_s():
    import re

    header = pathlib.Path(__file__).resolve().parents[3] / "medius-capi" / "include" / "medius.h"
    if not header.exists():
        pytest.skip(f"{header} not present")
    declared = {
        name: int(value)
        for name, value in re.findall(r"^    MEDIUS_STATUS_(\w+) = (\d+),$", header.read_text(), re.M)
    }
    assert declared == {s.name: s.value for s in Status}
    assert declared["ERR_CLIP_PACKET_TRIGGER"] == 33


def test_clip_builder_gap_zero_is_noop():
    with MockBox() as mock, Device.with_mock(mock) as d:
        b = ClipBuilder()
        b.gap(0)
        clip = d.clip()
        clip.append(b)
        appends = _clip_frames(d, mock, FrameType.CLIP_APPEND)
    assert appends == [], "an empty clip appends nothing"


def test_ctypes_structs_match_the_c_header():
    """Every shared struct must be the size the library thinks it is.

    This is not a decode concern, it is memory safety: `medius_event_stream_recv` writes
    `sizeof(MediusCatchEvent)` bytes into a buffer this module allocates, so a mirror short by one
    field lets the library write past the end of it on every event. A field added to the header and
    missed here is silent until it corrupts the heap, which is exactly how it happened.
    """
    import ctypes
    import re

    from medius import _native

    header = (
        pathlib.Path(__file__).resolve().parents[3]
        / "medius-capi"
        / "include"
        / "medius.h"
    )
    if not header.exists():
        pytest.skip(f"{header} not present")
    text = header.read_text()

    # The C compiler is the authority; parse each struct out of the header and sizeof it for real.
    # Derived from the header rather than listed here: a hardcoded list silently skips whatever it
    # does not name, which is how MediusLockEntry went uncovered through a field-meaning change.
    # cbindgen closes a typedef'd struct with `} Name;` at column 0; an enum closes with a bare `};`
    # and a nested member is indented, so neither is picked up.
    probe = pathlib.Path(tempfile.mkdtemp()) / "sizes.c"
    present = re.findall(r"^\} (Medius\w+);$", text, re.M)
    for must in (
        "MediusLockEntry",
        "MediusLocks",
        "MediusBearing",
        "MediusClipSettings",
        "MediusClipPacketTrigger",
    ):
        assert must in present, f"the header no longer declares {must}"
        assert hasattr(_native, must), f"{must} has no ctypes mirror to compare"
    # sizeof alone lets a same-size field REORDER through, which is a silent misread of every event
    # rather than a crash. Compare each field's offset too.
    fields = {
        n: [f[0] for f in getattr(_native, n)._fields_]
        for n in present
        if hasattr(_native, n)
    }
    # The typedef name, not `struct N`: cbindgen typedefs both structs and unions, and one of them
    # (MediusCatchEventData) is a union.
    lines = [f'    printf("%s %zu\\n", "{n}", sizeof({n}));' for n in present]
    for n, fs in fields.items():
        for f in fs:
            cname = "class_" if f == "class_" else f
            lines.append(
                f'    printf("%s.%s %zu\\n", "{n}", "{f}", offsetof({n}, {cname}));'
            )
    body = "\n".join(lines)
    probe.write_text(
        f'#include <stdio.h>\n#include <stddef.h>\n#include "{header}"\nint main(void) {{\n{body}\n    return 0;\n}}\n'
    )
    exe = probe.with_suffix("")
    if subprocess.run(["gcc", str(probe), "-o", str(exe)], capture_output=True).returncode != 0:
        pytest.skip("no working C compiler for the layout probe")
    out = subprocess.run([str(exe)], capture_output=True, text=True).stdout

    mismatches = []
    checked = 0
    for line in out.split("\n"):
        if not line.strip():
            continue
        what, value = line.split()
        value = int(value)
        if "." in what:
            name, field = what.split(".", 1)
            mirror = getattr(_native, name, None)
            if mirror is None:
                continue
            got = getattr(mirror, field).offset
            checked += 1
            if got != value:
                mismatches.append(f"{name}.{field}: C offset {value} vs python {got}")
        else:
            mirror = getattr(_native, what, None)
            if mirror is None:
                continue
            checked += 1
            if ctypes.sizeof(mirror) != value:
                mismatches.append(f"{what}: C {value} vs python {ctypes.sizeof(mirror)}")
    assert not mismatches, "ctypes mirrors drifted from medius.h: " + "; ".join(mismatches)
    assert checked > 150, f"the probe only compared {checked} things; it stopped covering the structs"
    # Every struct in the header that this module mirrors must have been compared; a mirror that
    # stops matching its name is a mirror that stops being checked.
    mirrored = {n for n in present if hasattr(_native, n)}
    compared = {line.split()[0].split(".", 1)[0] for line in out.split("\n") if line.strip()}
    assert mirrored <= compared, f"never compared: {sorted(mirrored - compared)}"


def test_input_events_decode_snapshots_into_edges():
    esc = Usage.key(Key.ESCAPE)
    a = Usage.key(Key.A)
    with MockBox() as mock, Device.with_mock(mock) as d:
        with d.input_events(CatchFilter.all_input()) as s:
            mock.push_usages(1, 1_000, UsageSnapshot([esc], Class.KEY, Direction.PRESS))
            mock.push_usages(2, 2_000, UsageSnapshot([esc, a], Class.KEY, Direction.PRESS))
            mock.push_usages(3, 3_000, UsageSnapshot([a], Class.KEY, Direction.RELEASE))
            mock.push_motion(4, 4_000, MotionEvent(dx=3, dy=-4, dz=0))

            ev = s.recv_timeout(2000)
            assert ev.kind == InputKind.PRESS and ev.usage == esc and ev.ts_us == 1_000
            assert (ev.dx, ev.dy, ev.dz) == (0, 0, 0)
            assert s.recv_timeout(2000).usage == a
            ev = s.recv_timeout(2000)
            assert ev.kind == InputKind.RELEASE and ev.usage == esc
            assert s.held(Class.KEY) == [a]
            ev = s.recv_timeout(2000)
            assert ev.kind == InputKind.MOTION and (ev.dx, ev.dy, ev.dz) == (3, -4, 0)
            assert ev.usage is None
            assert s.try_recv() is None


def test_input_events_refuse_what_they_cannot_decode():
    # Each refusal has its own status across the ABI, so a caller can tell a wrong filter from a
    # dead link. Folding them into ERR_UNKNOWN would lose exactly that.
    with MockBox() as mock, Device.with_mock(mock) as d:
        with pytest.raises(medius.NotAnInputFilterError):
            d.input_events(CatchFilter.traffic_class(TrafficClass.VENDOR_BULK))
        with pytest.raises(medius.WildcardNotInputError):
            d.input_events(CatchFilter.everything())
        with pytest.raises(medius.HalfEdgeInputFilterError):
            d.input_events(CatchFilter.watch(Usage.key(Key.A)).on_press())
        with pytest.raises(medius.CaptureNotApplicableError):
            d.catch_events(CatchFilter.watch_class(Class.KEY).with_capture(8))
        # 0xFFFF is the every-id sentinel, and a MediusCatchFilter carries nothing that could tell an
        # exact id apart from the blanket, so across this ABI a media usage of 0xFFFF IS the class
        # blanket. The native API refuses it outright; here it is a documented wire limitation, and
        # what matters is that it is the blanket rather than something narrower.
        assert CatchFilter.watch(Usage.media(0xFFFF)).id is None
        assert CatchFilter.watch(Usage.media(0xFFFF)) == CatchFilter.watch_class(Class.MEDIA)
        with d.input_events(CatchFilter.all_input()) as s:
            assert s.dropped == 0


def test_the_filter_constructors_address_inputs_like_lock_does():
    # The whole point of the input constructors: a key enum goes straight in, as it does for lock.
    # Requiring Usage.key(Key.A) here would put back the id arithmetic the rework removed.
    assert CatchFilter.watch(Key.A) == CatchFilter.watch(Usage.key(Key.A))
    assert CatchFilter.watch(Button.LEFT) == CatchFilter.watch(Usage.button(Button.LEFT))
    assert CatchFilter.watch(MediaKey.VOLUME_UP) == CatchFilter.watch(
        Usage.media(MediaKey.VOLUME_UP)
    )
    # A bare int names no class, so it is refused rather than guessed at.
    with pytest.raises(TypeError):
        CatchFilter.watch(4)

    key = CatchFilter.watch(Key.A)
    assert key.catch_class == CatchClass.KEY and key.id == Key.A
    btn = CatchFilter.watch(Usage.button(Button.LEFT))
    assert btn.catch_class == CatchClass.BUTTON and btn.id == int(Button.LEFT)
    assert CatchFilter.watch_axis(Axis.WHEEL).id == int(Axis.WHEEL)
    assert CatchFilter.watch_axes().id is None
    assert [f.catch_class for f in CatchFilter.all_input()] == [
        CatchClass.BUTTON,
        CatchClass.KEY,
        CatchClass.MEDIA,
        CatchClass.AXIS,
    ]
    # Capture is not part of a filter's address; direction is.
    bulk = CatchFilter.traffic(TrafficClass.VENDOR_BULK, 3)
    assert bulk.same_address(bulk.with_capture(16))
    assert not bulk.same_address(bulk.outbound())
    assert bulk != bulk.with_capture(16)
    assert CatchFilter.everything().capture == Capture.WHOLE
    assert CatchClass.KEY.is_input() and CatchClass.VENDOR_BULK.is_traffic()


def test_an_unknown_control_status_does_not_raise():
    # The C ABI reports a status this build does not know as OTHER, and the byte itself stays on
    # `flags`. Without the member, decoding one raised ValueError: the exact failure the distinct
    # variant was added to prevent, reintroduced one binding down.
    unknown = TrafficEvent(
        catch_class=CatchClass.CONTROL,
        id=0,
        direction=Direction.IN,
        flags=0x42,
        true_len=8,
        bytes=bytes(8),
    )
    with MockBox() as mock, Device.with_mock(mock) as d:
        with d.catch_events(CatchFilter.traffic_class(TrafficClass.CONTROL)) as s:
            ev = _push_and_recv(mock, s, unknown)
    assert ev.traffic.control_status() == ControlStatus.OTHER
    assert ev.traffic.flags == 0x42


def test_timeline_unwraps_the_rollover_and_maps_onto_the_callers_clock():
    with MockBox() as mock, Device.with_mock(mock) as d:
        with d.catch_events(CatchFilter.watch_axes()) as s, medius.Timeline() as t:
            # 32 bits of microseconds is 71.6 minutes; a raw subtraction across the wrap comes out
            # about 4295 seconds negative.
            mock.push_motion(1, 0xFFFFFE0C, MotionEvent(dx=1, dy=0, dz=0))
            mock.push_motion(2, 500, MotionEvent(dx=1, dy=0, dz=0))
            a = t.observe(s.recv_timeout(2000), 50_000_000)
            b = t.observe(s.recv_timeout(2000), 51_001_000)
            assert a.box_us == 0xFFFFFE0C
            assert b.box_us == (1 << 32) + 500
            assert b.host_ns > a.host_ns
            assert b.host_ns - a.host_ns == 1_000_000  # 1000 us on the box
            assert b.excess_ns == 1_000  # and 1 us later than the floor on the wall
            assert t.samples(ClockDomain.HOST_CHIP) == 2
            t.reset(ClockDomain.HOST_CHIP)
            assert t.samples(ClockDomain.HOST_CHIP) == 0


def test_every_enum_parameter_is_checked_before_it_reaches_the_boundary():
    # Each of these used to hand the C ABI a byte it materialised as a `#[repr(u8)]` enum before any
    # check could run: SIGSEGV where the value fell outside the jump table, and the wrong command on
    # the wire where it did not. There is no status to read back from a crashed interpreter.
    with MockBox() as mock, Device.with_mock(mock) as d:
        with pytest.raises(ValueError):
            d.led(LedTarget.DEVICE, 77, 5)
        with pytest.raises(ValueError):
            d.led(200, LedMode.SOLID, 55)
        with pytest.raises(ValueError):
            d.led(LedTarget.DEVICE, LedMode.SOLID, 300)
        with pytest.raises(ValueError):
            d.reboot(200)
        with pytest.raises(ValueError):
            d.inject(Usage.button(Button.LEFT), 200)
        with pytest.raises(ValueError):
            d.set_emit_pace(EmitPace(9, 1000))
        with pytest.raises(ValueError):
            d.move_axis(Motion.cursor(1, 2), 9, PendingMotion.KEEP)
        with pytest.raises(ValueError):
            d.move_axis(Motion.cursor(1, 2), MoveTiming.RIDE, 9)
        with pytest.raises(ValueError):
            d.move_rel(70_000, 0)
        with pytest.raises(ValueError):
            Usage.button(300)
        with pytest.raises(ValueError):
            Usage.key(300)
        with pytest.raises(ValueError):
            Motion.cursor(70_000, 0)
        # Nothing that raised reached the wire.
        assert mock.recorded() == 0
        # and the named values still send what they always did
        d.led(LedTarget.BOTH, LedMode.BLINK, 128)
        assert _clip_frames(d, mock, FrameType.LED) == [bytes([2, 3, 128])]


def test_clip_enum_parameters_are_checked():
    with MockBox() as mock, Device.with_mock(mock) as d:
        clip = d.clip()
        with pytest.raises(ValueError):
            clip.unbind(Usage.button(Button.LEFT), 200)
        with pytest.raises(ValueError):
            clip.bind(ClipTrigger(Usage.button(Button.LEFT), 200, ClipAction.START))
        with pytest.raises(ValueError):
            clip.bind(ClipTrigger(Usage.button(Button.LEFT), Edge.PRESS, 200))
        with pytest.raises(ValueError):
            clip.set_autolock([Blanket.AIM, 99])
        b = ClipBuilder()
        with pytest.raises(ValueError):
            b.edge(Usage.button(Button.LEFT), 200)
        with pytest.raises(ValueError):
            b.frame(edges=[(Usage.button(Button.LEFT), 200)])
        with pytest.raises(ValueError):
            b.move(70_000, 0)
        b.close()
        assert _clip_frames(d, mock, FrameType.CLIP_TRIGGER) == []
        assert _clip_frames(d, mock, FrameType.CLIP_SET) == []


def test_mock_and_stream_enum_parameters_are_checked():
    with MockBox() as mock, Device.with_mock(mock) as d:
        with pytest.raises(ValueError):
            mock.push_log(200, "hi")
        with pytest.raises(ValueError):
            mock.saw(200)
        with pytest.raises(ValueError):
            mock.set_emit_pace(EmitPace(9, 0))
        with pytest.raises(ValueError):
            mock.push_traffic(0, 0, 200, TrafficEvent(TrafficClass.HID_IN, 1, Direction.IN, 0, 0, b""))
        with pytest.raises(ValueError):
            mock.set_device_info(DeviceInfo(1, 2, 3, 4, False, False, 200, ""))
        with pytest.raises(ValueError):
            mock.set_clip_status(
                ClipStatus(200, free=0, total=0, played=0, ticks=0, underruns=0, overruns=0,
                           seq_gaps=0, xfers=0, xfer_errs=0, gated=0, held=[])
            )
        with pytest.raises(ValueError):
            mock.push_usages(0, 0, UsageSnapshot([], 200, Direction.POSITIVE))
        t = Timeline()
        with pytest.raises(ValueError):
            t.reset(200)
        with pytest.raises(ValueError):
            t.samples(200)
        t.close()
        stream = d.input_events(CatchFilter.all_input())
        with pytest.raises(ValueError):
            stream.held(200)
        stream.close()


# --- Advanced control layer (§3.14): raw injection, control transfers, rewrite rules, descriptor patches ---


def _allowed():
    return ImperfectStatus(allowed=True, over_capacity=False, clone_imperfect=False)


def test_dev_layer_health_bits_roundtrip():
    health = Health(
        link_up=True,
        mouse_attached=False,
        clone_configured=False,
        injection_active=False,
        rate_confident=False,
        lock_on=False,
        catch_on=False,
        kbd_attached=False,
        rewrite_on=True,
        patch_on=False,
        transform_on=True,
    )
    with MockBox() as mock:
        mock.set_health(health)
        with Device.with_mock(mock) as d:
            got = d.query_health()
    assert got.rewrite_on is True
    assert got.patch_on is False
    assert got.transform_on is True
    assert got == health


def test_dev_layer_frames_carry_their_type():
    with MockBox() as mock:
        mock.set_imperfect_status(_allowed())
        with Device.with_mock(mock) as d:
            d.raw(1, Direction.IN, b"\x00\x01\x02\x03")
            d.set_rewrite(RewriteRule(RewriteClass.EMIT, 1, Direction.IN, RewriteAction.DROP))
            d.set_patch(Patch(PatchSection.DEVICE, 0, 0, 8, b"\x34\x12"))
            d.transfer(0, Setup(0x80, 0x06, 0x0100, 0, 18))
        assert mock.saw(FrameType.RAW)
        assert mock.saw(FrameType.REWRITE)
        assert mock.saw(FrameType.PATCH)
        assert mock.saw(FrameType.TRANSFER)


def test_raw_reaches_the_wire_verbatim():
    with MockBox() as mock:
        mock.set_imperfect_status(_allowed())
        with Device.with_mock(mock) as d:
            d.raw(1, Direction.IN, b"\x00\x01\x00\x00")
        frame = next(
            mock.recorded_frame(i)
            for i in range(mock.recorded())
            if mock.recorded_frame(i).type == FrameType.RAW
        )
    # RAW payload is [ep_num][dir][bytes...]: endpoint 1, IN, then the report.
    assert bytes(frame.payload) == b"\x01\x01\x00\x01\x00\x00"


def test_gated_dev_layer_calls_need_the_opt_in():
    with MockBox() as mock, Device.with_mock(mock) as d:
        with pytest.raises(ImperfectRequiredError):
            d.set_rewrite(RewriteRule(RewriteClass.EMIT, 1, Direction.IN, RewriteAction.DROP))
        with pytest.raises(ImperfectRequiredError):
            d.apply_patch()


def test_raw_sends_without_reading_the_opt_in():
    with MockBox() as mock, Device.with_mock(mock) as d:
        d.raw(1, Direction.IN, b"\x00\x01")
        assert any(
            mock.recorded_frame(i).type == FrameType.RAW for i in range(mock.recorded())
        )


def test_raw_rejects_a_direction_that_is_not_a_flow():
    with MockBox() as mock:
        mock.set_imperfect_status(_allowed())
        with Device.with_mock(mock) as d:
            # Both names two flows at once; the bearing-relative pair has no bearing here.
            with pytest.raises(RawDirectionError):
                d.raw(1, Direction.BOTH, b"\x00")
            with pytest.raises(RelativeDirectionError):
                d.raw(1, Direction.WITH, b"\x00")


def test_transfer_roundtrips_the_answer():
    reply = bytes([0x12, 0x01, 0x10, 0x02])
    with MockBox() as mock:
        mock.set_imperfect_status(_allowed())
        mock.set_transfer_reply(TransferStatus.OK, reply)
        with Device.with_mock(mock) as d:
            out = d.transfer(0, Setup(0x80, 0x06, 0x0100, 0, 18))
    assert isinstance(out, TransferOutcome)
    assert out.status == TransferStatus.OK
    assert out.is_ok
    assert out.data == reply


def test_transfer_takes_its_own_reply_wait():
    reply = bytes([0x12, 0x01])
    with MockBox() as mock:
        mock.set_imperfect_status(_allowed())
        mock.set_transfer_reply(TransferStatus.OK, reply)
        with Device.with_mock(mock) as d:
            out = d.transfer(
                0, Setup(0x80, 0x06, 0x0100, 0, 2), timeout_ms=medius.default_transfer_timeout_ms()
            )
    assert out.status == TransferStatus.OK
    assert out.data == reply
    assert medius.default_transfer_timeout_ms() >= 800


def test_transfer_is_refused_without_the_opt_in():
    with MockBox() as mock, Device.with_mock(mock) as d:
        out = d.transfer(0, Setup(0x80, 0x06, 0x0100, 0, 18))
    assert out.status == TransferStatus.REFUSED
    assert not out.is_ok
    assert out.data == b""


def test_a_non_ok_transfer_carries_no_data():
    # The box zeroes the IN length unless the status is OK, so a stall scripted with bytes drops them.
    with MockBox() as mock:
        mock.set_imperfect_status(_allowed())
        mock.set_transfer_reply(TransferStatus.STALL, b"\x12\x01\x00\x02")
        with Device.with_mock(mock) as d:
            out = d.transfer(0, Setup(0x80, 0x06, 0x0100, 0, 18))
    assert out.status == TransferStatus.STALL
    assert out.data == b""


def test_rewrite_survives_the_query_roundtrip():
    payload = bytes([0xAB] * 40)
    with MockBox() as mock:
        mock.set_imperfect_status(_allowed())
        with Device.with_mock(mock) as d:
            d.set_rewrite(
                RewriteRule(
                    RewriteClass.CONTROL,
                    0,
                    Direction.BOTH,
                    RewriteAction.REPLY_PATCH,
                    offset=258,
                    match_bytes=bytes([0x80, 0x06]),
                    mask=bytes([0xFF, 0xFF]),
                    payload=payload,
                )
            )
            table = d.query_rewrite()
            assert isinstance(table, RewriteTable)
            assert len(table.entries) == 1
            entry = table.entries[0]
            assert isinstance(entry, RewriteEntry)
            assert entry.rewrite_class == RewriteClass.CONTROL
            assert entry.action == RewriteAction.REPLY_PATCH
            assert entry.offset == 258
            assert entry.payload_len == 40

            read = d.query_rewrite_entry(0)
            assert read.rewrite_class == RewriteClass.CONTROL
            assert read.direction == Direction.BOTH
            assert read.action == RewriteAction.REPLY_PATCH
            assert read.offset == 258
            assert read.match_bytes == bytes([0x80, 0x06])
            assert read.mask == bytes([0xFF, 0xFF])
            assert read.payload == payload


def test_clear_rewrite_empties_the_table():
    with MockBox() as mock:
        mock.set_imperfect_status(_allowed())
        with Device.with_mock(mock) as d:
            d.set_rewrite(RewriteRule(RewriteClass.EMIT, 1, Direction.IN, RewriteAction.DROP))
            assert len(d.query_rewrite().entries) == 1
            d.clear_rewrite()
            assert d.query_rewrite().entries == []


def test_rewrite_validation_errors_have_their_own_exception():
    with MockBox() as mock:
        mock.set_imperfect_status(_allowed())
        with Device.with_mock(mock) as d:
            with pytest.raises(RewriteMaskLengthError):
                d.set_rewrite(
                    RewriteRule(
                        RewriteClass.EMIT,
                        1,
                        Direction.IN,
                        RewriteAction.DROP,
                        match_bytes=b"\x01\x02",
                        mask=b"\xFF",
                    )
                )
            with pytest.raises(RewriteActionClassError):
                d.set_rewrite(RewriteRule(RewriteClass.CONTROL, 0, Direction.BOTH, RewriteAction.DROP))
            # REPLY_REPLACE is the last action the box names.
            with pytest.raises(ValueError):
                d.set_rewrite(RewriteRule(RewriteClass.HID_IN, 2, Direction.IN, RewriteAction.REPLY_REPLACE + 1))
            with pytest.raises(RelativeDirectionError):
                d.set_rewrite(RewriteRule(RewriteClass.EMIT, 1, Direction.WITH, RewriteAction.DROP))
            with pytest.raises(RewritePayloadTooLargeError):
                d.set_rewrite(
                    RewriteRule(
                        RewriteClass.EMIT,
                        1,
                        Direction.IN,
                        RewriteAction.REPLACE,
                        payload=bytes(100),
                    )
                )


def test_over_capacity_bytes_are_refused_before_ctypes():
    # The C struct holds a fixed 512 payload bytes. A longer payload raises here, because ctypes would
    # cut it to fit and the box would apply the shorter rule.
    with MockBox() as mock:
        mock.set_imperfect_status(_allowed())
        with Device.with_mock(mock) as d:
            with pytest.raises(ValueError):
                d.set_rewrite(
                    RewriteRule(
                        RewriteClass.CONTROL,
                        0,
                        Direction.BOTH,
                        RewriteAction.ANSWER,
                        payload=bytes(513),
                    )
                )
            with pytest.raises(ValueError):
                d.set_patch(Patch(PatchSection.DEVICE, 0, 0, 0, bytes(513)))


def test_a_rewrite_match_past_the_limit_has_its_own_exception():
    def rule(match_len, mask_len):
        return RewriteRule(
            RewriteClass.HID_IN,
            2,
            Direction.IN,
            RewriteAction.PASS,
            match_bytes=bytes([0x11]) * match_len,
            mask=bytes([0xFF]) * mask_len,
        )

    with MockBox() as mock:
        mock.set_imperfect_status(_allowed())
        with Device.with_mock(mock) as d:
            for n in (17, 40, 0xFFFF):
                with pytest.raises(RewriteMatchTooLongError) as e:
                    d.set_rewrite(rule(n, n))
                assert e.value.status == Status.ERR_REWRITE_MATCH_TOO_LONG
                with pytest.raises(RewriteMatchTooLongError):
                    d.remove_rewrite(rule(n, n))
            # The mask length is checked first.
            with pytest.raises(RewriteMaskLengthError):
                d.set_rewrite(rule(17, 16))
            with pytest.raises(RewriteMaskLengthError):
                d.set_rewrite(rule(16, 17))
            assert d.query_rewrite().entries == []

            d.set_rewrite(rule(16, 16))
            assert d.query_rewrite_entry(0) == rule(16, 16)


def test_patch_survives_the_query_roundtrip():
    data = bytes([0xCD] * 40)
    with MockBox() as mock, Device.with_mock(mock) as d:
        # set_patch is not gated on the opt-in; the box always stores it.
        d.set_patch(Patch(PatchSection.REPORT, 1, 2, 258, data))
        pset = d.query_patches()
        assert isinstance(pset, PatchSet)
        assert len(pset.entries) == 1
        entry = pset.entries[0]
        assert isinstance(entry, PatchEntry)
        assert entry.section == PatchSection.REPORT
        assert (entry.cfg, entry.index, entry.offset, entry.len) == (1, 2, 258, 40)

        read = d.query_patch_entry(0)
        assert read.section == PatchSection.REPORT
        assert (read.cfg, read.index, read.offset) == (1, 2, 258)
        assert read.bytes == data


def test_apply_and_clear_patch_reach_the_wire():
    with MockBox() as mock:
        mock.set_imperfect_status(_allowed())
        with Device.with_mock(mock) as d:
            d.set_patch(Patch(PatchSection.DEVICE, 0, 0, 8, b"\x34\x12"))
            assert d.query_patches().pending is True
            d.apply_patch()
            assert d.query_patches().applied is True
            d.clear_patch()
            assert d.query_patches().entries == []


def test_an_empty_patch_removes_the_stored_one():
    with MockBox() as mock, Device.with_mock(mock) as d:
        d.set_patch(Patch(PatchSection.DEVICE, 0, 0, 8, b"\x34\x12"))
        assert len(d.query_patches().entries) == 1
        d.set_patch(Patch(PatchSection.DEVICE, 0, 0, 8, b""))
        assert d.query_patches().entries == []


def test_pan_frames_carry_the_motion_tag():
    with MockBox() as mock, Device.with_mock(mock) as d:
        d.pan(3)
        d.pan_now(-2)
        d.move_axis(Motion.pan(4), MoveTiming.NOW, PendingMotion.KEEP)
        sent = [mock.recorded_frame(i) for i in range(3)]
    payloads = [bytes(f.payload) for f in sent]
    assert all(f.type == FrameType.MOVE for f in sent)
    # MOVE AC Pan: [motion=2][dpan i16 LE][flags]. Ride/Keep is 0x00, Now is 0x01.
    assert payloads == [
        bytes([2, 3, 0, 0x00]),
        bytes([2, 0xFE, 0xFF, 0x01]),
        bytes([2, 4, 0, 0x01]),
    ]


def test_buttons_past_five_address_by_id():
    # Button is an open id, not just the five named ones, so any u8 addresses one.
    u = Usage.button(7)
    assert u.kind == Class.BUTTON
    assert u.id == 7
    with MockBox() as mock, Device.with_mock(mock) as d:
        d.press(Usage.button(7))
        d.lock(LockTarget.button(9), Direction.PRESS)
        frame = mock.recorded_frame(0)
    # INJECT: [class=0][id u16 LE][action=press(1)].
    assert frame.type == FrameType.INJECT
    assert bytes(frame.payload) == bytes([0, 7, 0, 1])


def test_transform_verbs_reach_the_wire_ungated():
    # A transform is faithful, so it needs no imperfect-clone opt-in (unlike the rewrite/patch layer).
    with MockBox() as mock, Device.with_mock(mock) as d:
        d.transform_swap(Axis.X, Axis.Y)
        d.transform_remap(Axis.X, Axis.WHEEL)
        d.transform(Transform.remap(Axis.WHEEL, Axis.Y))
        d.untransform(Transform.remap(Axis.WHEEL, Axis.Y))
        d.clear_transforms()
        assert mock.saw(FrameType.TRANSFORM)
        # Five verbs, one TRANSFORM frame each.
        types = [mock.recorded_frame(i).type for i in range(mock.recorded())]
        assert types.count(FrameType.TRANSFORM) == 5


def test_a_transform_survives_the_query_roundtrip():
    with MockBox() as mock, Device.with_mock(mock) as d:
        d.transform_swap(Axis.X, Axis.Y)
        d.transform_remap(Axis.WHEEL, Axis.Y)
        d.transform_remap(Axis.Y, Axis.X)
        got = d.query_transforms()
    assert isinstance(got, Transforms)
    assert got.table_full is False
    assert len(got.entries) == 3
    assert got.entries[0].op == TransformOp.SWAP
    assert got.entries[0].source.kind == LockTargetKind.X
    assert got.entries[0].dest.kind == LockTargetKind.Y
    assert got.entries[1].op == TransformOp.REMAP
    assert got.entries[1].source.kind == LockTargetKind.WHEEL
    assert got.entries[1].dest.kind == LockTargetKind.Y
    assert got.entries[2].op == TransformOp.REMAP
    assert got.entries[2].source.kind == LockTargetKind.Y
    assert got.entries[2].dest.kind == LockTargetKind.X


def test_the_op_bytes_are_the_ones_the_wire_uses():
    # A disagreement here is a wrong transform on the wire, not a type error, because `op` crosses
    # the ABI as a plain byte. Weighing is the lock's, so there is no scale op and no invert.
    assert (int(TransformOp.REMAP), int(TransformOp.SWAP)) == (0, 1)
    assert not hasattr(TransformOp, "SCALE")
    assert not hasattr(TransformOp, "INVERT")
    assert not hasattr(Transform.swap(Axis.X, Axis.Y), "scale")


def test_a_pan_axis_transform_is_first_class():
    with MockBox() as mock:
        mock.set_mouse_caps(
            MouseCaps(
                n_buttons=5,
                has_x=True,
                has_y=True,
                has_wheel=True,
                pan=True,
                has_report_id=False,
                n_hid=1,
            )
        )
        with Device.with_mock(mock) as d:
            assert d.caps().mouse.pan is True
            d.transform_remap(Axis.WHEEL, Axis.PAN)
            got = d.query_transforms()
    assert len(got.entries) == 1
    assert got.entries[0].op == TransformOp.REMAP
    assert got.entries[0].dest.kind == LockTargetKind.PAN


def test_transform_validation_errors_have_their_own_exception():
    with MockBox() as mock, Device.with_mock(mock) as d:
        # Remap cannot move an axis into a usage.
        with pytest.raises(TransformOpFieldsError):
            d.transform(
                Transform(TransformOp.REMAP, LockTarget.x(), LockTarget.button(Button.LEFT))
            )
        # Both ops MOVE a value, so a field onto itself names no operation at all.
        with pytest.raises(TransformOpFieldsError):
            d.transform(Transform(TransformOp.SWAP, LockTarget.y(), LockTarget.y()))
        with pytest.raises(TransformOpFieldsError):
            d.transform(
                Transform(
                    TransformOp.REMAP,
                    LockTarget.button(Button.LEFT),
                    LockTarget.button(Button.LEFT),
                )
            )


def test_a_negative_lock_scale_reverses_and_is_refused_where_it_cannot():
    x = LockTarget.x()
    with MockBox() as mock, Device.with_mock(mock) as d:
        d.scale(x, Direction.BOTH, -100)
        locks = d.query_locks()
        assert locks.scale_of(x, Direction.POSITIVE) == -100
        assert locks.scale_of(x, Direction.NEGATIVE) == -100
        # A reversal is not a block: everything still arrives, the other way round.
        assert not locks.is_locked(x, Direction.BOTH)
        # One bit has nothing to reverse, and a magnitude past the bound is refused rather than
        # applied at the bound with the readback echoing what was sent. Neither reaches the wire.
        before = mock.recorded()
        with pytest.raises(medius.LockScaleUsageError):
            d.scale(LockTarget.button(Button.LEFT), Direction.POSITIVE, -100)
        with pytest.raises(medius.LockScaleRangeError):
            d.scale(x, Direction.BOTH, LOCK_SCALE_MIN - 1)
        with pytest.raises(medius.LockScaleRangeError):
            d.scale(x, Direction.BOTH, LOCK_SCALE_MAX + 1)
        assert mock.recorded() == before


def test_input_event_carries_pan():
    with MockBox() as mock, Device.with_mock(mock) as d:
        with d.input_events(CatchFilter.all_input()) as s:
            mock.push_motion(1, 4_000, MotionEvent(dx=3, dy=-4, dz=0, pan=5))
            ev = s.recv_timeout(2000)
    assert ev is not None
    assert ev.kind == InputKind.MOTION
    assert (ev.dx, ev.dy, ev.dz, ev.pan) == (3, -4, 0, 5)
