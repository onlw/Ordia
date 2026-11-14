//! Bidirectional spreadsheet clipboard codec (TSV + HTML `<table>`) compatible with Excel and WPS.

use std::collections::HashMap;

use ordia_sheet_engine::{CellCoord, CellRange, ColorRgba, EngineError};

use crate::SheetDocument;

/// Single cell representation inside a clipboard transfer grid.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClipboardCell {
    /// Display or literal input string of the cell.
    pub text: String,
    /// Bold font weight.
    pub bold: bool,
    /// Italic font posture.
    pub italic: bool,
    /// Cell background fill color (`#RRGGBB`).
    pub bg_color: Option<ColorRgba>,
    /// Cell text foreground color (`#RRGGBB`).
    pub fg_color: Option<ColorRgba>,
}

/// Rectangular 2D grid of copied/pasted spreadsheet cells.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClipboardGrid {
    /// Row-major cells.
    pub rows: Vec<Vec<ClipboardCell>>,
}

impl ClipboardGrid {
    /// Returns `true` when the grid contains zero cells.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty() || self.rows.iter().all(Vec::is_empty)
    }
}

/// Dual-flavor clipboard payload written by Ordia (`text/plain` TSV + `text/html` `<table>`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardPayload {
    /// Tab-separated plain text representation.
    pub tsv: String,
    /// HTML `<table>` fragment with inline CSS (`font-weight`, `font-style`, `background-color`, `color`).
    pub html: String,
}

/// Serializes `range` on the active sheet of `doc` into both TSV and Excel/WPS-compatible HTML.
pub fn export_range_clipboard(
    doc: &SheetDocument,
    range: CellRange,
) -> Result<ClipboardPayload, EngineError> {
    let norm = CellRange::new(range.start, range.end);
    let mut tsv_lines = Vec::with_capacity((norm.end.row - norm.start.row + 1) as usize);
    let mut html = String::from(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"></head><body><!--StartFragment-->\
         <table style=\"border-collapse:collapse;\">",
    );

    for row in norm.start.row..=norm.end.row {
        let mut tsv_cols = Vec::with_capacity((norm.end.col - norm.start.col + 1) as usize);
        html.push_str("<tr>");
        for col in norm.start.col..=norm.end.col {
            let coord = CellCoord::new(row, col);
            let text = doc.cell_display_text(coord)?;
            let style = doc.cell_style(coord)?;
            tsv_cols.push(encode_tsv_cell(&text));

            let mut css = String::new();
            if style.font.bold {
                css.push_str("font-weight:bold;");
            }
            if style.font.italic {
                css.push_str("font-style:italic;");
            }
            if let Some(bg) = style.fill_color {
                css.push_str(&format!("background-color:{};", bg.to_hex_rgb()));
            }
            if let Some(fg) = style.font.color {
                css.push_str(&format!("color:{};", fg.to_hex_rgb()));
            }
            if css.is_empty() {
                html.push_str("<td>");
            } else {
                html.push_str(&format!("<td style=\"{css}\">"));
            }
            html.push_str(&escape_html(&text));
            html.push_str("</td>");
        }
        html.push_str("</tr>");
        tsv_lines.push(tsv_cols.join("\t"));
    }

    html.push_str("</table><!--EndFragment--></body></html>");
    Ok(ClipboardPayload {
        tsv: tsv_lines.join("\n"),
        html,
    })
}

/// Parses an incoming OS clipboard payload, preferring rich HTML `<table>` from Excel/WPS/Ordia
/// and falling back to TSV plain text.
pub fn parse_clipboard_payload(html: Option<&str>, tsv: Option<&str>) -> ClipboardGrid {
    if let Some(raw_html) = html {
        let grid = parse_html_table(raw_html);
        if !grid.is_empty() {
            return grid;
        }
    }
    if let Some(raw_tsv) = tsv {
        return parse_tsv(raw_tsv);
    }
    ClipboardGrid::default()
}

fn encode_tsv_cell(text: &str) -> String {
    if text.contains(['\t', '\n', '\r', '"']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("<br/>"),
            _ => out.push(ch),
        }
    }
    out
}

/// Parses TSV text (supporting Excel-quoted multi-line and tab-containing cells).
pub fn parse_tsv(input: &str) -> ClipboardGrid {
    let trimmed = input
        .strip_suffix("\r\n")
        .or_else(|| input.strip_suffix('\n'))
        .unwrap_or(input);
    if trimmed.is_empty() {
        return ClipboardGrid::default();
    }
    let mut rows: Vec<Vec<ClipboardCell>> = Vec::new();
    let mut cur_row: Vec<ClipboardCell> = Vec::new();
    let mut cur_cell = String::new();
    let mut in_quotes = false;
    let mut chars = trimmed.chars().peekable();

    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    let _ = chars.next();
                    cur_cell.push('"');
                } else {
                    in_quotes = false;
                }
            } else if ch != '\r' {
                cur_cell.push(ch);
            }
        } else {
            match ch {
                '"' if cur_cell.is_empty() => in_quotes = true,
                '\t' => {
                    cur_row.push(ClipboardCell {
                        text: std::mem::take(&mut cur_cell),
                        ..ClipboardCell::default()
                    });
                }
                '\r' => {}
                '\n' => {
                    cur_row.push(ClipboardCell {
                        text: std::mem::take(&mut cur_cell),
                        ..ClipboardCell::default()
                    });
                    rows.push(std::mem::take(&mut cur_row));
                }
                other => cur_cell.push(other),
            }
        }
    }
    cur_row.push(ClipboardCell {
        text: cur_cell,
        ..ClipboardCell::default()
    });
    rows.push(cur_row);
    ClipboardGrid { rows }
}

