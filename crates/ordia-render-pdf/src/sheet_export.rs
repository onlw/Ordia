//! Converts a [`SheetDocument`] cell range into a platform-independent [`DisplayList`] page.

use ordia_render::{
    Color, DashPattern, DisplayList, FontWeight, Point, RectF, StrokeStyle, TextRunStyle,
};
use ordia_sheet_model::{
    BorderLineStyle, CellCoord, CellRange, CellValue, ColorRgba, DEFAULT_COL_WIDTH_PX,
    DEFAULT_ROW_HEIGHT_PX, EngineError, HorizontalAlign, SheetDocument,
};

/// Page geometry and styling options when exporting a spreadsheet range to PDF.
#[derive(Clone, Debug)]
pub struct PdfExportOptions {
    /// Page width in PDF points (`595.28` = A4 portrait, `841.89` = A4 landscape).
    pub page_width_pt: f32,
    /// Page height in PDF points.
    pub page_height_pt: f32,
    /// Page margin in PDF points.
    pub margin_pt: f32,
    /// Whether faint cell gridlines should be drawn behind explicit borders.
    pub show_gridlines: bool,
}

impl Default for PdfExportOptions {
    fn default() -> Self {
        Self {
            page_width_pt: 841.89,
            page_height_pt: 595.28,
            margin_pt: 36.0,
            show_gridlines: true,
        }
    }
}

fn to_render_color(c: ColorRgba) -> Color {
    Color::rgba(c.r, c.g, c.b, c.a)
}

/// Builds a [`DisplayList`] representing `range` on the active sheet of `doc`.
pub fn build_sheet_range_display_list(
    doc: &SheetDocument,
    range: CellRange,
    options: &PdfExportOptions,
) -> Result<DisplayList, EngineError> {
    let norm = CellRange::new(range.start, range.end);
    let mut dl = DisplayList::new(options.page_width_pt, options.page_height_pt);
    dl.fill_rect(
        RectF::new(0.0, 0.0, options.page_width_pt, options.page_height_pt),
        Color::WHITE,
    );

    let col_spans = doc.custom_col_metrics()?;
    let row_metrics = doc.custom_row_metrics()?;

    let mut col_widths = Vec::with_capacity((norm.end.col - norm.start.col + 1) as usize);
    for col in norm.start.col..=norm.end.col {
        let mut w = DEFAULT_COL_WIDTH_PX;
        for span in &col_spans {
            if col >= span.start_col && col <= span.end_col {
                w = if span.hidden { 0.0 } else { span.width_px };
                break;
            }
        }
        col_widths.push(w);
    }

    let mut row_heights = Vec::with_capacity((norm.end.row - norm.start.row + 1) as usize);
    for row in norm.start.row..=norm.end.row {
        let mut h = DEFAULT_ROW_HEIGHT_PX;
        if let Ok(idx) = row_metrics.binary_search_by_key(&row, |m| m.row) {
            let m = &row_metrics[idx];
            h = if m.hidden { 0.0 } else { m.height_px };
        }
        row_heights.push(h);
    }

    let grid_stroke = StrokeStyle::solid(0.5, Color::rgb(218, 222, 228));
    let mut cur_y = options.margin_pt;

    for (r_idx, row) in (norm.start.row..=norm.end.row).enumerate() {
        let h = row_heights[r_idx];
        if h <= 0.0 {
            continue;
        }
        let mut cur_x = options.margin_pt;
        for (c_idx, col) in (norm.start.col..=norm.end.col).enumerate() {
            let w = col_widths[c_idx];
            if w <= 0.0 {
                continue;
            }
            let coord = CellCoord::new(row, col);
            let cell_rect = RectF::new(cur_x, cur_y, w, h);
            let style = doc.cell_style(coord)?;

            if let Some(bg) = style.fill_color {
                dl.fill_rect(cell_rect, to_render_color(bg));
            }
            if options.show_gridlines {
                dl.stroke_rect(cell_rect, grid_stroke);
            }

            draw_cell_borders(&mut dl, cell_rect, &style.borders);

            let text = doc.cell_display_text(coord)?;
            if !text.is_empty() {
                let value = doc.cell_value(coord)?;
                let align = match style.horizontal_align {
                    HorizontalAlign::General => match value {
                        CellValue::Number(_) => HorizontalAlign::Right,
                        CellValue::Boolean(_) | CellValue::Error(_) => HorizontalAlign::Center,
                        CellValue::Empty | CellValue::Text(_) => HorizontalAlign::Left,
                    },
                    explicit => explicit,
                };
                let font_size = style.font.size_pt.clamp(6.0, 72.0);
                let est_w = estimate_text_width(&text, font_size);
                let pad_x = 5.0;
                let text_x = match align {
                    HorizontalAlign::Right => (cur_x + w - pad_x - est_w).max(cur_x + pad_x),
                    HorizontalAlign::Center | HorizontalAlign::CenterContinuous => {
                        (cur_x + (w - est_w) * 0.5).max(cur_x + pad_x)
                    }
                    _ => cur_x + pad_x,
                };
                let baseline_y = cur_y + (h + font_size * 0.72) * 0.5;
                let fg = style
                    .font
                    .color
                    .map(to_render_color)
                    .unwrap_or_else(|| Color::rgb(24, 28, 34));
                let weight = if style.font.bold {
                    FontWeight::Bold
                } else {
                    FontWeight::Regular
                };
                let family = if style.font.name.is_empty() {
                    None
                } else {
                    Some(style.font.name.clone())
                };

                dl.with_clip(cell_rect, |clipped| {
                    clipped.draw_text(
                        Point::new(text_x, baseline_y),
                        text,
                        TextRunStyle {
                            font_family: family,
                            font_size,
                            weight,
                            italic: style.font.italic,
                            underline: style.font.underline,
                            strikethrough: style.font.strikethrough,
                            color: fg,
                            ..TextRunStyle::default()
                        },
                    );
                });
            }

            cur_x += w;
        }
        cur_y += h;
    }

    Ok(dl)
}

