//! Raw Linux serial transport via `libc` (§6 of the design spec).
//!
//! Opens `/dev/ttyACMx` directly for the box's non-standard 4 Mbaud raw link, with DTR/RTS
//! deasserted before configuration (asserting either resets the device chip). Custom baud uses
//! `termios2` + `BOTHER`, which standard `termios`/`cfsetspeed` cannot express. The modem-line ioctl
//! (`TIOCMBIC`) and bits (`TIOCM_DTR`, `TIOCM_RTS`) aren't exposed by `libc` on gnu/x86_64, so they
//! are declared below from the stable, arch-invariant Linux UAPI (`ioctls.h`, `termbits.h`).

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use super::Transport;

// ---- Constants not exposed by libc on gnu/x86_64 (stable, arch-invariant Linux UAPI) ----

/// `TIOCMBIC` — clear the given modem control bits (UAPI `ioctls.h`).
const TIOCMBIC: libc::Ioctl = 0x5417;
/// `TIOCM_DTR` — DTR modem line bit (UAPI `termios.h`).
const TIOCM_DTR: libc::c_int = 0x002;
/// `TIOCM_RTS` — RTS modem line bit (UAPI `termios.h`).
const TIOCM_RTS: libc::c_int = 0x004;

/// The control link baud (§6) — non-standard, hence `termios2` + `BOTHER`.
const CTRL_BAUD: libc::speed_t = 4_000_000;

/// `c_cc[VTIME]` is in deciseconds; `1` == a 100 ms read timeout, so an idle `read` returns `Ok(0)`
/// ~every 100 ms and the reader thread can poll its stop flag.
const READ_TIMEOUT_DECISECONDS: libc::cc_t = 1;

/// Compute the termios2 flag words for raw 8-N-1 at custom baud. Pure/device-free for unit testing.
///
/// `c_cflag` clears the baud-select bits and `PARENB|CSTOPB|CRTSCTS|HUPCL`, then sets
/// `BOTHER|CS8|CLOCAL|CREAD`. `HUPCL` is cleared so closing the port does not drop DTR (which would
/// reset the chip). `c_iflag/c_oflag/c_lflag = 0` for a fully transparent, non-canonical path.
fn configure_termios2_flags(base_cflag: libc::tcflag_t) -> Termios2Flags {
    let clear = libc::CBAUD | libc::PARENB | libc::CSTOPB | libc::CRTSCTS | libc::HUPCL;
    let set = libc::BOTHER | libc::CS8 | libc::CLOCAL | libc::CREAD;
    let c_cflag = (base_cflag & !clear) | set;
    Termios2Flags {
        c_cflag,
        c_iflag: 0,
        c_oflag: 0,
        c_lflag: 0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Termios2Flags {
    c_cflag: libc::tcflag_t,
    c_iflag: libc::tcflag_t,
    c_oflag: libc::tcflag_t,
    c_lflag: libc::tcflag_t,
}

/// An owned, configured serial fd, closed on drop.
pub(crate) struct LinuxSerial {
    fd: libc::c_int,
}

impl std::fmt::Debug for LinuxSerial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinuxSerial").field("fd", &self.fd).finish()
    }
}

impl LinuxSerial {
    /// Open and configure `path` (e.g. `/dev/ttyACM0`) for the 4 Mbaud raw control link. DTR/RTS are
    /// deasserted before the termios config is applied (asserting them resets the chip).
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        let cpath = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains a NUL byte"))?;

        // SAFETY: `cpath` is a valid NUL-terminated C string; flags are valid open(2) flags.
        let fd = unsafe {
            libc::open(
                cpath.as_ptr(),
                libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }

        // Wrap immediately so any early return below still closes the fd via Drop.
        let serial = LinuxSerial { fd };

        serial.deassert_dtr_rts()?;
        serial.configure()?;
        serial.flush_input()?;
        Ok(serial)
    }

