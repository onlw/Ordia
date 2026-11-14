//! Headless UI integration tests for `SheetWorkspaceView`:
//! cell selection, inline formula editing, Chinese IME composition/bounds tracking, and 1M-row frozen scrolling.

use gpui_kit::test::{ClickOptions, TestInput, TestWindowExt as _};
use gpui_kit::{
    AppContext as _, Bounds, ElementInputHandler, EntityInputHandler as _, ScrollDelta,
    TestAppContext, WindowBounds, WindowOptions, point, px, size,
};
use ordia_sheet_layout::{DEFAULT_COL_HEADER_HEIGHT_PX, DEFAULT_ROW_HEADER_WIDTH_PX};
use ordia_sheet_model::{CellCoord, CellValue, FreezePane, SheetDocument, SheetOp, Transaction};
use ordia_sheet_view::{SheetWorkspaceView, register_sheet_keybindings};

/// Computes the center offset (relative to `sheet-grid-surface` top-left) of `(row, col)`
/// under default 90x25 cell metrics with zero scroll.
fn unscrolled_cell_offset(row: u32, col: u32) -> gpui_kit::Point<gpui_kit::Pixels> {
    let x = DEFAULT_ROW_HEADER_WIDTH_PX + (col as f32) * 90.0 + 45.0;
    let y = DEFAULT_COL_HEADER_HEIGHT_PX + (row as f32) * 25.0 + 12.5;
    point(px(x), px(y))
}

#[gpui_kit::test]
fn inline_formula_editing_escape_cancel_and_undo_redo(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        register_sheet_keybindings(cx);
    });

    let (handle, view) = cx.update(|cx| {
        let doc =
            SheetDocument::new_empty("FormulaTest.xlsx").expect("create empty sheet document");
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1024.), px(680.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| SheetWorkspaceView::new(doc, window, cx)),
        )
        .expect("open headless sheet window")
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("name-box").label(), Some("A1"));
        assert!(window.try_find("cell-inline-editor").is_none());

        // 1. Double-click A1 -> inline editor opens -> enter 12.5 -> Enter commits & moves to A2.
        window.click_with_options(
            "sheet-grid-surface",
            ClickOptions::new()
                .with_offset(unscrolled_cell_offset(0, 0))
                .with_count(2),
            cx,
        );
        assert!(window.find("cell-inline-editor").visible());
        window.click("cell-inline-editor", cx);
        window.input("12.5", cx);
        window.press("enter", cx);
        assert!(window.try_find("cell-inline-editor").is_none());
        assert_eq!(window.find("name-box").label(), Some("A2"));

        // 2. Double-click B1 -> enter 29.5 -> Enter commits & moves to B2.
        window.click_with_options(
            "sheet-grid-surface",
            ClickOptions::new()
                .with_offset(unscrolled_cell_offset(0, 1))
                .with_count(2),
            cx,
        );
        window.click("cell-inline-editor", cx);
        window.input("29.5", cx);
        window.press("enter", cx);
        assert_eq!(window.find("name-box").label(), Some("B2"));

        // 3. Double-click C1 -> enter formula =A1+B1 -> Enter evaluates to 42!
        window.click_with_options(
            "sheet-grid-surface",
            ClickOptions::new()
                .with_offset(unscrolled_cell_offset(0, 2))
                .with_count(2),
            cx,
        );
        window.click("cell-inline-editor", cx);
        window.input("=A1+B1", cx);
        window.press("enter", cx);
        assert_eq!(window.find("name-box").label(), Some("C2"));

        // 4. Negative case: double-click C1, type garbage, press Escape -> preserves =A1+B1 (42)!
        window.click_with_options(
            "sheet-grid-surface",
            ClickOptions::new()
                .with_offset(unscrolled_cell_offset(0, 2))
                .with_count(2),
            cx,
        );
        window.click("cell-inline-editor", cx);
        window.input("BROKEN_TEXT", cx);
        window.press("escape", cx);
        assert!(window.try_find("cell-inline-editor").is_none());
    })
    .expect("exercise inline formula editing");

    cx.update(|cx| {
        let state = view.read(cx);
        let c1 = CellCoord::new(0, 2);
        assert_eq!(
            state.document().cell_value(c1).expect("read C1 value"),
            CellValue::Number(42.0)
        );
        assert_eq!(
            state.document().cell_input(c1).expect("read C1 formula"),
            "=A1+B1"
        );
    });

    // 5. Exercise Undo / Redo shortcuts on the focused grid.
    cx.update_window(handle, |_, window, cx| {
        window.click_at("sheet-grid-surface", unscrolled_cell_offset(0, 2), cx);
        window.press("secondary-z", cx);
    })
    .expect("undo C1 formula");

    cx.update(|cx| {
        let state = view.read(cx);
        assert_eq!(
            state
                .document()
                .cell_value(CellCoord::new(0, 2))
                .expect("C1 after undo"),
            CellValue::Empty
        );
    });

    cx.update_window(handle, |_, window, cx| {
        window.press("secondary-shift-z", cx);
    })
    .expect("redo C1 formula");

    cx.update(|cx| {
        let state = view.read(cx);
        assert_eq!(
            state
                .document()
                .cell_value(CellCoord::new(0, 2))
                .expect("C1 after redo"),
            CellValue::Number(42.0)
        );
    });
}