#[derive(Clone, Debug, Default)]
struct CssPatch {
    bold: bool,
    italic: bool,
    bg_color: Option<ColorRgba>,
    fg_color: Option<ColorRgba>,
}

/// Parses an HTML `<table>` from Excel, WPS, Numbers, Google Sheets, or Ordia.
pub fn parse_html_table(raw_html: &str) -> ClipboardGrid {
    let class_map = extract_css_classes(raw_html);
    let lower = raw_html.to_ascii_lowercase();
    let table_slice = if let Some(t_start) = lower.find("<table") {
        let end = lower[t_start..]
            .find("</table>")
            .map(|rel| t_start + rel + "</table>".len())
            .unwrap_or(raw_html.len());
        &raw_html[t_start..end]
    } else {
        return ClipboardGrid::default();
    };

    let mut rows = Vec::new();
    let lower_tbl = table_slice.to_ascii_lowercase();
    let mut cursor = 0usize;

    while let Some(tr_rel) = lower_tbl[cursor..].find("<tr") {
        let tr_start = cursor + tr_rel;
        let Some(tr_gt) = table_slice[tr_start..].find('>') else {
            break;
        };
        let body_start = tr_start + tr_gt + 1;
        let tr_end = lower_tbl[body_start..]
            .find("</tr>")
            .map(|rel| body_start + rel)
            .unwrap_or(table_slice.len());
        let tr_body = &table_slice[body_start..tr_end];
        let tr_cells = parse_tr_cells(tr_body, &class_map);
        if !tr_cells.is_empty() {
            rows.push(tr_cells);
        }
        cursor = (tr_end + "</tr>".len()).min(table_slice.len());
    }

    ClipboardGrid { rows }
}

