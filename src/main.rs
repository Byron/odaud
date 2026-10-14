#[cfg(not(target_os = "macos"))]
compile_error!("ODAUD requires macOS");

mod config;
mod hal;
mod power;
mod probe;

use std::io::Write;
use std::path::Path;
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

const HELP: &str = "ODAUD — temporary stereo output with native delay compensation

  odaud devices [--all]
  odaud inspect UID
  odaud check CONFIG.toml
  odaud run CONFIG.toml
  odaud cleanup ORPHAN_UID

Use stable UIDs from 'devices' in the configuration. Run stays in the foreground.
SIGHUP reloads delays; SIGINT/SIGTERM stops and removes the temporary output.
Select ODAUD in the macOS sound controls or another application's output selector.
ODAUD never changes the default output. Output/name changes require restart.

Experimental diagnostic commands:
  odaud probe-native STACKED SECONDS RELOAD_FILE_OR_DASH UID DELAY_MS [UID DELAY_MS ...]
  odaud probe-play UID SECONDS
  odaud probe-capture UID SECONDS OUTPUT.wav

STACKED is 0 or 1. Native delay values are experimental HAL compensation inputs,
not yet verified playback delays. RELOAD_FILE contains one delay in milliseconds
per output, separated by whitespace; SIGHUP applies it without recreating the device.
The first output is the clock reference. No default output is changed.
SIGINT/SIGTERM cleanly remove the probe device. Playback uses quiet synthetic chirps.";

fn main() {
    unsafe {
        for number in [1, 2, 15] {
            if signal(number, on_signal as *const () as usize) == usize::MAX {
                eprintln!("odaud: cannot register shutdown signal handler");
                std::process::exit(1);
            }
        }
    }
    if let Err(error) = run(&std::env::args().skip(1).collect::<Vec<_>>()) {
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

fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        None | Some("--help" | "-h" | "help") => println!("{HELP}"),
        Some("devices") if args.len() == 1 || (args.len() == 2 && args[1] == "--all") => {
            let devices = hal::devices()?;
            if devices.is_empty() {
                return Err(
                    "Core Audio returned no devices; run outside a restricted sandbox".into(),
                );
            }
            for d in devices
                .into_iter()
                .filter(|device| args.len() == 2 || device.is_physical_output())
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
        Some("inspect") if args.len() == 2 => {
            println!("{}", hal::inspect(find_device(&args[1])?.id)?);
        }
        Some("check") if args.len() == 2 => {
            let config = config::Config::load(Path::new(&args[1]))?;
            let (selected, rate) = selected_outputs(&config)?;
            println!(
                "valid: {:?}, {} outputs at {rate} Hz",
                config.name,
                selected.len()
            );
            print_outputs(&config, rate);
        }
        Some("run") if args.len() == 2 => configured(Path::new(&args[1]))?,
        Some("cleanup") if args.len() == 2 => {
            hal::cleanup_orphan(&args[1])?;
            println!("removed orphan {:?}", args[1]);
        }
        Some("probe-play") if args.len() == 3 => {
            println!(
                "{}",
                probe::play(find_device(&args[1])?.id, seconds(&args[2])?)?
            );
        }
        Some("probe-capture") if args.len() == 4 => {
            println!(
                "{}",
                probe::capture(
                    find_device(&args[1])?.id,
                    seconds(&args[2])?,
                    Path::new(&args[3])
                )?
            );
        }
        Some("probe-native") if args.len() >= 6 && args.len() % 2 == 0 => native(&args[1..])?,
        _ => return Err(format!("invalid arguments\n\n{HELP}")),
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

fn configured(path: &Path) -> Result<(), String> {
    let mut config = config::Config::load(path)?;
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
    println!(
        "Select this device explicitly in macOS or your player. SIGHUP reloads delays; Ctrl-C stops."
    );
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    let mut result = Ok(());
    let mut health_at = Instant::now();
    while !STOP.load(Ordering::Relaxed) {
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
    let cleanup = aggregate.destroy();
    let sleep = power.allow_sleep();
    cleanup?;
    sleep?;
    println!("stopped: removed {uid:?}");
    result
}

fn native(args: &[String]) -> Result<(), String> {
    let stacked = match args[0].as_str() {
        "0" => false,
        "1" => true,
        _ => return Err("STACKED must be 0 or 1".into()),
    };
    let duration = seconds(&args[1])?;
    let reload_path = &args[2];
    let selected = args[3..]
        .chunks_exact(2)
        .map(|pair| find_device(&pair[0]))
        .collect::<Result<Vec<_>, _>>()?;
    let raw_delays = args[3..]
        .chunks_exact(2)
        .map(|pair| pair[1].as_str())
        .collect::<Vec<_>>();
    let requested = delays(&raw_delays, selected.len())?;
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
                if reload_path == "-" {
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
