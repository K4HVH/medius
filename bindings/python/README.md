# medius

Python bindings for [Medius](https://medius.k4tech.net), replacement firmware for the MAKCU box. A `ctypes` wrapper
over the Medius C ABI with no runtime dependencies; the wheel bundles the native library.

[Docs](https://medius.k4tech.net/bindings/python) · [Install Medius on a box](https://medius.k4tech.net/dashboard/setup) · [Discord](https://discord.gg/ArRqcA84pB)

## Install

```sh
pip install medius
```

Platforms without a prebuilt wheel build from the source distribution, which needs a Rust toolchain.

## Usage

```python
import medius

with medius.Device.find() as dev:
    print(dev.query_version())
    dev.move_rel(100, -50)
    dev.press(medius.Usage.button(medius.Button.LEFT))
    with dev.input_events(medius.CatchFilter.all_input()) as events:
        for event in events:                        # native input, live
            ...
```

Calls are synchronous: a command returns once it is sent, a query once its reply arrives. Failures raise
`MediusError`.

## License

MIT.
