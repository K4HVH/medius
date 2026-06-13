//! medius — operator / hardware-validation CLI (feature = `cli`).
//!
//! A `clap`-derive operator tool and on-hardware validation harness (§10): the device command/query
//! surface, the paced movement session (`pace`/`bench` — the CLI's reason to exist over
//! `tools/medius.py`), reboot/flash box management, and a LOG monitor. `--json` emits machine-readable
//! output; `-v/-vv` installs a `tracing` subscriber; the `flash` subcommand needs the `flash` feature.
//!
//! `pace`/`bench` are long-running and explicitly user-invoked — never auto-launched.

use std::io::{self, BufRead, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::{Args, Parser, Subcommand, ValueEnum};

use medius::{Button, ButtonAction, Device, RebootTarget};

/// Top-level CLI: global options + a subcommand.
#[derive(Parser, Debug)]
#[command(
    name = "medius",
    version,
    about = "Operator / validation CLI for the medius mouse passthrough box",
    propagate_version = true
)]
struct Cli {
    /// Serial port (e.g. /dev/ttyACM0 or COM7). If omitted, the first medius box (by VID/PID) is used.
    #[arg(long, short = 'p', global = true)]
    port: Option<String>,

    /// Emit machine-readable JSON instead of human text (where applicable).
    #[arg(long, global = true)]
    json: bool,

    /// Increase log verbosity: -v = info, -vv = debug, -vvv = trace (installs a tracing subscriber).
    #[arg(long, short = 'v', global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Enumerate medius boxes by CH343 VID/PID.
    List,
    /// Connect and print the box Version + Health.
    Info,
    /// Stream device LOG frames (and periodic health) until interrupted.
    Monitor(MonitorArgs),
    /// Run a scripted exercise of every command (a smoke selftest).
    Selftest,
    /// One-shot relative move: `move DX DY`.
    Move(MoveArgs),
    /// One-shot scroll: `wheel DELTA`.
    Wheel(WheelArgs),
    /// One-shot button override: `button BUTTON ACTION`.
    Button(ButtonArgs),
    /// Clear all injection (return to passthrough).
    Reset,
    /// Drive the pacer: push a fixed delta each tick for a duration, or deltas streamed from stdin.
    Pace(PaceArgs),
    /// Run the pacer and print achieved-rate / jitter stats (uses `metrics`).
    Bench(BenchArgs),
    /// Reboot a chip: `reboot [--run|--download] [--host]`.
    Reboot(RebootArgs),
    /// Flash firmware to a chip (only with the `flash` feature): `flash BIN [--host]`.
    #[cfg(feature = "flash")]
    Flash(FlashArgs),
}

#[derive(Args, Debug)]
struct MonitorArgs {
    /// Also poll + print health every N milliseconds (0 = never).
    #[arg(long, default_value_t = 1000)]
    health_ms: u64,
}

#[derive(Args, Debug)]
#[command(allow_negative_numbers = true)]
struct MoveArgs {
    /// Relative X (right positive).
    dx: i16,
    /// Relative Y (down positive).
    dy: i16,
}

#[derive(Args, Debug)]
#[command(allow_negative_numbers = true)]
struct WheelArgs {
    /// Scroll delta (up positive).
    delta: i16,
}

/// A button name on the CLI (snake_case, matching the wire vocabulary).
#[derive(ValueEnum, Clone, Copy, Debug)]
#[value(rename_all = "snake_case")]
enum CliButton {
    Left,
    Right,
    Middle,
    Side1,
    Side2,
}

impl From<CliButton> for Button {
    fn from(b: CliButton) -> Self {
        match b {
            CliButton::Left => Button::Left,
            CliButton::Right => Button::Right,
            CliButton::Middle => Button::Middle,
            CliButton::Side1 => Button::Side1,
            CliButton::Side2 => Button::Side2,
        }
    }
}

/// A button action on the CLI.
#[derive(ValueEnum, Clone, Copy, Debug)]
#[value(rename_all = "snake_case")]
enum CliAction {
    Press,
    SoftRelease,
    ForceRelease,
}

impl From<CliAction> for ButtonAction {
    fn from(a: CliAction) -> Self {
        match a {
            CliAction::Press => ButtonAction::Press,
            CliAction::SoftRelease => ButtonAction::SoftRelease,
            CliAction::ForceRelease => ButtonAction::ForceRelease,
        }
    }
}

