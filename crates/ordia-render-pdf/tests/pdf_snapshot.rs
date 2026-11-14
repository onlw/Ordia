//! Snapshot and CJK font subsetting tests for `ordia-render-pdf`.

use ordia_render_pdf::{
    PdfExportOptions, build_sheet_range_display_list, export_sheet_range_to_pdf_bytes,
    export_sheet_range_to_pdf_file, inspect_pdf_structure,
};
use ordia_sheet_model::{
    BorderEdge, BorderLineStyle, CellCoord, CellRange, CellStyle, ColorRgba, SheetDocument,
    SheetOp, Transaction,
};
use ordia_text::FontCatalog;
use serde::Serialize;

#[derive(Serialize)]
struct SnapshotReport {
    display_list_width: f32,
    display_list_height: f32,
    balanced_clips: bool,
    command_count: usize,
    pdf_structure: ordia_render_pdf::PdfStructureSummary,
}

fn build_chinese_budget_doc() -> SheetDocument {
    let mut doc = SheetDocument::new_empty("Budget2026.xlsx").expect("new doc");
    let headers = [
        (0, "项目类别"),
        (1, "一季度预算"),
        (2, "二季度预算"),
        (3, "上半年合计"),
    ];
    for (col, label) in headers {
        doc.set_cell_input(CellCoord::new(0, col), label)
            .expect("set header");
    }

    let rows = [
        (1, "云基础设施", "128000", "142500", "=B2+C2"),
        (2, "研发工具采购", "64000", "71000", "=B3+C3"),
        (3, "总计", "=SUM(B2:B3)", "=SUM(C2:C3)", "=SUM(D2:D3)"),
    ];
    for (row, name, q1, q2, sum_formula) in rows {
        doc.set_cell_input(CellCoord::new(row, 0), name)
            .expect("set name");
        doc.set_cell_input(CellCoord::new(row, 1), q1)
            .expect("set q1");
        doc.set_cell_input(CellCoord::new(row, 2), q2)
            .expect("set q2");
        doc.set_cell_input(CellCoord::new(row, 3), sum_formula)
            .expect("set formula");
    }

    // Style header row (bold + pale blue fill + bottom border) via Transaction
    let mut header_style = CellStyle::default();
    header_style.font.bold = true;
    header_style.fill_color = Some(ColorRgba::rgb(232, 240, 254));
    header_style.borders.bottom = Some(BorderEdge {
        style: BorderLineStyle::Medium,
        color: Some(ColorRgba::rgb(59, 130, 246)),
    });

    let mut total_style = CellStyle::default();
    total_style.font.bold = true;
    total_style.fill_color = Some(ColorRgba::rgb(248, 250, 252));

    let mut ops = Vec::new();
    for col in 0..=3 {
        let coord = CellCoord::new(0, col);
        ops.push(SheetOp::SetCellStyle {
            sheet: 0,
            coord,
            old_style: Box::new(doc.cell_style(coord).expect("old style")),
            new_style: Box::new(header_style.clone()),
        });
        let total_coord = CellCoord::new(3, col);
        ops.push(SheetOp::SetCellStyle {
            sheet: 0,
            coord: total_coord,
            old_style: Box::new(doc.cell_style(total_coord).expect("old total style")),
            new_style: Box::new(total_style.clone()),
        });
    }
    doc.execute_transaction(Transaction::new("format_budget", ops))
        .expect("apply styles");

    doc
}

#[test]
fn chinese_sheet_pdf_export_subsets_fonts_and_matches_structural_snapshot() {
    let doc = build_chinese_budget_doc();
    let range = CellRange::new(CellCoord::new(0, 0), CellCoord::new(3, 3));
    let options = PdfExportOptions::default();

    let dl = build_sheet_range_display_list(&doc, range, &options).expect("build display list");
    assert!(dl.has_balanced_clips());

    let fonts = FontCatalog::from_system();
    let pdf_bytes =
        export_sheet_range_to_pdf_bytes(&doc, range, &options, &fonts).expect("render pdf");

    // Verify deterministic byte-for-byte reproducibility across two runs on the same catalog
    let pdf_bytes_second =
        export_sheet_range_to_pdf_bytes(&doc, range, &options, &fonts).expect("render pdf 2");
    assert_eq!(
        pdf_bytes, pdf_bytes_second,
        "PDF output must be deterministic across repeated exports"
    );

    // Full CJK system font files are 10-30 MiB; subsetted PDF must stay compact (< 512 KiB)
    assert!(
        pdf_bytes.len() > 512 && pdf_bytes.len() < 512 * 1024,
        "unexpected PDF byte length {} (font subsetting may not have triggered)",
        pdf_bytes.len()
    );

    let summary = inspect_pdf_structure(&pdf_bytes);
    assert!(summary.has_eof_marker);
    assert_eq!(summary.page_count, 1);
    assert!(
        summary.has_to_unicode_cmap,
        "PDF must embed /ToUnicode CMap so Chinese text is selectable and searchable"
    );
    assert!(
        summary.has_cid_font_subset,
        "PDF must embed CID-keyed subsetted font"
    );
    // Normalize font_descriptor_count to >= 1 check + fixed cap for cross-OS snapshot stability
    // (macOS PingFang SC uses 2 faces for regular + bold; Linux CI with single face may use 1-2)
    assert!(summary.font_descriptor_count >= 1 && summary.font_descriptor_count <= 4);

    let mut normalized_summary = summary;
    normalized_summary.font_descriptor_count =
        usize::from(normalized_summary.font_descriptor_count >= 1);

    let report = SnapshotReport {
        display_list_width: dl.width,
        display_list_height: dl.height,
        balanced_clips: dl.has_balanced_clips(),
        command_count: dl.commands.len(),
        pdf_structure: normalized_summary,
    };

    insta::assert_yaml_snapshot!("chinese_budget_pdf_structure", report);

    // Also test atomic file export
    let dir = tempfile::tempdir().expect("tempdir");
    let out_path = dir.path().join("budget_2026.pdf");
    export_sheet_range_to_pdf_file(&doc, range, &options, &fonts, &out_path)
        .expect("atomic pdf write");
    let disk_bytes = std::fs::read(&out_path).expect("read exported pdf");
    assert_eq!(disk_bytes, pdf_bytes);
}
