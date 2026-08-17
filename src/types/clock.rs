//! Time: which chip stamped an event, the measured offset between the two, and putting a box stamp
//! on this machine's clock.

use core::time::Duration;
use std::time::Instant;

use crate::protocol::opcode::CLK_RATE_NONE;
use crate::types::{CatchEvent, InputEvent, MotionEvent, TrafficEvent, UsageSnapshot};

/// Which chip's clock stamped an event.
///
/// The two chips boot independently, so a stamp is only meaningful against another from the same
/// domain. [`Timeline`] puts both on one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ClockDomain {
    /// The device-facing chip, stamped in USB interrupt context when the real device's transfer
    /// completed. Everything the real device produced carries this.
    HostChip,
    /// The PC-facing chip, stamped at the tap. Everything the PC produced, and everything the clone
    /// emitted, carries this — the host chip never saw those bytes.
    DeviceChip,
}

impl ClockDomain {
    /// Both domains, in wire order.
    pub const ALL: [ClockDomain; 2] = [ClockDomain::HostChip, ClockDomain::DeviceChip];

    pub(crate) fn from_u8(v: u8) -> ClockDomain {
        if v == 0 {
            ClockDomain::HostChip
        } else {
            ClockDomain::DeviceChip
        }
    }

    fn index(self) -> usize {
        match self {
            ClockDomain::HostChip => 0,
            ClockDomain::DeviceChip => 1,
        }
    }
}

/// The measured difference between the two chips' clocks, from `RESP(CATCH)` (§4.9).
///
/// Measured with a four-timestamp exchange over the inter-chip link, stamped as each frame reaches
/// the wire rather than when it is queued — queueing is the largest and most variable delay on that
/// link, so stamping late removes it from the measurement instead of filtering around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClockEstimate {
    /// The host chip's clock minus the device chip's, in microseconds.
    pub offset_us: i32,
    /// Relative drift between the two crystals in parts per billion, or `None` when the box has not
    /// fitted one. That is a different answer from a fitted zero, which says the two crystals match:
    /// on a link busy enough that too few clean exchanges reach the box's filter, no fit is made at
    /// all — precisely when assuming no drift is least safe.
    pub rate_ppb: Option<i32>,
    /// Best measured round trip in the window. The offset is good to about half of this.
    pub delay_us: u16,
    /// Age of the estimate, or `None` if the box has no estimate yet — which is how a caller tells
    /// that apart from an offset that happens to be zero.
    pub age: Option<Duration>,
}

impl ClockEstimate {
    /// Decode the 12-byte clock block inside a `RESP(CATCH)` payload.
    pub(crate) fn from_payload(p: &[u8]) -> Option<ClockEstimate> {
        if p.len() < 12 {
            return None;
        }
        let age_ms = u16::from_le_bytes([p[10], p[11]]);
        Some(ClockEstimate {
            offset_us: i32::from_le_bytes([p[0], p[1], p[2], p[3]]),
            rate_ppb: match i32::from_le_bytes([p[4], p[5], p[6], p[7]]) {
                CLK_RATE_NONE => None,
                v => Some(v),
            },
            delay_us: u16::from_le_bytes([p[8], p[9]]),
            // 0xFFFF is the box saying it has no estimate, which is not the same as an estimate that
            // happens to be zero microseconds old.
            age: if age_ms == u16::MAX {
                None
            } else {
                Some(Duration::from_millis(age_ms as u64))
            },
        })
    }

    /// Half the measured round trip: the bound on how wrong [`Self::offset_us`] can be.
    pub fn error_bound_us(&self) -> u16 {
        self.delay_us / 2
    }

    /// Translate a device-chip stamp into the host chip's domain. `None` when there is no estimate.
    ///
    /// This applies the offset alone. The box corrects for drift against the moment IT measured the
    /// offset, which is a reference this side does not have — so over a long-lived stream the two
    /// crystals pull apart at up to 20 ppm, roughly 20 us per second of estimate age. Re-read
    /// [`Device::query_catch`](crate::Device::query_catch) when [`Self::age`] has grown large
    /// relative to [`Self::error_bound_us`], and use [`Self::drift_us_over`] to see how much it costs.
    pub fn to_host_domain(&self, device_us: u32) -> Option<i64> {
        self.age?;
        Some(device_us as i64 + self.offset_us as i64)
    }

    /// How far the offset has drifted over `elapsed`, in microseconds. Add this to
    /// [`Self::error_bound_us`] for the honest bound on a stamp translated `elapsed` after the
    /// estimate was taken.
    /// 0 when the box has fitted no rate: it is what is known, not a claim that there is no drift.
    pub fn drift_us_over(&self, elapsed: Duration) -> i64 {
        let Some(ppb) = self.rate_ppb else { return 0 };
        (elapsed.as_micros() as i64).saturating_mul(ppb as i64) / 1_000_000_000
    }
}

