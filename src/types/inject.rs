//! The relative-axis drive for the field-generic injection verbs (§3.1–3.2): `move` drives a [`Motion`],
//! `inject` sets a momentary [`Usage`](crate::Usage). One verb per field kind, not one per device class.

/// A relative axis to drive with the [`move_axis`](crate::Device::move_axis) verb.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Motion {
    Cursor { dx: i16, dy: i16 },
    Wheel(i16),
}
