//! Buffered clip playback: the opaque clip-entry builder and clip handle, and their functions.

use medius::{ClipBuilder, ClipHandle};

use crate::convert::{clip_settings_to_c, input_to_medius};
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

fn edge_to_medius(e: MediusEdge) -> medius::Edge {
    match e {
        MediusEdge::Both => medius::Edge::Both,
        MediusEdge::Press => medius::Edge::Press,
        MediusEdge::Release => medius::Edge::Release,
    }
}
fn action_to_medius(a: MediusClipAction) -> medius::ClipAction {
    match a {
        MediusClipAction::Start => medius::ClipAction::Start,
        MediusClipAction::Stop => medius::ClipAction::Stop,
        MediusClipAction::Pause => medius::ClipAction::Pause,
        MediusClipAction::Resume => medius::ClipAction::Resume,
        MediusClipAction::Restart => medius::ClipAction::Restart,
        MediusClipAction::Toggle => medius::ClipAction::Toggle,
    }
}

/// Set the autolock scope: the input groups `scope` points at (NULL / 0 = none). Set before the first append.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_set_autolock(
    clip: *mut MediusClip,
    scope: *const MediusBlanket,
    scope_len: usize,
) -> MediusStatus {
    guard_status(|| {
        if clip.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null clip handle");
        }
        let groups: Vec<medius::Blanket> = if scope.is_null() || scope_len == 0 {
            Vec::new()
        } else {
            (0..scope_len)
                .map(|i| medius::Blanket::from(unsafe { *scope.add(i) }))
                .collect()
        };
        status_of(unsafe { &(*clip).inner }.set_autolock(&groups))
    })
}

/// Loop playback at the clip end (retained mode only).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_set_loop(clip: *mut MediusClip, on: u8) -> MediusStatus {
    with_clip(clip, |c| c.set_loop(on != 0))
}

/// Retain the loaded clip so it can rewind and replay (0 = streaming, the default). Set before the first append.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_set_retain(clip: *mut MediusClip, on: u8) -> MediusStatus {
    with_clip(clip, |c| c.set_retain(on != 0))
}

/// Add or overwrite a trigger binding.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_bind(
    clip: *mut MediusClip,
    trigger: MediusClipTrigger,
) -> MediusStatus {
    let Some(on) = input_to_medius(trigger.on) else {
        return fail(MediusStatus::ErrInvalidArg, "invalid clip trigger usage");
    };
    let t = medius::ClipTrigger {
        on,
        edge: edge_to_medius(trigger.edge),
        action: action_to_medius(trigger.action),
        consume: trigger.consume != 0,
    };
    with_clip(clip, |c| c.bind(t))
}

/// Remove the trigger binding on `usage`'s `edge`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_unbind(
    clip: *mut MediusClip,
    usage: MediusUsage,
    edge: MediusEdge,
) -> MediusStatus {
    let Some(u) = input_to_medius(usage) else {
        return fail(MediusStatus::ErrInvalidArg, "invalid clip trigger usage");
    };
    with_clip(clip, |c| c.unbind(u, edge_to_medius(edge)))
}

/// Remove every trigger binding.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_clear_triggers(clip: *mut MediusClip) -> MediusStatus {
    with_clip(clip, |c| c.clear_triggers())
}

/// Rewind and play (resume from a pause).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_start(clip: *mut MediusClip) -> MediusStatus {
    with_clip(clip, |c| c.start())
}

/// Stop, flush a streaming clip (rewind a retained one), release held input and the clip auto-lock.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_stop(clip: *mut MediusClip) -> MediusStatus {
    with_clip(clip, |c| c.stop())
}

/// Halt mid-clip, retaining the cursor and any held input.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_pause(clip: *mut MediusClip) -> MediusStatus {
    with_clip(clip, |c| c.pause())
}

/// Continue from the paused cursor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_resume(clip: *mut MediusClip) -> MediusStatus {
    with_clip(clip, |c| c.resume())
}

/// Force a rewind and play, even mid-playback.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_restart(clip: *mut MediusClip) -> MediusStatus {
    with_clip(clip, |c| c.restart())
}

/// Toggle: play if idle/paused, stop if playing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_toggle(clip: *mut MediusClip) -> MediusStatus {
    with_clip(clip, |c| c.toggle())
}

/// Discard the loaded clip, free the ring, and clear a fault.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_clear(clip: *mut MediusClip) -> MediusStatus {
    with_clip(clip, |c| c.clear())
}

/// Finalize a retained clip: fix its end so it can replay and loop.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_finalize(clip: *mut MediusClip) -> MediusStatus {
    with_clip(clip, |c| c.finalize())
}

/// Query the ring depth, progress, and playback counters. A `Faulted` state means recover with `medius_clip_clear`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_query_status(
    clip: *mut MediusClip,
    out: *mut MediusClipStatus,
) -> MediusStatus {
    guard_status(|| {
        if clip.is_null() || out.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        match unsafe { &(*clip).inner }.query_status() {
            Ok(s) => {
                unsafe { *out = MediusClipStatus::from(s) };
                clear_error();
                MediusStatus::Ok
            }
            Err(e) => record(&e),
        }
    })
}

/// Query the clip configuration: autolock scope, loop/retain, finalized, and the trigger set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_query_config(
    clip: *mut MediusClip,
    out: *mut MediusClipSettings,
) -> MediusStatus {
    guard_status(|| {
        if clip.is_null() || out.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        match unsafe { &(*clip).inner }.query_config() {
            Ok(s) => {
                unsafe { *out = clip_settings_to_c(&s) };
                clear_error();
                MediusStatus::Ok
            }
            Err(e) => record(&e),
        }
    })
}
