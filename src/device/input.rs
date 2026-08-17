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

/// A live stream of decoded [`InputEvent`]s: press and release edges, and motion.
///
/// Built on the same subscription as [`EventStream`], with the held-usage snapshots turned into the
/// edges they represent. One report can produce several events, so `recv` takes `&mut self`.
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

    // A snapshot is the CLASS's state, so the box sends every held usage of that class once ANY
    // subscriber in the process has widened the table -- routing has to be class-only or the release
    // edge is lost. That is right for delivery and wrong for decoding: a stream that asked for one
    // key would otherwise report edges for every key someone else subscribed to. Filter here instead,
    // where the subscriber's own address is still known.
    //
    // The class check is not redundant: each usage carries its own class byte, and one that disagrees
    // with the frame's would otherwise be filed under the wrong class and could never be released.
    fn subscribed(&self, class: Class, usage: Usage) -> bool {
        usage.class == class
            && self
                .filters
                .iter()
                .any(|f| f.matches(CatchClass::from(class), usage.id, Direction::Both))
    }

    fn pump(&mut self, event: CatchEvent) {
        match event {
            // A report that moved nothing is not motion. The box never emits one, but the routing
            // fallback for an unaddressable event delivers it, and a phantom zero-delta event is the
            // exact shape the emission-suppression work was about.
            CatchEvent::Motion(m) if m.axes().next().is_some() => {
                self.pending.push_back(InputEvent {
                    ts_us: m.ts_us,
                    clock: m.clock,
                    input: Input::Motion {
                        dx: m.dx,
                        dy: m.dy,
                        dz: m.dz,
                    },
                })
            }
            CatchEvent::Motion(_) => {}
            CatchEvent::Usages(u) => {
                let slot = class_index(u.class);
                // Deduplicated: a malformed snapshot listing one usage twice would otherwise fire two
                // presses with no release between them, and leave `held` a multiset.
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

    /// The next decoded event, or `None` if nothing is queued (never blocks).
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
    /// `None` means "nothing yet" **or** "nothing ever again", and on a closed stream it returns at
    /// once, so a poll loop that ignores [`Self::is_connected`] spins.
    pub fn recv_timeout(&mut self, timeout: Duration) -> Option<InputEvent> {
        // A timeout too large to add to `now` is a caller asking to wait indefinitely, not one asking
        // to give up immediately -- which is what `?` on the overflow would have done.
        let Some(deadline) = Instant::now().checked_add(timeout) else {
            return self.recv().ok();
        };
        loop {
            if let Some(e) = self.pending.pop_front() {
                return Some(e);
            }
            // A report can decode to nothing at all -- an empty snapshot for a class that was already
            // empty -- so the deadline has to survive a pump that yields no event.
            let left = deadline.checked_duration_since(Instant::now())?;
            let event = self.events.recv_timeout(left)?;
            self.pump(event);
        }
    }

    /// Await the next input event; runtime-agnostic, runs under any executor.
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

    /// Events the underlying subscription dropped because the consumer fell behind.
    pub fn dropped(&self) -> u64 {
        self.events.dropped()
    }

    /// Whether the box is still delivering to this stream. [`Self::recv_timeout`] and
    /// [`Self::try_recv`] answer `None` for both "nothing yet" and "nothing ever again"; this
    /// separates them.
    pub fn is_connected(&self) -> bool {
        !self.pending.is_empty() || self.events.is_connected()
    }

    /// Which usages of `class` are currently held, as this stream has tracked them.
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
    /// (use [`CatchFilter::all_input`]), and a filter narrowed to one edge gives
    /// [`Error::HalfEdgeInputFilter`]. The missing edge is what tells a fresh press from a chord, so
    /// match on [`Input::Press`] instead.
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
