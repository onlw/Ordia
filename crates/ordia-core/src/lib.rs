//! Core foundational types, document abstractions, error definitions, and operation journals for Ordia.

/// Atomic TOML settings, recent files, and window geometry persistence (`M1-05`).
pub mod settings;

pub use settings::{
    AdvancedSettings, AppSettings, AppStateSnapshot, AppearanceMode, AppearanceSettings,
    EditingSettings, EnterMoveDirection, GeneralSettings, LanguageSetting, PersistedRecentFile,
    PersistedWindowBounds, SettingsError, SettingsStore,
};
