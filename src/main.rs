#[cfg(not(target_os = "macos"))]
compile_error!("ODAUD requires macOS");

mod config;
mod hal;
mod power;
mod probe;

use clap::{Args, Parser, Subcommand};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub static STOP: AtomicBool = AtomicBool::new(false);
static RELOAD: AtomicBool = AtomicBool::new(false);

unsafe extern "C" {
    fn signal(number: i32, handler: usize) -> usize;
}

extern "C" fn on_signal(number: i32) {
    if number == 1 {
        RELOAD.store(true, Ordering::Relaxed);
    } else {
        STOP.store(true, Ordering::Relaxed);
    }
}

#[derive(Parser)]
#[command(
    version,
    about = "Temporary macOS stereo output with native delay compensation"
)]
#[command(
    after_help = "SIGHUP reloads delays; Ctrl-C stops and removes the device. System defaults are never changed."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List physical output devices and their stable UIDs
    Devices {
        /// Include aggregate, virtual, and input devices
        #[arg(long)]
        all: bool,
    },
    /// Inspect a device's formats, clocks, and reported latencies
    Inspect { uid: String },
    /// Validate a configuration and connected outputs without creating a device
    Check { config: PathBuf },
    /// Run the configured output until Ctrl-C; select it in macOS or your player
    Run { config: PathBuf },
    /// Play quiet chirps through the configured outputs and delays, then stop
    #[command(
        after_help = "SIGHUP reloads delay edits while chirps play. No manual output selection is needed."
    )]
    Test {
        config: PathBuf,
        /// Test duration in seconds (1–7200); Ctrl-C stops early
        #[arg(long, default_value = "30", value_parser = test_seconds)]
        seconds: f64,
    },
    /// Remove an exact orphaned ODAUD UID whose owner has exited
    Cleanup { uid: String },
    /// Experiment with native aggregate settings
    ProbeNative(NativeArgs),
    /// Play quiet chirps to an existing device UID
    ProbePlay {
        uid: String,
        #[arg(value_parser = seconds)]
        seconds: f64,
    },
    /// Record a short diagnostic WAV from an input UID
    ProbeCapture {
        uid: String,
        #[arg(value_parser = capture_seconds)]
        seconds: f64,
        output: PathBuf,
    },
}

#[derive(Args)]
struct NativeArgs {
    /// Native stacked setting: 1 mirrored stereo, 0 separate channels on the tested OS
    #[arg(value_parser = clap::value_parser!(u8).range(0..=1))]
    stacked: u8,
    #[arg(value_parser = seconds)]
    seconds: f64,
    /// File with whitespace-separated delay values for SIGHUP, or - to disable
    #[arg(value_name = "RELOAD_FILE_OR_DASH")]
    reload: PathBuf,
    /// Physical output UID and delay in milliseconds, repeated for each output
    #[arg(required = true, num_args = 2.., value_names = ["UID", "DELAY_MS"], allow_hyphen_values = true)]
    outputs: Vec<String>,
}

fn main() {
    let cli = Cli::parse();
    unsafe {
        for number in [1, 2, 15] {
            if signal(number, on_signal as *const () as usize) == usize::MAX {
                eprintln!("odaud: cannot register shutdown signal handler");
                std::process::exit(1);
            }
        }
    }
    if let Err(error) = run(cli.command) {
        eprintln!("odaud: {error}");
        std::process::exit(1);
    }
}

fn find_device(uid: &str) -> Result<hal::Device, String> {
    hal::devices()?
        .into_iter()
        .find(|d| d.uid == uid)
        .ok_or_else(|| format!("device UID {uid:?} is unavailable"))
}

fn seconds(text: &str) -> Result<f64, String> {
    let value = text.parse::<f64>().map_err(|_| "invalid duration")?;
    if !value.is_finite() || !(0.1..=7200.0).contains(&value) {
        return Err("duration must be between 0.1 and 7200 seconds".into());
    }
    Ok(value)
}

