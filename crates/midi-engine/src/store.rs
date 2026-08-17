use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use virtual_midi_core::Config;

#[derive(Debug)]
pub struct ConfigStore {
    path: PathBuf,
}

#[derive(Debug)]
pub struct ConfigLoad {
    pub config: Config,
    pub recovered_invalid_file: Option<PathBuf>,
}

impl ConfigStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<ConfigLoad> {
        if !self.path.exists() {
            return Ok(ConfigLoad {
                config: Config::default(),
                recovered_invalid_file: None,
            });
        }
        let contents = fs::read_to_string(&self.path)
            .with_context(|| format!("read configuration {}", self.path.display()))?;
        match serde_json::from_str::<Config>(&contents) {
            Ok(config) if config.validate().is_ok() => Ok(ConfigLoad {
                config,
                recovered_invalid_file: None,
            }),
            _ => {
                let suffix = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let invalid = self.path.with_extension(format!("invalid-{suffix}.json"));
                fs::rename(&self.path, &invalid).with_context(|| {
                    format!("preserve invalid configuration as {}", invalid.display())
                })?;
                Ok(ConfigLoad {
                    config: Config::default(),
                    recovered_invalid_file: Some(invalid),
                })
            }
        }
    }

    pub fn save(&self, config: &Config) -> Result<()> {
        config.validate().context("validate configuration")?;
        let parent = self.path.parent().context("configuration has no parent")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("create configuration directory {}", parent.display()))?;
        let temporary = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(config).context("serialize configuration")?;
        {
            let mut file = fs::File::create(&temporary)
                .with_context(|| format!("create temporary file {}", temporary.display()))?;
            file.write_all(&bytes).context("write configuration")?;
            file.sync_all().context("sync configuration")?;
        }
        atomic_replace(&temporary, &self.path)
            .with_context(|| format!("replace configuration {}", self.path.display()))?;
        Ok(())
    }
}

#[cfg(not(windows))]
fn atomic_replace(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(temporary, destination)
}

#[cfg(windows)]
fn atomic_replace(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{REPLACE_FILE_FLAGS, ReplaceFileW};
    use windows::core::PCWSTR;

    if !destination.exists() {
        return fs::rename(temporary, destination);
    }
    let destination_wide = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let temporary_wide = temporary
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    unsafe {
        ReplaceFileW(
            PCWSTR(destination_wide.as_ptr()),
            PCWSTR(temporary_wide.as_ptr()),
            PCWSTR::null(),
            REPLACE_FILE_FLAGS(0),
            None,
            None,
        )
        .map_err(|error| std::io::Error::other(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;
    use virtual_midi_core::{Config, VirtualDevice};

    use super::*;

    #[test]
    fn saves_and_loads_a_valid_configuration() {
        let directory = tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("config.json"));
        let config = Config {
            virtual_devices: vec![VirtualDevice {
                id: uuid::Uuid::new_v4(),
                name: "Main".into(),
                enabled: true,
                effects: Vec::new(),
            }],
            ..Config::default()
        };

        store.save(&config).unwrap();
        let loaded = store.load().unwrap();

        assert_eq!(loaded.config, config);
        assert!(loaded.recovered_invalid_file.is_none());
    }

    #[test]
    fn preserves_a_damaged_configuration_and_recovers_with_defaults() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("config.json");
        fs::write(&path, "{ definitely not json").unwrap();
        let store = ConfigStore::new(path.clone());

        let loaded = store.load().unwrap();

        assert_eq!(loaded.config, Config::default());
        let invalid = loaded.recovered_invalid_file.unwrap();
        assert_eq!(
            fs::read_to_string(invalid).unwrap(),
            "{ definitely not json"
        );
        assert!(!path.exists());
    }
}
