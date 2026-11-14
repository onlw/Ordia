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
