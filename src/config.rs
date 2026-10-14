use serde::Deserialize;
use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
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
        Self::load_with_source(path).map(|(config, _)| config)
    }

    pub fn load_with_source(path: &Path) -> Result<(Self, String), String> {
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
        Ok((config, text))
    }

    /// Save only enabled output delays, retaining the original document and rejecting external edits.
    pub fn save_delays(&self, path: &Path, original: &str) -> Result<String, String> {
        self.validate()?;
        let read_original = || {
            let metadata = fs::symlink_metadata(path)
                .map_err(|error| format!("Inspect {} before saving: {error}", path.display()))?;
            if !metadata.is_file() || metadata.nlink() != 1 {
                return Err(
                    "Saving requires a regular configuration file without symbolic or hard links"
                        .to_string(),
                );
            }
            let (config, source) = Self::load_with_source(path)?;
            if source != original {
                return Err("Configuration changed on disk; reload it before saving".to_string());
            }
            Ok((config, metadata))
        };
        let (current, metadata) = read_original()?;
        if !self.same_topology(&current) {
            return Err(
                "Saving delays cannot change the device name or enabled output topology".into(),
            );
        }
        let mut document: toml_edit::DocumentMut = original
            .parse()
            .map_err(|error| format!("Parse configuration for saving: {error}"))?;
        for (index, old) in current
            .outputs
            .iter()
            .enumerate()
            .filter(|(_, o)| o.enabled)
        {
            let output = self
                .enabled()
                .find(|output| output.uid == old.uid)
                .ok_or("Enabled output disappeared while saving")?;
            if output.delay_ms == old.delay_ms {
                continue;
            }
            let table = document
                .get_mut("outputs")
                .and_then(|outputs| outputs.get_mut(index))
                .and_then(toml_edit::Item::as_table_like_mut)
                .ok_or("Cannot locate output table while saving")?;
            let mut value = toml_edit::Value::from(output.delay_ms);
            if let Some(previous) = table.get("delay_ms").and_then(toml_edit::Item::as_value) {
                *value.decor_mut() = previous.decor().clone();
            }
            table.insert("delay_ms", toml_edit::Item::Value(value));
        }
        let rendered = document.to_string();
        if rendered.len() as u64 > MAX_FILE_BYTES {
            return Err("Updated configuration exceeds the 1 MiB limit".into());
        }
        let validated: Self = toml::from_str(&rendered)
            .map_err(|error| format!("Validate updated configuration: {error}"))?;
        validated.validate()?;
        if !self.same_topology(&validated)
            || self
                .enabled()
                .zip(validated.enabled())
                .any(|(expected, actual)| expected.delay_ms != actual.delay_ms)
        {
            return Err("Updated configuration does not match the requested output delays".into());
        }

        static NEXT_SAVE: AtomicU64 = AtomicU64::new(0);
        let temporary = path.with_file_name(format!(
            ".odaud-save-{}-{}.tmp",
            std::process::id(),
            NEXT_SAVE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|error| format!("Create {}: {error}", temporary.display()))?;
        let save = (|| {
            file.write_all(rendered.as_bytes())
                .and_then(|()| file.set_permissions(metadata.permissions()))
                .and_then(|()| file.sync_all())
                .map_err(|error| format!("Write {}: {error}", temporary.display()))?;
            let (_, latest) = read_original()?;
            if latest.dev() != metadata.dev()
                || latest.ino() != metadata.ino()
                || latest.mode() != metadata.mode()
            {
                return Err(
                    "Configuration was replaced or its permissions changed while saving".into(),
                );
            }
            // shortcut: other editors can race this final check; coordinate writers if concurrent saves are needed.
            fs::rename(&temporary, path)
                .map_err(|error| format!("Replace {}: {error}", path.display()))
        })();
        if save.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        save?;
        Ok(rendered)
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
        os::unix::fs::{PermissionsExt, symlink},
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct ConfigFile {
        directory: PathBuf,
        path: PathBuf,
    }

    impl ConfigFile {
        fn new(source: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let directory = std::env::temp_dir().join(format!(
                "odaud-save-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&directory).unwrap();
            let path = directory.join("outputs.toml");
            fs::write(&path, source).unwrap();
            Self { directory, path }
        }
    }

    impl Drop for ConfigFile {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

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
    fn saves_enabled_delays_preserving_comments_disabled_outputs_and_permissions() {
        let source = "# Listening room\nname = 'Desk'\n[[outputs]] # reference\nuid = 'a'\ndelay_ms = 2.5 # measured\n\n[[outputs]]\nuid = 'unused'\nenabled = false\ndelay_ms = 7 # leave this alone\n\n[[outputs]]\nuid = 'b'\n";
        let file = ConfigFile::new(source);
        fs::set_permissions(&file.path, fs::Permissions::from_mode(0o640)).unwrap();
        let (mut config, baseline) = Config::load_with_source(&file.path).unwrap();
        assert_eq!(baseline, source);
        config.outputs[0].delay_ms = 3.5;
        config.outputs[1].delay_ms = 99.0;
        config.outputs[2].delay_ms = 4.0;
        let saved = config.save_delays(&file.path, &baseline).unwrap();
        assert_eq!(saved, source.replace("2.5", "3.5") + "delay_ms = 4.0\n");
        assert_eq!(fs::read_to_string(&file.path).unwrap(), saved);
        assert_eq!(
            fs::metadata(&file.path).unwrap().permissions().mode() & 0o777,
            0o640
        );
        let (loaded, new_baseline) = Config::load_with_source(&file.path).unwrap();
        assert_eq!(loaded.outputs[0].delay_ms, 3.5);
        assert_eq!(loaded.outputs[1].delay_ms, 7.0);
        assert_eq!(loaded.outputs[2].delay_ms, 4.0);
        assert_eq!(new_baseline, saved);
        config.outputs[0].delay_ms = 5.0;
        assert!(config.save_delays(&file.path, &new_baseline).is_ok());
        assert_eq!(fs::read_dir(&file.directory).unwrap().count(), 1);
    }

    #[test]
    fn saves_inline_tables_without_reformatting_other_values() {
        let source = "outputs = [{ uid = 'a', delay_ms = 2.0 }, { uid = 'b', enabled = false, delay_ms = 7.0 }] # original layout\n";
        let file = ConfigFile::new(source);
        let mut config = Config::load(&file.path).unwrap();
        config.outputs[0].delay_ms = 12.0;
        assert_eq!(
            config.save_delays(&file.path, source).unwrap(),
            source.replace("2.0", "12.0")
        );
    }

    #[test]
    fn refuses_stale_or_invalid_saves_without_changing_the_original() {
        let source = "[[outputs]]\nuid = 'a'\ndelay_ms = 2.0\n";
        let file = ConfigFile::new(source);
        let mut config = Config::load(&file.path).unwrap();
        config.outputs[0].delay_ms = 3.0;
        let external = format!("{source}# external edit\n");
        fs::write(&file.path, &external).unwrap();
        assert!(
            config
                .save_delays(&file.path, source)
                .unwrap_err()
                .contains("changed on disk")
        );
        assert_eq!(fs::read_to_string(&file.path).unwrap(), external);
        fs::write(&file.path, source).unwrap();
        config.outputs[0].delay_ms = f64::NAN;
        assert!(config.save_delays(&file.path, source).is_err());
        config.outputs[0].delay_ms = 3.0;
        config.name = "Different name".into();
        assert!(
            config
                .save_delays(&file.path, source)
                .unwrap_err()
                .contains("topology")
        );
        assert_eq!(fs::read_to_string(&file.path).unwrap(), source);
        assert_eq!(fs::read_dir(&file.directory).unwrap().count(), 1);
    }

    #[test]
    fn refuses_to_replace_symbolic_and_hard_links() {
        let source = "[[outputs]]\nuid = 'a'\n";
        let file = ConfigFile::new(source);
        let mut config = Config::load(&file.path).unwrap();
        config.outputs[0].delay_ms = 1.0;
        let symbolic = file.directory.join("symbolic.toml");
        symlink(&file.path, &symbolic).unwrap();
        assert!(
            config
                .save_delays(&symbolic, source)
                .unwrap_err()
                .contains("symbolic")
        );
        assert!(
            fs::symlink_metadata(&symbolic)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let hard = file.directory.join("hard.toml");
        fs::hard_link(&file.path, hard).unwrap();
        assert!(
            config
                .save_delays(&file.path, source)
                .unwrap_err()
                .contains("hard links")
        );
        assert_eq!(fs::read_to_string(&file.path).unwrap(), source);
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
