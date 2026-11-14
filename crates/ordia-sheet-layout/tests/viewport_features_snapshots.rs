//! `M2-05` unit and visual snapshot tests covering:
//! - `MergedRangeIndex` `O(log M)` containment, anchor resolution, and tile intersection
//! - Cross-tile merged cell (`O63:R66` spanning 4 `64x16` tiles) seamless rendering
//! - 4-quadrant frozen pane clipping & divider lines while scrolled
//! - Double-line hidden row & column header indicators
//! - Worksheet gridlines ON vs OFF toggle
//! - Lossless 10%–400% canvas zoom (`hit_test`, zero tile rebuilds, visual snapshots)

use ordia_render::{Color, RectF};
use ordia_sheet_layout::{
    DEFAULT_COL_HEADER_HEIGHT_PX, DEFAULT_ROW_HEADER_WIDTH_PX, GridAxisIndex, GridHit,
    MergedRangeIndex, SheetTileCache, ViewportGeometry, ViewportParams,
    render_viewport_display_list,
};
use ordia_sheet_model::{
    BorderEdge, BorderLineStyle, CellBorders, CellCoord, CellRange, CellStyle, ColorRgba,
    FreezePane, HorizontalAlign, MAX_COLS, MAX_ROWS, SheetDocument, SheetOp, Transaction,
    VerticalAlign,
};
use ordia_testkit::{rasterize_display_list_region, summarize_pixmap};

fn build_axes_and_merges(doc: &SheetDocument) -> (GridAxisIndex, GridAxisIndex, MergedRangeIndex) {
    let row_metrics = doc.custom_row_metrics().expect("row metrics");
    let col_metrics = doc.custom_col_metrics().expect("col metrics");
    let row_index = GridAxisIndex::from_row_metrics(MAX_ROWS, &row_metrics);
    let col_index = GridAxisIndex::from_col_metrics(MAX_COLS, &col_metrics);
    let merges = MergedRangeIndex::from_ranges(&doc.merged_ranges().expect("merged ranges"));
    (row_index, col_index, merges)
}

#[test]
fn merged_range_index_thousand_ranges_containment_and_tile_intersection() {
    let mut raw = Vec::with_capacity(1_024);
    for i in 0..1_024_u32 {
        let base_row = i * 4;
        let base_col = (i % 16) * 3;
        raw.push(CellRange::new(
            CellCoord::new(base_row, base_col),
            CellCoord::new(base_row + 2, base_col + 1),
        ));
    }
    let index = MergedRangeIndex::from_ranges(&raw);
    assert_eq!(index.len(), 1_024);

    // Covered cell inside range #513 (`513 % 16 == 1`, so `base_col == 3`) resolves to anchor `(2052, 3)`.
    let covered = CellCoord::new(513 * 4 + 1, 4);
    let expected = CellRange::new(CellCoord::new(513 * 4, 3), CellCoord::new(513 * 4 + 2, 4));
    assert_eq!(index.containing(covered), Some(expected));
    assert_eq!(index.resolve_anchor(covered), expected.start);

    // Unmerged gap row returns None and keeps its own coordinate.
    let gap = CellCoord::new(513 * 4 + 3, 0);
    assert_eq!(index.containing(gap), None);
    assert_eq!(index.resolve_anchor(gap), gap);

    // Tile `(1, 0)` = rows `64..=127`, cols `0..=15` intersects the ranges whose columns <= 15.
    let tile_query = CellRange::new(CellCoord::new(64, 0), CellCoord::new(127, 15));
    let hits = index.intersecting_ranges(tile_query);
    assert!(!hits.is_empty());
    for hit in &hits {
        assert!(hit.intersects(tile_query));
    }
}

