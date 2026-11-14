//! Atomic TOML settings, recent files, and window geometry persistence (`M1-05`).
//!
//! Stores user configuration (`settings.toml`) and session shell state (`state.toml`) inside
//! the OS application data directory (`dirs::config_dir()/Ordia` or `ORDIA_DATA_DIR` override),
//! writing every update atomically via sibling `.tmp` write + fsync + TOML self-check + rename.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// Errors produced when loading or atomically saving Ordia configuration files.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    /// Filesystem I/O failure.
    #[error("settings I/O error at {path}: {source}")]
    Io {
        /// File path where I/O failed.
        path: PathBuf,
        /// Underlying OS I/O error.
        source: std::io::Error,
    },
    /// TOML deserialization error.
    #[error("failed to parse TOML at {path}: {source}")]
    Parse {
        /// File path being parsed.
        path: PathBuf,
        /// Underlying TOML parser error.
        source: toml::de::Error,
    },
    /// TOML serialization error.
    #[error("failed to serialize TOML for {path}: {source}")]
    Serialize {
        /// Target file path.
        path: PathBuf,
        /// Underlying TOML serializer error.
        source: toml::ser::Error,
    },
}

/// Appearance theme preference (`System`, `Light`, or `Dark`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppearanceMode {
    /// Follow the operating system's appearance (`default`).
    #[default]
    System,
    /// Always use Light theme.
    Light,
    /// Always use Dark theme (shell chrome dark, spreadsheet canvas invariant `#FFFFFF`).
    Dark,
}

/// Persisted interface language preference (`System`, `zh-CN`, or `en`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LanguageSetting {
    /// Follow the operating system's locale (`default`).
    #[default]
    System,
    /// Simplified Chinese (`zh-CN`).
    ZhCn,
    /// English (`en`).
    En,
}

/// Cursor movement direction after pressing `Enter` in a spreadsheet cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EnterMoveDirection {
    /// Move selection one row down (`default`, Excel standard).
    #[default]
    Down,
    /// Move selection one column right.
    Right,
    /// Move selection one row up.
    Up,
    /// Move selection one column left.
    Left,
    /// Keep selection on the edited cell.
    Stay,
}

/// General startup and recent-files preferences.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct GeneralSettings {
    /// Whether to reopen tabs from the previous session on launch.
    pub reopen_last_workbooks: bool,
    /// Maximum number of recent files retained on the Home screen.
    pub max_recent_files: usize,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            reopen_last_workbooks: true,
            max_recent_files: 20,
        }
    }
}

/// Appearance preferences (`theme`, UI scale).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct AppearanceSettings {
    /// Preferred chrome theme mode.
    pub theme: AppearanceMode,
    /// Interface zoom percentage (`80..=150`, default `100`).
    pub ui_scale_percent: u16,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: AppearanceMode::System,
            ui_scale_percent: 100,
        }
    }
}

/// Spreadsheet cell editing preferences.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct EditingSettings {
    /// Selection movement direction after pressing `Enter`.
    pub enter_move_direction: EnterMoveDirection,
    /// Optional fixed decimal point insertion count.
    pub auto_decimal_places: Option<u8>,
}

impl Default for EditingSettings {
    fn default() -> Self {
        Self {
            enter_move_direction: EnterMoveDirection::Down,
            auto_decimal_places: None,
        }
    }
}

/// Advanced crash-recovery and autosave preferences.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct AdvancedSettings {
    /// Whether the append-only crash recovery journal (`M1-07`) is enabled.
    pub enable_crash_recovery: bool,
    /// Periodic journal flush / snapshot interval in seconds.
    pub autosave_interval_secs: u32,
}

impl Default for AdvancedSettings {
    fn default() -> Self {
        Self {
            enable_crash_recovery: true,
            autosave_interval_secs: 10,
        }
    }
}

/// Complete user-facing Ordia settings document (`settings.toml`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct AppSettings {
    /// General startup & recent-files preferences.
    pub general: GeneralSettings,
    /// Appearance & theme preferences.
    pub appearance: AppearanceSettings,
    /// Interface language preference (`System`, `zh-CN`, `en`).
    pub language: LanguageSetting,
    /// Cell editing preferences.
    pub editing: EditingSettings,
    /// Advanced recovery & autosave preferences.
    pub advanced: AdvancedSettings,
}

