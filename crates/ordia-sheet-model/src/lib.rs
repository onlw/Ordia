//! Workbook domain model, selection state, reversible operations, transactions, and undo/redo history.

mod document;
mod ops;

pub use document::SheetDocument;
pub use ops::{SheetOp, Transaction};

pub use ordia_sheet_engine::{
    BorderEdge, BorderLineStyle, CellBorders, CellComment, CellCoord, CellHyperlink, CellRange,
    CellStyle, CellValue, ColSpanMetric, ColorRgba, DEFAULT_COL_WIDTH_PX, DEFAULT_ROW_HEIGHT_PX,
    DefinedNameInfo, EngineError, FontStyle, FormulaError, FreezePane, HorizontalAlign,
    IronCalcEngine, MAX_COLS, MAX_ROWS, RowMetric, SheetEngine, SheetInfo, SheetVisibility,
    VerticalAlign, col_to_letters, letters_to_col_1based,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_cell_input_recalculates_and_supports_undo_redo() {
        let mut doc = SheetDocument::new_empty("ModelTest.xlsx").expect("create empty doc");
        doc.set_cell_input(CellCoord::new(0, 0), "18")
            .expect("set A1");
        doc.set_cell_input(CellCoord::new(0, 1), "24")
            .expect("set B1");
        doc.set_cell_input(CellCoord::new(0, 2), "=A1+B1")
            .expect("set C1 formula");

        assert_eq!(
            doc.cell_display_text(CellCoord::new(0, 2))
                .expect("read C1"),
            "42"
        );
        assert!(doc.is_dirty());

        // Update A1 -> C1 recalculates
        doc.set_cell_input(CellCoord::new(0, 0), "100")
            .expect("update A1");
        assert_eq!(
            doc.cell_display_text(CellCoord::new(0, 2))
                .expect("read updated C1"),
            "124"
        );

        // Undo restores A1=18 and recalculates C1=42
        assert!(doc.undo().expect("undo"));
        assert_eq!(
            doc.cell_display_text(CellCoord::new(0, 2))
                .expect("read undone C1"),
            "42"
        );

        // Redo restores A1=100 and recalculates C1=124
        assert!(doc.redo().expect("redo"));
        assert_eq!(
            doc.cell_display_text(CellCoord::new(0, 2))
                .expect("read redone C1"),
            "124"
        );
    }

    #[test]
    fn demo_workbook_has_frozen_panes_and_evaluated_totals() {
        let doc = SheetDocument::new_demo("Demo.xlsx").expect("demo doc");
        assert!(!doc.is_dirty());
        assert_eq!(
            doc.freeze_pane().expect("freeze"),
            FreezePane {
                frozen_rows: 1,
                frozen_cols: 1,
            }
        );
        // Row 1 ("Subscription"): 1280 + 1420 + 1590 + 1810 = 6100
        assert_eq!(
            doc.cell_display_text(CellCoord::new(1, 5))
                .expect("row total"),
            "6100"
        );
    }
}
