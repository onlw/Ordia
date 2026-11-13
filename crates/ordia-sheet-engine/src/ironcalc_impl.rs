//! IronCalc-backed implementation of [`crate::SheetEngine`].
//!
//! All `ironcalc` and `ironcalc_base` types remain strictly private to this module so they never
//! leak into `ordia-sheet-engine`'s public API (`AGENTS.md` §4.2, `D-004`).

use crate::error::EngineError;
use crate::traits::SheetEngine;
use crate::types::{
    BorderEdge, BorderLineStyle, CellBorders, CellComment, CellCoord, CellHyperlink, CellRange,
    CellStyle, CellValue, ColSpanMetric, ColorRgba, DEFAULT_COL_WIDTH_PX, DEFAULT_ROW_HEIGHT_PX,
    DefinedNameInfo, FontStyle, FormulaError, FreezePane, HorizontalAlign, MAX_COLS, MAX_ROWS,
    RowMetric, SheetInfo, SheetVisibility, VerticalAlign,
};
use ironcalc::error::XlsxError;
use ironcalc::export::save_xlsx_to_writer;
use ironcalc::import::load_from_xlsx_bytes;
use ironcalc_base::expressions::token::Error as IcError;
use ironcalc_base::expressions::types::Area;
use ironcalc_base::language::get_default_language;
use ironcalc_base::types::{
    Alignment, Border, BorderItem, BorderStyle as IcBorderStyle, Cell as IcCell, Color as IcColor,
    Fill, Font as IcFont, FontScheme, FormulaValue, HorizontalAlignment, Link, SheetState,
    SpillValue, Style as IcStyle, Theme as IcTheme, VerticalAlignment,
};
use ironcalc_base::{COLUMN_WIDTH_FACTOR, Model, ROW_HEIGHT_FACTOR};
use std::io::{Cursor, Write};
use std::path::Path;

/// Spreadsheet engine backed by IronCalc (`ironcalc` / `ironcalc_base` `=0.9.1`).
pub struct IronCalcEngine {
    model: Model<'static>,
    dirty_evaluation: bool,
}

impl std::fmt::Debug for IronCalcEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IronCalcEngine")
            .field("workbook_name", &self.model.workbook.name)
            .field("sheet_count", &self.model.workbook.worksheets.len())
            .field("dirty_evaluation", &self.dirty_evaluation)
            .finish()
    }
}

impl IronCalcEngine {
    /// Creates a new workbook with a single empty worksheet (`Sheet1`).
    pub fn new_empty(name: &str) -> Result<Self, EngineError> {
        let mut model =
            Model::new_empty("Workbook", "en", "UTC", "en").map_err(EngineError::Format)?;
        if !name.is_empty() {
            model.workbook.name = name.to_string();
        }
        Ok(Self {
            model,
            dirty_evaluation: false,
        })
    }

    /// Loads an `.xlsx` file from `path` and evaluates formulas.
    pub fn from_xlsx_path(path: &Path) -> Result<Self, EngineError> {
        let bytes = std::fs::read(path)?;
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Workbook".to_string());
        Self::from_xlsx_bytes(&bytes, &name)
    }

    /// Loads an `.xlsx` workbook from in-memory bytes and evaluates formulas.
    pub fn from_xlsx_bytes(bytes: &[u8], name: &str) -> Result<Self, EngineError> {
        let mut engine = Self::from_xlsx_bytes_unrecalculated(bytes, name)?;
        engine.evaluate();
        Ok(engine)
    }

    /// Loads an `.xlsx` workbook from bytes **without** recalculating formulas yet, preserving
    /// cached `<v>` results from Excel/WPS so callers (such as `cargo xtask corpus --recalc`)
    /// can inspect cached outputs before calling [`SheetEngine::evaluate`].
    pub fn from_xlsx_bytes_unrecalculated(bytes: &[u8], name: &str) -> Result<Self, EngineError> {
        let workbook = load_from_xlsx_bytes(bytes, name, "en", "UTC").map_err(map_xlsx_error)?;
        let model = Model::from_workbook(workbook, "en").map_err(EngineError::Format)?;
        let has_unevaluated = model.workbook.worksheets.iter().any(|ws| {
            ws.sheet_data.cells().any(|(_, _, c)| {
                matches!(
                    &*c,
                    IcCell::CellFormula {
                        v: FormulaValue::Unevaluated,
                        ..
                    } | IcCell::ArrayFormula {
                        v: FormulaValue::Unevaluated,
                        ..
                    }
                )
            })
        });
        Ok(Self {
            model,
            dirty_evaluation: has_unevaluated,
        })
    }

    /// Returns the workbook display name.
    #[must_use]
    pub fn workbook_name(&self) -> &str {
        &self.model.workbook.name
    }

