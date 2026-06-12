//! Host-driven flashing (feature = `flash`) — `reboot_download` → `esptool` handoff (§9).
//!
//! [`flash`] reboots a chip into ROM download mode (a `REBOOT_DL` frame: device = target 0, host =
//! target 1) and then invokes `esptool` to write the firmware, mirroring `tools/flash_device.sh` in
//! the firmware repo **exactly**: `esptool --chip esp32s3 --port <PORT> --before no_reset --after
//! hard_reset write_flash 0x10000 <BIN>`.
//!
//! Both medius chips are ESP32-S3 and the app partition is at `0x10000`, so device and host flash use
//! the same `--chip`/address — they differ only in which chip is rebooted into download mode (the
//! `host` flag). The `--before no_reset` is essential: the chip is *already* in the ROM bootloader
//! (we just put it there), and a reset would bounce it back to the app.
//!
//! ## Injectable command runner (testable without esptool)
//!
//! Running esptool is abstracted behind the [`CommandRunner`] trait, and the pre-flash reboot behind a
//! closure, so [`flash_with`] can be unit-tested with a fake runner that **records the argv** and a
//! no-op reboot — asserting the exact program, flags, address, and bin path **without** spawning
//! esptool or opening a serial port. The production [`flash`] wires the real [`SystemRunner`] and a
//! real [`Device`](crate::Device) reboot.
//!
//! esptool's stdout/stderr are captured; on success the stdout is surfaced via `tracing`
//! (`medius::flash`, INFO), and on a non-zero exit the stderr is folded into
//! [`Error::FlashTool`](crate::Error::FlashTool).

use std::path::Path;
use std::time::Duration;

use crate::error::{Error, Result};

/// The esptool program name (matches `flash_device.sh`; on PATH).
pub const ESPTOOL: &str = "esptool.py";

/// The ESP32 chip both medius MCUs use.
pub const CHIP: &str = "esp32s3";

/// The flash address of the app partition (matches `flash_device.sh`'s `@0x10000`).
pub const FLASH_ADDR: &str = "0x10000";

/// How long to wait after the `REBOOT_DOWNLOAD` frame for the chip to enter the ROM bootloader
/// (matches `flash_device.sh`'s `sleep 2.0`).
pub const ROM_SETTLE: Duration = Duration::from_secs(2);

/// The outcome of running an external command.
#[derive(Debug, Clone)]
pub struct CommandOutput {
    /// Whether the process exited successfully (status code 0).
    pub success: bool,
    /// Captured standard output (lossy UTF-8).
    pub stdout: String,
    /// Captured standard error (lossy UTF-8).
    pub stderr: String,
}

/// An injectable runner for the external flash command — the seam that makes [`flash_with`] testable
/// without spawning `esptool`.
pub trait CommandRunner {
    /// Run `program` with `args`, capturing its output. An error here is a *spawn* failure (e.g. the
    /// program is not on PATH); a non-zero exit is reported via [`CommandOutput::success`] = `false`.
    fn run(&self, program: &str, args: &[String]) -> Result<CommandOutput>;
}

