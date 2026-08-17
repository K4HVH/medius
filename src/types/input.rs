//! Decoded input edges: what [`Device::input_events`](crate::Device::input_events) yields.

use crate::types::{Axis, ClockDomain, Direction, Usage};

/// One thing the real device did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Input {
    /// A button, key or media usage went down.
    Press(Usage),
    /// A button, key or media usage came up.
    Release(Usage),
    /// A relative-motion report. Deltas are per report, not cumulative.
    Motion {
        /// Relative X (right positive).
        dx: i16,
        /// Relative Y (down positive).
        dy: i16,
        /// Wheel delta (up positive).
        dz: i16,
    },
}

impl Input {
    /// The usage this is an edge on, or `None` for motion.
    pub fn usage(self) -> Option<Usage> {
        match self {
            Input::Press(u) | Input::Release(u) => Some(u),
            Input::Motion { .. } => None,
        }
    }

    /// Whether this is a press.
    pub fn is_press(self) -> bool {
        matches!(self, Input::Press(_))
    }

    /// Whether this is a release.
    pub fn is_release(self) -> bool {
        matches!(self, Input::Release(_))
    }

    /// The edge as a [`Direction`]; [`Direction::Both`] for motion.
    pub fn direction(self) -> Direction {
        match self {
            Input::Press(_) => Direction::PRESS,
            Input::Release(_) => Direction::RELEASE,
            Input::Motion { .. } => Direction::Both,
        }
    }

    /// The axes a motion report moved, with their deltas; empty for an edge.
    pub fn axes(self) -> impl Iterator<Item = (Axis, i16)> + use<> {
        let d = match self {
            Input::Motion { dx, dy, dz } => [(Axis::X, dx), (Axis::Y, dy), (Axis::Wheel, dz)],
            _ => [(Axis::X, 0), (Axis::Y, 0), (Axis::Wheel, 0)],
        };
        d.into_iter().filter(|(_, v)| *v != 0)
    }
}

/// One [`Input`] and when the real device produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InputEvent {
    /// The report's arrival stamp, in the stamping chip's microseconds.
    pub ts_us: u32,
    /// Which chip's clock stamped it; always [`ClockDomain::HostChip`] for physical input.
    pub clock: ClockDomain,
    /// What happened.
    pub input: Input,
}
