//! Connection / session configuration ([`ConnectOptions`]) — the serde-able config surface (§10).
//!
//! A plain value type (no live handles) grouping the query timeout and keepalive cadence. Applied at
//! construction via [`Device::open_with`](crate::Device::open_with) /
//! [`Device::find_with`](crate::Device::find_with). The same knobs stay settable on the live objects;
//! this is the declarative surface over them.

use std::time::Duration;

/// Default query timeout: one second.
pub const DEFAULT_QUERY_TIMEOUT: Duration = Duration::from_secs(1);

/// Default keepalive cadence: 500 ms — sub-1 s so a held override outlives the firmware's 1000 ms
/// silence auto-clear.
pub const DEFAULT_KEEPALIVE_CADENCE: Duration = Duration::from_millis(500);

/// Declarative connection / session configuration.
///
/// A plain, copyable, `serde`-able value type — set the `pub` fields directly (with `..Default::default()`
/// for the rest) or deserialize from JSON/TOML, then pass it to
/// [`Device::open_with`](crate::Device::open_with) / [`Device::find_with`](crate::Device::find_with).
///
/// The two `Duration` fields serialize as integer-millisecond `_ms` pairs (`query_timeout_ms`,
/// `keepalive_cadence_ms`) rather than serde's default `{secs,nanos}`, so hand-written config stays
/// human-friendly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ConnectOptions {
    /// How long queries wait for the correlated `RESP` before returning
    /// [`Error::QueryTimeout`](crate::Error::QueryTimeout). Default `DEFAULT_QUERY_TIMEOUT` (1 s).
    #[cfg_attr(
        feature = "serde",
        serde(with = "duration_ms", rename = "query_timeout_ms")
    )]
    pub query_timeout: Duration,

    /// How often a held override is refreshed to defeat the firmware's 1000 ms silence auto-clear
    /// (§8). Must stay sub-1 s. Default `DEFAULT_KEEPALIVE_CADENCE` (500 ms).
    #[cfg_attr(
        feature = "serde",
        serde(with = "duration_ms", rename = "keepalive_cadence_ms")
    )]
    pub keepalive_cadence: Duration,
}

impl Default for ConnectOptions {
    fn default() -> Self {
        ConnectOptions {
            query_timeout: DEFAULT_QUERY_TIMEOUT,
            keepalive_cadence: DEFAULT_KEEPALIVE_CADENCE,
        }
    }
}

impl ConnectOptions {
    /// A fresh [`ConnectOptions`] with the defaults.
    pub fn new() -> Self {
        Self::default()
    }
}

/// serde helper: a `Duration` as integer milliseconds rather than serde's `{secs,nanos}`.
#[cfg(feature = "serde")]
mod duration_ms {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(d.as_millis() as u64)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let ms = u64::deserialize(d)?;
        Ok(Duration::from_millis(ms))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_documented_constants() {
        let o = ConnectOptions::default();
        assert_eq!(o.query_timeout, DEFAULT_QUERY_TIMEOUT);
        assert_eq!(o.keepalive_cadence, DEFAULT_KEEPALIVE_CADENCE);
    }

    #[test]
    fn fields_set_directly() {
        let o = ConnectOptions {
            query_timeout: Duration::from_millis(250),
            keepalive_cadence: Duration::from_millis(300),
        };
        assert_eq!(o.query_timeout, Duration::from_millis(250));
        assert_eq!(o.keepalive_cadence, Duration::from_millis(300));
    }

    #[test]
    fn struct_update_keeps_defaults() {
        let o = ConnectOptions {
            keepalive_cadence: Duration::from_millis(250),
            ..Default::default()
        };
        assert_eq!(o.keepalive_cadence, Duration::from_millis(250));
        assert_eq!(o.query_timeout, DEFAULT_QUERY_TIMEOUT);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_round_trips_with_ms_fields() {
        let o = ConnectOptions::default();
        let j = serde_json::to_string(&o).unwrap();
        assert!(j.contains("\"query_timeout_ms\":1000"), "json was {j}");
        assert!(j.contains("\"keepalive_cadence_ms\":500"), "json was {j}");
        let back: ConnectOptions = serde_json::from_str(&j).unwrap();
        assert_eq!(back, o);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_parses_a_handwritten_config() {
        let j = r#"{"query_timeout_ms": 250, "keepalive_cadence_ms": 400}"#;
        let o: ConnectOptions = serde_json::from_str(j).unwrap();
        assert_eq!(o.query_timeout, Duration::from_millis(250));
        assert_eq!(o.keepalive_cadence, Duration::from_millis(400));
    }
}
