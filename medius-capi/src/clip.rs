//! Buffered clip playback: the opaque clip frame, clip-entry builder and clip handle, and their functions.

use medius::{ClipBuilder, ClipFrame, ClipHandle};

use crate::convert::{
    action_from_c, clip_action_from_c, clip_packet_trigger_from_c, clip_packet_unbind_from_c,
    clip_settings_to_c, clip_status_to_c, edge_from_c, input_to_medius, opt_slice, setup_from_c,
};
use crate::ctypes::*;
use crate::device::MediusDevice;
use crate::error::{MediusStatus, clear_error, fail, guard, guard_status, record, status_of};

/// An opaque clip frame: everything the box does on one tick.
pub struct MediusClipFrame {
    pub(crate) inner: ClipFrame,
}

/// An opaque builder for a clip entry stream.
pub struct MediusClipBuilder {
    pub(crate) inner: ClipBuilder,
}

/// A handle to one box's buffered-clip playback (owns the append-sequence counter, one per session).
pub struct MediusClip {
    pub(crate) inner: ClipHandle,
}

fn with_frame(f: *mut MediusClipFrame, op: impl FnOnce(ClipFrame) -> ClipFrame) -> MediusStatus {
    guard_status(|| {
        if f.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null clip frame");
        }
        let inner = unsafe { &mut (*f).inner };
        *inner = op(std::mem::take(inner));
        clear_error();
        MediusStatus::Ok
    })
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

/// A new empty clip frame. The caller owns it and must free it with `medius_clip_frame_free`.
#[unsafe(no_mangle)]
pub extern "C" fn medius_clip_frame_new() -> *mut MediusClipFrame {
    Box::into_raw(Box::new(MediusClipFrame {
        inner: ClipFrame::new(),
    }))
}

/// Free a clip frame. Null is a no-op.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_frame_free(f: *mut MediusClipFrame) {
    guard((), || {
        if !f.is_null() {
            drop(unsafe { Box::from_raw(f) });
        }
    });
}

/// Clear the frame to reuse it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_frame_clear(f: *mut MediusClipFrame) -> MediusStatus {
    with_frame(f, |_| ClipFrame::new())
}

/// Set the frame's cursor motion (`dx`/`dy`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_frame_move(
    f: *mut MediusClipFrame,
    dx: i16,
    dy: i16,
) -> MediusStatus {
    with_frame(f, |cf| cf.move_by(dx, dy))
}

/// Set the frame's wheel motion.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_frame_wheel(f: *mut MediusClipFrame, dz: i16) -> MediusStatus {
    with_frame(f, |cf| cf.wheel(dz))
}

/// Set the frame's pan (horizontal scroll) motion.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_frame_pan(f: *mut MediusClipFrame, dpan: i16) -> MediusStatus {
    with_frame(f, |cf| cf.pan(dpan))
}

/// Add an edge driving `usage` with `action`; `ErrInvalidArg` on a null frame or invalid usage.
fn frame_edge(f: *mut MediusClipFrame, usage: MediusUsage, action: medius::Action) -> MediusStatus {
    let Some(u) = input_to_medius(usage) else {
        return fail(MediusStatus::ErrInvalidArg, "invalid clip usage");
    };
    with_frame(f, |cf| cf.edge(u, action))
}

/// Add an edge on any usage with an explicit `action`. `action` takes a `MEDIUS_ACTION_*` constant;
/// any other value is `MEDIUS_STATUS_ERR_INVALID_ARG`. `medius_clip_append` checks the edge count: a
/// frame past `MEDIUS_CLIP_EDGES_MAX` is `MEDIUS_STATUS_ERR_CLIP_FRAME_COUNT` there.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_frame_edge(
    f: *mut MediusClipFrame,
    usage: MediusUsage,
    action: u8,
) -> MediusStatus {
    let Some(action) = action_from_c(action) else {
        return fail(MediusStatus::ErrInvalidArg, "invalid clip edge action");
    };
    frame_edge(f, usage, action)
}

/// Add an edge that presses a usage (a button, key, or media usage).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_frame_press(
    f: *mut MediusClipFrame,
    usage: MediusUsage,
) -> MediusStatus {
    frame_edge(f, usage, medius::Action::Press)
}

/// Add an edge that soft-releases a usage (clears the injected press; a physical hold is left intact).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_frame_release(
    f: *mut MediusClipFrame,
    usage: MediusUsage,
) -> MediusStatus {
    frame_edge(f, usage, medius::Action::SoftRelease)
}

