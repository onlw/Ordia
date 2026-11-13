//! Integration and round-trip tests for `ordia-sheet-engine`.

use ordia_sheet_engine::{
    BorderEdge, BorderLineStyle, CellCoord, CellRange, CellStyle, CellValue, ColorRgba,
    FormulaError, FreezePane, HorizontalAlign, IronCalcEngine, SheetEngine, SheetVisibility,
    VerticalAlign,
};
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root two levels above crate manifest")
        .to_path_buf()
}

fn build_budget_sample_engine() -> Result<IronCalcEngine, ordia_sheet_engine::EngineError> {
    let mut engine = IronCalcEngine::new_empty("m0_budget_sample")?;
    engine.rename_sheet(0, "预算总览")?;
    let summary_sheet = 0;

    // Merged title banner A1:D1
    engine.set_cell_input_deferred(summary_sheet, CellCoord::new(0, 0), "2026 年度研发预算表")?;
    let mut banner_style = CellStyle::default();
    banner_style.font.bold = true;
    banner_style.font.size_pt = 14.0;
    banner_style.font.color = Some(ColorRgba::rgb(255, 255, 255));
    banner_style.fill_color = Some(ColorRgba::rgb(31, 78, 121));
    banner_style.horizontal_align = HorizontalAlign::Center;
    banner_style.vertical_align = VerticalAlign::Center;
    banner_style.borders.bottom = Some(BorderEdge {
        style: BorderLineStyle::Medium,
        color: Some(ColorRgba::rgb(15, 36, 62)),
    });
    engine.set_cell_style(summary_sheet, CellCoord::new(0, 0), &banner_style)?;
    engine.merge_range(
        summary_sheet,
        CellRange::new(CellCoord::new(0, 0), CellCoord::new(0, 3)),
    )?;

    // Header row (row 1 -> Excel row 2)
    for (col, title) in ["科目", "Q1 预算", "Q2 预算", "半年合计"]
        .into_iter()
        .enumerate()
    {
        engine.set_cell_input_deferred(summary_sheet, CellCoord::new(1, col as u32), title)?;
    }
    engine.set_freeze_pane(
        summary_sheet,
        FreezePane {
            frozen_rows: 2,
            frozen_cols: 1,
        },
    )?;

    // Data rows (Excel rows 3..=5)
    let items = [
        ("基础设施", "12000", "13500", "=B3+C3"),
        ("客户端研发", "48000", "52000", "=B4+C4"),
        ("质量保障", "15000", "16500", "=B5+C5"),
    ];
    for (idx, (name, q1, q2, total_formula)) in items.into_iter().enumerate() {
        let r = (idx + 2) as u32;
        engine.set_cell_input_deferred(summary_sheet, CellCoord::new(r, 0), name)?;
        engine.set_cell_input_deferred(summary_sheet, CellCoord::new(r, 1), q1)?;
        engine.set_cell_input_deferred(summary_sheet, CellCoord::new(r, 2), q2)?;
        engine.set_cell_input_deferred(summary_sheet, CellCoord::new(r, 3), total_formula)?;
    }

    // Grand total row (Excel row 6)
    engine.set_cell_input_deferred(summary_sheet, CellCoord::new(5, 0), "总计")?;
    engine.set_cell_input_deferred(summary_sheet, CellCoord::new(5, 1), "=SUM(B3:B5)")?;
    engine.set_cell_input_deferred(summary_sheet, CellCoord::new(5, 2), "=SUM(C3:C5)")?;
    engine.set_cell_input_deferred(summary_sheet, CellCoord::new(5, 3), "=SUM(D3:D5)")?;

    // Defined name + second worksheet referencing it
    engine.set_defined_name("HalfYearTotal", None, "='预算总览'!$D$6")?;
    let kpi_sheet = engine.add_sheet(Some("KPI"))?;
    engine.set_cell_input_deferred(kpi_sheet, CellCoord::new(0, 0), "税后总预算")?;
    engine.set_cell_input_deferred(kpi_sheet, CellCoord::new(0, 1), "=HalfYearTotal*1.06")?;
    engine.evaluate();
    Ok(engine)
}

