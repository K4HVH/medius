# medius bindings

The [`medius`](../README.md) library is Rust; a C ABI drives the box from any
language. `medius-capi` exports a flat C API over the safe core, its generated
header compiles as C and C++, and the Python package is built on it. See
[Differences from the Rust API](#differences-from-the-rust-api).

```
medius (safe Rust crate)
  └── medius-capi   extern "C" + cbindgen → include/medius.h, libmedius_capi.{so,a}
        ├── C / C++   include the header, link the library
        └── Python    bindings/python   ctypes package (pip install)
```

## The C ABI

Build the library and use the generated header:

```sh
cargo build -p medius-capi --release          # target/release/libmedius_capi.{so,a}
cargo build -p medius-capi --release --features mock   # the mock box, for tests
```

The header is `medius-capi/include/medius.h`, committed and regenerated with
`tools/gen-header.sh` (cbindgen). It compiles as C99, C23, and C++. The mock
surface sits under `#if defined(MEDIUS_FEATURE_MOCK)`; define that macro for a
library built with the `mock` cargo feature.

Conventions: every fallible call returns a `MediusStatus` (`MEDIUS_STATUS_OK` is 0)
and writes its result through an out-param. `medius_last_error_message()` gives the
last failure's text on the calling thread. Handles (`MediusDevice`, `MediusEventStream`,
`MediusLogStream`, `MediusMockBox`) are opaque pointers with a `*_free`. Catch events
and log lines are fixed-size PODs sized to the protocol's limits; nothing is freed
per event.

Check the ABI once at start-up: `medius_abi_version()` is the loaded library's number,
`MEDIUS_ABI_VERSION` the header's. On a mismatch call nothing else; the struct layouts
differ, so rebuild against the header shipped with that library.

```c
#include <medius.h>

if (medius_abi_version() != MEDIUS_ABI_VERSION) { /* rebuild against the library's header */ }
MediusDevice *dev = NULL;
if (medius_device_find(&dev) != MEDIUS_STATUS_OK) { /* medius_last_error_message(...) */ }
MediusVersion v;
medius_device_query_version(dev, &v);
medius_device_move_rel(dev, 100, -50);
medius_device_press(dev, MEDIUS_BUTTON_LEFT);
medius_device_reset(dev);
medius_device_free(dev);
```

C++ includes the same header, links the same library and calls the C API directly.

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
    with dev.input_events(medius.CatchFilter.all_input()) as events:
        for event in events:
            ...
```

The wheel bundles `libmedius_capi`, so `pip install` needs no Rust toolchain. For
development, point `MEDIUS_LIB` at a local build (e.g.
`target/debug/libmedius_capi.so`; the test suite needs `--features mock`). Import
raises `ImportError`, naming both numbers, when the library's
`medius_abi_version()` differs from the ABI the ctypes mirrors target, so a library
from another release fails at import.

## Differences from the Rust API

The bindings cover the whole API; only the shape changes at the boundary.

| | Rust | C | Python |
|---|---|---|---|
| Errors | `Result` | a `MediusStatus`, detail in `medius_last_error_message()` | raises `MediusError` |
| `inject` | generic: `inject(Button::Left, ...)` | a built value: `medius_input_button(...)` | a built value: `Input.button(...)` |
| Async | `AsyncDevice` (feature `async`) | none: use a thread, or `try_recv` / `recv_timeout` | none: `asyncio.to_thread`, or `try_recv` / `recv_timeout` |

The direct verbs (`press`, `key_down`, `media_down`, …) are unchanged in all three.

## Packages

Publishing follows the crate's release flow in `.github/workflows/ci.yml`: bump the
version (`tools/bump_version.sh`) and push to master; the `publish` job publishes the
crate and creates the GitHub Release, then the bindings jobs ship the wheels and
C/C++ assets for that version.

### Python → PyPI

Builds the wheel matrix and sdist and uploads through PyPI trusted publishing (OIDC,
no token). One-time setup before the first publish, while the project isn't on PyPI:
register a *pending publisher* at `pypi.org/manage/account/publishing/` with
project `medius`, owner `K4HVH`, repo `medius`, **workflow `ci.yml`**, environment
`pypi`. Then `pip install medius`.

### C / C++ → GitHub Release assets

Attaches a `medius-capi-<target>.tar.gz` per platform to the release, each with
`include/medius.h` and the prebuilt `libmedius_capi` (shared + static). Include the
header and link the library, from C or C++.

C/C++ consumers use these tarballs or build `medius-capi` from source. It is
`publish = false`, so it isn't on crates.io; Rust users depend on `medius`.
