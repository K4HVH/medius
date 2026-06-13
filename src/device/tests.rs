//! Device core tests — reader routing, handshake, shutdown.
//!
//! All run against the in-memory [`MockTransport`]; none touch hardware. Thread-lifecycle tests carry a
//! short wall-clock budget so a regression that wedges the reader/Drop fails fast instead of hanging CI.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::Error;
use crate::protocol::types::LogLevel;
use crate::protocol::{FrameType, encode};
use crate::transport::mock::MockTransport;

use super::Device;

/// A `Device` plus an `Arc` to the same mock, so a test can push inbound frames / inspect writes.
fn device_with_mock() -> (Device, Arc<MockTransport>) {
    let mock = Arc::new(MockTransport::new());
    let device = Device::from_transport(mock.clone());
    (device, mock)
}

/// A responder answering `QUERY(VERSION)`/`QUERY(HEALTH)` with the given values, echoing the `SEQ`.
fn version_health_responder(version: [u8; 4], health_flags: u8) -> Arc<MockTransport> {
    Arc::new(MockTransport::with_responder(move |ty, seq, payload| {
        if ty != FrameType::Query {
            return Vec::new();
        }
        match payload.first().copied() {
            Some(0) => encode(
                FrameType::Resp,
                seq,
                &[0, version[0], version[1], version[2], version[3]],
            )
            .unwrap(),
            Some(1) => encode(FrameType::Resp, seq, &[1, health_flags]).unwrap(),
            _ => Vec::new(),
        }
    }))
}

/// Run `f` on its own thread and fail if it doesn't finish within `budget`, so a wedged reader/Drop
/// fails fast instead of hanging the suite.
fn within<F: FnOnce() + Send + 'static>(budget: Duration, f: F) {
    let (tx, rx) = flume::bounded(1);
    let h = std::thread::spawn(move || {
        f();
        let _ = tx.send(());
    });
    match rx.recv_timeout(budget) {
        Ok(()) => {
            h.join().unwrap();
        }
        Err(_) => panic!("operation did not complete within {budget:?}"),
    }
}

/// `from_transport` spawns a working reader and `counters` reflect a sent frame.
#[test]
fn from_transport_spawns_reader_and_counts_tx() {
    let (device, _mock) = device_with_mock();
    device.send(FrameType::Reset, &[]).unwrap();
    assert_eq!(device.counters().frames_tx, 1);
}

/// Handshake succeeds when the mock answers `QUERY(VERSION)` with the right proto_ver.
#[test]
fn handshake_succeeds_on_matching_version() {
    let mock = version_health_responder([1, 2, 3, 4], 0x0F);
    let device = Device::open_transport(mock).expect("handshake should succeed");
    let v = device.query_version().unwrap();
    assert_eq!(v.proto_ver, 1);
    assert_eq!((v.fw_major, v.fw_minor, v.fw_patch), (2, 3, 4));
}

/// A wrong protocol version is rejected with `BadProtoVer { got }`.
#[test]
fn handshake_rejects_wrong_proto_ver() {
    let mock = version_health_responder([9, 0, 0, 0], 0x00);
    let err = Device::open_transport(mock).unwrap_err();
    assert!(matches!(err, Error::BadProtoVer { got: 9 }), "got {err:?}");
}

/// A silent mock makes the handshake fail with `NoReply` (the underlying query times out).
#[test]
fn handshake_on_silent_box_is_no_reply() {
    let mock = Arc::new(MockTransport::new());
    // Drive the query directly with a short timeout rather than eat the full 1 s handshake timeout.
    let device = Device::from_transport(mock.clone());
    let err = device
        .query_timeout(0, Duration::from_millis(50))
        .unwrap_err();
    assert!(matches!(err, Error::QueryTimeout), "got {err:?}");
}

/// The query timeout removes the stale waiter from `pending` (no leak).
#[test]
fn query_timeout_removes_pending_waiter() {
    let (device, _mock) = device_with_mock();
    let _ = device.query_timeout(0, Duration::from_millis(30));
    assert_eq!(device.pending_len(), 0, "timed-out waiter must be removed");
}

