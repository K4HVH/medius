//! Decoded `RESP(IMPERFECT)`: the imperfect-clone opt-in and the clone's status against it (§4.14).

/// The imperfect-clone opt-in plus the attached device's status against it (§4.14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ImperfectStatus {
    /// The opt-in toggle: cloning a device the box cannot clone exactly is allowed.
    pub allowed: bool,
    /// The attached device needs more interrupt-IN endpoints or HID interfaces than the box serves,
    /// or runs at high speed.
    pub over_capacity: bool,
    /// The live clone is not an exact copy: an opted-in device the box can't clone exactly, a forced
    /// rate, or a descriptor-patch set it serves.
    pub clone_imperfect: bool,
}

impl ImperfectStatus {
    /// Decode a `RESP(OPTIONS, IMPERFECT)` payload (§4.14): `[what][id][allowed][over_capacity][clone_imperfect]`.
    pub(crate) fn from_payload(p: &[u8]) -> Option<ImperfectStatus> {
        if p.len() < 5 {
            return None;
        }
        Some(ImperfectStatus {
            allowed: p[2] != 0,
            over_capacity: p[3] != 0,
            clone_imperfect: p[4] != 0,
        })
    }
}
