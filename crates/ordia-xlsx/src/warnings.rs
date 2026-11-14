//! Pre-save integrity checks detecting user edits that could invalidate preserved OOXML features.

use crate::preserve::PreservedPackage;
use ordia_sheet_engine::{CellCoord, CellRange, CellValue, SheetEngine};

/// Category of potential incompatibility between a user edit and preserved OOXML parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreservationHazardKind {
    /// A worksheet referenced by a preserved PivotCache was deleted or renamed.
    PivotSourceSheetMissing {
        /// Preserved PivotCache `cacheId`.
        cache_id: String,
        /// Expected worksheet name.
        expected_sheet: String,
    },
    /// Every cell inside a preserved PivotCache's source range was cleared to empty.
    PivotSourceRangeCleared {
        /// Preserved PivotCache `cacheId`.
        cache_id: String,
        /// Source worksheet name.
        sheet_name: String,
        /// Source A1 range string.
        range_ref: String,
    },
    /// A worksheet referenced by a preserved Chart series formula was deleted or renamed.
    ChartSourceSheetMissing {
        /// Chart part path inside the ZIP archive.
        chart_part: String,
        /// Expected worksheet name.
        expected_sheet: String,
    },
    /// A worksheet that hosted preserved drawings, slicers, or PivotTables was deleted.
    WorksheetWithAttachmentsDeleted {
        /// Original `sheetId`.
        sheet_id: u32,
        /// Original worksheet name.
        orig_name: String,
    },
}

/// Human-readable + structured warning emitted prior to saving when preserved content may break.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreservationWarning {
    /// Structured hazard kind for programmatic UI handling and localization.
    pub kind: PreservationHazardKind,
    /// Diagnostic summary describing the affected part and recommended user action.
    pub summary: String,
}

/// Checks `engine` state against `preserved` metadata and returns all detected hazards.
#[must_use]
pub fn check_preservation_warnings(
    engine: &dyn SheetEngine,
    preserved: &PreservedPackage,
) -> Vec<PreservationWarning> {
    let mut warnings = Vec::new();
    let sheets = engine.sheets();

    // 1. Check whether any worksheet carrying preserved drawings/slicers/pivots was deleted.
    for (sheet_id, attach) in &preserved.sheet_attachments {
        if !sheets.iter().any(|s| s.sheet_id == *sheet_id) {
            warnings.push(PreservationWarning {
                kind: PreservationHazardKind::WorksheetWithAttachmentsDeleted {
                    sheet_id: *sheet_id,
                    orig_name: attach.orig_name.clone(),
                },
                summary: format!(
                    "Deleted worksheet '{}' (sheetId={sheet_id}) contained preserved objects (drawings, slicers, or pivot tables).",
                    attach.orig_name
                ),
            });
        }
    }

    // 2. Check PivotCache worksheet sources.
    for pc in &preserved.pivot_caches {
        let Some(ref src_sheet) = pc.source_sheet else {
            continue;
        };
        let Some(sheet_idx) = engine.sheet_index_by_name(src_sheet) else {
            warnings.push(PreservationWarning {
                kind: PreservationHazardKind::PivotSourceSheetMissing {
                    cache_id: pc.cache_id.clone(),
                    expected_sheet: src_sheet.clone(),
                },
                summary: format!(
                    "PivotCache #{} references worksheet '{}', which was renamed or deleted.",
                    pc.cache_id, src_sheet
                ),
            });
            continue;
        };

        if let Some(ref range_str) = pc.source_ref
            && let Some(range) = parse_a1_range(range_str)
            && is_range_entirely_empty(engine, sheet_idx, range)
        {
            warnings.push(PreservationWarning {
                kind: PreservationHazardKind::PivotSourceRangeCleared {
                    cache_id: pc.cache_id.clone(),
                    sheet_name: src_sheet.clone(),
                    range_ref: range_str.clone(),
                },
                summary: format!(
                    "PivotCache #{} source range '{}!{}' was completely cleared.",
                    pc.cache_id, src_sheet, range_str
                ),
            });
        }
    }

    // 3. Check Chart formula references.
    for cref in &preserved.chart_refs {
        if engine.sheet_index_by_name(&cref.sheet_name).is_none() {
            let duplicate = warnings.iter().any(|w| {
                matches!(
                    &w.kind,
                    PreservationHazardKind::ChartSourceSheetMissing {
                        chart_part,
                        expected_sheet,
                    } if chart_part == &cref.chart_part && expected_sheet == &cref.sheet_name
                )
            });
            if !duplicate {
                warnings.push(PreservationWarning {
                    kind: PreservationHazardKind::ChartSourceSheetMissing {
                        chart_part: cref.chart_part.clone(),
                        expected_sheet: cref.sheet_name.clone(),
                    },
                    summary: format!(
                        "Chart '{}' references worksheet '{}', which was renamed or deleted.",
                        cref.chart_part, cref.sheet_name
                    ),
                });
            }
        }
    }

    warnings
}

fn is_range_entirely_empty(engine: &dyn SheetEngine, sheet: u32, range: CellRange) -> bool {
    let max_r = range.end.row.min(range.start.row.saturating_add(128));
    let max_c = range.end.col.min(range.start.col.saturating_add(32));
    for r in range.start.row..=max_r {
        for c in range.start.col..=max_c {
            if let Ok(val) = engine.cell_value(sheet, CellCoord::new(r, c))
                && !matches!(val, CellValue::Empty)
            {
                return false;
            }
        }
    }
    true
}

fn parse_a1_range(raw: &str) -> Option<CellRange> {
    let trimmed = raw.trim();
    if let Some((a, b)) = trimmed.split_once(':') {
        Some(CellRange::new(parse_a1_cell(a)?, parse_a1_cell(b)?))
    } else {
        Some(CellRange::single(parse_a1_cell(trimmed)?))
    }
}

fn parse_a1_cell(token: &str) -> Option<CellCoord> {
    let clean: String = token.chars().filter(|c| *c != '$').collect();
    let split_ix = clean.find(|c: char| c.is_ascii_digit())?;
    let (col_str, row_str) = clean.split_at(split_ix);
    if col_str.is_empty() || row_str.is_empty() {
        return None;
    }
    let mut col_1based: u32 = 0;
    for ch in col_str.chars() {
        if !ch.is_ascii_alphabetic() {
            return None;
        }
        let digit = u32::from(ch.to_ascii_uppercase() as u8 - b'A') + 1;
        col_1based = col_1based.checked_mul(26)?.checked_add(digit)?;
    }
    let row_1based = row_str.parse::<u32>().ok()?;
    if col_1based == 0 || row_1based == 0 {
        return None;
    }
    Some(CellCoord::new(row_1based - 1, col_1based - 1))
}
