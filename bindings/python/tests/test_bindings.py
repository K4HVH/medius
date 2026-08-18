"""Mock-backed tests for the Python bindings."""

import gc
import pathlib
import subprocess
import tempfile
import time

import pytest

import medius
from medius import (
    Axis,
    BadProtoVerError,
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
    ClipAction,
    ClipBuilder,
    ClipSettings,
    ClipState,
    ClipStatus,
    ClipTrigger,
    ClockDomain,
    ClockEstimate,
    ControlStatus,
    Edge,
    LOCK_SCALE_BLOCK,
    LOCK_SCALE_MAX,
    LOCK_SCALE_PASS,
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
)


def test_mock_feature_present():
    assert medius.HAS_MOCK, "tests need a mock-enabled libmedius_capi"


def test_meta_functions():
    # These are a hand-written mirror of the C structs, so a bumped ABI means they are stale until
    # someone re-reads the header. Pin it rather than accept anything newer.
    assert medius.abi_version() == 4
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
        # No host-side validation (like the other setters): the box sanitizes. These all just send.
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
        mouse=MouseCaps(n_buttons=5, has_x=True, has_y=True, has_wheel=True, has_report_id=False, n_hid=2),
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
        d.set_bearing(20, BearingMode.VECTOR)
        d.set_bearing(None)
        bearing = d.query_bearing()
    # The mock answers the default, which is what the box boots holding.
    assert bearing.mode == BearingMode.PER_AXIS


def test_relative_directions_report_themselves():
    assert Direction.WITH.is_relative and Direction.AGAINST.is_relative
    assert not Direction.BOTH.is_relative
    assert not Direction.POSITIVE.is_relative and not Direction.NEGATIVE.is_relative
    assert (int(Direction.WITH), int(Direction.AGAINST)) == (3, 4)
    assert (LOCK_SCALE_BLOCK, LOCK_SCALE_PASS, LOCK_SCALE_MAX) == (0, 100, 255)


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
                CatchFilter.traffic(TrafficClass.VENDOR_BULK, 0x83).with_direction(
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
    assert got.entries[1].filter.id == 0x83


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
        mock.set_emit_pace(EmitPace.fixed(500))
        with Device.with_mock(mock) as d:
            status = d.query_emit_pace()
        assert status.mode == EmitPace.fixed(500)
        assert status.resolved_hz == 500  # Fixed clamps to its hz
        mock.set_emit_pace(EmitPace.learned())
        with Device.with_mock(mock) as d:
            status = d.query_emit_pace()
        assert status.mode == EmitPace.learned()
        assert status.resolved_hz == 0  # learnt/adaptive


def test_counters_readable():
    with MockBox() as mock, Device.with_mock(mock) as d:
        d.move_rel(1, 0)
        c = d.counters()
        assert c.frames_tx >= 1


def test_catch_delivers_motion_event():
    with MockBox() as mock, Device.with_mock(mock) as d:
        with d.catch_events(CatchFilter.everything()) as stream:
            mock.push_motion(1, 7_000, MotionEvent(dx=12, dy=-34, dz=1))
            ev = stream.recv_timeout(2000)
            assert ev is not None
            assert ev.kind == CatchEventKind.MOTION
            assert ev.ts_us == 7_000
            assert ev.motion.dx == 12
            assert ev.motion.dy == -34
            assert ev.motion.dz == 1


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
        id=0x83,
        direction=Direction.POSITIVE,
        flags=0x03,
        true_len=512,
        bytes=bytes(range(16)),
    )
    with MockBox() as mock, Device.with_mock(mock) as d:
        with d.catch_events(CatchFilter.traffic(TrafficClass.VENDOR_BULK, 0x83).with_capture(16)) as s:
            ev = _push_and_recv(mock, s, cut)
    assert ev.traffic.true_len == 512
    assert len(ev.traffic.bytes) == 16
    assert ev.traffic.truncated()
    assert ev.traffic.bulk_end_of_transfer()
    assert ev.traffic.bulk_zlp()

    whole = TrafficEvent(CatchClass.VENDOR_BULK, 0x83, Direction.POSITIVE, 0, 16, bytes(16))
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
        b.frame(1, 2, -1, [(Usage.button(Button.LEFT), Action.PRESS), (Usage.key(0x04), Action.PRESS)])
        d.clip().append(b)
        b.close()
        appends = _clip_frames(d, mock, FrameType.CLIP_APPEND)
    # flags XY|WHEEL|EDGES=0x07, dx=1 dy=2, wheel=-1, n=2, [btn left press][key 0x04 press]
    assert appends[0] == bytes(
        [0x07, 0x01, 0x00, 0x02, 0x00, 0xFF, 0xFF, 0x02, 0x00, 0x00, 0x00, 0x01, 0x01, 0x04, 0x00, 0x01]
    )