#[derive(Args, Debug)]
struct ButtonArgs {
    /// Which button.
    button: CliButton,
    /// The override action.
    action: CliAction,
}

#[derive(Args, Debug)]
#[command(allow_negative_numbers = true)]
struct PaceArgs {
    /// Per-tick X delta to push (with --vy, pushed every ~1 ms for --ms milliseconds).
    #[arg(long)]
    vx: Option<i16>,
    /// Per-tick Y delta to push.
    #[arg(long)]
    vy: Option<i16>,
    /// How long to pace, in milliseconds (push-loop mode).
    #[arg(long, default_value_t = 1000)]
    ms: u64,
    /// Pacer rate in Hz.
    #[arg(long, default_value_t = medius::DEFAULT_RATE_HZ)]
    rate: u32,
    /// Read `DX DY` delta pairs (one per line) from stdin and push them, instead of the push loop.
    #[arg(long)]
    stdin: bool,
}

#[derive(Args, Debug)]
struct BenchArgs {
    /// How long to run the pacer, in milliseconds.
    #[arg(long, default_value_t = 2000)]
    ms: u64,
    /// Pacer rate in Hz.
    #[arg(long, default_value_t = medius::DEFAULT_RATE_HZ)]
    rate: u32,
}

#[derive(Args, Debug)]
struct RebootArgs {
    /// Reboot to ROM download mode (default is reboot-to-run).
    #[arg(long)]
    download: bool,
    /// Reboot to run the firmware (the default; explicit for symmetry).
    #[arg(long, conflicts_with = "download")]
    run: bool,
    /// Target the host chip instead of the device chip.
    #[arg(long)]
    host: bool,
}