fn test_seconds(text: &str) -> Result<f64, String> {
    let value = seconds(text)?;
    if value < 1.0 {
        return Err("chirp tests must last at least one second".into());
    }
    Ok(value)
}

fn capture_seconds(text: &str) -> Result<f64, String> {
    let value = seconds(text)?;
    if value > 60.0 {
        return Err("diagnostic capture is limited to 60 seconds".into());
    }
    Ok(value)
}

fn delays(text: &[&str], expected: usize) -> Result<Vec<f64>, String> {
    if text.len() != expected {
        return Err(format!(
            "expected {expected} delays, received {}",
            text.len()
        ));
    }
    text.iter()
        .map(|s| {
            let value = s
                .parse::<f64>()
                .map_err(|_| format!("invalid delay {s:?}"))?;
            if !value.is_finite() || !(0.0..=5000.0).contains(&value) {
                return Err("delay must be finite and between 0 and 5000 ms".into());
            }
            Ok(value)
        })
        .collect()
}

fn run(command: Command) -> Result<(), String> {
    match command {
        Command::Devices { all } => {
            let devices = hal::devices()?;
            if devices.is_empty() {
                return Err(
                    "Core Audio returned no devices; run outside a restricted sandbox".into(),
                );
            }
            for d in devices
                .into_iter()
                .filter(|device| all || device.is_physical_output())
            {
                println!(
                    "{:?}\tuid={:?}\tid={}\toutputs={}\tinputs={}\trate={}",
                    d.name, d.uid, d.id, d.output_channels, d.input_channels, d.sample_rate
                );
                if hal::is_owned_uid(&d.uid) {
                    println!(
                        "  ODAUD-owned UID; cleanup requires its owner process to have exited"
                    );
                }
            }
        }
        Command::Inspect { uid } => {
            println!("{}", hal::inspect(find_device(&uid)?.id)?);
        }
        Command::Check { config } => {
            let config = config::Config::load(&config)?;
            let (selected, rate) = selected_outputs(&config)?;
            println!(
                "valid: {:?}, {} outputs at {rate} Hz",
                config.name,
                selected.len()
            );
            print_outputs(&config, rate);
        }
        Command::Run { config } => configured(&config, None)?,
        Command::Test { config, seconds } => configured(&config, Some(seconds))?,
        Command::Cleanup { uid } => {
            hal::cleanup_orphan(&uid)?;
            println!("removed orphan {uid:?}");
        }
        Command::ProbePlay { uid, seconds } => {
            println!("{}", probe::play(find_device(&uid)?.id, seconds)?);
        }
        Command::ProbeCapture {
            uid,
            seconds,
            output,
        } => {
            println!(
                "{}",
                probe::capture(find_device(&uid)?.id, seconds, &output)?
            );
        }
        Command::ProbeNative(args) => native(args)?,
    }
    Ok(())
}

fn selected_outputs(config: &config::Config) -> Result<(Vec<hal::Device>, f64), String> {
    let available = hal::devices()?;
    let selected = config
        .enabled()
        .map(|output| {
            let device = available
                .iter()
                .find(|device| device.uid == output.uid)
                .ok_or_else(|| format!("configured output UID {:?} is unavailable", output.uid))?;
            if !device.is_physical_output() || device.output_channels != 2 {
                return Err(format!(
                    "{:?} must be a physical stereo output",
                    device.name
                ));
            }
            Ok(device.clone())
        })
        .collect::<Result<Vec<_>, String>>()?;
    let rate = selected.first().ok_or("no enabled outputs")?.sample_rate;
    if !rate.is_finite()
        || !(8_000.0..=192_000.0).contains(&rate)
        || selected.iter().any(|device| device.sample_rate != rate)
    {
        return Err("enabled outputs must share a current nominal rate between 8 and 192 kHz; use Audio MIDI Setup to choose matching rates (clock drift is compensated separately)".into());
    }
    Ok((selected, rate))
}

