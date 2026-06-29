// medius C++ wrapper. Header-only, C++17, exceptions. Rides the C ABI in medius.h.
// See https://github.com/K4HVH/medius. MIT.
#ifndef MEDIUS_HPP
#define MEDIUS_HPP

#include <chrono>
#include <cstddef>
#include <cstdint>
#include <optional>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

#include <medius.h>

namespace medius {

enum class Status : int32_t {
    Ok = MEDIUS_STATUS_OK,
    ErrIo = MEDIUS_STATUS_ERR_IO,
    ErrNotFound = MEDIUS_STATUS_ERR_NOT_FOUND,
    ErrNoReply = MEDIUS_STATUS_ERR_NO_REPLY,
    ErrBadProtoVer = MEDIUS_STATUS_ERR_BAD_PROTO_VER,
    ErrQueryTimeout = MEDIUS_STATUS_ERR_QUERY_TIMEOUT,
    ErrDisconnected = MEDIUS_STATUS_ERR_DISCONNECTED,
    ErrFrameTooLong = MEDIUS_STATUS_ERR_FRAME_TOO_LONG,
    ErrFlashTool = MEDIUS_STATUS_ERR_FLASH_TOOL,
    ErrInvalidArg = MEDIUS_STATUS_ERR_INVALID_ARG,
    ErrPanic = MEDIUS_STATUS_ERR_PANIC,
    ErrUnknown = MEDIUS_STATUS_ERR_UNKNOWN,
};

enum class Button : uint8_t {
    Left = MEDIUS_BUTTON_LEFT,
    Right = MEDIUS_BUTTON_RIGHT,
    Middle = MEDIUS_BUTTON_MIDDLE,
    Side1 = MEDIUS_BUTTON_SIDE1,
    Side2 = MEDIUS_BUTTON_SIDE2,
};

enum class Action : uint8_t {
    SoftRelease = MEDIUS_ACTION_SOFT_RELEASE,
    Press = MEDIUS_ACTION_PRESS,
    ForceRelease = MEDIUS_ACTION_FORCE_RELEASE,
};

enum class LockDirection : uint8_t {
    Both = MEDIUS_LOCK_DIRECTION_BOTH,
    Positive = MEDIUS_LOCK_DIRECTION_POSITIVE,
    Negative = MEDIUS_LOCK_DIRECTION_NEGATIVE,
};

enum class Blanket : uint8_t {
    Keys = MEDIUS_BLANKET_KEYS,
    Media = MEDIUS_BLANKET_MEDIA,
    Buttons = MEDIUS_BLANKET_BUTTONS,
};

enum class LedTarget : uint8_t {
    Device = MEDIUS_LED_TARGET_DEVICE,
    Host = MEDIUS_LED_TARGET_HOST,
    Both = MEDIUS_LED_TARGET_BOTH,
};

enum class LedMode : uint8_t {
    Auto = MEDIUS_LED_MODE_AUTO,
    Off = MEDIUS_LED_MODE_OFF,
    Solid = MEDIUS_LED_MODE_SOLID,
    Blink = MEDIUS_LED_MODE_BLINK,
};

enum class RebootTarget : uint8_t {
    DeviceDownload = MEDIUS_REBOOT_TARGET_DEVICE_DOWNLOAD,
    HostDownload = MEDIUS_REBOOT_TARGET_HOST_DOWNLOAD,
    DeviceRun = MEDIUS_REBOOT_TARGET_DEVICE_RUN,
    HostRun = MEDIUS_REBOOT_TARGET_HOST_RUN,
};

enum class LogLevel : uint8_t {
    Error = MEDIUS_LOG_LEVEL_ERROR,
    Warn = MEDIUS_LOG_LEVEL_WARN,
    Info = MEDIUS_LOG_LEVEL_INFO,
    Debug = MEDIUS_LOG_LEVEL_DEBUG,
    Verbose = MEDIUS_LOG_LEVEL_VERBOSE,
};

enum class CatchEventKind : uint8_t {
    Mouse = MEDIUS_CATCH_EVENT_KIND_MOUSE,
    Keyboard = MEDIUS_CATCH_EVENT_KIND_KEYBOARD,
    Media = MEDIUS_CATCH_EVENT_KIND_MEDIA,
};

enum class FrameType : uint8_t {
    Move = MEDIUS_FRAME_TYPE_MOVE,
    Inject = MEDIUS_FRAME_TYPE_INJECT,
    Reset = MEDIUS_FRAME_TYPE_RESET,
    Query = MEDIUS_FRAME_TYPE_QUERY,
    Resp = MEDIUS_FRAME_TYPE_RESP,
    RebootDl = MEDIUS_FRAME_TYPE_REBOOT_DL,
    Log = MEDIUS_FRAME_TYPE_LOG,
    Led = MEDIUS_FRAME_TYPE_LED,
    Lock = MEDIUS_FRAME_TYPE_LOCK,
    Catch = MEDIUS_FRAME_TYPE_CATCH,
    MouseEvent = MEDIUS_FRAME_TYPE_MOUSE_EVENT,
    KbEvent = MEDIUS_FRAME_TYPE_KB_EVENT,
    ConsEvent = MEDIUS_FRAME_TYPE_CONS_EVENT,
    Option = MEDIUS_FRAME_TYPE_OPTION,
};

/// CATCH subscription class flags. OR them together.
enum class CatchMask : uint8_t {
    Motion = MEDIUS_CATCH_MASK_MOTION,
    Wheel = MEDIUS_CATCH_MASK_WHEEL,
    Buttons = MEDIUS_CATCH_MASK_BUTTONS,
    Keys = MEDIUS_CATCH_MASK_KEYS,
    All = MEDIUS_CATCH_MASK_ALL,
};

constexpr CatchMask operator|(CatchMask a, CatchMask b) {
    return static_cast<CatchMask>(static_cast<uint8_t>(a) | static_cast<uint8_t>(b));
}

using Key = MediusKey;
using MediaKey = MediusMediaKey;
using Version = MediusVersion;
using Health = MediusHealth;
using MouseInfo = MediusMouseInfo;
using MouseCaps = MediusMouseCaps;
using KbdCaps = MediusKbdCaps;
using Stats = MediusStats;
using CatchState = MediusCatchState;
using ImperfectStatus = MediusImperfectStatus;
using CountersSnapshot = MediusCountersSnapshot;
using PortInfo = MediusPortInfo;

/// A failed C ABI call. Carries the status code and the thread-local last-error text.
class Error : public std::runtime_error {
public:
    explicit Error(Status status)
        : std::runtime_error(fetch_message()),
          status_(status),
          proto_ver_(medius_last_error_proto_ver()) {}

