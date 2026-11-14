//! Four-quadrant frozen-pane spreadsheet viewport geometry, hit-testing, and scroll clamping.

use crate::axis::GridAxisIndex;
use ordia_sheet_model::{CellCoord, FreezePane};
use std::ops::RangeInclusive;

/// Default width of the left row-number header gutter in logical pixels.
pub const DEFAULT_ROW_HEADER_WIDTH_PX: f32 = 54.0;

/// Default height of the top column-letter header bar in logical pixels.
pub const DEFAULT_COL_HEADER_HEIGHT_PX: f32 = 24.0;

/// Axis-aligned rectangle in viewport-local logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LogicalRect {
    /// Left X coordinate relative to the grid canvas origin.
    pub x: f32,
    /// Top Y coordinate relative to the grid canvas origin.
    pub y: f32,
    /// Width in logical pixels (`>= 0`).
    pub width: f32,
    /// Height in logical pixels (`>= 0`).
    pub height: f32,
}

impl LogicalRect {
    /// Constructs a logical rectangle.
    #[must_use]
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Right X edge (`x + width`).
    #[must_use]
    pub const fn right(self) -> f32 {
        self.x + self.width
    }

    /// Bottom Y edge (`y + height`).
    #[must_use]
    pub const fn bottom(self) -> f32 {
        self.y + self.height
    }

    /// Returns `true` if both `width > 0` and `height > 0`.
    #[must_use]
    pub const fn is_non_empty(self) -> bool {
        self.width > 0.0 && self.height > 0.0
    }

    /// Returns `true` if `(px, py)` lies inside `[x, right) x [y, bottom)`.
    #[must_use]
    pub const fn contains(self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.width && py >= self.y && py < self.y + self.height
    }
}

/// One of up to four frozen/scrollable cell quadrants in the grid viewport.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneRegion {
    /// Clipping rectangle of this quadrant in viewport-local logical pixels.
    pub clip_rect: LogicalRect,
    /// Visible inclusive row index range in this quadrant.
    pub rows: RangeInclusive<u32>,
    /// Visible inclusive column index range in this quadrant.
    pub cols: RangeInclusive<u32>,
    /// Sheet-space X coordinate corresponding to `clip_rect.x`.
    pub sheet_origin_x: f64,
    /// Sheet-space Y coordinate corresponding to `clip_rect.y`.
    pub sheet_origin_y: f64,
}

/// Result of hit-testing a pointer coordinate against a [`ViewportGeometry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridHit {
    /// Top-left select-all corner button.
    CornerHeader,
    /// Column header for zero-based `col`.
    ColHeader(u32),
    /// Row header for zero-based `row`.
    RowHeader(u32),
    /// Data cell at `CellCoord`.
    Cell(CellCoord),
    /// Outside the grid viewport bounds.
    Outside,
}

/// Precomputed layout and scroll geometry for a single spreadsheet frame.
#[derive(Debug, Clone)]
pub struct ViewportGeometry {
    /// Full canvas rectangle (`0, 0, canvas_width, canvas_height`).
    pub canvas_rect: LogicalRect,
    /// Width of the row-number gutter on the left.
    pub row_header_width: f32,
    /// Height of the column-letter bar at the top.
    pub col_header_height: f32,
    /// Effective frozen row/column counts (clamped if viewport is smaller than frozen area).
    pub freeze: FreezePane,
    /// Pixel width of frozen columns (`cols 0..frozen_cols`).
    pub frozen_width: f32,
    /// Pixel height of frozen rows (`rows 0..frozen_rows`).
    pub frozen_height: f32,
    /// Clamped horizontal scroll offset (distance scrolled past `frozen_width`).
    pub scroll_x: f64,
    /// Clamped vertical scroll offset (distance scrolled past `frozen_height`).
    pub scroll_y: f64,
    /// Maximum valid `scroll_x`.
    pub max_scroll_x: f64,
    /// Maximum valid `scroll_y`.
    pub max_scroll_y: f64,
    /// Top-left frozen quadrant (`rows 0..frozen_rows`, `cols 0..frozen_cols`), if non-empty.
    pub top_left_pane: Option<PaneRegion>,
    /// Top frozen-rows quadrant (`rows 0..frozen_rows`, scrolled columns), if non-empty.
    pub top_pane: Option<PaneRegion>,
    /// Left frozen-columns quadrant (scrolled rows, `cols 0..frozen_cols`), if non-empty.
    pub left_pane: Option<PaneRegion>,
    /// Main scrollable quadrant (scrolled rows, scrolled columns), if non-empty.
    pub main_pane: Option<PaneRegion>,
}