fn parse_tr_cells(tr_body: &str, class_map: &HashMap<String, CssPatch>) -> Vec<ClipboardCell> {
    let mut cells = Vec::new();
    let lower = tr_body.to_ascii_lowercase();
    let mut pos = 0usize;

    while pos < tr_body.len() {
        let td_pos = lower[pos..].find("<td").map(|i| (pos + i, "</td>"));
        let th_pos = lower[pos..].find("<th").map(|i| (pos + i, "</th>"));
        let Some((cell_start, close_tag)) = (match (td_pos, th_pos) {
            (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }) else {
            break;
        };

        let Some(gt_rel) = tr_body[cell_start..].find('>') else {
            break;
        };
        let open_tag = &tr_body[cell_start..cell_start + gt_rel + 1];
        let inner_start = cell_start + gt_rel + 1;
        let inner_end = lower[inner_start..]
            .find(close_tag)
            .map(|rel| inner_start + rel)
            .unwrap_or(tr_body.len());
        let inner_html = &tr_body[inner_start..inner_end];

        let mut cell = ClipboardCell {
            bold: close_tag == "</th>",
            ..ClipboardCell::default()
        };

        if let Some(cls_attr) = extract_attr(open_tag, "class") {
            for token in cls_attr.split_whitespace() {
                if let Some(patch) = class_map.get(&token.to_ascii_lowercase()) {
                    apply_css_patch(&mut cell, patch);
                }
            }
        }
        if let Some(bg_attr) = extract_attr(open_tag, "bgcolor")
            && let Some(c) = parse_css_color(&bg_attr)
        {
            cell.bg_color = Some(c);
        }
        if let Some(style_attr) = extract_attr(open_tag, "style") {
            let patch = parse_inline_css(&style_attr);
            apply_css_patch(&mut cell, &patch);
        }

        let inner_lower = inner_html.to_ascii_lowercase();
        if inner_lower.contains("<b>")
            || inner_lower.contains("<b ")
            || inner_lower.contains("<strong>")
        {
            cell.bold = true;
        }
        if inner_lower.contains("<i>") || inner_lower.contains("<em>") {
            cell.italic = true;
        }
        cell.text = strip_tags_and_unescape(inner_html);

        let colspan = extract_attr(open_tag, "colspan")
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(1)
            .clamp(1, 256);
        cells.push(cell);
        for _ in 1..colspan {
            cells.push(ClipboardCell::default());
        }

        pos = (inner_end + close_tag.len()).min(tr_body.len());
    }

    cells
}

fn apply_css_patch(cell: &mut ClipboardCell, patch: &CssPatch) {
    if patch.bold {
        cell.bold = true;
    }
    if patch.italic {
        cell.italic = true;
    }
    if patch.bg_color.is_some() {
        cell.bg_color = patch.bg_color;
    }
    if patch.fg_color.is_some() {
        cell.fg_color = patch.fg_color;
    }
}

fn extract_css_classes(raw_html: &str) -> HashMap<String, CssPatch> {
    let mut map = HashMap::new();
    let lower = raw_html.to_ascii_lowercase();
    let mut pos = 0usize;
    while let Some(s_rel) = lower[pos..].find("<style") {
        let s_start = pos + s_rel;
        let Some(gt_rel) = raw_html[s_start..].find('>') else {
            break;
        };
        let body_start = s_start + gt_rel + 1;
        let Some(end_rel) = lower[body_start..].find("</style>") else {
            break;
        };
        let css_block = &raw_html[body_start..body_start + end_rel];
        for rule in css_block.split('}') {
            if let Some((selectors, decls)) = rule.split_once('{') {
                let patch = parse_inline_css(decls);
                for sel in selectors.split(',') {
                    let trimmed = sel.trim();
                    if let Some(dot_idx) = trimmed.rfind('.') {
                        let cls_name = trimmed[dot_idx + 1..].trim().to_ascii_lowercase();
                        if !cls_name.is_empty() {
                            map.insert(cls_name, patch.clone());
                        }
                    }
                }
            }
        }
        pos = body_start + end_rel + "</style>".len();
    }
    map
}

fn parse_inline_css(decls: &str) -> CssPatch {
    let mut patch = CssPatch::default();
    for pair in decls.split(';') {
        let Some((raw_k, raw_v)) = pair.split_once(':') else {
            continue;
        };
        let k = raw_k.trim().to_ascii_lowercase();
        let v = raw_v
            .trim()
            .trim_end_matches("!important")
            .trim()
            .to_ascii_lowercase();
        match k.as_str() {
            "font-weight" => {
                if v == "bold" || v == "bolder" || v.parse::<u16>().is_ok_and(|w| w >= 600) {
                    patch.bold = true;
                }
            }
            "font-style" => {
                if v == "italic" || v == "oblique" {
                    patch.italic = true;
                }
            }
            "background-color" | "background" => {
                if let Some(c) = parse_css_color(&v) {
                    patch.bg_color = Some(c);
                }
            }
            "color" => {
                if let Some(c) = parse_css_color(&v) {
                    patch.fg_color = Some(c);
                }
            }
            _ => {}
        }
    }
    patch
}

fn parse_css_color(val: &str) -> Option<ColorRgba> {
    let token = val
        .split_whitespace()
        .find(|t| t.starts_with('#') || t.starts_with("rgb(") || t.starts_with("rgba("))
        .unwrap_or(val.trim());

    if let Some(hex) = token.strip_prefix('#') {
        if hex.len() == 6 {
            return ColorRgba::from_hex_rgb(hex);
        } else if hex.len() == 3 {
            let mut expanded = String::with_capacity(6);
            for ch in hex.chars() {
                expanded.push(ch);
                expanded.push(ch);
            }
            return ColorRgba::from_hex_rgb(&expanded);
        }
    }
    let inside = token
        .strip_prefix("rgba(")
        .or_else(|| token.strip_prefix("rgb("))
        .and_then(|s| s.strip_suffix(')'))?;
    let parts: Vec<&str> = inside.split(',').collect();
    if parts.len() >= 3 {
        let r = parts[0].trim().parse::<u8>().ok()?;
        let g = parts[1].trim().parse::<u8>().ok()?;
        let b = parts[2].trim().parse::<u8>().ok()?;
        return Some(ColorRgba::rgb(r, g, b));
    }
    None
}

fn extract_attr(open_tag: &str, attr_name: &str) -> Option<String> {
    let lower = open_tag.to_ascii_lowercase();
    let needle = format!("{attr_name}=");
    let idx = lower.find(&needle)?;
    let rest = open_tag[idx + needle.len()..].trim_start();
    if let Some(quoted) = rest.strip_prefix('"') {
        Some(quoted.split('"').next()?.to_string())
    } else if let Some(single) = rest.strip_prefix('\'') {
        Some(single.split('\'').next()?.to_string())
    } else {
        Some(
            rest.split(|c: char| c.is_whitespace() || c == '>')
                .next()?
                .to_string(),
        )
    }
}

fn strip_tags_and_unescape(html: &str) -> String {
    let mut raw = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut tag_buf = String::new();

    for ch in html.chars() {
        if in_tag {
            if ch == '>' {
                in_tag = false;
                let t = tag_buf.trim().to_ascii_lowercase();
                if t.starts_with("br") {
                    raw.push('\n');
                }
                tag_buf.clear();
            } else {
                tag_buf.push(ch);
            }
        } else if ch == '<' {
            in_tag = true;
        } else {
            raw.push(ch);
        }
    }

    let unescaped = raw
        .replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&");
    unescaped.trim_matches(['\r', '\n']).trim().to_string()
}
