//! Error types for `.xlsx` / `.xlsm` package reading, preservation, and atomic writing.

use ordia_sheet_engine::EngineError;
use thiserror::Error;

/// Errors returned by `ordia-xlsx` operations.
#[derive(Debug, Error)]
pub enum XlsxError {
    /// Filesystem I/O failure.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// ZIP container read/write failure.
    #[error("ZIP archive error: {0}")]
    Zip(#[from] zip::result::ZipError),

    /// XML parsing or UTF-8 decoding failure inside an OOXML part.
    #[error("OOXML XML error: {0}")]
    Xml(String),

    /// Underlying calculation engine error.
    #[error("Spreadsheet engine error: {0}")]
    Engine(#[from] EngineError),

    /// Output archive integrity validation failure prior to atomic rename.
    #[error("Archive integrity validation failed: {0}")]
    Integrity(String),
}