    Status status() const noexcept { return status_; }

    /// The BadProtoVer byte, or 0 if the last error carried none.
    uint8_t proto_ver() const noexcept { return proto_ver_; }

private:
    static std::string fetch_message() {
        size_t len = medius_last_error_message(nullptr, 0);
        if (len == 0) {
            return std::string();
        }
        std::string s(len, '\0');
        medius_last_error_message(s.data(), len + 1);
        return s;
    }

    Status status_;
    uint8_t proto_ver_;
};

inline void check(MediusStatus st) {
    if (st != MEDIUS_STATUS_OK) {
        throw Error(static_cast<Status>(st));
    }
}

/// One physical mouse report. `buttons` is a bitmask by button id.
struct MouseEvent : MediusMouseEvent {
    MouseEvent() : MediusMouseEvent{} {}
    MouseEvent(const MediusMouseEvent& c) : MediusMouseEvent(c) {}
    bool is_pressed(Button b) const {
        return medius_mouse_event_is_pressed(this, static_cast<MediusButton>(b));
    }
};

/// One physical keyboard snapshot: a modifier bitmap plus the pressed non-modifier keycodes.
struct KeyboardEvent : MediusKeyboardEvent {
    KeyboardEvent() : MediusKeyboardEvent{} {}
    KeyboardEvent(const MediusKeyboardEvent& c) : MediusKeyboardEvent(c) {}
    bool is_pressed(Key k) const { return medius_keyboard_event_is_pressed(this, k); }
    std::vector<Key> key_list() const { return std::vector<Key>(keys, keys + n_keys); }
};

/// One physical media snapshot: the active Consumer usages.
struct MediaEvent : MediusMediaEvent {
    MediaEvent() : MediusMediaEvent{} {}
    MediaEvent(const MediusMediaEvent& c) : MediusMediaEvent(c) {}
    bool is_pressed(MediaKey m) const { return medius_media_event_is_pressed(this, m); }
    std::vector<MediaKey> key_list() const { return std::vector<MediaKey>(keys, keys + n_keys); }
};

/// The whole cloned device's capabilities.
struct Caps : MediusCaps {
    Caps() : MediusCaps{} {}
    Caps(const MediusCaps& c) : MediusCaps(c) {}
    bool has_mouse() const { return medius_caps_has_mouse(*this); }
    bool has_keyboard() const { return medius_caps_has_keyboard(*this); }
    bool is_composite() const { return medius_caps_is_composite(*this); }
};

/// The live native report rate and clone poll period.
struct Rate : MediusRate {
    Rate() : MediusRate{} {}
    Rate(const MediusRate& c) : MediusRate(c) {}
    /// The native report rate in Hz, or nullopt when there is no continuous cadence.
    std::optional<float> native_hz() const {
        float hz = 0.0f;
        return medius_rate_native_hz(*this, &hz) ? std::optional<float>(hz) : std::nullopt;
    }
};

/// A lock target. `button` is meaningful only when kind is Button.
struct LockTarget : MediusLockTarget {
    LockTarget(const MediusLockTarget& c) : MediusLockTarget(c) {}
    static LockTarget x() { return MediusLockTarget{MEDIUS_LOCK_TARGET_KIND_X, MEDIUS_BUTTON_LEFT}; }
    static LockTarget y() { return MediusLockTarget{MEDIUS_LOCK_TARGET_KIND_Y, MEDIUS_BUTTON_LEFT}; }
    static LockTarget wheel() {
        return MediusLockTarget{MEDIUS_LOCK_TARGET_KIND_WHEEL, MEDIUS_BUTTON_LEFT};
    }
    static LockTarget button(Button b) {
        return MediusLockTarget{MEDIUS_LOCK_TARGET_KIND_BUTTON, static_cast<MediusButton>(b)};
    }
};

/// The active lock bitmask.
struct Locks : MediusLocks {
    Locks() : MediusLocks{} {}
    Locks(const MediusLocks& c) : MediusLocks(c) {}
    bool is_locked(LockTarget target, LockDirection dir) const {
        return medius_locks_is_locked(*this, target, static_cast<MediusLockDirection>(dir));
    }
};

/// A relative-axis drive for move_axis.
struct Motion : MediusMotion {
    Motion(const MediusMotion& c) : MediusMotion(c) {}
    static Motion cursor(int16_t dx, int16_t dy) { return medius_motion_cursor(dx, dy); }
    static Motion wheel(int16_t delta) { return medius_motion_wheel(delta); }
};

/// A momentary usage for inject (a button id, key usage, or media usage).
struct Input : MediusInput {
    Input(const MediusInput& c) : MediusInput(c) {}
    static Input button(Button b) { return medius_input_button(static_cast<MediusButton>(b)); }
    static Input key(Key k) { return medius_input_key(k); }
    static Input media(MediaKey m) { return medius_input_media(m); }
};

/// One catch-stream event. Read the arm matching kind().
class CatchEvent {
public:
    CatchEvent() = default;
    explicit CatchEvent(const MediusCatchEvent& c) : raw_(c) {}

