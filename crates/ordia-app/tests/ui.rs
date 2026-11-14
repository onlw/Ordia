//! Headless UI integration tests for the Ordia application shell (`M1-02`).

use std::path::PathBuf;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, Bounds, TestAppContext, WindowBounds, WindowOptions, px, size};
use ordia_app::MainWindow;

#[gpui_kit::test]
fn shell_new_switch_close_reorder_tabs_home_inspector_and_multiwindow(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        ordia_app::register_bindings_and_menus(cx);
    });

    let (handle, view) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1200.), px(780.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| MainWindow::new(window, cx)),
        )
        .expect("open headless main window")
    });

    let initial_dark = cx.update(|cx| cx.theme().mode.is_dark());

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);

        // 1. Theme toggle round-trip.
        window.click("toggle-theme", cx);
        assert_eq!(cx.theme().mode.is_dark(), !initial_dark);
        window.click("toggle-theme", cx);
        assert_eq!(cx.theme().mode.is_dark(), initial_dark);

        // 2. New tab via '+' button (`Book4.xlsx`, id=4).
        window.click("new-workbook-tab", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).tabs().len(), 4);
        assert_eq!(view.read(cx).active_tab_ix(), 3);
        assert_eq!(
            view.read(cx).active_tab().map(|t| t.title().as_ref()),
            Some("Book4.xlsx")
        );

        // 3. Switch tab by clicking tab pill `workbook-tab-2` (`Sales_Pipeline.xlsx`).
        window.click("workbook-tab-2", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).active_tab_ix(), 1);
        assert_eq!(
            view.read(cx).active_tab().map(|t| t.title().as_ref()),
            Some("Sales_Pipeline.xlsx")
        );

        // 4. Drag-reorder tabs: move `Budget_2026.xlsx` (ix 0) to ix 2; active tab `Sales_Pipeline.xlsx` (id=2) shifts to ix 0!
        view.update(cx, |this, cx| {
            this.move_tab(0, 2, cx);
        });
        window.render_frame(cx);
        assert_eq!(view.read(cx).active_tab_ix(), 0);
        assert_eq!(
            view.read(cx).active_tab().map(|t| t.title().as_ref()),
            Some("Sales_Pipeline.xlsx")
        );

        // 5. Right contextual property inspector slot toggle & close button.
        assert!(!view.read(cx).is_inspector_open());
        window.click("toggle-inspector", cx);
        window.render_frame(cx);
        assert!(view.read(cx).is_inspector_open());
        window.click("close-inspector", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).is_inspector_open());

        // 6. Close tab via per-tab close button (`close-tab-4`), then close remaining tabs to enter Home view.
        window.click("close-tab-4", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).tabs().len(), 3);

        view.update(cx, |this, cx| {
            while !this.tabs().is_empty() {
                this.close_tab(0, window, cx);
            }
        });
        window.render_frame(cx);
        assert!(view.read(cx).is_showing_home());
        assert!(view.read(cx).active_tab().is_none());

        // 7. Create a new workbook directly from the Home screen (`home-new-blank`) and drop an external `.xlsx` file.
        window.click("home-new-blank", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).is_showing_home());
        assert_eq!(view.read(cx).tabs().len(), 1);

        let sample_xlsx = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/public/m0_budget_sample.xlsx");
        view.update(cx, |this, cx| {
            this.open_dropped_paths(&[sample_xlsx], window, cx);
        });
        window.render_frame(cx);
        assert_eq!(view.read(cx).tabs().len(), 2);
        assert_eq!(
            view.read(cx).active_tab().map(|t| t.title().as_ref()),
            Some("m0_budget_sample.xlsx")
        );
    })
    .expect("drive main window interactions");

    // 8. Multi-window: open a second independent MainWindow instance.
    let (_handle2, view2) = cx.update(|cx| MainWindow::open(cx).expect("open second window"));
    cx.update(|cx| {
        assert_eq!(view.read(cx).tabs().len(), 2);
        assert_eq!(view2.read(cx).tabs().len(), 3);
    });
}

#[gpui_kit::test]
fn command_palette_pinyin_search_and_registry_toolbar_execution(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        ordia_app::register_bindings_and_menus(cx);
    });

    let (handle, view) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1200.), px(780.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| MainWindow::new(window, cx)),
        )
        .expect("open headless main window")
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(!view.read(cx).is_command_palette_open());
        assert_eq!(view.read(cx).saved_count(), 0);

        // Execute `file.save` via the toolbar button generated from `CommandRegistry`.
        window.click("toolbar-file.save", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).saved_count(), 1);

        // Open the `Cmd+K` Command Palette, verify Pinyin query `"bc"` surfaces `"file.save"` (`保存`),
        // and verify closing the palette restores clean state.
        window.click("open-command-palette", cx);
        window.render_frame(cx);
        assert!(view.read(cx).is_command_palette_open());

        let hits = view.read(cx).command_registry().search("bc");
        assert!(!hits.is_empty());
        assert_eq!(hits[0].id(), "file.save");
        assert_eq!(hits[0].label_zh(), "保存");

        view.update(cx, |this, cx| {
            this.close_command_palette(window, cx);
        });
        window.render_frame(cx);
        assert!(!view.read(cx).is_command_palette_open());
    })
    .expect("drive command palette UI test");
}