    /// Discard already-received bytes once at open, so a stale RX buffer (ROM-bootloader preamble,
    /// leftover frame bytes) cannot precede and mis-frame the first real reply. The decoder resyncs
    /// on SOF regardless; this just removes a connect-handshake flake source.
    fn flush_input(&self) -> io::Result<()> {
        // SAFETY: `self.fd` is a valid open fd; tcflush only acts on its queues.
        let rc = unsafe { libc::tcflush(self.fd, libc::TCIFLUSH) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Clear DTR and RTS before configuring, so the port never momentarily asserts them (which
    /// resets the device chip, §6).
    fn deassert_dtr_rts(&self) -> io::Result<()> {
        let bits: libc::c_int = TIOCM_DTR | TIOCM_RTS;
        // SAFETY: `self.fd` is a valid open fd; `&bits` is a live, aligned `c_int` TIOCMBIC reads.
        let rc = unsafe { libc::ioctl(self.fd, TIOCMBIC, &bits) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Apply the raw 8-N-1 / custom-4M-baud `termios2` configuration.
    fn configure(&self) -> io::Result<()> {
        // SAFETY: `termios2` is POD; a zeroed value is valid before TCGETS2 overwrites it.
        let mut tio: libc::termios2 = unsafe { core::mem::zeroed() };

        // SAFETY: `self.fd` is valid; `&mut tio` is a live, correctly sized `termios2` TCGETS2 fills.
        let rc = unsafe { libc::ioctl(self.fd, libc::TCGETS2, &mut tio) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }

        let flags = configure_termios2_flags(tio.c_cflag);
        tio.c_cflag = flags.c_cflag;
        tio.c_iflag = flags.c_iflag;
        tio.c_oflag = flags.c_oflag;
        tio.c_lflag = flags.c_lflag;

        // Custom speed (only meaningful because BOTHER is set in c_cflag).
        tio.c_ispeed = CTRL_BAUD;
        tio.c_ospeed = CTRL_BAUD;

        // VMIN=0, VTIME=1: non-canonical read, ~100 ms timeout, returns Ok(0) on idle.
        tio.c_cc[libc::VMIN] = 0;
        tio.c_cc[libc::VTIME] = READ_TIMEOUT_DECISECONDS;

        // SAFETY: `self.fd` is valid; `&tio` is the fully initialized `termios2` TCSETS2 reads.
        let rc = unsafe { libc::ioctl(self.fd, libc::TCSETS2, &tio) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Transport for LinuxSerial {
    fn write_all(&self, buf: &[u8]) -> io::Result<()> {
        let mut written = 0;
        while written < buf.len() {
            // SAFETY: `self.fd` is valid; the pointer + count stay within `buf`'s remaining length.
            let n = unsafe {
                libc::write(
                    self.fd,
                    buf[written..].as_ptr() as *const libc::c_void,
                    buf.len() - written,
                )
            };
            if n < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }
            if n == 0 {
                // 0 from write is unexpected on a serial fd; treat as disconnect.
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "serial write returned 0",
                ));
            }
            written += n as usize;
        }
        Ok(())
    }

    fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            // SAFETY: `self.fd` is valid; `buf`'s pointer + exact length bound the kernel's write.
            let n =
                unsafe { libc::read(self.fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
            if n < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }
            // n == 0 is the VTIME timeout (VMIN=0), not EOF: report Ok(0) so the reader thread polls.
            return Ok(n as usize);
        }
    }
}

impl Drop for LinuxSerial {
    fn drop(&mut self) {
        // SAFETY: `self.fd` is owned exclusively and closed exactly once. HUPCL was cleared so this
        // close does not drop DTR / reset the chip.
        unsafe {
            libc::close(self.fd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The computed `c_cflag` sets the required bits and clears the forbidden ones, regardless of
    /// the kernel's starting value.
    #[test]
    fn cflag_sets_and_clears_correctly() {
        // Start from a "dirty" cflag with everything we must clear already set, plus a stray baud.
        let base =
            libc::CBAUD | libc::PARENB | libc::CSTOPB | libc::CRTSCTS | libc::HUPCL | libc::CS8;
        let f = configure_termios2_flags(base);

        // Required bits set.
        assert_ne!(f.c_cflag & libc::BOTHER, 0, "BOTHER must be set");
        assert_eq!(f.c_cflag & libc::CS8, libc::CS8, "CS8 must be set");
        assert_ne!(f.c_cflag & libc::CLOCAL, 0, "CLOCAL must be set");
        assert_ne!(f.c_cflag & libc::CREAD, 0, "CREAD must be set");

        // Forbidden bits cleared.
        assert_eq!(f.c_cflag & libc::PARENB, 0, "PARENB must be clear");
        assert_eq!(f.c_cflag & libc::CSTOPB, 0, "CSTOPB must be clear");
        assert_eq!(f.c_cflag & libc::CRTSCTS, 0, "CRTSCTS must be clear");
        assert_eq!(f.c_cflag & libc::HUPCL, 0, "HUPCL must be clear");

        // CBAUD includes the BOTHER bit, so after masking + setting it, BOTHER is the only one left.
        assert_eq!(f.c_cflag & libc::CBAUD, libc::BOTHER);
    }

    /// The input/output/local flags are fully raw (zeroed): no canonical mode, echo, signals, or
    /// any input/output translation.
    #[test]
    fn iflag_oflag_lflag_are_raw() {
        let f = configure_termios2_flags(0);
        assert_eq!(f.c_iflag, 0, "input flags must be raw");
        assert_eq!(f.c_oflag, 0, "output flags must be raw (no OPOST)");
        assert_eq!(
            f.c_lflag, 0,
            "local flags must be raw (no ICANON/ECHO/ISIG)"
        );
        assert_eq!(f.c_lflag & libc::ICANON, 0);
        assert_eq!(f.c_lflag & libc::ECHO, 0);
        assert_eq!(f.c_lflag & libc::ISIG, 0);
        assert_eq!(f.c_iflag & libc::IXON, 0);
        assert_eq!(f.c_iflag & libc::ICRNL, 0);
        assert_eq!(f.c_oflag & libc::OPOST, 0);
    }

    /// Re-applying to an already-clean cflag is idempotent (set/clear masks don't fight each other).
    #[test]
    fn cflag_is_idempotent() {
        let once = configure_termios2_flags(0).c_cflag;
        let twice = configure_termios2_flags(once).c_cflag;
        assert_eq!(once, twice);
    }

    /// The locally declared modem/ioctl constants match the stable Linux UAPI values.
    #[test]
    fn local_abi_constants() {
        assert_eq!(TIOCMBIC, 0x5417);
        assert_eq!(TIOCM_DTR, 0x002);
        assert_eq!(TIOCM_RTS, 0x004);
        assert_eq!(CTRL_BAUD, 4_000_000);
    }

    /// Opening a nonexistent path returns an `Err`, not a panic (fd ownership / Drop never runs).
    #[test]
    fn open_nonexistent_path_errors() {
        let res = LinuxSerial::open(Path::new("/dev/medius-does-not-exist-xyz"));
        assert!(res.is_err());
    }
}
