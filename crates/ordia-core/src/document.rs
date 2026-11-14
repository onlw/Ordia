//! Core document abstraction, format detection (extension + magic bytes), progress reporting,
//! and atomic file saving (`M1-06`).

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Magic signature of a ZIP local file header (`PK\x03\x04`, used by OOXML `.xlsx` / `.xlsm`).
const ZIP_LOCAL_HEADER_MAGIC: &[u8] = b"PK\x03\x04";
/// Magic signature of an empty ZIP end-of-central-directory record (`PK\x05\x06`).
const ZIP_EMPTY_ARCHIVE_MAGIC: &[u8] = b"PK\x05\x06";
/// Magic signature of a legacy Microsoft Compound File Binary Format (`.xls` BIFF8, `D0 CF 11 E0 A1 B1 1A E1`).
const OLE2_COMPOUND_MAGIC: &[u8] = &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// Supported logical document formats recognized by Ordia.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DocumentKind {
    /// OpenXML Spreadsheet workbook (`.xlsx`).
    SpreadsheetXlsx,
    /// OpenXML Macro-Enabled Spreadsheet workbook (`.xlsm`; VBA binary preserved verbatim).
    SpreadsheetXlsm,
    /// Comma-separated values text table (`.csv`).
    SpreadsheetCsv,
    /// Tab-separated values text table (`.tsv`).
    SpreadsheetTsv,
}

impl DocumentKind {
    /// Canonical lowercase file extension (without leading dot).
    #[must_use]
    pub const fn canonical_extension(self) -> &'static str {
        match self {
            Self::SpreadsheetXlsx => "xlsx",
            Self::SpreadsheetXlsm => "xlsm",
            Self::SpreadsheetCsv => "csv",
            Self::SpreadsheetTsv => "tsv",
        }
    }
}

/// Phase of an asynchronous or synchronous document load pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum LoadStage {
    /// Probing file existence, size, extension, and header magic bytes.
    DetectingFormat,
    /// Streaming or reading raw bytes from storage.
    ReadingBytes,
    /// Decompressing OOXML parts or parsing delimited rows into the engine.
    ParsingWorkbook,
    /// Workbook model is ready for layout and rendering.
    Ready,
}

/// Progress snapshot emitted while loading a document from disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct LoadProgress {
    /// Current pipeline stage.
    pub stage: LoadStage,
    /// Bytes processed so far.
    pub bytes_processed: u64,
    /// Total file size in bytes, if known.
    pub total_bytes: Option<u64>,
}

impl LoadProgress {
    /// Constructs a new [`LoadProgress`] snapshot.
    #[must_use]
    pub const fn new(stage: LoadStage, bytes_processed: u64, total_bytes: Option<u64>) -> Self {
        Self {
            stage,
            bytes_processed,
            total_bytes,
        }
    }

    /// Estimated completion percentage (`0..=100`).
    #[must_use]
    pub fn percent(self) -> u8 {
        match self.stage {
            LoadStage::DetectingFormat => 5,
            LoadStage::ReadingBytes => match self.total_bytes {
                Some(total) if total > 0 => {
                    let ratio = (self.bytes_processed.min(total) * 50) / total;
                    (10 + ratio as u8).min(60)
                }
                _ => 30,
            },
            LoadStage::ParsingWorkbook => 80,
            LoadStage::Ready => 100,
        }
    }
}

/// Structured, user-presentable errors raised during format detection, loading, or atomic saving.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum DocumentError {
    /// Filesystem I/O failure (missing file, permission denied, disk full).
    #[error("I/O error on `{}`: {source}", path.display())]
    Io {
        /// Target file path.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// Zero-byte empty file that cannot form a valid OOXML archive or table.
    #[error("file `{}` is empty (0 bytes)", path.display())]
    EmptyFile {
        /// Target file path.
        path: PathBuf,
    },
    /// Truncated, damaged, or non-ZIP data inside an `.xlsx`/`.xlsm` container.
    #[error("corrupt or unreadable workbook `{}`: {reason}", path.display())]
    CorruptArchive {
        /// Target file path.
        path: PathBuf,
        /// Technical diagnostic reason.
        reason: String,
    },
    /// Legacy binary Excel 97-2003 `.xls` (OLE2 Compound Document; slated for 1.x import).
    #[error(
        "legacy binary Excel `.xls` format (`{}`) is not yet supported; please save as `.xlsx` or `.csv`",
        path.display()
    )]
    LegacyBinaryXls {
        /// Target file path.
        path: PathBuf,
    },
    /// Unrecognized binary magic signature or unsupported file extension.
    #[error("unsupported file format for `{}` ({detail})", path.display())]
    UnsupportedFormat {
        /// Target file path.
        path: PathBuf,
        /// Diagnostic detail (extension or hex magic).
        detail: String,
    },
    /// Workbook XML or CSV structure failed validation.
    #[error("failed to parse document `{}`: {detail}", path.display())]
    ParseError {
        /// Target file path.
        path: PathBuf,
        /// Diagnostic detail.
        detail: String,
    },
    /// Atomic save or pre-replace validation failed.
    #[error("failed to save document `{}`: {detail}", path.display())]
    SaveFailed {
        /// Target file path.
        path: PathBuf,
        /// Diagnostic detail.
        detail: String,
    },
}