fn native_offsets(config: &config::Config, rate: f64) -> Vec<(String, f64)> {
    config
        .enabled()
        .map(|output| {
            (
                output.uid.clone(),
                (output.delay_ms * rate / 1000.0).round(),
            )
        })
        .collect()
}

fn print_outputs(config: &config::Config, rate: f64) {
    for output in config.enabled() {
        let frames = (output.delay_ms * rate / 1000.0).round();
        println!(
            "  {:?}: requested {:.3} ms; native offset {frames:.0} frames ({:.3} ms)",
            output.uid,
            output.delay_ms,
            frames * 1000.0 / rate
        );
    }
}

fn configured(path: &Path, test_duration: Option<f64>) -> Result<(), String> {
    let mut config = config::Config::load(path)?;
    if let Some(duration) = test_duration {
        let max_delay = config
            .enabled()
            .map(|output| output.delay_ms)
            .fold(0.0, f64::max);
        if duration < max_delay / 1000.0 + 1.0 {
            return Err(
                "test duration must exceed the largest configured delay by at least one second"
                    .into(),
            );
        }
    }
    let (selected, rate) = selected_outputs(&config)?;
    // Keep the power watcher alive until the aggregate is gone before acknowledging sleep.
    let mut power = power::PowerWatch::new()?;
    let uid = format!(
        "com.github.Byron.odaud.run.{}.{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    );
    let mut aggregate = hal::Aggregate::create(
        &config.name,
        &uid,
        &native_offsets(&config, rate),
        &selected[0].uid,
        true,
        false,
    )?;
    let aggregate_device = find_device(&uid)?;
    if aggregate_device.output_channels != 2 || aggregate_device.sample_rate != rate {
        return Err(
            "Core Audio did not create the expected stereo output at the configured rate".into(),
        );
    }
    println!(
        "running: {:?}; pid={}; uid={uid:?}; stereo {rate} Hz",
        config.name,
        std::process::id()
    );
    println!(
        "clock reference: {:?}; native drift compensation enabled for other outputs",
        selected[0].name
    );
    print_outputs(&config, rate);
    if let Some(duration) = test_duration {
        println!(
            "Playing quiet chirps for {duration} seconds through the configured delays. SIGHUP reloads delays; Ctrl-C stops early."
        );
    } else {
        println!(
            "Select this device explicitly in macOS or your player. SIGHUP reloads delays; Ctrl-C stops."
        );
    }
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    let chirps = test_duration
        .map(|duration| probe::ChirpPlayback::start(aggregate.id, duration))
        .transpose()?;
    let mut result = Ok(());
    let mut health_at = Instant::now();
    while !STOP.load(Ordering::Relaxed) {
        if chirps
            .as_ref()
            .is_some_and(probe::ChirpPlayback::is_finished)
        {
            break;
        }
        if power.poll() {
            println!("stopping: system sleep requested; run again after waking");
            break;
        }
        if Instant::now() >= health_at {
            let health = selected_outputs(&config).and_then(|(current, current_rate)| {
                if current_rate != rate || current.iter().zip(&selected).any(|(a, b)| a.id != b.id)
                {
                    return Err(
                        "an output reconnected or changed sample rate; restart ODAUD".into(),
                    );
                }
                let running = find_device(&uid)?;
                if running.sample_rate != rate || running.output_channels != 2 {
                    return Err("the aggregate format changed; restart ODAUD".into());
                }
                Ok(())
            });
            if let Err(error) = health {
                result = Err(error);
                break;
            }
            health_at = Instant::now() + Duration::from_millis(250);
        }
        if RELOAD.swap(false, Ordering::Relaxed) {
            let update = (|| {
                let next = config::Config::load(path)?;
                if !config.same_topology(&next) {
                    return Err(
                        "name or enabled outputs changed; stop and restart to apply those changes"
                            .into(),
                    );
                }
                let (_, current_rate) = selected_outputs(&next)?;
                if current_rate != rate {
                    return Err("output sample rate changed; restart required".into());
                }
                Ok::<_, String>(next)
            })();
            match update {
                Err(error) => eprintln!("reload rejected: {error}"),
                Ok(next) => {
                    if let Err(error) = aggregate.set_latencies(&native_offsets(&next, rate)) {
                        result = Err(format!(
                            "native delay update failed; stopping because its state is uncertain: {error}"
                        ));
                        break;
                    }
                    config = next;
                    println!("reloaded delays:");
                    print_outputs(&config, rate);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let playback = chirps.map(probe::ChirpPlayback::finish).transpose();
    let cleanup = aggregate.destroy();
    let sleep = power.allow_sleep();
    cleanup?;
    sleep?;
    println!("stopped: removed {uid:?}");
    if let Some(report) = playback? {
        println!("{report}");
    }
    result
}

fn native(args: NativeArgs) -> Result<(), String> {
    if args.outputs.len() % 2 != 0 {
        return Err("native outputs must be UID DELAY_MS pairs".into());
    }
    let stacked = args.stacked == 1;
    let duration = args.seconds;
    let reload_path = &args.reload;
    let raw_delays = args
        .outputs
        .chunks_exact(2)
        .map(|pair| pair[1].as_str())
        .collect::<Vec<_>>();
    let requested = delays(&raw_delays, args.outputs.len() / 2)?;
    let selected = args
        .outputs
        .chunks_exact(2)
        .map(|pair| find_device(&pair[0]))
        .collect::<Result<Vec<_>, _>>()?;
    let rate = selected[0].sample_rate;
    for (i, device) in selected.iter().enumerate() {
        if device.output_channels != 2 || device.uid.starts_with("com.github.Byron.odaud.") {
            return Err(format!(
                "{:?} must be a physical stereo output",
                device.name
            ));
        }
        if !rate.is_finite() || rate <= 0.0 || device.sample_rate != rate {
            return Err(
                "the probe requires the selected outputs to have the same current sample rate"
                    .into(),
            );
        }
        if selected[..i].iter().any(|other| other.uid == device.uid) {
            return Err("duplicate output UID".into());
        }
    }
    let to_frames = |values: &[f64]| {
        selected
            .iter()
            .zip(values)
            .map(|(device, ms)| (device.uid.clone(), (ms * rate / 1000.0).round()))
            .collect::<Vec<_>>()
    };
    let uid = format!(
        "com.github.Byron.odaud.probe.{}.{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    );
    let mut aggregate = hal::Aggregate::create(
        "ODAUD Feasibility",
        &uid,
        &to_frames(&requested),
        &selected[0].uid,
        stacked,
        false,
    )?;
    println!(
        "ready pid={} id={} uid={uid:?} stacked={} rate={} native_delay_ms={requested:?}",
        std::process::id(),
        aggregate.id,
        u8::from(stacked),
        rate
    );
    println!("{}", hal::inspect(aggregate.id)?);
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    let start = Instant::now();
    while !STOP.load(Ordering::Relaxed) && start.elapsed().as_secs_f64() < duration {
        if RELOAD.swap(false, Ordering::Relaxed) {
            let update = (|| {
                if reload_path == Path::new("-") {
                    return Err("no reload file was supplied".into());
                }
                let contents = std::fs::read_to_string(reload_path).map_err(|e| e.to_string())?;
                let values = delays(
                    &contents.split_whitespace().collect::<Vec<_>>(),
                    selected.len(),
                )?;
                aggregate.set_latencies(&to_frames(&values))?;
                println!(
                    "reloaded elapsed={:.3} native_delay_ms={values:?}",
                    start.elapsed().as_secs_f64()
                );
                println!("{}", hal::inspect(aggregate.id)?);
                Ok::<_, String>(())
            })();
            if let Err(e) = update {
                eprintln!("reload failed: {e}");
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    aggregate.destroy()?;
    println!("removed uid={uid:?}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_has_generated_help_and_version() {
        Cli::command().debug_assert();
        for args in [vec!["odaud", "--help"], vec!["odaud", "test", "--help"]] {
            assert_eq!(
                Cli::try_parse_from(args).err().unwrap().kind(),
                clap::error::ErrorKind::DisplayHelp
            );
        }
        assert_eq!(
            Cli::try_parse_from(["odaud", "--version"])
                .err()
                .unwrap()
                .kind(),
            clap::error::ErrorKind::DisplayVersion
        );
    }

    #[test]
    fn cli_parses_chirp_duration_and_rejects_invalid_arguments() {
        let Command::Test { config, seconds } = Cli::try_parse_from(["odaud", "test", "room.toml"])
            .unwrap()
            .command
        else {
            panic!("wrong command")
        };
        assert_eq!(config, Path::new("room.toml"));
        assert_eq!(seconds, 30.0);
        let Command::Test { seconds, .. } =
            Cli::try_parse_from(["odaud", "test", "room.toml", "--seconds", "45"])
                .unwrap()
                .command
        else {
            panic!("wrong command")
        };
        assert_eq!(seconds, 45.0);
        for duration in ["0", "0.1", "NaN", "inf", "-1", "7201"] {
            assert!(
                Cli::try_parse_from(["odaud", "test", "room.toml", "--seconds", duration]).is_err()
            );
        }
        for args in [
            vec!["odaud", "test"],
            vec!["odaud", "devices", "--typo"],
            vec!["odaud", "check", "one.toml", "extra"],
            vec!["odaud", "probe-capture", "mic", "61", "out.wav"],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
    }

    #[test]
    fn cli_preserves_native_probe_pairs_and_non_unicode_paths() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};
        let Command::ProbeNative(args) = Cli::try_parse_from([
            "odaud",
            "probe-native",
            "1",
            "3",
            "-",
            "speaker",
            "0",
            "usb",
            "50",
        ])
        .unwrap()
        .command
        else {
            panic!("wrong command")
        };
        assert_eq!(args.outputs, ["speaker", "0", "usb", "50"]);
        assert_eq!(args.reload, Path::new("-"));
        let command = Cli::try_parse_from([
            "odaud",
            "probe-native",
            "1",
            "3",
            "-",
            "speaker",
            "0",
            "unpaired",
        ])
        .unwrap()
        .command;
        assert_eq!(
            run(command).unwrap_err(),
            "native outputs must be UID DELAY_MS pairs"
        );
        let path = OsString::from_vec(b"room-\xff.toml".to_vec());
        let Command::Check { config } =
            Cli::try_parse_from([OsString::from("odaud"), "check".into(), path.clone()])
                .unwrap()
                .command
        else {
            panic!("wrong command")
        };
        assert_eq!(config, PathBuf::from(path));
    }

    #[test]
    fn native_offsets_round_frames_and_ignore_disabled_outputs() {
        let config: config::Config = toml::from_str("[[outputs]]\nuid='a'\ndelay_ms=12.5\n[[outputs]]\nuid='b'\ndelay_ms=0.01\n[[outputs]]\nuid='c'\nenabled=false\ndelay_ms=5000").unwrap();
        assert_eq!(
            native_offsets(&config, 48_000.0),
            [("a".into(), 600.0), ("b".into(), 0.0)]
        );
    }

    #[test]
    fn rejects_invalid_delays_before_touching_hardware() {
        for invalid in ["NaN", "inf", "-1", "5000.1", "oops"] {
            assert!(delays(&[invalid], 1).is_err());
        }
        assert!(delays(&["0"], 2).is_err());
        assert_eq!(
            delays(&["0", "5000", "0.25"], 3).unwrap(),
            [0.0, 5000.0, 0.25]
        );
    }
}