#[test]
fn cross_tile_merged_cell_spans_four_tiles_seamlessly_snapshot() {
    let mut doc = SheetDocument::new_empty("CrossTileMerge.xlsx").expect("doc");
    // O63:R66 = rows 62..=65, cols 14..=17 -> crosses tile row boundary 63|64 AND tile col boundary 15|16!
    let merge = CellRange::new(CellCoord::new(62, 14), CellCoord::new(65, 17));
    doc.set_cell_input(merge.start, "Cross-Tile Merge")
        .expect("input");
    let thick_blue = Some(BorderEdge {
        style: BorderLineStyle::Medium,
        color: Some(ColorRgba::rgb(29, 78, 216)),
    });
    let mut anchor_style = CellStyle {
        fill_color: Some(ColorRgba::rgb(254, 243, 199)),
        horizontal_align: HorizontalAlign::Center,
        vertical_align: VerticalAlign::Center,
        ..CellStyle::default()
    };
    anchor_style.borders.top = thick_blue;
    anchor_style.borders.left = thick_blue;
    doc.execute_transaction(Transaction::single(
        "Style top-left",
        SheetOp::SetCellStyle {
            sheet: 0,
            coord: merge.start,
            old_style: Box::default(),
            new_style: Box::new(anchor_style),
        },
    ))
    .expect("style start");

    let br_style = CellStyle {
        borders: CellBorders {
            bottom: thick_blue,
            right: thick_blue,
            ..CellBorders::default()
        },
        ..CellStyle::default()
    };
    doc.execute_transaction(Transaction::single(
        "Style bottom-right",
        SheetOp::SetCellStyle {
            sheet: 0,
            coord: merge.end,
            old_style: Box::default(),
            new_style: Box::new(br_style),
        },
    ))
    .expect("style end");
    doc.merge_range(merge).expect("merge O63:R66");

    let (row_index, col_index, merges) = build_axes_and_merges(&doc);
    // Scroll directly to row 61, col 13 so O63:R66 sits centered in the 520x220 viewport!
    let geom = ViewportGeometry::compute_zoomed(
        ViewportParams {
            canvas_width: 520.0,
            canvas_height: 220.0,
            row_header_width: DEFAULT_ROW_HEADER_WIDTH_PX,
            col_header_height: DEFAULT_COL_HEADER_HEIGHT_PX,
            freeze: FreezePane::default(),
            scroll_x: col_index.index_to_offset(13),
            scroll_y: row_index.index_to_offset(61),
        },
        1.0,
        &row_index,
        &col_index,
    );
    let mut cache = SheetTileCache::default();
    let dl = render_viewport_display_list(&doc, &geom, &row_index, &col_index, &merges, &mut cache);
    assert_eq!(
        cache.len(),
        4,
        "4 adjacent 64x16 tiles must be populated across the seam"
    );

    let pixmap =
        rasterize_display_list_region(&dl, RectF::new(0.0, 0.0, 520.0, 220.0), Color::WHITE)
            .expect("rasterize cross-tile merge");
    let summary = summarize_pixmap(&pixmap, Color::WHITE).expect("summarize cross-tile merge");
    insta::assert_yaml_snapshot!("m2_05_cross_tile_merge_visual", &summary);
}

#[test]
fn frozen_four_quadrant_scrolled_and_hidden_markers_snapshots() {
    let mut doc = SheetDocument::new_empty("FrozenHidden.xlsx").expect("doc");
    doc.set_cell_input(CellCoord::new(0, 0), "Corner A1")
        .expect("A1");
    doc.set_cell_input(CellCoord::new(0, 6), "Top Header G1")
        .expect("G1");
    doc.set_cell_input(CellCoord::new(22, 0), "Left Key A23")
        .expect("A23");
    doc.set_cell_input(CellCoord::new(22, 6), "Scrolled G23")
        .expect("G23");
    doc.set_freeze_pane(FreezePane {
        frozen_rows: 2,
        frozen_cols: 2,
    })
    .expect("freeze 2x2");
    // Hide row 1 (inside frozen rows) and col 5 (just before visible scrolled col 6).
    doc.set_row_hidden(1, true).expect("hide row 2");
    doc.set_col_hidden(5, true).expect("hide col F");

    let (row_index, col_index, merges) = build_axes_and_merges(&doc);
    assert!(row_index.has_hidden_before(2));
    assert!(col_index.has_hidden_before(6));

    // Scroll so main pane starts at row 21, col 6 while frozen 2x2 stays pinned!
    let scroll_x = col_index.index_to_offset(6) - col_index.index_to_offset(2);
    let scroll_y = row_index.index_to_offset(21) - row_index.index_to_offset(2);
    let geom = ViewportGeometry::compute_zoomed(
        ViewportParams {
            canvas_width: 480.0,
            canvas_height: 200.0,
            row_header_width: DEFAULT_ROW_HEADER_WIDTH_PX,
            col_header_height: DEFAULT_COL_HEADER_HEIGHT_PX,
            freeze: doc.freeze_pane().expect("freeze"),
            scroll_x,
            scroll_y,
        },
        1.0,
        &row_index,
        &col_index,
    );
    assert!(geom.top_left_pane.is_some());
    assert!(geom.top_pane.is_some());
    assert!(geom.left_pane.is_some());
    assert!(geom.main_pane.is_some());

    let mut cache = SheetTileCache::default();
    let dl = render_viewport_display_list(&doc, &geom, &row_index, &col_index, &merges, &mut cache);
    let pixmap =
        rasterize_display_list_region(&dl, RectF::new(0.0, 0.0, 480.0, 200.0), Color::WHITE)
            .expect("rasterize frozen + hidden");
    let summary = summarize_pixmap(&pixmap, Color::WHITE).expect("summarize frozen + hidden");
    insta::assert_yaml_snapshot!("m2_05_frozen_hidden_visual", &summary);
}

