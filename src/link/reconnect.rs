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

use super::correlation::HELLO_SEQ;
use super::counters::Counters;
use super::reconcile::DesiredState;
use super::restart::RestartWatch;
use super::slot::TransportSlot;
use super::{Link, write_frame};

const AUTO_RECONNECT_MIN: Duration = Duration::from_millis(100);
const AUTO_RECONNECT_MAX: Duration = Duration::from_secs(2);

const PROBE_DEADLINE: Duration = Duration::from_millis(1200);
// The box drops PC-owned state on a fresh control-link open and can miss the first query while it
// settles, so the probe repeats this often within the deadline.
const PROBE_QUERY_GAP: Duration = Duration::from_millis(300);
// Any `SEQ` but the hello's, so a probe's `RESP(VERSION)` never reads as one.
const PROBE_SEQ: u8 = 0x80;

/// Stable box identity: CH343 serial (may be absent) and the device chip's base MAC.
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
    pub(crate) restart: Arc<RestartWatch>,
}

// Reads the reply off the local handle before it is swapped in, so the read never races the reader
// thread (on the disconnected slot here).
pub(crate) fn probe<T>(
    transport: &dyn Transport,
    what: u8,
    read: impl Fn(&[u8]) -> Option<T>,
) -> Option<T> {
    probe_seeing_hello(transport, what, read).0
}

// Also says whether the box's hello went past, which after a blip means the chip booted during it.
fn probe_seeing_hello<T>(
    transport: &dyn Transport,
    what: u8,
    read: impl Fn(&[u8]) -> Option<T>,
) -> (Option<T>, bool) {
    let Ok(frame) = encode(FrameType::Query, PROBE_SEQ, &[what]) else {
        return (None, false);
    };
    let mut decoder = FrameDecoder::new();
    let start = Instant::now();
    let mut last_query: Option<Instant> = None;
    let mut found: Option<Option<T>> = None;
    let mut hello = false;
    let mut rx = [0u8; 256];
    while found.is_none() && start.elapsed() < PROBE_DEADLINE {
        if last_query.is_none_or(|t| t.elapsed() >= PROBE_QUERY_GAP) {
            if transport.write_all(&frame).is_err() {
                return (None, hello);
            }
            last_query = Some(Instant::now());
        }
        match transport.read(&mut rx) {
            Ok(0) => {}
            Ok(n) => decoder.feed(&rx[..n], |f| {
                if f.ty != FrameType::Resp {
                    return;
                }
                hello |= f.seq == HELLO_SEQ && f.payload.first() == Some(&Q_VERSION);
                if found.is_none() && f.payload.first() == Some(&what) {
                    found = Some(read(&f.payload));
                }
            }),
            Err(_) => return (None, hello),
        }
    }
    (found.flatten(), hello)
}

// A rescan confirms the MAC before adopting a port.
fn probe_version(transport: &dyn Transport) -> (Option<Version>, bool) {
    probe_seeing_hello(transport, Q_VERSION, |p| match parse_resp(p) {
        Some(Resp::Version(v)) => Some(v),
        _ => None,
    })
}

// `None` for a box that does not reply or reports no buttons; a wide-button blanket then keeps the
// count the handshake or a prior reconnect cached.
fn probe_caps(transport: &dyn Transport) -> Option<u8> {
    probe(transport, Q_CAPS, |p| match parse_resp(p) {
        Some(Resp::Caps(c)) => Some(c.mouse.n_buttons),
        _ => None,
    })
    .filter(|&n| n > 0)
}

// A drop shorter than the box's silence window leaves the clip, its settings and triggers; a longer
// one clears them.
pub(crate) fn probe_clip(transport: &dyn Transport) -> Option<(ClipStatus, ClipSettings)> {
    probe(transport, Q_CLIP, |p| {
        Some((ClipStatus::from_payload(p)?, ClipSettings::from_payload(p)?))
    })
}

