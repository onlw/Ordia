//! Single-row contextual toolbar slot, right property inspector panel slot, and status bar
//! for [`crate::MainWindow`] (`M1-02`, `PLAN.md` §5.1).

use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, StyledExt as _, command::Command, h_flex,
    status_bar::StatusBar, v_flex,
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
    /// Renders the single-row contextual toolbar slot (`PLAN.md` §5.1), generating command
    /// buttons (`file.save`, `edit.undo`, `edit.redo`, `view.toggle_inspector`) from [`crate::CommandRegistry`].
    pub(crate) fn render_toolbar_slot(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = OrdiaTokens::for_mode(cx.theme().mode);
        let entity = cx.entity();

        let leading_cmds: Vec<_> = ["file.save", "edit.undo", "edit.redo"]
            .into_iter()
            .filter_map(|id| self.command_registry.get(id).cloned())
            .map(|spec| {
                let btn_id = SharedString::from(format!("toolbar-{}", spec.id()));
                let enabled = spec.is_enabled(self, cx);
                let tip = SharedString::from(format!("{} · {}", spec.label_zh(), spec.label_en()));
                let icon = spec.icon();
                let entity = entity.clone();
                let mut btn = ToolbarButton::new(btn_id)
                    .disabled(!enabled)
                    .tooltip(tip)
                    .on_click(move |_, window, cx| {
                        let spec = spec.clone();
                        entity.update(cx, |this, cx| {
                            spec.execute(this, window, cx);
                        });
                    });
                if let Some(icon) = icon {
                    btn = btn.icon(icon);
                }
                btn
            })
            .collect();

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
                    .children(leading_cmds)
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
                            .tooltip(ordia_i18n::t!("toolbar.bold")),
                    )
                    .child(
                        ToolbarButton::new("toolbar-italic")
                            .icon(OrdiaIcon::Italic)
                            .tooltip(ordia_i18n::t!("toolbar.italic")),
                    )
                    .child(
                        ToolbarButton::new("toolbar-underline")
                            .icon(OrdiaIcon::Underline)
                            .tooltip(ordia_i18n::t!("toolbar.underline")),
                    )
                    .child(
                        ToolbarColorPicker::new(&self.toolbar_fill_picker)
                            .icon(OrdiaIcon::FillColor)
                            .accessibility_label(ordia_i18n::t!("toolbar.fill_color")),
                    )
                    .child(
                        ToolbarColorPicker::new(&self.toolbar_text_picker)
                            .icon(OrdiaIcon::TextColor)
                            .accessibility_label(ordia_i18n::t!("toolbar.text_color")),
                    )
                    .child(Self::toolbar_divider(cx))
                    .child(
                        SegmentedControl::new("toolbar-align")
                            .option(SegmentOption::icon(
                                OrdiaIcon::AlignLeft,
                                ordia_i18n::t!("toolbar.align_left"),
                            ))
                            .option(SegmentOption::icon(
                                OrdiaIcon::AlignCenter,
                                ordia_i18n::t!("toolbar.align_center"),
                            ))
                            .option(SegmentOption::icon(
                                OrdiaIcon::AlignRight,
                                ordia_i18n::t!("toolbar.align_right"),
                            ))
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
                            .tooltip(ordia_i18n::t!("toolbar.insert_formula")),
                    )
                    .child(
                        ToolbarButton::new("toolbar-chart")
                            .icon(OrdiaIcon::Chart)
                            .tooltip(ordia_i18n::t!("toolbar.insert_chart")),
                    ),
            )
            .child(
                ToolbarButton::new("toggle-inspector")
                    .icon(OrdiaIcon::Inspector)
                    .selected(self.is_inspector_open())
                    .tooltip(ordia_i18n::t!("toolbar.toggle_inspector"))
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
            .unwrap_or_else(|| SharedString::from(ordia_i18n::t!("inspector.no_workbook")));
        let tab_summary = self
            .active_tab()
            .map(|t| t.summary().clone())
            .unwrap_or_else(|| SharedString::from(ordia_i18n::t!("inspector.empty_hint")));

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
                            .child(ordia_i18n::t!("inspector.title")),
                    )
                    .child(
                        ToolbarButton::new("close-inspector")
                            .icon(OrdiaIcon::Close)
                            .tooltip(ordia_i18n::t!("inspector.close"))
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
                            .child(ordia_i18n::t!("inspector.active_selection")),
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
                            .child(ordia_i18n::t!("inspector.sections_heading")),
                    )
                    .children(
                        [
                            ordia_i18n::t!("inspector.sec_cell_align"),
                            ordia_i18n::t!("inspector.sec_number_format"),
                            ordia_i18n::t!("inspector.sec_conditional"),
                            ordia_i18n::t!("inspector.sec_chart"),
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
            SharedString::from(ordia_i18n::t!("status.home_ready"))
        } else if let Some(tab) = self.active_tab() {
            SharedString::from(format!("{} · {}", tab.title(), tab.summary()))
        } else {
            SharedString::from(ordia_i18n::t!("status.ready"))
        };

        let center_stats = if self.is_showing_home() {
            let count_str = self.recent_files().len().to_string();
            SharedString::from(ordia_i18n::t!("status.recent_count", "count" => count_str))
        } else if let Some(sheet) = self.active_sheet_view() {
            let coord = sheet.read(cx).document().active_cell().to_a1();
            SharedString::from(ordia_i18n::t!("status.active_cell_ready", "cell" => coord))
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

    /// Renders the floating `Cmd/Ctrl+K` Command Palette modal with Pinyin + fuzzy search.
    pub(crate) fn render_command_palette_overlay(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let tokens = OrdiaTokens::for_mode(cx.theme().mode);
        let items = self
            .command_registry
            .build_palette_items(&self.palette_query, self, cx);
        let entity = cx.entity();

        div()
            .id("command-palette-backdrop")
            .occlude()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_start()
            .justify_center()
            .pt(px(72.0))
            .bg(tokens.colors.focus_ring.opacity(0.18))
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.close_command_palette(window, cx);
                }),
            )
            .child(
                div()
                    .id("command-palette-modal")
                    .w(px(520.0))
                    .rounded(tokens.radius.lg)
                    .bg(cx.theme().popover)
                    .shadow(tokens.elevation.lg)
                    .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(
                        Command::new(&self.palette_state)
                            .filterable(false)
                            .placeholder(ordia_i18n::t!("palette.placeholder"))
                            .items(items)
                            .on_query({
                                let entity = entity.clone();
                                move |q, _, cx| {
                                    let q_str = SharedString::from(q.to_string());
                                    entity.update(cx, |this, cx| {
                                        this.palette_query = q_str;
                                        cx.notify();
                                    });
                                }
                            })
                            .on_confirm({
                                let entity = entity.clone();
                                move |_, window, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.close_command_palette(window, cx);
                                    });
                                }
                            })
                            .on_cancel(move |window, cx| {
                                entity.update(cx, |this, cx| {
                                    this.close_command_palette(window, cx);
                                });
                            }),
                    ),
            )
    }
}