def test_clip_status_and_config_roundtrip():
    status = ClipStatus(
        ClipState.PLAYING, free=512, total=40, played=8, ticks=99, underruns=2, overruns=0,
        seq_gaps=1, held=[Usage.button(Button.SIDE1), Usage.key(Key.A)],
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
    )
    with MockBox() as mock:
        mock.set_clip_status(status)
        mock.set_clip_settings(settings)
        with Device.with_mock(mock) as d:
            got = d.clip().query_status()
            cfg = d.clip().query_config()
    assert got == status
    assert got.state == ClipState.PLAYING
    assert got.is_held(Usage.button(Button.SIDE1))
    assert got.is_held(Usage.key(Key.A))
    assert not got.is_held(Usage.button(Button.LEFT))
    assert cfg == settings


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
    missed here is silent until it corrupts the heap -- which is exactly how it happened.
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
    probe = pathlib.Path(tempfile.mkdtemp()) / "sizes.c"
    names = [
        "MediusUsage",
        "MediusMotionEvent",
        "MediusUsageEvent",
        "MediusTrafficEvent",
        "MediusCatchEvent",
        "MediusClockEstimate",
        "MediusCatchEntry",
        "MediusCatchFilter",
        "MediusInputEvent",
        "MediusStamped",
        # A struct whose LAYOUT this repo has changed: `ride` was inserted mid-struct, moving `triggers`
        # and `n`, and medius_clip_query_config writes sizeof(MediusClipSettings) into the mirror.
        "MediusClipSettings",
        "MediusClipTrigger",
    ]
    present = [n for n in names if re.search(rf"\}} {n};", text)]
    # sizeof alone lets a same-size field REORDER through, which is a silent misread of every event
    # rather than a crash. Compare each field's offset too.
    fields = {
        n: [f[0] for f in getattr(_native, n)._fields_]
        for n in present
        if hasattr(_native, n)
    }
    lines = [f'    printf("%s %zu\\n", "{n}", sizeof(struct {n}));' for n in present]
    for n, fs in fields.items():
        for f in fs:
            cname = "class_" if f == "class_" else f
            lines.append(
                f'    printf("%s.%s %zu\\n", "{n}", "{f}", offsetof(struct {n}, {cname}));'
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
    assert checked > 40, f"the probe only compared {checked} things; it stopped covering the structs"


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
        # exact id apart from the blanket -- so across this ABI a media usage of 0xFFFF IS the class
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
    bulk = CatchFilter.traffic(TrafficClass.VENDOR_BULK, 0x83)
    assert bulk.same_address(bulk.with_capture(16))
    assert not bulk.same_address(bulk.outbound())
    assert bulk != bulk.with_capture(16)
    assert CatchFilter.everything().capture == Capture.WHOLE
    assert CatchClass.KEY.is_input() and CatchClass.VENDOR_BULK.is_traffic()


def test_an_unknown_control_status_does_not_raise():
    # The C ABI reports a status this build does not know as OTHER, and the byte itself stays on
    # `flags`. Without the member, decoding one raised ValueError -- the exact failure the distinct
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
