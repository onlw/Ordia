//! Interactive Ordia Design System Gallery (`cargo run -p ordia-app -- --gallery`).
//!
//! Displays live interactive Ordia toolbar components alongside simultaneous Light and Dark
//! token & component preview surfaces so both theme variants can be inspected side-by-side.

use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, StyledExt as _, ThemeMode, TitleBar,
    color_picker::ColorPickerState, h_flex, scroll::ScrollableElement as _, v_flex,
};
use gpui_kit::{
    App, Context, Entity, FocusHandle, Focusable, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Window, div, px,
};

use crate::components::{
    SegmentOption, SegmentedControl, ToolbarButton, ToolbarColorPicker, ToolbarSelect,
    ToolbarSelectState, new_toolbar_color_picker_state, new_toolbar_select_state,
};
use crate::icons::OrdiaIcon;
use crate::theme::{OrdiaTokens, apply_ordia_theme};

/// Interactive showcase view for `ordia-ui` tokens, icons, and component wrappers.
pub struct DesignGalleryView {
    focus_handle: FocusHandle,
    font_family_select: Entity<ToolbarSelectState>,
    font_size_select: Entity<ToolbarSelectState>,
    fill_color_picker: Entity<ColorPickerState>,
    text_color_picker: Entity<ColorPickerState>,
    bold: bool,
    italic: bool,
    underline: bool,
    alignment_ix: usize,
}

impl DesignGalleryView {
    /// Creates a new interactive design gallery view and applies Ordia's initial theme tokens.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let initial_mode = cx.theme().mode;
        apply_ordia_theme(initial_mode, Some(window), cx);

        let light = OrdiaTokens::light();
        let font_family_select = new_toolbar_select_state(
            ["Carlito", "PingFang SC", "Inter", "JetBrains Mono"],
            Some(0),
            window,
            cx,
        );
        let font_size_select =
            new_toolbar_select_state(["10", "11", "12", "14", "18", "24"], Some(1), window, cx);
        let fill_color_picker =
            new_toolbar_color_picker_state(Some(light.colors.accent_subtle), window, cx);
        let text_color_picker =
            new_toolbar_color_picker_state(Some(light.colors.primary), window, cx);