#[gpui_kit::test]
fn chinese_ime_composition_tracks_cell_bounds_and_commits_cleanly(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        register_sheet_keybindings(cx);
    });

    let (handle, view) = cx.update(|cx| {
        let doc = SheetDocument::new_empty("ImeTest.xlsx").expect("create IME sheet document");
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(960.), px(640.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| SheetWorkspaceView::new(doc, window, cx)),
        )
        .expect("open headless IME window")
    });

    let target_cell = CellCoord::new(2, 1); // B3

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);

        // Select cell B3 with a single click (standing on the grid, NOT yet in edit mode).
        window.click_at("sheet-grid-surface", unscrolled_cell_offset(2, 1), cx);
        assert_eq!(window.find("name-box").label(), Some("B3"));
        assert!(window.try_find("cell-inline-editor").is_none());

        let grid_bounds = window.find("sheet-grid-surface").bounds();
        let expected_cell_bounds = Bounds::new(
            point(
                grid_bounds.origin.x + px(DEFAULT_ROW_HEADER_WIDTH_PX + 90.0),
                grid_bounds.origin.y + px(DEFAULT_COL_HEADER_HEIGHT_PX + 50.0),
            ),
            size(px(90.0), px(25.0)),
        );

        // Bridge OS IME directly onto SheetWorkspaceView (simulating first Pinyin keystroke on B3).
        let handler = ElementInputHandler::new(expected_cell_bounds, view.clone());
        let mut ime = TestInput::new(handler, window);

        // Start Pinyin preedit composition: "ni hao"
        ime.compose_text(None, "ni hao", Some(0..6), cx);
        assert_eq!(ime.marked_text_range(cx), Some(0..6));

        // Inline editor must now be active and synchronized with formula bar during preedit.
        assert!(window.find("cell-inline-editor").visible());
        assert_eq!(window.find("cell-inline-editor").value(), Some("ni hao"));
        assert_eq!(window.find("formula-bar-input").value(), Some("ni hao"));

        // Verify IME candidate popup anchor bounds sit at cell B3's window coordinates.
        let candidate_bounds = view.update(cx, |state, cx| {
            state.bounds_for_range(0..6, expected_cell_bounds, window, cx)
        });
        let resolved = candidate_bounds.expect("IME candidate bounds must resolve");
        assert!(resolved.origin.x >= expected_cell_bounds.origin.x - px(4.0));
        assert!(resolved.origin.y >= expected_cell_bounds.origin.y - px(4.0));
        assert!(
            resolved.origin.y
                <= expected_cell_bounds.origin.y + expected_cell_bounds.size.height + px(4.0)
        );

        // Commit composed Chinese candidate "你好，世界" and press Enter to finalize into model.
        let handler2 = ElementInputHandler::new(expected_cell_bounds, view.clone());
        let mut ime2 = TestInput::new(handler2, window);
        ime2.commit_text("你好，世界", cx);
        assert_eq!(ime2.marked_text_range(cx), None);
        assert_eq!(
            window.find("cell-inline-editor").value(),
            Some("你好，世界")
        );

        window.press("enter", cx);
        assert!(window.try_find("cell-inline-editor").is_none());
        assert_eq!(window.find("name-box").label(), Some("B4"));
    })
    .expect("drive Chinese IME composition");

    cx.update(|cx| {
        let state = view.read(cx);
        assert_eq!(
            state
                .document()
                .cell_value(target_cell)
                .expect("read B3 value"),
            CellValue::Text("你好，世界".to_string())
        );
    });
}

