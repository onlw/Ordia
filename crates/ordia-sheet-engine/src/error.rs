//! Error types for the Ordia spreadsheet engine abstraction.

use thiserror::Error;

/// Errors returned by [`crate::SheetEngine`] operations.
#[derive(Debug, Error)]
pub enum EngineError {
    /// Underlying filesystem I/O failure while reading or atomically writing a workbook.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Malformed `.xlsx` ZIP package or OOXML structure.
    #[error("workbook format error: {0}")]
    Format(String),

    /// Worksheet index or name does not exist in the workbook.
    #[error("invalid worksheet reference: {0}")]
    InvalidSheet(String),

    /// Cell coordinate or range exceeds Excel's `(1_048_576, 16_384)` grid bounds.
    #[error("invalid cell coordinate: {0}")]
    InvalidCoordinate(String),

    /// Formula parsing, defined-name validation, or calculation error.
    #[error("formula error: {0}")]
    Formula(String),

    /// Structural or style mutation rejected by engine invariants (e.g. overlapping merge).
    #[error("workbook mutation error: {0}")]
    Mutation(String),
}
