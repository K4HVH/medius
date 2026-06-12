//! Typed command **payload** encoders (PC → box).
//!
//! Each function returns only the payload bytes for one command (§3) — frame wrapping (SOF, TYPE,
//! SEQ, LEN, CRC) is the device layer's job, since it owns the rolling SEQ. All multi-byte integers
//! are little-endian (§2). These are pure, total, and panic-free.
//!
//! `RESET` (§3.4) has an empty payload, so there is no `reset_payload` — the device layer simply
//! frames [`FrameType::Reset`] with `&[]`.
//!
//! [`FrameType::Reset`]: super::opcode::FrameType::Reset

/// `MOVE` payload (§3.1): `[dx i16 LE][dy i16 LE]`.
///
/// `+dx` = right, `+dy` = down. No clamp — the full `i16` range is sent; the firmware clamps to the
/// clone's descriptor field width with carry.
///
/// # Examples
/// ```
/// # use medius::protocol::command::move_payload;
/// assert_eq!(move_payload(-1, 256), [0xFF, 0xFF, 0x00, 0x01]);
/// ```
pub fn move_payload(dx: i16, dy: i16) -> [u8; 4] {
    let dx = dx.to_le_bytes();
    let dy = dy.to_le_bytes();
    [dx[0], dx[1], dy[0], dy[1]]
}

/// `WHEEL` payload (§3.2): `[delta i16 LE]`.
///
/// `+` = up, `−` = down. No clamp — full `i16`; the firmware paces it across frames by the native
/// wheel-field width with carry.
///
/// # Examples
/// ```
/// # use medius::protocol::command::wheel_payload;
/// assert_eq!(wheel_payload(-3), [0xFD, 0xFF]);
/// ```
pub fn wheel_payload(delta: i16) -> [u8; 2] {
    delta.to_le_bytes()
}

/// `BUTTON` payload (§3.3): `[id u8][action u8]`.
///
/// `id` ∈ 0..=4 (Left/Right/Middle/Side1/Side2); `action` ∈ {0 soft-release, 1 press,
/// 2 force-release}. The raw bytes are passed through verbatim — validating/clamping is the typed
/// device API's job (a command for an absent button is a firmware no-op).
///
/// # Examples
/// ```
/// # use medius::protocol::command::button_payload;
/// assert_eq!(button_payload(3, 1), [0x03, 0x01]); // press Side1
/// ```
pub fn button_payload(id: u8, action: u8) -> [u8; 2] {
    [id, action]
}

/// `QUERY` payload (§3.5): `[what u8]` (0 = VERSION, 1 = HEALTH).
///
/// # Examples
/// ```
/// # use medius::protocol::command::query_payload;
/// assert_eq!(query_payload(1), [0x01]); // QUERY(HEALTH)
/// ```
pub fn query_payload(what: u8) -> [u8; 1] {
    [what]
}

/// `REBOOT_DL` payload (§3.6): `[target u8]`.
///
/// `0` = device → ROM download, `1` = host → ROM download, `2` = device → reboot to run,
/// `3` = host → reboot to run.
///
/// # Examples
/// ```
/// # use medius::protocol::command::reboot_payload;
/// assert_eq!(reboot_payload(2), [0x02]); // device -> reboot to run
/// ```
pub fn reboot_payload(target: u8) -> [u8; 1] {
    [target]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_payload_le_bytes() {
        assert_eq!(move_payload(0, 0), [0x00, 0x00, 0x00, 0x00]);
        assert_eq!(move_payload(40, 0), [0x28, 0x00, 0x00, 0x00]);
        // -1 = 0xFFFF LE; 256 = 0x0100 LE.
        assert_eq!(move_payload(-1, 256), [0xFF, 0xFF, 0x00, 0x01]);
        // Extremes.
        assert_eq!(move_payload(i16::MIN, i16::MAX), [0x00, 0x80, 0xFF, 0x7F]);
    }

    #[test]
    fn wheel_payload_le_bytes() {
        assert_eq!(wheel_payload(0), [0x00, 0x00]);
        assert_eq!(wheel_payload(3), [0x03, 0x00]);
        assert_eq!(wheel_payload(-3), [0xFD, 0xFF]);
        assert_eq!(wheel_payload(i16::MIN), [0x00, 0x80]);
    }

    #[test]
    fn button_payload_bytes() {
        assert_eq!(button_payload(0, 1), [0x00, 0x01]); // press Left
        assert_eq!(button_payload(4, 2), [0x04, 0x02]); // force-release Side2
        assert_eq!(button_payload(2, 0), [0x02, 0x00]); // soft-release Middle
    }

    #[test]
    fn query_payload_byte() {
        assert_eq!(query_payload(0), [0x00]); // VERSION
        assert_eq!(query_payload(1), [0x01]); // HEALTH
    }

    #[test]
    fn reboot_payload_byte() {
        assert_eq!(reboot_payload(0), [0x00]);
        assert_eq!(reboot_payload(1), [0x01]);
        assert_eq!(reboot_payload(2), [0x02]);
        assert_eq!(reboot_payload(3), [0x03]);
    }
}
