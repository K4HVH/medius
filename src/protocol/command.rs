use super::opcode::{INJ_MOTION_CURSOR, INJ_MOTION_WHEEL, OPT_EMIT, OPT_IMPERFECT, OPT_MOVE_RIDE};

/// `MOVE` cursor (§3.1): `[motion=0][dx i16 LE][dy i16 LE]`, no clamp (firmware clamps with carry).
pub fn move_cursor_payload(dx: i16, dy: i16) -> [u8; 5] {
    let dx = dx.to_le_bytes();
    let dy = dy.to_le_bytes();
    [INJ_MOTION_CURSOR, dx[0], dx[1], dy[0], dy[1]]
}

/// `MOVE` wheel (§3.1): `[motion=1][dz i16 LE]`, no clamp (firmware paces across frames with carry).
pub fn move_wheel_payload(dz: i16) -> [u8; 3] {
    let d = dz.to_le_bytes();
    [INJ_MOTION_WHEEL, d[0], d[1]]
}

/// `INJECT` (§3.2): `[class u8][id u16 LE][action u8]` — class 0 button / 1 key / 2 media; tri-state action.
pub fn inject_payload(class: u8, id: u16, action: u8) -> [u8; 4] {
    let u = id.to_le_bytes();
    [class, u[0], u[1], action]
}

/// `QUERY` (§3.5): `[what u8]` — 0 = VERSION, 1 = HEALTH.
pub fn query_payload(what: u8) -> [u8; 1] {
    [what]
}

/// `LED` (§3.7): `[target u8][mode u8][level u8]`.
pub fn led_payload(target: u8, mode: u8, level: u8) -> [u8; 3] {
    [target, mode, level]
}

/// `LOCK` (§3.8): `[class u8][usage u16 LE][direction u8][state u8]` — state 0 unlock / 1 lock.
/// `usage` is class-specific (mouse target, keyboard usage, or media usage; ignored for blanket classes).
pub fn lock_payload(class: u8, usage: u16, direction: u8, state: u8) -> [u8; 5] {
    let u = usage.to_le_bytes();
    [class, u[0], u[1], direction, state]
}

/// `CATCH` (§3.9): `[mask u8]` — subscribe to physical-input event classes (0 = unsubscribe).
pub fn catch_payload(mask: u8) -> [u8; 1] {
    [mask]
}

/// `OPTION(IMPERFECT)` (§3.10): `[id=0][allow u8]` — 1 = opt into cloning over-capacity devices, 0 = faithful-only.
pub fn imperfect_payload(allow: bool) -> [u8; 2] {
    [OPT_IMPERFECT, allow as u8]
}

/// `OPTION(MOVE_RIDE)` (§3.10): `[id=1][timeout u16 LE ms]` — 0 = off, N = ride window in milliseconds.
pub fn move_ride_payload(timeout_ms: u16) -> [u8; 3] {
    let t = timeout_ms.to_le_bytes();
    [OPT_MOVE_RIDE, t[0], t[1]]
}

/// `OPTION(EMIT)` (§3.10): `[id=2][mode u8][rate_hz u16 LE]` — mode 0 learnt / 1 bInterval / 2 fixed
/// (`rate_hz` is read only for mode 2).
pub fn emit_pace_payload(mode: u8, hz: u16) -> [u8; 4] {
    let h = hz.to_le_bytes();
    [OPT_EMIT, mode, h[0], h[1]]
}