    CatchEventKind kind() const { return static_cast<CatchEventKind>(raw_.kind); }

    std::optional<MouseEvent> mouse() const {
        if (raw_.kind != MEDIUS_CATCH_EVENT_KIND_MOUSE) {
            return std::nullopt;
        }
        return MouseEvent(raw_.data.mouse);
    }
    std::optional<KeyboardEvent> keyboard() const {
        if (raw_.kind != MEDIUS_CATCH_EVENT_KIND_KEYBOARD) {
            return std::nullopt;
        }
        return KeyboardEvent(raw_.data.keyboard);
    }
    std::optional<MediaEvent> media() const {
        if (raw_.kind != MEDIUS_CATCH_EVENT_KIND_MEDIA) {
            return std::nullopt;
        }
        return MediaEvent(raw_.data.media);
    }

    const MediusCatchEvent& raw() const noexcept { return raw_; }

private:
    MediusCatchEvent raw_{};
};

/// One device log line.
struct LogLine {
    LogLevel level{};
    std::string text;
};

/// A live CATCH event stream. Move-only; unsubscribes on destruction.
class EventStream {
public:
    EventStream(EventStream&& o) noexcept : h_(o.h_) { o.h_ = nullptr; }
    EventStream& operator=(EventStream&& o) noexcept {
        if (this != &o) {
            reset();
            h_ = o.h_;
            o.h_ = nullptr;
        }
        return *this;
    }
    EventStream(const EventStream&) = delete;
    EventStream& operator=(const EventStream&) = delete;
    ~EventStream() { reset(); }