/// The production [`CommandRunner`] — spawns the real process via [`std::process::Command`].
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&self, program: &str, args: &[String]) -> Result<CommandOutput> {
        let output = std::process::Command::new(program)
            .args(args)
            .output()?; // spawn/io failure → Error::Io via `?`
        Ok(CommandOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

/// Build the exact `esptool` argument vector for a `write_flash`, mirroring `flash_device.sh`.
///
/// Pure (no I/O): `--chip esp32s3 --port <port> --before no_reset --after hard_reset write_flash
/// 0x10000 <bin>`. Factored out so the argv is unit-tested directly.
pub fn esptool_args(port: &str, bin_path: &Path) -> Vec<String> {
    vec![
        "--chip".to_string(),
        CHIP.to_string(),
        "--port".to_string(),
        port.to_string(),
        "--before".to_string(),
        "no_reset".to_string(),
        "--after".to_string(),
        "hard_reset".to_string(),
        "write_flash".to_string(),
        FLASH_ADDR.to_string(),
        bin_path.to_string_lossy().into_owned(),
    ]
}

/// The `REBOOT_DL` target byte for a download-mode reboot: `1` (host) or `0` (device) — §9.
fn download_target(host: bool) -> u8 {
    if host { 1 } else { 0 }
}

/// Flash `bin_path` to a medius chip on `port`: reboot it into ROM download (`host` selects the host
/// chip vs the device chip), then run `esptool`.
///
/// Production entry point — uses the real [`SystemRunner`] and opens a real
/// [`Device`](crate::Device) to send the `REBOOT_DOWNLOAD` frame, then waits [`ROM_SETTLE`] for the
/// chip to enter the bootloader before flashing.
///
/// # Errors
/// - [`Error::Io`] / handshake errors from opening the box to send the reboot frame.
/// - [`Error::Io`] if `esptool` cannot be spawned (not on PATH).
/// - [`Error::FlashTool`] if `esptool` exits non-zero (its stderr is included).
#[cfg(any(target_os = "linux", windows))]
pub fn flash(port: &str, bin_path: impl AsRef<Path>, host: bool) -> Result<()> {
    flash_with(port, bin_path.as_ref(), host, &SystemRunner, |port, host| {
        // Open the box, send REBOOT_DOWNLOAD(target), drop it (closing the port), and let the chip
        // settle into the ROM bootloader before esptool reopens the same port.
        let device = crate::Device::open(port)?;
        device.reboot_download(reboot_target(host))?;
        drop(device);
        std::thread::sleep(ROM_SETTLE);
        Ok(())
    })
}

/// Map the `host` flag to the typed [`RebootTarget`](crate::RebootTarget) for a download reboot.
#[cfg(any(target_os = "linux", windows))]
fn reboot_target(host: bool) -> crate::RebootTarget {
    if host {
        crate::RebootTarget::HostDownload
    } else {
        crate::RebootTarget::DeviceDownload
    }
}

/// The generic flash flow with an injectable runner and reboot step (the test seam).
///
/// `reboot(port, host)` puts the target chip into ROM download mode; `runner` runs `esptool`. This
/// lets a unit test pass a no-op reboot and a recording runner to assert the argv **without** touching
/// hardware or esptool. The `download_target` byte is computed here so the test can also verify the
/// host/device selection if it inspects the reboot closure's argument.
pub fn flash_with<R, F>(
    port: &str,
    bin_path: &Path,
    host: bool,
    runner: &R,
    reboot: F,
) -> Result<()>
where
    R: CommandRunner,
    F: FnOnce(&str, bool) -> Result<()>,
{
    let _target = download_target(host); // documents the device/host selection (0/1)
    trace_event!(
        target: "medius::flash",
        tracing::Level::INFO,
        port = port,
        host,
        bin = %bin_path.display(),
        "flashing: rebooting into download mode",
    );

    // 1. Put the chip into ROM download mode.
    reboot(port, host)?;

    // 2. Run esptool with the exact flash_device.sh argv.
    let args = esptool_args(port, bin_path);
    trace_event!(
        target: "medius::flash",
        tracing::Level::INFO,
        program = ESPTOOL,
        addr = FLASH_ADDR,
        "running esptool write_flash",
    );
    let out = runner.run(ESPTOOL, &args)?;

    if out.success {
        trace_event!(
            target: "medius::flash",
            tracing::Level::INFO,
            "{}",
            out.stdout.trim(),
        );
        Ok(())
    } else {
        trace_event!(
            target: "medius::flash",
            tracing::Level::ERROR,
            "{}",
            out.stderr.trim(),
        );
        Err(Error::FlashTool(format!(
            "esptool exited non-zero; stderr: {}",
            out.stderr.trim()
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::Path;

    use super::*;

    /// A recording fake runner: captures the program + args it was asked to run, and returns a
    /// scripted outcome.
    struct FakeRunner {
        calls: RefCell<Vec<(String, Vec<String>)>>,
        success: bool,
        stderr: String,
    }

    impl FakeRunner {
        fn ok() -> Self {
            FakeRunner {
                calls: RefCell::new(Vec::new()),
                success: true,
                stderr: String::new(),
            }
        }
        fn failing(stderr: &str) -> Self {
            FakeRunner {
                calls: RefCell::new(Vec::new()),
                success: false,
                stderr: stderr.to_string(),
            }
        }
    }

    impl CommandRunner for FakeRunner {
        fn run(&self, program: &str, args: &[String]) -> Result<CommandOutput> {
            self.calls
                .borrow_mut()
                .push((program.to_string(), args.to_vec()));
            Ok(CommandOutput {
                success: self.success,
                stdout: "wrote 0x10000".to_string(),
                stderr: self.stderr.clone(),
            })
        }
    }

    #[test]
    fn esptool_args_match_flash_device_sh() {
        let args = esptool_args("/dev/ttyACM0", Path::new("fw.bin"));
        assert_eq!(
            args,
            vec![
                "--chip",
                "esp32s3",
                "--port",
                "/dev/ttyACM0",
                "--before",
                "no_reset",
                "--after",
                "hard_reset",
                "write_flash",
                "0x10000",
                "fw.bin",
            ]
        );
    }

    #[test]
    fn download_target_selects_host_or_device() {
        assert_eq!(download_target(false), 0); // device
        assert_eq!(download_target(true), 1); // host
    }

    #[test]
    fn flash_with_runs_esptool_with_exact_argv() {
        let runner = FakeRunner::ok();
        let mut reboot_seen: Option<(String, bool)> = None;
        let res = flash_with(
            "/dev/ttyACM0",
            Path::new("/tmp/medius_device.bin"),
            false,
            &runner,
            |port, host| {
                reboot_seen = Some((port.to_string(), host));
                Ok(())
            },
        );
        assert!(res.is_ok());
        // The reboot step was invoked for the device chip on the right port.
        assert_eq!(reboot_seen, Some(("/dev/ttyACM0".to_string(), false)));
        // esptool was run exactly once, with the flash_device.sh argv.
        let calls = runner.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "esptool.py");
        assert_eq!(
            calls[0].1,
            esptool_args("/dev/ttyACM0", Path::new("/tmp/medius_device.bin"))
        );
        // Address is present and correct.
        assert!(calls[0].1.contains(&"0x10000".to_string()));
    }

    #[test]
    fn flash_with_host_flag_reaches_reboot() {
        let runner = FakeRunner::ok();
        let mut host_flag = None;
        let _ = flash_with(
            "COM7",
            Path::new("host.bin"),
            true,
            &runner,
            |_port, host| {
                host_flag = Some(host);
                Ok(())
            },
        );
        assert_eq!(host_flag, Some(true)); // host chip selected
    }

    #[test]
    fn flash_with_surfaces_nonzero_exit_as_flash_tool_error() {
        let runner = FakeRunner::failing("A fatal error occurred: cannot open port");
        let err = flash_with("/dev/ttyACM0", Path::new("fw.bin"), false, &runner, |_, _| {
            Ok(())
        })
        .unwrap_err();
        match err {
            Error::FlashTool(msg) => assert!(msg.contains("cannot open port"), "msg: {msg}"),
            other => panic!("expected FlashTool, got {other:?}"),
        }
    }

    #[test]
    fn flash_with_propagates_reboot_failure() {
        let runner = FakeRunner::ok();
        let err = flash_with("/dev/ttyACM0", Path::new("fw.bin"), false, &runner, |_, _| {
            Err(Error::NotFound)
        })
        .unwrap_err();
        assert!(matches!(err, Error::NotFound));
        // esptool must NOT run if the reboot failed.
        assert_eq!(runner.calls.borrow().len(), 0);
    }
}