/// Anything carrying a box timestamp and the domain that produced it.
///
/// [`Timeline`] takes this rather than one concrete event, so a decoded
/// [`InputEvent`] can be placed on the host clock exactly like a raw
/// [`CatchEvent`] — the two features compose.
pub trait Timestamped {
    /// The stamp, in the producing chip's microseconds.
    fn ts_us(&self) -> u32;
    /// Which chip's clock produced it.
    fn clock(&self) -> ClockDomain;
}

impl Timestamped for CatchEvent {
    fn ts_us(&self) -> u32 {
        CatchEvent::ts_us(self)
    }
    fn clock(&self) -> ClockDomain {
        CatchEvent::clock(self)
    }
}

impl Timestamped for InputEvent {
    fn ts_us(&self) -> u32 {
        self.ts_us
    }
    fn clock(&self) -> ClockDomain {
        self.clock
    }
}

impl Timestamped for MotionEvent {
    fn ts_us(&self) -> u32 {
        self.ts_us
    }
    fn clock(&self) -> ClockDomain {
        self.clock
    }
}

impl Timestamped for UsageSnapshot {
    fn ts_us(&self) -> u32 {
        self.ts_us
    }
    fn clock(&self) -> ClockDomain {
        self.clock
    }
}

impl Timestamped for TrafficEvent {
    fn ts_us(&self) -> u32 {
        self.ts_us
    }
    fn clock(&self) -> ClockDomain {
        self.clock
    }
}

/// One event placed on this machine's clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Stamped {
    /// When the event happened, on this machine's monotonic clock.
    pub host: Instant,
    /// The event's own stamp, unwrapped past the 32-bit rollover.
    pub box_us: u64,
    /// How much later than the measured floor this event reached you. Jitter, not latency: the
    /// constant part of the delay is unknowable from here and falls out of [`Self::host`].
    pub excess: Duration,
}

/// Samples per floor block. The floor is the minimum over the current block plus the previous one,
/// so its age is bounded by two blocks — about 8 s at 1 kHz. An all-time minimum cannot be right: the
/// two crystals drift at up to 20 ppm, so a floor from an hour ago is 72 ms wrong and only ever gets
/// worse. Bounding the window lets the floor rise as well as fall.
const FLOOR_BLOCK: u32 = 4096;

/// A [`Stamped::host`] correction larger than this re-anchors the timeline instead of being absorbed.
/// Small corrections are smoothed so time never visibly runs backwards; a large one is the estimate
/// being wrong, and holding a wrong answer to keep it monotonic wedges the stream for as long as the
/// error lasts.
const RESYNC_NS: u64 = 1_000_000;

/// Half the 32-bit stamp range. A backward step shorter than this is the box's own priority queues
/// delivering out of tap order, not a rollover.
const HALF_RANGE: u32 = 1 << 31;

#[derive(Debug, Default, Clone, Copy)]
struct DomainState {
    epoch: u64,
    last: u32,
    seen: bool,
    min_now: Option<i128>,
    min_prev: Option<i128>,
    block: u32,
    last_host_ns: u64,
    samples: u64,
}

impl DomainState {
    fn floor(&self) -> Option<i128> {
        match (self.min_now, self.min_prev) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    fn push_lag(&mut self, lag_ns: i128) -> i128 {
        self.min_now = Some(self.min_now.map_or(lag_ns, |m| m.min(lag_ns)));
        self.block += 1;
        if self.block >= FLOOR_BLOCK {
            self.min_prev = self.min_now;
            self.min_now = None;
            self.block = 0;
        }
        self.floor().unwrap_or(lag_ns)
    }
}

/// Puts box stamps on this machine's clock.
///
/// A catch stamp is microseconds on a chip that booted before this process did: it wraps every ~71.6
/// minutes, restarts at zero if that chip reboots, and has no relation to any clock here. Feed every
/// event in as you receive it.
///
/// ```no_run
/// # use medius::{CatchFilter, Device, Timeline};
/// # fn f(dev: &Device) -> medius::Result<()> {
/// let events = dev.catch_events(CatchFilter::all_input())?;
/// let mut time = Timeline::new();
/// for ev in &events {
///     println!("{ev:?} at {:?}", time.observe(&ev).host);
/// }
/// # Ok(()) }
/// ```
///
/// Each domain is tracked separately, so stamps from both chips land on one comparable timeline
/// without needing a [`ClockEstimate`]: each domain's floor absorbs its own chip's offset.
///
/// # What it is good for, and what it is not
///
/// The mapping keeps a per-domain **minimum** of (elapsed here − elapsed on the box) over a bounded
/// window, rather than an average: an event can be delivered late but never early, so the fastest
/// recent delivery is the closest thing to the truth.
///
/// That makes it a good answer for events arriving one at a time and a poor one for a burst. When a
/// slow consumer stalls and then drains a backlog, every event in the backlog arrives at nearly the
/// same instant, and no filter over arrival times can recover when they were really produced: the
/// burst maps into the span it was drained in, and reports little [`Stamped::excess`]. Read
/// [`Stamped::box_us`] when you need the box's own spacing, which is exact.
///
/// Ordering across domains is good to the difference in how far the two floors have converged, which
/// is largest just after a domain's first event. Within one domain, [`Stamped::box_us`] is exact.
#[derive(Debug)]
pub struct Timeline {
    origin: Instant,
    domains: [DomainState; 2],
}

impl Default for Timeline {
    fn default() -> Timeline {
        Timeline::new()
    }
}

impl Timeline {
    /// A fresh timeline, anchored to now.
    pub fn new() -> Timeline {
        Timeline {
            origin: Instant::now(),
            domains: [DomainState::default(); 2],
        }
    }