#[gpui_kit::test]
fn million_row_virtualized_scrolling_keeps_frozen_panes_pinned(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        register_sheet_keybindings(cx);
    });

    let (handle, view) = cx.update(|cx| {
        let mut doc =
            SheetDocument::new_demo("MillionRowFreeze.xlsx").expect("create demo workbook");
        let old_freeze = doc.freeze_pane().unwrap_or_default();
        doc.execute_transaction(Transaction::single(
            "Freeze row 1 and column A",
            SheetOp::SetFreezePane {
                sheet: 0,
                old_freeze,
                new_freeze: FreezePane {
                    frozen_rows: 1,
                    frozen_cols: 1,
                },
            },
        ))
        .expect("set freeze pane");
        // Populate row 1,000,000 (0-based row 999,999, col 1 -> B1000000).
        doc.set_cell_input(CellCoord::new(999_999, 1), "Row 1M Sentinel")
            .expect("populate B1000000");

        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1024.), px(700.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| SheetWorkspaceView::new(doc, window, cx)),
        )
        .expect("open 1M-row window")
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);

        // Enable performance overlay button and verify HUD mounts.
        window.click("toggle-perf-overlay", cx);
        assert!(window.find("perf-overlay").visible());

        // Exercise mouse-wheel scrolling on the grid surface.
        window.scroll(
            "sheet-grid-surface",
            ScrollDelta::Pixels(point(px(0.0), px(-500.0))),
            cx,
        );
    })
    .expect("initial scroll wheel step");

    // Jump deep to row 999,995 (near row 1,000,000) and render frames.
    cx.update(|cx| {
        view.update(cx, |state, cx| {
            state.set_scroll_offset(0.0, 999_990.0 * 25.0, cx);
        });
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let hud_label = window
            .find("perf-overlay")
            .label()
            .expect("perf overlay label")
            .to_string();
        assert!(
            hud_label.contains("/1048576"),
            "HUD must reflect 1,048,576 total rows, got: {hud_label}"
        );

        // Even while scrolled to row ~1,000,000, clicking frozen top-left cell (0, 0) hits A1!
        window.click_at("sheet-grid-surface", unscrolled_cell_offset(0, 0), cx);
        assert_eq!(window.find("name-box").label(), Some("A1"));
    })
    .expect("verify frozen corner hit-testing at row 1M");

    cx.update(|cx| {
        let state = view.read(cx);
        assert!(state.scroll_y() > 24_900_000.0);
        assert!(state.frame_stats().total_frames >= 3);
    });
}

