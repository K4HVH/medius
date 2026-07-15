//! Buffered clip playback: the opaque clip-entry builder and clip handle, and their functions.

use medius::{Button, ClipBuilder, ClipEdge, ClipHandle, Key, MediaKey};

use crate::ctypes::*;
use crate::device::MediusDevice;
use crate::error::{MediusStatus, clear_error, fail, guard, guard_status, record, status_of};

/// An opaque builder for a clip entry stream. Create with `medius_clip_builder_new`, fill with the
/// `medius_clip_builder_*` calls, append with `medius_clip_append`, and free with `medius_clip_builder_free`.
pub struct MediusClipBuilder {
    pub(crate) inner: ClipBuilder,
}

/// A handle to one box's buffered-clip playback. Create with `medius_device_clip`, release with
/// `medius_clip_free`. It owns the append-sequence counter, so keep one handle per clip session.
pub struct MediusClip {
    pub(crate) inner: ClipHandle,
}

fn with_builder(b: *mut MediusClipBuilder, f: impl FnOnce(&mut ClipBuilder)) -> MediusStatus {
    guard_status(|| {
        if b.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null clip builder");
        }
        f(unsafe { &mut (*b).inner });
        clear_error();
        MediusStatus::Ok
    })
}

fn with_clip(
    clip: *mut MediusClip,
    f: impl FnOnce(&ClipHandle) -> Result<(), medius::Error>,
) -> MediusStatus {
    guard_status(|| {
        if clip.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null clip handle");
        }
        status_of(f(unsafe { &(*clip).inner }))
    })
}

// --- builder lifecycle ---

/// A new empty clip-entry builder. The caller owns it and must free it with `medius_clip_builder_free`.
#[unsafe(no_mangle)]
pub extern "C" fn medius_clip_builder_new() -> *mut MediusClipBuilder {
    Box::into_raw(Box::new(MediusClipBuilder {
        inner: ClipBuilder::new(),
    }))
}

/// Free a clip-entry builder. Null is a no-op.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_builder_free(b: *mut MediusClipBuilder) {
    guard((), || {
        if !b.is_null() {
            drop(unsafe { Box::from_raw(b) });
        }
    });
}

/// Clear the builder to reuse it after an append.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_builder_clear(b: *mut MediusClipBuilder) -> MediusStatus {
    with_builder(b, |cb| cb.clear())
}

// --- builder entries ---

/// A gap run: emit nothing for `frames` native frames (a zero count is a no-op).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_builder_gap(
    b: *mut MediusClipBuilder,
    frames: u16,
) -> MediusStatus {
    with_builder(b, |cb| {
        cb.gap(frames);
    })
}

/// A cursor-motion frame (`dx`/`dy`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_builder_move(
    b: *mut MediusClipBuilder,
    dx: i16,
    dy: i16,
) -> MediusStatus {
    with_builder(b, |cb| {
        cb.move_by(dx, dy);
    })
}

/// A wheel frame.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_builder_wheel(
    b: *mut MediusClipBuilder,
    dz: i16,
) -> MediusStatus {
    with_builder(b, |cb| {
        cb.wheel(dz);
    })
}

/// A frame that presses a button.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_builder_press(
    b: *mut MediusClipBuilder,
    button: MediusButton,
) -> MediusStatus {
    with_builder(b, |cb| {
        cb.press(Button::from(button));
    })
}

/// A frame that soft-releases a button (clears the injected press; a physical hold is left intact).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_builder_release(
    b: *mut MediusClipBuilder,
    button: MediusButton,
) -> MediusStatus {
    with_builder(b, |cb| {
        cb.release(Button::from(button));
    })
}

/// A frame that force-releases a button (masks a physical hold too).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_builder_force_release(
    b: *mut MediusClipBuilder,
    button: MediusButton,
) -> MediusStatus {
    with_builder(b, |cb| {
        cb.force_release(Button::from(button));
    })
}

/// A frame carrying one key edge.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_builder_key(
    b: *mut MediusClipBuilder,
    key: MediusKey,
    action: MediusAction,
) -> MediusStatus {
    with_builder(b, |cb| {
        cb.key(Key::new(key), action.into());
    })
}

/// A frame carrying one media edge.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_builder_media(
    b: *mut MediusClipBuilder,
    media: MediusMediaKey,
    action: MediusAction,
) -> MediusStatus {
    with_builder(b, |cb| {
        cb.media(MediaKey::new(media), action.into());
    })
}

/// A general content frame: a motion delta (`dx`/`dy` cursor, `wheel`) plus `n` edges from `edges`
/// (may be null when `n` is 0). An all-zero frame with no edges is a zero-motion tick, never a gap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_builder_frame(
    b: *mut MediusClipBuilder,
    dx: i16,
    dy: i16,
    wheel: i16,
    edges: *const MediusClipEdge,
    n: usize,
) -> MediusStatus {
    guard_status(|| {
        if b.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null clip builder");
        }
        if n > 0 && edges.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null edges with n > 0");
        }
        let es: Vec<ClipEdge> = (0..n)
            .map(|i| {
                let e = unsafe { *edges.add(i) };
                clip_edge_of(e)
            })
            .collect();
        unsafe { &mut (*b).inner }.frame(dx, dy, wheel, &es);
        clear_error();
        MediusStatus::Ok
    })
}

