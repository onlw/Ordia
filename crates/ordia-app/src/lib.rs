//! Application shell, window management, commands, settings, and platform integration for Ordia.

/// Platform-specific native integrations (`macos`, `windows`).
pub mod platform;

use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, ThemeMode, TitleBar,
    button::{Button, ButtonVariants as _},
    h_flex,
    sidebar::{
        Sidebar, SidebarCollapsible, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu,
        SidebarMenuItem, SidebarToggleButton,
    },
    tab::{Tab, TabBar},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyBinding, Menu, MenuItem, ParentElement as _, Render, SharedString, Styled as _,
    Window, WindowBounds, WindowOptions, div, px, size,
};
use ordia_sheet_model::SheetDocument;
use ordia_sheet_view::{SheetWorkspaceView, register_sheet_keybindings};
use std::path::Path;

/// Keyboard and menu command actions for the application shell.
#[allow(missing_docs)]
pub mod app_actions {
    gpui_kit::actions!(
        ordia_app,
        [Quit, ToggleTheme, ToggleSidebar, NewWorkbookTab]
    );
}
pub use app_actions::{NewWorkbookTab, Quit, ToggleSidebar, ToggleTheme};

/// Returns the application display name.
pub const fn app_name() -> &'static str {
    "Ordia"
}

/// Identifies sidebar navigation sections in the application shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarSection {
    /// Recent workbooks and quick-open list.
    Recent,
    /// Built-in spreadsheet templates.
    Templates,
    /// Application preferences.
    Settings,
}

/// Metadata for an open workbook tab in the shell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkbookTab {
    id: u64,
    title: SharedString,
    summary: SharedString,
}

impl WorkbookTab {
    /// Creates a workbook tab descriptor with a stable numeric identifier.
    pub fn new(id: u64, title: impl Into<SharedString>, summary: impl Into<SharedString>) -> Self {
        Self {
            id,
            title: title.into(),
            summary: summary.into(),
        }
    }

    /// Stable numeric identifier for `ElementId` keying.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Display title shown in the tab bar.
    pub fn title(&self) -> &SharedString {
        &self.title
    }

    /// Description rendered in the workspace summary area.
    pub fn summary(&self) -> &SharedString {
        &self.summary
    }
}

/// Registers global keyboard shortcuts and native application menus.
///
/// Key bindings are registered before `set_menus` so shortcuts appear in native menus.
pub fn register_bindings_and_menus(cx: &mut App) {
    #[cfg(target_os = "macos")]
    let bindings = [
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-b", ToggleSidebar, None),
        KeyBinding::new("cmd-t", NewWorkbookTab, None),
        KeyBinding::new("cmd-shift-d", ToggleTheme, None),
    ];

    #[cfg(not(target_os = "macos"))]
    let bindings = [
        KeyBinding::new("ctrl-q", Quit, None),
        KeyBinding::new("ctrl-b", ToggleSidebar, None),
        KeyBinding::new("ctrl-t", NewWorkbookTab, None),
        KeyBinding::new("ctrl-shift-d", ToggleTheme, None),
    ];

    cx.bind_keys(bindings);
    register_sheet_keybindings(cx);

    cx.on_action(|_: &Quit, cx| {
        cx.quit();
    });

    cx.set_menus(vec![
        Menu {
            name: "Ordia".into(),
            items: vec![MenuItem::action("Quit Ordia", Quit)],
            disabled: false,
        },
        Menu {
            name: "File".into(),
            items: vec![MenuItem::action("New Workbook", NewWorkbookTab)],
            disabled: false,
        },
        Menu {
            name: "View".into(),
            items: vec![
                MenuItem::action("Toggle Sidebar", ToggleSidebar),
                MenuItem::action("Switch Appearance", ToggleTheme),
            ],
            disabled: false,
        },
    ]);
}

/// Opens the interactive Ordia Design System Gallery window (`--gallery`).
pub fn open_design_gallery(
    cx: &mut App,
) -> anyhow::Result<(
    gpui_kit::AnyWindowHandle,
    Entity<ordia_ui::DesignGalleryView>,
)> {
    let options = default_window_options(cx);
    let opened = gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| ordia_ui::DesignGalleryView::new(window, cx))
    })?;
    Ok(opened)
}

/// Builds default window options using GPUI Kit's custom title bar chrome.
///
/// Uses explicit initial window bounds (`1200x780`) and minimum window dimensions (`720x480`),
/// which represent physical OS window constraints rather than internal layout spacing.
pub fn default_window_options(cx: &App) -> WindowOptions {
    let mut options = TitleBar::window_options();
    let bounds = Bounds::centered(None, size(px(1200.), px(780.)), cx);
    options.window_bounds = Some(WindowBounds::Windowed(bounds));
    options.window_min_size = Some(size(px(720.), px(480.)));
    options
}

