// Mirrors the README quick start. Needs a connected box to run; here it only has to compile.
#include <chrono>
#include <cstdio>

#include <medius/medius.hpp>

int main() {
    using namespace medius;
    try {
        Device dev = Device::find(); // or Device::open("/dev/ttyACM0")
        std::printf("medius-capi %s (abi %u)\n", version_string().c_str(), abi_version());

        Version v = dev.query_version();
        std::printf("firmware %u.%u.%u (proto %u)\n", v.fw_major, v.fw_minor, v.fw_patch,
                    v.proto_ver);

        dev.move_rel(100, -50);
        dev.press(Button::Left);
        dev.soft_release(Button::Left);
        dev.wheel(-3);

        EventStream events = dev.catch_events(CatchMask::All);
        while (std::optional<CatchEvent> ev = events.recv_timeout(std::chrono::milliseconds(500))) {
            if (auto m = ev->mouse()) {
                if (m->is_pressed(Button::Side1)) {
                    std::printf("side1 down: dx=%d dy=%d\n", m->dx, m->dy);
                }
            } else if (auto kb = ev->keyboard()) {
                std::printf("keyboard: %zu keys\n", kb->key_list().size());
            } else if (auto md = ev->media()) {
                std::printf("media: %zu usages\n", md->key_list().size());
            }
        }

        dev.reset();
    } catch (const Error& e) {
        std::fprintf(stderr, "medius error (%d): %s\n", static_cast<int>(e.status()), e.what());
        return 1;
    }
    return 0;
}
