//! Synthetic workbook generators and end-to-end performance benchmark suites for Ordia.

use anyhow::{Context, Result};
use ordia_sheet_engine::{CellCoord, IronCalcEngine, SheetEngine, col_to_letters};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::process::Command;
use std::time::Instant;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

/// Bilingual shared string pool used for text columns in synthetic benchmarks.
const SHARED_STRINGS: &[&str] = &[
    "华东大区",
    "华北总部",
    "华南分部",
    "西南研发",
    "North America",
    "Western Europe",
    "Asia Pacific",
    "企业旗舰版",
    "专业团队版",
    "个人标准版",
    "云原生底座",
    "高性能表格引擎",
    "跨平台桌面套件",
    "年度订阅",
    "季度续费",
    "渠道合作伙伴",
];

/// Metrics captured for one synthetic workbook scale in the `engine` benchmark suite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineBenchResult {
    /// Human-readable label (`"10k"`, `"100k"`, `"1m"`).
    pub label: String,
    /// Number of worksheet rows.
    pub rows: u32,
    /// Number of worksheet columns.
    pub cols: u32,
    /// On-disk `.xlsx` archive size in bytes.
    pub file_size_bytes: u64,
    /// Generation time in milliseconds (`0.0` if cached file was reused).
    pub generate_ms: f64,
    /// Time to parse `.xlsx` archive into `IronCalcEngine` (before recalculation), in ms.
    pub load_ms: f64,
    /// Full workbook formula evaluation (`evaluate()`) wall-clock time, in ms.
    pub full_recalc_ms: f64,
    /// Single-cell edit + recalculation (`set_cell_input(A1)`) wall-clock time, in ms.
    pub incremental_recalc_ms: f64,
    /// Serialization + atomic `.xlsx` save wall-clock time, in ms.
    pub save_ms: f64,
    /// Process resident set size before loading workbook, in MiB.
    pub rss_before_mib: f64,
    /// Peak observed process resident set size across load/recalc/save, in MiB.
    pub peak_rss_mib: f64,
    /// Net resident memory delta (`rss_after_recalc - rss_before`), in MiB.
    pub net_rss_delta_mib: f64,
}

/// Generates a valid streaming `.xlsx` file with `rows × cols` cells:
/// - 70% numeric (`f64`)
/// - 10% dates (`numFmtId="14"` `yyyy-mm-dd`)
/// - 10% shared strings (bilingual Chinese & English)
/// - 10% formulas (5% shared formula + 5% explicit per-row `ROUND(...)` formula)
pub fn generate_synthetic_xlsx(path: &Path, rows: u32, cols: u32) -> Result<u64> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = File::create(path)
        .with_context(|| format!("failed to create synthetic xlsx {}", path.display()))?;
    let writer = BufWriter::with_capacity(256 * 1024, file);
    let mut zip = zip::ZipWriter::new(writer);
    let opts = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(1));

    zip.start_file("[Content_Types].xml", opts)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
  <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
  <Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>
  <Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>
</Types>"#,
    )?;

    zip.start_file("_rels/.rels", opts)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#,
    )?;

    zip.start_file("xl/workbook.xml", opts)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets>
    <sheet name="BenchmarkData" sheetId="1" r:id="rId1"/>
  </sheets>
</workbook>"#,
    )?;

    zip.start_file("xl/_rels/workbook.xml.rels", opts)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
  <Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>
</Relationships>"#,
    )?;

    zip.start_file("xl/styles.xml", opts)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <fonts count="1"><font><sz val="11"/><name val="Inter"/></font></fonts>
  <fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills>
  <borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>
  <cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>
  <cellXfs count="2">
    <xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>
    <xf numFmtId="14" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>
  </cellXfs>
  <cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>