/// Main window view for the Ordia desktop shell.
///
/// Owns sidebar collapse state, navigation section, open workbook tabs, their embedded
/// [`SheetWorkspaceView`] entities, and keyboard focus.
pub struct MainWindow {
    focus_handle: FocusHandle,
    sidebar_collapsed: bool,
    active_section: SidebarSection,
    tabs: Vec<WorkbookTab>,
    sheet_views: Vec<Entity<SheetWorkspaceView>>,
    active_tab_ix: usize,
    next_tab_id: u64,
}

impl MainWindow {
    /// Constructs a new main window view with starter workbook tabs.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        ordia_ui::apply_ordia_theme(cx.theme().mode, Some(window), cx);
        let focus_handle = cx.focus_handle().tab_stop(true);
        let tabs = vec![
            WorkbookTab::new(
                1,
                "Budget_2026.xlsx",
                "12 sheets · Financial forecast & monthly variance",
            ),
            WorkbookTab::new(
                2,
                "Sales_Pipeline.xlsx",
                "4 sheets · Regional quarterly revenue breakdown",
            ),
            WorkbookTab::new(
                3,
                "Inventory.csv",
                "100,000 rows · Warehouse SKU stock snapshot",
            ),
        ];

        let demo_doc = SheetDocument::new_demo("Budget_2026")
            .or_else(|_| SheetDocument::new_empty("Budget_2026"))
            .expect("initialize starter demo workbook");
        let sales_doc =
            SheetDocument::new_empty("Sales_Pipeline").expect("initialize sales workbook");
        let inv_doc = SheetDocument::new_empty("Inventory").expect("initialize inventory workbook");

        let cb = platform::system_clipboard_bridge();
        let sheet_views = vec![
            cx.new(|cx| {
                let mut v = SheetWorkspaceView::new(demo_doc, window, cx);
                v.set_clipboard_bridge(cb.clone());
                v
            }),
            cx.new(|cx| {
                let mut v = SheetWorkspaceView::new(sales_doc, window, cx);
                v.set_clipboard_bridge(cb.clone());
                v
            }),
            cx.new(|cx| {
                let mut v = SheetWorkspaceView::new(inv_doc, window, cx);
                v.set_clipboard_bridge(cb.clone());
                v
            }),
        ];

