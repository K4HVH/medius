use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::error::{Error, Result};
use crate::protocol::command::{
    catch_payload, inject_payload, lock_payload, rewrite_payload, transform_payload,
};
use crate::protocol::opcode::{Q_CAPS, Q_CLIP, Q_VERSION};
use crate::protocol::{FrameDecoder, FrameType, PROTO_VER, Resp, encode, parse_resp};
use crate::transport::Transport;
use crate::types::{ClipSettings, ClipStatus, Version};

use super::counters::Counters;
use super::reconcile::DesiredState;
use super::slot::TransportSlot;
use super::{Link, write_frame};

const AUTO_RECONNECT_MIN: Duration = Duration::from_millis(100);
const AUTO_RECONNECT_MAX: Duration = Duration::from_secs(2);

const PROBE_DEADLINE: Duration = Duration::from_millis(1200);
// The box drops PC-owned state on a fresh control-link open and can miss the first query while it
// settles, so re-send the probe this often within the deadline.
const PROBE_QUERY_GAP: Duration = Duration::from_millis(300);

/// The opened box's stable identity: CH343 serial (may be absent) plus the device chip's base MAC.
#[derive(Clone, Debug)]
pub(crate) struct BoxIdentity {
    pub(crate) serial: Option<String>,
    pub(crate) mac: [u8; 6],
}

pub(crate) struct ReconnectCtx {
    pub(crate) transport: Arc<TransportSlot>,
    pub(crate) write_lock: Arc<Mutex<()>>,
    pub(crate) seq: Arc<AtomicU8>,
    pub(crate) counters: Arc<Counters>,
    pub(crate) desired: Arc<Mutex<DesiredState>>,
    pub(crate) reconnect_lock: Arc<Mutex<()>>,
    pub(crate) identity: Arc<Mutex<Option<BoxIdentity>>>,
    // The lock subscribe and unsubscribe commit under.
    pub(crate) catch_lock: Arc<Mutex<()>>,
    // Both halves of the update reply path.
    pub(crate) held_updates: Arc<Mutex<Vec<Vec<u8>>>>,
    pub(crate) updates_rx: flume::Receiver<Vec<u8>>,
}

// Asks the reopened port one `QUERY` and reads the answer off the local handle before it is swapped
// in, so the read never races the reader thread (which is on the disconnected slot here).
pub(crate) fn probe<T>(
    transport: &dyn Transport,
    what: u8,
    read: impl Fn(&[u8]) -> Option<T>,
) -> Option<T> {
    let frame = encode(FrameType::Query, 0, &[what]).ok()?;
    let mut decoder = FrameDecoder::new();
    let start = Instant::now();
    let mut last_query: Option<Instant> = None;
    let mut found: Option<Option<T>> = None;
    let mut rx = [0u8; 256];
    while found.is_none() && start.elapsed() < PROBE_DEADLINE {
        if last_query.is_none_or(|t| t.elapsed() >= PROBE_QUERY_GAP) {
            if transport.write_all(&frame).is_err() {
                return None;
            }
            last_query = Some(Instant::now());
        }
        match transport.read(&mut rx) {
            Ok(0) => {}
            Ok(n) => decoder.feed(&rx[..n], |f| {
                if found.is_none() && f.ty == FrameType::Resp && f.payload.first() == Some(&what) {
                    found = Some(read(&f.payload));
                }
            }),
            Err(_) => return None,
        }
    }
    found.flatten()
}

// A rescan confirms the MAC before adopting a port.
fn probe_version(transport: &dyn Transport) -> Option<Version> {
    probe(transport, Q_VERSION, |p| match parse_resp(p) {
        Some(Resp::Version(v)) => Some(v),
        _ => None,
    })
}

// The reopened clone's declared button count. `None` for a box that does not answer or reports no
// buttons; a wide-button blanket then keeps whatever count the handshake or a prior reconnect cached.
fn probe_caps(transport: &dyn Transport) -> Option<u8> {
    probe(transport, Q_CAPS, |p| match parse_resp(p) {
        Some(Resp::Caps(c)) => Some(c.mouse.n_buttons),
        _ => None,
    })
    .filter(|&n| n > 0)
}

// What the box holds of a clip after the blip. A drop shorter than the box's silence window leaves the
// clip, its settings and its triggers standing; a longer one clears them.
pub(crate) fn probe_clip(transport: &dyn Transport) -> Option<(ClipStatus, ClipSettings)> {
    probe(transport, Q_CLIP, |p| {
        Some((ClipStatus::from_payload(p)?, ClipSettings::from_payload(p)?))
    })
}

