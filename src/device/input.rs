use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use crate::types::{
    CatchClass, CatchEvent, CatchFilter, Class, Direction, Input, InputEvent, Usage,
};

use super::Device;
use super::catch::EventStream;

fn class_index(class: Class) -> usize {
    match class {
        Class::Button => 0,
        Class::Key => 1,
        Class::Media => 2,
    }
}

/// Live stream of decoded [`InputEvent`]s: press and release edges, and motion.
///
/// An [`EventStream`] subscription with held-usage snapshots decoded into edges. One report can
/// produce several events, so `recv` takes `&mut self`.
#[derive(Debug)]
pub struct InputStream {
    events: EventStream,
    filters: Vec<CatchFilter>,
    held: [Vec<Usage>; 3],
    pending: VecDeque<InputEvent>,
}

impl InputStream {
    fn new(events: EventStream, filters: Vec<CatchFilter>) -> InputStream {
        InputStream {
            events,
            filters,
            held: [Vec::new(), Vec::new(), Vec::new()],
            pending: VecDeque::new(),
        }
    }

    // A snapshot lists every held usage of the class once any subscriber widens the table, so
    // routing is class-only (or the release edge is lost) and each stream filters to its own address
    // here. The class check matters: a usage whose class byte disagrees with the frame's would be
    // filed under the wrong class and never released.
    fn subscribed(&self, class: Class, usage: Usage) -> bool {
        usage.class == class
            && self
                .filters
                .iter()
                .any(|f| f.matches(CatchClass::from(class), usage.id, Direction::Both))
    }

    fn pump(&mut self, event: CatchEvent) {
        match event {
            CatchEvent::Motion(m) if m.axes().next().is_some() => {
                self.pending.push_back(InputEvent {
                    ts_us: m.ts_us,
                    clock: m.clock,
                    input: Input::Motion {
                        dx: m.dx,
                        dy: m.dy,
                        dz: m.dz,
                        pan: m.pan,
                    },
                })
            }
            CatchEvent::Motion(_) => {}
            CatchEvent::Usages(u) => {
                let slot = class_index(u.class);
                // Deduplicated: a snapshot listing a usage twice would fire two presses with no
                // release between, and leave `held` a multiset.
                let mut now: Vec<Usage> = Vec::with_capacity(u.usages.len());
                for usage in u.usages {
                    if self.subscribed(u.class, usage) && !now.contains(&usage) {
                        now.push(usage);
                    }
                }
                let was = std::mem::take(&mut self.held[slot]);
                let mut edge = |input| {
                    self.pending.push_back(InputEvent {
                        ts_us: u.ts_us,
                        clock: u.clock,
                        input,
                    })
                };
                // Releases first: within one report a swap reads as "A came up, B went down".
                for old in was.iter().filter(|o| !now.contains(o)) {
                    edge(Input::Release(*old));
                }
                for fresh in now.iter().filter(|n| !was.contains(n)) {
                    edge(Input::Press(*fresh));
                }
                self.held[slot] = now;
            }
            CatchEvent::Traffic(_) => {}
        }
    }

    /// Block until the next input event.
    pub fn recv(&mut self) -> Result<InputEvent> {
        loop {
            if let Some(e) = self.pending.pop_front() {
                return Ok(e);
            }
            let event = self.events.recv()?;
            self.pump(event);
        }
    }

    /// Next decoded event, or `None` if nothing is queued. Never blocks.
    pub fn try_recv(&mut self) -> Option<InputEvent> {
        loop {
            if let Some(e) = self.pending.pop_front() {
                return Some(e);
            }
            let event = self.events.try_recv()?;
            self.pump(event);
        }
    }

    /// Block up to `timeout` for the next input event.
    ///
    /// `None` means "nothing yet" **or** "nothing ever again"; on a closed stream it returns at
    /// once, so a poll loop that ignores [`Self::is_connected`] spins.
    pub fn recv_timeout(&mut self, timeout: Duration) -> Option<InputEvent> {
        // A timeout too large to add to `now` means wait indefinitely; `?` would give up at once.
        let Some(deadline) = Instant::now().checked_add(timeout) else {
            return self.recv().ok();
        };
        loop {
            if let Some(e) = self.pending.pop_front() {
                return Some(e);
            }
            // A report can decode to nothing (an empty snapshot for an already-empty class), so the
            // deadline must survive a pump that yields no event.
            let left = deadline.checked_duration_since(Instant::now())?;
            let event = self.events.recv_timeout(left)?;
            self.pump(event);
        }
    }

    /// Await the next input event; runs under any executor.
    #[cfg(feature = "async")]
    pub async fn recv_async(&mut self) -> Result<InputEvent> {
        loop {
            if let Some(e) = self.pending.pop_front() {
                return Ok(e);
            }
            let event = self.events.recv_async().await?;
            self.pump(event);
        }
    }

    /// Events the subscription dropped because the consumer fell behind.
    pub fn dropped(&self) -> u64 {
        self.events.dropped()
    }

    /// Whether the box still delivers to this stream. Separates the two meanings of `None` from
    /// [`Self::recv_timeout`] and [`Self::try_recv`]: "nothing yet" and "nothing ever again".
    pub fn is_connected(&self) -> bool {
        !self.pending.is_empty() || self.events.is_connected()
    }

    /// Usages of `class` held, as this stream tracked them.
    pub fn held(&self, class: Class) -> &[Usage] {
        &self.held[class_index(class)]
    }
}

impl Iterator for InputStream {
    type Item = InputEvent;

    fn next(&mut self) -> Option<InputEvent> {
        self.recv().ok()
    }
}

impl Device {
    /// Subscribe to decoded input: press and release edges, and motion.
    ///
    /// ```no_run
    /// # use medius::{CatchFilter, Device, Input, Key};
    /// # fn f(dev: &Device) -> medius::Result<()> {
    /// for ev in dev.input_events([CatchFilter::watch(Key::F)])? {
    ///     match ev.input {
    ///         Input::Press(u) => println!("down {u:?}"),
    ///         Input::Release(u) => println!("up {u:?}"),
    ///         _ => {}
    ///     }
    /// }
    /// # Ok(()) }
    /// ```
    ///
    /// # Errors
    ///
    /// Every filter must name an input class and cover both edges. A traffic class gives
    /// [`Error::NotAnInputFilter`], [`CatchFilter::everything`] gives [`Error::WildcardNotInput`]
    /// (use [`CatchFilter::all_input`]), and a one-edge filter gives [`Error::HalfEdgeInputFilter`]:
    /// the missing edge tells a fresh press from a chord, so match on [`Input::Press`] instead.
    pub fn input_events(
        &self,
        filters: impl IntoIterator<Item = CatchFilter>,
    ) -> Result<InputStream> {
        let wanted: Vec<CatchFilter> = filters.into_iter().collect();
        for f in &wanted {
            match f.class() {
                None => return Err(Error::WildcardNotInput),
                Some(c) if c.is_traffic() => return Err(Error::NotAnInputFilter { class: c }),
                Some(_) if f.direction() != Direction::Both => {
                    return Err(Error::HalfEdgeInputFilter);
                }
                Some(_) => {}
            }
        }
        Ok(InputStream::new(self.catch_events(wanted.clone())?, wanted))
    }
}