    /// Returns the total number of built-in Excel functions supported by IronCalc (`497` in `0.9.1`).
    #[must_use]
    pub fn supported_function_count() -> usize {
        // Verified against `ironcalc_base::functions::Function::into_iter() -> IntoIter<Function, 497>`.
        let lang = get_default_language();
        let probe = [
            &lang.functions.sum,
            &lang.functions.xlookup,
            &lang.functions.lambda,
        ];
        let _ = probe;
        497
    }

    fn check_sheet(&self, sheet: u32) -> Result<usize, EngineError> {
        let idx = sheet as usize;
        if idx < self.model.workbook.worksheets.len() {
            Ok(idx)
        } else {
            Err(EngineError::InvalidSheet(format!(
                "sheet index {sheet} out of range (count: {})",
                self.model.workbook.worksheets.len()
            )))
        }
    }

    fn to_ic_coord(coord: CellCoord) -> Result<(i32, i32), EngineError> {
        if !coord.is_valid() {
            return Err(EngineError::InvalidCoordinate(format!(
                "({}, {}) exceeds max ({MAX_ROWS}, {MAX_COLS})",
                coord.row, coord.col
            )));
        }
        Ok(((coord.row + 1) as i32, (coord.col + 1) as i32))
    }

    fn to_ic_area(sheet: u32, range: CellRange) -> Result<Area, EngineError> {
        let (row, column) = Self::to_ic_coord(range.start)?;
        let _ = Self::to_ic_coord(range.end)?;
        Ok(Area {
            sheet,
            row,
            column,
            width: range.width() as i32,
            height: range.height() as i32,
        })
    }
}

impl SheetEngine for IronCalcEngine {
    fn sheets(&self) -> Vec<SheetInfo> {
        let theme = &self.model.workbook.theme;
        self.model
            .workbook
            .worksheets
            .iter()
            .enumerate()
            .map(|(idx, ws)| {
                let visibility = match ws.state {
                    SheetState::Visible => SheetVisibility::Visible,
                    SheetState::Hidden => SheetVisibility::Hidden,
                    SheetState::VeryHidden => SheetVisibility::VeryHidden,
                };
                let tab_color = ic_color_to_rgba(&ws.color, theme);
                let used_range = if ws.sheet_data.is_empty() {
                    None
                } else {
                    let dim = ws.dimension();
                    if dim.min_row >= 1
                        && dim.min_column >= 1
                        && dim.max_row >= dim.min_row
                        && dim.max_column >= dim.min_column
                    {
                        Some(CellRange::new(
                            CellCoord::new((dim.min_row - 1) as u32, (dim.min_column - 1) as u32),
                            CellCoord::new((dim.max_row - 1) as u32, (dim.max_column - 1) as u32),
                        ))
                    } else {
                        None
                    }
                };
                SheetInfo {
                    index: idx as u32,
                    sheet_id: ws.sheet_id,
                    name: ws.name.clone(),
                    visibility,
                    tab_color,
                    show_grid_lines: ws.show_grid_lines,
                    used_range,
                }
            })
            .collect()
    }

    fn sheet_count(&self) -> usize {
        self.model.workbook.worksheets.len()
    }

    fn sheet_index_by_name(&self, name: &str) -> Option<u32> {
        let needle = name.to_uppercase();
        self.model
            .workbook
            .worksheets
            .iter()
            .position(|ws| ws.name.to_uppercase() == needle)
            .map(|idx| idx as u32)
    }

    fn add_sheet(&mut self, name: Option<&str>) -> Result<u32, EngineError> {
        if let Some(sheet_name) = name {
            self.model
                .add_sheet(sheet_name)
                .map_err(EngineError::Mutation)?;
            Ok((self.model.workbook.worksheets.len() - 1) as u32)
        } else {
            let (_, idx) = self.model.new_sheet();
            Ok(idx)
        }
    }

    fn rename_sheet(&mut self, sheet: u32, new_name: &str) -> Result<(), EngineError> {
        let _ = self.check_sheet(sheet)?;
        self.model
            .rename_sheet_by_index(sheet, new_name)
            .map_err(EngineError::Mutation)
    }

    fn delete_sheet(&mut self, sheet: u32) -> Result<(), EngineError> {
        let _ = self.check_sheet(sheet)?;
        self.model
            .delete_sheet(sheet)
            .map_err(EngineError::Mutation)
    }

    fn duplicate_sheet(&mut self, sheet: u32) -> Result<u32, EngineError> {
        let _ = self.check_sheet(sheet)?;
        let (_, new_idx) = self
            .model
            .duplicate_sheet(sheet)
            .map_err(EngineError::Mutation)?;
        Ok(new_idx)
    }

    fn move_sheet(&mut self, from_index: u32, to_index: u32) -> Result<(), EngineError> {
        let _ = self.check_sheet(from_index)?;
        let _ = self.check_sheet(to_index)?;
        self.model
            .move_sheet(from_index, to_index)
            .map_err(EngineError::Mutation)
    }

