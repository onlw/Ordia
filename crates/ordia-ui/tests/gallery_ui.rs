//! Unit and headless GPUI Kit UI integration tests for `ordia-ui`.

use gpui_kit::assets::IconNamed as _;
use gpui_kit::component::{ActiveTheme as _, ThemeMode};
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, Bounds, TestAppContext, WindowBounds, WindowOptions, px, size};
use ordia_ui::{DesignGalleryView, OrdiaIcon, OrdiaTokens, spreadsheet_color_swatches};

#[test]
fn tokens_preserve_paper_white_spreadsheet_invariance_and_distinct_chrome() {
    let light = OrdiaTokens::light();
    let dark = OrdiaTokens::dark();

    assert_eq!(light.colors.sheet_paper_bg, dark.colors.sheet_paper_bg);
    assert_ne!(light.colors.background, dark.colors.background);
    assert_ne!(light.colors.surface, dark.colors.surface);
    assert_ne!(light.colors.foreground, dark.colors.foreground);
    assert_eq!(spreadsheet_color_swatches().len(), 18);

    for icon in OrdiaIcon::ALL {
        assert!(
            icon.path().starts_with("icons/") && icon.path().ends_with(".svg"),
            "unexpected icon path for {:?}: {}",
            icon,
            icon.path()
        );
        assert!(!icon.name().is_empty());
    }
}

#[gpui_kit::test]
fn design_gallery_renders_light_and_dark_themes_and_interactive_controls(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let (handle, gallery) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1200.0), px(800.0)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| DesignGalleryView::new(window, cx)),
        )
        .expect("open design gallery window")
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(gallery.read(cx).is_bold());

        // Click Bold toolbar button to toggle off.
        window.click("gallery-btn-bold", cx);
        assert!(!gallery.read(cx).is_bold());

        // Switch global theme to Dark and render a frame.
        gallery.update(cx, |this, cx| {
            this.set_theme_mode(ThemeMode::Dark, window, cx);
            this.set_alignment_ix(1, cx);
        });
        window.render_frame(cx);
        assert!(cx.theme().mode.is_dark());
        assert_eq!(gallery.read(cx).alignment_ix(), 1);
        assert_eq!(
            cx.theme().primary,
            OrdiaTokens::dark().colors.primary,
            "dark semantic primary token should be projected onto GPUI Kit Theme"
        );

        // Switch global theme back to Light and render a frame.
        gallery.update(cx, |this, cx| {
            this.set_theme_mode(ThemeMode::Light, window, cx);
        });
        window.render_frame(cx);
        assert!(!cx.theme().mode.is_dark());
        assert_eq!(
            cx.theme().primary,
            OrdiaTokens::light().colors.primary,
            "light semantic primary token should be projected onto GPUI Kit Theme"
        );
    })
    .expect("exercise gallery interactions");
}
