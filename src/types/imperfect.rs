//! Decoded `RESP(IMPERFECT)`: the imperfect-clone opt-in and the clone's status against it (§4.14).

/// Imperfect-clone opt-in and the real device's status against it (§4.14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ImperfectStatus {
    /// Opt-in: the box may clone a device it can't clone faithfully.
    pub allowed: bool,
    /// The attached device runs at high speed, has >6 HID interfaces, or has more IN endpoints live at
    /// once than the box's 4 transmit FIFOs.
    pub over_capacity: bool,
    /// The live clone is imperfect: an opted-in device the box can't clone faithfully, a forced
    /// rate, or a served descriptor-patch set.
    pub clone_imperfect: bool,
}

impl ImperfectStatus {
    /// `[what][id][allowed][over_capacity][clone_imperfect]` (§4.14).
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
