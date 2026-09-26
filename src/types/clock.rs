//! Clock domains, the measured inter-chip offset, and mapping box stamps onto this machine's clock.

use core::time::Duration;
use std::time::Instant;

use crate::protocol::opcode::CLK_RATE_NONE;
use crate::types::{CatchEvent, InputEvent, MotionEvent, TrafficEvent, UsageSnapshot};

/// Chip whose clock stamped an event.
///
/// The chips boot independently, so a stamp compares only with stamps from the same domain.
/// [`Timeline`] puts both on one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ClockDomain {
    /// Device-facing chip, stamped in USB interrupt context when the real device's transfer
    /// completes. Carried by everything the real device produced.
    HostChip,
    /// PC-facing chip, stamped at the tap. Carried by everything the PC produced and the clone
    /// emitted; the host chip never sees those bytes.
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

/// Measured offset between the two chips' clocks, from `RESP(CATCH)` (§4.9).
///
/// Four-timestamp exchange over the inter-chip link, each frame stamped as it reaches the wire.
/// That keeps queueing, the link's largest and most variable delay, out of the measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClockEstimate {
    /// Host chip clock minus device chip clock, in microseconds.
    pub offset_us: i32,
    /// Relative drift between the crystals, in parts per billion; `None` when the box has fitted
    /// none. A fitted zero means the crystals match. A link too busy for enough clean exchanges to
    /// reach the box's filter gets no fit, exactly when assuming zero drift is least safe.
    pub rate_ppb: Option<i32>,
    /// Best round trip in the window; the offset is good to about half this.
    pub delay_us: u16,
    /// Estimate age; `None` when the box has no estimate yet, which tells that apart from a zero
    /// offset.
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
            // 0xFFFF: no estimate, distinct from a zero-age one.
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

    /// Device-chip stamp in the host chip's domain; `None` with no estimate.
    ///
    /// Applies the offset only. The box corrects drift against the instant it measured the offset,
    /// which this side lacks, so over a long stream the crystals diverge at up to 20 ppm, about
    /// 20 us per second of estimate age. Re-read [`Device::query_catch`](crate::Device::query_catch)
    /// when [`Self::age`] grows large relative to [`Self::error_bound_us`];
    /// [`Self::drift_us_over`] gives the cost.
    pub fn to_host_domain(&self, device_us: u32) -> Option<i64> {
        self.age?;
        Some(device_us as i64 + self.offset_us as i64)
    }

    /// Offset drift over `elapsed`, in microseconds; add [`Self::error_bound_us`] for the total bound
    /// on a stamp translated `elapsed` after the estimate. 0 when the box has fitted no rate, which
    /// does not mean zero drift.
    pub fn drift_us_over(&self, elapsed: Duration) -> i64 {
        let Some(ppb) = self.rate_ppb else { return 0 };
        (elapsed.as_micros() as i64).saturating_mul(ppb as i64) / 1_000_000_000
    }
}

/// Event carrying a box timestamp and the domain that produced it.
///
/// [`Timeline`] takes this, so it places a decoded [`InputEvent`] on the host clock exactly like a
/// raw [`CatchEvent`].
pub trait Timestamped {
    /// Stamp, in the producing chip's microseconds.
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

/// Event placed on this machine's clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Stamped {
    /// Event time on this machine's monotonic clock.
    pub host: Instant,
    /// Box stamp, unwrapped past the 32-bit rollover.
    pub box_us: u64,
    /// Arrival delay beyond the measured floor. Jitter only: the constant part of the delay is
    /// unknowable here and drops out of [`Self::host`].
    pub excess: Duration,
}

// Samples per floor block. The floor is the minimum over this block and the previous one, so it is
// at most two blocks (~8 s at 1 kHz) old.
const FLOOR_BLOCK: u32 = 4096;

// A `Stamped::host` correction past this re-anchors the timeline; smaller ones are absorbed.
const RESYNC_NS: u64 = 1_000_000;

// Half the 32-bit range. A shorter backward step is the box's priority queues delivering out of tap
// order, not a rollover.
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