    /// Block for the next event. Throws Error(ErrDisconnected) when the stream closes.
    CatchEvent recv() {
        MediusCatchEvent c{};
        check(medius_event_stream_recv(h_, &c));
        return CatchEvent(c);
    }
    /// The next buffered event, or nullopt if none is queued (never blocks).
    std::optional<CatchEvent> try_recv() {
        MediusCatchEvent c{};
        if (!medius_event_stream_try_recv(h_, &c)) {
            return std::nullopt;
        }
        return CatchEvent(c);
    }
    /// Block up to `timeout` for the next event; nullopt on timeout or close.
    std::optional<CatchEvent> recv_timeout(std::chrono::milliseconds timeout) {
        MediusCatchEvent c{};
        if (!medius_event_stream_recv_timeout(
                h_, static_cast<uint64_t>(timeout.count()), &c)) {
            return std::nullopt;
        }
        return CatchEvent(c);
    }
    /// Events dropped because the consumer fell behind.
    uint64_t dropped() const { return medius_event_stream_dropped(h_); }

    class iterator {
    public:
        using iterator_category = std::input_iterator_tag;
        using value_type = CatchEvent;
        using difference_type = std::ptrdiff_t;
        using pointer = CatchEvent*;
        using reference = CatchEvent&;
        iterator() = default;
        explicit iterator(EventStream* s) : s_(s) { advance(); }
        reference operator*() { return *cur_; }
        pointer operator->() { return &*cur_; }
        iterator& operator++() {
            advance();
            return *this;
        }
        bool operator!=(const iterator& o) const { return (s_ != nullptr) != (o.s_ != nullptr); }
        bool operator==(const iterator& o) const { return !(*this != o); }

    private:
        void advance() {
            if (s_ == nullptr) {
                return;
            }
            cur_ = s_->next_or_end();
            if (!cur_) {
                s_ = nullptr;
            }
        }
        EventStream* s_ = nullptr;
        std::optional<CatchEvent> cur_;
    };
    iterator begin() { return iterator(this); }
    iterator end() { return iterator(); }