    fn set_sheet_visibility(
        &mut self,
        sheet: u32,
        visibility: SheetVisibility,
    ) -> Result<(), EngineError> {
        let _ = self.check_sheet(sheet)?;
        let state = match visibility {
            SheetVisibility::Visible => SheetState::Visible,
            SheetVisibility::Hidden => SheetState::Hidden,
            SheetVisibility::VeryHidden => SheetState::VeryHidden,
        };
        self.model
            .set_sheet_state(sheet, state)
            .map_err(EngineError::Mutation)
    }

    fn set_sheet_tab_color(
        &mut self,
        sheet: u32,
        color: Option<ColorRgba>,
    ) -> Result<(), EngineError> {
        let _ = self.check_sheet(sheet)?;
        let ic_color = rgba_to_ic_color(color);
        self.model
            .set_sheet_color(sheet, &ic_color)
            .map_err(EngineError::Mutation)
    }

    fn set_show_grid_lines(&mut self, sheet: u32, show: bool) -> Result<(), EngineError> {
        let _ = self.check_sheet(sheet)?;
        self.model
            .set_show_grid_lines(sheet, show)
            .map_err(EngineError::Mutation)
    }

    fn cell_value(&self, sheet: u32, coord: CellCoord) -> Result<CellValue, EngineError> {
        let ws_idx = self.check_sheet(sheet)?;
        let (ic_row, ic_col) = Self::to_ic_coord(coord)?;
        let ws = &self.model.workbook.worksheets[ws_idx];
        let Some(cell) = ws.cell(ic_row, ic_col) else {
            return Ok(CellValue::Empty);
        };
        Ok(match &*cell {
            IcCell::EmptyCell { .. } => CellValue::Empty,
            IcCell::NumberCell { v, .. } => CellValue::Number(*v),
            IcCell::BooleanCell { v, .. } => CellValue::Boolean(*v),
            IcCell::SharedString { si, .. } => {
                let s = self
                    .model
                    .workbook
                    .shared_strings
                    .get(*si as usize)
                    .cloned()
                    .unwrap_or_default();
                CellValue::Text(s)
            }
            IcCell::ErrorCell { ei, .. } => CellValue::Error(map_ic_error(ei)),
            IcCell::CellFormula { v, .. } | IcCell::ArrayFormula { v, .. } => match v {
                FormulaValue::Unevaluated => CellValue::Empty,
                FormulaValue::Number(n) => CellValue::Number(*n),
                FormulaValue::Boolean(b) => CellValue::Boolean(*b),
                FormulaValue::Text(s) => CellValue::Text(s.clone()),
                FormulaValue::Error { ei, .. } => CellValue::Error(map_ic_error(ei)),
            },
            IcCell::SpillCell { v, .. } => match v {
                SpillValue::Number(n) => CellValue::Number(*n),
                SpillValue::Boolean(b) => CellValue::Boolean(*b),
                SpillValue::Text(s) => CellValue::Text(s.clone()),
                SpillValue::Error(ei) => CellValue::Error(map_ic_error(ei)),
            },
        })
    }

    fn cell_display_text(&self, sheet: u32, coord: CellCoord) -> Result<String, EngineError> {
        let _ = self.check_sheet(sheet)?;
        let (ic_row, ic_col) = Self::to_ic_coord(coord)?;
        self.model
            .get_formatted_cell_value(sheet, ic_row, ic_col)
            .map_err(EngineError::Mutation)
    }

    fn cell_formula(&self, sheet: u32, coord: CellCoord) -> Result<Option<String>, EngineError> {
        let _ = self.check_sheet(sheet)?;
        let (ic_row, ic_col) = Self::to_ic_coord(coord)?;
        self.model
            .get_cell_formula(sheet, ic_row, ic_col)
            .map_err(EngineError::Formula)
    }

    fn cell_input(&self, sheet: u32, coord: CellCoord) -> Result<String, EngineError> {
        let _ = self.check_sheet(sheet)?;
        let (ic_row, ic_col) = Self::to_ic_coord(coord)?;
        if let Some(formula) = self
            .model
            .get_cell_formula(sheet, ic_row, ic_col)
            .map_err(EngineError::Formula)?
        {
            return Ok(formula);
        }
        let raw = self
            .model
            .get_localized_cell_content(sheet, ic_row, ic_col)
            .map_err(EngineError::Mutation)?;
        if let Ok(style) = self.model.get_style_for_cell(sheet, ic_row, ic_col)
            && style.quote_prefix
        {
            return Ok(format!("'{raw}"));
        }
        Ok(raw)
    }

    fn set_cell_input(
        &mut self,
        sheet: u32,
        coord: CellCoord,
        input: &str,
    ) -> Result<(), EngineError> {
        self.set_cell_input_deferred(sheet, coord, input)?;
        self.evaluate();
        Ok(())
    }