fn reconnect(ctx: &ReconnectCtx) -> Result<()> {
    let _guard = ctx.reconnect_lock.lock();
    let identity = ctx.identity.lock().clone();
    let ports = crate::transport::scan::find_medius();

    // With a known serial, try matching port(s) first; if none match (no serial served, or it
    // changed), fall back to every port and let the MAC confirm which is ours.
    let candidates: Vec<_> = match &identity {
        Some(id) if id.serial.is_some() => {
            let matched: Vec<_> = ports
                .iter()
                .filter(|p| p.serial == id.serial)
                .cloned()
                .collect();
            if matched.is_empty() { ports } else { matched }
        }
        _ => ports,
    };
    if candidates.is_empty() {
        return Err(Error::NotFound);
    }

    ctx.transport.swap(Arc::new(crate::transport::Disconnected));
    std::thread::sleep(Duration::from_millis(200));

    let opened = candidates.into_iter().filter_map(|port| {
        let serial =
            crate::transport::serial::SerialTransport::open(std::path::Path::new(&port.path))
                .ok()?;
        Some((port.path, Arc::new(serial) as Arc<dyn Transport>))
    });
    adopt_first(ctx, identity.as_ref(), opened)
}

// Takes back the first reopened port that is this box on this build's protocol. A box that answers on
// another protocol is refused with it, as the handshake refuses one.
#[cfg_attr(not(feature = "tracing"), allow(unused_variables))] // `path` is only read by trace_event!
fn adopt_first(
    ctx: &ReconnectCtx,
    identity: Option<&BoxIdentity>,
    opened: impl Iterator<Item = (String, Arc<dyn Transport>)>,
) -> Result<()> {
    let mut refused = None;
    for (path, port) in opened {
        let version = probe_version(&*port);
        // With an identity on record, confirm the MAC before committing so a rescan never adopts the
        // wrong box. Without one (a transport opened bare, e.g. a mock), accept the first that opens.
        if let Some(id) = identity
            && version.as_ref().is_none_or(|v| v.mac != id.mac)
        {
            continue;
        }
        // A box reflashed while held comes back on its new protocol, whose replies this build would
        // misread from the clip probe on.
        if let Some(got) = version
            .as_ref()
            .map(|v| v.proto_ver)
            .filter(|&p| p != PROTO_VER)
        {
            trace_event!(
                target: "medius::device",
                tracing::Level::WARN,
                port = %path,
                got,
                expected = PROTO_VER,
                "reconnect: unsupported protocol version",
            );
            refused = Some(got);
            continue;
        }
        // Refresh the declared button count off the reopened clone before the replay, so a wide-button
        // blanket re-asserts onto the count the box reports now and a device swapped in during the blip
        // re-asserts onto the new device's count.
        if let Some(n) = probe_caps(&*port) {
            ctx.desired.lock().note_declared_buttons(n);
        }
        // Held from the read of the box's clip to the end of the replay. A clip call sends and records
        // under this lock, so it lands whole on one side of the read and its adoption.
        let _reassert = ctx.catch_lock.lock();
        let clip = probe_clip(&*port);
        ctx.transport.swap(port);
        ctx.held_updates.lock().clear();
        while ctx.updates_rx.try_recv().is_ok() {}
        ctx.counters.inc_reconnects();
        trace_event!(
            target: "medius::device",
            tracing::Level::INFO,
            port = %path,
            reason = "rescan",
            "reconnected",
        );
        // A clip is the caller's to reload, so the replay sends none of it. The keepalive holds
        // whatever of it the box still has.
        if let Some((status, settings)) = clip {
            ctx.desired.lock().clip_adopt(&status, &settings);
        }
        return reapply_held_locked(ctx);
    }
    Err(match refused {
        Some(got) => Error::BadProtoVer { got },
        None => Error::NotFound,
    })
}

fn reapply_held(ctx: &ReconnectCtx) -> Result<()> {
    let _serial = ctx.catch_lock.lock();
    reapply_held_locked(ctx)
}