/// The reader routes a pushed `LOG` frame onto the `logs()` channel, decoded.
#[test]
fn reader_routes_log_to_logs_channel() {
    let (device, mock) = device_with_mock();
    let rx = device.logs();
    mock.push_frame(FrameType::Log, 0, &[2, b'h', b'i']); // level=info "hi"

    let line = rx
        .recv_timeout(Duration::from_secs(1))
        .expect("a LOG line should arrive");
    assert_eq!(line.level, LogLevel::Info);
    assert_eq!(line.text, "hi");
    assert!(device.counters().frames_rx >= 1);
}

/// Multiple pushed LOG frames arrive in order.
#[test]
fn reader_preserves_log_order() {
    let (device, mock) = device_with_mock();
    let rx = device.logs();
    for i in 0..5u8 {
        mock.push_frame(FrameType::Log, i, &[2, b'0' + i]);
    }
    let mut texts = Vec::new();
    for _ in 0..5 {
        texts.push(rx.recv_timeout(Duration::from_secs(1)).unwrap().text);
    }
    assert_eq!(texts, vec!["0", "1", "2", "3", "4"]);
}

/// Dropping the last `Device` clone stops and joins the reader within a tight budget (no hang).
#[test]
fn drop_joins_reader_quickly() {
    within(Duration::from_millis(500), || {
        let (device, _mock) = device_with_mock();
        let start = Instant::now();
        drop(device);
        assert!(
            start.elapsed() < Duration::from_millis(300),
            "drop took too long: {:?}",
            start.elapsed()
        );
    });
}

/// A clone keeps the box alive; dropping one clone does not stop the reader.
#[test]
fn clone_keeps_reader_alive_until_last_drop() {
    let (device, mock) = device_with_mock();
    let clone = device.clone();
    drop(device);
    // Reader still alive: a pushed LOG routes through the surviving clone.
    let rx = clone.logs();
    mock.push_frame(FrameType::Log, 0, &[2, b'x']);
    let line = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(line.text, "x");
}

/// `Device` is `Send + Sync` (shared as a handle across threads).
#[test]
fn device_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Device>();
}

/// Tracing acceptance tests: a tiny capturing subscriber (no `tracing-subscriber` dev-dep) tallies
/// events by `target`, to assert connect/query events fire and the pacer does NOT trace per tick.
#[cfg(feature = "tracing")]
mod tracing_capture {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use tracing::span::{Attributes, Id, Record};
    use tracing::{Event, Metadata, Subscriber};

    use crate::protocol::{FrameType, encode};
    use crate::transport::mock::MockTransport;

    use super::Device;