    MediusEventStream* raw() const noexcept { return h_; }

private:
    friend class Device;
    explicit EventStream(MediusEventStream* h) : h_(h) {}
    void reset() noexcept {
        if (h_) {
            medius_event_stream_free(h_);
            h_ = nullptr;
        }
    }
    std::optional<CatchEvent> next_or_end() {
        MediusCatchEvent c{};
        MediusStatus st = medius_event_stream_recv(h_, &c);
        if (st == MEDIUS_STATUS_OK) {
            return CatchEvent(c);
        }
        if (st == MEDIUS_STATUS_ERR_DISCONNECTED) {
            return std::nullopt;
        }
        throw Error(static_cast<Status>(st));
    }
    MediusEventStream* h_ = nullptr;
};

/// A device LOG stream. Move-only.
class LogStream {
public:
    LogStream(LogStream&& o) noexcept : h_(o.h_) { o.h_ = nullptr; }
    LogStream& operator=(LogStream&& o) noexcept {
        if (this != &o) {
            reset();
            h_ = o.h_;
            o.h_ = nullptr;
        }
        return *this;
    }
    LogStream(const LogStream&) = delete;
    LogStream& operator=(const LogStream&) = delete;
    ~LogStream() { reset(); }

    /// Block for the next line. Throws Error(ErrDisconnected) on close.
    LogLine recv() {
        MediusLogLine c{};
        check(medius_log_stream_recv(h_, &c));
        return from_c(c);
    }
    std::optional<LogLine> try_recv() {
        MediusLogLine c{};
        if (!medius_log_stream_try_recv(h_, &c)) {
            return std::nullopt;
        }
        return from_c(c);
    }
    std::optional<LogLine> recv_timeout(std::chrono::milliseconds timeout) {
        MediusLogLine c{};
        if (!medius_log_stream_recv_timeout(h_, static_cast<uint64_t>(timeout.count()), &c)) {
            return std::nullopt;
        }
        return from_c(c);
    }

    class iterator {
    public:
        using iterator_category = std::input_iterator_tag;
        using value_type = LogLine;
        using difference_type = std::ptrdiff_t;
        using pointer = LogLine*;
        using reference = LogLine&;
        iterator() = default;
        explicit iterator(LogStream* s) : s_(s) { advance(); }
        reference operator*() { return *cur_; }
        pointer operator->() { return &*cur_; }
        iterator& operator++() {
            advance();
            return *this;
        }
        bool operator!=(const iterator& o) const { return (s_ != nullptr) != (o.s_ != nullptr); }
        bool operator==(const iterator& o) const { return !(*this != o); }

    private:
        void advance() {
            if (s_ == nullptr) {
                return;
            }
            cur_ = s_->next_or_end();
            if (!cur_) {
                s_ = nullptr;
            }
        }
        LogStream* s_ = nullptr;
        std::optional<LogLine> cur_;
    };
    iterator begin() { return iterator(this); }
    iterator end() { return iterator(); }

    MediusLogStream* raw() const noexcept { return h_; }

private:
    friend class Device;
    explicit LogStream(MediusLogStream* h) : h_(h) {}
    void reset() noexcept {
        if (h_) {
            medius_log_stream_free(h_);
            h_ = nullptr;
        }
    }
    static LogLine from_c(const MediusLogLine& c) {
        return LogLine{static_cast<LogLevel>(c.level), std::string(c.text)};
    }
    std::optional<LogLine> next_or_end() {
        MediusLogLine c{};
        MediusStatus st = medius_log_stream_recv(h_, &c);
        if (st == MEDIUS_STATUS_OK) {
            return from_c(c);
        }
        if (st == MEDIUS_STATUS_ERR_DISCONNECTED) {
            return std::nullopt;
        }
        throw Error(static_cast<Status>(st));
    }
    MediusLogStream* h_ = nullptr;
};

#ifdef MEDIUS_FEATURE_MOCK
/// A scriptable fake box for tests without hardware. Move-only.
class MockBox {
public:
    MockBox() : h_(medius_mock_new()) {}
    MockBox(MockBox&& o) noexcept : h_(o.h_) { o.h_ = nullptr; }
    MockBox& operator=(MockBox&& o) noexcept {
        if (this != &o) {
            reset();
            h_ = o.h_;
            o.h_ = nullptr;
        }
        return *this;
    }
    MockBox(const MockBox&) = delete;
    MockBox& operator=(const MockBox&) = delete;
    ~MockBox() { reset(); }

