use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::protocol::command::query_payload;
use crate::protocol::opcode::{Q_CAPS, Q_CLIP, Q_STATS};
use crate::protocol::{FrameType, PROTO_VER, Resp, parse_resp};
use crate::types::{ClipStatus, Stats};

use super::correlation;
use super::keepalive::KeepaliveCtx;
use super::reconnect::reapply_locked;
use super::slot::TransportSlot;
use super::write_frame;

// How often a recovery asks whether the clone is up: closely at first, then at a walk for a box that
// has no device to clone.
const CLONE_POLL: Duration = Duration::from_millis(50);
const CLONE_POLL_SLOW: Duration = Duration::from_millis(500);
const CLONE_POLL_CLOSE: Duration = Duration::from_secs(5);
const QUERY_TIMEOUT: Duration = Duration::from_millis(250);
const QUERY_SLICE: Duration = Duration::from_millis(20);
// How long after a command that can present the clone again the keepalive looks for it every slice.
const REPRESENT_WATCH: Duration = Duration::from_secs(5);

/// What sent the box through the recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cause {
    /// The device chip booted.
    Restart,
    /// The box released the session without a boot: a re-clone, a detach, the inter-chip link
    /// dropping, a reset or a silence.
    Released,
}

/// Where one step of a recovery left it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Done,
    /// No clone yet, or no link: ask again later.
    Wait,
    /// A hello came after the clone answered: another boot to recover from.
    Again,
}

/// A recovery the keepalive owes, and when it next asks whether the clone is up.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pending {
    pub(crate) cause: Cause,
    since: Instant,
    pub(crate) next: Instant,
}

impl Pending {
    pub(crate) fn new(cause: Cause, now: Instant) -> Pending {
        Pending {
            cause,
            since: now,
            next: now,
        }
    }

    pub(crate) fn wait(&mut self, now: Instant) {
        let gap = if now - self.since < CLONE_POLL_CLOSE {
            CLONE_POLL
        } else {
            CLONE_POLL_SLOW
        };
        self.next = now + gap;
    }
}

/// The device chip's unsolicited hellos as the reader saw them, the recovery one of them owes, and
/// the box's `RESP(STATS)` session counter.
#[derive(Debug, Default)]
pub(crate) struct RestartWatch {
    state: Mutex<Watch>,
}

#[derive(Debug, Default)]
struct Watch {
    // The chip has answered this link since its last hello, so another hello is a new boot.
    armed: bool,
    // One boot sends two hellos (at boot and on first contact); the second lands while this is set.
    recovering: bool,
    owed: bool,
    hellos: u64,
    // A hello landed while a recovery was running: a boot to count when it ends.
    booted: bool,
    // `RESP(STATS)` session when last read; the box moves it each time it releases what a host set.
    session: Option<u16>,
    represent_until: Option<Instant>,
}

impl RestartWatch {
    pub(crate) fn note_reply(&self) {
        self.state.lock().armed = true;
    }

    // A hello the crate did not ask for, which owes a recovery when it is a new boot. A box back on
    // another protocol is refused the way a reconnect refuses one: every command fails with it.
    pub(crate) fn note_hello(&self, payload: &[u8], transport: &TransportSlot) {
        let proto = match parse_resp(payload) {
            Some(Resp::Version(v)) => v.proto_ver,
            _ => return,
        };
        if proto != PROTO_VER {
            trace_event!(
                target: "medius::device",
                tracing::Level::WARN,
                got = proto,
                expected = PROTO_VER,
                "restart: the box came back on another protocol",
            );
            transport.refuse(proto);
            return;
        }
        // Back on this build's protocol, after a refusal: a boot like any other.
        let was_refused = transport.refused().is_some();
        transport.accept();
        let mut w = self.state.lock();
        w.armed |= was_refused;
        if w.recovering {
            w.hellos += 1;
            w.booted = true;
            return;
        }
        if !w.armed {
            return;
        }
        w.armed = false;
        w.recovering = true;
        w.owed = true;
        w.hellos += 1;
        trace_event!(
            target: "medius::device",
            tracing::Level::INFO,
            "device chip restarted",
        );
    }

    // A reconnect found the chip freshly booted.
    pub(crate) fn begin(&self) {
        let mut w = self.state.lock();
        w.recovering = true;
        w.owed = true;
        w.hellos += 1;
    }

    pub(crate) fn take_owed(&self) -> bool {
        let mut w = self.state.lock();
        if w.owed {
            w.recovering = true;
        }
        std::mem::take(&mut w.owed)
    }

    fn hellos(&self) -> u64 {
        self.state.lock().hellos
    }

    // Ends the recovery unless a hello arrived after `seen`, which is another boot.
    fn finish(&self, seen: u64) -> bool {
        let mut w = self.state.lock();
        if w.hellos != seen {
            return false;
        }
        w.recovering = false;
        true
    }

    pub(crate) fn note_session(&self, session: u16) {
        self.state.lock().session = Some(session);
    }

    fn session_is(&self, session: u16) -> bool {
        self.state.lock().session == Some(session)
    }

    // Records `session` and says whether it moved since the last reading.
    fn session_moved(&self, session: u16) -> bool {
        let mut w = self.state.lock();
        w.session.replace(session).is_some_and(|was| was != session)
    }

    // A command went out that may present the clone again.
    pub(crate) fn expect_represent(&self) {
        self.state.lock().represent_until = Some(Instant::now() + REPRESENT_WATCH);
    }

    pub(crate) fn expecting_represent(&self, now: Instant) -> bool {
        self.state.lock().represent_until.is_some_and(|t| now < t)
    }