#[cfg(feature = "flash")]
#[derive(Args, Debug)]
struct FlashArgs {
    /// Firmware .bin to write.
    bin: std::path::PathBuf,
    /// Flash the host chip instead of the device chip.
    #[arg(long)]
    host: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Install a `tracing` subscriber if `-v` was passed (level scales with the count).
fn init_tracing(verbose: u8) {
    if verbose == 0 {
        return;
    }
    let level = match verbose {
        1 => tracing::Level::INFO,
        2 => tracing::Level::DEBUG,
        _ => tracing::Level::TRACE,
    };
    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_target(true)
        .with_writer(io::stderr)
        .init();
}

/// Dispatch the chosen subcommand.
fn run(cli: &Cli) -> medius::Result<()> {
    match &cli.command {
        Command::List => cmd_list(cli.json),
        Command::Info => cmd_info(cli),
        Command::Monitor(a) => cmd_monitor(cli, a),
        Command::Selftest => cmd_selftest(cli),
        Command::Move(a) => open(cli)?.move_rel(a.dx, a.dy),
        Command::Wheel(a) => open(cli)?.wheel(a.delta),
        Command::Button(a) => open(cli)?.button(a.button.into(), a.action.into()),
        Command::Reset => open(cli)?.reset(),
        Command::Pace(a) => cmd_pace(cli, a),
        Command::Bench(a) => cmd_bench(cli, a),
        Command::Reboot(a) => cmd_reboot(cli, a),
        #[cfg(feature = "flash")]
        Command::Flash(a) => cmd_flash(cli, a),
    }
}

/// Open the device at `--port`, or the first discovered box if `--port` is absent.
fn open(cli: &Cli) -> medius::Result<Device> {
    match &cli.port {
        Some(p) => Device::open(p),
        None => Device::find(),
    }
}

fn cmd_list(json: bool) -> medius::Result<()> {
    let ports = medius::find_medius();
    if json {
        println!("{}", serde_json::to_string_pretty(&ports).unwrap());
    } else if ports.is_empty() {
        println!("no medius boxes found");
    } else {
        for p in &ports {
            println!("{}  (VID {:04x} PID {:04x})", p.path, p.vid, p.pid);
        }
    }
    Ok(())
}

fn cmd_info(cli: &Cli) -> medius::Result<()> {
    let device = open(cli)?;
    let version = device.query_version()?;
    let health = device.query_health()?;
    if cli.json {
        let obj = serde_json::json!({ "version": version, "health": health });
        println!("{}", serde_json::to_string_pretty(&obj).unwrap());
    } else {
        println!("{version}");
        println!("proto_ver: {}", version.proto_ver);
        println!("link_up:          {}", health.link_up);
        println!("mouse_attached:   {}", health.mouse_attached);
        println!("clone_configured: {}", health.clone_configured);
        println!("injection_active: {}", health.injection_active);
    }
    Ok(())
}

fn cmd_monitor(cli: &Cli, args: &MonitorArgs) -> medius::Result<()> {
    let device = open(cli)?;
    let logs = device.logs();
    eprintln!("monitoring device LOG frames (Ctrl-C to stop)…");

    let mut next_health = Instant::now();
    loop {
        while let Ok(line) = logs.try_recv() {
            if cli.json {
                println!("{}", serde_json::to_string(&line).unwrap());
            } else {
                println!("[{:?}] {}", line.level, line.text);
            }
        }
        if args.health_ms > 0 && Instant::now() >= next_health {
            if let Ok(h) = device.query_health() {
                if cli.json {
                    println!("{}", serde_json::to_string(&h).unwrap());
                } else {
                    eprintln!(
                        "health: link={} mouse={} clone={} inject={}",
                        h.link_up, h.mouse_attached, h.clone_configured, h.injection_active
                    );
                }
            }
            next_health = Instant::now() + Duration::from_millis(args.health_ms);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn cmd_selftest(cli: &Cli) -> medius::Result<()> {
    let device = open(cli)?;
    let version = device.query_version()?;
    let health = device.query_health()?;
    eprintln!("connected: {version} (health link={})", health.link_up);
    // Gentle scripted exercise — no large motion, no buttons left held.
    device.move_rel(5, 0)?;
    device.move_rel(-5, 0)?;
    device.wheel(1)?;
    device.wheel(-1)?;
    device.press(Button::Left)?;
    device.release(Button::Left)?;
    device.reset()?;
    let counters = device.counters();
    if cli.json {
        let obj = serde_json::json!({ "ok": true, "counters": counters });
        println!("{}", serde_json::to_string_pretty(&obj).unwrap());
    } else {
        println!("selftest ok — {} frames sent", counters.frames_tx);
    }
    Ok(())
}

fn cmd_pace(cli: &Cli, args: &PaceArgs) -> medius::Result<()> {
    let device = open(cli)?;
    let session = device.movement_at(args.rate);

    if args.stdin {
        eprintln!("paceing stdin deltas at {} Hz (EOF to stop)…", args.rate);
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let line = line.map_err(medius::Error::Io)?;
            let mut it = line.split_whitespace();
            if let (Some(dx), Some(dy)) = (it.next(), it.next())
                && let (Ok(dx), Ok(dy)) = (dx.parse::<i16>(), dy.parse::<i16>())
            {
                session.push(dx, dy);
            }
        }
    } else {
        let vx = args.vx.unwrap_or(0);
        let vy = args.vy.unwrap_or(0);
        eprintln!(
            "pushing ({vx}, {vy}) at {} Hz for {} ms…",
            args.rate, args.ms
        );
        // Operator-driven push loop paced to --rate so the pacer is fed at its own tick rate (a fixed
        // 1 ms cadence would cap emission near 1 kHz regardless of a higher --rate).
        let push_period = Duration::from_nanos(1_000_000_000 / args.rate.max(1) as u64);
        let deadline = Instant::now() + Duration::from_millis(args.ms);
        while Instant::now() < deadline {
            session.push(vx, vy);
            std::thread::sleep(push_period);
        }
    }
    drop(session); // joins the pacer thread
    let _ = io::stdout().flush();
    Ok(())
}

fn cmd_bench(cli: &Cli, args: &BenchArgs) -> medius::Result<()> {
    let device = open(cli)?;
    let session = device.movement_at(args.rate);
    // Push (1,0) paced to --rate (one push per tick) so the pacer is exercised at the chosen rate.
    let push_period = Duration::from_nanos(1_000_000_000 / args.rate.max(1) as u64);
    let deadline = Instant::now() + Duration::from_millis(args.ms);
    while Instant::now() < deadline {
        session.push(1, 0);
        std::thread::sleep(push_period);
    }
    let stats = session.stats();
    drop(session);

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&stats).unwrap());
    } else {
        println!("ticks:      {}", stats.ticks);
        println!("late_ticks: {}", stats.late_ticks);
        println!(
            "jitter ns   p50={} p90={} p99={} max={}",
            stats.jitter.p50, stats.jitter.p90, stats.jitter.p99, stats.jitter.max
        );
        println!(
            "write ns    p50={} p90={} p99={} max={}",
            stats.write_latency.p50,
            stats.write_latency.p90,
            stats.write_latency.p99,
            stats.write_latency.max
        );
    }
    Ok(())
}

