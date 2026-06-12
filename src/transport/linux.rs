//! Raw Linux serial transport via `libc` (§6 of the design spec).
//!
//! Opens `/dev/ttyACMx` directly and configures it for the box's non-standard 4 Mbaud raw link,
//! with **DTR and RTS deasserted before configuration** (asserting either resets the device chip).
//! Custom baud is done via `termios2` + `BOTHER`, which `cfsetspeed`/standard `termios` cannot
//! express.
//!
//! ## Locally declared ABI
//!
//! `termios2`, `TCGETS2`, `TCSETS2`, `BOTHER`, `CBAUD`, and the c*flag bit constants come from
//! `libc`. The modem-line ioctls (`TIOCMBIC`) and bits (`TIOCM_DTR`, `TIOCM_RTS`) are **not** exposed
//! by `libc` on the gnu/x86_64 target in the pinned version, so they are declared here from the
//! stable Linux UAPI (`include/uapi/asm-generic/ioctls.h`, `termbits.h`): `TIOCMBIC = 0x5417`,
//! `TIOCM_DTR = 0x002`, `TIOCM_RTS = 0x004`. These values are identical across Linux architectures.
//!
//! ## `unsafe`
//!
//! Every FFI call is wrapped in an explicit `unsafe {}` block with a `// SAFETY:` note (the crate
//! sets `#![forbid(unsafe_op_in_unsafe_fn)]`). The owned `fd` is closed exactly once in [`Drop`].

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use super::Transport;

// ---- Locally declared constants not exposed by libc on gnu/x86_64 (stable Linux UAPI) ----

/// `TIOCMBIC` — clear the given modem control bits (UAPI `ioctls.h`). Arch-invariant.
const TIOCMBIC: libc::Ioctl = 0x5417;
/// `TIOCM_DTR` — the DTR modem line bit (UAPI `termios.h`). Arch-invariant.
const TIOCM_DTR: libc::c_int = 0x002;
/// `TIOCM_RTS` — the RTS modem line bit (UAPI `termios.h`). Arch-invariant.
const TIOCM_RTS: libc::c_int = 0x004;

/// The control link baud (§6) — non-standard, hence `termios2` + `BOTHER`.
const CTRL_BAUD: libc::speed_t = 4_000_000;

/// `c_cc[VTIME]` unit is deciseconds; `1` == a 100 ms read timeout (so `read` returns `Ok(0)`
/// roughly every 100 ms when idle, letting the reader thread poll its stop flag).
const READ_TIMEOUT_DECISECONDS: libc::cc_t = 1;

/// Compute the `c_cflag` and `c_lflag`/`c_iflag`/`c_oflag` for raw 8-N-1 at custom baud.
///
/// Pure and device-free so it can be unit-tested. Given the `c_cflag` read back from the kernel
/// (`base_cflag`), returns the flag set to write back, plus the raw input/output/local flags:
///
/// - `c_cflag`: clear the baud-select bits ([`libc::CBAUD`]) and `PARENB | CSTOPB | CRTSCTS | HUPCL`,
///   then set `BOTHER | CS8 | CLOCAL | CREAD` (custom baud, 8 data bits, ignore modem ctrl lines,
///   enable receiver). `HUPCL` is cleared so closing the port does **not** drop DTR (which would
///   reset the chip).
/// - `c_iflag = 0`: no `IXON|IXOFF|ICRNL|INLCR|…` — a fully transparent input path.
/// - `c_oflag = 0`: no `OPOST` — output is passed through unmodified.
/// - `c_lflag = 0`: no `ICANON|ECHO|ISIG|IEXTEN` — non-canonical, no echo, no signal generation.
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

/// The four termios flag words computed by [`configure_termios2_flags`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Termios2Flags {
    c_cflag: libc::tcflag_t,
    c_iflag: libc::tcflag_t,
    c_oflag: libc::tcflag_t,
    c_lflag: libc::tcflag_t,
}

/// An owned, configured serial fd. Closes the fd on drop.
pub(crate) struct LinuxSerial {
    fd: libc::c_int,
}

impl std::fmt::Debug for LinuxSerial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinuxSerial").field("fd", &self.fd).finish()
    }
}

impl LinuxSerial {
    /// Open and configure `path` (e.g. `/dev/ttyACM0`) for the 4 Mbaud raw control link.
    ///
    /// Deasserts DTR/RTS **before** applying the termios config (asserting them resets the chip),
    /// then configures custom baud via `termios2`/`BOTHER` and a 100 ms read timeout.
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        let cpath = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains a NUL byte"))?;

