//! Settings page view (`General`, `Appearance`, `Language`, `Editing`, `Advanced`)
//! for [`crate::MainWindow`] (`M1-05`).

use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    scroll::ScrollableElement as _,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _, SharedString, Styled as _,
    div, px,
};
use ordia_core::{AppearanceMode, EnterMoveDirection, LanguageSetting};
use ordia_ui::{OrdiaIcon, OrdiaTokens, SegmentOption, SegmentedControl};

use crate::MainWindow;

impl MainWindow {
    pub(crate) fn render_settings_screen(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = OrdiaTokens::for_mode(cx.theme().mode);
        let entity = cx.entity();

        let appearance_ix = match self.settings.appearance.theme {
            AppearanceMode::System => 0,
            AppearanceMode::Light => 1,
            AppearanceMode::Dark => 2,
        };
        let lang_ix = match self.settings.language {
            LanguageSetting::System => 0,
            LanguageSetting::ZhCn => 1,
            LanguageSetting::En => 2,
        };
        let enter_dir_ix = match self.settings.editing.enter_move_direction {
            EnterMoveDirection::Down => 0,
            EnterMoveDirection::Right => 1,
            EnterMoveDirection::Up => 2,
            EnterMoveDirection::Left => 3,
            EnterMoveDirection::Stay => 4,
        };

        v_flex()
            .id("ordia-settings-screen")
            .flex_1()
            .size_full()
            .overflow_y_scrollbar()
            .p(tokens.spacing.xl)
            .gap(tokens.spacing.lg)
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(
                        v_flex()
                            .gap(tokens.spacing.xxs)
                            .child(
                                div()
                                    .font_semibold()
                                    .text_size(tokens.typography.lg)
                                    .child(ordia_i18n::t!("settings.title")),
                            )
                            .child(
                                div()
                                    .text_size(tokens.typography.sm)
                                    .text_color(cx.theme().muted_foreground)
                                    .child(ordia_i18n::t!("settings.subtitle")),
                            ),
                    )
                    .when(!self.tabs.is_empty(), |row| {
                        row.child(
                            Button::new("settings-back-to-sheet")
                                .ghost()
                                .small()
                                .icon(Icon::new(OrdiaIcon::Spreadsheet))
                                .label(ordia_i18n::t!("settings.back_to_sheet"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.show_settings = false;
                                    this.show_home = false;
                                    cx.notify();
                                })),
                        )
                    }),
            )
            // 1. General
            .child(Self::settings_card(
                ordia_i18n::t!("settings.sec_general"),
                &tokens,
                cx,
                vec![
                    Self::settings_row(
                        ordia_i18n::t!("settings.reopen_workbooks"),
                        Button::new("settings-toggle-reopen")
                            .ghost()
                            .small()
                            .label(if self.settings.general.reopen_last_workbooks {
                                ordia_i18n::t!("settings.enabled")
                            } else {
                                ordia_i18n::t!("settings.disabled")
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.settings.general.reopen_last_workbooks =
                                    !this.settings.general.reopen_last_workbooks;
                                this.persist_settings();
                                cx.notify();
                            }))
                            .into_any_element(),
                    ),
                    Self::settings_row(
                        ordia_i18n::t!("settings.max_recent"),
                        div()
                            .font_family(tokens.typography.ui_mono.clone())
                            .text_size(tokens.typography.sm)
                            .child(SharedString::from(
                                self.settings.general.max_recent_files.to_string(),
                            ))
                            .into_any_element(),
                    ),
                ],
            ))
            // 2. Appearance
            .child(Self::settings_card(
                ordia_i18n::t!("settings.sec_appearance"),
                &tokens,
                cx,
                vec![Self::settings_row(
                    ordia_i18n::t!("settings.theme_mode"),
                    SegmentedControl::new("settings-appearance")
                        .option(SegmentOption::label(ordia_i18n::t!(
                            "settings.theme_system"
                        )))
                        .option(SegmentOption::icon_and_label(
                            OrdiaIcon::LightMode,
                            ordia_i18n::t!("shell.theme_light"),
                        ))
                        .option(SegmentOption::icon_and_label(
                            OrdiaIcon::DarkMode,
                            ordia_i18n::t!("shell.theme_dark"),
                        ))
                        .selected_index(appearance_ix)
                        .on_select({
                            let entity = entity.clone();
                            move |ix, window, cx| {
                                let mode = match ix {
                                    1 => AppearanceMode::Light,
                                    2 => AppearanceMode::Dark,
                                    _ => AppearanceMode::System,
                                };
                                entity.update(cx, |this, cx| {
                                    this.set_appearance_mode(mode, window, cx);
                                });
                            }
                        })
                        .into_any_element(),
                )],
            ))
            // 3. Language
            .child(Self::settings_card(
                ordia_i18n::t!("settings.sec_language"),
                &tokens,
                cx,
                vec![Self::settings_row(
                    ordia_i18n::t!("settings.ui_language"),
                    SegmentedControl::new("settings-language")
                        .option(SegmentOption::label(ordia_i18n::t!("settings.lang_system")))
                        .option(SegmentOption::label(ordia_i18n::t!("settings.lang_zh_cn")))
                        .option(SegmentOption::label(ordia_i18n::t!("settings.lang_en")))
                        .selected_index(lang_ix)
                        .on_select({
                            let entity = entity.clone();
                            move |ix, _, cx| {
                                let lang = match ix {
                                    1 => LanguageSetting::ZhCn,
                                    2 => LanguageSetting::En,
                                    _ => LanguageSetting::System,
                                };
                                entity.update(cx, |this, cx| {
                                    this.set_language_setting(lang, cx);
                                });
                            }
                        })
                        .into_any_element(),
                )],
            ))
            // 4. Editing
            .child(Self::settings_card(
                ordia_i18n::t!("settings.sec_editing"),
                &tokens,
                cx,
                vec![Self::settings_row(
                    ordia_i18n::t!("settings.enter_direction"),
                    SegmentedControl::new("settings-enter-direction")
                        .option(SegmentOption::label(ordia_i18n::t!("settings.dir_down")))
                        .option(SegmentOption::label(ordia_i18n::t!("settings.dir_right")))
                        .option(SegmentOption::label(ordia_i18n::t!("settings.dir_up")))
                        .option(SegmentOption::label(ordia_i18n::t!("settings.dir_left")))
                        .option(SegmentOption::label(ordia_i18n::t!("settings.dir_stay")))
                        .selected_index(enter_dir_ix)
                        .on_select({
                            let entity = entity.clone();
                            move |ix, _, cx| {
                                let dir = match ix {
                                    1 => EnterMoveDirection::Right,
                                    2 => EnterMoveDirection::Up,
                                    3 => EnterMoveDirection::Left,
                                    4 => EnterMoveDirection::Stay,
                                    _ => EnterMoveDirection::Down,
                                };
                                entity.update(cx, |this, cx| {
                                    this.set_enter_move_direction(dir, cx);
                                });
                            }
                        })
                        .into_any_element(),
                )],
            ))
            // 5. Advanced
            .child(Self::settings_card(
                ordia_i18n::t!("settings.sec_advanced"),
                &tokens,
                cx,
                vec![
                    Self::settings_row(
                        ordia_i18n::t!("settings.crash_recovery"),
                        Button::new("settings-toggle-recovery")
                            .ghost()
                            .small()
                            .label(if self.settings.advanced.enable_crash_recovery {
                                ordia_i18n::t!("settings.enabled")
                            } else {
                                ordia_i18n::t!("settings.disabled")
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.settings.advanced.enable_crash_recovery =
                                    !this.settings.advanced.enable_crash_recovery;
                                this.persist_settings();
                                cx.notify();
                            }))
                            .into_any_element(),
                    ),
                    Self::settings_row(
                        ordia_i18n::t!("settings.autosave_interval"),
                        div()
                            .font_family(tokens.typography.ui_mono.clone())
                            .text_size(tokens.typography.sm)
                            .child(SharedString::from(format!(
                                "{}s",
                                self.settings.advanced.autosave_interval_secs
                            )))
                            .into_any_element(),
                    ),
                ],
            ))
    }

    fn settings_card(
        heading: &'static str,
        tokens: &OrdiaTokens,
        cx: &Context<Self>,
        rows: Vec<gpui_kit::AnyElement>,
    ) -> impl IntoElement {
        v_flex()
            .max_w(px(760.0))
            .w_full()
            .p(tokens.spacing.lg)
            .gap(tokens.spacing.md)
            .rounded(tokens.radius.lg)
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .font_semibold()
                    .text_size(tokens.typography.md)
                    .child(heading),
            )
            .children(rows)
    }

    fn settings_row(label: &'static str, control: gpui_kit::AnyElement) -> gpui_kit::AnyElement {
        h_flex()
            .w_full()
            .justify_between()
            .items_center()
            .gap_4()
            .child(div().text_sm().child(label))
            .child(control)
            .into_any_element()
    }
}
