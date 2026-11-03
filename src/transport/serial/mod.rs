//! Serial transport for the control link.

use std::time::Duration;

pub(crate) const CTRL_BAUD: u32 = 6_000_000;

// Bounds a parked read so the reader thread stays responsive to stop and reconnect.
const IO_TIMEOUT: Duration = Duration::from_millis(100);

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::SerialTransport;

#[cfg(not(windows))]
mod portable;
#[cfg(not(windows))]
pub(crate) use portable::SerialTransport;

/// The other processes with the port at `path` open, as `"<name> (pid N)"`; empty where the OS can't
/// say. Each descriptor's link is read, never followed: a stat of one on a hung mount would hang.
#[cfg(target_os = "linux")]
pub(crate) fn holders(path: &std::path::Path) -> Vec<String> {
    let Ok(want) = std::fs::canonicalize(path) else {
        return Vec::new();
    };
    let Ok(procs) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let me = std::process::id();
    let mut held = Vec::new();
    for entry in procs.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == me {
            continue;
        }
        let Ok(fds) = std::fs::read_dir(entry.path().join("fd")) else {
            continue;
        };
        let holds = fds
            .flatten()
            .any(|fd| std::fs::read_link(fd.path()).is_ok_and(|t| t == want));
        if holds {
            let name = std::fs::read_to_string(entry.path().join("comm"))
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|_| "?".into());
            held.push(format!("{name} (pid {pid})"));
        }
    }
    held
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn holders(_path: &std::path::Path) -> Vec<String> {
    Vec::new()
}
