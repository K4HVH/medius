//! The precise tick clock behind the pacer thread (`pacer/clock.rs`, Task 4.1).
//!
//! [`PrecisePacer`] turns a fixed tick **period** into a stream of absolute deadlines and blocks until
//! each one — one `wait_next_tick()` per frame window.
//!
//! Drift-free by construction: the deadline advances by exactly one period per tick
//! (`deadline += period`) and the clock sleeps until that absolute instant, never "for a duration", so
//! a late tick does not push the following ticks out. Error is bounded per-tick rather than
//! accumulating — the reason a compiled host holds a steady 1 kHz where a `sleep(period)` loop drifts.
//! Mirrors `smooth_inject.c`'s `clock_nanosleep(TIMER_ABSTIME)` scheduler.
//!
//! Backends:
//! - **Linux** ([`LinuxClock`]): a raw [`libc::timespec`] deadline slept on with
//!   `clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME)`. A `std::time::Instant` cannot be converted to
//!   a raw `CLOCK_MONOTONIC` `timespec`, so the deadline is tracked as a `timespec` directly.
//! - **Windows** ([`WindowsClock`]): a high-resolution waitable timer armed per tick, then a short
//!   `QueryPerformanceCounter` spin trims residual jitter to the absolute grid.
//! - **Fallback** ([`FallbackClock`]): an [`Instant`]-grid hybrid — sleep most of the way, spin the
//!   last sliver.
//!
//! The first deadline is seeded lazily on the first `wait_next_tick`, so construction does no syscall.

use std::time::Duration;

/// A precise, drift-free tick clock: each [`wait_next_tick`](PrecisePacer::wait_next_tick) blocks
/// until the next absolute deadline on a fixed-period grid. See the [module docs](self) for the
/// per-platform backend and why absolute stepping avoids cumulative drift.
#[derive(Debug)]
pub(crate) struct PrecisePacer {
    period: Duration,
    backend: Backend,
}

impl PrecisePacer {
    /// Create a pacer ticking every `period`. Does no syscall yet — the grid origin is seeded on the
    /// first [`wait_next_tick`](Self::wait_next_tick). A zero period is clamped to 1 ns so the deadline
    /// always advances.
    pub(crate) fn new(period: Duration) -> Self {
        let period = period.max(Duration::from_nanos(1));
        PrecisePacer {
            period,
            backend: Backend::new(),
        }
    }

    /// The current tick period.
    pub(crate) fn period(&self) -> Duration {
        self.period
    }

    /// Change the tick period from the next [`wait_next_tick`](Self::wait_next_tick). The next deadline
    /// is the already-seeded grid point plus the new period, so a rate change doesn't reset the grid.
    pub(crate) fn set_period(&mut self, period: Duration) {
        self.period = period.max(Duration::from_nanos(1));
    }

    /// Block until the next tick deadline. The first call seeds the grid origin and returns almost
    /// immediately; later calls advance the deadline one period and sleep until that absolute instant.
    pub(crate) fn wait_next_tick(&mut self) {
        self.backend.wait_next_tick(self.period);
    }
}

// ---------------------------------------------------------------------------------------------------
// Backend selection
// ---------------------------------------------------------------------------------------------------

#[cfg(target_os = "linux")]
type Backend = LinuxClock;

#[cfg(windows)]
type Backend = WindowsClock;

#[cfg(not(any(target_os = "linux", windows)))]
type Backend = FallbackClock;

// ---------------------------------------------------------------------------------------------------
// Linux: clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME)
// ---------------------------------------------------------------------------------------------------

#[cfg(target_os = "linux")]
const NANOS_PER_SEC: i64 = 1_000_000_000;

/// Linux backend: an absolute `CLOCK_MONOTONIC` deadline tracked as a [`libc::timespec`] and slept on
/// with `clock_nanosleep(TIMER_ABSTIME)`.
#[cfg(target_os = "linux")]
#[derive(Debug)]
pub(crate) struct LinuxClock {
    /// The next absolute deadline, or `None` until the first tick seeds the grid.
    deadline: Option<libc::timespec>,
}