        Self {
            focus_handle,
            sidebar_collapsed: false,
            active_section: SidebarSection::Recent,
            tabs,
            sheet_views,
            active_tab_ix: 0,
            next_tab_id: 4,
        }
    }

    /// Constructs a main window pre-loaded with a specific [`SheetDocument`].
    pub fn new_with_document(
        doc: SheetDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        ordia_ui::apply_ordia_theme(cx.theme().mode, Some(window), cx);
        let focus_handle = cx.focus_handle().tab_stop(true);
        let title = doc.title().to_string();
        let sheet_count = doc.sheet_count();
        let summary = format!("{sheet_count} sheet(s) · Local workbook");
        let tabs = vec![WorkbookTab::new(1, title, summary)];
        let cb = platform::system_clipboard_bridge();
        let sheet_views = vec![cx.new(|cx| {
            let mut v = SheetWorkspaceView::new(doc, window, cx);
            v.set_clipboard_bridge(cb);
            v
        })];

        Self {
            focus_handle,
            sidebar_collapsed: true,
            active_section: SidebarSection::Recent,
            tabs,
            sheet_views,
            active_tab_ix: 0,
            next_tab_id: 2,
        }
    }

    /// Creates and opens a main window wrapped in GPUI Kit's `Root`.
    pub fn open(cx: &mut App) -> anyhow::Result<(gpui_kit::AnyWindowHandle, Entity<Self>)> {
        Self::open_with_path(None, cx)
    }

    /// Creates and opens a main window, optionally loading an `.xlsx` file from `path`.
    pub fn open_with_path(
        path: Option<&Path>,
        cx: &mut App,
    ) -> anyhow::Result<(gpui_kit::AnyWindowHandle, Entity<Self>)> {
        let preloaded = match path {
            Some(p) => Some(SheetDocument::open_xlsx(p)?),
            None => None,
        };
        let options = default_window_options(cx);
        let opened = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| match preloaded {
                Some(doc) => Self::new_with_document(doc, window, cx),
                None => Self::new(window, cx),
            })
        })?;
        Ok(opened)
    }

    /// Returns the embedded [`SheetWorkspaceView`] for the currently active tab, if any.
    pub fn active_sheet_view(&self) -> Option<&Entity<SheetWorkspaceView>> {
        self.sheet_views.get(self.active_tab_ix)
    }

    /// Returns whether the navigation sidebar is collapsed.
    pub fn is_sidebar_collapsed(&self) -> bool {
        self.sidebar_collapsed
    }

    /// Returns the currently active sidebar section.
    pub fn active_section(&self) -> SidebarSection {
        self.active_section
    }

    /// Returns the open workbook tabs.
    pub fn tabs(&self) -> &[WorkbookTab] {
        &self.tabs
    }

    /// Returns the zero-based index of the active workbook tab.
    pub fn active_tab_ix(&self) -> usize {
        self.active_tab_ix
    }

    /// Returns the currently selected workbook tab, if any.
    pub fn active_tab(&self) -> Option<&WorkbookTab> {
        self.tabs.get(self.active_tab_ix)
    }

    /// Toggles between light and dark theme modes across the application.
    pub fn toggle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next_mode = if cx.theme().mode.is_dark() {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        };
        ordia_ui::apply_ordia_theme(next_mode, Some(window), cx);
        cx.notify();
    }

    /// Toggles the left navigation sidebar between expanded and icon-collapsed states.
    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_collapsed = !self.sidebar_collapsed;
        cx.notify();
    }

    /// Appends a new blank workbook tab and makes it active.
    pub fn add_workbook_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let title = format!("Book{id}.xlsx");
        let doc = SheetDocument::new_empty(&title).expect("initialize empty workbook tab");
        self.tabs
            .push(WorkbookTab::new(id, title, "1 sheet · Empty workbook"));
        let cb = platform::system_clipboard_bridge();
        self.sheet_views.push(cx.new(|cx| {
            let mut v = SheetWorkspaceView::new(doc, window, cx);
            v.set_clipboard_bridge(cb);
            v
        }));
        self.active_tab_ix = self.tabs.len().saturating_sub(1);
        cx.notify();
    }

    /// Selects a workbook tab by index if within bounds.
    pub fn select_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.tabs.len() && self.active_tab_ix != ix {
            self.active_tab_ix = ix;
            cx.notify();
        }
    }

    fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let is_dark = cx.theme().mode.is_dark();
        let theme_icon = if is_dark {
            IconName::Sun
        } else {
            IconName::Moon
        };
        let theme_label = if is_dark { "Light" } else { "Dark" };

        let tab_items: Vec<Tab> = self
            .tabs
            .iter()
            .map(|tab| Tab::new().label(tab.title().clone()))
            .collect();

        TitleBar::new().child(
            h_flex()
                .w_full()
                .pr_2()
                .gap_2()
                .justify_between()
                .child(
                    h_flex()
                        .gap_2()
                        .min_w_0()
                        .flex_1()
                        .child(
                            SidebarToggleButton::new()
                                .collapsed(self.sidebar_collapsed)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.toggle_sidebar(cx);
                                })),
                        )
                        .child(
                            div()
                                .text_sm()
                                .font_semibold()
                                .text_color(cx.theme().foreground)
                                .child(app_name()),
                        )
                        .child(
                            div().flex_1().min_w_0().child(
                                TabBar::new("workbook-tabs")
                                    .folder()
                                    .small()
                                    .menu(true)
                                    .selected_index(self.active_tab_ix)
                                    .children(tab_items)
                                    .on_click(cx.listener(|this, ix, _, cx| {
                                        this.select_tab(*ix, cx);
                                    }))
                                    .suffix(
                                        Button::new("new-workbook-tab")
                                            .ghost()
                                            .xsmall()
                                            .icon(IconName::Plus)
                                            .tooltip("New workbook")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.add_workbook_tab(window, cx);
                                            })),
                                    ),
                            ),
                        ),
                )
                .child(
                    h_flex().gap_1().child(
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

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
                                cx.notify();
                            })),
                        SidebarMenuItem::new("Templates")
                            .icon(IconName::Folder)
                            .active(active == SidebarSection::Templates)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.active_section = SidebarSection::Templates;
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

    fn render_workspace_content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active_sheet = self.active_sheet_view().cloned();

        v_flex()
            .h_full()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .bg(cx.theme().background)
            .when_some(active_sheet, |el, sheet_view| el.child(sheet_view))
    }
}

impl Focusable for MainWindow {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for MainWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("ordia-main-window")
            .key_context("OrdiaMainWindow")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &ToggleTheme, window, cx| {
                this.toggle_theme(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.toggle_sidebar(cx);
            }))
            .on_action(cx.listener(|this, _: &NewWorkbookTab, window, cx| {
                this.add_workbook_tab(window, cx);
            }))
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_title_bar(cx))
            .child(
                h_flex()
                    .items_stretch()
                    .flex_1()
                    .min_h_0()
                    .size_full()
                    .child(self.render_sidebar(cx))
                    .child(self.render_workspace_content(cx)),
            )
    }
}