fn cmd_reboot(cli: &Cli, args: &RebootArgs) -> medius::Result<()> {
    let device = open(cli)?;
    let target = match (args.download, args.host) {
        (true, false) => RebootTarget::DeviceDownload,
        (true, true) => RebootTarget::HostDownload,
        (false, false) => RebootTarget::DeviceRun,
        (false, true) => RebootTarget::HostRun,
    };
    if args.download {
        device.reboot_download(target)?;
    } else {
        device.reboot(target)?;
    }
    eprintln!("sent reboot ({target:?})");
    Ok(())
}

#[cfg(feature = "flash")]
fn cmd_flash(cli: &Cli, args: &FlashArgs) -> medius::Result<()> {
    // Needs a concrete port: the box re-enumerates through download mode.
    let port = cli
        .port
        .clone()
        .or_else(|| medius::find_medius().into_iter().next().map(|p| p.path))
        .ok_or(medius::Error::NotFound)?;
    eprintln!(
        "flashing {} to the {} chip on {port}…",
        args.bin.display(),
        if args.host { "host" } else { "device" }
    );
    medius::flash::flash(&port, &args.bin, args.host)?;
    eprintln!("flash complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    /// clap's own internal consistency check — catches conflicting args, bad value parsers, etc.
    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    /// Each subcommand parses with representative args, and global flags attach.
    #[test]
    fn subcommands_parse() {
        let cli = Cli::try_parse_from(["medius", "--port", "/dev/ttyACM0", "move", "10", "-5"])
            .expect("move parses");
        assert!(matches!(cli.command, Command::Move(_)));
        assert_eq!(cli.port.as_deref(), Some("/dev/ttyACM0"));

        let cli = Cli::try_parse_from(["medius", "-vv", "--json", "list"]).unwrap();
        assert_eq!(cli.verbose, 2);
        assert!(cli.json);
        assert!(matches!(cli.command, Command::List));

        let cli = Cli::try_parse_from(["medius", "button", "side1", "force_release"]).unwrap();
        match cli.command {
            Command::Button(a) => {
                assert!(matches!(a.button, CliButton::Side1));
                assert!(matches!(a.action, CliAction::ForceRelease));
            }
            _ => panic!("expected button"),
        }

        for argv in [
            vec!["medius", "info"],
            vec!["medius", "monitor"],
            vec!["medius", "selftest"],
            vec!["medius", "wheel", "3"],
            vec!["medius", "reset"],
            vec!["medius", "pace", "--vx", "2", "--ms", "100"],
            vec!["medius", "pace", "--stdin"],
            vec!["medius", "bench", "--ms", "500"],
            vec!["medius", "reboot", "--download", "--host"],
        ] {
            Cli::try_parse_from(&argv).unwrap_or_else(|e| panic!("{argv:?} failed: {e}"));
        }
    }

    /// `reboot --run` and `--download` conflict (mutually exclusive).
    #[test]
    fn reboot_run_and_download_conflict() {
        let res = Cli::try_parse_from(["medius", "reboot", "--run", "--download"]);
        assert!(res.is_err(), "--run and --download must conflict");
    }

    /// The `flash` subcommand parses only when the feature is on.
    #[cfg(feature = "flash")]
    #[test]
    fn flash_subcommand_parses_with_feature() {
        let cli = Cli::try_parse_from(["medius", "flash", "fw.bin", "--host"]).unwrap();
        match cli.command {
            Command::Flash(a) => {
                assert_eq!(a.bin, std::path::PathBuf::from("fw.bin"));
                assert!(a.host);
            }
            _ => panic!("expected flash"),
        }
    }
}