        Self {
            focus_handle: cx.focus_handle().tab_stop(true),
            font_family_select,
            font_size_select,
            fill_color_picker,
            text_color_picker,
            bold: true,
            italic: false,
            underline: false,
            alignment_ix: 0,
        }
    }

    /// Returns whether the sample Bold toggle is active.
    pub fn is_bold(&self) -> bool {
        self.bold
    }

    /// Returns whether the sample Italic toggle is active.
    pub fn is_italic(&self) -> bool {
        self.italic
    }

    /// Returns whether the sample Underline toggle is active.
    pub fn is_underline(&self) -> bool {
        self.underline
    }

    /// Returns the selected horizontal alignment index (`0 = Left`, `1 = Center`, `2 = Right`).
    pub fn alignment_ix(&self) -> usize {
        self.alignment_ix
    }

    /// Returns the currently active global [`ThemeMode`].
    pub fn theme_mode(&self, cx: &App) -> ThemeMode {
        cx.theme().mode
    }

    /// Toggles the Bold toolbar state.
    pub fn toggle_bold(&mut self, cx: &mut Context<Self>) {
        self.bold = !self.bold;
        cx.notify();
    }

    /// Updates the selected horizontal alignment segment (`0..=2`).
    pub fn set_alignment_ix(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.alignment_ix = ix.min(2);
        cx.notify();
    }

    /// Switches Ordia's global theme mode (`Light` or `Dark`) and updates GPUI Kit semantic tokens.
    pub fn set_theme_mode(&mut self, mode: ThemeMode, window: &mut Window, cx: &mut Context<Self>) {
        apply_ordia_theme(mode, Some(window), cx);
        cx.notify();
    }

    fn render_theme_preview_card(tokens: &OrdiaTokens) -> impl IntoElement {
        let is_dark = tokens.mode.is_dark();
        let mode_title = if is_dark {
            "Dark Theme Preview (Slate Chrome + Paper Canvas)"
        } else {
            "Light Theme Preview (Zinc Chrome + Paper Canvas)"
        };
        let card_id = if is_dark {
            "preview-dark"
        } else {
            "preview-light"
        };

        let swatches: [(&str, Hsla); 8] = [
            ("background", tokens.colors.background),
            ("surface", tokens.colors.surface),
            ("muted", tokens.colors.surface_muted),
            ("primary", tokens.colors.primary),
            ("accent", tokens.colors.accent_subtle),
            ("success", tokens.colors.success),
            ("warning", tokens.colors.warning),
            ("danger", tokens.colors.danger),
        ];

        v_flex()
            .id(card_id)
            .flex_1()
            .gap(tokens.spacing.md)
            .p(tokens.spacing.lg)
            .rounded(tokens.radius.lg)
            .bg(tokens.colors.background)
            .border_1()
            .border_color(tokens.colors.border)
            .shadow(tokens.elevation.md.clone())
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_size(tokens.typography.md)
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(tokens.colors.foreground)
                            .child(mode_title),
                    )
                    .child(
                        div()
                            .px(tokens.spacing.sm)
                            .py(tokens.spacing.xxs)
                            .rounded(tokens.radius.xs)
                            .bg(tokens.colors.accent_subtle)
                            .text_size(tokens.typography.caption)
                            .text_color(tokens.colors.accent_foreground)
                            .child(if is_dark { "DARK" } else { "LIGHT" }),
                    ),
            )
            // Simulated compact toolbar strip in this theme's exact surface tokens
            .child(
                h_flex()
                    .gap(tokens.spacing.xs)
                    .p(tokens.spacing.sm)
                    .rounded(tokens.radius.md)
                    .bg(tokens.colors.surface)
                    .border_1()
                    .border_color(tokens.colors.border)
                    .items_center()
                    .child(
                        div()
                            .px(tokens.spacing.sm)
                            .py(tokens.spacing.xs)
                            .rounded(tokens.radius.sm)
                            .bg(tokens.colors.accent_subtle)
                            .text_size(tokens.typography.sm)
                            .text_color(tokens.colors.accent_foreground)
                            .child("B Active"),
                    )
                    .child(
                        div()
                            .px(tokens.spacing.sm)
                            .py(tokens.spacing.xs)
                            .rounded(tokens.radius.sm)
                            .text_size(tokens.typography.sm)
                            .text_color(tokens.colors.foreground)
                            .child("Carlito 11 ▾"),
                    )
                    .child(
                        div()
                            .px(tokens.spacing.sm)
                            .py(tokens.spacing.xs)
                            .rounded(tokens.radius.sm)
                            .bg(tokens.colors.surface_muted)
                            .text_size(tokens.typography.sm)
                            .text_color(tokens.colors.muted_foreground)
                            .child("Left | Center | Right"),
                    )
                    .child(
                        div()
                            .px(tokens.spacing.sm)
                            .py(tokens.spacing.xs)
                            .rounded(tokens.radius.sm)
                            .bg(tokens.colors.primary)
                            .text_size(tokens.typography.sm)
                            .text_color(tokens.colors.primary_foreground)
                            .child("Primary"),
                    ),
            )
            // Semantic color swatches
            .child(
                h_flex()
                    .flex_wrap()
                    .gap(tokens.spacing.sm)
                    .children(swatches.into_iter().map(|(name, color)| {
                        v_flex()
                            .gap(tokens.spacing.xs)
                            .items_center()
                            .child(
                                div()
                                    .w(px(36.0))
                                    .h(px(24.0))
                                    .rounded(tokens.radius.xs)
                                    .bg(color)
                                    .border_1()
                                    .border_color(tokens.colors.border),
                            )
                            .child(
                                div()
                                    .text_size(tokens.typography.caption)
                                    .text_color(tokens.colors.muted_foreground)
                                    .child(name),
                            )
                    })),
            )
            // Spreadsheet paper-white invariance demonstration (`PLAN.md` §5.2)
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .p(tokens.spacing.sm)
                    .rounded(tokens.radius.sm)
                    .bg(tokens.colors.sheet_paper_bg)
                    .border_2()
                    .border_color(tokens.colors.selection_border)
                    .child(
                        div()
                            .text_size(tokens.typography.base)
                            .text_color(OrdiaTokens::light().colors.foreground)
                            .child("B4 · Spreadsheet Paper Canvas (Invariant #FFFFFF)"),
                    )
                    .child(
                        div()
                            .font_family(tokens.typography.ui_mono.clone())
                            .text_size(tokens.typography.sm)
                            .text_color(tokens.colors.selection_border)
                            .child("=SUM(B2:B3) → ¥128,400.00"),
                    ),
            )
    }
}