#[cfg(target_os = "linux")]
impl LinuxClock {
    fn new() -> Self {
        LinuxClock { deadline: None }
    }

    /// Read `CLOCK_MONOTONIC` into a `timespec`.
    fn now() -> libc::timespec {
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: `ts` is valid and writable; `clock_gettime` fully initializes it.
        unsafe {
            libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
        }
        ts
    }

    /// Advance `deadline` by `period`, normalizing `tv_nsec` into `[0, 1e9)`.
    fn advance(deadline: &mut libc::timespec, period: Duration) {
        deadline.tv_nsec += period.subsec_nanos() as i64;
        deadline.tv_sec += period.as_secs() as libc::time_t;
        while deadline.tv_nsec >= NANOS_PER_SEC {
            deadline.tv_nsec -= NANOS_PER_SEC;
            deadline.tv_sec += 1;
        }
    }

    /// Sleep until the absolute `deadline`, retrying on `EINTR`.
    fn sleep_until(deadline: &libc::timespec) {
        loop {
            // SAFETY: `deadline` is a valid, normalized `timespec`; the null remainder is ignored under
            // TIMER_ABSTIME. The call only reads `*deadline` and blocks.
            let rc = unsafe {
                libc::clock_nanosleep(
                    libc::CLOCK_MONOTONIC,
                    libc::TIMER_ABSTIME,
                    deadline,
                    std::ptr::null_mut(),
                )
            };
            // clock_nanosleep returns the error directly (not errno); only EINTR retries.
            if rc != libc::EINTR {
                return;
            }
        }
    }