/// Input configuration for [`ViewportGeometry::compute`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportParams {
    /// Total canvas width in logical pixels.
    pub canvas_width: f32,
    /// Total canvas height in logical pixels.
    pub canvas_height: f32,
    /// Row-number header gutter width in logical pixels.
    pub row_header_width: f32,
    /// Column-letter header bar height in logical pixels.
    pub col_header_height: f32,
    /// Frozen rows/columns configuration.
    pub freeze: FreezePane,
    /// Requested horizontal scroll offset in logical pixels.
    pub scroll_x: f64,
    /// Requested vertical scroll offset in logical pixels.
    pub scroll_y: f64,
}

impl ViewportGeometry {
    /// Computes the 4-quadrant frozen/scrollable viewport geometry and clamps scroll offsets.
    #[must_use]
    pub fn compute(
        params: ViewportParams,
        row_index: &GridAxisIndex,
        col_index: &GridAxisIndex,
    ) -> Self {
        let cw = params.canvas_width.max(0.0);
        let ch = params.canvas_height.max(0.0);
        let rhw = params.row_header_width.min(cw).max(0.0);
        let chh = params.col_header_height.min(ch).max(0.0);

        let data_w = (cw - rhw).max(0.0);
        let data_h = (ch - chh).max(0.0);

        let max_frozen_cols = params.freeze.frozen_cols.min(col_index.count());
        let max_frozen_rows = params.freeze.frozen_rows.min(row_index.count());

        let raw_frozen_w = col_index.range_extent(0, max_frozen_cols) as f32;
        let raw_frozen_h = row_index.range_extent(0, max_frozen_rows) as f32;

        // Ensure frozen panes never swallow the entire viewport when the window is narrow.
        let (eff_frozen_cols, frozen_w) = if raw_frozen_w < data_w * 0.8 {
            (max_frozen_cols, raw_frozen_w)
        } else {
            (0, 0.0)
        };
        let (eff_frozen_rows, frozen_h) = if raw_frozen_h < data_h * 0.8 {
            (max_frozen_rows, raw_frozen_h)
        } else {
            (0, 0.0)
        };

        let scrollable_view_w = f64::from((data_w - frozen_w).max(0.0));
        let scrollable_view_h = f64::from((data_h - frozen_h).max(0.0));

        let unfrozen_sheet_w = (col_index.total_extent() - f64::from(frozen_w)).max(0.0);
        let unfrozen_sheet_h = (row_index.total_extent() - f64::from(frozen_h)).max(0.0);

        let max_scroll_x = (unfrozen_sheet_w - scrollable_view_w).max(0.0);
        let max_scroll_y = (unfrozen_sheet_h - scrollable_view_h).max(0.0);

        let clamped_scroll_x = params.scroll_x.clamp(0.0, max_scroll_x);
        let clamped_scroll_y = params.scroll_y.clamp(0.0, max_scroll_y);

        let scrolled_sheet_x = f64::from(frozen_w) + clamped_scroll_x;
        let scrolled_sheet_y = f64::from(frozen_h) + clamped_scroll_y;

        let frozen_col_range = if eff_frozen_cols > 0 && frozen_w > 0.0 {
            Some(0..=(eff_frozen_cols - 1))
        } else {
            None
        };
        let frozen_row_range = if eff_frozen_rows > 0 && frozen_h > 0.0 {
            Some(0..=(eff_frozen_rows - 1))
        } else {
            None
        };

        let scrolled_col_range =
            col_index.visible_range(eff_frozen_cols, scrolled_sheet_x, scrollable_view_w);
        let scrolled_row_range =
            row_index.visible_range(eff_frozen_rows, scrolled_sheet_y, scrollable_view_h);

        let top_left_pane = match (frozen_row_range.clone(), frozen_col_range.clone()) {
            (Some(rows), Some(cols)) => Some(PaneRegion {
                clip_rect: LogicalRect::new(rhw, chh, frozen_w, frozen_h),
                rows,
                cols,
                sheet_origin_x: 0.0,
                sheet_origin_y: 0.0,
            }),
            _ => None,
        };

        let top_pane = match (frozen_row_range, scrolled_col_range.clone()) {
            (Some(rows), Some(cols)) if scrollable_view_w > 0.0 => Some(PaneRegion {
                clip_rect: LogicalRect::new(
                    rhw + frozen_w,
                    chh,
                    scrollable_view_w as f32,
                    frozen_h,
                ),
                rows,
                cols,
                sheet_origin_x: scrolled_sheet_x,
                sheet_origin_y: 0.0,
            }),
            _ => None,
        };

        let left_pane = match (scrolled_row_range.clone(), frozen_col_range) {
            (Some(rows), Some(cols)) if scrollable_view_h > 0.0 => Some(PaneRegion {
                clip_rect: LogicalRect::new(
                    rhw,
                    chh + frozen_h,
                    frozen_w,
                    scrollable_view_h as f32,
                ),
                rows,
                cols,
                sheet_origin_x: 0.0,
                sheet_origin_y: scrolled_sheet_y,
            }),
            _ => None,
        };

        let main_pane = match (scrolled_row_range, scrolled_col_range) {
            (Some(rows), Some(cols)) if scrollable_view_w > 0.0 && scrollable_view_h > 0.0 => {
                Some(PaneRegion {
                    clip_rect: LogicalRect::new(
                        rhw + frozen_w,
                        chh + frozen_h,
                        scrollable_view_w as f32,
                        scrollable_view_h as f32,
                    ),
                    rows,
                    cols,
                    sheet_origin_x: scrolled_sheet_x,
                    sheet_origin_y: scrolled_sheet_y,
                })
            }
            _ => None,
        };

        Self {
            canvas_rect: LogicalRect::new(0.0, 0.0, cw, ch),
            row_header_width: rhw,
            col_header_height: chh,
            freeze: FreezePane {
                frozen_rows: eff_frozen_rows,
                frozen_cols: eff_frozen_cols,
            },
            frozen_width: frozen_w,
            frozen_height: frozen_h,
            scroll_x: clamped_scroll_x,
            scroll_y: clamped_scroll_y,
            max_scroll_x,
            max_scroll_y,
            top_left_pane,
            top_pane,
            left_pane,
            main_pane,
        }
    }