impl Focusable for DesignGalleryView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for DesignGalleryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active_tokens = OrdiaTokens::for_mode(cx.theme().mode);
        let light_tokens = OrdiaTokens::light();
        let dark_tokens = OrdiaTokens::dark();
        let mode_ix = usize::from(cx.theme().mode.is_dark());
        let entity = cx.entity();

        v_flex()
            .id("ordia-design-gallery")
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new().child(
                    h_flex()
                        .w_full()
                        .pr(active_tokens.spacing.md)
                        .items_center()
                        .justify_between()
                        .child(
                            h_flex()
                                .gap(active_tokens.spacing.sm)
                                .items_center()
                                .child(Icon::new(OrdiaIcon::Spreadsheet).small())
                                .child(
                                    div()
                                        .font_semibold()
                                        .text_size(active_tokens.typography.base)
                                        .child("Ordia Design System Gallery"),
                                ),
                        )
                        .child(
                            SegmentedControl::new("gallery-theme-mode")
                                .option(SegmentOption::icon_and_label(
                                    OrdiaIcon::LightMode,
                                    "Light",
                                ))
                                .option(SegmentOption::icon_and_label(OrdiaIcon::DarkMode, "Dark"))
                                .selected_index(mode_ix)
                                .on_select({
                                    let entity = entity.clone();
                                    move |ix, window, cx| {
                                        let next = if ix == 1 {
                                            ThemeMode::Dark
                                        } else {
                                            ThemeMode::Light
                                        };
                                        entity.update(cx, |this, cx| {
                                            this.set_theme_mode(next, window, cx);
                                        });
                                    }
                                }),
                        ),
                ),
            )
            .child(
                v_flex()
                    .id("gallery-scroll-body")
                    .flex_1()
                    .overflow_y_scrollbar()
                    .p(active_tokens.spacing.xl)
                    .gap(active_tokens.spacing.xl)
                    // Section 1: Live Interactive Wrapped Components
                    .child(
                        v_flex()
                            .gap(active_tokens.spacing.md)
                            .p(active_tokens.spacing.lg)
                            .rounded(active_tokens.radius.lg)
                            .bg(cx.theme().popover)
                            .border_1()
                            .border_color(cx.theme().border)
                            .child(
                                div()
                                    .font_semibold()
                                    .text_size(active_tokens.typography.md)
                                    .child(
                                        "Interactive Toolbar Components (ToolbarButton · ToolbarSelect · ToolbarColorPicker · SegmentedControl)",
                                    ),
                            )
                            .child(
                                h_flex()
                                    .flex_wrap()
                                    .gap(active_tokens.spacing.sm)
                                    .items_center()
                                    .child(
                                        ToolbarSelect::new(
                                            "gallery-font-family",
                                            &self.font_family_select,
                                        )
                                        .with_width(px(148.0))
                                        .with_menu_width(px(180.0)),
                                    )
                                    .child(
                                        ToolbarSelect::new(
                                            "gallery-font-size",
                                            &self.font_size_select,
                                        )
                                        .with_width(px(72.0)),
                                    )
                                    .child(
                                        ToolbarButton::new("gallery-btn-bold")
                                            .icon(OrdiaIcon::Bold)
                                            .tooltip("Bold (Cmd/Ctrl+B)")
                                            .selected(self.bold)
                                            .on_click({
                                                let entity = entity.clone();
                                                move |_, _, cx| {
                                                    entity.update(cx, |this, cx| {
                                                        this.toggle_bold(cx);
                                                    });
                                                }
                                            }),
                                    )
                                    .child(
                                        ToolbarButton::new("gallery-btn-italic")
                                            .icon(OrdiaIcon::Italic)
                                            .tooltip("Italic (Cmd/Ctrl+I)")
                                            .selected(self.italic)
                                            .on_click({
                                                let entity = entity.clone();
                                                move |_, _, cx| {
                                                    entity.update(cx, |this, cx| {
                                                        this.italic = !this.italic;
                                                        cx.notify();
                                                    });
                                                }
                                            }),
                                    )
                                    .child(
                                        ToolbarButton::new("gallery-btn-underline")
                                            .icon(OrdiaIcon::Underline)
                                            .tooltip("Underline (Cmd/Ctrl+U)")
                                            .selected(self.underline)
                                            .on_click({
                                                let entity = entity.clone();
                                                move |_, _, cx| {
                                                    entity.update(cx, |this, cx| {
                                                        this.underline = !this.underline;
                                                        cx.notify();
                                                    });
                                                }
                                            }),
                                    )
                                    .child(
                                        ToolbarButton::new("gallery-btn-disabled")
                                            .icon(OrdiaIcon::Redo)
                                            .label("Redo")
                                            .disabled(true)
                                            .tooltip("Nothing to redo"),
                                    )
                                    .child(
                                        ToolbarColorPicker::new(&self.fill_color_picker)
                                            .icon(OrdiaIcon::FillColor)
                                            .accessibility_label("Cell fill color"),
                                    )
                                    .child(
                                        ToolbarColorPicker::new(&self.text_color_picker)
                                            .icon(OrdiaIcon::TextColor)
                                            .accessibility_label("Cell text color"),
                                    )
                                    .child(
                                        SegmentedControl::new("gallery-alignment")
                                            .option(SegmentOption::icon(
                                                OrdiaIcon::AlignLeft,
                                                "Align left",
                                            ))
                                            .option(SegmentOption::icon(
                                                OrdiaIcon::AlignCenter,
                                                "Align center",
                                            ))
                                            .option(SegmentOption::icon(
                                                OrdiaIcon::AlignRight,
                                                "Align right",
                                            ))
                                            .selected_index(self.alignment_ix)
                                            .on_select({
                                                let entity = entity.clone();
                                                move |ix, _, cx| {
                                                    entity.update(cx, |this, cx| {
                                                        this.set_alignment_ix(ix, cx);
                                                    });
                                                }
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(active_tokens.typography.sm)
                                    .text_color(cx.theme().muted_foreground)
                                    .child(SharedString::from(format!(
                                        "State · Bold={}  Italic={}  Underline={}  Align={}  Theme={:?}",
                                        self.bold,
                                        self.italic,
                                        self.underline,
                                        ["Left", "Center", "Right"][self.alignment_ix],
                                        cx.theme().mode,
                                    ))),
                            ),
                    )
                    // Section 2: Simultaneous Light & Dark Theme Comparison
                    .child(
                        h_flex()
                            .gap(active_tokens.spacing.lg)
                            .items_start()
                            .child(Self::render_theme_preview_card(&light_tokens))
                            .child(Self::render_theme_preview_card(&dark_tokens)),
                    )
                    // Section 3: Semantic Lucide Icon Catalog (`OrdiaIcon::ALL`)
                    .child(
                        v_flex()
                            .gap(active_tokens.spacing.md)
                            .p(active_tokens.spacing.lg)
                            .rounded(active_tokens.radius.lg)
                            .bg(cx.theme().popover)
                            .border_1()
                            .border_color(cx.theme().border)
                            .child(
                                div()
                                    .font_semibold()
                                    .text_size(active_tokens.typography.md)
                                    .child(SharedString::from(format!(
                                        "Ordia Semantic Icons ({} Lucide glyphs)",
                                        OrdiaIcon::ALL.len()
                                    ))),
                            )
                            .child(
                                h_flex()
                                    .flex_wrap()
                                    .gap(active_tokens.spacing.sm)
                                    .children(OrdiaIcon::ALL.iter().copied().map(|icon| {
                                        h_flex()
                                            .gap(active_tokens.spacing.xs)
                                            .px(active_tokens.spacing.sm)
                                            .py(active_tokens.spacing.xs)
                                            .rounded(active_tokens.radius.sm)
                                            .bg(cx.theme().secondary)
                                            .items_center()
                                            .child(Icon::new(icon).small())
                                            .child(
                                                div()
                                                    .text_size(active_tokens.typography.caption)
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(icon.name()),
                                            )
                                    })),
                            ),
                    ),
            )
    }
}
