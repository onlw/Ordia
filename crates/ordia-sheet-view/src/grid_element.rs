//! Custom GPUI `Element` rendering a 4-quadrant frozen-pane virtualized spreadsheet grid.

use crate::workspace_view::SheetWorkspaceView;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    App, BorderStyle, Bounds, ContentMask, Element, ElementId, ElementInputHandler, Entity,
    GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollDelta,
    ScrollEvent, SharedString, Size, Style, TextAlign, Window, fill, outline, point, px, relative,
    rgba, size,
};
use ordia_sheet_layout::{
    DEFAULT_COL_HEADER_HEIGHT_PX, DEFAULT_ROW_HEADER_WIDTH_PX, GridHit, LogicalRect, PaneRegion,
    ViewportGeometry, ViewportParams,
};
use ordia_sheet_model::{
    CellCoord, CellRange, CellValue, ColorRgba, HorizontalAlign, MAX_COLS, MAX_ROWS, col_to_letters,
};
use std::time::Instant;

/// Converts a document cell [`ColorRgba`] into GPUI [`Hsla`].
///
/// Document cell fills and font colors represent authored `.xlsx` data rather than UI chrome,
/// and intentionally preserve their original RGB values across light and dark themes (`PLAN.md` §5.2).
fn doc_color_to_hsla(c: ColorRgba) -> Hsla {
    let packed = u32::from_be_bytes([c.r, c.g, c.b, c.a]);
    Hsla::from(rgba(packed))
}

/// Converts a viewport-local [`LogicalRect`] into window-space [`Bounds<Pixels>`].
fn to_window_bounds(canvas_origin: Point<Pixels>, rect: LogicalRect) -> Bounds<Pixels> {
    Bounds::new(
        point(canvas_origin.x + px(rect.x), canvas_origin.y + px(rect.y)),
        size(px(rect.width.max(0.0)), px(rect.height.max(0.0))),
    )
}

/// Low-level virtualized spreadsheet grid [`Element`] backed by [`SheetWorkspaceView`].
pub struct SheetGridElement {
    id: ElementId,
    view: Entity<SheetWorkspaceView>,
}

/// Layout-to-prepaint state for [`SheetGridElement`].
pub struct GridLayoutState {
    start_instant: Instant,
}

/// Prepaint-to-paint state for [`SheetGridElement`].
pub struct GridPrepaintState {
    hitbox: Hitbox,
    geometry: ViewportGeometry,
}

impl SheetGridElement {
    /// Creates a virtualized grid element bound to `view`.
    #[must_use]
    pub fn new(id: impl Into<ElementId>, view: Entity<SheetWorkspaceView>) -> Self {
        Self {
            id: id.into(),
            view,
        }
    }
}

