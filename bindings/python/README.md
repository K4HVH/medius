# medius

Python bindings for [medius](https://github.com/K4HVH/medius), custom firmware and a control library for MAKCU mouse-passthrough boxes.

A MAKCU box sits inline between a mouse and a PC: the real mouse passes through to the PC while your program injects movement, buttons, scroll, and keystrokes over USB-serial. This package drives the box from Python. It's a `ctypes` wrapper over the medius C ABI with no runtime dependencies, and the wheel bundles the native library, so `pip install` needs no Rust toolchain.

Full documentation is at [medius.k4tech.net](https://medius.k4tech.net).

## Install

```sh
pip install medius
```

Platforms without a prebuilt wheel fall back to the source distribution, which builds the library and needs a Rust toolchain.

## Usage

```python
import medius

with medius.Device.find() as dev:
    print(dev.query_version())
    dev.move_rel(100, -50)            # relative move
    dev.press(medius.Button.LEFT)     # force a button down
    with dev.catch_events(medius.CatchMask.ALL) as events:
        for event in events:          # the user's real input, live
            ...
```

Calls are synchronous and each sends one firmware frame; failures raise `MediusError`. The API covers mouse, keyboard, and media control, plus catching physical input. See [medius.k4tech.net](https://medius.k4tech.net) for the full reference.

## License

MIT.
