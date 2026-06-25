//! Decoded `RESP(IMPERFECT)` — the imperfect-clone opt-in and over-capacity status (§4.14).

/// The imperfect-clone opt-in plus whether the attached device is over-capacity (needs more interrupt-IN
/// endpoints than the box has) and whether the live clone went ahead anyway with one interface dead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ImperfectStatus {
    /// The opt-in toggle: cloning over-capacity devices is allowed.
    pub allowed: bool,
    /// The currently-attached device needs an IN endpoint the box can't service.
    pub over_capacity: bool,
    /// The live clone is over-capacity and was cloned anyway, so one interface is dead.
    pub clone_imperfect: bool,
}

impl ImperfectStatus {
    /// Decode a `RESP(IMPERFECT)` payload (§4.14): `[what][allowed][over_capacity][clone_imperfect]`.
    pub(crate) fn from_payload(p: &[u8]) -> Option<ImperfectStatus> {
        if p.len() < 4 {
            return None;
        }
        Some(ImperfectStatus {
            allowed: p[1] != 0,
            over_capacity: p[2] != 0,
            clone_imperfect: p[3] != 0,
        })
    }
}