#[gpui_kit::test]
fn copy_and_paste_shortcut_preserves_values_bold_and_background_color(cx: &mut TestAppContext) {
    let (handle, entity) = cx.update(|cx| {
        gpui_kit::init(cx);
        register_sheet_keybindings(cx);
        let doc = SheetDocument::new_demo("ClipboardUi.xlsx").expect("demo doc");
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.0), px(0.0)),
                    size(px(960.0), px(640.0)),
                ))),
                ..WindowOptions::default()
            },
            cx,
            |window, cx| cx.new(|cx| SheetWorkspaceView::new(doc, window, cx)),
        )
        .expect("open headless clipboard window")
    });

    #[cfg(target_os = "macos")]
    let (copy_key, paste_key) = ("cmd-c", "cmd-v");
    #[cfg(not(target_os = "macos"))]
    let (copy_key, paste_key) = ("ctrl-c", "ctrl-v");

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        entity.update(cx, |state, cx| {
            state.select_cell(CellCoord::new(0, 0), window, cx);
            state.set_selection_range(CellCoord::new(0, 0), CellCoord::new(1, 1), cx);
        });
        window.render_frame(cx);
        window.press(copy_key, cx);

        entity.update(cx, |state, cx| {
            state.select_cell(CellCoord::new(8, 0), window, cx);
        });
        window.render_frame(cx);
        window.press(paste_key, cx);
    })
    .expect("execute copy and paste shortcuts");

    cx.update(|cx| {
        let state = entity.read(cx);
        let doc = state.document();
        assert_eq!(
            doc.cell_display_text(CellCoord::new(8, 0)).expect("A9"),
            "项目 / Item"
        );
        assert_eq!(
            doc.cell_display_text(CellCoord::new(9, 1)).expect("B10"),
            "1280"
        );
        let style = doc.cell_style(CellCoord::new(8, 0)).expect("A9 style");
        assert!(style.font.bold);
        assert_eq!(
            style.fill_color,
            Some(ordia_sheet_model::ColorRgba::rgb(241, 245, 249))
        );
    });
}

#[gpui_kit::test]
fn tile_cache_single_cell_invalidation_and_scroll_p99_benchmark(cx: &mut TestAppContext) {
    let (handle, view) = cx.update(|cx| {
        gpui_kit::init(cx);
        register_sheet_keybindings(cx);
        let mut doc = SheetDocument::new_demo("TileCacheScroll.xlsx").expect("demo");
        // Seed cells across multiple 64x16 tiles so scrolling visits populated cached tiles.
        for r in (0..640_u32).step_by(8) {
            doc.set_cell_input(CellCoord::new(r, 2), &format!("Item-{r}"))
                .expect("seed cell");
            doc.set_cell_input(CellCoord::new(r, 18), &format!("{}", r * 10))
                .expect("seed right-tile cell");
        }
        let _ = doc.drain_damage();
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.0), px(0.0)),
                    size(px(1200.0), px(760.0)),
                ))),
                ..WindowOptions::default()
            },
            cx,
            |window, cx| cx.new(|cx| SheetWorkspaceView::new(doc, window, cx)),
        )
        .expect("open window")
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let (cached_before, _hits0, _misses0, inv0) = view.read(cx).tile_cache_stats();
        assert!(
            cached_before >= 4,
            "expected initial visible + prefetch tiles"
        );

        // Changing selection must invalidate ZERO tiles (`PLAN.md` §4.3 separate selection layer).
        view.update(cx, |state, cx| {
            state.set_selection_range(CellCoord::new(0, 0), CellCoord::new(20, 8), cx);
        });
        window.render_frame(cx);
        let (_len1, hits1, _misses1, inv1) = view.read(cx).tile_cache_stats();
        assert_eq!(
            inv1, inv0,
            "selection changes must not invalidate content tiles"
        );
        assert!(hits1 > 0);

        // Editing one unreferenced cell `D10` (`(9, 3)`) must invalidate ONLY 1 tile (`(0, 0, 0)`).
        view.update(cx, |state, cx| {
            state.begin_edit_existing(CellCoord::new(9, 3), window, cx);
        });
        window.render_frame(cx);
        window.click("cell-inline-editor", cx);
        window.input("EditedOnce", cx);
        view.update(cx, |state, cx| {
            state.commit_edit(true, window, cx);
        });
        window.render_frame(cx);
        let (_len2, _hits2, _misses2, inv2) = view.read(cx).tile_cache_stats();
        assert_eq!(
            inv2 - inv1,
            1,
            "editing one cell must invalidate exactly 1 tile"
        );

        // Continuous 120-frame scroll benchmark recording P50 / P99 frame times.
        view.update(cx, |state, _cx| state.reset_frame_stats());
        for _ in 0..120 {
            view.update(cx, |state, cx| {
                state.scroll_by(12.0, 96.0, cx);
            });
            window.render_frame(cx);
        }
        let stats = view.read(cx).frame_stats();
        assert_eq!(stats.total_frames, 120);
        assert!(
            stats.p99_ms < 25.0,
            "expected smooth P99 scroll frame time < 25ms in debug headless test, got {:.2}ms",
            stats.p99_ms
        );
    })
    .expect("run tile cache invalidation and scroll P99 benchmark");
}