    // The close watch stays open: a release can come in two parts (the opt-in going off drops its
    // rules at once and presents the clone again later), and the second needs the same watch.
    pub(crate) fn begin_release(&self) {
        self.state.lock().recovering = true;
    }

    fn take_booted(&self) -> bool {
        std::mem::take(&mut self.state.lock().booted)
    }
}

// Whether the box released the session a host set since the last reading of its counter.
pub(crate) fn session_released(ctx: &KeepaliveCtx) -> bool {
    let Some(stats) = read_stats(ctx) else {
        return false;
    };
    if !ctx.restart.session_moved(stats.session) {
        return false;
    }
    trace_event!(
        target: "medius::device",
        tracing::Level::INFO,
        session = stats.session,
        "the box released the session",
    );
    true
}

fn read_stats(ctx: &KeepaliveCtx) -> Option<Stats> {
    match query(ctx, Q_STATS).as_deref().and_then(parse_resp) {
        Some(Resp::Stats(s)) => Some(s),
        _ => None,
    }
}

// The declared button count when a clone is up; `None` when none is, or the box did not answer.
fn clone_up(ctx: &KeepaliveCtx) -> Option<u8> {
    match query(ctx, Q_CAPS).as_deref().and_then(parse_resp) {
        Some(Resp::Caps(c)) if c.mouse.n_hid > 0 => Some(c.mouse.n_buttons),
        _ => None,
    }
}

// What the box's ring holds, noted against the append it was read after. A reading that never came
// says nothing, and one taken across a release or a boot is left to that recovery, which reports the
// loss: noted here it would clear the ring with nothing lost.
pub(crate) fn read_ring(ctx: &KeepaliveCtx) {
    let seen = ctx.desired.lock().clip_ring_gen();
    let hellos = ctx.restart.hellos();
    let Some(status) = query(ctx, Q_CLIP).and_then(|p| ClipStatus::from_payload(&p)) else {
        return;
    };
    let Some(stats) = read_stats(ctx) else {
        return;
    };
    if ctx.restart.hellos() != hellos || !ctx.restart.session_is(stats.session) {
        return;
    }
    ctx.desired.lock().clip_note_ring(seen, &status, false);
}

// One step of a recovery, on the keepalive thread. What the crate holds goes back only to a clone that
// is up, since the box drops a lock, a transform or a clip append sent before it exists; a release
// that left the clone standing (the link, a silence, a short detach) finds it up at once.
pub(crate) fn step(ctx: &KeepaliveCtx, cause: Cause) -> Outcome {
    let Some(buttons) = clone_up(ctx) else {
        return Outcome::Wait;
    };
    // Read after the reply, so every hello the box sent before it is counted.
    let seen = ctx.restart.hellos();
    if buttons > 0 {
        ctx.desired.lock().note_declared_buttons(buttons);
    }
    // Everything released up to this reading is answered by the re-send below.
    let Some(before) = read_stats(ctx).map(|s| s.session) else {
        return Outcome::Wait;
    };
    let sent = {
        let _serial = ctx.catch_lock.lock();
        reapply_locked(
            &ctx.transport,
            &ctx.write_lock,
            &ctx.seq,
            &ctx.counters,
            &ctx.desired,
            true,
        )
    };
    if sent.is_err() {
        return Outcome::Wait;
    }
    // A release comes in two parts at times (a detach counts at once and takes the clone down after
    // its grace), and a re-send between them lands on a clone that is going. It stuck, and the ring
    // read beside it describes this release, only if the clone is still up and nothing was released
    // since.
    let up = clone_up(ctx).is_some();
    let ring_seen = ctx.desired.lock().clip_ring_gen();
    let ring = query(ctx, Q_CLIP).and_then(|p| ClipStatus::from_payload(&p));
    let settled = up && read_stats(ctx).map(|s| s.session) == Some(before);
    if !settled {
        return Outcome::Wait;
    }
    if let Some(status) = ring {
        ctx.desired.lock().clip_note_ring(ring_seen, &status, true);
    }
    ctx.restart.note_session(before);
    if ctx.restart.take_booted() || cause == Cause::Restart {
        ctx.counters.inc_restarts();
    }
    if ctx.restart.finish(seen) {
        Outcome::Done
    } else {
        Outcome::Again
    }
}

fn query(ctx: &KeepaliveCtx, what: u8) -> Option<Vec<u8>> {
    query_sent(ctx, what).flatten()
}

// One `QUERY` and its reply. The outer `None` is a frame that never went out. The wait gives way to a
// device being dropped.
fn query_sent(ctx: &KeepaliveCtx, what: u8) -> Option<Option<Vec<u8>>> {
    let (seq, gen_id, rx) = correlation::register(
        &ctx.pending,
        &ctx.query_gen,
        &ctx.seq,
        FrameType::Resp,
        what,
    );
    let sent = write_frame(
        &ctx.transport,
        &ctx.write_lock,
        &ctx.counters,
        seq,
        FrameType::Query,
        &query_payload(what),
    );
    if sent.is_err() {
        correlation::cancel(&ctx.pending, seq, gen_id);
        return None;
    }
    let deadline = Instant::now() + QUERY_TIMEOUT;
    let reply = loop {
        match rx.recv_timeout(QUERY_SLICE) {
            Ok(reply) => break Some(reply),
            Err(_) if ctx.stop.load(Ordering::SeqCst) || Instant::now() >= deadline => break None,
            Err(_) => {}
        }
    };
    if reply.is_none() {
        correlation::cancel(&ctx.pending, seq, gen_id);
    }
    Some(reply)
}