    fn set_cell_input_deferred(
        &mut self,
        sheet: u32,
        coord: CellCoord,
        input: &str,
    ) -> Result<(), EngineError> {
        let _ = self.check_sheet(sheet)?;
        let (ic_row, ic_col) = Self::to_ic_coord(coord)?;
        self.model
            .set_user_input(sheet, ic_row, ic_col, input.to_string())
            .map_err(EngineError::Mutation)?;
        self.dirty_evaluation = true;
        Ok(())
    }

    fn clear_range_contents(&mut self, sheet: u32, range: CellRange) -> Result<(), EngineError> {
        let _ = self.check_sheet(sheet)?;
        let area = Self::to_ic_area(sheet, range)?;
        self.model
            .range_clear_contents(&area)
            .map_err(EngineError::Mutation)?;
        self.evaluate();
        Ok(())
    }

    fn populated_cells(&self, sheet: u32) -> Result<Vec<CellCoord>, EngineError> {
        let ws_idx = self.check_sheet(sheet)?;
        let ws = &self.model.workbook.worksheets[ws_idx];
        let mut coords = Vec::new();
        for (ic_row, ic_col, cell) in ws.sheet_data.cells() {
            if !matches!(&*cell, IcCell::EmptyCell { .. }) && ic_row >= 1 && ic_col >= 1 {
                coords.push(CellCoord::new((ic_row - 1) as u32, (ic_col - 1) as u32));
            }
        }
        Ok(coords)
    }

    fn evaluate(&mut self) {
        self.model.evaluate();
        self.dirty_evaluation = false;
    }

    fn cell_style(&self, sheet: u32, coord: CellCoord) -> Result<CellStyle, EngineError> {
        let _ = self.check_sheet(sheet)?;
        let (ic_row, ic_col) = Self::to_ic_coord(coord)?;
        let style = self
            .model
            .get_style_for_cell(sheet, ic_row, ic_col)
            .map_err(EngineError::Mutation)?;
        Ok(ic_style_to_ordia(&style, &self.model.workbook.theme))
    }

    fn set_cell_style(
        &mut self,
        sheet: u32,
        coord: CellCoord,
        style: &CellStyle,
    ) -> Result<(), EngineError> {
        let _ = self.check_sheet(sheet)?;
        let (ic_row, ic_col) = Self::to_ic_coord(coord)?;
        let ic_style = ordia_style_to_ic(style);
        self.model
            .set_cell_style(sheet, ic_row, ic_col, &ic_style)
            .map_err(EngineError::Mutation)
    }

    fn merged_ranges(&self, sheet: u32) -> Result<Vec<CellRange>, EngineError> {
        let _ = self.check_sheet(sheet)?;
        let merges = self
            .model
            .get_merged_cells(sheet)
            .map_err(EngineError::InvalidSheet)?;
        Ok(merges
            .iter()
            .filter(|m| m.row >= 1 && m.column >= 1 && m.width >= 1 && m.height >= 1)
            .map(|m| {
                let start = CellCoord::new((m.row - 1) as u32, (m.column - 1) as u32);
                let end = CellCoord::new((m.last_row() - 1) as u32, (m.last_column() - 1) as u32);
                CellRange::new(start, end)
            })
            .collect())
    }

    fn merge_range(&mut self, sheet: u32, range: CellRange) -> Result<(), EngineError> {
        let _ = self.check_sheet(sheet)?;
        let area = Self::to_ic_area(sheet, range)?;
        self.model.merge_cells(&area).map_err(EngineError::Mutation)
    }

    fn unmerge_range(&mut self, sheet: u32, range: CellRange) -> Result<(), EngineError> {
        let _ = self.check_sheet(sheet)?;
        let area = Self::to_ic_area(sheet, range)?;
        self.model
            .unmerge_cells(&area)
            .map_err(EngineError::Mutation)
    }

    fn freeze_pane(&self, sheet: u32) -> Result<FreezePane, EngineError> {
        let _ = self.check_sheet(sheet)?;
        let rows = self
            .model
            .get_frozen_rows_count(sheet)
            .map_err(EngineError::InvalidSheet)?;
        let cols = self
            .model
            .get_frozen_columns_count(sheet)
            .map_err(EngineError::InvalidSheet)?;
        Ok(FreezePane {
            frozen_rows: rows.max(0) as u32,
            frozen_cols: cols.max(0) as u32,
        })
    }

    fn set_freeze_pane(&mut self, sheet: u32, freeze: FreezePane) -> Result<(), EngineError> {
        let _ = self.check_sheet(sheet)?;
        self.model
            .set_frozen_rows(sheet, freeze.frozen_rows as i32)
            .map_err(EngineError::Mutation)?;
        self.model
            .set_frozen_columns(sheet, freeze.frozen_cols as i32)
            .map_err(EngineError::Mutation)
    }

