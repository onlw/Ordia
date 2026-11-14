//! High-level workbook document model owning a [`SheetEngine`], selection state, and undo/redo history.

use crate::ops::{SheetOp, Transaction};
use ordia_core::{
    Document, DocumentError, DocumentKind, LoadProgress, LoadStage, atomic_write_bytes,
    detect_document_format,
};
use ordia_sheet_engine::{
    CellCoord, CellRange, CellStyle, CellValue, ColSpanMetric, ColorRgba, EngineError, FontStyle,
    FreezePane, IronCalcEngine, MAX_COLS, MAX_ROWS, RowMetric, SheetEngine, SheetInfo,
    SheetVisibility,
};
use std::collections::BTreeMap;
use std::io::Read as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

/// File size threshold (`8 MiB`) above which `.xlsx` files use cached `<v>` values on initial open
/// to avoid blocking first paint with a full multi-million-cell recalculation.
const UNRECALCULATED_OPEN_THRESHOLD_BYTES: u64 = 8 * 1024 * 1024;

/// Fine-grained visual invalidation scope emitted by [`SheetDocument`] mutations (`M2-02`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SheetDamage {
    /// A single cell's content or style changed.
    Cell {
        /// Zero-based worksheet index.
        sheet: u32,
        /// Target cell coordinate.
        coord: CellCoord,
    },
    /// A rectangular range of cells changed.
    Range {
        /// Zero-based worksheet index.
        sheet: u32,
        /// Inclusive cell range.
        range: CellRange,
    },
    /// Row height or visibility changed at `from_row` (shifts/invalidates tiles from `from_row` downward).
    FromRow {
        /// Zero-based worksheet index.
        sheet: u32,
        /// First affected zero-based row.
        from_row: u32,
    },
    /// Column width or visibility changed at `from_col` (shifts/invalidates tiles from `from_col` rightward).
    FromCol {
        /// Zero-based worksheet index.
        sheet: u32,
        /// First affected zero-based column.
        from_col: u32,
    },
    /// Entire worksheet's tiles should be invalidated.
    Sheet {
        /// Zero-based worksheet index.
        sheet: u32,
    },
    /// Entire workbook's tile cache should be invalidated.
    All,
}

const FORMULA_DIFF_LIMIT: usize = 8_192;

/// Stateful spreadsheet workbook model mediating all reads and [`Transaction`]-based mutations.
pub struct SheetDocument {
    engine: Box<dyn SheetEngine>,
    cached_sheets: Vec<SheetInfo>,
    document_kind: DocumentKind,
    title: String,
    source_path: Option<PathBuf>,
    active_sheet: u32,
    active_cell: CellCoord,
    selection: CellRange,
    sheet_selections: BTreeMap<u32, (CellCoord, CellRange)>,
    dirty: bool,
    revision: u64,
    layout_revision: u64,
    pending_damage: Vec<SheetDamage>,
    undo_stack: Vec<Transaction>,
    redo_stack: Vec<Transaction>,
}