/// Add an edge that force-releases a usage (masks a physical hold too).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_frame_force_release(
    f: *mut MediusClipFrame,
    usage: MediusUsage,
) -> MediusStatus {
    frame_edge(f, usage, medius::Action::ForceRelease)
}

/// Add a raw report, as `medius_device_raw` sends one: `bytes[0..len]` verbatim on endpoint number
/// `ep_num` in `dir`. `dir` takes a `MEDIUS_DIRECTION_*` constant; any other value is
/// `MEDIUS_STATUS_ERR_INVALID_ARG`. `medius_clip_append` checks the rest: a direction that is neither
/// IN nor OUT, and a frame past `MEDIUS_CLIP_RAW_MAX` raw reports, which is
/// `MEDIUS_STATUS_ERR_CLIP_FRAME_COUNT`. The box plays it only while the imperfect-clone opt-in is on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_frame_raw(
    f: *mut MediusClipFrame,
    ep_num: u8,
    dir: u8,
    bytes: *const u8,
    len: usize,
) -> MediusStatus {
    let Some(dir) = medius::Direction::from_u8(dir) else {
        return fail(MediusStatus::ErrInvalidArg, "invalid direction");
    };
    let Some(slice) = (unsafe { opt_slice(bytes, len) }) else {
        return fail(MediusStatus::ErrInvalidArg, "null bytes with len > 0");
    };
    with_frame(f, |cf| cf.raw(ep_num, dir, slice))
}

/// Add a control transfer against the real device, as `medius_device_transfer` runs one.
/// `out_data[0..out_len]` is the OUT data stage: `setup.length` bytes for an OUT request, none for an
/// IN one, checked by `medius_clip_append`. The answer arrives as a
/// `MEDIUS_CATCH_CLASS_CLIP_TRANSFER` event. The box runs it only while the imperfect-clone opt-in
/// is on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_frame_transfer(
    f: *mut MediusClipFrame,
    ep: u8,
    setup: MediusSetup,
    out_data: *const u8,
    out_len: usize,
) -> MediusStatus {
    let Some(data) = (unsafe { opt_slice(out_data, out_len) }) else {
        return fail(
            MediusStatus::ErrInvalidArg,
            "null out_data with out_len > 0",
        );
    };
    with_frame(f, |cf| cf.transfer(ep, setup_from_c(setup), data))
}

