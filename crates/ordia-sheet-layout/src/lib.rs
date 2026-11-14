//! Grid geometry, O(log N) coordinate indexing, tile caching, pagination, and `DisplayList` generation.

mod axis;
mod viewport;

pub use axis::GridAxisIndex;
pub use viewport::{
    DEFAULT_COL_HEADER_HEIGHT_PX, DEFAULT_ROW_HEADER_WIDTH_PX, GridHit, LogicalRect, PaneRegion,
    ViewportGeometry, ViewportParams,
};

#[cfg(test)]
mod tests {
    use super::*;
    use ordia_sheet_model::{CellCoord, FreezePane, MAX_COLS, MAX_ROWS, RowMetric};

    #[test]
    fn million_row_prefix_sum_binary_search_is_exact() {
        let overrides = vec![
            RowMetric {
                row: 5,
                height_px: 50.0,
                custom_height: true,
                hidden: false,
            },
            RowMetric {
                row: 10,
                height_px: 25.0,
                custom_height: false,
                hidden: true,
            },
        ];
        let axis = GridAxisIndex::from_row_metrics(MAX_ROWS, &overrides);
        assert_eq!(axis.count(), MAX_ROWS);
        // 1,048,576 * 25 + (50 - 25) - 25 = 26,214,400.0
        assert!((axis.total_extent() - 26_214_400.0).abs() < 1e-6);

        // Row 5 spans [125.0, 175.0)
        assert_eq!(axis.offset_to_index(124.9), 4);
        assert_eq!(axis.offset_to_index(125.0), 5);
        assert_eq!(axis.offset_to_index(174.9), 5);
        assert_eq!(axis.offset_to_index(175.0), 6);

        // Row 10 is hidden at offset 275.0 -> binary search skips zero-height row 10 and lands on row 11!
        assert_eq!(axis.span_size(10), 0.0);
        assert_eq!(axis.offset_to_index(275.0), 11);

        // Last row 1,048,575
        assert_eq!(axis.offset_to_index(26_214_399.0), MAX_ROWS - 1);
    }

    #[test]
    fn frozen_quadrant_geometry_and_hit_testing() {
        let rows = GridAxisIndex::from_row_metrics(MAX_ROWS, &[]);
        let cols = GridAxisIndex::from_col_metrics(MAX_COLS, &[]);

        // Canvas 800x600, headers 54x24, freeze 1 row (25px) + 1 col (90px)
        // Scrolled down 10,000px (400 rows) and right 900px (10 cols)
        let geom = ViewportGeometry::compute(
            ViewportParams {
                canvas_width: 800.0,
                canvas_height: 600.0,
                row_header_width: 54.0,
                col_header_height: 24.0,
                freeze: FreezePane {
                    frozen_rows: 1,
                    frozen_cols: 1,
                },
                scroll_x: 900.0,
                scroll_y: 10_000.0,
            },
            &rows,
            &cols,
        );

        assert!(geom.top_left_pane.is_some());
        assert!(geom.top_pane.is_some());
        assert!(geom.left_pane.is_some());
        let main = geom.main_pane.as_ref().expect("main pane");
        // Frozen row 0 takes 25px, scroll_y = 10,000 -> sheet_y = 10,025 -> row 401!
        assert_eq!(*main.rows.start(), 401);
        // Frozen col 0 takes 90px, scroll_x = 900 -> sheet_x = 990 -> col 11!
        assert_eq!(*main.cols.start(), 11);

        // Hit-test frozen corner A1 despite scroll!
        assert_eq!(
            geom.hit_test(60.0, 30.0, &rows, &cols),
            GridHit::Cell(CellCoord::new(0, 0))
        );
        // Hit-test scrolled main pane top-left cell -> (401, 11)
        assert_eq!(
            geom.hit_test(54.0 + 90.0 + 5.0, 24.0 + 25.0 + 5.0, &rows, &cols),
            GridHit::Cell(CellCoord::new(401, 11))
        );
        // Hit-test headers
        assert_eq!(
            geom.hit_test(10.0, 10.0, &rows, &cols),
            GridHit::CornerHeader
        );
        assert_eq!(
            geom.hit_test(54.0 + 90.0 + 5.0, 10.0, &rows, &cols),
            GridHit::ColHeader(11)
        );
        assert_eq!(
            geom.hit_test(10.0, 24.0 + 25.0 + 5.0, &rows, &cols),
            GridHit::RowHeader(401)
        );
    }
}
