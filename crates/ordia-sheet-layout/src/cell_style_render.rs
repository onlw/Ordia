//! Comprehensive cell style layout and `DisplayList` emission (`M2-03`):
//! fonts, sizes, bold/italic/underline/strikethrough, foreground & fill colors, all 13 OOXML
//! border line styles (including parallel `Double` lines and diagonals), horizontal & vertical
//! alignment, indentation, automatic CJK/Latin line wrapping, and overflow into empty neighbors.

use crate::axis::GridAxisIndex;
use crate::merged::MergedRangeIndex;
use crate::tile_cache::{CachedTile, TileKey, TileRenderTheme};
use ordia_render::{
    Color, DashPattern, DisplayList, FontWeight, Point, RectF, StrokeStyle, TextHAlign,
    TextRunStyle,
};
use ordia_sheet_model::{
    BorderEdge, BorderLineStyle, CellBorders, CellCoord, CellRange, CellStyle, CellValue,
    ColorRgba, HorizontalAlign, MAX_COLS, SheetDocument, VerticalAlign,
};
use ordia_text::is_cjk_char;

const MAX_OVERFLOW_NEIGHBORS: u32 = 12;
const CELL_PAD_X: f32 = 5.0;
const INDENT_STEP_PX: f32 = 8.0;

pub(crate) fn to_render_color(c: ColorRgba) -> Color {
    Color::rgba(c.r, c.g, c.b, c.a)
}

/// Estimates rendered advance width of `text` in logical pixels at `font_size_px`.
#[must_use]
pub fn estimate_text_width_px(text: &str, font_size_px: f32, bold: bool) -> f32 {
    let weight_scale = if bold { 1.06 } else { 1.0 };
    text.chars()
        .map(|ch| {
            let em = if is_cjk_char(ch) {
                1.0
            } else if ch.is_ascii_uppercase() || ch == 'W' || ch == 'M' {
                0.64
            } else if matches!(ch, 'i' | 'l' | 'I' | '.' | ',' | ':' | ';' | '\'' | '!') {
                0.32
            } else if ch == ' ' {
                0.30
            } else {
                0.56
            };
            font_size_px * em * weight_scale
        })
        .sum()
}