/// Build a button [`MediusClipEdge`] for `medius_clip_builder_frame`.
#[unsafe(no_mangle)]
pub extern "C" fn medius_clip_edge_button(
    button: MediusButton,
    action: MediusAction,
) -> MediusClipEdge {
    edge_to_c(ClipEdge::button(Button::from(button), action.into()))
}

/// Build a key [`MediusClipEdge`] for `medius_clip_builder_frame`.
#[unsafe(no_mangle)]
pub extern "C" fn medius_clip_edge_key(key: MediusKey, action: MediusAction) -> MediusClipEdge {
    edge_to_c(ClipEdge::key(Key::new(key), action.into()))
}

/// Build a media [`MediusClipEdge`] for `medius_clip_builder_frame`.
#[unsafe(no_mangle)]
pub extern "C" fn medius_clip_edge_media(
    media: MediusMediaKey,
    action: MediusAction,
) -> MediusClipEdge {
    edge_to_c(ClipEdge::media(MediaKey::new(media), action.into()))
}

fn edge_to_c(e: ClipEdge) -> MediusClipEdge {
    MediusClipEdge {
        id: e.id(),
        class: e.class(),
        action: e.action(),
    }
}

fn clip_edge_of(e: MediusClipEdge) -> ClipEdge {
    ClipEdge::raw(e.class, e.id, e.action)
}

// --- clip handle ---

/// A handle to this box's buffered-clip playback. The caller owns it and must free it with
/// `medius_clip_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_clip(
    dev: *mut MediusDevice,
    out: *mut *mut MediusClip,
) -> MediusStatus {
    guard_status(|| {
        if dev.is_null() || out.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        let handle = unsafe { &(*dev).inner }.clip();
        unsafe { *out = Box::into_raw(Box::new(MediusClip { inner: handle })) };
        clear_error();
        MediusStatus::Ok
    })
}

/// Free a clip handle. Null is a no-op.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_free(clip: *mut MediusClip) {
    guard((), || {
        if !clip.is_null() {
            drop(unsafe { Box::from_raw(clip) });
        }
    });
}

/// Append the builder's entries to the ring (whole-entry frames, each with the next append sequence).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_append(
    clip: *mut MediusClip,
    builder: *const MediusClipBuilder,
) -> MediusStatus {
    guard_status(|| {
        if clip.is_null() || builder.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        let b = unsafe { &(*builder).inner };
        status_of(unsafe { &(*clip).inner }.append(b))
    })
}

/// Begin playback from the ring head.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_start(clip: *mut MediusClip) -> MediusStatus {
    with_clip(clip, |c| c.start())
}

/// Begin playback with clip-owned auto-lock (`lock_mask` = 0 locks all mouse axes+buttons).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_start_autolock(
    clip: *mut MediusClip,
    lock_mask: u16,
) -> MediusStatus {
    with_clip(clip, |c| c.start_autolock(lock_mask))
}

/// Stop playback, flush the ring, release any clip-owned auto-lock.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_stop(clip: *mut MediusClip) -> MediusStatus {
    with_clip(clip, |c| c.stop())
}

/// Set the auto-lock options a later start (including a catch-triggered one) uses, without starting.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_config(
    clip: *mut MediusClip,
    autolock: bool,
    lock_mask: u16,
) -> MediusStatus {
    with_clip(clip, |c| c.config(autolock, lock_mask))
}

/// Arm an on-device catch-trigger on a physical press of `button`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_arm_catch(
    clip: *mut MediusClip,
    button: MediusButton,
) -> MediusStatus {
    with_clip(clip, |c| c.arm_catch(Some(Button::from(button))))
}

/// Arm an on-device catch-trigger on a physical press of any mouse button.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_arm_catch_any(clip: *mut MediusClip) -> MediusStatus {
    with_clip(clip, |c| c.arm_catch(None))
}

/// Clear a pending catch-arm.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_disarm(clip: *mut MediusClip) -> MediusStatus {
    with_clip(clip, |c| c.disarm())
}

/// Query the ring depth and playback counters. A `Faulted` state means re-sync (stop + rebuild).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_status(
    clip: *mut MediusClip,
    out: *mut MediusClipStatus,
) -> MediusStatus {
    guard_status(|| {
        if clip.is_null() || out.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        match unsafe { &(*clip).inner }.status() {
            Ok(s) => {
                unsafe { *out = MediusClipStatus::from(s) };
                clear_error();
                MediusStatus::Ok
            }
            Err(e) => record(&e),
        }
    })
}