#[test]
fn corpus_sample_load_modify_save_and_reload_roundtrip() {
    let sample_path = workspace_root().join("corpus/public/m0_budget_sample.xlsx");
    if !sample_path.exists() {
        let mut seed = build_budget_sample_engine().expect("seed sample workbook");
        seed.save_xlsx(&sample_path)
            .expect("write corpus/public/m0_budget_sample.xlsx");
    }

    // 1. Load from corpus
    let mut loaded = IronCalcEngine::from_xlsx_path(&sample_path).expect("load corpus sample");
    let sheets = loaded.sheets();
    assert_eq!(sheets.len(), 2);
    assert_eq!(sheets[0].name, "预算总览");
    assert_eq!(sheets[1].name, "KPI");

    // Verify freeze panes & merged ranges
    assert_eq!(
        loaded.freeze_pane(0).expect("freeze pane"),
        FreezePane {
            frozen_rows: 2,
            frozen_cols: 1,
        }
    );
    assert_eq!(
        loaded.merged_ranges(0).expect("merged ranges"),
        vec![CellRange::new(CellCoord::new(0, 0), CellCoord::new(0, 3))]
    );

    // Verify style on A1
    let a1_style = loaded
        .cell_style(0, CellCoord::new(0, 0))
        .expect("A1 style");
    assert!(a1_style.font.bold);
    assert_eq!(a1_style.fill_color, Some(ColorRgba::rgb(31, 78, 121)));
    assert_eq!(a1_style.horizontal_align, HorizontalAlign::Center);

    // Verify values and formulas before modification
    // B3=12000, C3=13500 => D3=25500; D4=100000; D5=31500 => D6=157000
    assert_eq!(
        loaded
            .cell_value(0, CellCoord::new(2, 3))
            .expect("D3 value"),
        CellValue::Number(25_500.0)
    );
    assert_eq!(
        loaded
            .cell_formula(0, CellCoord::new(5, 3))
            .expect("D6 formula")
            .as_deref(),
        Some("=SUM(D3:D5)")
    );
    assert_eq!(
        loaded
            .cell_value(0, CellCoord::new(5, 3))
            .expect("D6 value"),
        CellValue::Number(157_000.0)
    );
    assert_eq!(
        loaded.cell_value(1, CellCoord::new(0, 1)).expect("KPI!B1"),
        CellValue::Number(166_420.0)
    );

    // 2. Modify B3 from 12000 to 22000 (+10000) and add an AVERAGE formula in E6
    loaded
        .set_cell_input(0, CellCoord::new(2, 1), "22000")
        .expect("modify B3");
    loaded
        .set_cell_input(0, CellCoord::new(5, 4), "=AVERAGE(D3:D5)")
        .expect("set E6 formula");

    // Save to temporary file atomically and reload
    let dir = tempfile::tempdir().expect("tempdir");
    let saved_path = dir.path().join("roundtrip_modified.xlsx");
    loaded.save_xlsx(&saved_path).expect("atomic save xlsx");

    let reloaded = IronCalcEngine::from_xlsx_path(&saved_path).expect("reload modified xlsx");
    assert_eq!(
        reloaded.cell_value(0, CellCoord::new(2, 3)).expect("D3"),
        CellValue::Number(35_500.0)
    );
    assert_eq!(
        reloaded.cell_value(0, CellCoord::new(5, 3)).expect("D6"),
        CellValue::Number(167_000.0)
    );
    assert_eq!(
        reloaded
            .cell_formula(0, CellCoord::new(5, 4))
            .expect("E6 formula")
            .as_deref(),
        Some("=AVERAGE(D3:D5)")
    );
    let e6 = reloaded.cell_value(0, CellCoord::new(5, 4)).expect("E6");
    match e6 {
        CellValue::Number(v) => assert!((v - (167_000.0 / 3.0)).abs() < 1e-9),
        other => panic!("expected numeric average, got {other:?}"),
    }
    assert_eq!(
        reloaded
            .cell_value(1, CellCoord::new(0, 1))
            .expect("KPI!B1"),
        CellValue::Number(177_020.0)
    );
}