/// The bytes the frame takes in the ring, at most `MEDIUS_CLIP_ENTRY_MAX` for one
/// `medius_clip_append` accepts. 0 for a null frame.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_frame_byte_len(f: *const MediusClipFrame) -> usize {
    guard(0, || {
        if f.is_null() {
            return 0;
        }
        unsafe { &(*f).inner }.byte_len()
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

/// The bytes the builder's entries take in the ring: what to hold against `MediusClipStatus::free`
/// before a `medius_clip_append`. 0 for a null builder.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_builder_byte_len(b: *const MediusClipBuilder) -> usize {
    guard(0, || {
        if b.is_null() {
            return 0;
        }
        unsafe { &(*b).inner }.byte_len()
    })
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

/// A pan (horizontal scroll) frame.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_builder_pan(
    b: *mut MediusClipBuilder,
    dpan: i16,
) -> MediusStatus {
    with_builder(b, |cb| {
        cb.pan(dpan);
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

/// A one-edge frame for any usage with an explicit `action`. `action` takes a `MEDIUS_ACTION_*`
/// constant; any other value is `MEDIUS_STATUS_ERR_INVALID_ARG`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_builder_edge(
    b: *mut MediusClipBuilder,
    usage: MediusUsage,
    action: u8,
) -> MediusStatus {
    let Some(action) = action_from_c(action) else {
        return fail(MediusStatus::ErrInvalidArg, "invalid clip edge action");
    };
    builder_edge(b, usage, action)
}

/// A frame carrying one raw report (see `medius_clip_frame_raw`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_builder_raw(
    b: *mut MediusClipBuilder,
    ep_num: u8,
    dir: u8,
    bytes: *const u8,
    len: usize,
) -> MediusStatus {
    let Some(dir) = medius::Direction::from_u8(dir) else {
        return fail(MediusStatus::ErrInvalidArg, "invalid direction");
    };
    let Some(slice) = (unsafe { opt_slice(bytes, len) }) else {
        return fail(MediusStatus::ErrInvalidArg, "null bytes with len > 0");
    };
    with_builder(b, |cb| {
        cb.raw(ep_num, dir, slice);
    })
}

/// A frame carrying one control transfer (see `medius_clip_frame_transfer`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_builder_transfer(
    b: *mut MediusClipBuilder,
    ep: u8,
    setup: MediusSetup,
    out_data: *const u8,
    out_len: usize,
) -> MediusStatus {
    let Some(data) = (unsafe { opt_slice(out_data, out_len) }) else {
        return fail(
            MediusStatus::ErrInvalidArg,
            "null out_data with out_len > 0",
        );
    };
    with_builder(b, |cb| {
        cb.transfer(ep, setup_from_c(setup), data);
    })
}

/// One frame carrying whatever `frame` holds. The builder takes a copy, so `frame` stays usable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_builder_frame(
    b: *mut MediusClipBuilder,
    frame: *const MediusClipFrame,
) -> MediusStatus {
    guard_status(|| {
        if b.is_null() || frame.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        let f = unsafe { &(*frame).inner }.clone();
        unsafe { &mut (*b).inner }.frame(f);
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
/// Every entry is checked before the first frame goes out, so a refusal sends nothing: a frame past
/// `MEDIUS_CLIP_EDGES_MAX` edges or `MEDIUS_CLIP_RAW_MAX` raw reports
/// (`MEDIUS_STATUS_ERR_CLIP_FRAME_COUNT`), one that encodes past `MEDIUS_CLIP_ENTRY_MAX` bytes
/// (`..._CLIP_FRAME_TOO_LONG`), a raw report whose direction is neither IN nor OUT
/// (`..._RAW_DIRECTION`, or `..._RELATIVE_DIRECTION` for the bearing-relative pair), or a transfer
/// whose data does not match its setup packet (`..._CLIP_TRANSFER_DATA`).
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

/// Set the autolock scope: the input groups `scope` points at (NULL / 0 = none). Set before the first
/// append. Each entry takes a `MEDIUS_BLANKET_*` constant; any other value is
/// `MEDIUS_STATUS_ERR_INVALID_ARG`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_set_autolock(
    clip: *mut MediusClip,
    scope: *const u8,
    scope_len: usize,
) -> MediusStatus {
    guard_status(|| {
        if clip.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null clip handle");
        }
        let mut groups: Vec<medius::Blanket> = Vec::new();
        if !scope.is_null() {
            for i in 0..scope_len {
                let Some(b) = crate::convert::blanket_from_c(unsafe { *scope.add(i) }) else {
                    return fail(MediusStatus::ErrInvalidArg, "invalid blanket group");
                };
                groups.push(b);
            }
        }
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

/// Make the clip's motion wait to ride a native report (0 = the box's own clock, the default); only its wheel and pan while rendering is on with a profile armed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_set_ride(clip: *mut MediusClip, on: u8) -> MediusStatus {
    with_clip(clip, |c| c.set_ride(on != 0))
}

/// Add or overwrite an input trigger. `trigger.edge` takes a `MEDIUS_EDGE_*` constant and
/// `trigger.action` a `MEDIUS_CLIP_ACTION_*` one; any other value is
/// `MEDIUS_STATUS_ERR_INVALID_ARG`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_bind(
    clip: *mut MediusClip,
    trigger: MediusClipTrigger,
) -> MediusStatus {
    let (Some(on), Some(edge), Some(action)) = (
        input_to_medius(trigger.on),
        edge_from_c(trigger.edge),
        clip_action_from_c(trigger.action),
    ) else {
        return fail(MediusStatus::ErrInvalidArg, "invalid clip trigger");
    };
    let t = medius::ClipTrigger {
        on,
        edge,
        action,
        consume: trigger.consume != 0,
    };
    with_clip(clip, |c| c.bind(t))
}

/// Remove the input trigger on `usage`'s `edge`. `edge` takes a `MEDIUS_EDGE_*` constant; any other
/// value is `MEDIUS_STATUS_ERR_INVALID_ARG`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_unbind(
    clip: *mut MediusClip,
    usage: MediusUsage,
    edge: u8,
) -> MediusStatus {
    let (Some(u), Some(edge)) = (input_to_medius(usage), edge_from_c(edge)) else {
        return fail(
            MediusStatus::ErrInvalidArg,
            "invalid clip trigger usage or edge",
        );
    };
    with_clip(clip, |c| c.unbind(u, edge))
}

fn with_packet_trigger(
    clip: *mut MediusClip,
    trigger: *const MediusClipPacketTrigger,
    from_c: impl FnOnce(&MediusClipPacketTrigger) -> Option<medius::ClipPacketTrigger>,
    f: impl FnOnce(&ClipHandle, &medius::ClipPacketTrigger) -> Result<(), medius::Error>,
) -> MediusStatus {
    guard_status(|| {
        if clip.is_null() || trigger.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        let Some(t) = from_c(unsafe { &*trigger }) else {
            return fail(MediusStatus::ErrInvalidArg, "invalid clip packet trigger");
        };
        status_of(f(unsafe { &(*clip).inner }, &t))
    })
}

/// Add or overwrite a packet trigger: a packet `trigger` matches fires its action on the box's next
/// tick, no host round trip. Binding a key the box holds overwrites it. `trigger->class_` takes a
/// traffic `MEDIUS_CATCH_CLASS_*` constant, `trigger->direction` a `MEDIUS_DIRECTION_*` one and
/// `trigger->action` a `MEDIUS_CLIP_ACTION_*` one; any other value is
/// `MEDIUS_STATUS_ERR_INVALID_ARG`.
///
/// What the box would refuse is `MEDIUS_STATUS_ERR_CLIP_PACKET_TRIGGER` before anything is sent, and
/// `medius_last_error_message` says which:
///
/// - a class that is `MEDIUS_CATCH_CLASS_BUS` or `_CLIP_TRANSFER`;
/// - a `match_len` past `MEDIUS_MAX_PKT_MATCH`, or unlike `mask_len`;
/// - a direction the class never carries: `NEGATIVE` (OUT) on `HID_IN` or `EMIT`, `POSITIVE` (IN) on
///   `HID_OUT`;
/// - a match bit outside its mask, which no packet can equal;
/// - `consume` on `MEDIUS_CATCH_CLASS_CONTROL`;
/// - a `selector_len` without `once_per_run`;
/// - `once_per_run` without one stream (a class other than `CONTROL`, a concrete `id`, and `POSITIVE`
///   or `NEGATIVE`), without match bytes past its selector, or with no masked bit in them.
///
/// A bearing-relative direction is `MEDIUS_STATUS_ERR_RELATIVE_DIRECTION`. The match and mask go to
/// the box as given, so the key this trigger names is the key the box holds.
///
/// The box makes three checks this call cannot. A consuming trigger needs
/// `medius_device_allow_imperfect_clones`, the set holds `MEDIUS_CLIP_PKT_TRIG_MAX` triggers, and
/// their match bytes share a pool of `MEDIUS_CLIP_PKT_MATCH_POOL`. A bind the box refuses leaves its
/// set as it was: a new key is not held, and a key the box holds keeps the trigger that was there,
/// with its own action and flags. To confirm a bind, compare the fields `medius_clip_query_config`
/// reads back with the ones bound.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_bind_packet(
    clip: *mut MediusClip,
    trigger: *const MediusClipPacketTrigger,
) -> MediusStatus {
    with_packet_trigger(clip, trigger, clip_packet_trigger_from_c, |c, t| {
        c.bind_packet(t)
    })
}

/// Remove the packet trigger keyed by `trigger`'s `(class, id, direction, match, mask)`; its other
/// fields are ignored. A key the box cannot hold is refused as `medius_clip_bind_packet` refuses it:
/// the class, the lengths, the direction, and a match bit outside the mask.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_unbind_packet(
    clip: *mut MediusClip,
    trigger: *const MediusClipPacketTrigger,
) -> MediusStatus {
    with_packet_trigger(clip, trigger, clip_packet_unbind_from_c, |c, t| {
        c.unbind_packet(t)
    })
}

/// Remove every trigger of both kinds: the input triggers and the packet triggers.
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
                unsafe { clip_status_to_c(&s, out) };
                clear_error();
                MediusStatus::Ok
            }
            Err(e) => record(&e),
        }
    })
}

/// Query the clip configuration: autolock scope, loop/retain, finalized, and both kinds of trigger.
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
                unsafe { clip_settings_to_c(&s, out) };
                clear_error();
                MediusStatus::Ok
            }
            Err(e) => record(&e),
        }
    })
}
