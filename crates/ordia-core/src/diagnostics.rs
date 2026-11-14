//! Rolling file logging (`tracing-appender`) and privacy-preserving panic crash reporting (`M1-09`, `T-005`).
//!
//! Per `DECISIONS.md` `T-005`, Ordia collects **zero** telemetry and never uploads crash reports
//! automatically. When a panic occurs, [`install_panic_crash_hook`] writes a strictly sanitized
//! metadata TOML file (version, OS, CPU arch, relative crate source file/line/col, and path-scrubbed
//! symbol frames) omitting panic message payloads, user paths, and document/cell contents.

use std::backtrace::Backtrace;
use std::fs;
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt as _, util::SubscriberInitExt as _};

use crate::document::atomic_write_bytes;

/// Errors raised while initializing logging or reading crash diagnostics.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum DiagnosticsError {
    /// Filesystem I/O failure.
    #[error("diagnostics I/O error at `{}`: {source}", path.display())]
    Io {
        /// Target path.
        path: PathBuf,
        /// OS error.
        #[source]
        source: std::io::Error,
    },
    /// Serialization error.
    #[error("diagnostics serialization error: {0}")]
    Serialization(String),
}

/// Guard keeping the non-blocking background rolling log writer alive for the lifetime of `main`.
pub struct RollingLogGuard {
    _worker_guard: WorkerGuard,
    log_dir: PathBuf,
}

impl RollingLogGuard {
    /// Directory where rolling `ordia.log.*` files are stored.
    #[must_use]
    pub fn log_dir(&self) -> &Path {
        &self.log_dir
    }
}

/// Returns the `<data_dir>/logs` directory path.
#[must_use]
pub fn log_dir_for(data_dir: &Path) -> PathBuf {
    data_dir.join("logs")
}

/// Returns the `<data_dir>/crashes` directory path.
#[must_use]
pub fn crash_dir_for(data_dir: &Path) -> PathBuf {
    data_dir.join("crashes")
}

/// Initializes daily rolling file logging under `<data_dir>/logs/ordia.log.YYYY-MM-DD`
/// and prunes rotated log files beyond `max_retained_files`.
pub fn init_rolling_file_logger(
    data_dir: &Path,
    max_retained_files: usize,
) -> Result<RollingLogGuard, DiagnosticsError> {
    let log_dir = log_dir_for(data_dir);
    fs::create_dir_all(&log_dir).map_err(|source| DiagnosticsError::Io {
        path: log_dir.clone(),
        source,
    })?;
    prune_old_log_files(&log_dir, max_retained_files.max(1));

    let file_appender = tracing_appender::rolling::daily(&log_dir, "ordia.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let env_filter =
        EnvFilter::try_from_env("ORDIA_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let file_layer = fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_writer(non_blocking);

    // `try_init` succeeds once per process; subsequent calls in tests still return a valid guard.
    let _ = tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .try_init();

    Ok(RollingLogGuard {
        _worker_guard: guard,
        log_dir,
    })
}

fn prune_old_log_files(log_dir: &Path, keep: usize) {
    let Ok(entries) = fs::read_dir(log_dir) else {
        return;
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|n| n.starts_with("ordia.log"))
        })
        .collect();
    files.sort();
    while files.len() > keep {
        let oldest = files.remove(0);
        let _ = fs::remove_file(oldest);
    }
}

/// Strictly sanitized, zero-PII crash metadata persisted on panic (`T-005`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CrashReport {
    /// Unique crash identifier (`crash-<unix_ms>-<seq>`).
    pub report_id: String,
    /// ISO-8601 UTC timestamp.
    pub created_at_utc: String,
    /// Ordia application version.
    pub ordia_version: String,
    /// Host operating system (`"macos"`, `"windows"`, `"linux"`).
    pub os: String,
    /// Host CPU architecture (`"aarch64"`, `"x86_64"`).
    pub arch: String,
    /// Sanitized relative source location (`crates/.../file.rs:line:col`).
    pub location: String,
    /// Generic thread category (`"main"` or `"worker"`).
    pub thread_role: String,
    /// Path-scrubbed symbol summary (never includes panic payload strings or user file paths).
    pub sanitized_backtrace: String,
}

static CRASH_DATA_DIR: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
static HOOK_INSTALLED: OnceLock<()> = OnceLock::new();
static CRASH_SEQ: AtomicU64 = AtomicU64::new(1);

impl CrashReport {
    /// Builds a sanitized [`CrashReport`] from a [`PanicHookInfo`], intentionally discarding
    /// `info.payload()` and scrubbing all absolute filesystem paths (`T-005`).
    #[must_use]
    pub fn from_panic_info(info: &PanicHookInfo<'_>) -> Self {
        let now = Utc::now();
        let seq = CRASH_SEQ.fetch_add(1, Ordering::Relaxed);
        let report_id = format!("crash-{}-{seq}", now.timestamp_millis());
        let location = match info.location() {
            Some(loc) => format!(
                "{}:{}:{}",
                sanitize_source_path(loc.file()),
                loc.line(),
                loc.column()
            ),
            None => "<unknown>:0:0".to_string(),
        };
        let is_main = std::thread::current().name().is_some_and(|n| n == "main");
        let thread_role = if is_main {
            "main".to_string()
        } else {
            "worker".to_string()
        };
        let bt_raw = Backtrace::force_capture().to_string();
        let sanitized_backtrace = sanitize_backtrace_text(&bt_raw);

        Self {
            report_id,
            created_at_utc: now.to_rfc3339(),
            ordia_version: env!("CARGO_PKG_VERSION").to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            location,
            thread_role,
            sanitized_backtrace,
        }
    }

