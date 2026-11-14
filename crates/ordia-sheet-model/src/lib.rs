//! Workbook domain model, selection state, reversible operations, transactions, and undo/redo history.

mod clipboard;
mod document;
mod ops;

pub use clipboard::{
    ClipboardCell, ClipboardGrid, ClipboardPayload, export_range_clipboard,
    parse_clipboard_payload, parse_html_table, parse_tsv,
};
pub use document::{SheetDamage, SheetDocument};
pub use ops::{SheetOp, Transaction};
pub use ordia_core::{Document, DocumentError, DocumentKind, LoadProgress, LoadStage};

pub use ordia_sheet_engine::{
    BorderEdge, BorderLineStyle, CellBorders, CellComment, CellCoord, CellHyperlink, CellRange,
    CellStyle, CellValue, ColSpanMetric, ColorRgba, DEFAULT_COL_WIDTH_PX, DEFAULT_ROW_HEIGHT_PX,
    DefinedNameInfo, EngineError, FontStyle, FormattedValue, FormulaError, FreezePane,
    HorizontalAlign, IronCalcEngine, MAX_COLS, MAX_ROWS, RowMetric, SheetEngine, SheetInfo,
    SheetVisibility, VerticalAlign, col_to_letters, format_excel_number_str, format_excel_value,
    format_general_number, letters_to_col_1based,
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

    #[test]
    fn clipboard_roundtrip_preserves_values_bold_and_background_with_excel_and_wps() {
        let mut src = SheetDocument::new_demo("Source.xlsx").expect("demo");
        // Demo row 0 has bold + #F1F5F9 background fill
        src.set_selection(CellRange::new(CellCoord::new(0, 0), CellCoord::new(1, 2)));
        let payload = src.copy_selection_clipboard().expect("copy");

        assert!(
            payload
                .tsv
                .starts_with("项目 / Item\tQ1\tQ2\n订阅收入 / Subscription\t1280\t1420")
        );
        assert!(payload.html.contains("font-weight:bold;"));
        assert!(payload.html.contains("background-color:#F1F5F9;"));

        // Paste Ordia's HTML into a fresh workbook and verify values + bold + background + undo
        let mut dst = SheetDocument::new_empty("Dest.xlsx").expect("dest");
        let parsed = parse_clipboard_payload(Some(&payload.html), Some(&payload.tsv));
        let pasted = dst
            .paste_clipboard_grid(CellCoord::new(2, 1), &parsed)
            .expect("paste")
            .expect("non-empty range");
        assert_eq!(
            pasted,
            CellRange::new(CellCoord::new(2, 1), CellCoord::new(3, 3))
        );
        assert_eq!(
            dst.cell_display_text(CellCoord::new(2, 1)).expect("B3"),
            "项目 / Item"
        );
        assert_eq!(
            dst.cell_display_text(CellCoord::new(3, 2)).expect("C4"),
            "1280"
        );
        let header_pasted_style = dst.cell_style(CellCoord::new(2, 1)).expect("style B3");
        assert!(header_pasted_style.font.bold);
        assert_eq!(
            header_pasted_style.fill_color,
            Some(ColorRgba::rgb(241, 245, 249))
        );

        // Single undo reverts the entire pasted range atomically
        assert!(dst.undo().expect("undo paste"));
        assert_eq!(dst.cell_display_text(CellCoord::new(2, 1)).expect("B3"), "");

        // Also verify Microsoft Excel's `<style>.xl65{font-weight:700;background:#FFD966;}</style>` format
        let excel_html = r#"
            <html><head><style>
            .xl65 { font-weight:700; background:#FFD966; color:#1F2937; }
            </style></head><body>
            <table>
              <tr><td class=xl65>华东大区</td><td>48500</td></tr>
            </table></body></html>
        "#;
        let excel_grid = parse_clipboard_payload(Some(excel_html), None);
        assert_eq!(excel_grid.rows.len(), 1);
        assert_eq!(excel_grid.rows[0][0].text, "华东大区");
        assert!(excel_grid.rows[0][0].bold);
        assert_eq!(
            excel_grid.rows[0][0].bg_color,
            Some(ColorRgba::rgb(0xFF, 0xD9, 0x66))
        );
        assert_eq!(excel_grid.rows[0][1].text, "48500");

        // Verify TSV fallback with quoted multi-line cells
        let tsv_grid =
            parse_clipboard_payload(None, Some("\"备注A\n第二行\"\t99.5\r\n普通\t100\r\n"));
        assert_eq!(tsv_grid.rows.len(), 2);
        assert_eq!(tsv_grid.rows[0][0].text, "备注A\n第二行");
        assert_eq!(tsv_grid.rows[0][1].text, "99.5");
    }

    #[test]
    fn document_trait_handles_corrupt_xlsx_and_roundtrips_xlsx_and_csv() {
        let dir = tempfile::tempdir().expect("tempdir");

        // 1. Corrupt ZIP with valid PK magic but truncated body never panics; returns CorruptArchive.
        let corrupt_xlsx = dir.path().join("corrupt.xlsx");
        std::fs::write(
            &corrupt_xlsx,
            b"PK\x03\x04\x14\x00truncated-garbage-payload",
        )
        .expect("write corrupt xlsx");
        let err = SheetDocument::open_auto(&corrupt_xlsx);
        assert!(matches!(err, Err(DocumentError::CorruptArchive { .. })));

        // 2. Atomic save + open_from_path with LoadProgress stages on `.xlsx`.
        let mut doc = SheetDocument::new_demo("Quarterly.xlsx").expect("demo");
        doc.set_cell_input(CellCoord::new(0, 0), "营收")
            .expect("edit A1");
        assert!(doc.is_modified());

        let saved_xlsx = dir.path().join("saved_roundtrip.xlsx");
        doc.save_to_path(&saved_xlsx).expect("atomic save xlsx");
        assert!(!doc.is_modified());
        assert_eq!(doc.document_kind(), DocumentKind::SpreadsheetXlsx);

        let mut stages = Vec::new();
        let reopened = SheetDocument::open_from_path(&saved_xlsx, &mut |p| {
            stages.push(p.stage);
        })
        .expect("reopen saved xlsx");
        assert_eq!(stages.first(), Some(&LoadStage::DetectingFormat));
        assert_eq!(stages.last(), Some(&LoadStage::Ready));
        assert_eq!(
            reopened
                .cell_display_text(CellCoord::new(0, 0))
                .expect("A1"),
            "营收"
        );

        // 3. CSV roundtrip via `Document` trait.
        let saved_csv = dir.path().join("export.csv");
        let mut csv_src = reopened;
        csv_src.save_to_path(&saved_csv).expect("save csv");
        let csv_loaded = SheetDocument::open_auto(&saved_csv).expect("open csv");
        assert_eq!(csv_loaded.document_kind(), DocumentKind::SpreadsheetCsv);
        assert_eq!(
            csv_loaded
                .cell_display_text(CellCoord::new(0, 0))
                .expect("csv A1"),
            "营收"
        );
    }
}
