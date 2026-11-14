//! Roundtrip byte-identity and hazard-warning tests for `ordia-xlsx` unknown-parts preservation.

use ordia_sheet_engine::{CellCoord, CellRange, CellValue};
use ordia_xlsx::fixtures::{
    build_complex_chart_xlsx, build_macro_vba_xlsm, build_pivot_table_xlsx, build_slicer_xlsx,
    extract_unknown_part_bytes,
};
use ordia_xlsx::{PreservationHazardKind, XlsxWorkbook, atomic_write_xlsx, parse_relationships};
use std::collections::BTreeSet;
use std::io::{Cursor, Read as _};
use std::path::{Path, PathBuf};

fn workspace_corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .join("corpus")
        .join("public")
}

type FixtureBuilder = fn() -> Result<Vec<u8>, ordia_xlsx::XlsxError>;

fn ensure_corpus_fixtures_exist() -> PathBuf {
    let dir = workspace_corpus_dir();
    let specs: [(&str, FixtureBuilder); 4] = [
        ("m0_pivot_table.xlsx", build_pivot_table_xlsx),
        ("m0_macro_vba.xlsm", build_macro_vba_xlsm),
        ("m0_slicer.xlsx", build_slicer_xlsx),
        ("m0_complex_chart.xlsx", build_complex_chart_xlsx),
    ];
    for (filename, builder) in specs {
        let target = dir.join(filename);
        if !target.exists() {
            let bytes = builder().expect("build deterministic OOXML fixture");
            atomic_write_xlsx(&target, &bytes).expect("write corpus fixture");
        }
    }
    dir
}

fn read_zip_entry_string(zip_bytes: &[u8], entry_name: &str) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(zip_bytes)).expect("open zip");
    let mut file = archive.by_name(entry_name).expect("find zip entry");
    let mut s = String::new();
    file.read_to_string(&mut s).expect("read utf8 xml");
    s
}

fn assert_rels_have_unique_ids(zip_bytes: &[u8], entry_name: &str) {
    let mut archive = zip::ZipArchive::new(Cursor::new(zip_bytes)).expect("open zip");
    let Ok(mut file) = archive.by_name(entry_name) else {
        return;
    };
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).expect("read rels bytes");
    let rels = parse_relationships(&buf).expect("parse rels xml");
    let mut seen = BTreeSet::new();
    for r in &rels {
        assert!(
            seen.insert(r.orig_id.clone()),
            "duplicate Relationship Id `{}` in `{entry_name}`",
            r.orig_id
        );
    }
}

