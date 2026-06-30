# medius bindings

The [`medius`](../README.md) library is Rust, but the box can be driven from any
language through a C ABI. The Rust crate `medius-capi` exports a flat C API over
the safe core; the C++ and Python bindings here ride on top of it. Every command,
query, and event stream the Rust crate exposes is reachable from all three.

[DIFFERENCES.md](DIFFERENCES.md) lists exactly how the bindings deviate from the
native Rust API (mostly ergonomic; no capability is lost).

```
medius (safe Rust crate)
  └── medius-capi   extern "C" + cbindgen → include/medius.h, libmedius_capi.{so,a}
        ├── C       include the header, link the library
        ├── C++     bindings/cpp   header-only RAII wrapper (exceptions)
        └── Python  bindings/python   ctypes package (pip install)
```

## The C ABI

Build the library and use the generated header:

```sh
cargo build -p medius-capi --release          # target/release/libmedius_capi.{so,a}
cargo build -p medius-capi --release --features mock,flash   # opt-in surfaces
```

The header is `medius-capi/include/medius.h`, committed and regenerated with
`tools/gen-header.sh` (cbindgen). It compiles as C99, C23, and C++. The mock and
flash surfaces are wrapped in `#ifdef MEDIUS_FEATURE_MOCK` / `MEDIUS_FEATURE_FLASH`;
define those macros when you built the library with the matching cargo feature.

Conventions: every fallible call returns a `MediusStatus` (`MEDIUS_STATUS_OK` is 0)
and writes its result through an out-param. `medius_last_error_message()` gives the
last failure's text on the calling thread. Handles (`MediusDevice`, `MediusEventStream`,
`MediusLogStream`, `MediusMockBox`) are opaque pointers with a `*_free`. Catch events
and log lines are fixed-size PODs sized to the protocol's own limits, so there is
nothing to free per event.

```c
#include <medius.h>

MediusDevice *dev = NULL;
if (medius_device_find(&dev) != MEDIUS_STATUS_OK) { /* medius_last_error_message(...) */ }
MediusVersion v;
medius_device_query_version(dev, &v);
medius_device_move_rel(dev, 100, -50);
medius_device_press(dev, MEDIUS_BUTTON_LEFT);
medius_device_reset(dev);
medius_device_free(dev);
```

## C++

Header-only, C++17, exceptions. See [`bindings/cpp`](cpp/).

```cpp
#include <medius/medius.hpp>

auto dev = medius::Device::find();        // throws medius::Error on failure
std::cout << dev.query_version().fw_minor;
dev.move_rel(100, -50);
dev.press(medius::Button::Left);
auto events = dev.catch_events(medius::CatchMask::All);
for (const medius::CatchEvent &e : events) { /* ... */ }
```

Use CMake:

```cmake
add_subdirectory(path/to/medius/bindings/cpp)
target_link_libraries(your_app PRIVATE medius::medius)
```

The CMake project finds a prebuilt `libmedius_capi` under the workspace `target/`
dir, or builds it for you with `-DMEDIUS_CARGO_BUILD=ON`. Enable the optional
surfaces with `-DMEDIUS_FEATURE_MOCK=ON` / `-DMEDIUS_FEATURE_FLASH=ON`.

## Python

A ctypes package with no runtime dependencies. See [`bindings/python`](python/).

```sh
pip install ./bindings/python      # builds and bundles the library
```

```python
import medius

with medius.Device.find() as dev:
    print(dev.query_version())
    dev.move_rel(100, -50)
    dev.press(medius.Button.LEFT)
    with dev.catch_events(medius.CatchMask.ALL) as events:
        for event in events:
            ...
```

The wheel bundles its own `libmedius_capi`, so `pip install` needs no Rust
toolchain. For development, point `MEDIUS_LIB` at a locally built library
(e.g. `target/debug/libmedius_capi.so`, built with `--features mock` for the
test suite).

## Packages

Publishing rides the crate's existing release flow in `.github/workflows/ci.yml`:
bump the version (`tools/bump_version.sh`) and push to master, and the `publish`
job publishes the crate and creates the GitHub Release, then the bindings jobs
build and ship the wheels and C/C++ assets for that same version.

- **Python → PyPI.** Builds the wheel matrix + sdist and uploads via PyPI trusted
  publishing (OIDC, no token). One-time setup, before the first publish (the
  project doesn't exist on PyPI yet): register a *pending publisher* at
  `pypi.org/manage/account/publishing/` with project `medius`, owner `K4HVH`,
  repo `medius`, **workflow `ci.yml`**, environment `pypi`. Then `pip install medius`.
- **C / C++ → GitHub Release assets.** Attaches a `medius-capi-<target>.tar.gz`
  per platform to the release, each with `include/medius.h` and the prebuilt
  `libmedius_capi` (shared + static). Download, include the header, link the
  library. The CMake project (`bindings/cpp`) consumes either a prebuilt library
  or builds one with `-DMEDIUS_CARGO_BUILD=ON`.

There's no vcpkg or Conan port: those registries build C/C++ from source in
hermetic CI with no Rust toolchain, so a Rust-backed library doesn't fit. C/C++
consumers use the release tarballs or the CMake project above.

The `medius-capi` crate itself is `publish = false`: it's the substrate for
other languages, not a Rust dependency (Rust users use the `medius` crate), so
it isn't published to crates.io.
