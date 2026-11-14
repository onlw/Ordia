//! Single-row contextual toolbar slot, right property inspector panel slot, and status bar
//! for [`crate::MainWindow`] (`M1-02`, `PLAN.md` §5.1).

use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, status_bar::StatusBar, v_flex,
};
use gpui_kit::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _, SharedString, Styled as _,
    div, px,
};
use ordia_ui::{
    OrdiaIcon, OrdiaTokens, SegmentOption, SegmentedControl, ToolbarButton, ToolbarColorPicker,
    ToolbarSelect,
};

use crate::MainWindow;

impl MainWindow {
    /// Renders the single-row contextual toolbar slot (`PLAN.md` §5.1).
    pub(crate) fn render_toolbar_slot(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = OrdiaTokens::for_mode(cx.theme().mode);
        let entity = cx.entity();

        h_flex()
            .id("shell-toolbar-slot")
            .w_full()
            .px(tokens.spacing.md)
            .py(tokens.spacing.xs)
            .gap(tokens.spacing.xs)
            .items_center()
            .justify_between()
            .bg(cx.theme().popover)
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .flex_wrap()
                    .gap(tokens.spacing.xs)
                    .items_center()
                    .child(
                        ToolbarButton::new("toolbar-undo")
                            .icon(OrdiaIcon::Undo)
                            .tooltip("Undo (Cmd/Ctrl+Z)")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        if let Some(sheet) = this.active_sheet_view().cloned() {
                                            sheet.update(cx, |s, cx| {
                                                let _ = s.document_mut().undo();
                                                cx.notify();
                                            });
                                        }
                                    });
                                }
                            }),
                    )
                    .child(
                        ToolbarButton::new("toolbar-redo")
                            .icon(OrdiaIcon::Redo)
                            .tooltip("Redo (Cmd/Ctrl+Shift+Z)")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        if let Some(sheet) = this.active_sheet_view().cloned() {
                                            sheet.update(cx, |s, cx| {
                                                let _ = s.document_mut().redo();
                                                cx.notify();
                                            });
                                        }
                                    });
                                }
                            }),
                    )
                    .child(Self::toolbar_divider(cx))
                    .child(
                        ToolbarSelect::new("toolbar-font-family", &self.toolbar_font_select)
                            .with_width(px(136.0))
                            .with_menu_width(px(176.0)),
                    )
                    .child(
                        ToolbarSelect::new("toolbar-font-size", &self.toolbar_size_select)
                            .with_width(px(64.0)),
                    )
                    .child(Self::toolbar_divider(cx))
                    .child(
                        ToolbarButton::new("toolbar-bold")
                            .icon(OrdiaIcon::Bold)
                            .tooltip("Bold (Cmd/Ctrl+B)"),
                    )
                    .child(
                        ToolbarButton::new("toolbar-italic")
                            .icon(OrdiaIcon::Italic)
                            .tooltip("Italic (Cmd/Ctrl+I)"),
                    )
                    .child(
                        ToolbarButton::new("toolbar-underline")
                            .icon(OrdiaIcon::Underline)
                            .tooltip("Underline (Cmd/Ctrl+U)"),
                    )
                    .child(
                        ToolbarColorPicker::new(&self.toolbar_fill_picker)
                            .icon(OrdiaIcon::FillColor)
                            .accessibility_label("Cell fill color"),
                    )
                    .child(
                        ToolbarColorPicker::new(&self.toolbar_text_picker)
                            .icon(OrdiaIcon::TextColor)
                            .accessibility_label("Cell text color"),
                    )
                    .child(Self::toolbar_divider(cx))
                    .child(
                        SegmentedControl::new("toolbar-align")
                            .option(SegmentOption::icon(OrdiaIcon::AlignLeft, "Align left"))
                            .option(SegmentOption::icon(OrdiaIcon::AlignCenter, "Align center"))
                            .option(SegmentOption::icon(OrdiaIcon::AlignRight, "Align right"))
                            .selected_index(self.toolbar_align_ix)
                            .on_select({
                                let entity = entity.clone();
                                move |ix, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.toolbar_align_ix = ix;
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(Self::toolbar_divider(cx))
                    .child(
                        ToolbarButton::new("toolbar-formula")
                            .icon(OrdiaIcon::Formula)
                            .tooltip("Insert function"),
                    )
                    .child(
                        ToolbarButton::new("toolbar-chart")
                            .icon(OrdiaIcon::Chart)
                            .tooltip("Insert chart"),
                    ),
            )
            .child(
                ToolbarButton::new("toggle-inspector")
                    .icon(OrdiaIcon::Inspector)
                    .selected(self.is_inspector_open())
                    .tooltip("Toggle property inspector")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_inspector(cx);
                    })),
            )
    }

    fn toolbar_divider(cx: &Context<Self>) -> impl IntoElement {
        div().w(px(1.0)).h(px(16.0)).bg(cx.theme().border).mx_0p5()
    }

    /// Renders the collapsible right contextual property inspector panel slot (`PLAN.md` §5.1).
    pub(crate) fn render_inspector_slot(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = OrdiaTokens::for_mode(cx.theme().mode);
        let tab_title = self
            .active_tab()
            .map(|t| t.title().clone())
            .unwrap_or_else(|| SharedString::from("No workbook"));
        let tab_summary = self
            .active_tab()
            .map(|t| t.summary().clone())
            .unwrap_or_else(|| SharedString::from("Select a cell or object to inspect properties"));

        let active_addr = self
            .active_sheet_view()
            .map(|s| s.read(cx).document().active_cell().to_a1())
            .unwrap_or_else(|| String::from("A1"));

        v_flex()
            .id("shell-inspector-slot")
            .w(px(248.0))
            .h_full()
            .flex_shrink_0()
            .p(tokens.spacing.md)
            .gap(tokens.spacing.md)
            .bg(cx.theme().popover)
            .border_l_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .font_semibold()
                            .text_size(tokens.typography.base)
                            .child("Properties"),
                    )
                    .child(
                        ToolbarButton::new("close-inspector")
                            .icon(OrdiaIcon::Close)
                            .tooltip("Close inspector")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_inspector(cx);
                            })),
                    ),
            )
            .child(
                v_flex()
                    .p(tokens.spacing.sm)
                    .gap(tokens.spacing.xxs)
                    .rounded(tokens.radius.sm)
                    .bg(cx.theme().secondary)
                    .child(
                        div()
                            .text_size(tokens.typography.caption)
                            .text_color(cx.theme().muted_foreground)
                            .child("Active Selection"),
                    )
                    .child(
                        div()
                            .font_family(tokens.typography.ui_mono.clone())
                            .font_semibold()
                            .text_size(tokens.typography.base)
                            .child(SharedString::from(format!("{tab_title} · {active_addr}"))),
                    )
                    .child(
                        div()
                            .text_size(tokens.typography.caption)
                            .text_color(cx.theme().muted_foreground)
                            .child(tab_summary),
                    ),
            )
            .child(
                v_flex()
                    .gap(tokens.spacing.xs)
                    .child(
                        div()
                            .text_size(tokens.typography.caption)
                            .font_medium()
                            .text_color(cx.theme().muted_foreground)
                            .child("SECTIONS"),
                    )
                    .children(
                        [
                            "Cell & Alignment",
                            "Number Format",
                            "Conditional Rules",
                            "Chart",
                        ]
                        .into_iter()
                        .map(|sec| {
                            h_flex()
                                .px(tokens.spacing.sm)
                                .py(tokens.spacing.xs)
                                .rounded(tokens.radius.xs)
                                .bg(cx.theme().background)
                                .border_1()
                                .border_color(cx.theme().border)
                                .text_size(tokens.typography.sm)
                                .child(sec)
                        }),
                    ),
            )
    }

    /// Renders the bottom status bar (`PLAN.md` §5.1).
    pub(crate) fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let left_status = if self.is_showing_home() {
            SharedString::from("Home · Ready")
        } else if let Some(tab) = self.active_tab() {
            SharedString::from(format!("{} · {}", tab.title(), tab.summary()))
        } else {
            SharedString::from("Ready")
        };

        let center_stats = if self.is_showing_home() {
            SharedString::from(format!("{} recent workbooks", self.recent_files().len()))
        } else if let Some(sheet) = self.active_sheet_view() {
            let coord = sheet.read(cx).document().active_cell();
            SharedString::from(format!("Active {} · Ready", coord.to_a1()))
        } else {
            SharedString::from("")
        };

        StatusBar::new()
            .left(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .child(Icon::new(OrdiaIcon::Spreadsheet).xsmall())
                    .child(left_status),
            )
            .child(center_stats)
            .right(SharedString::from("100%"))
    }
}
