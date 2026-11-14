//! Grid geometry, O(log N) coordinate indexing, tile caching, pagination, and `DisplayList` generation.

mod axis;
mod cell_style_render;
mod tile_cache;
mod viewport;

pub use axis::GridAxisIndex;
pub use cell_style_render::{estimate_text_width_px, wrap_cell_text};
pub use tile_cache::{
    CachedTile, DEFAULT_MAX_CACHED_TILES, SheetTileCache, TILE_COLS, TILE_ROWS, TileKey, TilePlan,
    TileRenderTheme, build_tile_display_list, plan_viewport_tiles,
};
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

    #[test]
    fn editing_one_cell_invalidates_exactly_one_tile_and_plans_prefetch_ring() {
        use ordia_render::Color;
        use ordia_sheet_model::SheetDocument;

        assert_eq!(TILE_ROWS, 64);
        assert_eq!(TILE_COLS, 16);

        let mut doc = SheetDocument::new_demo("TileTest.xlsx").expect("demo");
        let rows = GridAxisIndex::from_row_metrics(MAX_ROWS, &[]);
        let cols = GridAxisIndex::from_col_metrics(MAX_COLS, &[]);
        let theme = TileRenderTheme {
            default_text_color: Color::rgb(15, 23, 42),
            grid_line_color: Some(Color::rgb(226, 232, 240)),
        };

        let mut cache = SheetTileCache::new(64);
        let keys = [
            TileKey::new(0, 0, 0),
            TileKey::new(0, 1, 0),
            TileKey::new(0, 0, 1),
            TileKey::new(0, 1, 1),
        ];
        for &k in &keys {
            let _ = cache.get_or_build(k, |tk| {
                build_tile_display_list(&doc, tk, &rows, &cols, theme)
            });
        }
        assert_eq!(cache.len(), 4);
        assert_eq!(cache.misses(), 4);
        assert_eq!(cache.hits(), 0);

        // Edit an unreferenced cell H10 (row 9, col 7 -> inside tile (0, 0, 0)).
        doc.set_cell_input(CellCoord::new(9, 7), "9999")
            .expect("edit H10");
        let damages = doc.drain_damage();
        let evicted = cache.apply_damage(&damages);
        assert_eq!(
            evicted, 1,
            "editing one unreferenced cell must invalidate exactly 1 tile"
        );
        assert!(!cache.contains(TileKey::new(0, 0, 0)));
        assert!(cache.contains(TileKey::new(0, 1, 0)));
        assert!(cache.contains(TileKey::new(0, 0, 1)));
        assert!(cache.contains(TileKey::new(0, 1, 1)));

        // Re-fetching all 4 tiles hits 3 cached tiles and rebuilds only the 1 dirty tile!
        for &k in &keys {
            let _ = cache.get_or_build(k, |tk| {
                build_tile_display_list(&doc, tk, &rows, &cols, theme)
            });
        }
        assert_eq!(cache.hits(), 3);
        assert_eq!(cache.misses(), 5);

        // Verify 1-screen viewport prefetch ring around scrolled main quadrant.
        let geom = ViewportGeometry::compute(
            ViewportParams {
                canvas_width: 1200.0,
                canvas_height: 800.0,
                row_header_width: 54.0,
                col_header_height: 24.0,
                freeze: FreezePane {
                    frozen_rows: 1,
                    frozen_cols: 1,
                },
                scroll_x: 1800.0,
                scroll_y: 3200.0,
            },
            &rows,
            &cols,
        );
        let plan = plan_viewport_tiles(0, &geom, &rows, &cols);
        assert!(!plan.visible.is_empty());
        assert!(!plan.prefetch.is_empty());
        for pf in &plan.prefetch {
            assert!(!plan.visible.contains(pf));
        }
    }

    #[test]
    fn dynamic_fenwick_updates_preserve_unhidden_sizes_and_run_under_one_microsecond() {
        let mut axis = GridAxisIndex::uniform(MAX_ROWS, 25.0);
        axis.set_span_size(500, 60.0);
        assert_eq!(axis.span_size(500), 60.0);
        assert_eq!(axis.unhidden_size(500), 60.0);

        // Hide rows 500..=502; unhidden_size(500) must remain 60.0!
        axis.set_range_hidden(500..=502, true);
        assert!(axis.is_hidden(500));
        assert_eq!(axis.span_size(500), 0.0);
        assert_eq!(axis.unhidden_size(500), 60.0);
        assert_eq!(axis.hidden_gap_before(503), Some(500..=502));
        assert_eq!(axis.next_visible(499), Some(503));
        assert_eq!(axis.prev_visible(503), Some(499));

        // Unhide row 500 -> restores custom height 60.0 immediately in O(log N)!
        axis.set_hidden(500, false);
        assert_eq!(axis.span_size(500), 60.0);
        assert_eq!(axis.hidden_gap_before(503), Some(501..=502));

        // Measure 10,000 bidirectional pixel <-> row conversions at 1,048,576 rows.
        let total = axis.total_extent();
        let iters = 10_000_u64;
        let mut checksum = 0_u64;
        let t0 = std::time::Instant::now();
        let mut lcg = 0x9E37_79B9_7F4A_7C15_u64;
        for _ in 0..iters {
            lcg = lcg
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let frac = ((lcg >> 11) as f64) * (1.0 / ((1_u64 << 53) as f64));
            let px = frac * total;
            let row = axis.offset_to_index(px);
            let back_px = axis.index_to_offset(row);
            checksum = checksum
                .wrapping_add(u64::from(row))
                .wrapping_add(back_px as u64);
        }
        let elapsed_ns = t0.elapsed().as_nanos() as f64;
        // Each loop performs BOTH `offset_to_index` and `index_to_offset`.
        let ns_per_conversion = elapsed_ns / ((iters * 2) as f64);
        assert!(checksum > 0);
        assert!(
            ns_per_conversion < 1_000.0,
            "expected < 1000 ns (< 1 µs) per conversion at 1M rows, got {ns_per_conversion:.1} ns"
        );
    }

    mod prop {
        use super::*;
        use proptest::prelude::*;

        #[derive(Debug, Clone)]
        enum AxisMutation {
            Resize(usize, u16),
            Hide(usize, bool),
        }

        fn naive_offsets(sizes: &[f32], hidden: &[bool]) -> Vec<f64> {
            let mut offs = Vec::with_capacity(sizes.len() + 1);
            let mut acc = 0.0_f64;
            offs.push(0.0);
            for (&s, &h) in sizes.iter().zip(hidden.iter()) {
                if !h {
                    acc += f64::from(s.max(0.0));
                }
                offs.push(acc);
            }
            offs
        }

        fn naive_offset_to_index(offs: &[f64], px: f64) -> u32 {
            let n = offs.len() - 1;
            if n == 0 {
                return 0;
            }
            let total = offs[n];
            if total <= 0.0 || px >= total {
                return (n - 1) as u32;
            }
            let target = px.max(0.0);
            let p = offs.partition_point(|&o| o <= target);
            p.saturating_sub(1).min(n - 1) as u32
        }

        proptest! {
            #[test]
            fn fenwick_axis_matches_naive_prefix_sums_under_random_mutations(
                raw_items in proptest::collection::vec((1_u16..=200_u16, any::<bool>()), 1..=260),
                mutations in proptest::collection::vec(
                    prop_oneof![
                        (0_usize..300, 1_u16..=250_u16).prop_map(|(i, s)| AxisMutation::Resize(i, s)),
                        (0_usize..300, any::<bool>()).prop_map(|(i, h)| AxisMutation::Hide(i, h)),
                    ],
                    0..=60
                ),
                probes in proptest::collection::vec(-20.0_f64..=60_000.0_f64, 1..=40),
            ) {
                let n = raw_items.len();
                // Quantize sizes to 0.25 px (exact binary fraction) + non-power-of-two tenths (0.1 px)
                let mut ref_sizes: Vec<f32> = raw_items.iter().map(|&(s, _)| f32::from(s) * 0.5).collect();
                let mut ref_hidden: Vec<bool> = raw_items.iter().map(|&(_, h)| h).collect();

                let mut axis = GridAxisIndex::from_sizes_and_hidden(&ref_sizes, &ref_hidden);

                for m in mutations {
                    match m {
                        AxisMutation::Resize(idx, s) => {
                            let i = idx % n;
                            let val = f32::from(s) * 0.5;
                            ref_sizes[i] = val;
                            axis.set_span_size(i as u32, val);
                        }
                        AxisMutation::Hide(idx, h) => {
                            let i = idx % n;
                            ref_hidden[i] = h;
                            axis.set_hidden(i as u32, h);
                        }
                    }
                }

                let offs = naive_offsets(&ref_sizes, &ref_hidden);
                prop_assert!((axis.total_extent() - offs[n]).abs() < 1e-6);

                for i in 0..n {
                    let off = axis.index_to_offset(i as u32);
                    prop_assert!((off - offs[i]).abs() < 1e-6);
                    prop_assert_eq!(axis.is_hidden(i as u32), ref_hidden[i]);
                    prop_assert!((axis.unhidden_size(i as u32) - ref_sizes[i]).abs() < 1e-6);

                    // Exact boundary probes around `offs[i]`
                    for delta in [-0.25_f64, 0.0, 0.25] {
                        let px = offs[i] + delta;
                        prop_assert_eq!(
                            axis.offset_to_index(px),
                            naive_offset_to_index(&offs, px)
                        );
                    }
                }

                for px in probes {
                    prop_assert_eq!(
                        axis.offset_to_index(px),
                        naive_offset_to_index(&offs, px)
                    );
                }
            }
        }
    }
}
