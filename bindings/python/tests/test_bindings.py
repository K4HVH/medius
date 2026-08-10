"""Mock-backed tests for the Python bindings."""

import gc

import pytest

import medius
from medius import (
    BadProtoVerError,
    Button,
    Caps,
    CatchEventKind,
    CatchMask,
    CatchState,
    Action,
    Blanket,
    ClipAction,
    ClipBuilder,
    ClipSettings,
    ClipState,
    ClipStatus,
    ClipTrigger,
    Edge,
    Usage,
    Device,
    EmitPace,
    FrameType,
    Health,
    ImperfectStatus,
    KbdCaps,
    Key,
    LockDirection,
    LockEntry,
    Locks,
    LockTarget,
    DeviceInfo,
    DeviceKind,
    LogLevel,
    MediaKey,
    MediusError,
    MockBox,
    MotionEvent,
    MouseCaps,
    Rate,
    Stats,
    Status,
    UsageSnapshot,
    Version,
)


def test_mock_feature_present():
    assert medius.HAS_MOCK, "tests need a mock-enabled libmedius_capi"


def test_meta_functions():
    assert medius.abi_version() >= 1
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
        mock.set_locks(Locks([LockEntry(x, is_blanket=False, positive=True, negative=True)]))
        with Device.with_mock(mock) as d:
            locks = d.query_locks()
    assert len(locks.entries) == 1
    assert locks.is_locked(x, LockDirection.BOTH)
    assert not locks.is_locked(LockTarget.y(), LockDirection.BOTH)


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


def test_catch_state_roundtrip():
    state = CatchState(mask=int(CatchMask.ALL), dropped=42)
    with MockBox() as mock:
        mock.set_catch_state(state)
        with Device.with_mock(mock) as d:
            got = d.query_catch()
    assert got.mask == int(CatchMask.ALL)
    assert got.dropped == 42


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
        with d.catch_events(CatchMask.ALL) as stream:
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
        with d.catch_events(CatchMask.KEYS) as stream:
            mock.push_usages(1, 7_000, UsageSnapshot([Usage.key(Key.ESCAPE)]))
            ev = stream.recv_timeout(2000)
            assert ev is not None
            assert ev.kind == CatchEventKind.USAGES
            assert ev.usages.is_held(Usage.key(Key.ESCAPE))
            assert not ev.usages.is_held(Usage.key(Key.A))


def test_catch_delivers_usage_event_for_media():
    with MockBox() as mock, Device.with_mock(mock) as d:
        with d.catch_events(CatchMask.ALL) as stream:
            mock.push_usages(1, 7_000, UsageSnapshot([Usage.media(MediaKey.VOLUME_UP)]))
            ev = stream.recv_timeout(2000)
            assert ev is not None
            assert ev.kind == CatchEventKind.USAGES
            assert ev.usages.is_held(Usage.media(MediaKey.VOLUME_UP))


def test_try_recv_returns_none_when_empty():
    with MockBox() as mock, Device.with_mock(mock) as d:
        with d.catch_events(CatchMask.ALL) as stream:
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
        with d.catch_events(CatchMask.ALL) as stream:
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
    stream = d.catch_events(CatchMask.ALL)
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