    fn row_height(&self, sheet: u32, row: u32) -> Result<f32, EngineError> {
        let _ = self.check_sheet(sheet)?;
        if row >= MAX_ROWS {
            return Err(EngineError::InvalidCoordinate(format!(
                "row {row} >= {MAX_ROWS}"
            )));
        }
        let ic_row = (row + 1) as i32;
        if self
            .model
            .is_row_hidden(sheet, ic_row)
            .map_err(EngineError::Mutation)?
        {
            return Ok(0.0);
        }
        self.model
            .get_row_height(sheet, ic_row)
            .map(|h| h as f32)
            .map_err(EngineError::Mutation)
    }

    fn set_row_height(&mut self, sheet: u32, row: u32, height_px: f32) -> Result<(), EngineError> {
        let _ = self.check_sheet(sheet)?;
        if row >= MAX_ROWS {
            return Err(EngineError::InvalidCoordinate(format!(
                "row {row} >= {MAX_ROWS}"
            )));
        }
        self.model
            .set_row_height(sheet, (row + 1) as i32, f64::from(height_px))
            .map_err(EngineError::Mutation)
    }

    fn is_row_hidden(&self, sheet: u32, row: u32) -> Result<bool, EngineError> {
        let _ = self.check_sheet(sheet)?;
        if row >= MAX_ROWS {
            return Err(EngineError::InvalidCoordinate(format!(
                "row {row} >= {MAX_ROWS}"
            )));
        }
        self.model
            .is_row_hidden(sheet, (row + 1) as i32)
            .map_err(EngineError::Mutation)
    }

    fn set_row_hidden(&mut self, sheet: u32, row: u32, hidden: bool) -> Result<(), EngineError> {
        let _ = self.check_sheet(sheet)?;
        if row >= MAX_ROWS {
            return Err(EngineError::InvalidCoordinate(format!(
                "row {row} >= {MAX_ROWS}"
            )));
        }
        self.model
            .set_row_hidden(sheet, (row + 1) as i32, hidden)
            .map_err(EngineError::Mutation)
    }

    fn custom_row_metrics(&self, sheet: u32) -> Result<Vec<RowMetric>, EngineError> {
        let ws_idx = self.check_sheet(sheet)?;
        let ws = &self.model.workbook.worksheets[ws_idx];
        Ok(ws
            .rows
            .iter()
            .filter(|r| r.r >= 1 && (r.r as u32) <= MAX_ROWS && (r.custom_height || r.hidden))
            .map(|r| RowMetric {
                row: (r.r - 1) as u32,
                height_px: if r.custom_height {
                    (r.height * ROW_HEIGHT_FACTOR) as f32
                } else {
                    DEFAULT_ROW_HEIGHT_PX
                },
                custom_height: r.custom_height,
                hidden: r.hidden,
            })
            .collect())
    }

    fn col_width(&self, sheet: u32, col: u32) -> Result<f32, EngineError> {
        let _ = self.check_sheet(sheet)?;
        if col >= MAX_COLS {
            return Err(EngineError::InvalidCoordinate(format!(
                "col {col} >= {MAX_COLS}"
            )));
        }
        self.model
            .get_column_width(sheet, (col + 1) as i32)
            .map(|w| w as f32)
            .map_err(EngineError::Mutation)
    }

    fn set_col_width(&mut self, sheet: u32, col: u32, width_px: f32) -> Result<(), EngineError> {
        let _ = self.check_sheet(sheet)?;
        if col >= MAX_COLS {
            return Err(EngineError::InvalidCoordinate(format!(
                "col {col} >= {MAX_COLS}"
            )));
        }
        self.model
            .set_column_width(sheet, (col + 1) as i32, f64::from(width_px))
            .map_err(EngineError::Mutation)
    }

    fn is_col_hidden(&self, sheet: u32, col: u32) -> Result<bool, EngineError> {
        let _ = self.check_sheet(sheet)?;
        if col >= MAX_COLS {
            return Err(EngineError::InvalidCoordinate(format!(
                "col {col} >= {MAX_COLS}"
            )));
        }
        self.model
            .is_column_hidden(sheet, (col + 1) as i32)
            .map_err(EngineError::Mutation)
    }

    fn set_col_hidden(&mut self, sheet: u32, col: u32, hidden: bool) -> Result<(), EngineError> {
        let _ = self.check_sheet(sheet)?;
        if col >= MAX_COLS {
            return Err(EngineError::InvalidCoordinate(format!(
                "col {col} >= {MAX_COLS}"
            )));
        }
        self.model
            .set_column_hidden(sheet, (col + 1) as i32, hidden)
            .map_err(EngineError::Mutation)
    }