#[gpui_kit::test]
fn merged_selection_hidden_rows_gridlines_toggle_and_lossless_zoom(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        register_sheet_keybindings(cx);
    });

    let (handle, view) = cx.update(|cx| {
        let mut doc = SheetDocument::new_empty("M205Features.xlsx").expect("doc");
        let merged = ordia_sheet_model::CellRange::new(CellCoord::new(1, 1), CellCoord::new(2, 3));
        doc.set_cell_input(merged.start, "Merged B2:D3")
            .expect("B2");
        doc.merge_range(merged).expect("merge B2:D3");
        doc.set_row_hidden(4, true).expect("hide row 5");
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1080.), px(720.)),
                ))),
                ..WindowOptions::default()
            },
            cx,
            |window, cx| cx.new(|cx| SheetWorkspaceView::new(doc, window, cx)),
        )
        .expect("open window")
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);

        // 1. Clicking covered cell C3 (`(2, 2)`) selects full merged range B2:D3 with anchor B2!
        window.click_at("sheet-grid-surface", unscrolled_cell_offset(2, 2), cx);
        assert_eq!(window.find("name-box").label(), Some("B2:D3"));
        assert_eq!(view.read(cx).document().active_cell(), CellCoord::new(1, 1));

        // 2. Row 5 (`row == 4`) is hidden (`0px`): clicking where row 5 used to sit hits row 6 (`B6`)!
        window.click_at("sheet-grid-surface", unscrolled_cell_offset(4, 1), cx);
        assert_eq!(window.find("name-box").label(), Some("B6"));

        // 3. Gridlines toggle button + Undo/Redo reversibility.
        assert!(view.read(cx).is_grid_lines_visible());
        window.click("toggle-gridlines", cx);
        assert!(!view.read(cx).is_grid_lines_visible());
        window.click_at("sheet-grid-surface", unscrolled_cell_offset(0, 0), cx);
        window.press("secondary-z", cx);
        assert!(view.read(cx).is_grid_lines_visible());

        // 4. Lossless 10%–400% zoom via buttons, shortcuts, clamping, and zero tile invalidations.
        window.render_frame(cx);
        let (_cached0, _hits0, _misses0, inv0) = view.read(cx).tile_cache_stats();
        assert_eq!(window.find("zoom-reset-btn").label(), Some("100%"));

        window.click("zoom-in-btn", cx);
        assert_eq!(view.read(cx).zoom_percent(), 110);
        assert_eq!(window.find("zoom-reset-btn").label(), Some("110%"));

        window.click("zoom-out-btn", cx);
        window.click("zoom-out-btn", cx);
        assert_eq!(view.read(cx).zoom_percent(), 90);

        window.click("zoom-reset-btn", cx);
        assert_eq!(view.read(cx).zoom_percent(), 100);

        // Clamp bounds at 400% and 10%, then test 200% zoomed pointer hit-testing.
        view.update(cx, |state, cx| {
            state.set_zoom_percent(999, window, cx);
        });
        assert_eq!(view.read(cx).zoom_percent(), 400);
        view.update(cx, |state, cx| {
            state.set_zoom_percent(1, window, cx);
        });
        assert_eq!(view.read(cx).zoom_percent(), 10);
        view.update(cx, |state, cx| {
            state.set_zoom_percent(200, window, cx);
        });
        window.render_frame(cx);
        let (_cached1, _hits1, _misses1, inv1) = view.read(cx).tile_cache_stats();
        assert_eq!(
            inv1, inv0,
            "changing lossless zoom must never invalidate unzoomed tile DisplayLists"
        );

        // At 200% zoom (`rhw = 97px`, `chh = 43px`, col width = `180px`, row height = `50px`),
        // `(320, 60)` lands squarely inside `B1` (`277..457 × 43..93`), whereas at 100% zoom `(320, 60)` would hit `C2`!
        window.click_at("sheet-grid-surface", point(px(320.0), px(60.0)), cx);
        assert_eq!(window.find("name-box").label(), Some("B1"));

        // Shortcut `Cmd/Ctrl+0` resets zoom to 100%.
        window.press("secondary-0", cx);
        assert_eq!(view.read(cx).zoom_percent(), 100);
    })
    .expect("verify M2-05 merged cells, hidden markers, gridlines toggle, and lossless zoom");
}

