use crate::types::LogLine;

pub(crate) const LOGS_CAPACITY: usize = 1024;

pub(crate) fn push(
    logs_tx: &flume::Sender<LogLine>,
    evict_rx: &flume::Receiver<LogLine>,
    line: LogLine,
) {
    match logs_tx.try_send(line) {
        Ok(()) => {}
        Err(flume::TrySendError::Full(line)) => {
            let _ = evict_rx.try_recv();
            let _ = logs_tx.try_send(line);
        }
        Err(flume::TrySendError::Disconnected(_)) => {}
    }
}
