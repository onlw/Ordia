//! Reversible operations and transactions for spreadsheet mutations (`AGENTS.md` §4.3).

use ordia_sheet_engine::{CellCoord, CellStyle, EngineError, FreezePane, SheetEngine};
use serde::{Deserialize, Serialize};

/// Atomic, reversible mutation applied to a [`SheetEngine`].
///
/// Every user or programmatic edit to a workbook must be represented as a [`SheetOp`] inside a
/// [`Transaction`] so undo/redo, operation journaling (`M1-07`), and dirty tracking stay consistent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SheetOp {
    /// Sets the raw input string (literal or `=formula`) of a single cell.
    SetCellInput {
        /// Zero-based worksheet index.
        sheet: u32,
        /// Target cell coordinate.
        coord: CellCoord,
        /// Previous raw input string (`""` if previously empty).
        old_input: String,
        /// New raw input string (`""` to clear).
        new_input: String,
    },
    /// Updates the resolved style of a single cell.
    SetCellStyle {
        /// Zero-based worksheet index.
        sheet: u32,
        /// Target cell coordinate.
        coord: CellCoord,
        /// Previous cell style.
        old_style: Box<CellStyle>,
        /// New cell style.
        new_style: Box<CellStyle>,
    },
    /// Updates frozen row and column counts on a worksheet.
    SetFreezePane {
        /// Zero-based worksheet index.
        sheet: u32,
        /// Previous freeze pane configuration.
        old_freeze: FreezePane,
        /// New freeze pane configuration.
        new_freeze: FreezePane,
    },
    /// Sets explicit height of a row in logical pixels.
    SetRowHeight {
        /// Zero-based worksheet index.
        sheet: u32,
        /// Zero-based row index.
        row: u32,
        /// Previous height in logical pixels.
        old_height_px: f32,
        /// New height in logical pixels.
        new_height_px: f32,
    },
    /// Sets explicit width of a column in logical pixels.
    SetColWidth {
        /// Zero-based worksheet index.
        sheet: u32,
        /// Zero-based column index.
        col: u32,
        /// Previous width in logical pixels.
        old_width_px: f32,
        /// New width in logical pixels.
        new_width_px: f32,
    },
}

impl SheetOp {
    /// Returns the exact inverse operation that undoes `self`.
    #[must_use]
    pub fn inverse(&self) -> Self {
        match self {
            Self::SetCellInput {
                sheet,
                coord,
                old_input,
                new_input,
            } => Self::SetCellInput {
                sheet: *sheet,
                coord: *coord,
                old_input: new_input.clone(),
                new_input: old_input.clone(),
            },
            Self::SetCellStyle {
                sheet,
                coord,
                old_style,
                new_style,
            } => Self::SetCellStyle {
                sheet: *sheet,
                coord: *coord,
                old_style: new_style.clone(),
                new_style: old_style.clone(),
            },
            Self::SetFreezePane {
                sheet,
                old_freeze,
                new_freeze,
            } => Self::SetFreezePane {
                sheet: *sheet,
                old_freeze: *new_freeze,
                new_freeze: *old_freeze,
            },
            Self::SetRowHeight {
                sheet,
                row,
                old_height_px,
                new_height_px,
            } => Self::SetRowHeight {
                sheet: *sheet,
                row: *row,
                old_height_px: *new_height_px,
                new_height_px: *old_height_px,
            },
            Self::SetColWidth {
                sheet,
                col,
                old_width_px,
                new_width_px,
            } => Self::SetColWidth {
                sheet: *sheet,
                col: *col,
                old_width_px: *new_width_px,
                new_width_px: *old_width_px,
            },
        }
    }

    /// Executes this operation against `engine` without recalculating (caller triggers recalculation
    /// once at the end of a [`Transaction`] when needed).
    pub(crate) fn apply_deferred(&self, engine: &mut dyn SheetEngine) -> Result<bool, EngineError> {
        match self {
            Self::SetCellInput {
                sheet,
                coord,
                new_input,
                ..
            } => {
                engine.set_cell_input_deferred(*sheet, *coord, new_input)?;
                Ok(true)
            }
            Self::SetCellStyle {
                sheet,
                coord,
                new_style,
                ..
            } => {
                engine.set_cell_style(*sheet, *coord, new_style)?;
                Ok(false)
            }
            Self::SetFreezePane {
                sheet, new_freeze, ..
            } => {
                engine.set_freeze_pane(*sheet, *new_freeze)?;
                Ok(false)
            }
            Self::SetRowHeight {
                sheet,
                row,
                new_height_px,
                ..
            } => {
                engine.set_row_height(*sheet, *row, *new_height_px)?;
                Ok(false)
            }
            Self::SetColWidth {
                sheet,
                col,
                new_width_px,
                ..
            } => {
                engine.set_col_width(*sheet, *col, *new_width_px)?;
                Ok(false)
            }
        }
    }
}

/// Group of one or more [`SheetOp`] mutations treated as a single undoable user step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transaction {
    /// Human-readable label describing the edit (e.g. `"Edit B2"`).
    pub label: String,
    /// Ordered sequence of operations in this transaction.
    pub ops: Vec<SheetOp>,
}

impl Transaction {
    /// Creates a transaction containing a single [`SheetOp`].
    #[must_use]
    pub fn single(label: impl Into<String>, op: SheetOp) -> Self {
        Self {
            label: label.into(),
            ops: vec![op],
        }
    }

    /// Creates a transaction from multiple [`SheetOp`] items.
    #[must_use]
    pub fn new(label: impl Into<String>, ops: Vec<SheetOp>) -> Self {
        Self {
            label: label.into(),
            ops,
        }
    }

    /// Returns the inverse transaction with operations reversed and inverted.
    #[must_use]
    pub fn inverse(&self) -> Self {
        let ops = self.ops.iter().rev().map(SheetOp::inverse).collect();
        Self {
            label: self.label.clone(),
            ops,
        }
    }

    /// Applies every operation in order and recalculates formulas once if any operation mutated values.
    pub(crate) fn apply(&self, engine: &mut dyn SheetEngine) -> Result<(), EngineError> {
        let mut needs_eval = false;
        for op in &self.ops {
            if op.apply_deferred(engine)? {
                needs_eval = true;
            }
        }
        if needs_eval {
            engine.evaluate();
        }
        Ok(())
    }
}