fn reapply_held_locked(ctx: &ReconnectCtx) -> Result<()> {
    let (held, held_locks, catch, rewrites, transforms) = {
        let d = ctx.desired.lock();
        (
            d.held().collect::<Vec<_>>(),
            d.held_locks(),
            d.catch(),
            d.held_rewrites(),
            d.held_transforms(),
        )
    };
    for (usage, action) in held {
        let (class, id) = usage.class_id();
        let seq = ctx.seq.fetch_add(1, Ordering::Relaxed);
        write_frame(
            &ctx.transport,
            &ctx.write_lock,
            &ctx.counters,
            seq,
            FrameType::Inject,
            &inject_payload(class, id, action.as_u8()),
        )?;
    }
    // Re-assert held scales: like injection, the firmware silence-clears every one after the ~1 s
    // window, so a blip past it would leave physical input passing untouched without this.
    for ((class, usage, direction), scale) in held_locks {
        let seq = ctx.seq.fetch_add(1, Ordering::Relaxed);
        write_frame(
            &ctx.transport,
            &ctx.write_lock,
            &ctx.counters,
            seq,
            FrameType::Lock,
            &lock_payload(class, usage, direction, scale),
        )?;
    }
    // Re-assert the catch table: a link drop past the firmware's ~1 s silence window makes the box
    // clear it, so without this the stream stays dead. Idempotent if the drop was short.
    for f in catch.values() {
        let seq = ctx.seq.fetch_add(1, Ordering::Relaxed);
        let (class, id) = f.wire();
        write_frame(
            &ctx.transport,
            &ctx.write_lock,
            &ctx.counters,
            seq,
            FrameType::Catch,
            &catch_payload(class, id, f.direction().as_u8(), 1, f.capture().as_u8()),
        )?;
    }
    // Re-assert the rewrite table: a drop past the firmware silence window, or a re-clone, clears
    // it box-side, so without this the rules stay dead.
    for r in rewrites {
        let seq = ctx.seq.fetch_add(1, Ordering::Relaxed);
        write_frame(
            &ctx.transport,
            &ctx.write_lock,
            &ctx.counters,
            seq,
            FrameType::Rewrite,
            &rewrite_payload(
                r.class,
                r.id,
                r.direction,
                1,
                r.action,
                r.offset,
                &r.match_bytes,
                &r.mask,
                &r.payload,
            ),
        )?;
    }
    // Re-assert the transform table for the same reason (§3.15): the box clears it past the silence
    // window or on a re-clone, and each goes out as state 1 (add/overwrite), idempotent if the drop was
    // short. A refused entry (its field gone on the swapped-in device) is simply absent from the box.
    for t in transforms {
        let seq = ctx.seq.fetch_add(1, Ordering::Relaxed);
        write_frame(
            &ctx.transport,
            &ctx.write_lock,
            &ctx.counters,
            seq,
            FrameType::Transform,
            &transform_payload(t.op, t.sclass, t.sid, t.dclass, t.did, 1),
        )?;
    }
    Ok(())
}

pub(crate) fn auto_reconnect(ctx: &ReconnectCtx, stop: &AtomicBool) {
    let mut backoff = AUTO_RECONNECT_MIN;
    while !stop.load(Ordering::SeqCst) {
        if reconnect(ctx).is_ok() {
            return;
        }
        std::thread::sleep(backoff);
        backoff = (backoff * 2).min(AUTO_RECONNECT_MAX);
    }
}

impl Link {
    fn reconnect_ctx(&self) -> ReconnectCtx {
        ReconnectCtx {
            transport: Arc::clone(&self.inner.transport),
            write_lock: Arc::clone(&self.inner.write_lock),
            seq: Arc::clone(&self.inner.seq),
            counters: Arc::clone(&self.inner.counters),
            desired: Arc::clone(&self.inner.desired),
            reconnect_lock: Arc::clone(&self.inner.reconnect_lock),
            identity: Arc::clone(&self.inner.identity),
            catch_lock: Arc::clone(&self.inner.catch_lock),
            held_updates: Arc::clone(&self.inner.held_updates),
            updates_rx: self.inner.updates_rx.clone(),
        }
    }

    /// Record the box's stable identity so a later rescan reconnects to this same box.
    pub(crate) fn set_identity(&self, id: BoxIdentity) {
        *self.inner.identity.lock() = Some(id);
    }

    pub(crate) fn reconnect(&self) -> Result<()> {
        reconnect(&self.reconnect_ctx())
    }

    // The rescan's adoption over ports a test has already opened.
    #[cfg(test)]
    pub(crate) fn adopt_reopened(&self, opened: Vec<(String, Arc<dyn Transport>)>) -> Result<()> {
        let ctx = self.reconnect_ctx();
        let _guard = ctx.reconnect_lock.lock();
        let identity = ctx.identity.lock().clone();
        adopt_first(&ctx, identity.as_ref(), opened.into_iter())
    }

    pub(crate) fn reapply(&self) -> Result<()> {
        reapply_held(&self.reconnect_ctx())
    }
}
