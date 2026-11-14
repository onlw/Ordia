//! Full-viewport 4-quadrant `DisplayList` composer for deterministic visual snapshots and headless export (`M2-05`).
//!
//! Composes frozen and scrollable quadrants from `64×16` [`SheetTileCache`] tiles (with lossless
//! `geom.zoom` vector scaling), column/row headers, double-line hidden row/column indicators, and
//! frozen pane divider lines into a single platform-independent [`DisplayList`].

use crate::axis::GridAxisIndex;
use crate::merged::MergedRangeIndex;
use crate::tile_cache::{
    SheetTileCache, TILE_COLS, TILE_ROWS, TileKey, TileRenderTheme,
    build_tile_display_list_with_merges,
};
use crate::viewport::{PaneRegion, ViewportGeometry};
use ordia_render::{
    Color, DashPattern, DisplayList, FontWeight, Point, RectF, RenderCommand, StrokeStyle,
    TextHAlign, TextRunStyle,
};
use ordia_sheet_model::{SheetDocument, col_to_letters};

fn scale_dash(d: DashPattern, s: f32) -> DashPattern {
    match d {
        DashPattern::Simple(a) => DashPattern::Simple(a.map(|v| (v * s).max(0.5))),
        DashPattern::DashDot(a) => DashPattern::DashDot(a.map(|v| (v * s).max(0.5))),
        DashPattern::DashDotDot(a) => DashPattern::DashDotDot(a.map(|v| (v * s).max(0.5))),
    }
}

fn append_scaled_commands(dst: &mut DisplayList, src: &[RenderCommand], ox: f32, oy: f32, s: f32) {
    for cmd in src {
        match cmd {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } => {
                dst.commands.push(RenderCommand::FillRect {
                    rect: RectF::new(
                        ox + rect.x * s,
                        oy + rect.y * s,
                        rect.width * s,
                        rect.height * s,
                    ),
                    color: *color,
                    corner_radius: *corner_radius * s,
                });
            }
            RenderCommand::StrokeRect { rect, stroke } => {
                dst.commands.push(RenderCommand::StrokeRect {
                    rect: RectF::new(
                        ox + rect.x * s,
                        oy + rect.y * s,
                        rect.width * s,
                        rect.height * s,
                    ),
                    stroke: StrokeStyle {
                        width: (stroke.width * s).max(0.5),
                        color: stroke.color,
                        dash: stroke.dash.map(|d| scale_dash(d, s)),
                    },
                });
            }
            RenderCommand::DrawLine { start, end, stroke } => {
                dst.commands.push(RenderCommand::DrawLine {
                    start: Point::new(ox + start.x * s, oy + start.y * s),
                    end: Point::new(ox + end.x * s, oy + end.y * s),
                    stroke: StrokeStyle {
                        width: (stroke.width * s).max(0.5),
                        color: stroke.color,
                        dash: stroke.dash.map(|d| scale_dash(d, s)),
                    },
                });
            }
            RenderCommand::DrawText {
                origin,
                text,
                style,
            } => {
                let mut scaled = style.clone();
                scaled.font_size = (style.font_size * s).clamp(3.0, 240.0);
                scaled.max_width = style.max_width.map(|w| w * s);
                dst.commands.push(RenderCommand::DrawText {
                    origin: Point::new(ox + origin.x * s, oy + origin.y * s),
                    text: text.clone(),
                    style: scaled,
                });
            }
            RenderCommand::PushClip { rect } => {
                dst.commands.push(RenderCommand::PushClip {
                    rect: RectF::new(
                        ox + rect.x * s,
                        oy + rect.y * s,
                        rect.width * s,
                        rect.height * s,
                    ),
                });
            }
            RenderCommand::PopClip => {
                dst.commands.push(RenderCommand::PopClip);
            }
        }
    }
}