/// Wraps `text` into lines fitting `max_width_px` (honoring explicit `\n` and `wrap_text`).
#[must_use]
pub fn wrap_cell_text(
    text: &str,
    max_width_px: f32,
    font_size_px: f32,
    bold: bool,
    wrap_text: bool,
) -> Vec<String> {
    if !wrap_text {
        if text.contains('\n') {
            return text
                .split('\n')
                .map(|s| s.trim_end_matches('\r').to_string())
                .collect();
        }
        return vec![text.to_string()];
    }

    let limit = max_width_px.max(font_size_px * 1.1);
    let mut out = Vec::new();
    for raw_para in text.split('\n') {
        let para = raw_para.trim_end_matches('\r');
        if para.is_empty() {
            out.push(String::new());
            continue;
        }
        wrap_paragraph(para, limit, font_size_px, bold, &mut out);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn wrap_paragraph(para: &str, limit: f32, font_size_px: f32, bold: bool, out: &mut Vec<String>) {
    let mut cur = String::new();
    let mut cur_w = 0.0_f32;
    let mut word = String::new();
    let mut word_w = 0.0_f32;

    let flush_word = |word: &mut String,
                      word_w: &mut f32,
                      cur: &mut String,
                      cur_w: &mut f32,
                      out: &mut Vec<String>| {
        if word.is_empty() {
            return;
        }
        if !cur.is_empty() && *cur_w + *word_w > limit {
            out.push(cur.trim_end().to_string());
            cur.clear();
            *cur_w = 0.0;
        }
        if *word_w <= limit {
            cur.push_str(word);
            *cur_w += *word_w;
        } else {
            for ch in word.chars() {
                let cw = estimate_text_width_px(ch.encode_utf8(&mut [0; 4]), font_size_px, bold);
                if !cur.is_empty() && *cur_w + cw > limit {
                    out.push(std::mem::take(cur));
                    *cur_w = 0.0;
                }
                cur.push(ch);
                *cur_w += cw;
            }
        }
        word.clear();
        *word_w = 0.0;
    };

    for ch in para.chars() {
        let cw = estimate_text_width_px(ch.encode_utf8(&mut [0; 4]), font_size_px, bold);
        if is_cjk_char(ch) {
            flush_word(&mut word, &mut word_w, &mut cur, &mut cur_w, out);
            if !cur.is_empty() && cur_w + cw > limit {
                out.push(cur.trim_end().to_string());
                cur.clear();
                cur_w = 0.0;
            }
            cur.push(ch);
            cur_w += cw;
        } else if ch == ' ' {
            flush_word(&mut word, &mut word_w, &mut cur, &mut cur_w, out);
            if !cur.is_empty() && cur_w + cw <= limit {
                cur.push(' ');
                cur_w += cw;
            } else if !cur.is_empty() {
                out.push(cur.trim_end().to_string());
                cur.clear();
                cur_w = 0.0;
            }
        } else {
            word.push(ch);
            word_w += cw;
        }
    }
    flush_word(&mut word, &mut word_w, &mut cur, &mut cur_w, out);
    if !cur.is_empty() {
        out.push(cur);
    }
}

/// Emits one cell border segment (including parallel `Double` strokes) onto `dl`.
pub(crate) fn emit_border_segment(
    dl: &mut DisplayList,
    p0: Point,
    p1: Point,
    normal: (f32, f32),
    edge: Option<&BorderEdge>,
) {
    let Some(edge) = edge else {
        return;
    };
    let color = edge.color.map_or(Color::BLACK, to_render_color);
    if edge.style == BorderLineStyle::Double {
        let stroke = StrokeStyle::solid(1.0, color);
        dl.draw_line(
            Point::new(p0.x - normal.0, p0.y - normal.1),
            Point::new(p1.x - normal.0, p1.y - normal.1),
            stroke,
        );
        dl.draw_line(
            Point::new(p0.x + normal.0, p0.y + normal.1),
            Point::new(p1.x + normal.0, p1.y + normal.1),
            stroke,
        );
        return;
    }

    let (width, dash) = match edge.style {
        BorderLineStyle::Thin => (1.0, None),
        BorderLineStyle::Medium => (2.0, None),
        BorderLineStyle::Thick => (3.0, None),
        BorderLineStyle::Double => unreachable!(),
        BorderLineStyle::Hair => (0.5, Some(DashPattern::Simple([1.0, 1.0]))),
        BorderLineStyle::Dotted => (1.0, Some(DashPattern::Simple([1.5, 1.5]))),
        BorderLineStyle::Dashed => (1.0, Some(DashPattern::Simple([4.0, 2.0]))),
        BorderLineStyle::DashDot => (1.0, Some(DashPattern::DashDot([5.0, 2.0, 1.5, 2.0]))),
        BorderLineStyle::DashDotDot => (
            1.0,
            Some(DashPattern::DashDotDot([5.0, 2.0, 1.5, 2.0, 1.5, 2.0])),
        ),
        BorderLineStyle::MediumDashed => (2.0, Some(DashPattern::Simple([6.0, 3.0]))),
        BorderLineStyle::MediumDashDot | BorderLineStyle::SlantDashDot => {
            (2.0, Some(DashPattern::DashDot([6.0, 2.5, 2.0, 2.5])))
        }
        BorderLineStyle::MediumDashDotDot => (
            2.0,
            Some(DashPattern::DashDotDot([6.0, 2.0, 2.0, 2.0, 2.0, 2.0])),
        ),
    };
    dl.draw_line(p0, p1, StrokeStyle { width, color, dash });
}

pub(crate) fn draw_cell_borders_into(dl: &mut DisplayList, rect: RectF, borders: &CellBorders) {
    let x0 = rect.x;
    let y0 = rect.y;
    let x1 = rect.right();
    let y1 = rect.bottom();
    emit_border_segment(
        dl,
        Point::new(x0, y0),
        Point::new(x1, y0),
        (0.0, 1.0),
        borders.top.as_ref(),
    );
    emit_border_segment(
        dl,
        Point::new(x0, y1),
        Point::new(x1, y1),
        (0.0, 1.0),
        borders.bottom.as_ref(),
    );
    emit_border_segment(
        dl,
        Point::new(x0, y0),
        Point::new(x0, y1),
        (1.0, 0.0),
        borders.left.as_ref(),
    );
    emit_border_segment(
        dl,
        Point::new(x1, y0),
        Point::new(x1, y1),
        (1.0, 0.0),
        borders.right.as_ref(),
    );
    if borders.diagonal_up {
        emit_border_segment(
            dl,
            Point::new(x0, y1),
            Point::new(x1, y0),
            (0.707, 0.707),
            borders.diagonal.as_ref(),
        );
    }
    if borders.diagonal_down {
        emit_border_segment(
            dl,
            Point::new(x0, y0),
            Point::new(x1, y1),
            (-0.707, 0.707),
            borders.diagonal.as_ref(),
        );
    }
}

fn resolve_h_align(style: &CellStyle, val: &CellValue) -> TextHAlign {
    match style.horizontal_align {
        HorizontalAlign::Right => TextHAlign::Right,
        HorizontalAlign::Center
        | HorizontalAlign::CenterContinuous
        | HorizontalAlign::Distributed => TextHAlign::Center,
        HorizontalAlign::General => match val {
            CellValue::Number(_) => TextHAlign::Right,
            CellValue::Boolean(_) | CellValue::Error(_) => TextHAlign::Center,
            CellValue::Empty | CellValue::Text(_) => TextHAlign::Left,
        },
        HorizontalAlign::Left | HorizontalAlign::Fill | HorizontalAlign::Justify => {
            TextHAlign::Left
        }
    }
}

fn is_empty_for_overflow(
    doc: &SheetDocument,
    sheet: u32,
    coord: CellCoord,
    merged_index: &MergedRangeIndex,
) -> bool {
    if merged_index.containing(coord).is_some() {
        return false;
    }
    matches!(
        doc.cell_value_on(sheet, coord).unwrap_or_default(),
        CellValue::Empty
    ) && doc
        .cell_display_text_on(sheet, coord)
        .map(|s| s.is_empty())
        .unwrap_or(true)
}

#[allow(clippy::too_many_arguments)]
fn compute_overflow_extent(
    doc: &SheetDocument,
    sheet: u32,
    coord: CellCoord,
    style: &CellStyle,
    h_align: TextHAlign,
    needed_extra_px: f32,
    col_index: &GridAxisIndex,
    merged_index: &MergedRangeIndex,
) -> (u32, u32, f32, f32) {
    if needed_extra_px <= 0.5 || style.wrap_text || style.horizontal_align == HorizontalAlign::Fill
    {
        return (0, 0, 0.0, 0.0);
    }

    let scan_right = |want_px: f32| -> (u32, f32) {
        if style.borders.right.is_some() {
            return (0, 0.0);
        }
        let mut cols = 0u32;
        let mut gained = 0.0_f32;
        let max_c = (coord.col + MAX_OVERFLOW_NEIGHBORS).min(MAX_COLS - 1);
        for next_col in (coord.col + 1)..=max_c {
            let next_coord = CellCoord::new(coord.row, next_col);
            let w = col_index.span_size(next_col);
            if w <= 0.0 {
                cols += 1;
                continue;
            }
            if !is_empty_for_overflow(doc, sheet, next_coord, merged_index) {
                break;
            }
            let next_style = doc.cell_style_on(sheet, next_coord).unwrap_or_default();
            if next_style.borders.left.is_some() {
                break;
            }
            cols += 1;
            gained += w;
            if gained >= want_px || next_style.borders.right.is_some() {
                break;
            }
        }
        (cols, gained)
    };

    let scan_left = |want_px: f32| -> (u32, f32) {
        if style.borders.left.is_some() || coord.col == 0 {
            return (0, 0.0);
        }
        let mut cols = 0u32;
        let mut gained = 0.0_f32;
        let min_c = coord.col.saturating_sub(MAX_OVERFLOW_NEIGHBORS);
        let mut prev_col = coord.col;
        while prev_col > min_c {
            prev_col -= 1;
            let prev_coord = CellCoord::new(coord.row, prev_col);
            let w = col_index.span_size(prev_col);
            if w <= 0.0 {
                cols += 1;
                continue;
            }
            if !is_empty_for_overflow(doc, sheet, prev_coord, merged_index) {
                break;
            }
            let prev_style = doc.cell_style_on(sheet, prev_coord).unwrap_or_default();
            if prev_style.borders.right.is_some() {
                break;
            }
            cols += 1;
            gained += w;
            if gained >= want_px || prev_style.borders.left.is_some() {
                break;
            }
        }
        (cols, gained)
    };

    match h_align {
        TextHAlign::Left => {
            let (rc, rpx) = scan_right(needed_extra_px);
            (0, rc, 0.0, rpx)
        }
        TextHAlign::Right => {
            let (lc, lpx) = scan_left(needed_extra_px);
            (lc, 0, lpx, 0.0)
        }
        TextHAlign::Center => {
            let half = needed_extra_px * 0.5;
            let (lc, lpx) = scan_left(half);
            let (rc, rpx) = scan_right(half);
            let sym = lpx.min(rpx);
            (lc, rc, sym, sym)
        }
    }
}

pub(crate) fn build_styled_tile(
    doc: &SheetDocument,
    key: TileKey,
    row_index: &GridAxisIndex,
    col_index: &GridAxisIndex,
    merged_index: &MergedRangeIndex,
    theme: TileRenderTheme,
) -> CachedTile {
    let range = key.cell_range();
    let r_count = range.height() as usize;
    let c_count = range.width() as usize;

    let sheet_origin_x = col_index.index_to_offset(range.start.col);
    let sheet_origin_y = row_index.index_to_offset(range.start.row);

    let mut local_y = Vec::with_capacity(r_count + 1);
    let mut acc_y = 0.0_f32;
    local_y.push(0.0);
    for r_off in 0..r_count {
        acc_y += row_index.span_size(range.start.row + r_off as u32);
        local_y.push(acc_y);
    }

    let mut local_x = Vec::with_capacity(c_count + 1);
    let mut acc_x = 0.0_f32;
    local_x.push(0.0);
    for c_off in 0..c_count {
        acc_x += col_index.span_size(range.start.col + c_off as u32);
        local_x.push(acc_x);
    }

    let tile_w = acc_x.max(0.0);
    let tile_h = acc_y.max(0.0);
    let mut dl = DisplayList::new(tile_w, tile_h);
    if tile_w <= 0.0 || tile_h <= 0.0 {
        return CachedTile {
            key,
            sheet_origin_x,
            sheet_origin_y,
            display_list: dl,
        };
    }

    let sheet_used = doc
        .sheets()
        .iter()
        .find(|s| s.index == key.sheet)
        .and_then(|s| s.used_range);

    // Track suppressed vertical & horizontal gridline boundaries inside the tile.
    let mut suppressed_v = vec![false; r_count * c_count];
    let mut suppressed_h = vec![false; r_count * c_count];

    let mut tile_merges: Vec<CellRange> = Vec::new();
    merged_index.intersecting(range, &mut tile_merges);
    for m in &tile_merges {
        let mr0 = m.start.row.max(range.start.row);
        let mr1 = m.end.row.min(range.end.row);
        let mc0 = m.start.col.max(range.start.col);
        let mc1 = m.end.col.min(range.end.col);
        for row in mr0..=mr1 {
            let r_off = (row - range.start.row) as usize;
            for col in mc0..=mc1 {
                let c_off = (col - range.start.col) as usize;
                if col < m.end.col {
                    suppressed_v[r_off * c_count + c_off] = true;
                }
                if row < m.end.row {
                    suppressed_h[r_off * c_count + c_off] = true;
                }
            }
        }
    }

    struct PreparedCell {
        coord: CellCoord,
        is_merged: bool,
        cx: f32,
        ry: f32,
        cw: f32,
        rh: f32,
        style: CellStyle,
        text: String,
        val: CellValue,
    }
    let mut prepared = Vec::new();

    if let Some(used) = sheet_used
        && used.intersects(range)
    {
        let eff_r0 = range.start.row.max(used.start.row);
        let eff_r1 = range.end.row.min(used.end.row);
        let eff_c0 = range.start.col.max(used.start.col);
        let eff_c1 = range.end.col.min(used.end.col);

        for row in eff_r0..=eff_r1 {
            let r_off = (row - range.start.row) as usize;
            let ry = local_y[r_off];
            let rh = local_y[r_off + 1] - ry;
            if rh <= 0.0 {
                continue;
            }
            for col in eff_c0..=eff_c1 {
                let c_off = (col - range.start.col) as usize;
                let cx = local_x[c_off];
                let cw = local_x[c_off + 1] - cx;
                if cw <= 0.0 {
                    continue;
                }
                let coord = CellCoord::new(row, col);
                if merged_index.containing(coord).is_some() {
                    continue;
                }
                let text = doc
                    .cell_display_text_on(key.sheet, coord)
                    .unwrap_or_default();
                let style = doc.cell_style_on(key.sheet, coord).unwrap_or_default();
                let val = if text.is_empty() {
                    CellValue::Empty
                } else {
                    doc.cell_value_on(key.sheet, coord).unwrap_or_default()
                };

                if !text.is_empty() && !style.wrap_text && !text.contains('\n') {
                    let font_size = (style.font.size_pt * 1.18).clamp(7.0, 84.0);
                    let indent_px = f32::from(style.indent) * INDENT_STEP_PX;
                    let inner_w = (cw - 2.0 * CELL_PAD_X - indent_px).max(0.0);
                    let est_w = estimate_text_width_px(&text, font_size, style.font.bold);
                    let h_align = resolve_h_align(&style, &val);
                    let (lc, rc, _, _) = compute_overflow_extent(
                        doc,
                        key.sheet,
                        coord,
                        &style,
                        h_align,
                        est_w - inner_w,
                        col_index,
                        merged_index,
                    );
                    for step in 0..rc {
                        let idx_c = c_off + step as usize;
                        if idx_c < c_count {
                            suppressed_v[r_off * c_count + idx_c] = true;
                        }
                    }
                    for step in 1..=lc {
                        if let Some(idx_c) = c_off.checked_sub(step as usize) {
                            suppressed_v[r_off * c_count + idx_c] = true;
                        }
                    }
                }

                prepared.push(PreparedCell {
                    coord,
                    is_merged: false,
                    cx,
                    ry,
                    cw,
                    rh,
                    style,
                    text,
                    val,
                });
            }
        }
    }

    // Prepare intersecting merged cells (including cross-tile merges whose anchor is outside `range`).
    for m in &tile_merges {
        let mx0 = (col_index.index_to_offset(m.start.col) - sheet_origin_x) as f32;
        let mx1 = (col_index.index_end_offset(m.end.col) - sheet_origin_x) as f32;
        let my0 = (row_index.index_to_offset(m.start.row) - sheet_origin_y) as f32;
        let my1 = (row_index.index_end_offset(m.end.row) - sheet_origin_y) as f32;
        let mw = (mx1 - mx0).max(0.0);
        let mh = (my1 - my0).max(0.0);
        if mw <= 0.0 || mh <= 0.0 {
            continue;
        }
        let anchor = m.start;
        let mut style = doc.cell_style_on(key.sheet, anchor).unwrap_or_default();
        if style.borders.right.is_none() && m.end.col > anchor.col {
            style.borders.right = doc
                .cell_style_on(key.sheet, CellCoord::new(anchor.row, m.end.col))
                .ok()
                .and_then(|s| s.borders.right);
        }
        if style.borders.bottom.is_none() && m.end.row > anchor.row {
            style.borders.bottom = doc
                .cell_style_on(key.sheet, CellCoord::new(m.end.row, anchor.col))
                .ok()
                .and_then(|s| s.borders.bottom);
        }
        let text = doc
            .cell_display_text_on(key.sheet, anchor)
            .unwrap_or_default();
        let val = if text.is_empty() {
            CellValue::Empty
        } else {
            doc.cell_value_on(key.sheet, anchor).unwrap_or_default()
        };
        prepared.push(PreparedCell {
            coord: anchor,
            is_merged: true,
            cx: mx0,
            ry: my0,
            cw: mw,
            rh: mh,
            style,
            text,
            val,
        });
    }

    // Pass 1: Gridlines (suppressing segments inside merged ranges or crossed by overflowing text).
    if let Some(grid_c) = theme.grid_line_color {
        for r_off in 0..r_count {
            let h = local_y[r_off + 1] - local_y[r_off];
            if h <= 0.0 {
                continue;
            }
            let y = local_y[r_off + 1] - 1.0;
            let any_suppressed = (0..c_count).any(|c| suppressed_h[r_off * c_count + c]);
            if !any_suppressed {
                dl.fill_rect(RectF::new(0.0, y, tile_w, 1.0), grid_c);
            } else {
                for c_off in 0..c_count {
                    let w = local_x[c_off + 1] - local_x[c_off];
                    if w > 0.0 && !suppressed_h[r_off * c_count + c_off] {
                        dl.fill_rect(RectF::new(local_x[c_off], y, w, 1.0), grid_c);
                    }
                }
            }
        }
        for c_off in 0..c_count {
            let w = local_x[c_off + 1] - local_x[c_off];
            if w <= 0.0 {
                continue;
            }
            let x = local_x[c_off + 1] - 1.0;
            let any_suppressed = (0..r_count).any(|r| suppressed_v[r * c_count + c_off]);
            if !any_suppressed {
                dl.fill_rect(RectF::new(x, 0.0, 1.0, tile_h), grid_c);
            } else {
                for r_off in 0..r_count {
                    let h = local_y[r_off + 1] - local_y[r_off];
                    if h > 0.0 && !suppressed_v[r_off * c_count + c_off] {
                        dl.fill_rect(RectF::new(x, local_y[r_off], 1.0, h), grid_c);
                    }
                }
            }
        }
    }

    let tile_clip = RectF::new(0.0, 0.0, tile_w, tile_h);

    // Pass 2: Cell background fills (clamped to tile bounds for cross-tile merged cells).
    for cell in &prepared {
        if let Some(fill_rgb) = cell.style.fill_color {
            let fx0 = cell.cx.max(0.0);
            let fy0 = cell.ry.max(0.0);
            let fx1 = (cell.cx + cell.cw - 1.0).min(tile_w);
            let fy1 = (cell.ry + cell.rh - 1.0).min(tile_h);
            if fx1 > fx0 && fy1 > fy0 {
                dl.fill_rect(
                    RectF::new(fx0, fy0, fx1 - fx0, fy1 - fy0),
                    to_render_color(fill_rgb),
                );
            }
        }
    }

    // Pass 3: Cell borders (clipped to tile bounds when a merged cell crosses a tile seam).
    for cell in &prepared {
        let cell_rect = RectF::new(cell.cx, cell.ry, cell.cw, cell.rh);
        if cell.is_merged
            && (cell.cx < 0.0
                || cell.ry < 0.0
                || cell.cx + cell.cw > tile_w
                || cell.ry + cell.rh > tile_h)
        {
            dl.with_clip(tile_clip, |clipped| {
                draw_cell_borders_into(clipped, cell_rect, &cell.style.borders);
            });
        } else {
            draw_cell_borders_into(&mut dl, cell_rect, &cell.style.borders);
        }
    }

    // Pass 4: Cell text (alignment, indent, multi-line wrap, vertical align, and overflow/tile clip).
    for cell in &prepared {
        if cell.text.is_empty() {
            continue;
        }
        emit_cell_text(
            &mut dl,
            doc,
            key.sheet,
            cell.coord,
            cell.is_merged,
            tile_clip,
            RectF::new(cell.cx, cell.ry, cell.cw, cell.rh),
            &cell.style,
            &cell.text,
            &cell.val,
            col_index,
            merged_index,
            theme.default_text_color,
        );
    }

    CachedTile {
        key,
        sheet_origin_x,
        sheet_origin_y,
        display_list: dl,
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_cell_text(
    dl: &mut DisplayList,
    doc: &SheetDocument,
    sheet: u32,
    coord: CellCoord,
    is_merged: bool,
    tile_clip: RectF,
    cell_rect: RectF,
    style: &CellStyle,
    raw_text: &str,
    val: &CellValue,
    col_index: &GridAxisIndex,
    merged_index: &MergedRangeIndex,
    default_text_color: Color,
) {
    let font_size = (style.font.size_pt * 1.18).clamp(7.0, 84.0);
    let line_h = (font_size * 1.22).ceil().max(font_size + 2.0);
    let ascent = font_size * 0.80;
    let h_align = resolve_h_align(style, val);
    let indent_px = f32::from(style.indent) * INDENT_STEP_PX;

    let (pad_left, pad_right) = match h_align {
        TextHAlign::Left => (CELL_PAD_X + indent_px, CELL_PAD_X),
        TextHAlign::Right => (CELL_PAD_X, CELL_PAD_X + indent_px),
        TextHAlign::Center => (CELL_PAD_X, CELL_PAD_X),
    };
    let base_inner_w = (cell_rect.width - pad_left - pad_right).max(0.0);

    // Handle `HorizontalAlign::Fill` by repeating text to cover `base_inner_w`.
    let effective_text: std::borrow::Cow<'_, str> =
        if style.horizontal_align == HorizontalAlign::Fill && !raw_text.contains('\n') {
            let unit_w = estimate_text_width_px(raw_text, font_size, style.font.bold).max(1.0);
            let count = ((base_inner_w / unit_w).ceil() as usize).clamp(1, 128);
            std::borrow::Cow::Owned(raw_text.repeat(count))
        } else {
            std::borrow::Cow::Borrowed(raw_text)
        };

    let lines = wrap_cell_text(
        &effective_text,
        base_inner_w,
        font_size,
        style.font.bold,
        style.wrap_text,
    );

    let (box_x, box_w, raw_clip) = if lines.len() == 1 && !style.wrap_text && !is_merged {
        let est_w = estimate_text_width_px(&lines[0], font_size, style.font.bold);
        let (_, _, extra_l, extra_r) = compute_overflow_extent(
            doc,
            sheet,
            coord,
            style,
            h_align,
            est_w - base_inner_w,
            col_index,
            merged_index,
        );
        let bx = cell_rect.x - extra_l + pad_left;
        let bw = (base_inner_w + extra_l + extra_r).max(0.0);
        let clip = RectF::new(
            cell_rect.x - extra_l,
            cell_rect.y,
            (cell_rect.width + extra_l + extra_r - 1.0).max(0.0),
            (cell_rect.height - 1.0).max(0.0),
        );
        (bx, bw, clip)
    } else {
        let clip = RectF::new(
            cell_rect.x,
            cell_rect.y,
            (cell_rect.width - 1.0).max(0.0),
            (cell_rect.height - 1.0).max(0.0),
        );
        (cell_rect.x + pad_left, base_inner_w, clip)
    };

    let cx0 = raw_clip.x.max(tile_clip.x);
    let cy0 = raw_clip.y.max(tile_clip.y);
    let cx1 = raw_clip.right().min(tile_clip.right());
    let cy1 = raw_clip.bottom().min(tile_clip.bottom());
    if cx1 <= cx0 || cy1 <= cy0 {
        return;
    }
    let clip_rect = RectF::new(cx0, cy0, cx1 - cx0, cy1 - cy0);

    let n_lines = lines.len() as f32;
    let total_h = n_lines * line_h;
    let pad_y = 2.5_f32.min(cell_rect.height * 0.15);
    let (first_top_y, step_y) = match style.vertical_align {
        VerticalAlign::Top => (cell_rect.y + pad_y, line_h),
        VerticalAlign::Center => (
            cell_rect.y + ((cell_rect.height - total_h) * 0.5).max(pad_y * 0.5),
            line_h,
        ),
        VerticalAlign::Bottom => (
            (cell_rect.bottom() - pad_y - total_h).max(cell_rect.y + pad_y * 0.5),
            line_h,
        ),
        VerticalAlign::Justify | VerticalAlign::Distributed => {
            if lines.len() > 1 && cell_rect.height > total_h + 2.0 * pad_y {
                let extra = cell_rect.height - 2.0 * pad_y - total_h;
                let gap = extra / ((lines.len() - 1) as f32);
                (cell_rect.y + pad_y, line_h + gap)
            } else {
                (
                    cell_rect.y + ((cell_rect.height - total_h) * 0.5).max(pad_y * 0.5),
                    line_h,
                )
            }
        }
    };

    let family = if style.font.name.is_empty()
        || style.font.name.eq_ignore_ascii_case("Calibri")
        || style.font.name.eq_ignore_ascii_case("Inter")
    {
        None
    } else {
        Some(style.font.name.clone())
    };
    let cond_color = style
        .number_format
        .contains('[')
        .then(|| ordia_sheet_model::format_excel_value(val, &style.number_format).color)
        .flatten();
    let resolved_fg = cond_color
        .or(style.font.color)
        .map_or(default_text_color, to_render_color);
    let base_style = TextRunStyle {
        font_family: family,
        font_size,
        weight: if style.font.bold {
            FontWeight::Bold
        } else {
            FontWeight::Regular
        },
        italic: style.font.italic,
        underline: style.font.underline,
        strikethrough: style.font.strikethrough,
        color: resolved_fg,
        h_align,
        max_width: Some(box_w),
    };

    dl.with_clip(clip_rect, |clipped| {
        for (idx, line) in lines.into_iter().enumerate() {
            if line.is_empty() {
                continue;
            }
            let baseline_y = first_top_y + (idx as f32) * step_y + ascent;
            if baseline_y - font_size > clip_rect.bottom() {
                break;
            }
            clipped.draw_text(Point::new(box_x, baseline_y), line, base_style.clone());
        }
    });
}
