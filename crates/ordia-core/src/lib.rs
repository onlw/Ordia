//! Core foundational types, document abstractions, error definitions, and operation journals for Ordia.

/// Rolling file logging (`tracing-appender`) and zero-PII panic crash reporting (`M1-09`, `T-005`).
pub mod diagnostics;
/// Core document trait, format detection, progress snapshots, and atomic file writer (`M1-06`).
pub mod document;
/// Append-only CRC32-framed operation journal and startup crash-recovery manager (`M1-07`).
pub mod journal;
/// Atomic TOML settings, recent files, and window geometry persistence (`M1-05`).
pub mod settings;
/// Point-in-time document version history snapshot store with retention pruning (`M1-07`).
pub mod version_history;

pub use diagnostics::{
    CrashReport, DiagnosticsError, RollingLogGuard, acknowledge_crash_reports, crash_dir_for,
    init_rolling_file_logger, install_panic_crash_hook, list_pending_crash_reports, log_dir_for,
};
pub use document::{
    Document, DocumentError, DocumentKind, LoadProgress, LoadStage, atomic_write_bytes,
    detect_document_format, detect_document_format_from_file,
};
pub use journal::{
    JournalError, JournalFlushPolicy, JournalRecoveryManager, JournalSessionMeta, JournalWriter,
    RecoveredJournal, UncleanSessionInfo, read_journal_file,
};
pub use settings::{
    AdvancedSettings, AppSettings, AppStateSnapshot, AppearanceMode, AppearanceSettings,
    EditingSettings, EnterMoveDirection, GeneralSettings, LanguageSetting, PersistedRecentFile,
    PersistedWindowBounds, SettingsError, SettingsStore,
};
pub use version_history::{SnapshotRetentionPolicy, VersionHistoryStore, VersionSnapshotMeta};
