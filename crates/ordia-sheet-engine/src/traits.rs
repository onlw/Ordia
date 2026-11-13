//! Trait boundary decoupling Ordia's spreadsheet model, layout, and UI from the calculation engine.

use crate::error::EngineError;
use crate::types::{
    CellComment, CellCoord, CellHyperlink, CellRange, CellStyle, CellValue, ColSpanMetric,
    ColorRgba, DefinedNameInfo, FreezePane, RowMetric, SheetInfo, SheetVisibility,
};
use std::path::Path;

/// Engine-agnostic interface for workbook storage, formula calculation, styling, and `.xlsx` I/O.
///
///Per `AGENTS.md` §4.2 (`D-004`), no IronCalc types may appear anywhere in this trait or in
/// `ordia-sheet-engine`'s public module hierarchy.
pub trait SheetEngine: Send {
    /// Returns metadata for all worksheets in tab order.
    fn sheets(&self) -> Vec<SheetInfo>;

    /// Returns the number of worksheets in the workbook.
    fn sheet_count(&self) -> usize;

    /// Finds a worksheet's zero-based tab index by case-insensitive name.
    fn sheet_index_by_name(&self, name: &str) -> Option<u32>;

    /// Appends a new worksheet (with `name` or an auto-generated unique name) and returns its index.
    fn add_sheet(&mut self, name: Option<&str>) -> Result<u32, EngineError>;

    /// Renames the worksheet at `sheet` (rewriting cross-sheet formula references).
    fn rename_sheet(&mut self, sheet: u32, new_name: &str) -> Result<(), EngineError>;

    /// Deletes the worksheet at `sheet` (cannot delete the sole remaining sheet).
    fn delete_sheet(&mut self, sheet: u32) -> Result<(), EngineError>;

    /// Duplicates the worksheet at `sheet` and returns the new copy's index.
    fn duplicate_sheet(&mut self, sheet: u32) -> Result<u32, EngineError>;

    /// Moves the worksheet at `from_index` to `to_index`.
    fn move_sheet(&mut self, from_index: u32, to_index: u32) -> Result<(), EngineError>;

    /// Updates a worksheet's visibility state.
    fn set_sheet_visibility(
        &mut self,
        sheet: u32,
        visibility: SheetVisibility,
    ) -> Result<(), EngineError>;

    /// Sets or clears a worksheet's custom tab accent color.
    fn set_sheet_tab_color(
        &mut self,
        sheet: u32,
        color: Option<ColorRgba>,
    ) -> Result<(), EngineError>;

    /// Shows or hides worksheet gridlines.
    fn set_show_grid_lines(&mut self, sheet: u32, show: bool) -> Result<(), EngineError>;

    /// Reads the evaluated [`CellValue`] at `(sheet, coord)`.
    fn cell_value(&self, sheet: u32, coord: CellCoord) -> Result<CellValue, EngineError>;

    /// Reads the number-formatted display string at `(sheet, coord)`.
    fn cell_display_text(&self, sheet: u32, coord: CellCoord) -> Result<String, EngineError>;

    /// Reads the formula string (including leading `'='`) if `(sheet, coord)` holds a formula.
    fn cell_formula(&self, sheet: u32, coord: CellCoord) -> Result<Option<String>, EngineError>;

    /// Reads the raw editable input string for the formula bar / cell editor (formula if present,
    /// `'text` if quote-prefixed, or raw unformatted literal value).
    fn cell_input(&self, sheet: u32, coord: CellCoord) -> Result<String, EngineError>;

    /// Sets a cell's user input (`"=SUM(A1:A5)"`, `"42.5"`, `"TRUE"`, `"'001"`, or `""` to clear)
    /// and immediately recalculates dependent formulas.
    fn set_cell_input(
        &mut self,
        sheet: u32,
        coord: CellCoord,
        input: &str,
    ) -> Result<(), EngineError>;

    /// Sets a cell's user input without running workbook recalculation yet (for bulk imports/pastes;
    /// follow with [`SheetEngine::evaluate`]).
    fn set_cell_input_deferred(
        &mut self,
        sheet: u32,
        coord: CellCoord,
        input: &str,
    ) -> Result<(), EngineError>;

    /// Clears cell values/formulas inside `range` while preserving cell styles, then recalculates.
    fn clear_range_contents(&mut self, sheet: u32, range: CellRange) -> Result<(), EngineError>;

    /// Returns coordinates of all non-empty cells on `sheet` in row-major order.
    fn populated_cells(&self, sheet: u32) -> Result<Vec<CellCoord>, EngineError>;

    /// Evaluates all pending/dirty formulas and conditional formatting across the workbook.
    fn evaluate(&mut self);

    /// Resolves the effective [`CellStyle`] at `(sheet, coord)`.
    fn cell_style(&self, sheet: u32, coord: CellCoord) -> Result<CellStyle, EngineError>;

