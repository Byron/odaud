use serde::Deserialize;
use std::{
    collections::HashSet, fs::OpenOptions, io::Read, os::unix::fs::OpenOptionsExt, path::Path,
};

const MAX_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "default_name")]
    pub name: String,
    pub outputs: Vec<Output>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub uid: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub delay_ms: f64,
}

fn default_name() -> String {
    "ODAUD".into()
}

fn default_enabled() -> bool {
    true
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, String> {
        // Darwin O_NONBLOCK lets us reject a FIFO without waiting for a writer, including a replacement race.
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(0x0000_0004)
            .open(path)
            .map_err(|e| format!("Open {}: {e}", path.display()))?;
        let metadata = file
            .metadata()
            .map_err(|e| format!("Inspect {}: {e}", path.display()))?;
        if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
            return Err("Configuration must be a regular file of at most 1 MiB".into());
        }
        let mut text = String::with_capacity(metadata.len() as usize);
        // Bound reads too: the file can grow after the metadata check.
        file.take(MAX_FILE_BYTES + 1)
            .read_to_string(&mut text)
            .map_err(|e| format!("Read {}: {e}", path.display()))?;
        if text.len() as u64 > MAX_FILE_BYTES {
            return Err("Configuration exceeds the 1 MiB limit".into());
        }
        let config: Self =
            toml::from_str(&text).map_err(|e| format!("Parse {}: {e}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() || self.name.contains('\0') || self.name.len() > 1024 {
            return Err(
                "Device name must be nonempty, contain no NUL, and use at most 1024 UTF-8 bytes"
                    .into(),
            );
        }
        let mut seen = HashSet::new();
        for output in &self.outputs {
            if output.uid.trim().is_empty() || output.uid.contains('\0') || output.uid.len() > 4096
            {
                return Err(
                    "Output UID must be nonempty, contain no NUL, and use at most 4096 UTF-8 bytes"
                        .into(),
                );
            }
            if !seen.insert(&output.uid) {
                return Err(format!("Duplicate output UID {:?}", output.uid));
            }
            if !output.delay_ms.is_finite() || !(0.0..=5000.0).contains(&output.delay_ms) {
                return Err(format!(
                    "Delay for {:?} must be finite and between 0 and 5000 ms",
                    output.uid
                ));
            }
        }
        if !(1..=32).contains(&self.enabled().count()) {
            return Err("Configuration must enable between 1 and 32 outputs".into());
        }
        Ok(())
    }

    pub fn enabled(&self) -> impl Iterator<Item = &Output> {
        self.outputs.iter().filter(|output| output.enabled)
    }

    /// Delay-only reloads retain the name and ordered list of enabled devices.
    pub fn same_topology(&self, other: &Self) -> bool {
        self.name == other.name
            && self
                .enabled()
                .map(|output| &output.uid)
                .eq(other.enabled().map(|output| &output.uid))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    fn parse(text: &str) -> Result<Config, String> {
        let config: Config = toml::from_str(text).map_err(|e| e.to_string())?;
        config.validate()?;
        Ok(config)
    }

    fn example() -> Config {
        parse("[[outputs]]\nuid='built-in'\n[[outputs]]\nuid='usb'\ndelay_ms=100.5\n").unwrap()
    }

    #[test]
    fn enabled_output_count_is_bounded() {
        let mut config = example();
        config.outputs = (0..32)
            .map(|i| Output {
                uid: format!("output-{i}"),
                enabled: true,
                delay_ms: 0.0,
            })
            .collect();
        assert!(config.validate().is_ok());
        config.outputs.push(Output {
            uid: "extra".into(),
            enabled: true,
            delay_ms: 0.0,
        });
        assert!(config.validate().is_err());
        config.outputs[32].enabled = false;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn defaults_and_enabled_order() {
        let config = example();
        assert_eq!(config.name, "ODAUD");
        assert_eq!(config.outputs[0].delay_ms, 0.0);
        assert!(config.outputs.iter().all(|output| output.enabled));
        assert_eq!(
            config
                .enabled()
                .map(|output| output.uid.as_str())
                .collect::<Vec<_>>(),
            ["built-in", "usb"]
        );
        let config = parse("name='Listening room'\n[[outputs]]\nuid='a'\nenabled=false\n[[outputs]]\nuid='b'\ndelay_ms=5000\n").unwrap();
        assert_eq!(config.name, "Listening room");
        assert_eq!(
            config
                .enabled()
                .map(|output| output.uid.as_str())
                .collect::<Vec<_>>(),
            ["b"]
        );
    }

    #[test]
    fn rejects_unknown_fields_invalid_delays_and_empty_selection() {
        for text in [
            "outputs=[]",
            "[[outputs]]\nuid='a'\nenabled=false",
            "typo=true\n[[outputs]]\nuid='a'",
            "[[outputs]]\nuid='a'\ndelay=1",
            "[[outputs]]\nuid='a'\nenabled='yes'",
        ] {
            assert!(parse(text).is_err(), "accepted {text}");
        }
        for delay in ["nan", "inf", "-inf", "-0.1", "5000.1"] {
            assert!(parse(&format!("[[outputs]]\nuid='a'\ndelay_ms={delay}")).is_err());
        }
        assert!(parse("[[outputs]]\nuid='a'\n[[outputs]]\nuid='a'\nenabled=false").is_err());
    }

    #[test]
    fn rejects_invalid_names_and_uids_including_disabled_outputs() {
        for name in [
            String::new(),
            "  ".into(),
            "bad\0name".into(),
            "x".repeat(1025),
        ] {
            let mut config = example();
            config.name = name;
            assert!(config.validate().is_err());
        }
        for uid in [
            String::new(),
            "  ".into(),
            "bad\0uid".into(),
            "x".repeat(4097),
        ] {
            let mut config = example();
            config.outputs[1].enabled = false;
            config.outputs[1].uid = uid;
            assert!(config.validate().is_err());
        }
        let mut config = example();
        config.name = "x".repeat(1024);
        config.outputs[0].uid = "x".repeat(4096);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn reload_topology_ignores_delays_but_preserves_name_and_enabled_order() {
        let original = example();
        let mut changed = original.clone();
        changed.outputs[1].delay_ms = 0.0;
        assert!(original.same_topology(&changed));
        changed.outputs.push(Output {
            uid: "unused".into(),
            enabled: false,
            delay_ms: 0.0,
        });
        assert!(original.same_topology(&changed));
        changed.outputs.swap(0, 1);
        assert!(!original.same_topology(&changed));
        changed = original.clone();
        changed.outputs[0].enabled = false;
        assert!(!original.same_topology(&changed));
        changed = original.clone();
        changed.name = "New device".into();
        assert!(!original.same_topology(&changed));
    }

    #[test]
    fn load_validates_and_rejects_oversized_files() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "odaud-config-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        file.set_len(MAX_FILE_BYTES + 1).unwrap();
        let result = Config::load(&path);
        fs::remove_file(&path).unwrap();
        assert!(result.unwrap_err().contains("1 MiB"));
        fs::write(&path, "[[outputs]]\nuid='a'\n").unwrap();
        let result = Config::load(&path);
        fs::remove_file(&path).unwrap();
        assert_eq!(result.unwrap().outputs[0].uid, "a");
    }

    #[test]
    fn rejects_fifo_without_waiting_for_writer() {
        use std::{
            process::{Command, Stdio},
            time::{Duration, Instant},
        };

        const TEST_PATH: &str = "ODAUD_CONFIG_FIFO_TEST_PATH";
        if let Some(path) = std::env::var_os(TEST_PATH) {
            assert!(
                Config::load(Path::new(&path))
                    .unwrap_err()
                    .contains("regular file")
            );
            return;
        }

        let path = std::env::temp_dir().join(format!("odaud-fifo-test-{}", std::process::id()));
        assert!(
            Command::new("mkfifo")
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        // A regression must fail the test, not block the entire test runner in open(2).
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("rejects_fifo_without_waiting_for_writer")
            .env(TEST_PATH, &path)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let completed = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status.success();
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        fs::remove_file(&path).unwrap();
        assert!(
            completed,
            "FIFO rejection failed or blocked waiting for a writer"
        );
    }
}