    void set_version(Version v) { medius_mock_set_version(h_, v); }
    void set_health(Health v) { medius_mock_set_health(h_, v); }
    void set_mouse_info(MouseInfo v) { medius_mock_set_mouse_info(h_, v); }
    void set_caps(MediusCaps v) { medius_mock_set_caps(h_, v); }
    void set_mouse_caps(MouseCaps v) { medius_mock_set_mouse_caps(h_, v); }
    void set_kbd_caps(KbdCaps v) { medius_mock_set_kbd_caps(h_, v); }
    void set_rate(MediusRate v) { medius_mock_set_rate(h_, v); }
    void set_stats(Stats v) { medius_mock_set_stats(h_, v); }
    void set_locks(MediusLocks v) { medius_mock_set_locks(h_, v); }
    void set_catch_state(CatchState v) { medius_mock_set_catch_state(h_, v); }
    void set_imperfect_status(ImperfectStatus v) { medius_mock_set_imperfect_status(h_, v); }
    void set_movement_riding(bool enabled, uint32_t window_ms) {
        medius_mock_set_movement_riding(h_, enabled, window_ms);
    }
    /// Make the mock unresponsive to queries (one-way; for testing timeouts).
    void silent() { medius_mock_silent(h_); }

    void push_raw(const uint8_t* bytes, size_t len) { medius_mock_push_raw(h_, bytes, len); }
    void push_log(LogLevel level, const std::string& text) {
        medius_mock_push_log(h_, static_cast<MediusLogLevel>(level), text.c_str());
    }
    void push_event(uint8_t seq, MediusMouseEvent report) {
        medius_mock_push_event(h_, seq, report);
    }
    void push_kb_event(uint8_t seq, const MediusKeyboardEvent& e) {
        medius_mock_push_kb_event(h_, seq, &e);
    }
    void push_cons_event(uint8_t seq, const MediusMediaEvent& e) {
        medius_mock_push_cons_event(h_, seq, &e);
    }

    size_t recorded() const { return medius_mock_recorded(h_); }
    bool saw(FrameType ty) const {
        return medius_mock_saw(h_, static_cast<MediusFrameType>(ty));
    }
    void clear_recorded() { medius_mock_clear_recorded(h_); }
    size_t recorded_frame(size_t idx, MediusFrameType* out_ty, uint8_t* out_seq,
                          uint8_t* payload_buf, size_t cap) const {
        return medius_mock_recorded_frame(h_, idx, out_ty, out_seq, payload_buf, cap);
    }

    const MediusMockBox* raw() const noexcept { return h_; }

private:
    void reset() noexcept {
        if (h_) {
            medius_mock_free(h_);
            h_ = nullptr;
        }
    }
    MediusMockBox* h_ = nullptr;
};
#endif // MEDIUS_FEATURE_MOCK

/// An open connection to one medius box. Move-only; frees on destruction.
class Device {
public:
    static Device find() {
        MediusDevice* h = nullptr;
        check(medius_device_find(&h));
        return Device(h);
    }
    static Device open(const std::string& path) {
        MediusDevice* h = nullptr;
        check(medius_device_open(path.c_str(), &h));
        return Device(h);
    }
#ifdef MEDIUS_FEATURE_MOCK
    /// Build a Device over the mock without a handshake.
    static Device with_mock(const MockBox& mock) {
        MediusDevice* h = nullptr;
        check(medius_device_with_mock(mock.raw(), &h));
        return Device(h);
    }
    /// Build a Device over the mock and run the version handshake.
    static Device open_mock(const MockBox& mock) {
        MediusDevice* h = nullptr;
        check(medius_device_open_mock(mock.raw(), &h));
        return Device(h);
    }
#endif

