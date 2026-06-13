//! Typed command payload encoders (PC → box).
//!
//! Each returns only the payload bytes for one command (§3); the device layer wraps the frame (it owns
//! the rolling SEQ). Multi-byte integers are little-endian. `RESET` (§3.4) has an empty payload, so
//! there is no `reset_payload`.

/// `MOVE` (§3.1): `[dx i16 LE][dy i16 LE]`, `+dx` right / `+dy` down. No clamp — the firmware clamps to
/// the clone's field width with carry.
pub fn move_payload(dx: i16, dy: i16) -> [u8; 4] {
    let dx = dx.to_le_bytes();
    let dy = dy.to_le_bytes();
    [dx[0], dx[1], dy[0], dy[1]]
}

/// `WHEEL` (§3.2): `[delta i16 LE]`, `+` up / `−` down. No clamp — the firmware paces it across frames
/// by the native wheel-field width with carry.
pub fn wheel_payload(delta: i16) -> [u8; 2] {
    delta.to_le_bytes()
}

/// `BUTTON` (§3.3): `[id u8][action u8]` — id 0..=4 (Left/Right/Middle/Side1/Side2), action 0
/// soft-release / 1 press / 2 force-release. The typed device API validates; bytes pass through.
pub fn button_payload(id: u8, action: u8) -> [u8; 2] {
    [id, action]
}

/// `QUERY` (§3.5): `[what u8]` — 0 = VERSION, 1 = HEALTH.
pub fn query_payload(what: u8) -> [u8; 1] {
    [what]
}

// `REBOOT_DL` carries a single `[target u8]` byte; `Device::reboot` sends it inline (see reboot.rs).