    /// Applies `style` to `(sheet, coord)`.
    fn set_cell_style(
        &mut self,
        sheet: u32,
        coord: CellCoord,
        style: &CellStyle,
    ) -> Result<(), EngineError>;

    /// Lists all merged cell ranges on `sheet`.
    fn merged_ranges(&self, sheet: u32) -> Result<Vec<CellRange>, EngineError>;

    /// Merges `range` on `sheet` (retaining anchor top-left content).
    fn merge_range(&mut self, sheet: u32, range: CellRange) -> Result<(), EngineError>;

    /// Unmerges any merged cells intersecting `range` on `sheet`.
    fn unmerge_range(&mut self, sheet: u32, range: CellRange) -> Result<(), EngineError>;

    /// Returns frozen row and column counts for `sheet`.
    fn freeze_pane(&self, sheet: u32) -> Result<FreezePane, EngineError>;

    /// Updates frozen row and column counts for `sheet`.
    fn set_freeze_pane(&mut self, sheet: u32, freeze: FreezePane) -> Result<(), EngineError>;

    /// Returns effective height of `row` in logical pixels (`0.0` if hidden).
    fn row_height(&self, sheet: u32, row: u32) -> Result<f32, EngineError>;

    /// Sets explicit height of `row` in logical pixels.
    fn set_row_height(&mut self, sheet: u32, row: u32, height_px: f32) -> Result<(), EngineError>;

    /// Returns `true` if `row` is hidden.
    fn is_row_hidden(&self, sheet: u32, row: u32) -> Result<bool, EngineError>;

    /// Hides or unhides `row`.
    fn set_row_hidden(&mut self, sheet: u32, row: u32, hidden: bool) -> Result<(), EngineError>;

    /// Returns sparse row height/visibility overrides on `sheet` in `O(custom_rows)` time.
    fn custom_row_metrics(&self, sheet: u32) -> Result<Vec<RowMetric>, EngineError>;

    /// Returns effective width of `col` in logical pixels (`0.0` if hidden).
    fn col_width(&self, sheet: u32, col: u32) -> Result<f32, EngineError>;

    /// Sets explicit width of `col` in logical pixels.
    fn set_col_width(&mut self, sheet: u32, col: u32, width_px: f32) -> Result<(), EngineError>;

    /// Returns `true` if `col` is hidden.
    fn is_col_hidden(&self, sheet: u32, col: u32) -> Result<bool, EngineError>;

    /// Hides or unhides `col`.
    fn set_col_hidden(&mut self, sheet: u32, col: u32, hidden: bool) -> Result<(), EngineError>;

    /// Returns sparse column span width/visibility overrides on `sheet` in `O(custom_spans)` time.
    fn custom_col_metrics(&self, sheet: u32) -> Result<Vec<ColSpanMetric>, EngineError>;

    /// Inserts `count` blank rows at `at_row`, shifting existing rows and rewriting formulas.
    fn insert_rows(&mut self, sheet: u32, at_row: u32, count: u32) -> Result<(), EngineError>;

    /// Deletes `count` rows starting at `at_row`, shifting subsequent rows and rewriting formulas.
    fn delete_rows(&mut self, sheet: u32, at_row: u32, count: u32) -> Result<(), EngineError>;

    /// Inserts `count` blank columns at `at_col`, shifting existing columns and rewriting formulas.
    fn insert_cols(&mut self, sheet: u32, at_col: u32, count: u32) -> Result<(), EngineError>;

    /// Deletes `count` columns starting at `at_col`, shifting subsequent columns and rewriting formulas.
    fn delete_cols(&mut self, sheet: u32, at_col: u32, count: u32) -> Result<(), EngineError>;

    /// Lists all workbook-global and sheet-local defined names.
    fn defined_names(&self) -> Vec<DefinedNameInfo>;

    /// Creates or updates a defined name (`formula` should start with `'='`, e.g. `"=Sheet1!$A$1:$A$10"`).
    fn set_defined_name(
        &mut self,
        name: &str,
        scope_sheet: Option<u32>,
        formula: &str,
    ) -> Result<(), EngineError>;

    /// Deletes a defined name.
    fn delete_defined_name(
        &mut self,
        name: &str,
        scope_sheet: Option<u32>,
    ) -> Result<(), EngineError>;

    /// Lists cell comments on `sheet`.
    fn comments(&self, sheet: u32) -> Result<Vec<CellComment>, EngineError>;

    /// Lists static and dynamic hyperlinks on `sheet`.
    fn hyperlinks(&self, sheet: u32) -> Result<Vec<CellHyperlink>, EngineError>;

    /// Serializes the workbook to `.xlsx` bytes in memory (evaluating any dirty formulas first).
    fn to_xlsx_bytes(&mut self) -> Result<Vec<u8>, EngineError>;

    /// Atomically writes the workbook to `path` via a sibling temporary file + fsync + rename.
    fn save_xlsx(&mut self, path: &Path) -> Result<(), EngineError>;
}