        // SAFETY: `cpath` is a valid NUL-terminated C string for the duration of the call. The flags
        // are valid open(2) flags; O_NOCTTY avoids the tty becoming our controlling terminal,
        // O_CLOEXEC closes the fd across exec. open returns -1 on error (checked below).
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
        Ok(serial)
    }

    /// Clear DTR and RTS via `ioctl(TIOCMBIC, TIOCM_DTR | TIOCM_RTS)` — done before configuring so
    /// the port never momentarily asserts them (which resets the device chip, §6).
    fn deassert_dtr_rts(&self) -> io::Result<()> {
        let bits: libc::c_int = TIOCM_DTR | TIOCM_RTS;
        // SAFETY: `self.fd` is a valid open fd. TIOCMBIC reads a single `c_int` of modem bits to
        // clear through the pointer; `&bits` points to a live, properly aligned `c_int`.
        let rc = unsafe { libc::ioctl(self.fd, TIOCMBIC, &bits) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Apply the raw 8-N-1 / custom-4M-baud `termios2` configuration.
    fn configure(&self) -> io::Result<()> {
        // SAFETY: a `termios2` is plain-old-data; zeroing it is a valid initial value before
        // TCGETS2 overwrites it.
        let mut tio: libc::termios2 = unsafe { core::mem::zeroed() };

        // SAFETY: `self.fd` is valid; TCGETS2 fills the `termios2` pointed to by `&mut tio`, which is
        // live and correctly sized/aligned for the kernel's struct.
        let rc = unsafe { libc::ioctl(self.fd, libc::TCGETS2, &mut tio) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }

        let flags = configure_termios2_flags(tio.c_cflag);
        tio.c_cflag = flags.c_cflag;
        tio.c_iflag = flags.c_iflag;
        tio.c_oflag = flags.c_oflag;
        tio.c_lflag = flags.c_lflag;

        // Custom input/output speed (only meaningful because BOTHER is set in c_cflag).
        tio.c_ispeed = CTRL_BAUD;
        tio.c_ospeed = CTRL_BAUD;

        // Non-canonical read: return as soon as ≥0 bytes are available, but block at most
        // VTIME deciseconds (VMIN=0, VTIME=1 ⇒ a 100 ms read timeout ⇒ read() returns 0 on idle).
        tio.c_cc[libc::VMIN] = 0;
        tio.c_cc[libc::VTIME] = READ_TIMEOUT_DECISECONDS;

        // SAFETY: `self.fd` is valid; TCSETS2 reads the `termios2` pointed to by `&tio`, which is
        // fully initialized above.
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
            // SAFETY: `self.fd` is valid; we pass a pointer into `buf` at offset `written` and a
            // count that stays within `buf`'s remaining length, so the kernel reads only valid bytes.
            let n = unsafe {
                libc::write(
                    self.fd,
                    buf[written..].as_ptr() as *const libc::c_void,
                    buf.len() - written,
                )
            };
            if n < 0 {
                let err = io::Error::last_os_error();
                // A signal-interrupted write should be retried, not surfaced.
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }
            if n == 0 {
                // 0 from write is unexpected on a serial fd; treat as a broken pipe / disconnect.
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
            // SAFETY: `self.fd` is valid; we pass `buf`'s pointer and its exact length, so the kernel
            // writes at most `buf.len()` bytes into the live, mutable `buf`.
            let n =
                unsafe { libc::read(self.fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
            if n < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }
            // n == 0 here means the VTIME read timeout elapsed with no data (VMIN=0): report it as a
            // timeout (Ok(0)) so the reader thread can poll its stop flag. (Serial fds do not signal
            // EOF this way.)
            return Ok(n as usize);
        }
    }
}

impl Drop for LinuxSerial {
    fn drop(&mut self) {
        // SAFETY: `self.fd` was opened by us and is owned exclusively; we close it exactly once.
        // HUPCL was cleared so this close does not drop DTR / reset the chip.
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

        // The baud-select field was cleared before BOTHER was applied (no stale standard baud).
        // CBAUD includes the CBAUDEX/BOTHER bit, so after masking and setting BOTHER, the only
        // CBAUD bit left set is BOTHER itself.
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
        // Specifically: none of the dangerous lflags survive.
        assert_eq!(f.c_lflag & libc::ICANON, 0);
        assert_eq!(f.c_lflag & libc::ECHO, 0);
        assert_eq!(f.c_lflag & libc::ISIG, 0);
        // And no input translation.
        assert_eq!(f.c_iflag & libc::IXON, 0);
        assert_eq!(f.c_iflag & libc::ICRNL, 0);
        // And no output post-processing.
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