    fn custom_col_metrics(&self, sheet: u32) -> Result<Vec<ColSpanMetric>, EngineError> {
        let ws_idx = self.check_sheet(sheet)?;
        let ws = &self.model.workbook.worksheets[ws_idx];
        Ok(ws
            .cols
            .iter()
            .filter(|c| c.min >= 1 && c.max >= c.min && (c.custom_width || c.hidden))
            .map(|c| ColSpanMetric {
                start_col: (c.min - 1) as u32,
                end_col: ((c.max - 1) as u32).min(MAX_COLS - 1),
                width_px: if c.custom_width {
                    (c.width * COLUMN_WIDTH_FACTOR) as f32
                } else {
                    DEFAULT_COL_WIDTH_PX
                },
                custom_width: c.custom_width,
                hidden: c.hidden,
            })
            .collect())
    }

    fn insert_rows(&mut self, sheet: u32, at_row: u32, count: u32) -> Result<(), EngineError> {
        let _ = self.check_sheet(sheet)?;
        if count == 0 {
            return Ok(());
        }
        self.model
            .insert_rows(sheet, (at_row + 1) as i32, count as i32)
            .map_err(EngineError::Mutation)?;
        self.evaluate();
        Ok(())
    }

    fn delete_rows(&mut self, sheet: u32, at_row: u32, count: u32) -> Result<(), EngineError> {
        let _ = self.check_sheet(sheet)?;
        if count == 0 {
            return Ok(());
        }
        self.model
            .delete_rows(sheet, (at_row + 1) as i32, count as i32)
            .map_err(EngineError::Mutation)?;
        self.evaluate();
        Ok(())
    }

    fn insert_cols(&mut self, sheet: u32, at_col: u32, count: u32) -> Result<(), EngineError> {
        let _ = self.check_sheet(sheet)?;
        if count == 0 {
            return Ok(());
        }
        self.model
            .insert_columns(sheet, (at_col + 1) as i32, count as i32)
            .map_err(EngineError::Mutation)?;
        self.evaluate();
        Ok(())
    }

    fn delete_cols(&mut self, sheet: u32, at_col: u32, count: u32) -> Result<(), EngineError> {
        let _ = self.check_sheet(sheet)?;
        if count == 0 {
            return Ok(());
        }
        self.model
            .delete_columns(sheet, (at_col + 1) as i32, count as i32)
            .map_err(EngineError::Mutation)?;
        self.evaluate();
        Ok(())
    }

    fn defined_names(&self) -> Vec<DefinedNameInfo> {
        self.model
            .get_defined_name_list()
            .into_iter()
            .map(|(name, scope_sheet, formula)| DefinedNameInfo {
                name,
                scope_sheet,
                formula,
            })
            .collect()
    }

    fn set_defined_name(
        &mut self,
        name: &str,
        scope_sheet: Option<u32>,
        formula: &str,
    ) -> Result<(), EngineError> {
        let clean_formula = formula.trim().strip_prefix('=').unwrap_or(formula.trim());
        let exists = self
            .model
            .get_defined_name_list()
            .iter()
            .any(|(n, s, _)| n.eq_ignore_ascii_case(name) && *s == scope_sheet);
        if exists {
            self.model
                .update_defined_name(name, scope_sheet, name, scope_sheet, clean_formula)
                .map_err(EngineError::Formula)?;
        } else {
            self.model
                .new_defined_name(name, scope_sheet, clean_formula)
                .map_err(EngineError::Formula)?;
        }
        self.evaluate();
        Ok(())
    }

    fn delete_defined_name(
        &mut self,
        name: &str,
        scope_sheet: Option<u32>,
    ) -> Result<(), EngineError> {
        self.model
            .delete_defined_name(name, scope_sheet)
            .map_err(EngineError::Formula)?;
        self.evaluate();
        Ok(())
    }

    fn comments(&self, sheet: u32) -> Result<Vec<CellComment>, EngineError> {
        let ws_idx = self.check_sheet(sheet)?;
        let ws = &self.model.workbook.worksheets[ws_idx];
        Ok(ws
            .comments
            .iter()
            .filter_map(|c| {
                let coord = CellCoord::parse_a1(&c.cell_ref)?;
                Some(CellComment {
                    coord,
                    author: c.author_name.clone(),
                    text: c.text.clone(),
                })
            })
            .collect())
    }

    fn hyperlinks(&self, sheet: u32) -> Result<Vec<CellHyperlink>, EngineError> {
        let ws_idx = self.check_sheet(sheet)?;
        let ws = &self.model.workbook.worksheets[ws_idx];
        let mut links: Vec<CellHyperlink> = ws
            .links
            .iter()
            .filter(|((r, c), _)| *r >= 1 && *c >= 1)
            .map(|((r, c), link)| {
                let coord = CellCoord::new((*r - 1) as u32, (*c - 1) as u32);
                match link {
                    Link::External { target, tooltip } => CellHyperlink {
                        coord,
                        target: target.clone(),
                        is_internal: false,
                        tooltip: tooltip.clone(),
                    },
                    Link::Internal { location, tooltip } => CellHyperlink {
                        coord,
                        target: location.clone(),
                        is_internal: true,
                        tooltip: tooltip.clone(),
                    },
                }
            })
            .collect();
        links.sort_by_key(|l| l.coord);
        Ok(links)
    }