fn draw_cell_borders(dl: &mut DisplayList, rect: RectF, borders: &ordia_sheet_model::CellBorders) {
    let emit_edge = |dl: &mut DisplayList,
                     p0: Point,
                     p1: Point,
                     normal: (f32, f32),
                     edge: &Option<ordia_sheet_model::BorderEdge>| {
        let Some(edge) = edge else {
            return;
        };
        let color = edge
            .color
            .map(to_render_color)
            .unwrap_or_else(|| Color::rgb(60, 66, 76));
        if edge.style == BorderLineStyle::Double {
            let thin = StrokeStyle::solid(0.65, color);
            dl.draw_line(
                Point::new(p0.x - normal.0 * 0.9, p0.y - normal.1 * 0.9),
                Point::new(p1.x - normal.0 * 0.9, p1.y - normal.1 * 0.9),
                thin,
            );
            dl.draw_line(
                Point::new(p0.x + normal.0 * 0.9, p0.y + normal.1 * 0.9),
                Point::new(p1.x + normal.0 * 0.9, p1.y + normal.1 * 0.9),
                thin,
            );
            return;
        }
        let (width, dash) = match edge.style {
            BorderLineStyle::Thin => (0.75, None),
            BorderLineStyle::Medium => (1.5, None),
            BorderLineStyle::Thick => (2.25, None),
            BorderLineStyle::Double => unreachable!(),
            BorderLineStyle::Hair => (0.4, Some(DashPattern::Simple([0.8, 0.8]))),
            BorderLineStyle::Dotted => (0.75, Some(DashPattern::Simple([1.0, 1.5]))),
            BorderLineStyle::Dashed => (0.75, Some(DashPattern::Simple([3.0, 2.0]))),
            BorderLineStyle::DashDot => (0.75, Some(DashPattern::DashDot([4.0, 1.5, 1.0, 1.5]))),
            BorderLineStyle::DashDotDot => (
                0.75,
                Some(DashPattern::DashDotDot([4.0, 1.5, 1.0, 1.5, 1.0, 1.5])),
            ),
            BorderLineStyle::MediumDashed => (1.35, Some(DashPattern::Simple([4.0, 2.0]))),
            BorderLineStyle::MediumDashDot | BorderLineStyle::SlantDashDot => {
                (1.35, Some(DashPattern::DashDot([4.5, 2.0, 1.5, 2.0])))
            }
            BorderLineStyle::MediumDashDotDot => (
                1.35,
                Some(DashPattern::DashDotDot([4.5, 1.8, 1.2, 1.8, 1.2, 1.8])),
            ),
        };
        dl.draw_line(p0, p1, StrokeStyle { width, color, dash });
    };

    emit_edge(
        dl,
        Point::new(rect.x, rect.y),
        Point::new(rect.right(), rect.y),
        (0.0, 1.0),
        &borders.top,
    );
    emit_edge(
        dl,
        Point::new(rect.x, rect.bottom()),
        Point::new(rect.right(), rect.bottom()),
        (0.0, 1.0),
        &borders.bottom,
    );
    emit_edge(
        dl,
        Point::new(rect.x, rect.y),
        Point::new(rect.x, rect.bottom()),
        (1.0, 0.0),
        &borders.left,
    );
    emit_edge(
        dl,
        Point::new(rect.right(), rect.y),
        Point::new(rect.right(), rect.bottom()),
        (1.0, 0.0),
        &borders.right,
    );
}

fn estimate_text_width(text: &str, font_size: f32) -> f32 {
    text.chars()
        .map(|ch| {
            if ch.is_ascii() {
                font_size * 0.54
            } else {
                font_size * 1.0
            }
        })
        .sum()
}
