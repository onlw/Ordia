//! Title bar, draggable/closable workbook tab strip, and left navigation sidebar
//! for [`crate::MainWindow`] (`M1-02`, `PLAN.md` §5.1).

use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, TitleBar,
    button::{Button, ButtonVariants as _},
    h_flex,
    scroll::ScrollableElement as _,
    sidebar::{
        Sidebar, SidebarCollapsible, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu,
        SidebarMenuItem, SidebarToggleButton,
    },
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, div, px,
};
use ordia_ui::{OrdiaIcon, OrdiaTokens};

use crate::MainWindow;
use crate::shell_types::{DraggedTab, DraggedTabPreview, SidebarSection};

impl MainWindow {
    pub(crate) fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let is_dark = cx.theme().mode.is_dark();
        let theme_icon = if is_dark {
            IconName::Sun
        } else {
            IconName::Moon
        };
        let theme_label = if is_dark { "Light" } else { "Dark" };
        let tokens = OrdiaTokens::for_mode(cx.theme().mode);
        let entity = cx.entity();

        let tab_pills: Vec<_> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(ix, tab)| {
                let is_active = !self.show_home && ix == self.active_tab_ix;
                let tab_id = tab.id();
                let title = tab.title().clone();
                let drag_payload = DraggedTab {
                    tab_id,
                    title: title.clone(),
                };
                let drag_id = SharedString::from(format!("drag-tab-{tab_id}"));
                let pill_id = SharedString::from(format!("workbook-tab-{tab_id}"));
                let close_id = SharedString::from(format!("close-tab-{tab_id}"));

                h_flex()
                    .id(drag_id)
                    .h(px(26.0))
                    .px(tokens.spacing.xxs)
                    .gap(tokens.spacing.xxs)
                    .items_center()
                    .flex_shrink_0()
                    .rounded(tokens.radius.sm)
                    .border_1()
                    .bg(if is_active {
                        cx.theme().popover
                    } else {
                        cx.theme().secondary
                    })
                    .border_color(if is_active {
                        cx.theme().primary
                    } else {
                        cx.theme().border
                    })
                    .on_drag(drag_payload, |drag, _, _, cx| {
                        cx.new(|_| DraggedTabPreview {
                            title: drag.title.clone(),
                        })
                    })
                    .drag_over::<DraggedTab>(|style, _, _, cx| {
                        style.border_l_2().border_color(cx.theme().primary)
                    })
                    .on_drop({
                        let entity = entity.clone();
                        move |drag: &DraggedTab, _, cx| {
                            let dragged_id = drag.tab_id;
                            entity.update(cx, |this, cx| {
                                if let Some(from_ix) =
                                    this.tabs.iter().position(|t| t.id() == dragged_id)
                                {
                                    this.move_tab(from_ix, ix, cx);
                                }
                            });
                        }
                    })
                    .child(
                        Button::new(pill_id)
                            .ghost()
                            .xsmall()
                            .compact()
                            .icon(Icon::new(OrdiaIcon::Spreadsheet))
                            .label(title)
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.select_tab(ix, cx);
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new(close_id)
                            .ghost()
                            .xsmall()
                            .compact()
                            .icon(IconName::Close)
                            .tooltip("Close tab")
                            .on_click({
                                let entity = entity.clone();
                                move |_, window, cx| {
                                    cx.stop_propagation();
                                    entity.update(cx, |this, cx| {
                                        this.close_tab_by_id(tab_id, window, cx);
                                    });
                                }
                            }),
                    )
            })
            .collect();

        TitleBar::new().child(
            h_flex()
                .w_full()
                .pr_2()
                .gap_2()
                .items_center()
                .justify_between()
                .child(
                    h_flex()
                        .gap_1p5()
                        .min_w_0()
                        .flex_1()
                        .items_center()
                        .child(
                            SidebarToggleButton::new()
                                .collapsed(self.sidebar_collapsed)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.toggle_sidebar(cx);
                                })),
                        )
                        .child(
                            Button::new("show-home")
                                .ghost()
                                .small()
                                .label(crate::app_name())
                                .tooltip("Home (Recent & Templates)")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.show_home_screen(cx);
                                })),
                        )
                        .child(
                            h_flex()
                                .id("workbook-tabs-strip")
                                .flex_1()
                                .min_w_0()
                                .overflow_x_scrollbar()
                                .gap_1()
                                .items_center()
                                .children(tab_pills)
                                .child(
                                    Button::new("new-workbook-tab")
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::Plus)
                                        .tooltip("New workbook (Cmd/Ctrl+T)")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.add_workbook_tab(window, cx);
                                        })),
                                ),
                        ),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(
                            Button::new("new-window-btn")
                                .ghost()
                                .xsmall()
                                .icon(Icon::new(OrdiaIcon::Open))
                                .tooltip("New window (Cmd/Ctrl+Shift+N)")
                                .on_click(|_, _, cx| {
                                    let _ = crate::MainWindow::open(cx);
                                }),
                        )
                        .child(
                            Button::new("toggle-theme")
                                .ghost()
                                .small()
                                .icon(theme_icon)
                                .label(theme_label)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.toggle_theme(window, cx);
                                })),
                        ),
                ),
        )
    }

    pub(crate) fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let collapsed = self.sidebar_collapsed;
        let active = self.active_section;

        Sidebar::new("navigation-sidebar")
            .collapsible(SidebarCollapsible::Icon)
            .collapsed(collapsed)
            .header(
                SidebarHeader::new()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .size_7()
                            .flex_shrink_0()
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().sidebar_primary)
                            .text_color(cx.theme().sidebar_primary_foreground)
                            .child(Icon::new(IconName::LayoutDashboard).small()),
                    )
                    .when(!collapsed, |header| {
                        header.child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .child(div().text_sm().font_semibold().child("Ordia Sheets"))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("Workspace"),
                                ),
                        )
                    }),
            )
            .child(
                SidebarGroup::new("Navigation").child(
                    SidebarMenu::new().children([
                        SidebarMenuItem::new("Recent")
                            .icon(IconName::FileText)
                            .active(active == SidebarSection::Recent)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.active_section = SidebarSection::Recent;
                                this.show_home = true;
                                cx.notify();
                            })),
                        SidebarMenuItem::new("Templates")
                            .icon(IconName::Folder)
                            .active(active == SidebarSection::Templates)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.active_section = SidebarSection::Templates;
                                this.show_home = true;
                                cx.notify();
                            })),
                        SidebarMenuItem::new("Settings")
                            .icon(IconName::Settings)
                            .active(active == SidebarSection::Settings)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.active_section = SidebarSection::Settings;
                                cx.notify();
                            })),
                    ]),
                ),
            )
            .footer(
                SidebarFooter::new().child(
                    h_flex()
                        .gap_2()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(Icon::new(IconName::HardDrive).small())
                        .when(!collapsed, |row| row.child("Local-first · Offline")),
                ),
            )
    }
}
