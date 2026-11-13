//! Headless UI integration tests for the Ordia GPUI Kit shell prototype.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, Bounds, TestAppContext, WindowBounds, WindowOptions, px, size};
use ordia_app::MainWindow;

#[gpui_kit::test]
fn main_window_theme_tabs_and_sidebar_interactions(cx: &mut TestAppContext) {
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
        .expect("open headless window")
    });

    let initial_dark = cx.update(|cx| cx.theme().mode.is_dark());

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);

        // Verify theme toggle button switches theme immediately.
        window.click("toggle-theme", cx);
        assert_eq!(cx.theme().mode.is_dark(), !initial_dark);

        // Toggle back and verify round-trip.
        window.click("toggle-theme", cx);
        assert_eq!(cx.theme().mode.is_dark(), initial_dark);

        // Click '+' button to create a new workbook tab.
        window.click("new-workbook-tab", cx);
    })
    .expect("drive headless window");

    cx.update(|cx| {
        let state = view.read(cx);
        assert_eq!(state.tabs().len(), 4);
        assert_eq!(state.active_tab_ix(), 3);
        assert_eq!(
            state.active_tab().map(|t| t.title().as_ref()),
            Some("Book4.xlsx")
        );
        assert!(!state.is_sidebar_collapsed());
    });

    // Exercise sidebar collapse and tab selection on the entity.
    cx.update(|cx| {
        view.update(cx, |this, cx| {
            this.toggle_sidebar(cx);
            this.select_tab(1, cx);
        });
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
    })
    .expect("render collapsed sidebar frame");

    cx.update(|cx| {
        let state = view.read(cx);
        assert!(state.is_sidebar_collapsed());
        assert_eq!(state.active_tab_ix(), 1);
        assert_eq!(
            state.active_tab().map(|t| t.title().as_ref()),
            Some("Sales_Pipeline.xlsx")
        );
    });
}