impl IntoElement for SheetGridElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for SheetGridElement {
    type RequestLayoutState = GridLayoutState;
    type PrepaintState = GridPrepaintState;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let start_instant = Instant::now();
        let style = Style {
            size: Size {
                width: relative(1.0).into(),
                height: relative(1.0).into(),
            },
            flex_grow: 1.0,
            ..Default::default()
        };
        let layout_id = window.request_layout(style, std::iter::empty(), cx);
        (layout_id, GridLayoutState { start_instant })
    }

    fn prepaint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let cw = f32::from(bounds.size.width).max(0.0);
        let ch = f32::from(bounds.size.height).max(0.0);

        let geometry = self.view.update(cx, |state, _cx| {
            state.ensure_layout_indices();
            let geom = ViewportGeometry::compute(
                ViewportParams {
                    canvas_width: cw,
                    canvas_height: ch,
                    row_header_width: DEFAULT_ROW_HEADER_WIDTH_PX,
                    col_header_height: DEFAULT_COL_HEADER_HEIGHT_PX,
                    freeze: state.freeze_pane_cached(),
                    scroll_x: state.scroll_x(),
                    scroll_y: state.scroll_y(),
                },
                state.row_index(),
                state.col_index(),
            );
            state.record_resolved_viewport(bounds, &geom);
            geom
        });

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        GridPrepaintState { hitbox, geometry }
    }

    fn paint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let origin = bounds.origin;
        let geom = &prepaint.geometry;

        let bg_color = cx.theme().background;
        let fg_color = cx.theme().foreground;
        let header_bg = cx.theme().secondary;
        let header_fg = cx.theme().muted_foreground;
        let header_active_bg = cx.theme().accent;
        let header_active_fg = cx.theme().foreground;
        let grid_line_color = cx.theme().border;
        let primary_color = cx.theme().primary;
        let selection_tint = primary_color.opacity(0.10);
        let frozen_divider_color = cx.theme().muted_foreground.opacity(0.45);
        let ui_font_family = cx.theme().font_family.clone();

        // 1. Base canvas background
        window.paint_quad(fill(bounds, bg_color));

        // 2. Paint each visible cell quadrant with hardware scissor clipping
        let panes = [
            geom.main_pane.as_ref(),
            geom.top_pane.as_ref(),
            geom.left_pane.as_ref(),
            geom.top_left_pane.as_ref(),
        ];

        for pane in panes.into_iter().flatten() {
            paint_cell_pane(
                &self.view,
                pane,
                origin,
                grid_line_color,
                fg_color,
                primary_color,
                selection_tint,
                &ui_font_family,
                window,
                cx,
            );
        }

        // 3. Paint top column headers, left row headers, and corner select-all box
        paint_headers(
            &self.view,
            geom,
            origin,
            header_bg,
            header_fg,
            header_active_bg,
            header_active_fg,
            grid_line_color,
            &ui_font_family,
            window,
            cx,
        );

        // 4. Paint frozen pane divider hairlines
        if geom.frozen_height > 0.0 {
            let fy = geom.col_header_height + geom.frozen_height - 1.0;
            window.paint_quad(fill(
                to_window_bounds(
                    origin,
                    LogicalRect::new(0.0, fy, geom.canvas_rect.width, 1.5),
                ),
                frozen_divider_color,
            ));
        }
        if geom.frozen_width > 0.0 {
            let fx = geom.row_header_width + geom.frozen_width - 1.0;
            window.paint_quad(fill(
                to_window_bounds(
                    origin,
                    LogicalRect::new(fx, 0.0, 1.5, geom.canvas_rect.height),
                ),
                frozen_divider_color,
            ));
        }

        // 5. Paint minimalist proportional scrollbars on trailing edges
        paint_scrollbars(geom, origin, header_fg, window);

        // 6. Outer border hairline
        window.paint_quad(outline(bounds, grid_line_color, BorderStyle::Solid));

        // 7. Register OS IME & direct-type `ElementInputHandler` on the grid's focus handle when not in inline Input mode
        let (is_editing, focus_handle, active_cell_bounds) = {
            let state = self.view.read(cx);
            let active = state.document().active_cell();
            let cell_rect = geom
                .cell_viewport_rect(active, state.row_index(), state.col_index())
                .unwrap_or_else(|| {
                    LogicalRect::new(geom.row_header_width, geom.col_header_height, 90.0, 25.0)
                });
            (
                state.editing_cell().is_some(),
                state.grid_focus_handle().clone(),
                to_window_bounds(origin, cell_rect),
            )
        };

        if !is_editing {
            window.handle_input(
                &focus_handle,
                ElementInputHandler::new(active_cell_bounds, self.view.clone()),
                cx,
            );
        }

        // 8. Pointer & wheel interactions
        register_grid_interactions(&self.view, &prepaint.hitbox, bounds, geom.clone(), window);

        // 9. Record total frame duration in ms
        let elapsed_ms = request_layout.start_instant.elapsed().as_secs_f32() * 1000.0;
        self.view.update(cx, |state, _cx| {
            state.record_frame_time_ms(elapsed_ms);
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_cell_pane(
    view: &Entity<SheetWorkspaceView>,
    pane: &PaneRegion,
    origin: Point<Pixels>,
    grid_line_color: Hsla,
    default_fg: Hsla,
    primary_color: Hsla,
    selection_tint: Hsla,
    ui_font_family: &SharedString,
    window: &mut Window,
    cx: &mut App,
) {
    let pane_bounds = to_window_bounds(origin, pane.clip_rect);
    window.with_content_mask(
        Some(ContentMask {
            bounds: pane_bounds,
        }),
        |window| {
            let (selection, active_cell, show_grid) = {
                let state = view.read(cx);
                let show_grid = state
                    .document()
                    .active_sheet_info()
                    .is_none_or(|info| info.show_grid_lines);
                (
                    state.document().selection(),
                    state.document().active_cell(),
                    show_grid,
                )
            };

            // Gridlines in O(visible_rows + visible_cols) quads
            if show_grid {
                let state = view.read(cx);
                for row in pane.rows.clone() {
                    let h = state.row_index().span_size(row);
                    if h <= 0.0 {
                        continue;
                    }
                    let y = pane.clip_rect.y
                        + ((state.row_index().index_to_offset(row) - pane.sheet_origin_y) as f32)
                        + h
                        - 1.0;
                    window.paint_quad(fill(
                        to_window_bounds(
                            origin,
                            LogicalRect::new(pane.clip_rect.x, y, pane.clip_rect.width, 1.0),
                        ),
                        grid_line_color,
                    ));
                }
                for col in pane.cols.clone() {
                    let w = state.col_index().span_size(col);
                    if w <= 0.0 {
                        continue;
                    }
                    let x = pane.clip_rect.x
                        + ((state.col_index().index_to_offset(col) - pane.sheet_origin_x) as f32)
                        + w
                        - 1.0;
                    window.paint_quad(fill(
                        to_window_bounds(
                            origin,
                            LogicalRect::new(x, pane.clip_rect.y, 1.0, pane.clip_rect.height),
                        ),
                        grid_line_color,
                    ));
                }
            }

            // Visible cells: fills, multi-cell selection tint, and shaped single-line text
            for row in pane.rows.clone() {
                let (row_y, row_h) = {
                    let state = view.read(cx);
                    let h = state.row_index().span_size(row);
                    let y = pane.clip_rect.y
                        + ((state.row_index().index_to_offset(row) - pane.sheet_origin_y) as f32);
                    (y, h)
                };
                if row_h <= 0.0 {
                    continue;
                }

                for col in pane.cols.clone() {
                    let (col_x, col_w) = {
                        let state = view.read(cx);
                        let w = state.col_index().span_size(col);
                        let x = pane.clip_rect.x
                            + ((state.col_index().index_to_offset(col) - pane.sheet_origin_x)
                                as f32);
                        (x, w)
                    };
                    if col_w <= 0.0 {
                        continue;
                    }

                    let coord = CellCoord::new(row, col);
                    let cell_rect = LogicalRect::new(col_x, row_y, col_w - 1.0, row_h - 1.0);

                    let (display_text, val_kind, style) = {
                        let state = view.read(cx);
                        let doc = state.document();
                        let text = doc.cell_display_text(coord).unwrap_or_default();
                        let style = doc.cell_style(coord).unwrap_or_default();
                        let val = if text.is_empty() {
                            CellValue::Empty
                        } else {
                            doc.cell_value(coord).unwrap_or_default()
                        };
                        (text, val, style)
                    };

                    if let Some(fill_rgb) = style.fill_color {
                        window.paint_quad(fill(
                            to_window_bounds(origin, cell_rect),
                            doc_color_to_hsla(fill_rgb),
                        ));
                    }

                    if selection.contains(coord) && coord != active_cell {
                        window
                            .paint_quad(fill(to_window_bounds(origin, cell_rect), selection_tint));
                    }

                    if !display_text.is_empty() {
                        let text_color = style.font.color.map_or(default_fg, doc_color_to_hsla);
                        let font_px = px((style.font.size_pt * 1.18).clamp(11.0, 28.0));
                        let shaped = view.update(cx, |state, _cx| {
                            state.shape_cached_line(
                                window,
                                SharedString::from(display_text),
                                ui_font_family.clone(),
                                font_px,
                                style.font.bold,
                                style.font.italic,
                                text_color,
                            )
                        });

                        let pad_x = 6.0_f32;
                        let inner_w = (col_w - pad_x * 2.0).max(0.0);
                        let align = match style.horizontal_align {
                            HorizontalAlign::Right => TextAlign::Right,
                            HorizontalAlign::Center | HorizontalAlign::CenterContinuous => {
                                TextAlign::Center
                            }
                            HorizontalAlign::General => match val_kind {
                                CellValue::Number(_) => TextAlign::Right,
                                CellValue::Boolean(_) | CellValue::Error(_) => TextAlign::Center,
                                CellValue::Empty | CellValue::Text(_) => TextAlign::Left,
                            },
                            _ => TextAlign::Left,
                        };

                        if shaped.width > px(inner_w) {
                            let cell_bounds = to_window_bounds(origin, cell_rect);
                            window.with_content_mask(
                                Some(ContentMask {
                                    bounds: cell_bounds,
                                }),
                                |window| {
                                    let _ = shaped.paint(
                                        point(origin.x + px(col_x + pad_x), origin.y + px(row_y)),
                                        px(row_h),
                                        align,
                                        Some(px(inner_w)),
                                        window,
                                        cx,
                                    );
                                },
                            );
                        } else {
                            let _ = shaped.paint(
                                point(origin.x + px(col_x + pad_x), origin.y + px(row_y)),
                                px(row_h),
                                align,
                                Some(px(inner_w)),
                                window,
                                cx,
                            );
                        }
                    }
                }
            }

            // Selection ring inside this pane if selection intersects it
            let pane_range = CellRange::new(
                CellCoord::new(*pane.rows.start(), *pane.cols.start()),
                CellCoord::new(*pane.rows.end(), *pane.cols.end()),
            );
            if selection.intersects(pane_range) {
                let (sx, sy, sw, sh) = {
                    let state = view.read(cx);
                    let x0 = pane.clip_rect.x
                        + ((state.col_index().index_to_offset(selection.start.col)
                            - pane.sheet_origin_x) as f32);
                    let x1 = pane.clip_rect.x
                        + ((state.col_index().index_end_offset(selection.end.col)
                            - pane.sheet_origin_x) as f32);
                    let y0 = pane.clip_rect.y
                        + ((state.row_index().index_to_offset(selection.start.row)
                            - pane.sheet_origin_y) as f32);
                    let y1 = pane.clip_rect.y
                        + ((state.row_index().index_end_offset(selection.end.row)
                            - pane.sheet_origin_y) as f32);
                    (x0, y0, (x1 - x0).max(1.0), (y1 - y0).max(1.0))
                };
                let sel_bounds = to_window_bounds(origin, LogicalRect::new(sx, sy, sw, sh));
                let mut quad = outline(sel_bounds, primary_color, BorderStyle::Solid);
                quad.border_widths = px(2.0).into();
                window.paint_quad(quad);

                // Fill-handle square at bottom-right corner
                let handle_rect = LogicalRect::new(sx + sw - 3.5, sy + sh - 3.5, 6.0, 6.0);
                window.paint_quad(fill(to_window_bounds(origin, handle_rect), primary_color));
            }
        },
    );
}

#[allow(clippy::too_many_arguments)]
fn paint_headers(
    view: &Entity<SheetWorkspaceView>,
    geom: &ViewportGeometry,
    origin: Point<Pixels>,
    header_bg: Hsla,
    header_fg: Hsla,
    header_active_bg: Hsla,
    header_active_fg: Hsla,
    border_color: Hsla,
    ui_font_family: &SharedString,
    window: &mut Window,
    cx: &mut App,
) {
    let selection = view.read(cx).document().selection();
    let rhw = geom.row_header_width;
    let chh = geom.col_header_height;

    // Top column header strip background + bottom border
    window.paint_quad(fill(
        to_window_bounds(
            origin,
            LogicalRect::new(0.0, 0.0, geom.canvas_rect.width, chh),
        ),
        header_bg,
    ));
    window.paint_quad(fill(
        to_window_bounds(
            origin,
            LogicalRect::new(0.0, chh - 1.0, geom.canvas_rect.width, 1.0),
        ),
        border_color,
    ));

    // Left row header strip background + right border
    window.paint_quad(fill(
        to_window_bounds(
            origin,
            LogicalRect::new(0.0, chh, rhw, (geom.canvas_rect.height - chh).max(0.0)),
        ),
        header_bg,
    ));
    window.paint_quad(fill(
        to_window_bounds(
            origin,
            LogicalRect::new(rhw - 1.0, 0.0, 1.0, geom.canvas_rect.height),
        ),
        border_color,
    ));

    // Column headers (frozen + scrolled)
    let col_panes = [
        geom.top_left_pane.as_ref().or(geom.left_pane.as_ref()),
        geom.top_pane.as_ref().or(geom.main_pane.as_ref()),
    ];
    for maybe_pane in col_panes.into_iter().flatten() {
        let header_clip = to_window_bounds(
            origin,
            LogicalRect::new(maybe_pane.clip_rect.x, 0.0, maybe_pane.clip_rect.width, chh),
        );
        window.with_content_mask(
            Some(ContentMask {
                bounds: header_clip,
            }),
            |window| {
                for col in maybe_pane.cols.clone() {
                    let (x, w) = {
                        let state = view.read(cx);
                        let w = state.col_index().span_size(col);
                        let x = maybe_pane.clip_rect.x
                            + ((state.col_index().index_to_offset(col) - maybe_pane.sheet_origin_x)
                                as f32);
                        (x, w)
                    };
                    if w <= 0.0 {
                        continue;
                    }
                    let is_selected = col >= selection.start.col && col <= selection.end.col;
                    if is_selected {
                        window.paint_quad(fill(
                            to_window_bounds(origin, LogicalRect::new(x, 0.0, w - 1.0, chh - 1.0)),
                            header_active_bg,
                        ));
                    }
                    window.paint_quad(fill(
                        to_window_bounds(origin, LogicalRect::new(x + w - 1.0, 0.0, 1.0, chh)),
                        border_color,
                    ));
                    let label = SharedString::from(col_to_letters(col));
                    let fg = if is_selected {
                        header_active_fg
                    } else {
                        header_fg
                    };
                    let shaped = view.update(cx, |state, _cx| {
                        state.shape_cached_line(
                            window,
                            label,
                            ui_font_family.clone(),
                            px(11.5),
                            is_selected,
                            false,
                            fg,
                        )
                    });
                    let _ = shaped.paint(
                        point(origin.x + px(x), origin.y),
                        px(chh),
                        TextAlign::Center,
                        Some(px(w)),
                        window,
                        cx,
                    );
                }
            },
        );
    }

    // Row headers (frozen + scrolled)
    let row_panes = [
        geom.top_left_pane.as_ref().or(geom.top_pane.as_ref()),
        geom.left_pane.as_ref().or(geom.main_pane.as_ref()),
    ];
    for maybe_pane in row_panes.into_iter().flatten() {
        let header_clip = to_window_bounds(
            origin,
            LogicalRect::new(
                0.0,
                maybe_pane.clip_rect.y,
                rhw,
                maybe_pane.clip_rect.height,
            ),
        );
        window.with_content_mask(
            Some(ContentMask {
                bounds: header_clip,
            }),
            |window| {
                for row in maybe_pane.rows.clone() {
                    let (y, h) = {
                        let state = view.read(cx);
                        let h = state.row_index().span_size(row);
                        let y = maybe_pane.clip_rect.y
                            + ((state.row_index().index_to_offset(row) - maybe_pane.sheet_origin_y)
                                as f32);
                        (y, h)
                    };
                    if h <= 0.0 {
                        continue;
                    }
                    let is_selected = row >= selection.start.row && row <= selection.end.row;
                    if is_selected {
                        window.paint_quad(fill(
                            to_window_bounds(origin, LogicalRect::new(0.0, y, rhw - 1.0, h - 1.0)),
                            header_active_bg,
                        ));
                    }
                    window.paint_quad(fill(
                        to_window_bounds(origin, LogicalRect::new(0.0, y + h - 1.0, rhw, 1.0)),
                        border_color,
                    ));
                    let label = SharedString::from((row + 1).to_string());
                    let fg = if is_selected {
                        header_active_fg
                    } else {
                        header_fg
                    };
                    let shaped = view.update(cx, |state, _cx| {
                        state.shape_cached_line(
                            window,
                            label,
                            ui_font_family.clone(),
                            px(11.0),
                            is_selected,
                            false,
                            fg,
                        )
                    });
                    let _ = shaped.paint(
                        point(origin.x, origin.y + px(y)),
                        px(h),
                        TextAlign::Center,
                        Some(px(rhw - 2.0)),
                        window,
                        cx,
                    );
                }
            },
        );
    }
}

fn paint_scrollbars(
    geom: &ViewportGeometry,
    origin: Point<Pixels>,
    thumb_base_color: Hsla,
    window: &mut Window,
) {
    let thumb_color = thumb_base_color.opacity(0.35);
    let track_h = (geom.canvas_rect.height - geom.col_header_height - 10.0).max(0.0);
    if geom.max_scroll_y > 0.0 && track_h > 24.0 {
        let ratio = (geom.scroll_y / geom.max_scroll_y).clamp(0.0, 1.0) as f32;
        let thumb_h = 32.0_f32.min(track_h * 0.5);
        let thumb_y = geom.col_header_height + 2.0 + ratio * (track_h - thumb_h);
        let mut quad = fill(
            to_window_bounds(
                origin,
                LogicalRect::new(geom.canvas_rect.width - 7.0, thumb_y, 5.0, thumb_h),
            ),
            thumb_color,
        );
        quad.corner_radii = px(2.5).into();
        window.paint_quad(quad);
    }

    let track_w = (geom.canvas_rect.width - geom.row_header_width - 10.0).max(0.0);
    if geom.max_scroll_x > 0.0 && track_w > 24.0 {
        let ratio = (geom.scroll_x / geom.max_scroll_x).clamp(0.0, 1.0) as f32;
        let thumb_w = 32.0_f32.min(track_w * 0.5);
        let thumb_x = geom.row_header_width + 2.0 + ratio * (track_w - thumb_w);
        let mut quad = fill(
            to_window_bounds(
                origin,
                LogicalRect::new(thumb_x, geom.canvas_rect.height - 7.0, thumb_w, 5.0),
            ),
            thumb_color,
        );
        quad.corner_radii = px(2.5).into();
        window.paint_quad(quad);
    }
}

fn register_grid_interactions(
    view: &Entity<SheetWorkspaceView>,
    hitbox: &Hitbox,
    bounds: Bounds<Pixels>,
    geom: ViewportGeometry,
    window: &mut Window,
) {
    // Mouse Down: select cell/header or double-click to start inline editing
    window.on_mouse_event({
        let view = view.clone();
        let hitbox = hitbox.clone();
        let geom = geom.clone();
        move |event: &MouseDownEvent, phase, window, cx| {
            if !phase.bubble() || event.button != MouseButton::Left || !hitbox.is_hovered(window) {
                return;
            }
            let local_x = f32::from(event.position.x - bounds.origin.x);
            let local_y = f32::from(event.position.y - bounds.origin.y);
            view.update(cx, |state, cx| {
                let hit = geom.hit_test(local_x, local_y, state.row_index(), state.col_index());
                if state.editing_cell().is_some() {
                    state.commit_edit(false, window, cx);
                }
                match hit {
                    GridHit::Cell(coord) => {
                        if event.modifiers.shift {
                            let anchor = state.document().active_cell();
                            state.set_selection_range(anchor, coord, cx);
                        } else {
                            state.select_cell(coord, window, cx);
                        }
                        if event.click_count >= 2 {
                            state.begin_edit_existing(coord, window, cx);
                        } else {
                            state.set_pointer_dragging(true);
                        }
                    }
                    GridHit::RowHeader(row) => {
                        state.select_cell(CellCoord::new(row, 0), window, cx);
                        state.set_selection_range(
                            CellCoord::new(row, 0),
                            CellCoord::new(row, MAX_COLS - 1),
                            cx,
                        );
                    }
                    GridHit::ColHeader(col) => {
                        state.select_cell(CellCoord::new(0, col), window, cx);
                        state.set_selection_range(
                            CellCoord::new(0, col),
                            CellCoord::new(MAX_ROWS - 1, col),
                            cx,
                        );
                    }
                    GridHit::CornerHeader => {
                        state.select_cell(CellCoord::new(0, 0), window, cx);
                        state.set_selection_range(
                            CellCoord::new(0, 0),
                            CellCoord::new(MAX_ROWS - 1, MAX_COLS - 1),
                            cx,
                        );
                    }
                    GridHit::Outside => {}
                }
            });
            cx.stop_propagation();
        }
    });

    // Mouse Move: drag rectangular selection
    window.on_mouse_event({
        let view = view.clone();
        let geom = geom.clone();
        move |event: &MouseMoveEvent, phase, _window, cx| {
            if !phase.bubble() || event.pressed_button != Some(MouseButton::Left) {
                return;
            }
            let is_dragging = view.read(cx).is_pointer_dragging();
            if !is_dragging {
                return;
            }
            let local_x = f32::from(event.position.x - bounds.origin.x);
            let local_y = f32::from(event.position.y - bounds.origin.y);
            view.update(cx, |state, cx| {
                if let GridHit::Cell(target) =
                    geom.hit_test(local_x, local_y, state.row_index(), state.col_index())
                {
                    let anchor = state.document().active_cell();
                    state.set_selection_range(anchor, target, cx);
                }
            });
        }
    });

    // Mouse Up: end drag
    window.on_mouse_event({
        let view = view.clone();
        move |event: &MouseUpEvent, phase, _window, cx| {
            if phase.bubble() && event.button == MouseButton::Left {
                view.update(cx, |state, _cx| state.set_pointer_dragging(false));
            }
        }
    });

    // Scroll Wheel: smooth 2D scrolling over 1,000,000+ rows
    window.on_mouse_event({
        let view = view.clone();
        let hitbox = hitbox.clone();
        move |event: &ScrollEvent, phase, window, cx| {
            if !phase.bubble() || !hitbox.is_hovered(window) {
                return;
            }
            let (dx, dy) = match event.delta {
                ScrollDelta::Pixels(pt) => {
                    (-f64::from(f32::from(pt.x)), -f64::from(f32::from(pt.y)))
                }
                ScrollDelta::Lines(pt) => (-f64::from(pt.x) * 45.0, -f64::from(pt.y) * 50.0),
            };
            if dx.abs() > 0.01 || dy.abs() > 0.01 {
                view.update(cx, |state, cx| {
                    state.scroll_by(dx, dy, cx);
                });
                cx.stop_propagation();
            }
        }
    });
}
