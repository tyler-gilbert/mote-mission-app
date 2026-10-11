use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use serde::{Deserialize, Serialize};

use crate::{
    transport::{TransportWorker, WorkerSnapshot},
    ui::ControlPanel,
};

const SETTINGS_DIRECTORY: &str = ".mote-mission";
const SETTINGS_FILE: &str = "settings.json";

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct PersistentSettings {
    pub(crate) mission_settings_path: String,
}

impl PersistentSettings {
    pub(crate) fn load() -> Result<Self, String> {
        let path = Self::file_path()?;
        Self::load_from(&path)
    }

    fn load_from(path: &Path) -> Result<Self, String> {
        match fs::read_to_string(path) {
            Ok(contents) => serde_json::from_str(&contents).map_err(|error| {
                format!("Could not parse app settings '{}': {error}", path.display())
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(format!(
                "Could not read app settings '{}': {error}",
                path.display()
            )),
        }
    }

    pub(crate) fn save(&self) -> Result<(), String> {
        let path = Self::file_path()?;
        self.save_to(&path)
    }

    fn file_path() -> Result<PathBuf, String> {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .ok_or_else(|| "Could not determine the home directory for app settings.".to_owned())?;
        Ok(home.join(SETTINGS_DIRECTORY).join(SETTINGS_FILE))
    }

    fn save_to(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "Could not create app settings directory '{}': {error}",
                    parent.display()
                )
            })?;
        }
        let contents = serde_json::to_string_pretty(self)
            .map_err(|error| format!("Could not serialize app settings: {error}"))?;
        fs::write(path, contents)
            .map_err(|error| format!("Could not write app settings '{}': {error}", path.display()))
    }
}

pub(crate) struct Context {
    pub(crate) worker: TransportWorker,
    pub(crate) persistent_settings: PersistentSettings,

    pub(crate) device_refresh_until: Option<Instant>,
    pub(crate) snapshot: WorkerSnapshot,
    pub(crate) last_error: Option<String>,
    pub(crate) active_panel: ControlPanel,
    pub(crate) refreshing_devices: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persistent_settings_round_trip() {
        let path = std::env::temp_dir().join(format!(
            "mote-mission-settings-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let settings = PersistentSettings {
            mission_settings_path: "/tmp/mission.json".to_owned(),
        };

        settings.save_to(&path).unwrap();

        assert_eq!(PersistentSettings::load_from(&path).unwrap(), settings);
        fs::remove_file(path).unwrap();
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum UiAction {
    SelectPanel(ControlPanel),
    Refresh,
    SelectDevice(String),
    Connect(String),
    Disconnect,
}
