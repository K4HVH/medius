// Mock-backed C++ wrapper tests. No hardware, no external framework.
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <optional>
#include <string>
#include <utility>

#include <medius/medius.hpp>

using namespace medius;

static int g_checks = 0;
static int g_failures = 0;

#define CHECK(cond)                                                              \
    do {                                                                         \
        ++g_checks;                                                              \
        if (!(cond)) {                                                           \
            ++g_failures;                                                        \
            std::fprintf(stderr, "CHECK failed: %s (%s:%d)\n", #cond, __FILE__,  \
                         __LINE__);                                              \
        }                                                                        \
    } while (0)

int odr_probe();

static Version make_version() {
    Version v{};
    v.proto_ver = 2; // the wire PROTO_VER the handshake expects
    v.fw_major = 9;
    v.fw_minor = 8;
    v.fw_patch = 7;
    return v;
}

static void test_query_version_roundtrip() {
    MockBox mock;
    mock.set_version(make_version());
    Device dev = Device::open_mock(mock);
    Version v = dev.query_version();
    CHECK(v.fw_major == 9);
    CHECK(v.fw_minor == 8);
    CHECK(v.fw_patch == 7);
}

static void test_commands_recorded() {
    MockBox mock;
    Device dev = Device::with_mock(mock);
    dev.move_rel(100, -50);
    dev.press(Button::Left);
    dev.soft_release(Button::Left);
    dev.key_down(MEDIUS_KEY_A);
    dev.lock(LockTarget::x(), LockDirection::Both);
    dev.led(LedTarget::Both, LedMode::Blink, 128);
    dev.reset();
    dev.set_movement_riding(std::chrono::milliseconds(5));
    dev.set_movement_riding(std::nullopt);

    CHECK(mock.saw(FrameType::Move));
    CHECK(mock.saw(FrameType::Inject));
    CHECK(mock.saw(FrameType::Lock));
    CHECK(mock.saw(FrameType::Led));
    CHECK(mock.saw(FrameType::Reset));
    CHECK(mock.saw(FrameType::Option));
    CHECK(mock.recorded() >= 8);
}

static void test_catch_mouse_event() {
    MockBox mock;
    Device dev = Device::with_mock(mock);
    EventStream stream = dev.catch_events(CatchMask::All);

    MediusMouseEvent report{};
    report.buttons = 1 << 3; // Side1
    report.dx = 12;
    report.dy = -34;
    report.wheel = 1;
    mock.push_event(1, report);

    std::optional<CatchEvent> ev = stream.recv_timeout(std::chrono::milliseconds(2000));
    CHECK(ev.has_value());
    if (ev) {
        CHECK(ev->kind() == CatchEventKind::Mouse);
        std::optional<MouseEvent> m = ev->mouse();
        CHECK(m.has_value());
        if (m) {
            CHECK(m->dx == 12);
            CHECK(m->dy == -34);
            CHECK(m->wheel == 1);
            CHECK(m->is_pressed(Button::Side1));
            CHECK(!m->is_pressed(Button::Left));
        }
        CHECK(!ev->keyboard().has_value());
        CHECK(!ev->media().has_value());
    }
}

static void test_catch_keyboard_event() {
    MockBox mock;
    Device dev = Device::with_mock(mock);
    EventStream stream = dev.catch_events(CatchMask::Keys);

    MediusKeyboardEvent kb{};
    kb.modifiers = 0;
    kb.n_keys = 1;
    kb.keys[0] = MEDIUS_KEY_ESCAPE;
    mock.push_kb_event(1, kb);

    std::optional<CatchEvent> ev = stream.recv_timeout(std::chrono::milliseconds(2000));
    CHECK(ev.has_value());
    if (ev) {
        CHECK(ev->kind() == CatchEventKind::Keyboard);
        std::optional<KeyboardEvent> k = ev->keyboard();
        CHECK(k.has_value());
        if (k) {
            CHECK(k->is_pressed(MEDIUS_KEY_ESCAPE));
            CHECK(!k->is_pressed(MEDIUS_KEY_A));
            CHECK(k->key_list().size() == 1);
        }
    }
}

static void test_log_stream() {
    MockBox mock;
    Device dev = Device::with_mock(mock);
    LogStream logs = dev.logs();
    mock.push_log(LogLevel::Warn, "hello world");
    std::optional<LogLine> line = logs.recv_timeout(std::chrono::milliseconds(2000));
    CHECK(line.has_value());
    if (line) {
        CHECK(line->level == LogLevel::Warn);
        CHECK(line->text == "hello world");
    }
}

static void test_silent_mock_throws() {
    MockBox mock;
    mock.silent();
    bool threw = false;
    try {
        Device dev = Device::open_mock(mock);
        (void)dev;
    } catch (const Error& e) {
        threw = true;
        CHECK(e.status() != Status::Ok);
        CHECK(std::string(e.what()).size() > 0);
    }
    CHECK(threw);
}