/// Maps box stamps onto this machine's clock.
///
/// A catch stamp counts microseconds on a chip that booted before this process: it wraps every
/// ~71.6 minutes, restarts at zero when that chip reboots, and is unrelated to any clock here. Feed
/// in every event on receipt.
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
/// Domains are tracked separately and each floor absorbs its chip's offset, so both chips' stamps
/// share one comparable timeline with no [`ClockEstimate`].
///
/// # Accuracy
///
/// The mapping keeps a per-domain minimum of (elapsed here minus elapsed on the box) over a bounded
/// window: an event arrives late but never early, so the fastest recent delivery is closest to
/// true.
///
/// That suits events arriving one at a time, not bursts. A backlog drained after a consumer stall
/// arrives at nearly one instant, and no filter over arrival times recovers when it was produced:
/// the burst maps into the span it drained in and reports little [`Stamped::excess`].
///
/// Cross-domain ordering is good to the difference in how far the two floors have converged,
/// largest just after a domain's first event. Within one domain, [`Stamped::box_us`] is exact.
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
    /// Empty timeline anchored to now.
    pub fn new() -> Timeline {
        Timeline {
            origin: Instant::now(),
            domains: [DomainState::default(); 2],
        }
    }

    /// Places an event on this machine's clock, arrival taken as now. Call on arrival:
    /// [`Stamped::excess`] includes any wait.
    pub fn observe(&mut self, event: &impl Timestamped) -> Stamped {
        self.observe_at(event, Instant::now())
    }

    /// [`Self::observe`] with the arrival supplied, for replaying a capture.
    pub fn observe_at(&mut self, event: &impl Timestamped, now: Instant) -> Stamped {
        self.observe_stamp(event.ts_us(), event.clock(), now)
    }

    /// [`Self::observe_at`] for a bare stamp and domain.
    pub fn observe_stamp(&mut self, ts_us: u32, domain: ClockDomain, now: Instant) -> Stamped {
        let box_us = self.box_us_of(ts_us, domain);
        let elapsed_ns = now.saturating_duration_since(self.origin).as_nanos() as i128;
        let box_ns = (box_us as i128) * 1_000;
        let lag_ns = elapsed_ns - box_ns;

        let d = &mut self.domains[domain.index()];
        d.samples += 1;
        let floor = d.push_lag(lag_ns);

        // Non-negative: at the floor-setting sample, box_ns + floor equals its elapsed, which is >= 0.
        let raw_host_ns = (box_ns + floor).max(0).min(u64::MAX as i128) as u64;
        // Small corrections are absorbed to stay monotonic; a large one means the estimate was wrong,
        // and pinning it would freeze the stream for as long as the error lasts.
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

    /// Event stamp unwrapped past the 32-bit rollover, monotonic within its domain.
    ///
    /// The box drains taps through strict-priority queues, so a later-tapped event can arrive first.
    /// A backward step under half the 32-bit range is that reordering and keeps its place on the
    /// timeline; only a longer one is a rollover.
    ///
    /// A reboot restarts the clock at zero, indistinguishable here from a very large jump. Call
    /// [`Self::reset`] for a restarted chip; a device-chip restart raises
    /// [`CountersSnapshot::restarts`](crate::CountersSnapshot::restarts).
    pub fn box_us(&mut self, event: &impl Timestamped) -> u64 {
        self.box_us_of(event.ts_us(), event.clock())
    }

    /// [`Self::box_us`] for a bare stamp and domain.
    pub fn box_us_of(&mut self, raw: u32, domain: ClockDomain) -> u64 {
        let d = &mut self.domains[domain.index()];
        if !d.seen {
            d.seen = true;
            d.last = raw;
            return d.epoch + raw as u64;
        }
        if raw.wrapping_sub(d.last) <= HALF_RANGE {
            if raw < d.last {
                d.epoch += 1u64 << 32;
            }
            d.last = raw;
            d.epoch + raw as u64
        } else {
            // Out of order: same epoch, high-water mark unchanged. Saturates for a straggler older
            // than the whole timeline.
            (d.epoch + d.last as u64).saturating_sub(d.last.wrapping_sub(raw) as u64)
        }
    }

    /// Clears one domain's rollover count and measured floor, for a rebooted chip.
    pub fn reset(&mut self, domain: ClockDomain) {
        self.domains[domain.index()] = DomainState::default();
    }

    /// Events observed in a domain. The floor is a minimum over them: a handful gives a loose
    /// estimate, a few hundred a tight one.
    pub fn samples(&self, domain: ClockDomain) -> u64 {
        self.domains[domain.index()].samples
    }
}
