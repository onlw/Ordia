//! Home screen view for [`crate::MainWindow`] (`M1-02`, `PLAN.md` §5.1).
//!
//! Displays quick-create workbook templates, recent files, and a drag-and-drop file target.

use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    scroll::ScrollableElement as _,
    v_flex,
};
use gpui_kit::{
    Context, ExternalPaths, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use ordia_ui::{OrdiaIcon, OrdiaTokens};

use crate::MainWindow;

impl MainWindow {
    pub(crate) fn render_home_screen(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = OrdiaTokens::for_mode(cx.theme().mode);
        let entity = cx.entity();

        v_flex()
            .id("ordia-home-screen")
            .flex_1()
            .size_full()
            .overflow_y_scrollbar()
            .p(tokens.spacing.xl)
            .gap(tokens.spacing.xl)
            .bg(cx.theme().background)
            // Hero & Quick Create section
            .child(
                v_flex()
                    .gap(tokens.spacing.md)
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .child(
                                v_flex()
                                    .gap(tokens.spacing.xs)
                                    .child(
                                        div()
                                            .font_semibold()
                                            .text_size(tokens.typography.lg)
                                            .text_color(cx.theme().foreground)
                                            .child(ordia_i18n::t!("home.welcome_title")),
                                    )
                                    .child(
                                        div()
                                            .text_size(tokens.typography.sm)
                                            .text_color(cx.theme().muted_foreground)
                                            .child(ordia_i18n::t!("home.welcome_subtitle")),
                                    ),
                            )
                            .child(
                                Button::new("home-new-blank")
                                    .primary()
                                    .small()
                                    .icon(Icon::new(OrdiaIcon::Add))
                                    .label(ordia_i18n::t!("home.new_workbook"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.add_workbook_tab(window, cx);
                                    })),
                            ),
                    )
                    // Template cards row
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap(tokens.spacing.md)
                            .child(Self::render_template_card(
                                "home-card-blank",
                                ordia_i18n::t!("home.tpl_blank_title"),
                                ordia_i18n::t!("home.tpl_blank_subtitle"),
                                OrdiaIcon::Add,
                                &tokens,
                                cx,
                                {
                                    let entity = entity.clone();
                                    move |window, cx| {
                                        entity.update(cx, |this, cx| {
                                            this.add_workbook_tab(window, cx);
                                        });
                                    }
                                },
                            ))
                            .child(Self::render_template_card(
                                "home-card-budget",
                                ordia_i18n::t!("home.tpl_budget_title"),
                                ordia_i18n::t!("home.tpl_budget_subtitle"),
                                OrdiaIcon::Spreadsheet,
                                &tokens,
                                cx,
                                {
                                    let entity = entity.clone();
                                    move |window, cx| {
                                        entity.update(cx, |this, cx| {
                                            this.open_demo_template(
                                                "Budget_2026.xlsx",
                                                ordia_i18n::t!("home.tpl_budget_summary"),
                                                window,
                                                cx,
                                            );
                                        });
                                    }
                                },
                            ))
                            .child(Self::render_template_card(
                                "home-card-sales",
                                ordia_i18n::t!("home.tpl_sales_title"),
                                ordia_i18n::t!("home.tpl_sales_subtitle"),
                                OrdiaIcon::Chart,
                                &tokens,
                                cx,
                                {
                                    let entity = entity.clone();
                                    move |window, cx| {
                                        entity.update(cx, |this, cx| {
                                            this.open_demo_template(
                                                "Sales_Pipeline.xlsx",
                                                ordia_i18n::t!("home.tpl_sales_summary"),
                                                window,
                                                cx,
                                            );
                                        });
                                    }
                                },
                            )),
                    ),
            )
            // Drag-and-Drop file target zone
            .child(
                v_flex()
                    .id("home-drop-zone")
                    .p(tokens.spacing.lg)
                    .gap(tokens.spacing.xs)
                    .items_center()
                    .justify_center()
                    .rounded(tokens.radius.lg)
                    .bg(cx.theme().secondary)
                    .border_1()
                    .border_color(cx.theme().border)
                    .drag_over::<ExternalPaths>(|style, _, _, cx| {
                        style
                            .border_2()
                            .border_color(cx.theme().primary)
                            .bg(cx.theme().accent)
                    })
                    .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                        this.open_dropped_paths(paths.paths(), window, cx);
                    }))
                    .child(Icon::new(OrdiaIcon::Open))
                    .child(
                        div()
                            .font_medium()
                            .text_size(tokens.typography.base)
                            .child(ordia_i18n::t!("home.drop_title")),
                    )
                    .child(
                        div()
                            .text_size(tokens.typography.caption)
                            .text_color(cx.theme().muted_foreground)
                            .child(ordia_i18n::t!("home.drop_subtitle")),
                    ),
            )
            // Recent Files list
            .child(
                v_flex()
                    .gap(tokens.spacing.sm)
                    .child(
                        div()
                            .font_semibold()
                            .text_size(tokens.typography.md)
                            .child(ordia_i18n::t!("home.recent_heading")),
                    )
                    .children(self.recent_files().iter().enumerate().map(|(ix, entry)| {
                        let path = entry.path().clone();
                        let title = entry.title().clone();
                        let meta = SharedString::from(format!(
                            "{} · {}",
                            entry.last_opened_label(),
                            entry.path().display()
                        ));
                        let row_id = SharedString::from(format!("recent-file-{ix}"));
                        let open_btn_id = SharedString::from(format!("open-recent-{ix}"));
                        let path_btn = path.clone();
                        let title_btn = title.clone();
                        h_flex()
                            .id(row_id)
                            .px(tokens.spacing.md)
                            .py(tokens.spacing.sm)
                            .rounded(tokens.radius.md)
                            .bg(cx.theme().popover)
                            .border_1()
                            .border_color(cx.theme().border)
                            .justify_between()
                            .items_center()
                            .hover(|s| s.bg(cx.theme().secondary))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_recent_or_demo_entry(&title, &path, window, cx);
                            }))
                            .child(
                                h_flex()
                                    .gap(tokens.spacing.sm)
                                    .items_center()
                                    .child(Icon::new(OrdiaIcon::Spreadsheet).small())
                                    .child(
                                        v_flex()
                                            .child(
                                                div()
                                                    .font_medium()
                                                    .text_size(tokens.typography.base)
                                                    .child(entry.title().clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_size(tokens.typography.caption)
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(meta),
                                            ),
                                    ),
                            )
                            .child(
                                Button::new(open_btn_id)
                                    .ghost()
                                    .xsmall()
                                    .label(ordia_i18n::t!("home.open_btn"))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.open_recent_or_demo_entry(
                                            &title_btn, &path_btn, window, cx,
                                        );
                                    })),
                            )
                    })),
            )
    }

    fn render_template_card(
        id: &'static str,
        title: &'static str,
        subtitle: &'static str,
        icon: OrdiaIcon,
        tokens: &OrdiaTokens,
        cx: &Context<Self>,
        on_activate: impl Fn(&mut gpui_kit::Window, &mut gpui_kit::App) + 'static,
    ) -> impl IntoElement {
        v_flex()
            .id(id)
            .w(px(236.0))
            .p(tokens.spacing.md)
            .gap(tokens.spacing.xs)
            .rounded(tokens.radius.md)
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .hover(|s| s.border_color(cx.theme().primary).bg(cx.theme().secondary))
            .on_click(move |_, window, cx| on_activate(window, cx))
            .child(Icon::new(icon).small())
            .child(
                div()
                    .font_semibold()
                    .text_size(tokens.typography.base)
                    .child(title),
            )
            .child(
                div()
                    .text_size(tokens.typography.caption)
                    .text_color(cx.theme().muted_foreground)
                    .child(subtitle),
            )
    }
}