static void test_query_helpers() {
    MockBox mock;
    MediusRate rate{};
    rate.native_period_us = 1000;
    rate.poll_period_us = 1000;
    rate.confident = 1;
    rate.change_driven = 0;
    mock.set_rate(rate);

    MediusLocks locks{};
    locks.mask = 0b11; // X positive + negative
    mock.set_locks(locks);

    MediusCaps caps{};
    caps.mouse.n_buttons = 5;
    caps.mouse.has_x = 1;
    caps.mouse.has_y = 1;
    caps.mouse.has_wheel = 1;
    caps.mouse.n_hid = 2;
    caps.keyboard.n_keys = 6;
    mock.set_caps(caps);

    Device dev = Device::with_mock(mock);

    std::optional<float> hz = dev.query_rate().native_hz();
    CHECK(hz.has_value());
    if (hz) {
        CHECK(*hz > 999.0f && *hz < 1001.0f);
    }

    Locks l = dev.query_locks();
    CHECK(l.is_locked(LockTarget::x(), LockDirection::Both));
    CHECK(!l.is_locked(LockTarget::y(), LockDirection::Both));

    Caps c = dev.caps();
    CHECK(c.has_mouse());
    CHECK(c.has_keyboard());
    CHECK(c.is_composite());

    std::optional<std::chrono::milliseconds> mr = dev.query_movement_riding();
    CHECK(!mr.has_value());
    mock.set_movement_riding(true, 5);
    std::optional<std::chrono::milliseconds> mr2 = dev.query_movement_riding();
    CHECK(mr2.has_value());
    if (mr2) {
        CHECK(mr2->count() == 5);
    }

    dev.move_rel(1, 0);
    CountersSnapshot counters = dev.counters();
    CHECK(counters.frames_tx >= 1);
}

static void test_raii_and_move() {
    MockBox mock;
    {
        Device dev = Device::with_mock(mock);
        dev.move_rel(1, 1);
        Device moved = std::move(dev);
        moved.move_rel(2, 2);
        // dev is moved-from; its destructor must be a no-op.
    }
    {
        Device dev = Device::with_mock(mock);
        {
            EventStream s1 = dev.catch_events(CatchMask::All);
            EventStream s2 = std::move(s1);
            (void)s2.dropped();
        }
        {
            LogStream l = dev.logs();
            std::optional<LogLine> none = l.try_recv();
            CHECK(!none.has_value());
        }
    }
    CHECK(true);
}

static void test_recorded_frame() {
    MockBox mock;
    Device dev = Device::with_mock(mock);
    dev.move_rel(1, 2);
    CHECK(mock.recorded() == 1);
    CHECK(mock.saw(FrameType::Move));

    MediusFrameType ty = MEDIUS_FRAME_TYPE_RESET;
    uint8_t seq = 0;
    uint8_t payload[64] = {0};
    size_t len = mock.recorded_frame(0, &ty, &seq, payload, sizeof(payload));
    CHECK(ty == MEDIUS_FRAME_TYPE_MOVE);
    CHECK(len > 0);
}

static void test_clone_shares_state() {
    MockBox mock;
    Device dev = Device::with_mock(mock);
    Device dev2 = dev.clone();
    dev.move_rel(1, 0);
    dev2.move_rel(2, 0);
    MockBox mock2 = mock.clone();
    CHECK(mock2.recorded() == 2);

    EventStream s = dev.catch_events(CatchMask::All);
    EventStream s2 = s.clone();
    mock.push_event(1, MediusMouseEvent{0, 7, 0, 0});
    auto ev = s2.recv_timeout(std::chrono::milliseconds(2000));
    CHECK(ev.has_value());
    if (ev) CHECK(ev->mouse().has_value() && ev->mouse()->dx == 7);
}

static void test_meta() {
    CHECK(abi_version() >= 1);
    CHECK(version_string().size() > 0);
    CHECK(default_query_timeout().count() > 0);
    CHECK(default_keepalive_cadence().count() > 0);
    std::vector<PortInfo> ports = find_ports(); // may be empty; must not crash
    (void)ports;
}

int main() {
    test_query_version_roundtrip();
    test_commands_recorded();
    test_catch_mouse_event();
    test_catch_keyboard_event();
    test_log_stream();
    test_silent_mock_throws();
    test_query_helpers();
    test_raii_and_move();
    test_recorded_frame();
    test_clone_shares_state();
    test_meta();

    CHECK(odr_probe() == 1);

    std::printf("%d checks, %d failures\n", g_checks, g_failures);
    return g_failures == 0 ? 0 : 1;
}