#[test]
fn roundtrip_preserves_pivot_vba_slicer_and_chart_parts_byte_for_byte() {
    let corpus_dir = ensure_corpus_fixtures_exist();
    let temp_dir = std::env::temp_dir().join(format!("ordia_xlsx_test_{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).expect("create temp dir");

    let cases = [
        ("m0_pivot_table.xlsx", "xl/pivotTables/pivotTable1.xml"),
        ("m0_macro_vba.xlsm", "xl/vbaProject.bin"),
        ("m0_slicer.xlsx", "xl/slicers/slicer1.xml"),
        ("m0_complex_chart.xlsx", "xl/charts/chart1.xml"),
    ];

    for (file_name, must_contain_part) in cases {
        let src_path = corpus_dir.join(file_name);
        let orig_file_bytes = std::fs::read(&src_path).expect("read original corpus file");
        let orig_unknown =
            extract_unknown_part_bytes(&orig_file_bytes).expect("extract original unknown parts");
        assert!(
            orig_unknown.contains_key(must_contain_part),
            "{file_name} must contain {must_contain_part}"
        );

        // Hop 1: Open -> edit regular cells -> save atomically -> verify zero warnings.
        let mut wb = XlsxWorkbook::open_path(&src_path).expect("open workbook with ordia-xlsx");
        assert_eq!(
            wb.engine()
                .cell_value(0, CellCoord::new(5, 2))
                .expect("read initial SUM"),
            CellValue::Number(4700.0)
        );
        wb.engine_mut()
            .set_cell_input(0, CellCoord::new(1, 2), "2000")
            .expect("edit C2 revenue");
        wb.engine_mut()
            .set_cell_input(0, CellCoord::new(1, 3), "已审核")
            .expect("edit D2 note");

        let out_path_1 = temp_dir.join(format!("hop1_{file_name}"));
        let warnings_1 = wb.save_path(&out_path_1).expect("atomic save hop 1");
        assert!(
            warnings_1.is_empty(),
            "normal cell edit on {file_name} should produce zero warnings, got {warnings_1:?}"
        );

        let hop1_bytes = std::fs::read(&out_path_1).expect("read hop1 saved bytes");
        let hop1_unknown =
            extract_unknown_part_bytes(&hop1_bytes).expect("extract hop1 unknown parts");
        assert_eq!(
            orig_unknown, hop1_unknown,
            "unknown parts in {file_name} must be 100% byte-identical after hop 1"
        );
        assert_rels_have_unique_ids(&hop1_bytes, "xl/_rels/workbook.xml.rels");
        assert_rels_have_unique_ids(&hop1_bytes, "xl/worksheets/_rels/sheet1.xml.rels");

        // Hop 2: Re-open hop1 file -> edit again -> save -> verify multi-hop byte identity & formula recalc!
        let mut wb2 = XlsxWorkbook::open_path(&out_path_1).expect("reopen hop1 file");
        assert_eq!(
            wb2.engine()
                .cell_value(0, CellCoord::new(5, 2))
                .expect("read recalculated SUM after hop1"),
            CellValue::Number(5500.0)
        );
        assert_eq!(
            wb2.engine()
                .cell_value(0, CellCoord::new(1, 3))
                .expect("read D2 note"),
            CellValue::Text("已审核".into())
        );
        wb2.engine_mut()
            .set_cell_input(0, CellCoord::new(2, 2), "1050")
            .expect("edit C3 on hop 2");

        let out_path_2 = temp_dir.join(format!("hop2_{file_name}"));
        let warnings_2 = wb2.save_path(&out_path_2).expect("atomic save hop 2");
        assert!(warnings_2.is_empty());

        let hop2_bytes = std::fs::read(&out_path_2).expect("read hop2 saved bytes");
        let hop2_unknown =
            extract_unknown_part_bytes(&hop2_bytes).expect("extract hop2 unknown parts");
        assert_eq!(
            orig_unknown, hop2_unknown,
            "unknown parts in {file_name} must remain 100% byte-identical after hop 2"
        );
    }

    // Verify specific OPC glue invariants on `.xlsm`, Pivot, Slicer, and Chart outputs.
    let xlsm_bytes = std::fs::read(temp_dir.join("hop2_m0_macro_vba.xlsm")).expect("read xlsm");
    let xlsm_ct = read_zip_entry_string(&xlsm_bytes, "[Content_Types].xml");
    assert!(xlsm_ct.contains("application/vnd.ms-excel.sheet.macroEnabled.main+xml"));
    assert!(xlsm_ct.contains(r#"Extension="bin""#));
    let xlsm_wb = read_zip_entry_string(&xlsm_bytes, "xl/workbook.xml");
    assert!(xlsm_wb.contains(r#"codeName="ThisWorkbook""#));

    let pivot_bytes = std::fs::read(temp_dir.join("hop2_m0_pivot_table.xlsx")).expect("read pivot");
    let pivot_wb = read_zip_entry_string(&pivot_bytes, "xl/workbook.xml");
    assert!(pivot_wb.contains("<pivotCaches><pivotCache cacheId=\"1\""));

    let slicer_bytes = std::fs::read(temp_dir.join("hop2_m0_slicer.xlsx")).expect("read slicer");
    let slicer_ws = read_zip_entry_string(&slicer_bytes, "xl/worksheets/sheet1.xml");
    assert!(slicer_ws.contains("<x14:slicerList>"));

    let chart_bytes =
        std::fs::read(temp_dir.join("hop2_m0_complex_chart.xlsx")).expect("read chart");
    let chart_ws = read_zip_entry_string(&chart_bytes, "xl/worksheets/sheet1.xml");
    assert!(chart_ws.contains("<drawing r:id=\""));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn hazard_detector_flags_deleted_or_cleared_preserved_sources() {
    let corpus_dir = ensure_corpus_fixtures_exist();

    // 1. Renaming Sheet1 breaks PivotCache worksheet reference -> warning emitted!
    let mut pivot_wb =
        XlsxWorkbook::open_path(corpus_dir.join("m0_pivot_table.xlsx")).expect("open pivot");
    pivot_wb
        .engine_mut()
        .rename_sheet(0, "SalesRenamed")
        .expect("rename Sheet1");
    let w_rename = pivot_wb.check_warnings();
    assert!(
        w_rename.iter().any(|w| matches!(
            &w.kind,
            PreservationHazardKind::PivotSourceSheetMissing { expected_sheet, .. }
                if expected_sheet == "Sheet1"
        )),
        "expected PivotSourceSheetMissing warning, got {w_rename:?}"
    );

    // 2. Clearing all cells in Sheet1!A1:C5 -> PivotSourceRangeCleared warning emitted!
    let mut pivot_wb2 =
        XlsxWorkbook::open_path(corpus_dir.join("m0_pivot_table.xlsx")).expect("open pivot 2");
    pivot_wb2
        .engine_mut()
        .clear_range_contents(
            0,
            CellRange::new(CellCoord::new(0, 0), CellCoord::new(4, 2)),
        )
        .expect("clear A1:C5");
    let w_clear = pivot_wb2.check_warnings();
    assert!(
        w_clear.iter().any(|w| matches!(
            &w.kind,
            PreservationHazardKind::PivotSourceRangeCleared { .. }
        )),
        "expected PivotSourceRangeCleared warning, got {w_clear:?}"
    );

    // 3. Deleting Sheet1 on complex chart workbook -> WorksheetWithAttachmentsDeleted + ChartSourceSheetMissing!
    let mut chart_wb =
        XlsxWorkbook::open_path(corpus_dir.join("m0_complex_chart.xlsx")).expect("open chart");
    let _ = chart_wb
        .engine_mut()
        .add_sheet(Some("Summary"))
        .expect("add Sheet2");
    chart_wb
        .engine_mut()
        .delete_sheet(0)
        .expect("delete Sheet1");
    let w_del = chart_wb.check_warnings();
    assert!(
        w_del.iter().any(|w| matches!(
            &w.kind,
            PreservationHazardKind::WorksheetWithAttachmentsDeleted { .. }
        )),
        "expected WorksheetWithAttachmentsDeleted, got {w_del:?}"
    );
    assert!(
        w_del.iter().any(|w| matches!(
            &w.kind,
            PreservationHazardKind::ChartSourceSheetMissing { .. }
        )),
        "expected ChartSourceSheetMissing, got {w_del:?}"
    );
}