    /// Install once a no-op global default subscriber hinting `max_level = TRACE`. tracing's `event!`
    /// macros consult the process-wide static max-level filter (computed from the global default)
    /// BEFORE the active subscriber; with no global default it sits at `OFF` and events are dropped
    /// before any per-thread `with_default` subscriber sees them. Pinning it at `TRACE` makes the
    /// captures below deterministic regardless of parallel test ordering.
    fn ensure_tracing_enabled() {
        use std::sync::Once;
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            struct TraceLevelHint;
            impl Subscriber for TraceLevelHint {
                fn enabled(&self, _: &Metadata<'_>) -> bool {
                    false // record nothing; exists only to raise the static max-level filter
                }
                fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
                    Some(tracing::level_filters::LevelFilter::TRACE)
                }
                fn new_span(&self, _: &Attributes<'_>) -> Id {
                    Id::from_u64(1)
                }
                fn record(&self, _: &Id, _: &Record<'_>) {}
                fn record_follows_from(&self, _: &Id, _: &Id) {}
                fn event(&self, _: &Event<'_>) {}
                fn enter(&self, _: &Id) {}
                fn exit(&self, _: &Id) {}
            }
            // If another test already set a global default, the static filter is already raised; ignore.
            let _ = tracing::subscriber::set_global_default(TraceLevelHint);
        });
    }

    /// A minimal [`Subscriber`] counting emitted events bucketed by `target` prefix (`medius::device` /
    /// `medius::transport` / `medius::pacer`).
    #[derive(Clone, Default)]
    struct CountingSubscriber {
        device: Arc<AtomicU64>,
        transport: Arc<AtomicU64>,
        pacer: Arc<AtomicU64>,
    }

    impl Subscriber for CountingSubscriber {
        fn enabled(&self, _: &Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }
        fn record(&self, _: &Id, _: &Record<'_>) {}
        fn record_follows_from(&self, _: &Id, _: &Id) {}
        fn event(&self, event: &Event<'_>) {
            let target = event.metadata().target();
            if target.starts_with("medius::device") {
                self.device.fetch_add(1, Ordering::Relaxed);
            } else if target.starts_with("medius::transport") {
                self.transport.fetch_add(1, Ordering::Relaxed);
            } else if target.starts_with("medius::pacer") {
                self.pacer.fetch_add(1, Ordering::Relaxed);
            }
        }
        fn enter(&self, _: &Id) {}
        fn exit(&self, _: &Id) {}
    }

    fn version_responder() -> Arc<MockTransport> {
        Arc::new(MockTransport::with_responder(|ty, seq, payload| {
            if ty == FrameType::Query && payload.first() == Some(&0) {
                encode(FrameType::Resp, seq, &[0, 1, 2, 3, 4]).unwrap()
            } else {
                Vec::new()
            }
        }))
    }

    /// A successful connect (handshake) emits at least one `medius::device` event.
    #[test]
    fn connect_emits_a_device_event() {
        ensure_tracing_enabled();
        let sub = CountingSubscriber::default();
        let device_count = Arc::clone(&sub.device);
        tracing::subscriber::with_default(sub, || {
            let _device = Device::open_transport(version_responder()).expect("handshake ok");
        });
        assert!(
            device_count.load(Ordering::Relaxed) >= 1,
            "expected ≥1 medius::device event from connect"
        );
    }

    /// A query emits a `medius::device` DEBUG event and per-frame `medius::transport` TRACE events.
    #[test]
    fn query_emits_device_and_transport_events() {
        ensure_tracing_enabled();
        let sub = CountingSubscriber::default();
        let device_count = Arc::clone(&sub.device);
        let transport_count = Arc::clone(&sub.transport);
        tracing::subscriber::with_default(sub, || {
            let device = Device::from_transport(version_responder());
            let _ = device.query_version().unwrap();
        });
        assert!(
            device_count.load(Ordering::Relaxed) >= 1,
            "query should emit a medius::device event"
        );
        assert!(
            transport_count.load(Ordering::Relaxed) >= 1,
            "the TX (and RX) frames should emit medius::transport events"
        );
    }

    /// The pacer must NOT trace per tick: over a 1 kHz run (hundreds of ticks) the `medius::pacer`
    /// event count stays tiny (≈ elapsed seconds), proving the aggregate-only hot-path discipline.
    #[test]
    fn pacer_does_not_trace_per_tick() {
        ensure_tracing_enabled();
        let sub = CountingSubscriber::default();
        let pacer_count = Arc::clone(&sub.pacer);
        tracing::subscriber::with_default(sub, || {
            let device = Device::from_transport(Arc::new(MockTransport::new()));
            let mv = device.movement(); // 1 kHz
            for _ in 0..250 {
                mv.push(1, 0); // drive an emission per tick (≈250 ticks)
                std::thread::sleep(Duration::from_millis(1));
            }
            drop(mv);
        });
        let pacer_events = pacer_count.load(Ordering::Relaxed);
        // Per-tick would be ~250; the aggregate is ~1/sec, so over 250 ms expect a small handful.
        assert!(
            pacer_events <= 3,
            "pacer must aggregate, not trace per tick — saw {pacer_events} pacer events"
        );
    }
}