    Device(Device&& o) noexcept : h_(o.h_) { o.h_ = nullptr; }
    Device& operator=(Device&& o) noexcept {
        if (this != &o) {
            free_handle();
            h_ = o.h_;
            o.h_ = nullptr;
        }
        return *this;
    }
    Device(const Device&) = delete;
    Device& operator=(const Device&) = delete;
    ~Device() { free_handle(); }

    void move_rel(int16_t dx, int16_t dy) { check(medius_device_move_rel(h_, dx, dy)); }
    void wheel(int16_t delta) { check(medius_device_wheel(h_, delta)); }
    void move_axis(MediusMotion motion) { check(medius_device_move_axis(h_, motion)); }

    void inject(MediusInput input, Action action) {
        check(medius_device_inject(h_, input, static_cast<MediusAction>(action)));
    }
    void button(Button b, Action action) {
        check(medius_device_button(h_, static_cast<MediusButton>(b),
                                   static_cast<MediusAction>(action)));
    }
    void press(Button b) { check(medius_device_press(h_, static_cast<MediusButton>(b))); }
    void soft_release(Button b) {
        check(medius_device_soft_release(h_, static_cast<MediusButton>(b)));
    }
    void force_release(Button b) {
        check(medius_device_force_release(h_, static_cast<MediusButton>(b)));
    }

    void key(Key k, Action action) {
        check(medius_device_key(h_, k, static_cast<MediusAction>(action)));
    }
    void key_down(Key k) { check(medius_device_key_down(h_, k)); }
    void key_up(Key k) { check(medius_device_key_up(h_, k)); }
    void key_force_release(Key k) { check(medius_device_key_force_release(h_, k)); }

    void media(MediaKey m, Action action) {
        check(medius_device_media(h_, m, static_cast<MediusAction>(action)));
    }
    void media_down(MediaKey m) { check(medius_device_media_down(h_, m)); }
    void media_up(MediaKey m) { check(medius_device_media_up(h_, m)); }
    void media_force_release(MediaKey m) { check(medius_device_media_force_release(h_, m)); }

    void lock(MediusLockTarget target, LockDirection dir) {
        check(medius_device_lock(h_, target, static_cast<MediusLockDirection>(dir)));
    }
    void unlock(MediusLockTarget target, LockDirection dir) {
        check(medius_device_unlock(h_, target, static_cast<MediusLockDirection>(dir)));
    }
    void lock_key(Key k, LockDirection dir) {
        check(medius_device_lock_key(h_, k, static_cast<MediusLockDirection>(dir)));
    }
    void unlock_key(Key k, LockDirection dir) {
        check(medius_device_unlock_key(h_, k, static_cast<MediusLockDirection>(dir)));
    }
    void lock_media(MediaKey m) { check(medius_device_lock_media(h_, m)); }
    void unlock_media(MediaKey m) { check(medius_device_unlock_media(h_, m)); }
    void lock_all(Blanket what) {
        check(medius_device_lock_all(h_, static_cast<MediusBlanket>(what)));
    }
    void unlock_all(Blanket what) {
        check(medius_device_unlock_all(h_, static_cast<MediusBlanket>(what)));
    }

    void led(LedTarget target, LedMode mode, uint8_t level) {
        check(medius_device_led(h_, static_cast<MediusLedTarget>(target),
                                static_cast<MediusLedMode>(mode), level));
    }

