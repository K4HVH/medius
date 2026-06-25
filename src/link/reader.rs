use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use parking_lot::Mutex;

use crate::protocol::{DecodedFrame, FrameDecoder, FrameType, parse_log};
use crate::types::LogLine;

use super::catch::{self, CatchReg};
use super::correlation::{self, PendingEntry};
use super::counters::Counters;
use super::logs;
use super::reconnect::{self, ReconnectCtx};
use super::slot::TransportSlot;

const READER_IDLE_POLL: Duration = Duration::from_millis(2);

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_reader(
    transport: Arc<TransportSlot>,
    pending: Arc<Mutex<HashMap<u8, PendingEntry>>>,
    logs_tx: flume::Sender<LogLine>,
    logs_rx: flume::Receiver<LogLine>,
    events: Arc<Mutex<CatchReg>>,
    counters: Arc<Counters>,
    stop: Arc<AtomicBool>,
    reconnect_ctx: ReconnectCtx,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("medius-reader".into())
        .spawn(move || {
            reader_loop(
                &transport,
                &pending,
                &logs_tx,
                &logs_rx,
                &events,
                &counters,
                &stop,
                &reconnect_ctx,
            )
        })
        .expect("spawn medius-reader thread")
}

#[allow(clippy::too_many_arguments)]
fn reader_loop(
    transport: &TransportSlot,
    pending: &Mutex<HashMap<u8, PendingEntry>>,
    logs_tx: &flume::Sender<LogLine>,
    logs_rx: &flume::Receiver<LogLine>,
    events: &Mutex<CatchReg>,
    counters: &Counters,
    stop: &AtomicBool,
    reconnect_ctx: &ReconnectCtx,
) {
    let mut decoder = FrameDecoder::new();
    let mut buf = [0u8; 1024];
    let mut seen_generation = transport.generation();

    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let generation = transport.generation();
        if generation != seen_generation {
            decoder = FrameDecoder::new();
            seen_generation = generation;
        }
        let current = transport.current();
        match current.read(&mut buf) {
            Ok(0) => {
                std::thread::sleep(READER_IDLE_POLL);
            }
            Ok(n) => {
                decoder.feed(&buf[..n], |frame| {
                    route_frame(frame, pending, logs_tx, logs_rx, events, counters);
                });
                counters.set_crc_drops(decoder.crc_error_count());
            }
            Err(_) => {
                drop(current);
                reconnect::auto_reconnect(reconnect_ctx, stop);
            }
        }
    }
}

fn route_frame(
    frame: DecodedFrame,
    pending: &Mutex<HashMap<u8, PendingEntry>>,
    logs_tx: &flume::Sender<LogLine>,
    logs_rx: &flume::Receiver<LogLine>,
    events: &Mutex<CatchReg>,
    counters: &Counters,
) {
    counters.inc_rx();
    trace_event!(
        target: "medius::transport",
        tracing::Level::TRACE,
        dir = "rx",
        opcode = u8::from(frame.ty),
        seq = frame.seq,
        len = frame.payload.len(),
    );
    match frame.ty {
        FrameType::Resp => correlation::deliver(pending, frame.seq, frame.payload),
        FrameType::Log => {
            let line = parse_log(&frame.payload);
            #[cfg(feature = "tracing")]
            crate::trace::emit_device_log(&line);
            logs::push(logs_tx, logs_rx, line);
        }
        FrameType::MouseEvent | FrameType::KbEvent | FrameType::ConsEvent => {
            catch::deliver_event(events, frame.ty, &frame.payload)
        }
        _ => {}
    }
}