/// Persisted window geometry (`x`, `y`, `width`, `height`, `maximized`) in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PersistedWindowBounds {
    /// Window origin X in logical pixels.
    pub x: f32,
    /// Window origin Y in logical pixels.
    pub y: f32,
    /// Window width in logical pixels.
    pub width: f32,
    /// Window height in logical pixels.
    pub height: f32,
    /// Whether the window was maximized.
    pub maximized: bool,
}

impl PersistedWindowBounds {
    /// Constructs persisted window bounds, clamping minimum dimensions to `480×360`.
    pub fn new(x: f32, y: f32, width: f32, height: f32, maximized: bool) -> Self {
        Self {
            x,
            y,
            width: width.max(480.0),
            height: height.max(360.0),
            maximized,
        }
    }
}

/// Persisted recent file entry stored in `state.toml`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PersistedRecentFile {
    /// Display file name.
    pub title: String,
    /// Full path on disk.
    pub path: PathBuf,
    /// Human-readable timestamp or tag.
    pub last_opened_label: String,
}

impl PersistedRecentFile {
    /// Creates a persisted recent-file record.
    pub fn new(
        title: impl Into<String>,
        path: impl Into<PathBuf>,
        last_opened_label: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            path: path.into(),
            last_opened_label: last_opened_label.into(),
        }
    }
}

/// Persisted shell state (`state.toml`: window bounds, recent files, panel visibility).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct AppStateSnapshot {
    /// Last active window geometry, if recorded.
    pub window_bounds: Option<PersistedWindowBounds>,
    /// Ordered MRU list of recently opened workbooks.
    pub recent_files: Vec<PersistedRecentFile>,
    /// Whether the left navigation sidebar was collapsed.
    pub sidebar_collapsed: bool,
    /// Whether the right property inspector was open.
    pub inspector_open: bool,
}

impl AppStateSnapshot {
    /// Inserts or promotes `entry` to index 0, deduplicating by `path` and capping at `max_len`.
    pub fn record_recent_file(&mut self, entry: PersistedRecentFile, max_len: usize) {
        self.recent_files.retain(|r| r.path != entry.path);
        self.recent_files.insert(0, entry);
        let cap = max_len.max(1);
        if self.recent_files.len() > cap {
            self.recent_files.truncate(cap);
        }
    }
}

/// Atomic TOML persistence store rooted at the application data directory.
#[derive(Clone, Debug)]
pub struct SettingsStore {
    dir: PathBuf,
}

impl SettingsStore {
    /// Resolves the default OS configuration directory (`ORDIA_DATA_DIR` override or `dirs::config_dir()/Ordia`).
    pub fn default_app_data_dir() -> PathBuf {
        if let Ok(override_dir) = std::env::var("ORDIA_DATA_DIR") {
            let trimmed = override_dir.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed);
            }
        }
        dirs::config_dir()
            .or_else(dirs::data_local_dir)
            .unwrap_or_else(|| PathBuf::from(".ordia-data"))
            .join("Ordia")
    }

    /// Creates a store using [`Self::default_app_data_dir`].
    pub fn default_store() -> Self {
        Self {
            dir: Self::default_app_data_dir(),
        }
    }

    /// Creates a store rooted at an explicit `dir` (ideal for isolated tests).
    pub fn with_dir(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Base directory holding `settings.toml` and `state.toml`.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Path to `settings.toml`.
    pub fn settings_path(&self) -> PathBuf {
        self.dir.join("settings.toml")
    }

    /// Path to `state.toml`.
    pub fn state_path(&self) -> PathBuf {
        self.dir.join("state.toml")
    }

    /// Loads [`AppSettings`] from `settings.toml`, returning [`AppSettings::default`] if absent.
    pub fn load_settings(&self) -> Result<AppSettings, SettingsError> {
        read_toml_or_default(&self.settings_path())
    }

    /// Atomically writes [`AppSettings`] to `settings.toml`.
    pub fn save_settings(&self, settings: &AppSettings) -> Result<(), SettingsError> {
        write_toml_atomic(&self.settings_path(), settings)
    }

    /// Loads [`AppStateSnapshot`] from `state.toml`, returning [`AppStateSnapshot::default`] if absent.
    pub fn load_state(&self) -> Result<AppStateSnapshot, SettingsError> {
        read_toml_or_default(&self.state_path())
    }

    /// Atomically writes [`AppStateSnapshot`] to `state.toml`.
    pub fn save_state(&self, state: &AppStateSnapshot) -> Result<(), SettingsError> {
        write_toml_atomic(&self.state_path(), state)
    }
}