    /// Pre-fills a GitHub Issue creation URL containing only sanitized version, OS, arch, and source location.
    #[must_use]
    pub fn github_issue_url(&self) -> String {
        let title = url_encode(&format!(
            "crash: panic at {} (v{}, {}-{})",
            self.location, self.ordia_version, self.os, self.arch
        ));
        let body = url_encode(&format!(
            "### Crash Metadata (Sanitized Local Report)\n- **Version**: `{}`\n- **OS / Arch**: `{}` / `{}`\n- **Location**: `{}`\n- **Thread**: `{}`\n",
            self.ordia_version, self.os, self.arch, self.location, self.thread_role
        ));
        format!("https://github.com/onlw/Ordia/issues/new?title={title}&body={body}")
    }
}

/// Installs (or updates the output directory of) Ordia's privacy-preserving panic hook (`T-005`).
pub fn install_panic_crash_hook(data_dir: impl Into<PathBuf>) {
    let slot = CRASH_DATA_DIR.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = slot.lock() {
        *guard = Some(data_dir.into());
    }
    HOOK_INSTALLED.get_or_init(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if let Some(lock) = CRASH_DATA_DIR.get()
                && let Ok(guard) = lock.lock()
                && let Some(ref base_dir) = *guard
            {
                let report = CrashReport::from_panic_info(info);
                let out = crash_dir_for(base_dir).join(format!("{}.toml", report.report_id));
                if let Ok(encoded) = toml::to_string_pretty(&report) {
                    let _ = atomic_write_bytes(&out, encoded.as_bytes(), |_| Ok(()));
                }
            }
            prev(info);
        }));
    });
}

/// Lists unacknowledged crash reports stored under `<data_dir>/crashes/`.
#[must_use]
pub fn list_pending_crash_reports(data_dir: &Path) -> Vec<(PathBuf, CrashReport)> {
    let dir = crash_dir_for(data_dir);
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        if let Ok(raw) = fs::read_to_string(&p)
            && let Ok(rep) = toml::from_str::<CrashReport>(&raw)
        {
            out.push((p, rep));
        }
    }
    out.sort_by(|a, b| a.1.report_id.cmp(&b.1.report_id));
    out
}

/// Deletes acknowledged crash report files under `<data_dir>/crashes/`.
pub fn acknowledge_crash_reports(data_dir: &Path) {
    for (path, _) in list_pending_crash_reports(data_dir) {
        let _ = fs::remove_file(path);
    }
}

fn sanitize_source_path(raw: &str) -> String {
    let norm = raw.replace('\\', "/");
    for marker in ["crates/", "xtask/"] {
        if let Some(idx) = norm.find(marker) {
            return norm[idx..].to_string();
        }
    }
    norm.rsplit('/').next().unwrap_or("<source>").to_string()
}

fn sanitize_backtrace_text(raw: &str) -> String {
    raw.lines()
        .map(str::trim)
        .filter(|l| {
            !l.starts_with("at ")
                && !l.contains("/Users/")
                && !l.contains("\\Users\\")
                && !l.contains("/home/")
        })
        .take(24)
        .collect::<Vec<_>>()
        .join("\n")
}

fn url_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triggered_panic_writes_crash_file_with_zero_document_or_path_data() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _log_guard = init_rolling_file_logger(dir.path(), 5).expect("init rolling logger");
        tracing::info!("diagnostics logger smoke event");

        install_panic_crash_hook(dir.path());

        let secret_cell = "CONFIDENTIAL_SALARY_CELL_987654321";
        let secret_doc_path = "/Users/alice_private/Payroll_2026_Secret.xlsx";

        let join_res = std::thread::spawn(move || {
            panic!("simulated failure on cell={secret_cell} file={secret_doc_path}");
        })
        .join();
        assert!(join_res.is_err(), "worker thread should have panicked");

        let pending = list_pending_crash_reports(dir.path());
        assert_eq!(pending.len(), 1);
        let (crash_path, report) = &pending[0];
        let raw_on_disk = fs::read_to_string(crash_path).expect("read crash file");

        assert_eq!(report.ordia_version, env!("CARGO_PKG_VERSION"));
        assert!(report.location.contains("diagnostics.rs:"));
        // Strict T-005 privacy assertions: zero document content, zero panic payload, zero user paths!
        assert!(!raw_on_disk.contains("CONFIDENTIAL_SALARY_CELL_987654321"));
        assert!(!raw_on_disk.contains("Payroll_2026_Secret.xlsx"));
        assert!(!raw_on_disk.contains("alice_private"));
        assert!(
            report
                .github_issue_url()
                .starts_with("https://github.com/onlw/Ordia/issues/new?")
        );

        acknowledge_crash_reports(dir.path());
        assert!(list_pending_crash_reports(dir.path()).is_empty());
    }
}