    fn to_xlsx_bytes(&mut self) -> Result<Vec<u8>, EngineError> {
        if self.dirty_evaluation {
            self.evaluate();
        }
        let cursor = Cursor::new(Vec::new());
        let writer = save_xlsx_to_writer(&self.model, cursor).map_err(map_xlsx_error)?;
        Ok(writer.into_inner())
    }

    fn save_xlsx(&mut self, path: &Path) -> Result<(), EngineError> {
        let bytes = self.to_xlsx_bytes()?;
        atomic_write_bytes(path, &bytes)
    }
}

fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> Result<(), EngineError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let stem = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "workbook.xlsx".to_string());
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_path = parent.join(format!(".{stem}.{pid}.{nanos}.tmp"));
    let write_res = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&tmp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    })();
    if let Err(err) = write_res {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(EngineError::Io(err));
    }
    if let Err(err) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(EngineError::Io(err));
    }
    Ok(())
}

fn map_xlsx_error(err: XlsxError) -> EngineError {
    EngineError::Format(err.to_string())
}

fn map_ic_error(err: &IcError) -> FormulaError {
    match err {
        IcError::DIV => FormulaError::Div0,
        IcError::NAME => FormulaError::Name,
        IcError::VALUE => FormulaError::Value,
        IcError::REF => FormulaError::Ref,
        IcError::NA => FormulaError::Na,
        IcError::NUM => FormulaError::Num,
        IcError::NULL => FormulaError::Null,
        IcError::SPILL => FormulaError::Spill,
        IcError::CALC => FormulaError::Calc,
        IcError::CIRC => FormulaError::Circular,
        IcError::ERROR => FormulaError::InvalidFormula,
        IcError::NIMPL => FormulaError::NotImplemented,
    }
}

fn ic_color_to_rgba(color: &IcColor, theme: &IcTheme) -> Option<ColorRgba> {
    if color.is_none() {
        return None;
    }
    let hex = color.to_rgb(theme);
    ColorRgba::from_hex_rgb(&hex)
}

fn rgba_to_ic_color(color: Option<ColorRgba>) -> IcColor {
    match color {
        Some(c) => IcColor::Rgb(c.to_hex_rgb()),
        None => IcColor::None,
    }
}

fn ic_border_style_to_ordia(style: &IcBorderStyle) -> BorderLineStyle {
    match style {
        IcBorderStyle::Thin => BorderLineStyle::Thin,
        IcBorderStyle::Medium => BorderLineStyle::Medium,
        IcBorderStyle::Thick => BorderLineStyle::Thick,
        IcBorderStyle::Double => BorderLineStyle::Double,
        IcBorderStyle::Dotted => BorderLineStyle::Dotted,
        IcBorderStyle::MediumDashed => BorderLineStyle::MediumDashed,
        IcBorderStyle::MediumDashDot => BorderLineStyle::MediumDashDot,
        IcBorderStyle::MediumDashDotDot => BorderLineStyle::MediumDashDotDot,
        IcBorderStyle::SlantDashDot => BorderLineStyle::SlantDashDot,
    }
}

fn ordia_border_style_to_ic(style: BorderLineStyle) -> IcBorderStyle {
    match style {
        BorderLineStyle::Thin => IcBorderStyle::Thin,
        BorderLineStyle::Medium => IcBorderStyle::Medium,
        BorderLineStyle::Thick => IcBorderStyle::Thick,
        BorderLineStyle::Double => IcBorderStyle::Double,
        BorderLineStyle::Dotted => IcBorderStyle::Dotted,
        BorderLineStyle::MediumDashed => IcBorderStyle::MediumDashed,
        BorderLineStyle::MediumDashDot => IcBorderStyle::MediumDashDot,
        BorderLineStyle::MediumDashDotDot => IcBorderStyle::MediumDashDotDot,
        BorderLineStyle::SlantDashDot => IcBorderStyle::SlantDashDot,
    }
}

fn ic_border_item_to_ordia(item: &Option<BorderItem>, theme: &IcTheme) -> Option<BorderEdge> {
    item.as_ref().map(|b| BorderEdge {
        style: ic_border_style_to_ordia(&b.style),
        color: ic_color_to_rgba(&b.color, theme),
    })
}

fn ordia_border_edge_to_ic(edge: Option<BorderEdge>) -> Option<BorderItem> {
    edge.map(|e| BorderItem {
        style: ordia_border_style_to_ic(e.style),
        color: rgba_to_ic_color(e.color),
    })
}

