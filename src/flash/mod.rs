//! Host-driven flashing (feature = `flash`) — download-mode `reboot` → `esptool` handoff (§9).
//!
//! [`flash`] reboots a chip into ROM download mode then invokes `esptool` to write the firmware,
//! mirroring `tools/flash_device.sh` **exactly**: `esptool --chip esp32s3 --port <PORT> --before
//! no_reset --after hard_reset write_flash 0x10000 <BIN>`. Both medius chips are ESP32-S3 with the app
//! partition at `0x10000`, so device and host flash differ only in which chip is rebooted (the `host`
//! flag). `--before no_reset` is essential: the chip is *already* in the ROM bootloader (we just put
//! it there), and a reset would bounce it back to the app.
//!
//! esptool is behind the [`CommandRunner`] trait and the pre-flash reboot behind a closure, so
//! [`flash_with`] is unit-testable with a recording runner and a no-op reboot — no esptool, no serial
//! port. esptool's stdout/stderr are captured; success surfaces stdout via `tracing`, a non-zero exit
//! folds stderr into [`Error::FlashTool`].

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

/// Injectable runner for the external flash command — the seam that makes [`flash_with`] testable
/// without spawning `esptool`.
pub trait CommandRunner {
    /// Run `program` with `args`, capturing its output. An error is a *spawn* failure (e.g. not on
    /// PATH); a non-zero exit is reported via [`CommandOutput::success`] = `false`.
    fn run(&self, program: &str, args: &[String]) -> Result<CommandOutput>;
}

/// The production [`CommandRunner`] — spawns the real process.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&self, program: &str, args: &[String]) -> Result<CommandOutput> {
        let output = std::process::Command::new(program).args(args).output()?; // spawn failure → Error::Io
        Ok(CommandOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

/// Build the exact `esptool write_flash` argv, mirroring `flash_device.sh`. Pure (no I/O), so the
/// argv is unit-tested directly.
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
/// vs device chip), then run `esptool`.
///
/// Production entry point — real [`SystemRunner`] and a real [`Device`](crate::Device) reboot,
/// waiting [`ROM_SETTLE`] before flashing.
///
/// # Errors
/// - [`Error::Io`] / handshake errors from opening the box to send the reboot frame.
/// - [`Error::Io`] if `esptool` cannot be spawned (not on PATH).
/// - [`Error::FlashTool`] if `esptool` exits non-zero (its stderr is included).
#[cfg(any(target_os = "linux", windows))]
pub fn flash(port: &str, bin_path: impl AsRef<Path>, host: bool) -> Result<()> {
    flash_with(
        port,
        bin_path.as_ref(),
        host,
        &SystemRunner,
        |port, host| {
            // Close the port (drop) before esptool reopens it, then let the chip settle into ROM.
            let device = crate::Device::open(port)?;
            device.reboot(reboot_target(host))?;
            drop(device);
            std::thread::sleep(ROM_SETTLE);
            Ok(())
        },
    )
}

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
/// `reboot(port, host)` puts the target chip into ROM download mode; `runner` runs `esptool`. A test
/// passes a no-op reboot and recording runner to assert the argv without touching hardware.
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
    let _target = download_target(host); // device/host selection (0/1)
    trace_event!(
        target: "medius::flash",
        tracing::Level::INFO,
        port = port,
        host,
        bin = %bin_path.display(),
        "flashing: rebooting into download mode",
    );

    reboot(port, host)?;

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