    fn wait_next_tick(&mut self, period: Duration) {
        match self.deadline.as_mut() {
            None => {
                self.deadline = Some(Self::now());
            }
            Some(deadline) => {
                Self::advance(deadline, period);
                Self::sleep_until(deadline);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// Windows: high-resolution waitable timer + QPC spin trim
// ---------------------------------------------------------------------------------------------------

/// Windows backend: a high-resolution waitable timer armed per tick with a negative relative due-time,
/// plus a short `QueryPerformanceCounter` spin to trim residual jitter to the absolute grid.
///
/// The grid is tracked in QPC ticks (absolute, monotonic), so the relative due-time is recomputed from
/// the absolute next deadline each tick — lateness does not accumulate.
#[cfg(windows)]
#[derive(Debug)]
pub(crate) struct WindowsClock {
    /// The waitable timer handle, or null if creation failed (then we fall back to a spin).
    timer: windows_sys::Win32::Foundation::HANDLE,
    /// QPC ticks per second, cached once.
    qpc_freq: i64,
    /// The next absolute deadline in QPC ticks, or `None` until the first tick seeds it.
    deadline_qpc: Option<i64>,
}

// SAFETY: the only non-`Send` field is the timer HANDLE — a kernel object usable from any thread,
// owned exclusively and moved into the single pacer thread. Never shared, so `Sync` is not claimed.
#[cfg(windows)]
unsafe impl Send for WindowsClock {}

#[cfg(windows)]
impl WindowsClock {
    fn new() -> Self {
        use windows_sys::Win32::System::Performance::QueryPerformanceFrequency;
        use windows_sys::Win32::System::Threading::{
            CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CreateWaitableTimerExW, TIMER_ALL_ACCESS,
        };

        // SAFETY: all-null args request a default unnamed timer, owned here and closed in Drop. A null
        // return is tolerated (handled as spin-only wait).
        let timer = unsafe {
            CreateWaitableTimerExW(
                std::ptr::null(),
                std::ptr::null(),
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                TIMER_ALL_ACCESS,
            )
        };

        let mut freq: i64 = 0;
        // SAFETY: `freq` is valid and writable; the call only writes through it. We still guard a zero
        // below when converting durations to ticks.
        unsafe {
            QueryPerformanceFrequency(&mut freq);
        }

        WindowsClock {
            timer,
            qpc_freq: if freq > 0 { freq } else { 1 },
            deadline_qpc: None,
        }
    }

    /// Read the current QPC counter value.
    fn now_qpc() -> i64 {
        use windows_sys::Win32::System::Performance::QueryPerformanceCounter;
        let mut c: i64 = 0;
        // SAFETY: `c` is valid and writable; the call only writes through it.
        unsafe {
            QueryPerformanceCounter(&mut c);
        }
        c
    }

    /// Convert a `Duration` to QPC ticks (saturating; the period is tiny, so no realistic overflow).
    fn duration_to_qpc(&self, d: Duration) -> i64 {
        // u128 intermediate avoids overflow, then clamp to i64.
        let ticks = d.as_nanos().saturating_mul(self.qpc_freq as u128) / 1_000_000_000u128;
        ticks.min(i64::MAX as u128) as i64
    }

    fn wait_next_tick(&mut self, period: Duration) {
        let now = Self::now_qpc();
        let next = match self.deadline_qpc {
            None => {
                self.deadline_qpc = Some(now);
                return;
            }
            Some(prev) => {
                // Absolute stepping: next = prev + period regardless of wake lateness → no drift.
                let next = prev.saturating_add(self.duration_to_qpc(period));
                self.deadline_qpc = Some(next);
                next
            }
        };

        // Already past the deadline (a very slow tick): don't sleep — catch up on the grid.
        let remaining_ticks = next - now;
        if remaining_ticks <= 0 {
            return;
        }

        self.coarse_wait(remaining_ticks);

        // Fine spin to the absolute QPC deadline.
        while Self::now_qpc() < next {
            std::hint::spin_loop();
        }
    }

    /// Arm the waitable timer for most of `remaining_ticks` and block on it, leaving a trailing window
    /// for the QPC spin. A null timer handle makes this a no-op (the spin realizes the whole wait).
    fn coarse_wait(&self, remaining_ticks: i64) {
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows_sys::Win32::System::Threading::{
            INFINITE, SetWaitableTimer, WaitForSingleObject,
        };

        if self.timer.is_null() || self.timer == INVALID_HANDLE_VALUE {
            return;
        }

        // Trailing spin window: ~250 µs, expressed in QPC ticks, reserved for the fine spin so the
        // coarse timer never overshoots the absolute deadline.
        let spin_margin = self.qpc_freq / 4000; // freq/4000 = 0.25 ms in ticks
        let coarse_ticks = remaining_ticks - spin_margin;
        if coarse_ticks <= 0 {
            return; // too close — let the spin handle all of it
        }

        // Coarse ticks → 100 ns units, negated for a *relative* due-time.
        let hundred_ns = (coarse_ticks as i128 * 10_000_000i128) / self.qpc_freq as i128;
        let due: i64 = -((hundred_ns.min(i64::MAX as i128)) as i64);

        // SAFETY: `self.timer` is a live owned HANDLE; `&due` is valid. Period 0 + null routine make a
        // one-shot timer with no APC; the call only reads `*due`. On failure we fall through to spin.
        let armed = unsafe {
            SetWaitableTimer(
                self.timer,
                &due,
                0,
                None,
                std::ptr::null(),
                0, // FALSE — don't resume a suspended system
            )
        };
        if armed == 0 {
            return; // arming failed → let the spin realize the wait
        }

        // SAFETY: `self.timer` is a live HANDLE; INFINITE blocks until it signals. An early wake is
        // corrected by the QPC spin afterwards.
        unsafe {
            WaitForSingleObject(self.timer, INFINITE);
        }
    }
}

#[cfg(windows)]
impl Drop for WindowsClock {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        if !self.timer.is_null() && self.timer != INVALID_HANDLE_VALUE {
            // SAFETY: owned HANDLE from `new`, closed exactly once here.
            unsafe {
                CloseHandle(self.timer);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// Fallback: Instant-based hybrid sleep + spin
// ---------------------------------------------------------------------------------------------------

/// Portable fallback backend: an [`Instant`]-grid hybrid that sleeps most of the remaining time then
/// spins the last sliver.
#[cfg(not(any(target_os = "linux", windows)))]
#[derive(Debug)]
pub(crate) struct FallbackClock {
    deadline: Option<std::time::Instant>,
}

#[cfg(not(any(target_os = "linux", windows)))]
impl FallbackClock {
    /// Remaining time left for the busy-spin (sub-ms residual trim).
    const SPIN_MARGIN: Duration = Duration::from_micros(300);

    fn new() -> Self {
        FallbackClock { deadline: None }
    }

    fn wait_next_tick(&mut self, period: Duration) {
        let now = std::time::Instant::now();
        let next = match self.deadline {
            None => {
                self.deadline = Some(now);
                return;
            }
            Some(prev) => {
                let next = prev + period;
                self.deadline = Some(next);
                next
            }
        };
        // Coarse sleep up to the spin margin, then spin to the absolute deadline.
        if let Some(coarse_until) = next.checked_sub(Self::SPIN_MARGIN) {
            let now = std::time::Instant::now();
            if coarse_until > now {
                std::thread::sleep(coarse_until - now);
            }
        }
        while std::time::Instant::now() < next {
            std::hint::spin_loop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// 50 ticks at 1 ms should take roughly 50 ms. Bounds are loose for CI: this only proves the pacer
    /// actually paces (not a microsecond busy-spin, nor a far-too-slow drifting loop).
    #[test]
    fn fifty_one_ms_ticks_take_about_fifty_ms() {
        let mut pacer = PrecisePacer::new(Duration::from_millis(1));
        let start = Instant::now();
        for _ in 0..50 {
            pacer.wait_next_tick();
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_millis(40),
            "50×1ms finished too fast ({elapsed:?}) — not actually pacing"
        );
        assert!(
            elapsed <= Duration::from_millis(120),
            "50×1ms took too long ({elapsed:?}) — excessive drift/overhead"
        );
    }

    /// The first `wait_next_tick` only seeds the grid; it must not block for a whole period.
    #[test]
    fn first_tick_seeds_without_sleeping() {
        let mut pacer = PrecisePacer::new(Duration::from_millis(50));
        let start = Instant::now();
        pacer.wait_next_tick(); // seed only
        assert!(
            start.elapsed() < Duration::from_millis(20),
            "first tick should seed the grid, not sleep a full period"
        );
    }

    /// No cumulative drift: after N ticks the elapsed tracks N×period within a small slack, and ticks
    /// advance monotonically.
    #[test]
    fn deadlines_advance_without_cumulative_drift() {
        let period = Duration::from_millis(1);
        let n = 100u32;
        let mut pacer = PrecisePacer::new(period);
        pacer.wait_next_tick(); // seed
        let start = Instant::now();
        let mut last = start;
        for _ in 0..n {
            pacer.wait_next_tick();
            let now = Instant::now();
            assert!(now >= last, "tick time must advance monotonically");
            last = now;
        }
        let elapsed = start.elapsed();
        let ideal = period * n;
        // Drift is bounded, not accumulating: total stays within a generous CI slack of ideal.
        assert!(
            elapsed >= ideal.saturating_sub(Duration::from_millis(5)),
            "finished implausibly early ({elapsed:?} vs ideal {ideal:?})"
        );
        assert!(
            elapsed <= ideal + Duration::from_millis(80),
            "cumulative drift detected ({elapsed:?} vs ideal {ideal:?})"
        );
    }

    /// `set_period` retunes the rate for subsequent ticks.
    #[test]
    fn set_period_changes_the_rate() {
        let mut pacer = PrecisePacer::new(Duration::from_millis(1));
        assert_eq!(pacer.period(), Duration::from_millis(1));
        pacer.set_period(Duration::from_micros(500));
        assert_eq!(pacer.period(), Duration::from_micros(500));
    }

    /// A zero period is clamped so the deadline always advances (no busy-loop on a 0 ns grid).
    #[test]
    fn zero_period_is_clamped() {
        let pacer = PrecisePacer::new(Duration::ZERO);
        assert!(pacer.period() >= Duration::from_nanos(1));
    }
}