/// Composes a complete 4-quadrant viewport frame into a platform-independent [`DisplayList`].
#[must_use]
pub fn render_viewport_display_list(
    doc: &SheetDocument,
    geom: &ViewportGeometry,
    row_index: &GridAxisIndex,
    col_index: &GridAxisIndex,
    merged_index: &MergedRangeIndex,
    tile_cache: &mut SheetTileCache,
) -> DisplayList {
    let cw = geom.canvas_rect.width;
    let ch = geom.canvas_rect.height;
    let rhw = geom.row_header_width;
    let chh = geom.col_header_height;
    let s = geom.zoom.max(0.10);
    let sheet = doc.active_sheet();
    let show_grid = doc.show_grid_lines_on(sheet);

    let tile_theme = TileRenderTheme {
        default_text_color: Color::rgb(15, 23, 42),
        grid_line_color: show_grid.then_some(Color::rgb(226, 232, 240)),
    };

    let mut dl = DisplayList::new(cw, ch);
    dl.fill_rect(
        RectF::new(rhw, chh, (cw - rhw).max(0.0), (ch - chh).max(0.0)),
        Color::WHITE,
    );

    let panes: [&Option<PaneRegion>; 4] = [
        &geom.main_pane,
        &geom.top_pane,
        &geom.left_pane,
        &geom.top_left_pane,
    ];
    for pane in panes.into_iter().flatten() {
        let clip = RectF::new(
            pane.clip_rect.x,
            pane.clip_rect.y,
            pane.clip_rect.width,
            pane.clip_rect.height,
        );
        dl.with_clip(clip, |clipped| {
            let r0 = *pane.rows.start() / TILE_ROWS;
            let r1 = *pane.rows.end() / TILE_ROWS;
            let c0 = *pane.cols.start() / TILE_COLS;
            let c1 = *pane.cols.end() / TILE_COLS;
            for tr in r0..=r1 {
                for tc in c0..=c1 {
                    let key = TileKey::new(sheet, tr, tc);
                    let tile = tile_cache.get_or_build(key, |k| {
                        build_tile_display_list_with_merges(
                            doc,
                            k,
                            row_index,
                            col_index,
                            merged_index,
                            tile_theme,
                        )
                    });
                    let ox =
                        pane.clip_rect.x + ((tile.sheet_origin_x - pane.sheet_origin_x) as f32) * s;
                    let oy =
                        pane.clip_rect.y + ((tile.sheet_origin_y - pane.sheet_origin_y) as f32) * s;
                    append_scaled_commands(clipped, &tile.display_list.commands, ox, oy, s);
                }
            }
        });
    }

    // Headers + hidden row/col double-line indicators
    let header_bg = Color::rgb(241, 245, 249);
    let header_border = Color::rgb(203, 213, 225);
    let header_fg = Color::rgb(71, 85, 105);
    let hidden_marker = Color::rgb(37, 99, 235);

    dl.fill_rect(RectF::new(0.0, 0.0, cw, chh), header_bg);
    dl.fill_rect(RectF::new(0.0, chh - 1.0, cw, 1.0), header_border);
    dl.fill_rect(RectF::new(0.0, chh, rhw, (ch - chh).max(0.0)), header_bg);
    dl.fill_rect(RectF::new(rhw - 1.0, 0.0, 1.0, ch), header_border);

    let hdr_font_sz = (11.0 * s.clamp(0.7, 1.6)).clamp(7.0, 22.0);
    let col_panes = [
        geom.top_left_pane.as_ref().or(geom.left_pane.as_ref()),
        geom.top_pane.as_ref().or(geom.main_pane.as_ref()),
    ];
    for pane in col_panes.into_iter().flatten() {
        let hclip = RectF::new(pane.clip_rect.x, 0.0, pane.clip_rect.width, chh);
        dl.with_clip(hclip, |clipped| {
            for col in pane.cols.clone() {
                let w = col_index.span_size(col) * s;
                if w <= 0.0 {
                    continue;
                }
                let x = pane.clip_rect.x
                    + ((col_index.index_to_offset(col) - pane.sheet_origin_x) as f32) * s;
                clipped.fill_rect(RectF::new(x + w - 1.0, 0.0, 1.0, chh), header_border);
                if col_index.has_hidden_before(col) {
                    clipped.fill_rect(RectF::new(x, 0.0, 1.0, chh), hidden_marker);
                    clipped.fill_rect(RectF::new(x + 2.5, 0.0, 1.0, chh), hidden_marker);
                }
                clipped.draw_text(
                    Point::new(x + 2.0, chh * 0.72),
                    col_to_letters(col),
                    TextRunStyle {
                        font_family: None,
                        font_size: hdr_font_sz,
                        weight: FontWeight::Medium,
                        italic: false,
                        underline: false,
                        strikethrough: false,
                        color: header_fg,
                        h_align: TextHAlign::Center,
                        max_width: Some((w - 4.0).max(1.0)),
                    },
                );
            }
        });
    }

    let row_panes = [
        geom.top_left_pane.as_ref().or(geom.top_pane.as_ref()),
        geom.left_pane.as_ref().or(geom.main_pane.as_ref()),
    ];
    for pane in row_panes.into_iter().flatten() {
        let hclip = RectF::new(0.0, pane.clip_rect.y, rhw, pane.clip_rect.height);
        dl.with_clip(hclip, |clipped| {
            for row in pane.rows.clone() {
                let h = row_index.span_size(row) * s;
                if h <= 0.0 {
                    continue;
                }
                let y = pane.clip_rect.y
                    + ((row_index.index_to_offset(row) - pane.sheet_origin_y) as f32) * s;
                clipped.fill_rect(RectF::new(0.0, y + h - 1.0, rhw, 1.0), header_border);
                if row_index.has_hidden_before(row) {
                    clipped.fill_rect(RectF::new(0.0, y, rhw, 1.0), hidden_marker);
                    clipped.fill_rect(RectF::new(0.0, y + 2.5, rhw, 1.0), hidden_marker);
                }
                clipped.draw_text(
                    Point::new(2.0, y + h * 0.68),
                    (row + 1).to_string(),
                    TextRunStyle {
                        font_family: None,
                        font_size: hdr_font_sz,
                        weight: FontWeight::Regular,
                        italic: false,
                        underline: false,
                        strikethrough: false,
                        color: header_fg,
                        h_align: TextHAlign::Center,
                        max_width: Some((rhw - 4.0).max(1.0)),
                    },
                );
            }
        });
    }

    // Frozen pane divider lines
    let divider_color = Color::rgb(100, 116, 139);
    if geom.frozen_height > 0.0 {
        dl.fill_rect(
            RectF::new(0.0, chh + geom.frozen_height - 1.0, cw, 2.0),
            divider_color,
        );
    }
    if geom.frozen_width > 0.0 {
        dl.fill_rect(
            RectF::new(rhw + geom.frozen_width - 1.0, 0.0, 2.0, ch),
            divider_color,
        );
    }

    dl
}
