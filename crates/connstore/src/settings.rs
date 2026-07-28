//! App-wide preferences (theme, update-check, UI state, editor prefs) persisted
//! as `settings.json` in the same platform config dir as `connections.json`.
//!
//! Nothing here is secret, so unlike [`crate::store::ConnStore`] there is no
//! secret backend — it is a plain serde round-trip with a full-file flush on
//! each mutation (the struct is tiny).

use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::error::{ConnStoreError, Result};

/// Which color scheme the UI should start in. Matches the app's single
/// light/dark toggle (`Theme.dark`); default light mirrors `tokens.slint`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    Light,
    Dark,
}

impl ThemeMode {
    /// The app renders theme as a single `dark` bool.
    pub fn is_dark(self) -> bool {
        matches!(self, ThemeMode::Dark)
    }

    pub fn from_dark(dark: bool) -> Self {
        if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        }
    }
}

/// UI state worth restoring across restarts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct UiState {
    /// Names of sidebar groups the user has collapsed.
    pub collapsed_groups: Vec<String>,
    /// Last connection-picker filter text.
    pub last_filter: String,
    /// Place the workspace sidebar on the right instead of the left.
    pub sidebar_right: bool,
}

/// Query-editor / results preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorPrefs {
    pub font_size: u16,
    pub default_page_size: u32,
    pub history_max_entries: u16,
}

impl Default for EditorPrefs {
    fn default() -> Self {
        EditorPrefs {
            font_size: 13,
            default_page_size: 100,
            history_max_entries: 50,
        }
    }
}

/// All persisted app preferences. Container-level `#[serde(default)]` means an
/// absent or partial `settings.json` fills every missing field from
/// [`AppSettings::default`] — including `update_check = true`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub theme: ThemeMode,
    /// Whether to check GitHub for a newer release on launch.
    pub update_check: bool,
    /// Unix seconds of the last update check, used to throttle to once/day.
    pub last_update_check: Option<i64>,
    pub ui_state: UiState,
    pub editor: EditorPrefs,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            theme: ThemeMode::default(),
            update_check: true,
            last_update_check: None,
            ui_state: UiState::default(),
            editor: EditorPrefs::default(),
        }
    }
}

/// Owns `settings.json` and flushes the whole file on each mutation.
pub struct SettingsStore {
    path: PathBuf,
    settings: AppSettings,
}

impl SettingsStore {
    /// Construct with an explicit path (used by tests and by callers that pin a
    /// directory). Loads the file if present, else starts from defaults.
    pub fn load(path: PathBuf) -> Result<Self> {
        let settings = if path.exists() {
            let raw = std::fs::read_to_string(&path)?;
            serde_json::from_str(&raw)?
        } else {
            AppSettings::default()
        };
        Ok(SettingsStore { path, settings })
    }

    /// Default location of `settings.json`: platform config dir for qualifier
    /// `dev`, org `dbm`, app `dbm` — the same dir as `connections.json`.
    pub fn default_path() -> Result<PathBuf> {
        let dirs = ProjectDirs::from("dev", "dbm", "dbm").ok_or(ConnStoreError::NoConfigDir)?;
        Ok(dirs.config_dir().join("settings.json"))
    }

    /// Open at the platform default path.
    pub fn open_default() -> Result<Self> {
        Self::load(Self::default_path()?)
    }

    /// Read-only view of the current settings.
    pub fn get(&self) -> &AppSettings {
        &self.settings
    }

    /// Mutate settings via a closure, then flush to disk.
    pub fn update(&mut self, f: impl FnOnce(&mut AppSettings)) -> Result<()> {
        f(&mut self.settings);
        self.flush()
    }

    fn flush(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.settings)?;
        std::fs::write(&self.path, json)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rdb-settings-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("settings.json")
    }

    #[test]
    fn defaults_when_file_absent() {
        let s = SettingsStore::load(tmp()).unwrap();
        assert_eq!(s.get().theme, ThemeMode::Light);
        assert!(s.get().update_check);
        assert_eq!(s.get().last_update_check, None);
        assert_eq!(s.get().editor.default_page_size, 100);
        assert_eq!(s.get().editor.history_max_entries, 50);
    }

    #[test]
    fn round_trips_through_disk() {
        let path = tmp();
        let mut s = SettingsStore::load(path.clone()).unwrap();
        s.update(|s| {
            s.theme = ThemeMode::Dark;
            s.update_check = false;
            s.last_update_check = Some(1_700_000_000);
            s.ui_state.collapsed_groups = vec!["Prod".into()];
            s.ui_state.last_filter = "pg".into();
            s.editor.font_size = 16;
            s.editor.history_max_entries = 100;
        })
        .unwrap();

        let reloaded = SettingsStore::load(path).unwrap();
        assert_eq!(reloaded.get().theme, ThemeMode::Dark);
        assert!(!reloaded.get().update_check);
        assert_eq!(reloaded.get().last_update_check, Some(1_700_000_000));
        assert_eq!(reloaded.get().ui_state.collapsed_groups, vec!["Prod"]);
        assert_eq!(reloaded.get().ui_state.last_filter, "pg");
        assert_eq!(reloaded.get().editor.font_size, 16);
        assert_eq!(reloaded.get().editor.history_max_entries, 100);
    }

    #[test]
    fn partial_json_fills_missing_with_defaults() {
        let path = tmp();
        std::fs::write(&path, r#"{"theme":"dark"}"#).unwrap();
        let s = SettingsStore::load(path).unwrap();
        assert_eq!(s.get().theme, ThemeMode::Dark);
        // update_check missing from JSON -> default true, not false.
        assert!(s.get().update_check);
        assert_eq!(s.get().editor.default_page_size, 100);
        assert_eq!(s.get().editor.history_max_entries, 50);
    }
}