    /// Hit-tests a canvas-local `(x, y)` point in `O(log N)` time.
    #[must_use]
    pub fn hit_test(
        &self,
        x: f32,
        y: f32,
        row_index: &GridAxisIndex,
        col_index: &GridAxisIndex,
    ) -> GridHit {
        if !self.canvas_rect.contains(x, y) {
            return GridHit::Outside;
        }
        let in_row_header = x < self.row_header_width;
        let in_col_header = y < self.col_header_height;
        if in_row_header && in_col_header {
            return GridHit::CornerHeader;
        }

        let col = if !in_row_header {
            let sheet_x = if x < self.row_header_width + self.frozen_width {
                f64::from(x - self.row_header_width)
            } else {
                f64::from(self.frozen_width)
                    + self.scroll_x
                    + f64::from(x - self.row_header_width - self.frozen_width)
            };
            if sheet_x >= col_index.total_extent() {
                return GridHit::Outside;
            }
            Some(col_index.offset_to_index(sheet_x))
        } else {
            None
        };

        let row = if !in_col_header {
            let sheet_y = if y < self.col_header_height + self.frozen_height {
                f64::from(y - self.col_header_height)
            } else {
                f64::from(self.frozen_height)
                    + self.scroll_y
                    + f64::from(y - self.col_header_height - self.frozen_height)
            };
            if sheet_y >= row_index.total_extent() {
                return GridHit::Outside;
            }
            Some(row_index.offset_to_index(sheet_y))
        } else {
            None
        };

        match (in_col_header, in_row_header, row, col) {
            (true, false, _, Some(c)) => GridHit::ColHeader(c),
            (false, true, Some(r), _) => GridHit::RowHeader(r),
            (false, false, Some(r), Some(c)) => GridHit::Cell(CellCoord::new(r, c)),
            _ => GridHit::Outside,
        }
    }