impl DocumentError {
    /// Path associated with this document error.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::Io { path, .. }
            | Self::EmptyFile { path }
            | Self::CorruptArchive { path, .. }
            | Self::LegacyBinaryXls { path }
            | Self::UnsupportedFormat { path, .. }
            | Self::ParseError { path, .. }
            | Self::SaveFailed { path, .. } => path,
        }
    }
}

/// Suite-wide kernel document abstraction (`M1-06`, `PLAN.md` §3.1).
///
/// Shared across Ordia Spreadsheets (`SheetDocument`), and in future stages Ordia Writer and Slides.
pub trait Document: Send {
    /// Returns the detected [`DocumentKind`].
    fn document_kind(&self) -> DocumentKind;

    /// Display title shown in workbook tabs and window chrome.
    fn document_title(&self) -> &str;

    /// Source or saved filesystem path, if backed by a file on disk.
    fn document_path(&self) -> Option<&Path>;

    /// Returns `true` if unsaved transactions have modified the document since open/save.
    fn is_modified(&self) -> bool;

    /// Loads a document from `path`, invoking `on_progress` across stages without panicking on corrupt inputs.
    fn open_from_path(
        path: &Path,
        on_progress: &mut dyn FnMut(LoadProgress),
    ) -> Result<Self, DocumentError>
    where
        Self: Sized;

    /// Atomically writes the document to `path` (temp file -> `sync_all` -> validate -> `rename`).
    fn save_to_path(&mut self, path: &Path) -> Result<(), DocumentError>;
}

/// Detects [`DocumentKind`] using both the file extension of `path` and initial `header_bytes`.
///
/// Ensures renamed or mislabeled files (e.g. a legacy `.xls` OLE2 binary renamed to `.xlsx`,
/// or a non-ZIP HTML/JSON error page saved with an `.xlsx` extension) are caught immediately
/// with a specific, friendly [`DocumentError`] rather than cryptic downstream parser failures.
pub fn detect_document_format(
    path: &Path,
    header_bytes: &[u8],
) -> Result<DocumentKind, DocumentError> {
    if header_bytes.is_empty() {
        return Err(DocumentError::EmptyFile {
            path: path.to_path_buf(),
        });
    }

    if header_bytes.starts_with(OLE2_COMPOUND_MAGIC) {
        return Err(DocumentError::LegacyBinaryXls {
            path: path.to_path_buf(),
        });
    }

    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase);

    let has_zip_magic = header_bytes.starts_with(ZIP_LOCAL_HEADER_MAGIC)
        || header_bytes.starts_with(ZIP_EMPTY_ARCHIVE_MAGIC);

    match ext.as_deref() {
        Some("xlsx") => {
            if has_zip_magic {
                Ok(DocumentKind::SpreadsheetXlsx)
            } else {
                Err(DocumentError::CorruptArchive {
                    path: path.to_path_buf(),
                    reason: "file does not begin with a valid ZIP/OOXML signature (PK\\x03\\x04)"
                        .to_string(),
                })
            }
        }
        Some("xlsm") => {
            if has_zip_magic {
                Ok(DocumentKind::SpreadsheetXlsm)
            } else {
                Err(DocumentError::CorruptArchive {
                    path: path.to_path_buf(),
                    reason: "file does not begin with a valid ZIP/OOXML signature (PK\\x03\\x04)"
                        .to_string(),
                })
            }
        }
        Some("xls") => Err(DocumentError::LegacyBinaryXls {
            path: path.to_path_buf(),
        }),
        Some("csv") => {
            if looks_like_text_or_utf16(header_bytes) {
                Ok(DocumentKind::SpreadsheetCsv)
            } else {
                Err(DocumentError::CorruptArchive {
                    path: path.to_path_buf(),
                    reason: "binary NUL bytes found in CSV without UTF-16 BOM".to_string(),
                })
            }
        }
        Some("tsv") => {
            if looks_like_text_or_utf16(header_bytes) {
                Ok(DocumentKind::SpreadsheetTsv)
            } else {
                Err(DocumentError::CorruptArchive {
                    path: path.to_path_buf(),
                    reason: "binary NUL bytes found in TSV without UTF-16 BOM".to_string(),
                })
            }
        }
        _ => {
            if has_zip_magic {
                Ok(DocumentKind::SpreadsheetXlsx)
            } else if looks_like_text_or_utf16(header_bytes)
                && (header_bytes.contains(&b',') || header_bytes.contains(&b'\t'))
            {
                let tabs = header_bytes.iter().filter(|&&b| b == b'\t').count();
                let commas = header_bytes.iter().filter(|&&b| b == b',').count();
                if tabs > commas {
                    Ok(DocumentKind::SpreadsheetTsv)
                } else {
                    Ok(DocumentKind::SpreadsheetCsv)
                }
            } else {
                let prefix_hex: String = header_bytes
                    .iter()
                    .take(8)
                    .map(|b| format!("{b:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                Err(DocumentError::UnsupportedFormat {
                    path: path.to_path_buf(),
                    detail: format!(
                        "extension={:?}, magic=[{prefix_hex}]",
                        ext.unwrap_or_default()
                    ),
                })
            }
        }
    }
}