fn ic_style_to_ordia(style: &IcStyle, theme: &IcTheme) -> CellStyle {
    let (horizontal_align, vertical_align, wrap_text) = match &style.alignment {
        Some(a) => {
            let h = match a.horizontal {
                HorizontalAlignment::General => HorizontalAlign::General,
                HorizontalAlignment::Left => HorizontalAlign::Left,
                HorizontalAlignment::Center => HorizontalAlign::Center,
                HorizontalAlignment::Right => HorizontalAlign::Right,
                HorizontalAlignment::Fill => HorizontalAlign::Fill,
                HorizontalAlignment::Justify => HorizontalAlign::Justify,
                HorizontalAlignment::CenterContinuous => HorizontalAlign::CenterContinuous,
                HorizontalAlignment::Distributed => HorizontalAlign::Distributed,
            };
            let v = match a.vertical {
                VerticalAlignment::Top => VerticalAlign::Top,
                VerticalAlignment::Center => VerticalAlign::Center,
                VerticalAlignment::Bottom => VerticalAlign::Bottom,
                VerticalAlignment::Justify => VerticalAlign::Justify,
                VerticalAlignment::Distributed => VerticalAlign::Distributed,
            };
            (h, v, a.wrap_text)
        }
        None => (HorizontalAlign::General, VerticalAlign::Bottom, false),
    };

    CellStyle {
        font: FontStyle {
            name: style.font.name.clone(),
            size_pt: style.font.sz as f32,
            bold: style.font.b,
            italic: style.font.i,
            underline: style.font.u,
            strikethrough: style.font.strike,
            color: ic_color_to_rgba(&style.font.color, theme),
        },
        fill_color: ic_color_to_rgba(&style.fill.color, theme),
        borders: CellBorders {
            top: ic_border_item_to_ordia(&style.border.top, theme),
            right: ic_border_item_to_ordia(&style.border.right, theme),
            bottom: ic_border_item_to_ordia(&style.border.bottom, theme),
            left: ic_border_item_to_ordia(&style.border.left, theme),
            diagonal: ic_border_item_to_ordia(&style.border.diagonal, theme),
            diagonal_up: style.border.diagonal_up,
            diagonal_down: style.border.diagonal_down,
        },
        horizontal_align,
        vertical_align,
        wrap_text,
        number_format: style.num_fmt.clone(),
        quote_prefix: style.quote_prefix,
    }
}

fn ordia_style_to_ic(style: &CellStyle) -> IcStyle {
    let horizontal = match style.horizontal_align {
        HorizontalAlign::General => HorizontalAlignment::General,
        HorizontalAlign::Left => HorizontalAlignment::Left,
        HorizontalAlign::Center => HorizontalAlignment::Center,
        HorizontalAlign::Right => HorizontalAlignment::Right,
        HorizontalAlign::Fill => HorizontalAlignment::Fill,
        HorizontalAlign::Justify => HorizontalAlignment::Justify,
        HorizontalAlign::CenterContinuous => HorizontalAlignment::CenterContinuous,
        HorizontalAlign::Distributed => HorizontalAlignment::Distributed,
    };
    let vertical = match style.vertical_align {
        VerticalAlign::Top => VerticalAlignment::Top,
        VerticalAlign::Center => VerticalAlignment::Center,
        VerticalAlign::Bottom => VerticalAlignment::Bottom,
        VerticalAlign::Justify => VerticalAlignment::Justify,
        VerticalAlign::Distributed => VerticalAlignment::Distributed,
    };
    let alignment = if horizontal == HorizontalAlignment::General
        && vertical == VerticalAlignment::Bottom
        && !style.wrap_text
    {
        None
    } else {
        Some(Alignment {
            horizontal,
            vertical,
            wrap_text: style.wrap_text,
        })
    };

    IcStyle {
        alignment,
        num_fmt: style.number_format.clone(),
        fill: Fill {
            color: rgba_to_ic_color(style.fill_color),
        },
        font: IcFont {
            strike: style.font.strikethrough,
            u: style.font.underline,
            b: style.font.bold,
            i: style.font.italic,
            sz: style.font.size_pt.round().max(1.0) as i32,
            color: rgba_to_ic_color(style.font.color),
            name: style.font.name.clone(),
            family: 2,
            scheme: FontScheme::None,
        },
        border: Border {
            diagonal_up: style.borders.diagonal_up,
            diagonal_down: style.borders.diagonal_down,
            left: ordia_border_edge_to_ic(style.borders.left),
            right: ordia_border_edge_to_ic(style.borders.right),
            top: ordia_border_edge_to_ic(style.borders.top),
            bottom: ordia_border_edge_to_ic(style.borders.bottom),
            diagonal: ordia_border_edge_to_ic(style.borders.diagonal),
        },
        quote_prefix: style.quote_prefix,
    }
}
