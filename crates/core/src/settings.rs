use crate::error::{Error, Result};
use crate::paths::sanitize_relative_path;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MIN_TIMER_MINUTES: u32 = 1;
pub const MAX_TIMER_MINUTES: u32 = 1440;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    #[serde(rename = "CHOSEN_FOLDER")]
    pub chosen_folder: PathBuf,
    #[serde(rename = "AUTO_DELETE")]
    pub auto_delete: bool,
    #[serde(rename = "TIMER_MINUTES")]
    pub timer_minutes: u32,
    #[serde(rename = "FAKE_EXE_DIR")]
    pub fake_exe_dir: String,
}

pub fn default_folder() -> PathBuf {
    dirs::desktop_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Desktop")))
        .unwrap_or_else(|| PathBuf::from("."))
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            chosen_folder: default_folder(),
            auto_delete: false,
            timer_minutes: 16,
            fake_exe_dir: "Win64".into(),
        }
    }
}

impl Settings {
    pub fn normalized(mut self) -> Result<Settings> {
        if self.chosen_folder.as_os_str().is_empty() || self.chosen_folder == Path::new("Desktop") {
            self.chosen_folder = default_folder();
        }
        if !(MIN_TIMER_MINUTES..=MAX_TIMER_MINUTES).contains(&self.timer_minutes) {
            return Err(Error::Invalid(format!(
                "Timer must be between {MIN_TIMER_MINUTES} and {MAX_TIMER_MINUTES} minutes."
            )));
        }
        self.fake_exe_dir = sanitize_relative_path(&self.fake_exe_dir);
        Ok(self)
    }

    pub fn load(path: &Path) -> Settings {
        let parsed = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Settings>(&text).ok());
        parsed
            .and_then(|s| s.normalized().ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| Error::Invalid(format!("Cannot serialise settings: {e}")))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)?;
        Ok(())
    }

    pub fn fake_dir(&self) -> PathBuf {
        self.chosen_folder.join(&self.fake_exe_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_settings_default_to_sixteen_minutes() {
        let dir = tempfile::tempdir().unwrap();
        let s = Settings::load(&dir.path().join("settings.json"));
        assert_eq!(s.timer_minutes, 16);
        assert!(!s.auto_delete);
        assert_eq!(s.fake_exe_dir, "Win64");
    }

    #[test]
    fn reads_legacy_json_with_missing_keys() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(&p, r#"{"AUTO_DELETE": true, "TIMER_MINUTES": 30}"#).unwrap();
        let s = Settings::load(&p);
        assert!(s.auto_delete);
        assert_eq!(s.timer_minutes, 30);
        assert_eq!(s.fake_exe_dir, "Win64");
    }

    #[test]
    fn broken_json_falls_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(&p, "{not json").unwrap();
        assert_eq!(Settings::load(&p), Settings::default());
    }

    #[test]
    fn out_of_range_timer_in_file_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(&p, r#"{"TIMER_MINUTES": 0}"#).unwrap();
        assert_eq!(Settings::load(&p), Settings::default());
    }

    #[test]
    fn normalized_rejects_bad_timer_and_sanitises_dir() {
        let bad = Settings { timer_minutes: 5000, ..Settings::default() };
        assert!(bad.normalized().is_err());
        let ok = Settings { fake_exe_dir: "..\\x:y".into(), ..Settings::default() };
        assert_eq!(ok.normalized().unwrap().fake_exe_dir, "xy");
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("nested").join("settings.json");
        let s = Settings {
            chosen_folder: dir.path().to_path_buf(),
            auto_delete: true,
            timer_minutes: 42,
            fake_exe_dir: "Bin".into(),
        };
        s.save(&p).unwrap();
        assert_eq!(Settings::load(&p), s);
    }
}