#[gpui_kit::test]
fn multi_worksheet_tabs_independent_scroll_tab_colors_and_hidden_sheets(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        register_sheet_keybindings(cx);
    });

    let (handle, view) = cx.update(|cx| {
        let mut doc = SheetDocument::new_empty("MultiSheetTest.xlsx").expect("doc");
        doc.set_cell_input(CellCoord::new(11, 3), "Sheet1-D12")
            .expect("S0 D12");

        let names = [
            "Q1 Revenue",
            "Q2 Revenue",
            "Q3 Revenue",
            "Q4 Revenue",
            "Forecast",
            "Summary",
            "Audit Log",
            "Raw Ledger",
        ];
        for name in names {
            doc.add_sheet(Some(name)).expect("add sheet");
        }
        // Populate sheet 1 (`Q1 Revenue`) and hidden sheet 7 (`Audit Log`).
        doc.set_active_sheet(1);
        doc.set_cell_input(CellCoord::new(0, 0), "Q1-Head")
            .expect("S1 A1");
        doc.set_cell_input(CellCoord::new(4, 1), "Q1-B5")
            .expect("S1 B5");
        doc.set_sheet_tab_color(1, Some(ordia_sheet_model::ColorRgba::rgb(16, 185, 129)))
            .expect("tab color S1");

        doc.set_active_sheet(7);
        doc.set_cell_input(CellCoord::new(0, 0), "CONFIDENTIAL-AUDIT")
            .expect("S7 A1");
        doc.set_sheet_tab_color(7, Some(ordia_sheet_model::ColorRgba::rgb(239, 68, 68)))
            .expect("tab color S7");

        doc.set_active_sheet(0);
        doc.set_sheet_visibility(7, ordia_sheet_model::SheetVisibility::Hidden)
            .expect("hide S7");
        doc.set_sheet_visibility(8, ordia_sheet_model::SheetVisibility::Hidden)
            .expect("hide S8");

        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1180.), px(720.)),
                ))),
                ..WindowOptions::default()
            },
            cx,
            |window, cx| cx.new(|cx| SheetWorkspaceView::new(doc, window, cx)),
        )
        .expect("open window")
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);

        // 1. Tab color bar renders on Q1 Revenue (`sheet-tab-color-1` == `#10B981`),
        //    and hidden sheets (7, 8) are excluded from the visible tab bar.
        assert_eq!(window.find("sheet-tab-color-1").label(), Some("#10B981"));
        assert!(window.try_find("sheet-tab-7").is_none());
        assert!(window.try_find("sheet-tab-8").is_none());

        // 2. With 7 visible sheets (`0..=6`) and window size 6, `sheet-tab-6` (`Summary`) requires scrolling right!
        assert!(window.try_find("sheet-tab-6").is_none());
        window.click("sheet-tabs-scroll-right", cx);
        assert_eq!(view.read(cx).tab_scroll_offset(), 1);
        assert_eq!(window.find("sheet-tab-6").label(), Some("Summary"));
        window.click("sheet-tabs-scroll-left", cx);
        assert_eq!(view.read(cx).tab_scroll_offset(), 0);

        // 3. Independent per-sheet scroll, selection, and zoom state memory!
        view.update(cx, |state, cx| {
            state.select_cell(CellCoord::new(11, 3), window, cx);
            state.set_scroll_offset(360.0, 1200.0, cx);
            state.set_zoom_percent(150, window, cx);
        });
        window.render_frame(cx);
        assert_eq!(window.find("name-box").label(), Some("D12"));

        // Switch to Q1 Revenue (`sheet-tab-1`): fresh `(0, 0)` scroll, `A1` selection, `100%` zoom.
        window.click("sheet-tab-1", cx);
        assert_eq!(view.read(cx).document().active_sheet(), 1);
        assert_eq!(view.read(cx).scroll_x(), 0.0);
        assert_eq!(view.read(cx).scroll_y(), 0.0);
        assert_eq!(view.read(cx).zoom_percent(), 100);
        assert_eq!(window.find("name-box").label(), Some("A1"));
        assert_eq!(
            view.read(cx).formula_editor().read(cx).value().as_ref(),
            "Q1-Head"
        );

        // Scroll & select on Q1 Revenue (`B5`, `(90.0, 500.0)`), then switch back to Sheet1 (`sheet-tab-0`).
        view.update(cx, |state, cx| {
            state.select_cell(CellCoord::new(4, 1), window, cx);
            state.set_scroll_offset(90.0, 500.0, cx);
        });
        window.render_frame(cx);

        window.click("sheet-tab-0", cx);
        assert_eq!(view.read(cx).document().active_sheet(), 0);
        assert!((view.read(cx).scroll_x() - 360.0).abs() < 1e-4);
        assert!((view.read(cx).scroll_y() - 1200.0).abs() < 1e-4);
        assert_eq!(view.read(cx).zoom_percent(), 150);
        assert_eq!(window.find("name-box").label(), Some("D12"));

        // Switch back to Q1 Revenue (`sheet-tab-1`) and confirm its `(90.0, 500.0)` + `B5` persisted too!
        window.click("sheet-tab-1", cx);
        assert!((view.read(cx).scroll_x() - 90.0).abs() < 1e-4);
        assert!((view.read(cx).scroll_y() - 500.0).abs() < 1e-4);
        assert_eq!(window.find("name-box").label(), Some("B5"));

        // 4. Hidden worksheets badge + quick-view popover jump.
        let badge = window
            .find("hidden-sheets-btn")
            .label()
            .expect("badge label")
            .to_string();
        assert!(badge.contains("(2)"), "unexpected hidden badge: {badge}");
        assert!(window.try_find("hidden-sheets-popover").is_none());

        window.click("hidden-sheets-btn", cx);
        assert!(window.find("hidden-sheets-popover").visible());
        assert_eq!(
            window.find("hidden-sheet-item-7").label(),
            Some("Audit Log")
        );
        assert_eq!(
            window.find("hidden-sheet-item-8").label(),
            Some("Raw Ledger")
        );

        // Clicking `Audit Log` in the quick-view popover switches to sheet 7 and displays its tab color!
        window.click("hidden-sheet-item-7", cx);
        assert!(window.try_find("hidden-sheets-popover").is_none());
        assert_eq!(view.read(cx).document().active_sheet(), 7);
        assert_eq!(
            view.read(cx).formula_editor().read(cx).value().as_ref(),
            "CONFIDENTIAL-AUDIT"
        );
        assert_eq!(window.find("sheet-tab-color-7").label(), Some("#EF4444"));
    })
    .expect("verify M2-06 multi-worksheet tabs, independent scroll memory, tab colors, and hidden sheets");
}