fn reconnect(ctx: &ReconnectCtx) -> Result<()> {
    let _guard = ctx.reconnect_lock.lock();
    let identity = ctx.identity.lock().clone();
    let ports = crate::transport::scan::find_medius();

    // With a known serial, try matching ports first; if none match (no serial, or it changed), try
    // every port and let the MAC confirm.
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

// Adopts the first reopened port that is this box on this build's protocol; a box on another
// protocol is refused with it, as the handshake refuses one.
#[cfg_attr(not(feature = "tracing"), allow(unused_variables))] // `path` is only read by trace_event!
fn adopt_first(
    ctx: &ReconnectCtx,
    identity: Option<&BoxIdentity>,
    opened: impl Iterator<Item = (String, Arc<dyn Transport>)>,
) -> Result<()> {
    let mut refused = None;
    for (path, port) in opened {
        let (version, restarted) = probe_version(&*port);
        // With an identity on record, the MAC must match; without one (a bare transport, e.g. a
        // mock), the first port that opens is taken.
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
        // Button count refreshed before the replay, so a wide-button blanket re-asserts onto the
        // current count, including a device swapped in during the blip.
        if let Some(n) = probe_caps(&*port) {
            ctx.desired.lock().note_declared_buttons(n);
        }
        // Held from the clip read to the end of the replay; a clip call sends and records under it,
        // so it lands wholly before or after the read and its adoption.
        let _reassert = ctx.catch_lock.lock();
        let clip = if restarted { None } else { probe_clip(&*port) };
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
        // A chip that booted during the blip has no clone yet: the restart recovery re-sends what
        // is held, the clip's settings and triggers with it, once it has one.
        if restarted {
            ctx.restart.begin();
            return Ok(());
        }
        // The caller reloads a clip, so the replay sends none; the keepalive keeps what the box still
        // has.
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
    reapply_locked(
        &ctx.transport,
        &ctx.write_lock,
        &ctx.seq,
        &ctx.counters,
        &ctx.desired,
        false,
    )
}

// Re-sends everything held, under the caller's `catch_lock`. `with_clip` adds the clip's settings and
// triggers, for a box that restarted and holds none of them.
pub(crate) fn reapply_locked(
    transport: &TransportSlot,
    write_lock: &Mutex<()>,
    seq: &AtomicU8,
    counters: &Counters,
    desired: &Mutex<DesiredState>,
    with_clip: bool,
) -> Result<()> {
    let (held, held_locks, catch, rewrites, transforms, clip) = {
        let d = desired.lock();
        (
            d.held().collect::<Vec<_>>(),
            d.held_locks(),
            d.catch(),
            d.held_rewrites(),
            d.held_transforms(),
            if with_clip {
                d.clip_config_frames()
            } else {
                Vec::new()
            },
        )
    };
    let send = |ty: FrameType, payload: &[u8]| {
        let seq = seq.fetch_add(1, Ordering::Relaxed);
        write_frame(transport, write_lock, counters, seq, ty, payload)
    };
    for (usage, action) in held {
        let (class, id) = usage.class_id();
        send(
            FrameType::Inject,
            &inject_payload(class, id, action.as_u8()),
        )?;
    }
    // Scales: like injection, the firmware clears them after ~1 s of silence, so a longer blip leaves
    // physical input untouched without this.
    for ((class, usage, direction), scale) in held_locks {
        send(
            FrameType::Lock,
            &lock_payload(class, usage, direction, scale),
        )?;
    }
    // Catch table: a drop past the ~1 s silence window clears it, and the stream stays dead without
    // this. Idempotent after a short drop.
    for f in catch.values() {
        let (class, id) = f.wire();
        send(
            FrameType::Catch,
            &catch_payload(class, id, f.direction().as_u8(), 1, f.capture().as_u8()),
        )?;
    }
    // Rewrite table: a drop past the silence window, or a re-clone, clears it on the box.
    for r in rewrites {
        send(
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
    // Transform table, likewise (§3.15): each goes out as state 1 (add/overwrite), idempotent after a
    // short drop. An entry whose field the swapped-in device lacks is absent from the box.
    for t in transforms {
        send(
            FrameType::Transform,
            &transform_payload(t.op, t.sclass, t.sid, t.dclass, t.did, 1),
        )?;
    }
    for (ty, payload) in clip {
        send(ty, &payload)?;
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
            restart: Arc::clone(&self.inner.restart),
        }
    }

    /// Records the box's identity so a later rescan reconnects to the same box.
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
