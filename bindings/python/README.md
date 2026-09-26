# medius

Python bindings for [medius](https://github.com/K4HVH/medius), custom firmware and a control library for MAKCU mouse-passthrough boxes.

A MAKCU box sits between a mouse and a PC: the mouse passes through while your program injects movement, buttons, scroll and keystrokes over USB-serial. This package is a `ctypes` wrapper over the medius C ABI with no runtime dependencies; the wheel bundles the native library, so `pip install` needs no Rust toolchain.

Documentation: [medius.k4tech.net](https://medius.k4tech.net).

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
    dev.move_rel(100, -50)                          # relative move
    dev.press(medius.Usage.button(medius.Button.LEFT))  # force a usage down
    with dev.input_events(medius.CatchFilter.all_input()) as events:
        for event in events:                        # physical input, live
            ...
```

Calls are synchronous, one firmware frame each; failures raise `MediusError`. The API covers mouse, keyboard and media control, and catching physical input.

## License

MIT.
