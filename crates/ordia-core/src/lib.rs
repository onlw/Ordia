//! Core foundational types, document abstractions, error definitions, and operation journals for Ordia.

/// Core document trait, format detection, progress snapshots, and atomic file writer (`M1-06`).
pub mod document;
/// Atomic TOML settings, recent files, and window geometry persistence (`M1-05`).
pub mod settings;

pub use document::{
    Document, DocumentError, DocumentKind, LoadProgress, LoadStage, atomic_write_bytes,
    detect_document_format, detect_document_format_from_file,
};
pub use settings::{
    AdvancedSettings, AppSettings, AppStateSnapshot, AppearanceMode, AppearanceSettings,
    EditingSettings, EnterMoveDirection, GeneralSettings, LanguageSetting, PersistedRecentFile,
    PersistedWindowBounds, SettingsError, SettingsStore,
};
