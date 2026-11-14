//! OOXML (`.xlsx` / `.xlsm`) reader, atomic writer, and unknown-parts byte preservation layer.

mod error;
pub mod fixtures;
mod merge;
mod preserve;
mod warnings;

pub use error::XlsxError;
pub use merge::{atomic_write_xlsx, merge_engine_with_preserved};
pub use preserve::{
    ChartSourceRef, PivotCacheEntry, PreservedPackage, PreservedPart, RawRelationship,
    SheetAttachments, is_engine_managed_part, parse_relationships,
};
pub use warnings::{PreservationHazardKind, PreservationWarning, check_preservation_warnings};

use ordia_sheet_engine::{IronCalcEngine, SheetEngine};
use std::path::Path;

/// High-level `.xlsx` / `.xlsm` package pairing a [`SheetEngine`] with its [`PreservedPackage`].
///
/// Implements Ordia's `"不认识也不破坏"` invariant: unrecognized parts (PivotTables, VBA `vbaProject.bin`,
/// slicers, complex charts, custom XML) are retained byte-for-byte across open -> edit -> save cycles.
pub struct XlsxWorkbook {
    engine: Box<dyn SheetEngine>,
    preserved: PreservedPackage,
}

impl XlsxWorkbook {
    /// Opens an `.xlsx` or `.xlsm` file from disk, parsing known parts into [`IronCalcEngine`]
    /// and capturing all unrecognized parts byte-for-byte.
    pub fn open_path(path: impl AsRef<Path>) -> Result<Self, XlsxError> {
        let path_ref = path.as_ref();
        let bytes = std::fs::read(path_ref)?;
        let name = path_ref
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Workbook");
        Self::from_bytes(&bytes, name)
    }

    /// Loads an `.xlsx` or `.xlsm` archive from in-memory bytes.
    pub fn from_bytes(bytes: &[u8], workbook_name: &str) -> Result<Self, XlsxError> {
        let preserved = PreservedPackage::from_zip_bytes(bytes)?;
        let engine = IronCalcEngine::from_xlsx_bytes(bytes, workbook_name)?;
        Ok(Self {
            engine: Box::new(engine),
            preserved,
        })
    }

    /// Shared reference to the underlying [`SheetEngine`].
    #[must_use]
    pub fn engine(&self) -> &dyn SheetEngine {
        self.engine.as_ref()
    }

    /// Mutable reference to the underlying [`SheetEngine`].
    pub fn engine_mut(&mut self) -> &mut dyn SheetEngine {
        self.engine.as_mut()
    }

    /// Consumes this wrapper and returns `(engine, preserved_package)`.
    #[must_use]
    pub fn into_parts(self) -> (Box<dyn SheetEngine>, PreservedPackage) {
        (self.engine, self.preserved)
    }

    /// Reference to the captured [`PreservedPackage`] metadata and raw part bytes.
    #[must_use]
    pub fn preserved(&self) -> &PreservedPackage {
        &self.preserved
    }

    /// Evaluates whether user edits have invalidated any preserved PivotTables, Charts, or Slicers.
    #[must_use]
    pub fn check_warnings(&self) -> Vec<PreservationWarning> {
        check_preservation_warnings(self.engine.as_ref(), &self.preserved)
    }

    /// Serializes the merged `.xlsx` / `.xlsm` package in memory, preserving unknown parts byte-for-byte.
    pub fn to_bytes(&mut self) -> Result<Vec<u8>, XlsxError> {
        let engine_bytes = self.engine.to_xlsx_bytes()?;
        merge_engine_with_preserved(&engine_bytes, &self.preserved)
    }

    /// Atomically writes the merged `.xlsx` / `.xlsm` package to `path`.
    pub fn save_path(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<Vec<PreservationWarning>, XlsxError> {
        let warnings = self.check_warnings();
        let merged_bytes = self.to_bytes()?;
        atomic_write_xlsx(path.as_ref(), &merged_bytes)?;
        Ok(warnings)
    }
}