fn read_toml_or_default<T: DeserializeOwned + Default>(path: &Path) -> Result<T, SettingsError> {
    if !path.exists() {
        return Ok(T::default());
    }
    let raw = fs::read_to_string(path).map_err(|source| SettingsError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    toml::from_str(&raw).map_err(|source| SettingsError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

fn write_toml_atomic<T: Serialize + DeserializeOwned>(
    path: &Path,
    value: &T,
) -> Result<(), SettingsError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| SettingsError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let encoded = toml::to_string_pretty(value).map_err(|source| SettingsError::Serialize {
        path: path.to_path_buf(),
        source,
    })?;
    // Pre-flight validation: ensure our serialized TOML parses back cleanly before touching target.
    let _: T = toml::from_str(&encoded).map_err(|source| SettingsError::Parse {
        path: path.to_path_buf(),
        source,
    })?;

    let tmp_path = path.with_extension("toml.tmp");
    {
        let mut file = File::create(&tmp_path).map_err(|source| SettingsError::Io {
            path: tmp_path.clone(),
            source,
        })?;
        file.write_all(encoded.as_bytes())
            .map_err(|source| SettingsError::Io {
                path: tmp_path.clone(),
                source,
            })?;
        file.sync_all().map_err(|source| SettingsError::Io {
            path: tmp_path.clone(),
            source,
        })?;
    }
    fs::rename(&tmp_path, path).map_err(|source| SettingsError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_settings_and_window_state_roundtrip_across_restarts() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let store = SettingsStore::with_dir(dir.path());

        // Missing files yield clean defaults.
        assert_eq!(
            store.load_settings().expect("default settings"),
            AppSettings::default()
        );
        assert_eq!(
            store.load_state().expect("default state"),
            AppStateSnapshot::default()
        );

        let mut settings = AppSettings {
            language: LanguageSetting::ZhCn,
            ..Default::default()
        };
        settings.appearance.theme = AppearanceMode::Dark;
        settings.editing.enter_move_direction = EnterMoveDirection::Right;
        settings.advanced.autosave_interval_secs = 30;
        store.save_settings(&settings).expect("save settings");

        let mut state = AppStateSnapshot {
            window_bounds: Some(PersistedWindowBounds::new(
                120.0, 80.0, 1440.0, 900.0, false,
            )),
            sidebar_collapsed: true,
            inspector_open: true,
            ..Default::default()
        };
        state.record_recent_file(
            PersistedRecentFile::new("Q1.xlsx", "/tmp/Q1.xlsx", "Just now"),
            2,
        );
        state.record_recent_file(
            PersistedRecentFile::new("Q2.xlsx", "/tmp/Q2.xlsx", "Just now"),
            2,
        );
        state.record_recent_file(
            PersistedRecentFile::new("Q3.xlsx", "/tmp/Q3.xlsx", "Just now"),
            2,
        );
        store.save_state(&state).expect("save state");

        // Ensure temporary file was cleaned up by atomic rename.
        assert!(!dir.path().join("settings.toml.tmp").exists());
        assert!(!dir.path().join("state.toml.tmp").exists());

        // Simulate restart with a brand-new SettingsStore pointing at the same directory.
        let reloaded_store = SettingsStore::with_dir(dir.path());
        let loaded_settings = reloaded_store.load_settings().expect("reload settings");
        let loaded_state = reloaded_store.load_state().expect("reload state");

        assert_eq!(loaded_settings, settings);
        assert_eq!(loaded_state.window_bounds, state.window_bounds);
        assert!(loaded_state.sidebar_collapsed);
        assert!(loaded_state.inspector_open);
        assert_eq!(loaded_state.recent_files.len(), 2);
        assert_eq!(loaded_state.recent_files[0].title, "Q3.xlsx");
        assert_eq!(loaded_state.recent_files[1].title, "Q2.xlsx");
    }
}
