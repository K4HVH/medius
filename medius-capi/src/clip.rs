//! Buffered clip playback: the opaque clip-entry builder and clip handle, and their functions.

use medius::{ClipBuilder, ClipHandle};

use crate::convert::input_to_medius;
use crate::ctypes::*;
use crate::device::MediusDevice;
use crate::error::{MediusStatus, clear_error, fail, guard, guard_status, record, status_of};

/// An opaque builder for a clip entry stream.
pub struct MediusClipBuilder {
    pub(crate) inner: ClipBuilder,
}

/// A handle to one box's buffered-clip playback (owns the append-sequence counter, one per session).
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

/// Append a one-edge frame driving `usage` with `action`; `ErrInvalidArg` on a null builder or invalid usage.
fn builder_edge(
    b: *mut MediusClipBuilder,
    usage: MediusUsage,
    action: medius::Action,
) -> MediusStatus {
    guard_status(|| {
        if b.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null clip builder");
        }
        let Some(u) = input_to_medius(usage) else {
            return fail(MediusStatus::ErrInvalidArg, "invalid clip usage");
        };
        unsafe { &mut (*b).inner }.edge(u, action);
        clear_error();
        MediusStatus::Ok
    })
}

/// A frame that presses a usage (a button, key, or media usage).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_builder_press(
    b: *mut MediusClipBuilder,
    usage: MediusUsage,
) -> MediusStatus {
    builder_edge(b, usage, medius::Action::Press)
}

/// A frame that soft-releases a usage (clears the injected press; a physical hold is left intact).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_builder_release(
    b: *mut MediusClipBuilder,
    usage: MediusUsage,
) -> MediusStatus {
    builder_edge(b, usage, medius::Action::SoftRelease)
}

/// A frame that force-releases a usage (masks a physical hold too).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_builder_force_release(
    b: *mut MediusClipBuilder,
    usage: MediusUsage,
) -> MediusStatus {
    builder_edge(b, usage, medius::Action::ForceRelease)
}

/// A one-edge frame for any usage with an explicit `action`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_builder_edge(
    b: *mut MediusClipBuilder,
    usage: MediusUsage,
    action: MediusAction,
) -> MediusStatus {
    builder_edge(b, usage, action.into())
}

/// A general content frame: a motion delta (`dx`/`dy`, `wheel`) plus `n` edges from parallel `inputs`/`actions` arrays.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_builder_frame(
    b: *mut MediusClipBuilder,
    dx: i16,
    dy: i16,
    wheel: i16,
    inputs: *const MediusUsage,
    actions: *const MediusAction,
    n: usize,
) -> MediusStatus {
    guard_status(|| {
        if b.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null clip builder");
        }
        if n > 0 && (inputs.is_null() || actions.is_null()) {
            return fail(
                MediusStatus::ErrInvalidArg,
                "null inputs/actions with n > 0",
            );
        }
        if n > medius::CLIP_EDGES_MAX {
            return fail(
                MediusStatus::ErrInvalidArg,
                "too many clip edges on one frame",
            );
        }
        let mut es = Vec::with_capacity(n);
        for i in 0..n {
            let Some(input) = input_to_medius(unsafe { *inputs.add(i) }) else {
                return fail(MediusStatus::ErrInvalidArg, "invalid clip edge input");
            };
            let action = match unsafe { *(actions.add(i) as *const u8) } {
                0 => medius::Action::SoftRelease,
                1 => medius::Action::Press,
                2 => medius::Action::ForceRelease,
                _ => return fail(MediusStatus::ErrInvalidArg, "invalid clip edge action"),
            };
            es.push((input, action));
        }
        unsafe { &mut (*b).inner }.frame(dx, dy, wheel, &es);
        clear_error();
        MediusStatus::Ok
    })
}

/// A handle to this box's buffered-clip playback; free it with `medius_clip_free`.
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

/// Build a `medius::ClipConfig` from the C config: the auto-lock groups it points at (NULL / 0 = none).
unsafe fn clip_config_from(config: MediusClipConfig) -> medius::ClipConfig {
    if config.autolock.is_null() || config.autolock_len == 0 {
        return medius::ClipConfig::new();
    }
    let groups: Vec<medius::Blanket> = (0..config.autolock_len)
        .map(|i| medius::Blanket::from(unsafe { *config.autolock.add(i) }))
        .collect();
    medius::ClipConfig::new().autolock(&groups)
}

/// Begin playback from the ring head with `config` (an empty `autolock` plays with no auto-lock).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_start(
    clip: *mut MediusClip,
    config: MediusClipConfig,
) -> MediusStatus {
    let cfg = unsafe { clip_config_from(config) };
    with_clip(clip, |c| c.start(&cfg))
}

/// Stop playback, flush the ring, release any clip-owned auto-lock.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_stop(clip: *mut MediusClip) -> MediusStatus {
    with_clip(clip, |c| c.stop())
}

/// Arm an on-device catch-trigger on a physical press of `input`, starting with `config` when it fires.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_arm_catch(
    clip: *mut MediusClip,
    input: MediusUsage,
    config: MediusClipConfig,
) -> MediusStatus {
    let Some(inp) = input_to_medius(input) else {
        return fail(
            MediusStatus::ErrInvalidArg,
            "invalid clip catch-trigger input",
        );
    };
    let cfg = unsafe { clip_config_from(config) };
    with_clip(clip, |c| c.arm_catch(inp, &cfg))
}

/// Arm an on-device catch-trigger on any physical input (button, key, or media), starting with `config`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_arm_catch_any(
    clip: *mut MediusClip,
    config: MediusClipConfig,
) -> MediusStatus {
    let cfg = unsafe { clip_config_from(config) };
    with_clip(clip, |c| c.arm_catch_any(&cfg))
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
