//! Host-driven flashing: reboot a chip into ROM download mode then hand off to `esptool`.

use std::path::Path;
use std::time::Duration;

use crate::error::{Error, Result};

/// The esptool program name (on PATH).
pub const ESPTOOL: &str = "esptool.py";

/// The ESP32 chip both medius MCUs use.
pub const CHIP: &str = "esp32s3";

/// The flash address of the app partition.
pub const FLASH_ADDR: &str = "0x10000";

/// How long to wait after the reboot frame for the chip to enter the ROM bootloader.
pub const ROM_SETTLE: Duration = Duration::from_secs(2);

/// The outcome of running an external command.
#[derive(Debug, Clone)]
pub struct CommandOutput {
    /// Whether the process exited successfully.
    pub success: bool,
    /// Captured standard output.
    pub stdout: String,
    /// Captured standard error.
    pub stderr: String,
}

/// Injectable runner for the external flash command — the test seam.
pub trait CommandRunner {
    /// Run `program` with `args`, capturing its output.
    fn run(&self, program: &str, args: &[String]) -> Result<CommandOutput>;
}

/// The production [`CommandRunner`] — spawns the real process.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&self, program: &str, args: &[String]) -> Result<CommandOutput> {
        let output = std::process::Command::new(program).args(args).output()?;
        Ok(CommandOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

/// Build the exact `esptool write_flash` argv.
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

fn download_target(host: bool) -> u8 {
    if host { 1 } else { 0 }
}

/// Flash `bin_path` to a medius chip on `port`, rebooting into ROM download first.
#[cfg(any(target_os = "linux", windows))]
pub fn flash(port: &str, bin_path: impl AsRef<Path>, host: bool) -> Result<()> {
    flash_with(
        port,
        bin_path.as_ref(),
        host,
        &SystemRunner,
        |port, host| {
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
    let _target = download_target(host);
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
