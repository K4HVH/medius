//! Public value types — the centralized data vocabulary of the API.
//!
//! One module per concern (button, version, health, log, reboot, counters, port). These are pure
//! value types with their wire-mapping helpers (`as_*`/`from_*`); the raw `u8` wire constants they map
//! to live in the crate-internal `protocol::opcode` module. `serde` derives use `snake_case` to match
//! the wire doc / `medius.py`. Stateful handles (`Device`, `LogStream`, …) are NOT here — they live
//! with their logic.

mod button;
mod counters;
mod health;
mod log;
mod port;
mod reboot;
mod version;

pub use button::{Button, ButtonAction};
pub use counters::CountersSnapshot;
pub use health::Health;
pub use log::{LogLevel, LogLine};
pub use port::PortInfo;
pub use reboot::RebootTarget;
pub use version::Version;