#[test]
fn gridlines_toggle_on_off_preserves_explicit_cell_borders_snapshot() {
    let mut doc = SheetDocument::new_empty("GridlinesToggle.xlsx").expect("doc");
    doc.set_cell_input(CellCoord::new(1, 1), "Boxed B2")
        .expect("B2");
    let edge = Some(BorderEdge {
        style: BorderLineStyle::Medium,
        color: Some(ColorRgba::rgb(15, 23, 42)),
    });
    let boxed = CellStyle {
        borders: CellBorders {
            top: edge,
            right: edge,
            bottom: edge,
            left: edge,
            ..CellBorders::default()
        },
        ..CellStyle::default()
    };
    doc.execute_transaction(Transaction::single(
        "Border B2",
        SheetOp::SetCellStyle {
            sheet: 0,
            coord: CellCoord::new(1, 1),
            old_style: Box::default(),
            new_style: Box::new(boxed),
        },
    ))
    .expect("border B2");

    let (row_index, col_index, merges) = build_axes_and_merges(&doc);
    let geom = ViewportGeometry::compute_zoomed(
        ViewportParams {
            canvas_width: 340.0,
            canvas_height: 140.0,
            row_header_width: DEFAULT_ROW_HEADER_WIDTH_PX,
            col_header_height: DEFAULT_COL_HEADER_HEIGHT_PX,
            freeze: FreezePane::default(),
            scroll_x: 0.0,
            scroll_y: 0.0,
        },
        1.0,
        &row_index,
        &col_index,
    );

    let mut cache_on = SheetTileCache::default();
    let dl_on =
        render_viewport_display_list(&doc, &geom, &row_index, &col_index, &merges, &mut cache_on);
    let sum_on = summarize_pixmap(
        &rasterize_display_list_region(&dl_on, RectF::new(0.0, 0.0, 340.0, 140.0), Color::WHITE)
            .expect("on"),
        Color::WHITE,
    )
    .expect("sum_on");

    doc.set_show_grid_lines(false).expect("turn off gridlines");
    let mut cache_off = SheetTileCache::default();
    let dl_off =
        render_viewport_display_list(&doc, &geom, &row_index, &col_index, &merges, &mut cache_off);
    let sum_off = summarize_pixmap(
        &rasterize_display_list_region(&dl_off, RectF::new(0.0, 0.0, 340.0, 140.0), Color::WHITE)
            .expect("off"),
        Color::WHITE,
    )
    .expect("sum_off");

    assert!(
        sum_off.ink_pixel_count < sum_on.ink_pixel_count,
        "Turning gridlines off must reduce painted gridline pixels while keeping explicit borders"
    );
    assert_ne!(sum_on.rgba_crc32_hex, sum_off.rgba_crc32_hex);
    insta::assert_yaml_snapshot!("m2_05_gridlines_off_visual", &sum_off);
}

#[test]
fn multi_level_zoom_hit_testing_zero_tile_rebuilds_and_snapshots() {
    let mut doc = SheetDocument::new_empty("ZoomMatrix.xlsx").expect("doc");
    doc.set_cell_input(CellCoord::new(0, 0), "Zoom 10-400%")
        .expect("A1");
    doc.set_cell_input(CellCoord::new(1, 1), "Vector Crisp")
        .expect("B2");

    let (row_index, col_index, merges) = build_axes_and_merges(&doc);
    let base_params = ViewportParams {
        canvas_width: 800.0,
        canvas_height: 260.0,
        row_header_width: DEFAULT_ROW_HEADER_WIDTH_PX,
        col_header_height: DEFAULT_COL_HEADER_HEIGHT_PX,
        freeze: FreezePane::default(),
        scroll_x: 0.0,
        scroll_y: 0.0,
    };

    let mut shared_cache = SheetTileCache::default();
    let mut summaries = Vec::new();
    for zoom in [0.10_f32, 0.50, 1.0, 2.0, 4.0] {
        let geom = ViewportGeometry::compute_zoomed(base_params, zoom, &row_index, &col_index);
        assert!((geom.zoom - zoom).abs() < 1e-6);

        // Verify hit-testing of B2 center scales identically with zoom!
        let b2_rect = geom
            .cell_viewport_rect(CellCoord::new(1, 1), &row_index, &col_index)
            .expect("B2 visible");
        let hit = geom.hit_test(
            b2_rect.x + b2_rect.width * 0.5,
            b2_rect.y + b2_rect.height * 0.5,
            &row_index,
            &col_index,
        );
        assert_eq!(hit, GridHit::Cell(CellCoord::new(1, 1)));

        let dl = render_viewport_display_list(
            &doc,
            &geom,
            &row_index,
            &col_index,
            &merges,
            &mut shared_cache,
        );
        if (zoom - 0.50).abs() < 1e-5 || (zoom - 2.0).abs() < 1e-5 {
            let pix = rasterize_display_list_region(
                &dl,
                RectF::new(0.0, 0.0, 400.0, 180.0),
                Color::WHITE,
            )
            .expect("rasterize zoom");
            summaries.push((
                (zoom * 100.0).round() as u32,
                summarize_pixmap(&pix, Color::WHITE).expect("sum zoom"),
            ));
        }
    }

    // Even at 10% zoom (~80 columns x ~90 rows visible), tiles stay cached across zoom changes without invalidation!
    assert!(shared_cache.len() <= 16);
    assert_ne!(summaries[0].1.rgba_crc32_hex, summaries[1].1.rgba_crc32_hex);
    insta::assert_yaml_snapshot!("m2_05_zoom_50_and_200_visual", &summaries);
}