/// Reads up to 512 header bytes from `path` on disk and detects its [`DocumentKind`].
pub fn detect_document_format_from_file(path: &Path) -> Result<DocumentKind, DocumentError> {
    let mut file = File::open(path).map_err(|source| DocumentError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut buf = [0u8; 512];
    let n = file.read(&mut buf).map_err(|source| DocumentError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    detect_document_format(path, &buf[..n])
}

fn looks_like_text_or_utf16(header_bytes: &[u8]) -> bool {
    let has_utf16_bom =
        header_bytes.starts_with(&[0xFF, 0xFE]) || header_bytes.starts_with(&[0xFE, 0xFF]);
    if has_utf16_bom {
        return true;
    }
    !header_bytes.contains(&0x00)
}

/// Atomically writes `bytes` to `path` (`AGENTS.md` §4.5):
/// writes a sibling temporary file -> `sync_all()` -> runs `verify(&tmp_path)` -> `fs::rename`.
pub fn atomic_write_bytes(
    path: &Path,
    bytes: &[u8],
    verify: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), DocumentError> {
    static SAVE_SEQ: AtomicU64 = AtomicU64::new(1);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| DocumentError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let seq = SAVE_SEQ.fetch_add(1, Ordering::Relaxed);
    let stem = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("document");
    let tmp_path = parent.join(format!(".{stem}.ordia-{}-{seq}.tmp", std::process::id()));

    let write_res = (|| -> Result<(), std::io::Error> {
        let mut f = File::create(&tmp_path)?;
        f.write_all(bytes)?;
        f.sync_all()
    })();
    if let Err(source) = write_res {
        let _ = fs::remove_file(&tmp_path);
        return Err(DocumentError::Io {
            path: tmp_path,
            source,
        });
    }

    if let Err(detail) = verify(&tmp_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(DocumentError::SaveFailed {
            path: path.to_path_buf(),
            detail,
        });
    }

    if let Err(source) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(DocumentError::Io {
            path: path.to_path_buf(),
            source,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_ooxml_csv_tsv_and_rejects_corrupt_or_legacy_xls() {
        assert_eq!(
            detect_document_format(Path::new("budget.xlsx"), b"PK\x03\x04\x14\x00\x06\x00")
                .expect("valid xlsx"),
            DocumentKind::SpreadsheetXlsx
        );
        assert_eq!(
            detect_document_format(Path::new("macros.XLSM"), b"PK\x03\x04\x14\x00\x06\x00")
                .expect("valid xlsm"),
            DocumentKind::SpreadsheetXlsm
        );
        assert_eq!(
            detect_document_format(Path::new("sales.csv"), b"id,name,amount\n1,Alice,42\n")
                .expect("valid utf8 csv"),
            DocumentKind::SpreadsheetCsv
        );
        assert_eq!(
            detect_document_format(
                Path::new("utf16.csv"),
                &[0xFF, 0xFE, b'a', 0x00, b',', 0x00]
            )
            .expect("valid utf16-le csv"),
            DocumentKind::SpreadsheetCsv
        );
        assert_eq!(
            detect_document_format(Path::new("table.tsv"), b"col1\tcol2\n10\t20\n")
                .expect("valid tsv"),
            DocumentKind::SpreadsheetTsv
        );

        // Renamed OLE2 binary `.xls` pretending to be `.xlsx` -> LegacyBinaryXls
        let ole2 = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0x00];
        assert!(matches!(
            detect_document_format(Path::new("fake.xlsx"), &ole2),
            Err(DocumentError::LegacyBinaryXls { .. })
        ));

        // HTML/garbage saved with `.xlsx` extension -> CorruptArchive
        assert!(matches!(
            detect_document_format(Path::new("broken.xlsx"), b"<html>404 Not Found</html>"),
            Err(DocumentError::CorruptArchive { .. })
        ));

        // Empty 0-byte file -> EmptyFile
        assert!(matches!(
            detect_document_format(Path::new("empty.xlsx"), &[]),
            Err(DocumentError::EmptyFile { .. })
        ));

        // Binary NUL garbage saved with `.csv` extension -> CorruptArchive
        assert!(matches!(
            detect_document_format(Path::new("bad.csv"), &[0x89, b'P', b'N', b'G', 0x00, 0x0A]),
            Err(DocumentError::CorruptArchive { .. })
        ));
    }

    #[test]
    fn atomic_write_preserves_existing_file_when_verification_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("important.xlsx");
        atomic_write_bytes(&target, b"PK\x03\x04ORIGINAL", |_| Ok(())).expect("initial save");

        let failed = atomic_write_bytes(&target, b"CORRUPT", |_| {
            Err("simulated pre-save verification failure".to_string())
        });
        assert!(matches!(failed, Err(DocumentError::SaveFailed { .. })));

        let on_disk = fs::read(&target).expect("read original");
        assert_eq!(on_disk, b"PK\x03\x04ORIGINAL");
    }
}