</styleSheet>"#,
    )?;

    zip.start_file("xl/sharedStrings.xml", opts)?;
    write!(
        zip,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="{0}" uniqueCount="{0}">"#,
        SHARED_STRINGS.len()
    )?;
    for s in SHARED_STRINGS {
        write!(zip, "<si><t>{s}</t></si>")?;
    }
    zip.write_all(b"</sst>")?;

    let col_names: Vec<String> = (0..cols).map(col_to_letters).collect();
    let max_col_str = col_to_letters(cols.saturating_sub(1));
    zip.start_file("xl/worksheets/sheet1.xml", opts)?;
    write!(
        zip,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:{max_col_str}{rows}"/><sheetViews><sheetView workbookViewId="0"><pane ySplit="1" xSplit="1" topLeftCell="B2" activePane="bottomRight" state="frozen"/></sheetView></sheetViews><sheetData>"#
    )?;

    let mut chunk_buf = Vec::with_capacity(256 * 1024);
    let num_end = (cols * 7 / 10).max(2).min(cols);
    let date_end = (cols * 8 / 10).max(num_end).min(cols);
    let str_end = (cols * 9 / 10).max(date_end).min(cols);

    for r in 1..=rows {
        write!(chunk_buf, "<row r=\"{r}\">")?;
        let a_val = f64::from((r % 997) + 1) * 1.25;
        let b_val = f64::from((r % 251) + 1) * 0.5;
        let s_cached = a_val + b_val * 2.0;

        for c in 0..cols {
            let col_letter = &col_names[c as usize];
            if c == 0 {
                write!(chunk_buf, "<c r=\"{col_letter}{r}\"><v>{a_val}</v></c>")?;
            } else if c == 1 {
                write!(chunk_buf, "<c r=\"{col_letter}{r}\"><v>{b_val}</v></c>")?;
            } else if c < num_end {
                let v = f64::from((r.wrapping_mul(c + 3)) % 10_000) * 0.1;
                write!(chunk_buf, "<c r=\"{col_letter}{r}\"><v>{v}</v></c>")?;
            } else if c < date_end {
                let serial = 46_023 + ((r + c) % 365);
                write!(
                    chunk_buf,
                    "<c r=\"{col_letter}{r}\" s=\"1\"><v>{serial}</v></c>"
                )?;
            } else if c < str_end {
                let si = ((r + c) as usize) % SHARED_STRINGS.len();
                write!(
                    chunk_buf,
                    "<c r=\"{col_letter}{r}\" t=\"s\"><v>{si}</v></c>"
                )?;
            } else if c == str_end {
                // First formula column: shared formula across all rows
                if r == 1 {
                    write!(
                        chunk_buf,
                        "<c r=\"{col_letter}1\"><f t=\"shared\" ref=\"{col_letter}1:{col_letter}{rows}\" si=\"0\">A1+B1*2</f><v>{s_cached}</v></c>"
                    )?;
                } else {
                    write!(
                        chunk_buf,
                        "<c r=\"{col_letter}{r}\"><f t=\"shared\" si=\"0\"/><v>{s_cached}</v></c>"
                    )?;
                }
            } else {
                // Remaining formula column(s): explicit per-row ROUND formula referencing first formula col
                let prev_col = &col_names[str_end as usize];
                let t_cached = (s_cached * 1.06 * 100.0).round() / 100.0;
                write!(
                    chunk_buf,
                    "<c r=\"{col_letter}{r}\"><f>ROUND({prev_col}{r}*1.06,2)</f><v>{t_cached}</v></c>"
                )?;
            }
        }
        chunk_buf.extend_from_slice(b"</row>");
        if chunk_buf.len() >= 192 * 1024 {
            zip.write_all(&chunk_buf)?;
            chunk_buf.clear();
        }
    }
    if !chunk_buf.is_empty() {
        zip.write_all(&chunk_buf)?;
    }
    zip.write_all(b"</sheetData></worksheet>")?;
    let mut writer = zip.finish()?;
    writer.flush()?;
    let len = std::fs::metadata(path)?.len();
    Ok(len)
}

/// Samples the current process resident set size (RSS) in MiB using OS process tools (`unsafe`-free).
#[must_use]
pub fn current_rss_mib() -> f64 {
    let pid = std::process::id().to_string();
    #[cfg(unix)]
    {
        if let Ok(out) = Command::new("ps").args(["-o", "rss=", "-p", &pid]).output()
            && out.status.success()
            && let Ok(kib) = String::from_utf8_lossy(&out.stdout).trim().parse::<f64>()
        {
            return kib / 1024.0;
        }
        0.0
    }
    #[cfg(windows)]
    {
        if let Ok(out) = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!("(Get-Process -Id {pid}).WorkingSet64"),
            ])
            .output()
            && out.status.success()
            && let Ok(bytes) = String::from_utf8_lossy(&out.stdout).trim().parse::<f64>()
        {
            return bytes / (1024.0 * 1024.0);
        }
        0.0
    }
}