    /// Computes the viewport-local [`LogicalRect`] of `coord` if its pane is currently visible.
    #[must_use]
    pub fn cell_viewport_rect(
        &self,
        coord: CellCoord,
        row_index: &GridAxisIndex,
        col_index: &GridAxisIndex,
    ) -> Option<LogicalRect> {
        let w = col_index.span_size(coord.col);
        let h = row_index.span_size(coord.row);
        if w <= 0.0 || h <= 0.0 {
            return None;
        }
        let cell_sheet_x = col_index.index_to_offset(coord.col);
        let cell_sheet_y = row_index.index_to_offset(coord.row);

        let is_frozen_col = coord.col < self.freeze.frozen_cols;
        let is_frozen_row = coord.row < self.freeze.frozen_rows;

        let (vx, min_x, max_x) = if is_frozen_col {
            (
                self.row_header_width + (cell_sheet_x as f32),
                self.row_header_width,
                self.row_header_width + self.frozen_width,
            )
        } else {
            let local_x = cell_sheet_x - (f64::from(self.frozen_width) + self.scroll_x);
            (
                self.row_header_width + self.frozen_width + (local_x as f32),
                self.row_header_width + self.frozen_width,
                self.canvas_rect.width,
            )
        };

        let (vy, min_y, max_y) = if is_frozen_row {
            (
                self.col_header_height + (cell_sheet_y as f32),
                self.col_header_height,
                self.col_header_height + self.frozen_height,
            )
        } else {
            let local_y = cell_sheet_y - (f64::from(self.frozen_height) + self.scroll_y);
            (
                self.col_header_height + self.frozen_height + (local_y as f32),
                self.col_header_height + self.frozen_height,
                self.canvas_rect.height,
            )
        };

        // Check if cell intersects its quadrant clip area
        if vx + w <= min_x || vx >= max_x || vy + h <= min_y || vy >= max_y {
            return None;
        }
        Some(LogicalRect::new(vx, vy, w, h))
    }

    /// Computes minimal adjusted `(scroll_x, scroll_y)` so `coord` is fully visible inside the viewport.
    #[must_use]
    pub fn scroll_into_view(
        &self,
        coord: CellCoord,
        row_index: &GridAxisIndex,
        col_index: &GridAxisIndex,
    ) -> (f64, f64) {
        let mut next_x = self.scroll_x;
        let mut next_y = self.scroll_y;

        let scrollable_w = f64::from(
            (self.canvas_rect.width - self.row_header_width - self.frozen_width).max(0.0),
        );
        let scrollable_h = f64::from(
            (self.canvas_rect.height - self.col_header_height - self.frozen_height).max(0.0),
        );

        if coord.col >= self.freeze.frozen_cols && scrollable_w > 0.0 {
            let col_start = col_index.index_to_offset(coord.col) - f64::from(self.frozen_width);
            let col_end = col_index.index_end_offset(coord.col) - f64::from(self.frozen_width);
            if col_start < next_x {
                next_x = col_start;
            } else if col_end > next_x + scrollable_w {
                next_x = (col_end - scrollable_w).max(0.0);
            }
        }

        if coord.row >= self.freeze.frozen_rows && scrollable_h > 0.0 {
            let row_start = row_index.index_to_offset(coord.row) - f64::from(self.frozen_height);
            let row_end = row_index.index_end_offset(coord.row) - f64::from(self.frozen_height);
            if row_start < next_y {
                next_y = row_start;
            } else if row_end > next_y + scrollable_h {
                next_y = (row_end - scrollable_h).max(0.0);
            }
        }

        (
            next_x.clamp(0.0, self.max_scroll_x),
            next_y.clamp(0.0, self.max_scroll_y),
        )
    }
}
