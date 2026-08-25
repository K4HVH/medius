use super::opcode::{
    INJ_MOTION_CURSOR, INJ_MOTION_WHEEL, OPT_BEARING, OPT_EMIT, OPT_IMPERFECT, OPT_MOVE_RIDE,
    OPT_NAME,
};

/// `MOVE` cursor (§3.1): `[motion=0][dx i16 LE][dy i16 LE][flags u8]`, no clamp (firmware clamps with carry).
pub fn move_cursor_payload(dx: i16, dy: i16, flags: u8) -> [u8; 6] {
    let dx = dx.to_le_bytes();
    let dy = dy.to_le_bytes();
    [INJ_MOTION_CURSOR, dx[0], dx[1], dy[0], dy[1], flags]
}

/// `MOVE` wheel (§3.1): `[motion=1][dz i16 LE][flags u8]`, no clamp (firmware paces across frames with carry).
pub fn move_wheel_payload(dz: i16, flags: u8) -> [u8; 4] {
    let d = dz.to_le_bytes();
    [INJ_MOTION_WHEEL, d[0], d[1], flags]
}

/// `INJECT` (§3.2): `[class u8][id u16 LE][action u8]`; class 0 button / 1 key / 2 media; tri-state action.
pub fn inject_payload(class: u8, id: u16, action: u8) -> [u8; 4] {
    let u = id.to_le_bytes();
    [class, u[0], u[1], action]
}

/// `QUERY` (§3.5): `[what u8]`; 0 = VERSION, 1 = HEALTH.
pub fn query_payload(what: u8) -> [u8; 1] {
    [what]
}

/// `LED` (§3.7): `[target u8][mode u8][level u8]`.
pub fn led_payload(target: u8, mode: u8, level: u8) -> [u8; 3] {
    [target, mode, level]
}

/// `LOCK` (§3.8): `[class u8][usage u16 LE][direction u8][scale u8]`; scale 0 blocks, 100 passes, above 100 amplifies.
pub fn lock_payload(class: u8, usage: u16, direction: u8, scale: u8) -> [u8; 5] {
    let u = usage.to_le_bytes();
    [class, u[0], u[1], direction, scale]
}

/// `CATCH` (§3.9): `[class u8][id u16 LE][dir u8][state u8][snaplen u8]`; add or remove one
/// subscription entry. A blanket `state = 0` (every class, every id) clears the whole table.
pub fn catch_payload(class: u8, id: u16, direction: u8, state: u8, snaplen: u8) -> [u8; 6] {
    let i = id.to_le_bytes();
    [class, i[0], i[1], direction, state, snaplen]
}

/// `OPTION(IMPERFECT)` (§3.10): `[id=0][allow u8]`; 1 = opt into cloning over-capacity devices, 0 = faithful-only.
pub fn imperfect_payload(allow: bool) -> [u8; 2] {
    [OPT_IMPERFECT, allow as u8]
}

/// `OPTION(BEARING)` (§3.12): `[id=4][window u16 LE ms][mode u8]`; 0 = the relative directions never engage.
pub fn bearing_payload(window_ms: u16, mode: u8) -> [u8; 4] {
    let w = window_ms.to_le_bytes();
    [OPT_BEARING, w[0], w[1], mode]
}

/// `OPTION(MOVE_RIDE)` (§3.10): `[id=1][timeout u16 LE ms]`; 0 = off, N = ride window in milliseconds.
pub fn move_ride_payload(timeout_ms: u16) -> [u8; 3] {
    let t = timeout_ms.to_le_bytes();
    [OPT_MOVE_RIDE, t[0], t[1]]
}

/// `OPTION(EMIT)` (§3.10): `[id=2][mode u8][rate_hz u16 LE][force_hz u16 LE]`; mode 0 learnt / 1 bInterval / 2 fixed.
pub fn emit_pace_payload(mode: u8, hz: u16, force_hz: u16) -> [u8; 6] {
    let h = hz.to_le_bytes();
    let f = force_hz.to_le_bytes();
    [OPT_EMIT, mode, h[0], h[1], f[0], f[1]]
}
/// `CLIP_CTRL` engine verb (§3.11): `[op]` (`CLIP_OP_*`).
pub fn clip_op_payload(op: u8) -> [u8; 1] {
    [op]
}

/// `CLIP_SET` (§3.11): `[id u8][value u8]`; an OPTION-shaped clip scalar setting (`CLIP_SET_*`).
pub fn clip_set_payload(id: u8, value: u8) -> [u8; 2] {
    [id, value]
}

/// `CLIP_TRIGGER` (§3.11): `[class u8][id u16 LE][edge u8][action u8][flags u8]`; add/remove one binding.
pub fn clip_trigger_payload(class: u8, id: u16, edge: u8, action: u8, flags: u8) -> [u8; 6] {
    let u = id.to_le_bytes();
    [class, u[0], u[1], edge, action, flags]
}

/// `OPTION(NAME)` set value: the id byte followed by the name's ASCII bytes; empty `name` clears to the default.
pub fn name_payload(name: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(1 + name.len());
    v.push(OPT_NAME);
    v.extend_from_slice(name.as_bytes());
    v
}
