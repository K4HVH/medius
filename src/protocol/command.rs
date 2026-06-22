/// `MOVE` (§3.1): `[dx i16 LE][dy i16 LE]`, no clamp (firmware clamps with carry).
pub fn move_payload(dx: i16, dy: i16) -> [u8; 4] {
    let dx = dx.to_le_bytes();
    let dy = dy.to_le_bytes();
    [dx[0], dx[1], dy[0], dy[1]]
}

/// `WHEEL` (§3.2): `[delta i16 LE]`, no clamp (firmware paces across frames with carry).
pub fn wheel_payload(delta: i16) -> [u8; 2] {
    delta.to_le_bytes()
}

/// `BUTTON` (§3.3): `[id u8][action u8]` — id 0..=4, action 0 soft-release / 1 press / 2 force-release.
pub fn button_payload(id: u8, action: u8) -> [u8; 2] {
    [id, action]
}

/// `QUERY` (§3.5): `[what u8]` — 0 = VERSION, 1 = HEALTH.
pub fn query_payload(what: u8) -> [u8; 1] {
    [what]
}

/// `LED` (§3.7): `[target u8][mode u8][level u8]`.
pub fn led_payload(target: u8, mode: u8, level: u8) -> [u8; 3] {
    [target, mode, level]
}

/// `LOCK` (§3.8): `[target u8][direction u8][state u8]` — state 0 unlock / 1 lock.
pub fn lock_payload(target: u8, direction: u8, state: u8) -> [u8; 3] {
    [target, direction, state]
}

/// `CATCH` (§3.9): `[mask u8]` — subscribe to physical-input event classes (0 = unsubscribe).
pub fn catch_payload(mask: u8) -> [u8; 1] {
    [mask]
}
