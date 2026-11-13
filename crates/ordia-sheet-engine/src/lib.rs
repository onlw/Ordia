//! Spreadsheet formula and calculation engine abstraction for Ordia.
//!
//! This crate isolates the formula calculation engine (`IronCalc` `=0.9.1`) behind Ordia's own
//! [`SheetEngine`] trait and domain types (`CellCoord`, `CellRange`, `CellValue`, `CellStyle`,
//! `FreezePane`, `SheetInfo`, etc.) so no upstream `ironcalc` or `ironcalc_base` types ever
//! appear in public signatures (`AGENTS.md` §4.2, `D-004`).

mod error;
mod ironcalc_impl;
mod traits;
mod types;

pub use error::EngineError;
pub use ironcalc_impl::IronCalcEngine;
pub use traits::SheetEngine;
pub use types::{
    BorderEdge, BorderLineStyle, CellBorders, CellComment, CellCoord, CellHyperlink, CellRange,
    CellStyle, CellValue, ColSpanMetric, ColorRgba, DEFAULT_COL_WIDTH_PX, DEFAULT_ROW_HEIGHT_PX,
    DefinedNameInfo, FontStyle, FormulaError, FreezePane, HorizontalAlign, MAX_COLS, MAX_ROWS,
    RowMetric, SheetInfo, SheetVisibility, VerticalAlign, col_to_letters, letters_to_col_1based,
};
