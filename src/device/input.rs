use std::collections::VecDeque;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::error::{Error, Result};
use crate::types::{
    CatchClass, CatchEvent, CatchFilter, Class, Direction, Input, InputEvent, Usage,
};

use super::Device;
use super::catch::EventStream;
use super::closer::StreamCloser;

fn class_index(class: Class) -> usize {
    match class {
        Class::Button => 0,
        Class::Key => 1,
        Class::Media => 2,
    }
}

/// Live stream of decoded [`InputEvent`]s: press and release edges, and motion.
///
/// An [`EventStream`] subscription with held-usage snapshots decoded into edges. It can be shared
/// between threads: receives run one at a time, since one report can decode to several events that
/// must come out in order, and [`Self::held`], [`Self::dropped`] and [`Self::is_connected`] never
/// wait for a receive.
#[derive(Debug)]
pub struct InputStream {
    events: EventStream,
    filters: Vec<CatchFilter>,
    // Held through a whole receive, so two receivers cannot decode reports out of order.
    turn: Mutex<()>,
    // Held only while decoding or reading, so the accessors never wait behind a blocked receive.
    state: Mutex<Decoded>,
}

#[derive(Debug, Default)]
struct Decoded {
    held: [Vec<Usage>; 3],
    pending: VecDeque<InputEvent>,
}

// A snapshot lists every held usage of the class once any subscriber widens the table, so routing
// is class-only (or the release edge is lost) and each stream filters to its subscribed addresses
// here. The class check matters: a usage whose class byte disagrees with the frame's would be filed
// under the wrong class and never released.
fn subscribed(filters: &[CatchFilter], class: Class, usage: Usage) -> bool {
    usage.class == class
        && filters
            .iter()
            .any(|f| f.matches(CatchClass::from(class), usage.id, Direction::Both))
}

impl Decoded {
    fn pump(&mut self, filters: &[CatchFilter], event: CatchEvent) {
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
                    if subscribed(filters, u.class, usage) && !now.contains(&usage) {
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
}

impl InputStream {
    fn new(events: EventStream, filters: Vec<CatchFilter>) -> InputStream {
        InputStream {
            events,
            filters,
            turn: Mutex::new(()),
            state: Mutex::new(Decoded::default()),
        }
    }

    fn pop(&self) -> Option<InputEvent> {
        self.state.lock().pending.pop_front()
    }

    fn pump(&self, event: CatchEvent) {
        self.state.lock().pump(&self.filters, event);
    }

    /// Block until the next input event; `Disconnected` once the box disconnects or the stream is
    /// closed.
    pub fn recv(&self) -> Result<InputEvent> {
        let _turn = self.turn.lock();
        loop {
            // Checked under the turn, so a receiver queued behind another gets nothing after a close.
            if self.events.closed() {
                return Err(Error::Disconnected);
            }
            if let Some(e) = self.pop() {
                return Ok(e);
            }
            let event = self.events.recv()?;
            self.pump(event);
        }
    }

    /// Next decoded event, or `None` if nothing is queued or another thread is receiving. Never
    /// blocks.
    pub fn try_recv(&self) -> Option<InputEvent> {
        let _turn = self.turn.try_lock()?;
        loop {
            if self.events.closed() {
                return None;
            }
            if let Some(e) = self.pop() {
                return Some(e);
            }
            let event = self.events.try_recv()?;
            self.pump(event);
        }
    }

    /// Block up to `timeout` for the next input event, waiting for another thread's receive first.
    ///
    /// `None` means "nothing yet" **or** "nothing ever again"; on a closed stream it returns at
    /// once, so a poll loop that ignores [`Self::is_connected`] spins.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<InputEvent> {
        // A timeout too large to add to `now` means wait indefinitely; `?` would give up at once.
        let Some(deadline) = Instant::now().checked_add(timeout) else {
            return self.recv().ok();
        };
        let _turn = self.turn.try_lock_until(deadline)?;
        loop {
            if self.events.closed() {
                return None;
            }
            if let Some(e) = self.pop() {
                return Some(e);
            }
            // A report can decode to nothing (an empty snapshot for an already-empty class), so the
            // deadline must survive a pump that yields no event. Past it, what is queued is still
            // read: a zero wait takes a queued report instead of giving up before looking.
            let left = deadline.saturating_duration_since(Instant::now());
            let event = self.events.recv_timeout(left)?;
            self.pump(event);
        }
    }

    /// Await the next input event; runs under any executor. Takes the stream mutably, so no other
    /// receive runs beside it.
    #[cfg(feature = "async")]
    pub async fn recv_async(&mut self) -> Result<InputEvent> {
        loop {
            if self.events.closed() {
                return Err(Error::Disconnected);
            }
            if let Some(e) = self.state.get_mut().pending.pop_front() {
                return Ok(e);
            }
            let event = self.events.recv_async().await?;
            self.state.get_mut().pump(&self.filters, event);
        }
    }

    /// Events the subscription dropped because the consumer fell behind.
    pub fn dropped(&self) -> u64 {
        self.events.dropped()
    }

    /// Whether the box still delivers to this stream. Separates the two meanings of `None` from
    /// [`Self::recv_timeout`] and [`Self::try_recv`]: "nothing yet" and "nothing ever again".
    pub fn is_connected(&self) -> bool {
        !self.events.closed()
            && (!self.state.lock().pending.is_empty() || self.events.is_connected())
    }

    /// Usages of `class` held, as this stream tracked them.
    pub fn held(&self, class: Class) -> Vec<Usage> {
        self.state.lock().held[class_index(class)].clone()
    }

    /// End the subscription. Every receive blocked on the stream returns at once, and every receive
    /// after it too; events still queued are dropped.
    ///
    /// For a thread that holds no handle to the stream, see [`Self::closer`].
    pub fn close(&self) {
        self.events.close();
    }

    /// A handle that closes this stream from any thread.
    pub fn closer(&self) -> StreamCloser {
        self.events.closer()
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