    void reset() { check(medius_device_reset(h_)); }
    void reapply() { check(medius_device_reapply(h_)); }
    void reconnect() { check(medius_device_reconnect(h_)); }
    void reboot(RebootTarget target) {
        check(medius_device_reboot(h_, static_cast<MediusRebootTarget>(target)));
    }
    void allow_imperfect_clones(bool allow) {
        check(medius_device_allow_imperfect_clones(h_, allow));
    }
    /// Set movement riding. nullopt turns it off; otherwise the window is rounded to whole ms.
    void set_movement_riding(std::optional<std::chrono::milliseconds> window) {
        bool enabled = window.has_value();
        uint32_t ms = enabled ? static_cast<uint32_t>(window->count()) : 0;
        check(medius_device_set_movement_riding(h_, enabled, ms));
    }

    Version query_version() {
        Version v{};
        check(medius_device_query_version(h_, &v));
        return v;
    }
    Health query_health() {
        Health v{};
        check(medius_device_query_health(h_, &v));
        return v;
    }
    MouseInfo query_mouse_info() {
        MouseInfo v{};
        check(medius_device_query_mouse_info(h_, &v));
        return v;
    }
    Caps caps() {
        MediusCaps v{};
        check(medius_device_caps(h_, &v));
        return Caps(v);
    }
    Rate query_rate() {
        MediusRate v{};
        check(medius_device_query_rate(h_, &v));
        return Rate(v);
    }
    Stats query_stats() {
        Stats v{};
        check(medius_device_query_stats(h_, &v));
        return v;
    }
    Locks query_locks() {
        MediusLocks v{};
        check(medius_device_query_locks(h_, &v));
        return Locks(v);
    }
    CatchState query_catch() {
        CatchState v{};
        check(medius_device_query_catch(h_, &v));
        return v;
    }
    ImperfectStatus query_imperfect() {
        ImperfectStatus v{};
        check(medius_device_query_imperfect(h_, &v));
        return v;
    }
    /// The movement-riding window, or nullopt when off.
    std::optional<std::chrono::milliseconds> query_movement_riding() {
        bool enabled = false;
        uint32_t window_ms = 0;
        check(medius_device_query_movement_riding(h_, &enabled, &window_ms));
        if (!enabled) {
            return std::nullopt;
        }
        return std::chrono::milliseconds(window_ms);
    }
    CountersSnapshot counters() {
        CountersSnapshot v{};
        check(medius_device_counters(h_, &v));
        return v;
    }

    EventStream catch_events(CatchMask mask) {
        MediusEventStream* s = nullptr;
        check(medius_device_catch_events(h_, static_cast<MediusCatchMask>(mask), &s));
        return EventStream(s);
    }
    LogStream logs() {
        MediusLogStream* s = nullptr;
        check(medius_device_logs(h_, &s));
        return LogStream(s);
    }

    MediusDevice* raw() const noexcept { return h_; }

private:
    explicit Device(MediusDevice* h) : h_(h) {}
    void free_handle() noexcept {
        if (h_) {
            medius_device_free(h_);
            h_ = nullptr;
        }
    }
    MediusDevice* h_ = nullptr;
};

inline std::vector<PortInfo> find_ports() {
    size_t total = 0;
    medius_find_ports(nullptr, 0, &total);
    std::vector<PortInfo> ports(total);
    if (total > 0) {
        size_t written = medius_find_ports(ports.data(), ports.size(), &total);
        ports.resize(written);
    }
    return ports;
}

inline std::chrono::milliseconds default_query_timeout() {
    return std::chrono::milliseconds(medius_default_query_timeout_ms());
}
inline std::chrono::milliseconds default_keepalive_cadence() {
    return std::chrono::milliseconds(medius_default_keepalive_cadence_ms());
}
inline uint32_t abi_version() { return medius_abi_version(); }
inline std::string version_string() { return std::string(medius_version_string()); }

#ifdef MEDIUS_FEATURE_FLASH
/// Reboot a chip to ROM download and flash `bin_path` via esptool. Linux/Windows only.
inline void flash(const std::string& port, const std::string& bin_path, bool host) {
    check(medius_flash(port.c_str(), bin_path.c_str(), host));
}
#endif

} // namespace medius

#endif // MEDIUS_HPP