impl SheetDocument {
    /// Wraps an existing [`SheetEngine`] implementation.
    #[must_use]
    pub fn from_engine(title: impl Into<String>, engine: Box<dyn SheetEngine>) -> Self {
        let cached_sheets = engine.sheets();
        let active_sheet = cached_sheets
            .iter()
            .find(|s| s.visibility == SheetVisibility::Visible)
            .map_or(0, |s| s.index);
        Self {
            engine,
            cached_sheets,
            document_kind: DocumentKind::SpreadsheetXlsx,
            title: title.into(),
            source_path: None,
            active_sheet,
            active_cell: CellCoord::new(0, 0),
            selection: CellRange::single(CellCoord::new(0, 0)),
            sheet_selections: BTreeMap::new(),
            dirty: false,
            revision: 1,
            layout_revision: 1,
            pending_damage: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    /// Creates a blank workbook with a single empty worksheet.
    pub fn new_empty(title: impl Into<String>) -> Result<Self, EngineError> {
        let title_str = title.into();
        let engine = IronCalcEngine::new_empty(&title_str)?;
        Ok(Self::from_engine(title_str, Box::new(engine)))
    }

    /// Creates a starter demonstration workbook showcasing frozen headers, bilingual text, and formulas.
    pub fn new_demo(title: impl Into<String>) -> Result<Self, EngineError> {
        let mut doc = Self::new_empty(title)?;
        let header_style = CellStyle {
            font: FontStyle {
                bold: true,
                ..FontStyle::default()
            },
            fill_color: Some(ColorRgba::rgb(241, 245, 249)),
            ..CellStyle::default()
        };

        let headers = ["项目 / Item", "Q1", "Q2", "Q3", "Q4", "全年合计 / Total"];
        let mut ops = Vec::new();
        for (col_ix, label) in headers.iter().enumerate() {
            let coord = CellCoord::new(0, col_ix as u32);
            ops.push(SheetOp::SetCellInput {
                sheet: 0,
                coord,
                old_input: String::new(),
                new_input: (*label).to_string(),
            });
            ops.push(SheetOp::SetCellStyle {
                sheet: 0,
                coord,
                old_style: Box::new(CellStyle::default()),
                new_style: Box::new(header_style.clone()),
            });
        }

        let rows: &[(&str, [i32; 4])] = &[
            ("订阅收入 / Subscription", [1280, 1420, 1590, 1810]),
            ("企业授权 / Enterprise", [860, 920, 1050, 1240]),
            ("云服务成本 / Cloud Cost", [-210, -225, -240, -265]),
            ("研发投入 / R&D", [-540, -560, -580, -610]),
        ];

        for (r_offset, (name, quarters)) in rows.iter().enumerate() {
            let row = (r_offset + 1) as u32;
            ops.push(SheetOp::SetCellInput {
                sheet: 0,
                coord: CellCoord::new(row, 0),
                old_input: String::new(),
                new_input: (*name).to_string(),
            });
            for (q_ix, val) in quarters.iter().enumerate() {
                ops.push(SheetOp::SetCellInput {
                    sheet: 0,
                    coord: CellCoord::new(row, (q_ix + 1) as u32),
                    old_input: String::new(),
                    new_input: val.to_string(),
                });
            }
            let excel_row = row + 1;
            ops.push(SheetOp::SetCellInput {
                sheet: 0,
                coord: CellCoord::new(row, 5),
                old_input: String::new(),
                new_input: format!("=SUM(B{excel_row}:E{excel_row})"),
            });
        }

        // Summary net row
        let total_row = 5u32;
        ops.push(SheetOp::SetCellInput {
            sheet: 0,
            coord: CellCoord::new(total_row, 0),
            old_input: String::new(),
            new_input: "净收益 / Net".to_string(),
        });
        for col in 1..=5u32 {
            let col_letter = ordia_sheet_engine::col_to_letters(col);
            ops.push(SheetOp::SetCellInput {
                sheet: 0,
                coord: CellCoord::new(total_row, col),
                old_input: String::new(),
                new_input: format!("=SUM({col_letter}2:{col_letter}5)"),
            });
        }

        ops.push(SheetOp::SetColWidth {
            sheet: 0,
            col: 0,
            old_width_px: ordia_sheet_engine::DEFAULT_COL_WIDTH_PX,
            new_width_px: 180.0,
        });
        ops.push(SheetOp::SetColWidth {
            sheet: 0,
            col: 5,
            old_width_px: ordia_sheet_engine::DEFAULT_COL_WIDTH_PX,
            new_width_px: 136.0,
        });
        ops.push(SheetOp::SetFreezePane {
            sheet: 0,
            old_freeze: FreezePane::default(),
            new_freeze: FreezePane {
                frozen_rows: 1,
                frozen_cols: 1,
            },
        });

        doc.execute_transaction(Transaction::new("Initialize demo sheet", ops))?;
        doc.undo_stack.clear();
        doc.pending_damage.clear();
        doc.dirty = false;
        doc.set_active_cell(CellCoord::new(1, 1));
        Ok(doc)
    }

    /// Opens an `.xlsx` workbook from disk.
    ///
    /// Large workbooks above 8 MiB load cached `<v>` values immediately via
    /// [`IronCalcEngine::from_xlsx_bytes_unrecalculated`] so 1M-row sheets render without waiting
    /// for an upfront full recalculation; subsequent edits recalculate normally.
    pub fn open_xlsx(path: impl AsRef<Path>) -> Result<Self, EngineError> {
        let path_ref = path.as_ref();
        let meta = std::fs::metadata(path_ref)?;
        let title = path_ref
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("Workbook.xlsx")
            .to_string();
        let engine = if meta.len() >= UNRECALCULATED_OPEN_THRESHOLD_BYTES {
            let bytes = std::fs::read(path_ref)?;
            IronCalcEngine::from_xlsx_bytes_unrecalculated(&bytes, &title)?
        } else {
            IronCalcEngine::from_xlsx_path(path_ref)?
        };
        let mut doc = Self::from_engine(title, Box::new(engine));
        doc.source_path = Some(path_ref.to_path_buf());
        Ok(doc)
    }

    /// Display title of the workbook.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Source file path if opened from or saved to disk.
    #[must_use]
    pub fn source_path(&self) -> Option<&Path> {
        self.source_path.as_deref()
    }

    /// Returns `true` if unsaved transactions have been applied.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Monotonically increasing revision counter incremented on every content/style/layout mutation.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Monotonically increasing counter incremented when row heights, column widths, freeze panes,
    /// or active worksheet change (used by views to invalidate cached coordinate indices).
    #[must_use]
    pub fn layout_revision(&self) -> u64 {
        self.layout_revision
    }

    /// Total number of worksheets in the workbook.
    #[must_use]
    pub fn sheet_count(&self) -> usize {
        self.engine.sheet_count()
    }

    /// Zero-based index of the currently active worksheet.
    #[must_use]
    pub fn active_sheet(&self) -> u32 {
        self.active_sheet
    }

    /// Switches the active worksheet tab if valid, preserving per-sheet selection state (`M2-06`).
    pub fn set_active_sheet(&mut self, sheet: u32) -> bool {
        if (sheet as usize) < self.engine.sheet_count() && self.active_sheet != sheet {
            self.sheet_selections
                .insert(self.active_sheet, (self.active_cell, self.selection));
            self.active_sheet = sheet;
            let (next_cell, next_sel) =
                self.sheet_selections
                    .get(&sheet)
                    .copied()
                    .unwrap_or_else(|| {
                        (
                            CellCoord::new(0, 0),
                            CellRange::single(CellCoord::new(0, 0)),
                        )
                    });
            self.active_cell = next_cell;
            self.selection = next_sel;
            self.layout_revision += 1;
            true
        } else {
            false
        }
    }

    /// Currently focused cell coordinate on the active worksheet.
    #[must_use]
    pub fn active_cell(&self) -> CellCoord {
        self.active_cell
    }

    /// Current rectangular selection range on the active worksheet.
    #[must_use]
    pub fn selection(&self) -> CellRange {
        self.selection
    }

    /// Moves the active cell and resets the selection to that single cell (clamped to sheet bounds).
    pub fn set_active_cell(&mut self, coord: CellCoord) {
        let clamped = CellCoord::new(
            coord.row.min(MAX_ROWS.saturating_sub(1)),
            coord.col.min(MAX_COLS.saturating_sub(1)),
        );
        self.active_cell = clamped;
        self.selection = CellRange::single(clamped);
    }

    /// Extends or sets the rectangular selection range while keeping `active_cell` inside `range`.
    pub fn set_selection(&mut self, range: CellRange) {
        let clamped = CellRange::new(
            CellCoord::new(
                range.start.row.min(MAX_ROWS.saturating_sub(1)),
                range.start.col.min(MAX_COLS.saturating_sub(1)),
            ),
            CellCoord::new(
                range.end.row.min(MAX_ROWS.saturating_sub(1)),
                range.end.col.min(MAX_COLS.saturating_sub(1)),
            ),
        );
        self.selection = clamped;
        if !clamped.contains(self.active_cell) {
            self.active_cell = clamped.start;
        }
    }

    /// Moves the active cell by `(delta_row, delta_col)` clamped to `[0..MAX_ROWS, 0..MAX_COLS]`.
    pub fn move_active_cell(&mut self, delta_row: i32, delta_col: i32) -> CellCoord {
        let next_row = (i64::from(self.active_cell.row) + i64::from(delta_row))
            .clamp(0, i64::from(MAX_ROWS - 1)) as u32;
        let next_col = (i64::from(self.active_cell.col) + i64::from(delta_col))
            .clamp(0, i64::from(MAX_COLS - 1)) as u32;
        let next = CellCoord::new(next_row, next_col);
        self.set_active_cell(next);
        next
    }

    /// Metadata for all worksheets in the workbook (cached in `O(1)` without rescanning cells).
    #[must_use]
    pub fn sheets(&self) -> &[SheetInfo] {
        &self.cached_sheets
    }

    /// Metadata for the currently active worksheet, if available (`O(1)`).
    #[must_use]
    pub fn active_sheet_info(&self) -> Option<&SheetInfo> {
        self.cached_sheets
            .iter()
            .find(|s| s.index == self.active_sheet)
    }

    /// Lists user-hidden worksheets (`SheetVisibility::Hidden`) in workbook order (`M2-06`).
    #[must_use]
    pub fn hidden_sheets(&self) -> Vec<&SheetInfo> {
        self.cached_sheets
            .iter()
            .filter(|s| s.visibility == SheetVisibility::Hidden)
            .collect()
    }

    /// Appends a new empty worksheet via an undoable [`Transaction`] and returns its index (`M2-06`).
    pub fn add_sheet(&mut self, name: Option<&str>) -> Result<u32, EngineError> {
        let index = self.engine.sheet_count() as u32;
        let resolved_name = match name {
            Some(n) if !n.trim().is_empty() => n.trim().to_string(),
            _ => {
                let mut candidate = index + 1;
                loop {
                    let s = format!("Sheet{candidate}");
                    if self.engine.sheet_index_by_name(&s).is_none() {
                        break s;
                    }
                    candidate += 1;
                }
            }
        };
        self.execute_transaction(Transaction::single(
            format!("Add sheet {resolved_name}"),
            SheetOp::AddEmptySheet {
                index,
                name: resolved_name,
            },
        ))?;
        Ok(index)
    }

    /// Updates `sheet`'s custom tab accent color via an undoable [`Transaction`] (`M2-06`).
    pub fn set_sheet_tab_color(
        &mut self,
        sheet: u32,
        new_color: Option<ColorRgba>,
    ) -> Result<(), EngineError> {
        let old_color = self
            .cached_sheets
            .iter()
            .find(|s| s.index == sheet)
            .and_then(|s| s.tab_color);
        if old_color == new_color {
            return Ok(());
        }
        self.execute_transaction(Transaction::single(
            "Set sheet tab color",
            SheetOp::SetSheetTabColor {
                sheet,
                old_color,
                new_color,
            },
        ))
    }

    /// Updates `sheet`'s visibility state via an undoable [`Transaction`] (`M2-06`).
    pub fn set_sheet_visibility(
        &mut self,
        sheet: u32,
        new_visibility: SheetVisibility,
    ) -> Result<(), EngineError> {
        let old_visibility = self
            .cached_sheets
            .iter()
            .find(|s| s.index == sheet)
            .map_or(SheetVisibility::Visible, |s| s.visibility);
        if old_visibility == new_visibility {
            return Ok(());
        }
        self.execute_transaction(Transaction::single(
            "Set sheet visibility",
            SheetOp::SetSheetVisibility {
                sheet,
                old_visibility,
                new_visibility,
            },
        ))
    }

    fn expand_cached_used_range(&mut self, tx: &Transaction) {
        for op in &tx.ops {
            let target_range = match op {
                SheetOp::SetCellInput {
                    sheet,
                    coord,
                    new_input,
                    ..
                } if !new_input.is_empty() => Some((*sheet, CellRange::single(*coord))),
                SheetOp::SetCellStyle { sheet, coord, .. } => {
                    Some((*sheet, CellRange::single(*coord)))
                }
                SheetOp::SetMerged {
                    sheet,
                    range,
                    merged: true,
                } => Some((*sheet, *range)),
                _ => None,
            };
            if let Some((sheet, r)) = target_range
                && let Some(info) = self.cached_sheets.iter_mut().find(|s| s.index == sheet)
            {
                info.used_range = Some(match info.used_range {
                    Some(existing) => CellRange::new(
                        CellCoord::new(
                            existing.start.row.min(r.start.row),
                            existing.start.col.min(r.start.col),
                        ),
                        CellCoord::new(
                            existing.end.row.max(r.end.row),
                            existing.end.col.max(r.end.col),
                        ),
                    ),
                    None => r,
                });
            }
        }
    }

    /// Reads the formatted display text of `coord` on `sheet`.
    pub fn cell_display_text_on(
        &self,
        sheet: u32,
        coord: CellCoord,
    ) -> Result<String, EngineError> {
        self.engine.cell_display_text(sheet, coord)
    }

    /// Reads the evaluated [`CellValue`] of `coord` on `sheet`.
    pub fn cell_value_on(&self, sheet: u32, coord: CellCoord) -> Result<CellValue, EngineError> {
        self.engine.cell_value(sheet, coord)
    }

    /// Reads the effective [`CellStyle`] of `coord` on `sheet`.
    pub fn cell_style_on(&self, sheet: u32, coord: CellCoord) -> Result<CellStyle, EngineError> {
        self.engine.cell_style(sheet, coord)
    }

    /// Reads the formatted display text of `coord` on the active worksheet.
    pub fn cell_display_text(&self, coord: CellCoord) -> Result<String, EngineError> {
        self.engine.cell_display_text(self.active_sheet, coord)
    }

    /// Reads the evaluated [`CellValue`] of `coord` on the active worksheet.
    pub fn cell_value(&self, coord: CellCoord) -> Result<CellValue, EngineError> {
        self.engine.cell_value(self.active_sheet, coord)
    }

    /// Reads the raw editable input string (formula or literal) of `coord` on the active worksheet.
    pub fn cell_input(&self, coord: CellCoord) -> Result<String, EngineError> {
        self.engine.cell_input(self.active_sheet, coord)
    }

    /// Reads the effective [`CellStyle`] of `coord` on the active worksheet.
    pub fn cell_style(&self, coord: CellCoord) -> Result<CellStyle, EngineError> {
        self.engine.cell_style(self.active_sheet, coord)
    }

    /// Returns the [`FreezePane`] settings of the active worksheet.
    pub fn freeze_pane(&self) -> Result<FreezePane, EngineError> {
        self.engine.freeze_pane(self.active_sheet)
    }

    /// Returns sparse row height/visibility overrides for the active worksheet.
    pub fn custom_row_metrics(&self) -> Result<Vec<RowMetric>, EngineError> {
        self.engine.custom_row_metrics(self.active_sheet)
    }

    /// Returns sparse column span width/visibility overrides for the active worksheet.
    pub fn custom_col_metrics(&self) -> Result<Vec<ColSpanMetric>, EngineError> {
        self.engine.custom_col_metrics(self.active_sheet)
    }

    /// Lists all merged cell ranges on `sheet`.
    pub fn merged_ranges_on(&self, sheet: u32) -> Result<Vec<CellRange>, EngineError> {
        self.engine.merged_ranges(sheet)
    }

    /// Lists all merged cell ranges on the active worksheet.
    pub fn merged_ranges(&self) -> Result<Vec<CellRange>, EngineError> {
        self.engine.merged_ranges(self.active_sheet)
    }

    /// Returns whether gridlines are visible on `sheet`.
    #[must_use]
    pub fn show_grid_lines_on(&self, sheet: u32) -> bool {
        self.cached_sheets
            .iter()
            .find(|s| s.index == sheet)
            .is_none_or(|s| s.show_grid_lines)
    }

    /// Returns whether gridlines are visible on the active worksheet.
    #[must_use]
    pub fn show_grid_lines(&self) -> bool {
        self.show_grid_lines_on(self.active_sheet)
    }

    /// Shows or hides gridlines on the active worksheet via an undoable [`Transaction`].
    pub fn set_show_grid_lines(&mut self, show: bool) -> Result<(), EngineError> {
        let old_show = self.show_grid_lines();
        if old_show == show {
            return Ok(());
        }
        self.execute_transaction(Transaction::single(
            "Toggle gridlines",
            SheetOp::SetShowGridLines {
                sheet: self.active_sheet,
                old_show,
                new_show: show,
            },
        ))
    }

    /// Merges `range` on the active worksheet via an undoable [`Transaction`].
    pub fn merge_range(&mut self, range: CellRange) -> Result<(), EngineError> {
        self.execute_transaction(Transaction::single(
            format!("Merge {range}"),
            SheetOp::SetMerged {
                sheet: self.active_sheet,
                range,
                merged: true,
            },
        ))
    }

    /// Unmerges `range` on the active worksheet via an undoable [`Transaction`].
    pub fn unmerge_range(&mut self, range: CellRange) -> Result<(), EngineError> {
        self.execute_transaction(Transaction::single(
            format!("Unmerge {range}"),
            SheetOp::SetMerged {
                sheet: self.active_sheet,
                range,
                merged: false,
            },
        ))
    }

    /// Hides or unhides `row` on the active worksheet via an undoable [`Transaction`].
    pub fn set_row_hidden(&mut self, row: u32, hidden: bool) -> Result<(), EngineError> {
        let old_hidden = self.engine.is_row_hidden(self.active_sheet, row)?;
        if old_hidden == hidden {
            return Ok(());
        }
        self.execute_transaction(Transaction::single(
            format!("Set row {} hidden={hidden}", row + 1),
            SheetOp::SetRowHidden {
                sheet: self.active_sheet,
                row,
                old_hidden,
                new_hidden: hidden,
            },
        ))
    }

    /// Hides or unhides `col` on the active worksheet via an undoable [`Transaction`].
    pub fn set_col_hidden(&mut self, col: u32, hidden: bool) -> Result<(), EngineError> {
        let old_hidden = self.engine.is_col_hidden(self.active_sheet, col)?;
        if old_hidden == hidden {
            return Ok(());
        }
        self.execute_transaction(Transaction::single(
            format!(
                "Set col {} hidden={hidden}",
                ordia_sheet_engine::col_to_letters(col)
            ),
            SheetOp::SetColHidden {
                sheet: self.active_sheet,
                col,
                old_hidden,
                new_hidden: hidden,
            },
        ))
    }

    /// Updates the [`FreezePane`] settings of the active worksheet via an undoable [`Transaction`].
    pub fn set_freeze_pane(&mut self, new_freeze: FreezePane) -> Result<(), EngineError> {
        let old_freeze = self.freeze_pane()?;
        if old_freeze == new_freeze {
            return Ok(());
        }
        self.execute_transaction(Transaction::single(
            "Set freeze panes",
            SheetOp::SetFreezePane {
                sheet: self.active_sheet,
                old_freeze,
                new_freeze,
            },
        ))
    }

    /// Drains fine-grained [`SheetDamage`] scopes accumulated since the last layout/tile sync (`M2-02`).
    pub fn drain_damage(&mut self) -> Vec<SheetDamage> {
        std::mem::take(&mut self.pending_damage)
    }

    fn apply_tx_with_damage(&mut self, tx: &Transaction) -> Result<bool, EngineError> {
        let mut touches_layout = false;
        let mut touches_inputs = false;
        let mut touches_sheets = false;
        for op in &tx.ops {
            match op {
                SheetOp::SetCellInput { .. } => touches_inputs = true,
                SheetOp::SetFreezePane { .. }
                | SheetOp::SetRowHeight { .. }
                | SheetOp::SetColWidth { .. }
                | SheetOp::SetRowHidden { .. }
                | SheetOp::SetColHidden { .. }
                | SheetOp::SetMerged { .. } => touches_layout = true,
                SheetOp::AddEmptySheet { .. } | SheetOp::DeleteEmptySheet { .. } => {
                    touches_layout = true;
                    touches_sheets = true;
                }
                SheetOp::SetSheetTabColor { .. } | SheetOp::SetSheetVisibility { .. } => {
                    touches_sheets = true;
                }
                SheetOp::SetCellStyle { .. } | SheetOp::SetShowGridLines { .. } => {}
            }
        }

        // Snapshot formula cell values before applying `SetCellInput` when formula count is bounded,
        // so editing an unreferenced cell invalidates ONLY its single 64x16 tile (`M2-02`).
        let formula_before = if touches_inputs && self.engine.has_formulas() {
            self.snapshot_formula_values()
        } else {
            Some(Vec::new())
        };

        tx.apply(self.engine.as_mut())?;

        for op in &tx.ops {
            match op {
                SheetOp::SetCellInput { sheet, coord, .. }
                | SheetOp::SetCellStyle { sheet, coord, .. } => {
                    self.pending_damage.push(SheetDamage::Cell {
                        sheet: *sheet,
                        coord: *coord,
                    });
                }
                SheetOp::SetRowHeight { sheet, row, .. }
                | SheetOp::SetRowHidden { sheet, row, .. } => {
                    self.pending_damage.push(SheetDamage::FromRow {
                        sheet: *sheet,
                        from_row: *row,
                    });
                }
                SheetOp::SetColWidth { sheet, col, .. }
                | SheetOp::SetColHidden { sheet, col, .. } => {
                    self.pending_damage.push(SheetDamage::FromCol {
                        sheet: *sheet,
                        from_col: *col,
                    });
                }
                SheetOp::SetShowGridLines {
                    sheet, new_show, ..
                } => {
                    if let Some(info) = self.cached_sheets.iter_mut().find(|s| s.index == *sheet) {
                        info.show_grid_lines = *new_show;
                    }
                    self.pending_damage
                        .push(SheetDamage::Sheet { sheet: *sheet });
                }
                SheetOp::SetMerged { sheet, range, .. } => {
                    self.pending_damage.push(SheetDamage::Range {
                        sheet: *sheet,
                        range: *range,
                    });
                }
                SheetOp::AddEmptySheet { .. } | SheetOp::DeleteEmptySheet { .. } => {
                    self.pending_damage.push(SheetDamage::All);
                }
                SheetOp::SetFreezePane { .. }
                | SheetOp::SetSheetTabColor { .. }
                | SheetOp::SetSheetVisibility { .. } => {}
            }
        }

        if touches_sheets {
            self.cached_sheets = self.engine.sheets();
            let count = self.engine.sheet_count() as u32;
            if count > 0 && self.active_sheet >= count {
                self.active_sheet = count - 1;
            }
        }

        if touches_inputs {
            match formula_before {
                Some(snaps) => {
                    for (sheet, coord, before_val) in snaps {
                        if let Ok(after_val) = self.engine.cell_value(sheet, coord)
                            && after_val != before_val
                        {
                            self.pending_damage.push(SheetDamage::Cell { sheet, coord });
                        }
                    }
                }
                None => self.pending_damage.push(SheetDamage::All),
            }
        }

        Ok(touches_layout)
    }

    fn snapshot_formula_values(&self) -> Option<Vec<(u32, CellCoord, CellValue)>> {
        let sheet_count = self.engine.sheet_count() as u32;
        let mut out = Vec::new();
        for sheet in 0..sheet_count {
            let rem = FORMULA_DIFF_LIMIT.saturating_sub(out.len());
            let coords = self.engine.formula_cells_bounded(sheet, rem).ok()??;
            for coord in coords {
                let val = self.engine.cell_value(sheet, coord).ok()?;
                out.push((sheet, coord, val));
            }
        }
        Some(out)
    }

    /// Applies a [`Transaction`], records it on the undo stack, clears the redo stack, and bumps revisions.
    pub fn execute_transaction(&mut self, tx: Transaction) -> Result<(), EngineError> {
        if tx.ops.is_empty() {
            return Ok(());
        }
        let touches_layout = self.apply_tx_with_damage(&tx)?;
        self.expand_cached_used_range(&tx);
        self.undo_stack.push(tx);
        self.redo_stack.clear();
        self.dirty = true;
        self.revision += 1;
        if touches_layout {
            self.layout_revision += 1;
        }
        Ok(())
    }

    /// Convenience helper that builds and executes a [`SheetOp::SetCellInput`] transaction on the active sheet.
    pub fn set_cell_input(&mut self, coord: CellCoord, new_input: &str) -> Result<(), EngineError> {
        let old_input = self.engine.cell_input(self.active_sheet, coord)?;
        if old_input == new_input {
            return Ok(());
        }
        let tx = Transaction::single(
            format!("Edit {coord}"),
            SheetOp::SetCellInput {
                sheet: self.active_sheet,
                coord,
                old_input,
                new_input: new_input.to_string(),
            },
        );
        self.execute_transaction(tx)
    }

    /// Undoes the most recent transaction, returning `true` if an operation was undone.
    pub fn undo(&mut self) -> Result<bool, EngineError> {
        let Some(tx) = self.undo_stack.pop() else {
            return Ok(false);
        };
        let inv = tx.inverse();
        let touches_layout = self.apply_tx_with_damage(&inv)?;
        self.redo_stack.push(tx);
        self.dirty = !self.undo_stack.is_empty();
        self.revision += 1;
        if touches_layout {
            self.layout_revision += 1;
        }
        Ok(true)
    }

    /// Redoes the most recently undone transaction, returning `true` if an operation was redone.
    pub fn redo(&mut self) -> Result<bool, EngineError> {
        let Some(tx) = self.redo_stack.pop() else {
            return Ok(false);
        };
        let touches_layout = self.apply_tx_with_damage(&tx)?;
        self.expand_cached_used_range(&tx);
        self.undo_stack.push(tx);
        self.dirty = true;
        self.revision += 1;
        if touches_layout {
            self.layout_revision += 1;
        }
        Ok(true)
    }

    /// Atomically saves the workbook to `path` and clears the dirty flag.
    pub fn save_xlsx(&mut self, path: impl AsRef<Path>) -> Result<(), EngineError> {
        let path_ref = path.as_ref();
        self.engine.save_xlsx(path_ref)?;
        self.source_path = Some(path_ref.to_path_buf());
        self.dirty = false;
        Ok(())
    }

    /// Exports the current rectangular selection into dual TSV + HTML clipboard formats.
    pub fn copy_selection_clipboard(&self) -> Result<crate::ClipboardPayload, EngineError> {
        crate::export_range_clipboard(self, self.selection)
    }

    /// Pastes a [`crate::ClipboardGrid`] anchored at `top_left` as a single undoable [`Transaction`],
    /// updating values and preserving bold/italic/background/foreground styles when present.
    pub fn paste_clipboard_grid(
        &mut self,
        top_left: CellCoord,
        grid: &crate::ClipboardGrid,
    ) -> Result<Option<CellRange>, EngineError> {
        if grid.is_empty() {
            return Ok(None);
        }
        let mut ops = Vec::new();
        let mut max_row = top_left.row;
        let mut max_col = top_left.col;

        for (r_off, row_cells) in grid.rows.iter().enumerate() {
            let Some(row) = top_left.row.checked_add(r_off as u32) else {
                break;
            };
            if row >= MAX_ROWS {
                break;
            }
            for (c_off, cell) in row_cells.iter().enumerate() {
                let Some(col) = top_left.col.checked_add(c_off as u32) else {
                    break;
                };
                if col >= MAX_COLS {
                    break;
                }
                let coord = CellCoord::new(row, col);
                max_row = max_row.max(row);
                max_col = max_col.max(col);

                let old_input = self.engine.cell_input(self.active_sheet, coord)?;
                if old_input != cell.text {
                    ops.push(SheetOp::SetCellInput {
                        sheet: self.active_sheet,
                        coord,
                        old_input,
                        new_input: cell.text.clone(),
                    });
                }

                if cell.bold || cell.italic || cell.bg_color.is_some() || cell.fg_color.is_some() {
                    let old_style = self.engine.cell_style(self.active_sheet, coord)?;
                    let mut new_style = old_style.clone();
                    if cell.bold {
                        new_style.font.bold = true;
                    }
                    if cell.italic {
                        new_style.font.italic = true;
                    }
                    if let Some(bg) = cell.bg_color {
                        new_style.fill_color = Some(bg);
                    }
                    if let Some(fg) = cell.fg_color {
                        new_style.font.color = Some(fg);
                    }
                    if new_style != old_style {
                        ops.push(SheetOp::SetCellStyle {
                            sheet: self.active_sheet,
                            coord,
                            old_style: Box::new(old_style),
                            new_style: Box::new(new_style),
                        });
                    }
                }
            }
        }

        let pasted_range = CellRange::new(top_left, CellCoord::new(max_row, max_col));
        self.execute_transaction(Transaction::new("Paste clipboard", ops))?;
        self.set_selection(pasted_range);
        Ok(Some(pasted_range))
    }
}

impl SheetDocument {
    /// Returns the detected or assigned [`DocumentKind`].
    #[must_use]
    pub fn kind(&self) -> DocumentKind {
        self.document_kind
    }

    /// Detects the file format of `path` by extension and magic bytes and opens it as a [`SheetDocument`].
    pub fn open_auto(path: impl AsRef<Path>) -> Result<Self, DocumentError> {
        Self::open_with_progress(path, &mut |_| {})
    }

    /// Detects the file format of `path`, reports [`LoadProgress`] milestones, and loads `.xlsx`/`.xlsm`/`.csv`/`.tsv`
    /// while isolating any malformed archive/parser errors into [`DocumentError`] (never panicking).
    pub fn open_with_progress(
        path: impl AsRef<Path>,
        on_progress: &mut dyn FnMut(LoadProgress),
    ) -> Result<Self, DocumentError> {
        let path_ref = path.as_ref();
        let meta = std::fs::metadata(path_ref).map_err(|source| DocumentError::Io {
            path: path_ref.to_path_buf(),
            source,
        })?;
        let total = Some(meta.len());
        on_progress(LoadProgress::new(LoadStage::DetectingFormat, 0, total));

        let mut header = [0u8; 512];
        let mut f = std::fs::File::open(path_ref).map_err(|source| DocumentError::Io {
            path: path_ref.to_path_buf(),
            source,
        })?;
        let header_len = f.read(&mut header).map_err(|source| DocumentError::Io {
            path: path_ref.to_path_buf(),
            source,
        })?;
        let kind = detect_document_format(path_ref, &header[..header_len])?;

        on_progress(LoadProgress::new(LoadStage::ReadingBytes, 0, total));
        let bytes = std::fs::read(path_ref).map_err(|source| DocumentError::Io {
            path: path_ref.to_path_buf(),
            source,
        })?;
        on_progress(LoadProgress::new(
            LoadStage::ReadingBytes,
            bytes.len() as u64,
            total,
        ));
        on_progress(LoadProgress::new(
            LoadStage::ParsingWorkbook,
            bytes.len() as u64,
            total,
        ));

        let title = path_ref
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("Workbook.xlsx")
            .to_string();

        let mut doc = match kind {
            DocumentKind::SpreadsheetXlsx | DocumentKind::SpreadsheetXlsm => {
                let is_large = meta.len() >= UNRECALCULATED_OPEN_THRESHOLD_BYTES;
                let parsed = catch_unwind(AssertUnwindSafe(|| {
                    if is_large {
                        IronCalcEngine::from_xlsx_bytes_unrecalculated(&bytes, &title)
                    } else {
                        IronCalcEngine::from_xlsx_bytes(&bytes, &title)
                    }
                }));
                let engine = match parsed {
                    Ok(Ok(engine)) => engine,
                    Ok(Err(err)) => {
                        return Err(DocumentError::CorruptArchive {
                            path: path_ref.to_path_buf(),
                            reason: err.to_string(),
                        });
                    }
                    Err(_) => {
                        return Err(DocumentError::CorruptArchive {
                            path: path_ref.to_path_buf(),
                            reason: "unexpected parser panic on malformed OOXML archive"
                                .to_string(),
                        });
                    }
                };
                Self::from_engine(title, Box::new(engine))
            }
            DocumentKind::SpreadsheetCsv => {
                Self::from_delimited_bytes(&title, &bytes, ',', path_ref)?
            }
            DocumentKind::SpreadsheetTsv => {
                Self::from_delimited_bytes(&title, &bytes, '\t', path_ref)?
            }
            _ => {
                return Err(DocumentError::UnsupportedFormat {
                    path: path_ref.to_path_buf(),
                    detail: format!("{kind:?}"),
                });
            }
        };

        doc.document_kind = kind;
        doc.source_path = Some(path_ref.to_path_buf());
        doc.dirty = false;
        on_progress(LoadProgress::new(
            LoadStage::Ready,
            bytes.len() as u64,
            total,
        ));
        Ok(doc)
    }

    fn from_delimited_bytes(
        title: &str,
        bytes: &[u8],
        delimiter: char,
        path: &Path,
    ) -> Result<Self, DocumentError> {
        let text = decode_text_bytes(bytes);
        let mut engine =
            IronCalcEngine::new_empty(title).map_err(|e| DocumentError::ParseError {
                path: path.to_path_buf(),
                detail: e.to_string(),
            })?;
        for (r_ix, row_fields) in parse_delimited_records(&text, delimiter)
            .into_iter()
            .enumerate()
        {
            if r_ix as u32 >= MAX_ROWS {
                break;
            }
            for (c_ix, field) in row_fields.into_iter().enumerate() {
                if c_ix as u32 >= MAX_COLS {
                    break;
                }
                if !field.is_empty() {
                    engine
                        .set_cell_input_deferred(
                            0,
                            CellCoord::new(r_ix as u32, c_ix as u32),
                            &field,
                        )
                        .map_err(|e| DocumentError::ParseError {
                            path: path.to_path_buf(),
                            detail: e.to_string(),
                        })?;
                }
            }
        }
        engine.evaluate();
        Ok(Self::from_engine(title, Box::new(engine)))
    }
}

impl Document for SheetDocument {
    fn document_kind(&self) -> DocumentKind {
        self.document_kind
    }

    fn document_title(&self) -> &str {
        &self.title
    }

    fn document_path(&self) -> Option<&Path> {
        self.source_path.as_deref()
    }

    fn is_modified(&self) -> bool {
        self.dirty
    }

    fn open_from_path(
        path: &Path,
        on_progress: &mut dyn FnMut(LoadProgress),
    ) -> Result<Self, DocumentError> {
        Self::open_with_progress(path, on_progress)
    }

    fn save_to_path(&mut self, path: &Path) -> Result<(), DocumentError> {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .map(str::to_ascii_lowercase);
        match ext.as_deref() {
            Some("csv") => {
                let bytes = format_delimited_sheet(self, ',')?;
                atomic_write_bytes(path, &bytes, |_| Ok(()))?;
                self.document_kind = DocumentKind::SpreadsheetCsv;
            }
            Some("tsv") => {
                let bytes = format_delimited_sheet(self, '\t')?;
                atomic_write_bytes(path, &bytes, |_| Ok(()))?;
                self.document_kind = DocumentKind::SpreadsheetTsv;
            }
            _ => {
                let bytes = self
                    .engine
                    .to_xlsx_bytes()
                    .map_err(|e| DocumentError::SaveFailed {
                        path: path.to_path_buf(),
                        detail: e.to_string(),
                    })?;
                atomic_write_bytes(path, &bytes, |tmp| {
                    if !bytes.starts_with(b"PK\x03\x04") {
                        return Err(format!(
                            "temporary archive `{}` missing PK header",
                            tmp.display()
                        ));
                    }
                    Ok(())
                })?;
                if ext.as_deref() == Some("xlsm") {
                    self.document_kind = DocumentKind::SpreadsheetXlsm;
                } else {
                    self.document_kind = DocumentKind::SpreadsheetXlsx;
                }
            }
        }
        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
            self.title = name.to_string();
        }
        self.source_path = Some(path.to_path_buf());
        self.dirty = false;
        Ok(())
    }
}

fn decode_text_bytes(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let u16s: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .copied()
            .map(u16::from_le_bytes)
            .collect();
        return String::from_utf16_lossy(&u16s);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let u16s: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .copied()
            .map(u16::from_be_bytes)
            .collect();
        return String::from_utf16_lossy(&u16s);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn parse_delimited_records(text: &str, delimiter: char) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    let _ = chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(ch);
            }
        } else if ch == '"' && field.is_empty() {
            in_quotes = true;
        } else if ch == delimiter {
            current_row.push(std::mem::take(&mut field));
        } else if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                let _ = chars.next();
            }
            current_row.push(std::mem::take(&mut field));
            rows.push(std::mem::take(&mut current_row));
        } else if ch == '\n' {
            current_row.push(std::mem::take(&mut field));
            rows.push(std::mem::take(&mut current_row));
        } else {
            field.push(ch);
        }
    }

    if !field.is_empty() || !current_row.is_empty() {
        current_row.push(field);
        rows.push(current_row);
    }
    rows
}

fn format_delimited_sheet(doc: &SheetDocument, delimiter: char) -> Result<Vec<u8>, DocumentError> {
    let Some(used) = doc.active_sheet_info().and_then(|s| s.used_range) else {
        return Ok(Vec::new());
    };
    let mut out = String::new();
    for r in used.start.row..=used.end.row {
        for c in used.start.col..=used.end.col {
            if c > used.start.col {
                out.push(delimiter);
            }
            let val = doc
                .cell_display_text(CellCoord::new(r, c))
                .unwrap_or_default();
            if val.contains(delimiter)
                || val.contains('"')
                || val.contains('\n')
                || val.contains('\r')
            {
                out.push('"');
                for ch in val.chars() {
                    if ch == '"' {
                        out.push_str("\"\"");
                    } else {
                        out.push(ch);
                    }
                }
                out.push('"');
            } else {
                out.push_str(&val);
            }
        }
        out.push('\n');
    }
    Ok(out.into_bytes())
}