    /// Place an event on this machine's clock, taking the arrival as now. Call it as soon as the
    /// event arrives — [`Stamped::excess`] includes however long you waited.
    pub fn observe(&mut self, event: &impl Timestamped) -> Stamped {
        self.observe_at(event, Instant::now())
    }

    /// [`Self::observe`] with the arrival supplied, for replaying a capture.
    pub fn observe_at(&mut self, event: &impl Timestamped, now: Instant) -> Stamped {
        self.observe_stamp(event.ts_us(), event.clock(), now)
    }

    /// [`Self::observe_at`] for a stamp and domain held on their own, rather than inside an event.
    pub fn observe_stamp(&mut self, ts_us: u32, domain: ClockDomain, now: Instant) -> Stamped {
        let box_us = self.box_us_of(ts_us, domain);
        let elapsed_ns = now.saturating_duration_since(self.origin).as_nanos() as i128;
        let box_ns = (box_us as i128) * 1_000;
        let lag_ns = elapsed_ns - box_ns;

        let d = &mut self.domains[domain.index()];
        d.samples += 1;
        let floor = d.push_lag(lag_ns);

        // Non-negative by construction: at the sample that set the floor, box_ns + floor is exactly
        // that sample's own elapsed, and elapsed since our own origin cannot be negative. The cast is
        // still saturated -- a fabricated epoch would otherwise wrap silently into a small number.
        let raw_host_ns = (box_ns + floor).max(0).min(u64::MAX as i128) as u64;
        // A small correction is absorbed so the timeline does not visibly run backwards; a large one
        // means the estimate was wrong, and pinning a wrong answer to stay monotonic would freeze the
        // stream for as long as the error lasts.
        let host_ns = if raw_host_ns + RESYNC_NS < d.last_host_ns {
            raw_host_ns
        } else {
            raw_host_ns.max(d.last_host_ns)
        };
        d.last_host_ns = host_ns;
        Stamped {
            host: self.origin + Duration::from_nanos(host_ns),
            box_us,
            excess: Duration::from_nanos((lag_ns - floor).max(0).min(u64::MAX as i128) as u64),
        }
    }

    /// The event's stamp unwrapped past the 32-bit rollover, monotonic within its domain.
    ///
    /// The box drains its taps through strict-priority queues, so a later-tapped event can arrive
    /// first. A backward step shorter than half the 32-bit range is read as that reordering and keeps
    /// its place on the timeline; only a step longer than half the range is a rollover. Treating
    /// every backward step as a rollover turned a 1 µs inversion into a permanent 71.6-minute jump.
    ///
    /// A reboot restarts the clock at zero, which this cannot tell from a very large jump. Nothing on
    /// the wire announces a chip reboot, so call [`Self::reset`] for a chip you know restarted.
    pub fn box_us(&mut self, event: &impl Timestamped) -> u64 {
        self.box_us_of(event.ts_us(), event.clock())
    }

    /// [`Self::box_us`] for a stamp and domain held on their own.
    pub fn box_us_of(&mut self, raw: u32, domain: ClockDomain) -> u64 {
        let d = &mut self.domains[domain.index()];
        if !d.seen {
            d.seen = true;
            d.last = raw;
            return d.epoch + raw as u64;
        }
        // Forward distance from the high-water mark, modulo the 32-bit range.
        if raw.wrapping_sub(d.last) <= HALF_RANGE {
            if raw < d.last {
                d.epoch += 1u64 << 32;
            }
            d.last = raw;
            d.epoch + raw as u64
        } else {
            // Out of order: same epoch line, and the high-water mark must not regress. Saturating
            // because a straggler older than the whole timeline has nowhere to go below zero.
            (d.epoch + d.last as u64).saturating_sub(d.last.wrapping_sub(raw) as u64)
        }
    }

    /// Forget one domain's rollover count and measured floor, for a chip that rebooted.
    pub fn reset(&mut self, domain: ClockDomain) {
        self.domains[domain.index()] = DomainState::default();
    }

    /// Events observed for a domain. The floor is a minimum over these: a handful is a loose
    /// estimate, a few hundred a tight one.
    pub fn samples(&self, domain: ClockDomain) -> u64 {
        self.domains[domain.index()].samples
    }
}
