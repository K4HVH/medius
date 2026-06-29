// Second translation unit including the header. If any header function were non-inline,
// linking this with test_mock.cpp would fail with a duplicate-symbol error.
#include <medius/medius.hpp>

using namespace medius;

int odr_probe() {
    (void)abi_version();
    (void)version_string();
    (void)default_query_timeout();
    (void)default_keepalive_cadence();
    std::vector<PortInfo> ports = find_ports();
    (void)ports;

    MockBox mock;
    Device dev = Device::with_mock(mock);
    dev.move_rel(3, 4);
    dev.button(Button::Right, Action::Press);
    dev.media_down(MEDIUS_MEDIA_VOLUME_UP);
    dev.unlock_all(Blanket::Buttons);
    dev.inject(Input::key(MEDIUS_KEY_B), Action::Press);
    dev.move_axis(Motion::cursor(1, 1));

    EventStream s = dev.catch_events(CatchMask::Motion | CatchMask::Buttons);
    std::optional<CatchEvent> e = s.try_recv();
    (void)e;

    CatchEvent ce;
    (void)ce.kind();

    return mock.saw(FrameType::Move) ? 1 : 0;
}