/// Generates (if needed) and benchmarks a single synthetic workbook scale in the current process.
pub fn run_engine_scale_in_process(
    label: &str,
    path: &Path,
    rows: u32,
    cols: u32,
    force_regenerate: bool,
) -> Result<EngineBenchResult> {
    let mut generate_ms = 0.0;
    let file_size_bytes = if force_regenerate || !path.exists() {
        let t0 = Instant::now();
        let sz = generate_synthetic_xlsx(path, rows, cols)?;
        generate_ms = t0.elapsed().as_secs_f64() * 1000.0;
        sz
    } else {
        std::fs::metadata(path)?.len()
    };

    let rss_before_mib = current_rss_mib();
    let mut peak_rss_mib = rss_before_mib;

    let raw_bytes = std::fs::read(path)
        .with_context(|| format!("failed to read benchmark file {}", path.display()))?;

    let t_load = Instant::now();
    let mut engine = IronCalcEngine::from_xlsx_bytes_unrecalculated(&raw_bytes, label)?;
    let load_ms = t_load.elapsed().as_secs_f64() * 1000.0;
    drop(raw_bytes);
    let rss_after_load = current_rss_mib();
    peak_rss_mib = peak_rss_mib.max(rss_after_load);

    let t_recalc = Instant::now();
    engine.evaluate();
    let full_recalc_ms = t_recalc.elapsed().as_secs_f64() * 1000.0;
    let rss_after_recalc = current_rss_mib();
    peak_rss_mib = peak_rss_mib.max(rss_after_recalc);
    let net_rss_delta_mib = (rss_after_recalc - rss_before_mib).max(0.0);

    let t_inc = Instant::now();
    engine.set_cell_input(0, CellCoord::new(0, 0), "999.5")?;
    let incremental_recalc_ms = t_inc.elapsed().as_secs_f64() * 1000.0;
    peak_rss_mib = peak_rss_mib.max(current_rss_mib());

    let save_tmp = std::env::temp_dir().join(format!(
        "ordia_bench_save_{label}_{}.xlsx",
        std::process::id()
    ));
    let t_save = Instant::now();
    engine.save_xlsx(&save_tmp)?;
    let save_ms = t_save.elapsed().as_secs_f64() * 1000.0;
    peak_rss_mib = peak_rss_mib.max(current_rss_mib());
    let _ = std::fs::remove_file(&save_tmp);

    Ok(EngineBenchResult {
        label: label.to_string(),
        rows,
        cols,
        file_size_bytes,
        generate_ms,
        load_ms,
        full_recalc_ms,
        incremental_recalc_ms,
        save_ms,
        rss_before_mib,
        peak_rss_mib,
        net_rss_delta_mib,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordia_sheet_engine::CellValue;

    #[test]
    fn synthetic_generator_produces_valid_recalculable_workbook() {
        let dir = tempfile::tempdir().expect("tempdir");
        let xlsx_path = dir.path().join("tiny.xlsx");
        let res = run_engine_scale_in_process("tiny", &xlsx_path, 50, 20, true)
            .expect("tiny engine benchmark");
        assert!(res.file_size_bytes > 1000);
        assert!(res.load_ms >= 0.0);

        let mut engine = IronCalcEngine::from_xlsx_path(&xlsx_path).expect("reload tiny xlsx");
        // Row 1 (index 0): A1 = 2 * 1.25 = 2.5, B1 = 2 * 0.5 = 1.0 => S1 = 2.5 + 2.0 = 4.5 => T1 = ROUND(4.5*1.06, 2) = 4.77
        assert_eq!(
            engine.cell_value(0, CellCoord::new(0, 18)).expect("S1"),
            CellValue::Number(4.5)
        );
        assert_eq!(
            engine.cell_value(0, CellCoord::new(0, 19)).expect("T1"),
            CellValue::Number(4.77)
        );
        // Edit A1 -> verify S1 & T1 recalculate
        engine
            .set_cell_input(0, CellCoord::new(0, 0), "10")
            .expect("edit A1");
        assert_eq!(
            engine
                .cell_value(0, CellCoord::new(0, 18))
                .expect("S1 updated"),
            CellValue::Number(12.0)
        );
        assert_eq!(
            engine
                .cell_value(0, CellCoord::new(0, 19))
                .expect("T1 updated"),
            CellValue::Number(12.72)
        );
    }
}
