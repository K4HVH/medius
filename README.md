# medius

[![Crates.io](https://img.shields.io/crates/v/medius)](https://crates.io/crates/medius)
[![Docs](https://img.shields.io/badge/docs-medius.k4tech.net-blue)](https://medius.k4tech.net/library)
[![CI](https://img.shields.io/github/actions/workflow/status/K4HVH/medius/ci.yml?label=CI)](https://github.com/K4HVH/medius/actions)
[![License](https://img.shields.io/crates/l/medius)](./LICENSE)
[![Discord](https://img.shields.io/badge/discord-medius-5865F2?logo=discord&logoColor=white)](https://discord.gg/ArRqcA84pB)

The Rust library for [Medius](https://medius.k4tech.net), replacement firmware for the MAKCU box. It drives a box
over its USB-serial control port: inject mouse, keyboard and media input, lock or scale native input, catch the
traffic through the box, play clips and update the firmware.

[Install Medius](https://medius.k4tech.net/dashboard/setup) · [Dashboard](https://medius.k4tech.net/dashboard) ·
[Docs](https://medius.k4tech.net/library) · [Stock vs Medius](https://medius.k4tech.net/comparison) ·
[Discord](https://discord.gg/ArRqcA84pB)

## Install

```sh
cargo add medius
```

| Feature | Adds |
|---|---|
| `async` | `AsyncDevice`: async queries, on any runtime |
| `mock` | `MockBox`: a fake box for tests without hardware |
| `tracing` | `tracing` events: the handshake, frames, queries, reconnects, chip restarts and the box's `LOG` lines |

## Usage

```rust
use medius::{Button, Device, Result};

fn main() -> Result<()> {
    let device = Device::find()?;            // first box found, protocol checked
    println!("{}", device.query_version()?);
    device.move_rel(100, -50)?;
    device.press(Button::LEFT)?;
    device.release(Button::LEFT)?;
    device.reset()?;                         // back to passthrough
    Ok(())
}
```

`Device` is `Send + Sync` and cheap to clone. A command returns as soon as its frame is sent; a query waits for the
reply. The crate reconnects by itself, and restores held keys and buttons, locks, rules, transforms and subscriptions
after a reconnect or a box restart.

## Other languages

| Language | Install | Docs |
|---|---|---|
| C, C++ | `medius.h` and the library from a [release](https://github.com/K4HVH/medius/releases) | [C / C++](https://medius.k4tech.net/bindings/c) |
| Python | `pip install medius` | [Python](https://medius.k4tech.net/bindings/python) |

## AI agents

Every docs page is served as Markdown (add `.md` to its URL), indexed in [llms.txt](https://medius.k4tech.net/llms.txt),
and exposed by a read-only [MCP server](https://medius.k4tech.net/ai#mcp):

```sh
claude mcp add --transport http medius-docs https://medius.k4tech.net/mcp
```

## Examples

```sh
cargo run --example basic                    # needs a box
cargo run --example hw_full --all-features   # every feature on hardware (Linux)
```

## License

MIT, see [LICENSE](LICENSE).
