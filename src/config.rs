//! Connection / session configuration ([`ConnectOptions`]) — the serde-able config surface (§10).
//!
//! [`ConnectOptions`] groups the tunables a host app may want to set or load from a config file: the
//! query timeout, the keepalive cadence, and the pacer rate. It is a **plain value type** (no live
//! handles), so it carries the `serde` derives like the rest of the public value surface and is the
//! one config struct the design spec (§10 `serde`) calls for.
//!
//! It is applied at construction: [`Device::open_with`](crate::Device::open_with) /
//! [`Device::find_with`](crate::Device::find_with) build a device whose keepalive cadence and query
//! timeout come from the options, and [`ConnectOptions::movement`] /
//! [`Device::movement_with`](crate::Device::movement_with) open a [`MovementSession`] at the
//! configured rate. The individual knobs remain settable on the live objects too
//! ([`MovementSession::set_rate`], etc.); `ConnectOptions` is the *declarative* surface over them.
//!
//! [`MovementSession`]: crate::MovementSession
//! [`MovementSession::set_rate`]: crate::MovementSession::set_rate

use std::time::Duration;

use crate::pacer::DEFAULT_RATE_HZ;

/// Default query timeout (mirrors the device layer's internal default): one second.
pub const DEFAULT_QUERY_TIMEOUT: Duration = Duration::from_secs(1);

/// Default keepalive cadence (mirrors the device layer's internal default): 500 ms (sub-1 s so a held
/// override outlives the firmware's 1000 ms silence auto-clear).
pub const DEFAULT_KEEPALIVE_CADENCE: Duration = Duration::from_millis(500);

/// Declarative connection / session configuration.
///
/// A plain, copyable, `serde`-able value type (the config surface, §10). Build it with [`Default`] and
/// the `with_*` setters, or deserialize it from JSON/TOML. Pass it to
/// [`Device::open_with`](crate::Device::open_with) / [`Device::find_with`](crate::Device::find_with)
/// to configure a device, and to [`Device::movement_with`](crate::Device::movement_with) (or
/// [`ConnectOptions::movement`]) for a session at the configured rate.
///
/// ## Serde representation
///
/// The two `Duration` fields serialize as a `_ms` integer-millisecond pair (`query_timeout_ms`,
/// `keepalive_cadence_ms`) rather than serde's default `{secs,nanos}` struct, so a hand-written config
/// file stays human-friendly (`"query_timeout_ms": 1000`). `rate_hz` is a plain integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ConnectOptions {
    /// How long [`query_version`](crate::Device::query_version) /
    /// [`query_health`](crate::Device::query_health) wait for the correlated `RESP` before returning
    /// [`Error::QueryTimeout`](crate::Error::QueryTimeout). Default [`DEFAULT_QUERY_TIMEOUT`] (1 s).
    #[cfg_attr(
        feature = "serde",
        serde(with = "duration_ms", rename = "query_timeout_ms")
    )]
    pub query_timeout: Duration,

    /// The keepalive cadence — how often a held override is refreshed to defeat the firmware's 1000 ms
    /// silence auto-clear (§8). Must stay sub-1 s. Default [`DEFAULT_KEEPALIVE_CADENCE`] (500 ms).
    #[cfg_attr(
        feature = "serde",
        serde(with = "duration_ms", rename = "keepalive_cadence_ms")
    )]
    pub keepalive_cadence: Duration,

    /// The pacer tick rate in Hz for [`movement`](ConnectOptions::movement)-opened sessions. Default
    /// [`DEFAULT_RATE_HZ`] (1000).
    pub rate_hz: u32,
}

impl Default for ConnectOptions {
    fn default() -> Self {
        ConnectOptions {
            query_timeout: DEFAULT_QUERY_TIMEOUT,
            keepalive_cadence: DEFAULT_KEEPALIVE_CADENCE,
            rate_hz: DEFAULT_RATE_HZ,
        }
    }
}

impl ConnectOptions {
    /// A fresh [`ConnectOptions`] with the defaults (alias for [`Default::default`]).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the query timeout (builder style).
    #[must_use]
    pub fn with_query_timeout(mut self, timeout: Duration) -> Self {
        self.query_timeout = timeout;
        self
    }

    /// Set the keepalive cadence (builder style).
    #[must_use]
    pub fn with_keepalive_cadence(mut self, cadence: Duration) -> Self {
        self.keepalive_cadence = cadence;
        self
    }

    /// Set the pacer rate in Hz (builder style).
    #[must_use]
    pub fn with_rate_hz(mut self, rate_hz: u32) -> Self {
        self.rate_hz = rate_hz;
        self
    }

    /// Open a [`MovementSession`](crate::MovementSession) over `device` at this config's `rate_hz`
    /// (equivalent to [`Device::movement_with`](crate::Device::movement_with)).
    pub fn movement(&self, device: &crate::Device) -> crate::MovementSession {
        device.movement_with(self)
    }
}

/// serde helper: represent a `Duration` as integer milliseconds (`query_timeout_ms` etc.) instead of
/// serde's default `{secs,nanos}` so config files are human-friendly.
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
        assert_eq!(o.rate_hz, DEFAULT_RATE_HZ);
    }

    #[test]
    fn builders_chain() {
        let o = ConnectOptions::new()
            .with_query_timeout(Duration::from_millis(250))
            .with_keepalive_cadence(Duration::from_millis(300))
            .with_rate_hz(500);
        assert_eq!(o.query_timeout, Duration::from_millis(250));
        assert_eq!(o.keepalive_cadence, Duration::from_millis(300));
        assert_eq!(o.rate_hz, 500);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_round_trips_with_ms_fields() {
        let o = ConnectOptions::default();
        let j = serde_json::to_string(&o).unwrap();
        // Durations serialize as integer-millisecond `_ms` fields, not {secs,nanos}.
        assert!(j.contains("\"query_timeout_ms\":1000"), "json was {j}");
        assert!(j.contains("\"keepalive_cadence_ms\":500"), "json was {j}");
        assert!(j.contains("\"rate_hz\":1000"), "json was {j}");
        let back: ConnectOptions = serde_json::from_str(&j).unwrap();
        assert_eq!(back, o);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_parses_a_handwritten_config() {
        let j = r#"{"query_timeout_ms": 250, "keepalive_cadence_ms": 400, "rate_hz": 2000}"#;
        let o: ConnectOptions = serde_json::from_str(j).unwrap();
        assert_eq!(o.query_timeout, Duration::from_millis(250));
        assert_eq!(o.keepalive_cadence, Duration::from_millis(400));
        assert_eq!(o.rate_hz, 2000);
    }
}