#[test]
fn dynamic_arrays_modern_functions_and_structural_edits_work() {
    let mut engine = IronCalcEngine::new_empty("ModernFunctions").expect("new empty");

    // Dynamic array SEQUENCE(3, 1, 10, 5) -> A1=10, A2=15, A3=20
    engine
        .set_cell_input(0, CellCoord::new(0, 0), "=SEQUENCE(3, 1, 10, 5)")
        .expect("sequence");
    assert_eq!(
        engine.cell_value(0, CellCoord::new(0, 0)).expect("A1"),
        CellValue::Number(10.0)
    );
    assert_eq!(
        engine
            .cell_value(0, CellCoord::new(1, 0))
            .expect("A2 spill"),
        CellValue::Number(15.0)
    );
    assert_eq!(
        engine
            .cell_value(0, CellCoord::new(2, 0))
            .expect("A3 spill"),
        CellValue::Number(20.0)
    );

    // LET + XLOOKUP + error handling
    engine
        .set_cell_input(0, CellCoord::new(0, 1), "Alpha")
        .expect("B1");
    engine
        .set_cell_input(0, CellCoord::new(1, 1), "Beta")
        .expect("B2");
    engine
        .set_cell_input(0, CellCoord::new(2, 1), "Gamma")
        .expect("B3");
    engine
        .set_cell_input(0, CellCoord::new(0, 2), "=XLOOKUP(15, A1:A3, B1:B3)")
        .expect("C1 xlookup");
    assert_eq!(
        engine.cell_value(0, CellCoord::new(0, 2)).expect("C1"),
        CellValue::Text("Beta".to_string())
    );

    engine
        .set_cell_input(0, CellCoord::new(1, 2), "=LET(x, 7, y, 6, x*y)")
        .expect("C2 let");
    assert_eq!(
        engine.cell_value(0, CellCoord::new(1, 2)).expect("C2"),
        CellValue::Number(42.0)
    );

    engine
        .set_cell_input(0, CellCoord::new(2, 2), "=1/0")
        .expect("C3 div0");
    assert_eq!(
        engine.cell_value(0, CellCoord::new(2, 2)).expect("C3"),
        CellValue::Error(FormulaError::Div0)
    );

    // Row/Col visibility & dimensions
    engine.set_row_height(0, 4, 42.0).expect("row height");
    assert!((engine.row_height(0, 4).expect("get row height") - 42.0).abs() < 0.1);
    engine.set_col_hidden(0, 5, true).expect("hide col F");
    assert!(engine.is_col_hidden(0, 5).expect("is col hidden"));
    assert_eq!(engine.col_width(0, 5).expect("hidden col width"), 0.0);

    // Sheet visibility & tab color
    let s2 = engine.add_sheet(Some("HiddenSheet")).expect("add sheet");
    engine
        .set_sheet_visibility(s2, SheetVisibility::Hidden)
        .expect("hide sheet");
    engine
        .set_sheet_tab_color(s2, Some(ColorRgba::rgb(220, 50, 47)))
        .expect("tab color");
    let info = &engine.sheets()[s2 as usize];
    assert_eq!(info.visibility, SheetVisibility::Hidden);
    assert_eq!(info.tab_color, Some(ColorRgba::rgb(220, 50, 47)));
    assert_eq!(IronCalcEngine::supported_function_count(), 497);
}

#[test]
fn public_api_exposes_zero_ironcalc_types() {
    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for file_name in ["traits.rs", "types.rs", "error.rs"] {
        let content =
            std::fs::read_to_string(src_dir.join(file_name)).expect("read public module source");
        for (line_no, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                continue;
            }
            assert!(
                !trimmed.contains("ironcalc"),
                "public API module {file_name}:{} must not reference ironcalc: {trimmed}",
                line_no + 1
            );
        }
    }

    // Verify `lib.rs` never publicly re-exports `ironcalc` or `ironcalc_base`
    let lib_src = std::fs::read_to_string(src_dir.join("lib.rs")).expect("read lib.rs");
    for (line_no, line) in lib_src.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("pub ") {
            assert!(
                !trimmed.contains("ironcalc::") && !trimmed.contains("ironcalc_base"),
                "lib.rs:{} must not re-export upstream ironcalc types: {trimmed}",
                line_no + 1
            );
        }
    }

    // Verify `pub` items in `ironcalc_impl.rs` never mention upstream IronCalc types
    let impl_src =
        std::fs::read_to_string(src_dir.join("ironcalc_impl.rs")).expect("read ironcalc_impl.rs");
    for (line_no, line) in impl_src.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("pub ") {
            assert!(
                !trimmed.contains("ironcalc")
                    && !trimmed.contains("Model<")
                    && !trimmed.contains("Workbook")
                    && !trimmed.contains("Ic"),
                "public item in ironcalc_impl.rs:{} leaked upstream type: {trimmed}",
                line_no + 1
            );
        }
    }
}
