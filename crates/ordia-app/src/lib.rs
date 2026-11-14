//! Application shell, window management, commands, settings, and platform integration for Ordia.

/// Unified command registry, shortcut conflict detection, and Command Palette items (`M1-03`).
pub mod commands;
/// Non-blocking background document loader, progress banner, and friendly error display (`M1-06`).
pub mod document_loader;
mod home_view;
/// GB2312 Level-1 Hanzi Pinyin-initials and fuzzy subsequence matcher (`M1-03`).
pub mod pinyin;
/// Platform-specific native integrations (`macos`, `windows`).
pub mod platform;
mod settings_view;
mod shell_chrome;
/// Domain data types for the application shell (`WorkbookTab`, `RecentFileEntry`, `SidebarSection`).
pub mod shell_types;
mod title_tabs;

use std::path::{Path, PathBuf};

use gpui_kit::base::GlobalState;
#[cfg(not(target_os = "macos"))]
use gpui_kit::component::menu::AppMenuBar;
use gpui_kit::component::{
    ActiveTheme as _, ThemeMode, TitleBar, color_picker::ColorPickerState, command::CommandState,
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Bounds, Context, Entity, ExternalPaths, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, Menu, OwnedMenu, ParentElement as _, Render,
    SharedString, Styled as _, Window, WindowBounds, WindowOptions, point, px, size,
};
use ordia_core::{
    AppSettings, AppStateSnapshot, AppearanceMode, EnterMoveDirection, LanguageSetting,
    PersistedRecentFile, PersistedWindowBounds, SettingsStore,
};
use ordia_i18n::LanguagePreference;
use ordia_sheet_model::SheetDocument;
use ordia_sheet_view::{SheetWorkspaceView, register_sheet_keybindings};
use ordia_ui::{
    OrdiaTokens, ToolbarSelectState, new_toolbar_color_picker_state, new_toolbar_select_state,
};

pub use commands::{
    CommandCategory, CommandRegistry, CommandSpec, ShortcutConflict, normalize_shortcut,
};
pub use document_loader::{
    ASYNC_LOAD_THRESHOLD_BYTES, LoadingBannerState, OpenErrorBanner, format_friendly_open_error,
};
pub use shell_types::{RecentFileEntry, SidebarSection, WorkbookTab};

/// Keyboard and menu command actions for the application shell.
#[allow(missing_docs)]
pub mod app_actions {
    gpui_kit::actions!(
        ordia_app,
        [
            Quit,
            OpenSettings,
            ToggleTheme,
            ToggleSidebar,
            ToggleInspector,
            NewWorkbookTab,
            CloseActiveTab,
            NewWindow,
            ShowHome,
            OpenCommandPalette,
            SaveWorkbook
        ]
    );
}
pub use app_actions::{
    CloseActiveTab, NewWindow, NewWorkbookTab, OpenCommandPalette, OpenSettings, Quit,
    SaveWorkbook, ShowHome, ToggleInspector, ToggleSidebar, ToggleTheme,
};

/// Returns the application display name.
pub const fn app_name() -> &'static str {
    "Ordia"
}

/// Registers global keyboard shortcuts, macOS native menu bar, and Windows/Linux `AppMenuBar` menus
/// directly from [`CommandRegistry::default_ordia`].
pub fn register_bindings_and_menus(cx: &mut App) {
    let registry = CommandRegistry::default_ordia();
    debug_assert!(
        registry.detect_shortcut_conflicts().is_empty(),
        "CommandRegistry contains shortcut conflicts"
    );

    cx.bind_keys(registry.build_keybindings(cx));
    register_sheet_keybindings(cx);

    cx.on_action(|_: &Quit, cx| {
        cx.quit();
    });
    cx.on_action(|_: &NewWindow, cx| {
        let _ = MainWindow::open(cx);
    });

    let owned_menus: Vec<OwnedMenu> = registry
        .build_menus()
        .into_iter()
        .map(Menu::owned)
        .collect();
    GlobalState::init(cx);
    GlobalState::global_mut(cx).set_app_menus(owned_menus);
    cx.set_menus(registry.build_menus());
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
pub fn default_window_options(cx: &App) -> WindowOptions {
    window_options_for_store(&SettingsStore::default_store(), cx)
}

/// Builds window options restoring persisted geometry from `store` if available.
pub fn window_options_for_store(store: &SettingsStore, cx: &App) -> WindowOptions {
    let mut options = TitleBar::window_options();
    let default_bounds = Bounds::centered(None, size(px(1200.), px(780.)), cx);
    options.window_bounds = match store.load_state().ok().and_then(|s| s.window_bounds) {
        Some(saved) => {
            let restored = Bounds::new(
                point(px(saved.x), px(saved.y)),
                size(px(saved.width), px(saved.height)),
            );
            Some(if saved.maximized {
                WindowBounds::Maximized(restored)
            } else {
                WindowBounds::Windowed(restored)
            })
        }
        None => Some(WindowBounds::Windowed(default_bounds)),
    };
    options.window_min_size = Some(size(px(720.), px(480.)));
    options
}

/// Main window view for the Ordia desktop shell (`PLAN.md` §5.1).
pub struct MainWindow {
    focus_handle: FocusHandle,
    #[cfg(not(target_os = "macos"))]
    app_menu_bar: Entity<AppMenuBar>,
    pub(crate) settings_store: SettingsStore,
    pub(crate) settings: AppSettings,
    pub(crate) window_bounds_override: Option<PersistedWindowBounds>,
    pub(crate) command_registry: CommandRegistry,
    pub(crate) palette_open: bool,
    pub(crate) palette_query: SharedString,
    pub(crate) palette_state: Entity<CommandState>,
    pub(crate) sidebar_collapsed: bool,
    pub(crate) inspector_open: bool,
    pub(crate) show_home: bool,
    pub(crate) show_settings: bool,
    pub(crate) active_section: SidebarSection,
    pub(crate) tabs: Vec<WorkbookTab>,
    pub(crate) sheet_views: Vec<Entity<SheetWorkspaceView>>,
    pub(crate) active_tab_ix: usize,
    next_tab_id: u64,
    saved_count: u64,
    pub(crate) recent_files: Vec<RecentFileEntry>,
    pub(crate) loading_banner: Option<LoadingBannerState>,
    pub(crate) open_error_banner: Option<OpenErrorBanner>,
    pub(crate) load_ticket: u64,
    pub(crate) active_load_task: Option<gpui_kit::Task<()>>,
    pub(crate) toolbar_font_select: Entity<ToolbarSelectState>,
    pub(crate) toolbar_size_select: Entity<ToolbarSelectState>,
    pub(crate) toolbar_fill_picker: Entity<ColorPickerState>,
    pub(crate) toolbar_text_picker: Entity<ColorPickerState>,
    pub(crate) toolbar_align_ix: usize,
}

impl MainWindow {
    fn build_shell_controls(
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (
        Entity<ToolbarSelectState>,
        Entity<ToolbarSelectState>,
        Entity<ColorPickerState>,
        Entity<ColorPickerState>,
    ) {
        let light = OrdiaTokens::light();
        let font_sel = new_toolbar_select_state(
            ["Carlito", "PingFang SC", "Inter", "JetBrains Mono"],
            Some(0),
            window,
            cx,
        );
        let size_sel = new_toolbar_select_state(
            ["9", "10", "11", "12", "14", "18", "24"],
            Some(2),
            window,
            cx,
        );
        let fill_picker =
            new_toolbar_color_picker_state(Some(light.colors.accent_subtle), window, cx);
        let text_picker = new_toolbar_color_picker_state(Some(light.colors.primary), window, cx);
        (font_sel, size_sel, fill_picker, text_picker)
    }

    fn default_recent_entries() -> Vec<RecentFileEntry> {
        vec![
            RecentFileEntry::new(
                "m0_budget_sample.xlsx",
                PathBuf::from("corpus/public/m0_budget_sample.xlsx"),
                ordia_i18n::t!("shell.recent_sample_workbook"),
            ),
            RecentFileEntry::new(
                "m0_pivot_table.xlsx",
                PathBuf::from("corpus/public/m0_pivot_table.xlsx"),
                ordia_i18n::t!("shell.recent_pivot_sample"),
            ),
            RecentFileEntry::new(
                "m0_complex_chart.xlsx",
                PathBuf::from("corpus/public/m0_complex_chart.xlsx"),
                ordia_i18n::t!("shell.recent_chart_sample"),
            ),
        ]
    }

    fn apply_loaded_preferences(
        settings: &AppSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let lang_pref = match settings.language {
            LanguageSetting::System => LanguagePreference::System,
            LanguageSetting::ZhCn => LanguagePreference::ZhCn,
            LanguageSetting::En => LanguagePreference::En,
        };
        ordia_i18n::set_language_preference(lang_pref);

        let theme_mode = match settings.appearance.theme {
            AppearanceMode::Light => ThemeMode::Light,
            AppearanceMode::Dark => ThemeMode::Dark,
            AppearanceMode::System => cx.theme().mode,
        };
        ordia_ui::apply_ordia_theme(theme_mode, Some(window), cx);
    }

    /// Constructs a new main window view isolated from host disk state (ideal for generic UI tests).
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        static EPHEMERAL_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let seq = EPHEMERAL_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let scratch_dir =
            std::env::temp_dir().join(format!("ordia-ephemeral-{}-{seq}", std::process::id()));
        Self::new_with_store(SettingsStore::with_dir(scratch_dir), window, cx)
    }

    /// Constructs a new main window view loading and persisting settings/state in `settings_store`.
    pub fn new_with_store(
        settings_store: SettingsStore,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings = settings_store.load_settings().unwrap_or_default();
        let saved_state = settings_store.load_state().unwrap_or_default();
        Self::apply_loaded_preferences(&settings, window, cx);

        let focus_handle = cx.focus_handle().tab_stop(true);
        let (toolbar_font_select, toolbar_size_select, toolbar_fill_picker, toolbar_text_picker) =
            Self::build_shell_controls(window, cx);
        #[cfg(not(target_os = "macos"))]
        let app_menu_bar = AppMenuBar::new(cx);

        let tabs = vec![
            WorkbookTab::new(
                1,
                "Budget_2026.xlsx",
                ordia_i18n::t!("shell.summary_demo_budget"),
            ),
            WorkbookTab::new(
                2,
                "Sales_Pipeline.xlsx",
                ordia_i18n::t!("shell.summary_demo_sales"),
            ),
            WorkbookTab::new(
                3,
                "Inventory.csv",
                ordia_i18n::t!("shell.summary_demo_inventory"),
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
                v.set_clipboard_bridge(cb);
                v
            }),
        ];

        let recent_files = if saved_state.recent_files.is_empty() {
            Self::default_recent_entries()
        } else {
            saved_state
                .recent_files
                .into_iter()
                .map(|r| RecentFileEntry::new(r.title, r.path, r.last_opened_label))
                .collect()
        };

        let palette_state = cx.new(|cx| CommandState::new(window, cx));

        Self {
            focus_handle,
            #[cfg(not(target_os = "macos"))]
            app_menu_bar,
            settings_store,
            settings,
            window_bounds_override: saved_state.window_bounds,
            command_registry: CommandRegistry::default_ordia(),
            palette_open: false,
            palette_query: SharedString::default(),
            palette_state,
            sidebar_collapsed: saved_state.sidebar_collapsed,
            inspector_open: saved_state.inspector_open,
            show_home: false,
            show_settings: false,
            active_section: SidebarSection::Recent,
            tabs,
            sheet_views,
            active_tab_ix: 0,
            next_tab_id: 4,
            saved_count: 0,
            recent_files,
            loading_banner: None,
            open_error_banner: None,
            load_ticket: 0,
            active_load_task: None,
            toolbar_font_select,
            toolbar_size_select,
            toolbar_fill_picker,
            toolbar_text_picker,
            toolbar_align_ix: 0,
        }
    }

    /// Constructs a main window pre-loaded with a specific [`SheetDocument`].
    pub fn new_with_document(
        doc: SheetDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings_store = SettingsStore::default_store();
        let settings = settings_store.load_settings().unwrap_or_default();
        let saved_state = settings_store.load_state().unwrap_or_default();
        Self::apply_loaded_preferences(&settings, window, cx);

        let focus_handle = cx.focus_handle().tab_stop(true);
        let (toolbar_font_select, toolbar_size_select, toolbar_fill_picker, toolbar_text_picker) =
            Self::build_shell_controls(window, cx);
        #[cfg(not(target_os = "macos"))]
        let app_menu_bar = AppMenuBar::new(cx);
        let palette_state = cx.new(|cx| CommandState::new(window, cx));

        let title = doc.title().to_string();
        let sheet_count = doc.sheet_count().to_string();
        let summary = ordia_i18n::t!("shell.summary_local_sheets", "count" => sheet_count);
        let tabs = vec![WorkbookTab::new(1, title, summary)];
        let cb = platform::system_clipboard_bridge();
        let sheet_views = vec![cx.new(|cx| {
            let mut v = SheetWorkspaceView::new(doc, window, cx);
            v.set_clipboard_bridge(cb);
            v
        })];

        Self {
            focus_handle,
            #[cfg(not(target_os = "macos"))]
            app_menu_bar,
            settings_store,
            settings,
            window_bounds_override: saved_state.window_bounds,
            command_registry: CommandRegistry::default_ordia(),
            palette_open: false,
            palette_query: SharedString::default(),
            palette_state,
            sidebar_collapsed: true,
            inspector_open: saved_state.inspector_open,
            show_home: false,
            show_settings: false,
            active_section: SidebarSection::Recent,
            tabs,
            sheet_views,
            active_tab_ix: 0,
            next_tab_id: 2,
            saved_count: 0,
            recent_files: Self::default_recent_entries(),
            loading_banner: None,
            open_error_banner: None,
            load_ticket: 0,
            active_load_task: None,
            toolbar_font_select,
            toolbar_size_select,
            toolbar_fill_picker,
            toolbar_text_picker,
            toolbar_align_ix: 0,
        }
    }

    /// Creates and opens a main window wrapped in GPUI Kit's `Root`.
    pub fn open(cx: &mut App) -> anyhow::Result<(gpui_kit::AnyWindowHandle, Entity<Self>)> {
        Self::open_with_path(None, cx)
    }

    /// Creates and opens a main window backed by an explicit [`SettingsStore`].
    pub fn open_with_store(
        store: SettingsStore,
        cx: &mut App,
    ) -> anyhow::Result<(gpui_kit::AnyWindowHandle, Entity<Self>)> {
        let options = window_options_for_store(&store, cx);
        let opened = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| Self::new_with_store(store, window, cx))
        })?;
        Ok(opened)
    }

    /// Creates and opens a main window, optionally loading a spreadsheet file from `path`.
    ///
    /// Large files (`>= 2 MiB`) load asynchronously on GPUI's background executor with a live progress banner,
    /// while damaged or unsupported files surface a friendly localized error banner without crashing.
    pub fn open_with_path(
        path: Option<&Path>,
        cx: &mut App,
    ) -> anyhow::Result<(gpui_kit::AnyWindowHandle, Entity<Self>)> {
        let store = SettingsStore::default_store();
        let target_path = path.map(Path::to_path_buf);
        let options = window_options_for_store(&store, cx);
        let opened = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| {
                let mut view = Self::new_with_store(store, window, cx);
                if let Some(p) = target_path {
                    view.tabs.clear();
                    view.sheet_views.clear();
                    view.active_tab_ix = 0;
                    view.show_home = true;
                    view.sidebar_collapsed = true;
                    let is_large = std::fs::metadata(&p)
                        .map(|m| m.len() >= ASYNC_LOAD_THRESHOLD_BYTES)
                        .unwrap_or(false);
                    if is_large {
                        view.open_file_async(p, window, cx);
                    } else {
                        view.open_file_sync(&p, window, cx);
                    }
                }
                view
            })
        })?;
        Ok(opened)
    }

    /// Returns the embedded [`SheetWorkspaceView`] for the currently active tab, if any.
    pub fn active_sheet_view(&self) -> Option<&Entity<SheetWorkspaceView>> {
        if self.show_home || self.show_settings {
            return None;
        }
        self.sheet_views.get(self.active_tab_ix)
    }

    /// Returns whether the navigation sidebar is collapsed.
    pub fn is_sidebar_collapsed(&self) -> bool {
        self.sidebar_collapsed
    }

    /// Returns whether the right contextual property inspector panel is expanded.
    pub fn is_inspector_open(&self) -> bool {
        self.inspector_open
    }

    /// Returns whether the Home screen is currently active (either explicitly or when all tabs are closed).
    pub fn is_showing_home(&self) -> bool {
        !self.show_settings && (self.show_home || self.tabs.is_empty())
    }

    /// Returns whether the Settings screen is currently active.
    pub fn is_showing_settings(&self) -> bool {
        self.show_settings
    }

    /// Returns the window's active [`AppSettings`].
    pub fn settings(&self) -> &AppSettings {
        &self.settings
    }

    /// Returns the last persisted or restored [`PersistedWindowBounds`], if any.
    pub fn persisted_window_bounds(&self) -> Option<PersistedWindowBounds> {
        self.window_bounds_override
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
        if self.is_showing_home() {
            return None;
        }
        self.tabs.get(self.active_tab_ix)
    }

    /// Returns the recent files list displayed on the Home screen.
    pub fn recent_files(&self) -> &[RecentFileEntry] {
        &self.recent_files
    }

    /// Returns a reference to the window's [`CommandRegistry`].
    pub fn command_registry(&self) -> &CommandRegistry {
        &self.command_registry
    }

    /// Returns whether the `Cmd/Ctrl+K` Command Palette overlay is open.
    pub fn is_command_palette_open(&self) -> bool {
        self.palette_open
    }

    /// Returns the number of times [`Self::save_active_workbook`] has succeeded in this session.
    pub fn saved_count(&self) -> u64 {
        self.saved_count
    }
}

impl MainWindow {
    /// Opens the floating `Cmd/Ctrl+K` Command Palette and focuses its search input.
    pub fn open_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette_open = true;
        self.palette_query = SharedString::default();
        self.palette_state.update(cx, |state, cx| {
            state.set_query("", window, cx);
        });
        let focus = self.palette_state.focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Closes the Command Palette overlay and restores focus to the main window.
    pub fn close_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette_open {
            self.palette_open = false;
            self.palette_query = SharedString::default();
            window.focus(&self.focus_handle, cx);
            cx.notify();
        }
    }

    /// Marks the active workbook tab as saved and increments the session save counter.
    pub fn save_active_workbook(&mut self, cx: &mut Context<Self>) {
        if self.is_showing_home() {
            return;
        }
        if let Some(tab) = self.tabs.get_mut(self.active_tab_ix) {
            tab.set_dirty(false);
            self.saved_count += 1;
            cx.notify();
        }
    }

    pub(crate) fn persist_settings(&self) {
        let _ = self.settings_store.save_settings(&self.settings);
    }

    pub(crate) fn persist_shell_state(&self) {
        let mut snap = AppStateSnapshot::default();
        snap.window_bounds = self.window_bounds_override;
        snap.sidebar_collapsed = self.sidebar_collapsed;
        snap.inspector_open = self.inspector_open;
        let max_len = self.settings.general.max_recent_files.max(1);
        snap.recent_files = self
            .recent_files
            .iter()
            .take(max_len)
            .map(|r| {
                PersistedRecentFile::new(
                    r.title().to_string(),
                    r.path().clone(),
                    r.last_opened_label().to_string(),
                )
            })
            .collect();
        let _ = self.settings_store.save_state(&snap);
    }

    /// Records explicit window geometry (`x`, `y`, `width`, `height`, `maximized`) and persists `state.toml`.
    pub fn record_window_bounds(&mut self, bounds: PersistedWindowBounds) {
        self.window_bounds_override = Some(bounds);
        self.persist_shell_state();
    }

    /// Snapshots the live GPUI `window.bounds()` and persists `state.toml`.
    pub fn snapshot_live_window_bounds(&mut self, window: &Window) {
        let b = window.bounds();
        let persisted = PersistedWindowBounds::new(
            f32::from(b.origin.x),
            f32::from(b.origin.y),
            f32::from(b.size.width),
            f32::from(b.size.height),
            window.is_maximized(),
        );
        self.record_window_bounds(persisted);
    }

    /// Opens the Settings view (`Cmd/Ctrl+,`).
    pub fn open_settings_screen(&mut self, cx: &mut Context<Self>) {
        self.active_section = SidebarSection::Settings;
        self.show_settings = true;
        cx.notify();
    }

    /// Updates and persists the [`AppearanceMode`] (`System`, `Light`, `Dark`).
    pub fn set_appearance_mode(
        &mut self,
        mode: AppearanceMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.appearance.theme = mode;
        let resolved = match mode {
            AppearanceMode::Light => ThemeMode::Light,
            AppearanceMode::Dark => ThemeMode::Dark,
            AppearanceMode::System => cx.theme().mode,
        };
        ordia_ui::apply_ordia_theme(resolved, Some(window), cx);
        self.persist_settings();
        cx.notify();
    }

    /// Updates and persists the [`LanguageSetting`] (`System`, `ZhCn`, `En`) immediately at runtime.
    pub fn set_language_setting(&mut self, lang: LanguageSetting, cx: &mut Context<Self>) {
        self.settings.language = lang;
        let pref = match lang {
            LanguageSetting::System => LanguagePreference::System,
            LanguageSetting::ZhCn => LanguagePreference::ZhCn,
            LanguageSetting::En => LanguagePreference::En,
        };
        ordia_i18n::set_language_preference(pref);
        self.persist_settings();
        cx.notify();
    }

    /// Updates and persists the [`EnterMoveDirection`] preference.
    pub fn set_enter_move_direction(&mut self, dir: EnterMoveDirection, cx: &mut Context<Self>) {
        self.settings.editing.enter_move_direction = dir;
        self.persist_settings();
        cx.notify();
    }

    /// Toggles between light and dark theme modes across the application.
    pub fn toggle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = if cx.theme().mode.is_dark() {
            AppearanceMode::Light
        } else {
            AppearanceMode::Dark
        };
        self.set_appearance_mode(next, window, cx);
    }

    /// Toggles the left navigation sidebar between expanded and icon-collapsed states.
    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_collapsed = !self.sidebar_collapsed;
        self.persist_shell_state();
        cx.notify();
    }

    /// Toggles the right contextual property inspector panel.
    pub fn toggle_inspector(&mut self, cx: &mut Context<Self>) {
        self.inspector_open = !self.inspector_open;
        self.persist_shell_state();
        cx.notify();
    }

    /// Navigates to the Home screen (`Recent` & `Templates`).
    pub fn show_home_screen(&mut self, cx: &mut Context<Self>) {
        self.show_settings = false;
        self.show_home = true;
        cx.notify();
    }

    /// Appends a new blank workbook tab and makes it active.
    pub fn add_workbook_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let title = format!("Book{id}.xlsx");
        let doc = SheetDocument::new_empty(&title).expect("initialize empty workbook tab");
        self.push_document_tab(
            WorkbookTab::new(id, title, ordia_i18n::t!("shell.summary_empty_workbook")),
            doc,
            window,
            cx,
        );
    }

    /// Opens a built-in starter template in a new tab.
    pub fn open_demo_template(
        &mut self,
        title: &str,
        summary: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let doc = SheetDocument::new_demo(title)
            .or_else(|_| SheetDocument::new_empty(title))
            .expect("initialize template workbook");
        self.push_document_tab(
            WorkbookTab::new(id, title.to_string(), summary.to_string()),
            doc,
            window,
            cx,
        );
    }

    /// Opens an `.xlsx` file from `path` if readable on disk, falling back to a demo workbook.
    pub fn open_recent_or_demo_entry(
        &mut self,
        title: &str,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(existing_ix) = self
            .tabs
            .iter()
            .position(|t| t.path().is_some_and(|p| p == path))
        {
            self.select_tab(existing_ix, cx);
            return;
        }
        let doc = SheetDocument::open_xlsx(path)
            .or_else(|_| SheetDocument::new_demo(title))
            .or_else(|_| SheetDocument::new_empty(title))
            .expect("open recent or demo workbook");
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let sheet_count = doc.sheet_count().to_string();
        let tab = WorkbookTab::new(
            id,
            title.to_string(),
            ordia_i18n::t!("shell.summary_local_sheets", "count" => sheet_count),
        )
        .with_path(path.to_path_buf());
        self.push_document_tab(tab, doc, window, cx);
    }

    /// Opens one or more dropped external files (`.xlsx`, `.xlsm`, `.csv`, `.tsv`) as workbook tabs.
    ///
    /// Large files (`>= 2 MiB`) dispatch onto GPUI's background executor so the UI never freezes;
    /// damaged or unsupported files surface a friendly localized error banner.
    pub fn open_dropped_paths(
        &mut self,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for path in paths {
            let is_large = std::fs::metadata(path)
                .map(|m| m.len() >= ASYNC_LOAD_THRESHOLD_BYTES)
                .unwrap_or(false);
            if is_large {
                self.open_file_async(path.clone(), window, cx);
            } else {
                self.open_file_sync(path, window, cx);
            }
        }
    }

    fn push_document_tab(
        &mut self,
        tab: WorkbookTab,
        doc: SheetDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cb = platform::system_clipboard_bridge();
        self.tabs.push(tab);
        self.sheet_views.push(cx.new(|cx| {
            let mut v = SheetWorkspaceView::new(doc, window, cx);
            v.set_clipboard_bridge(cb);
            v
        }));
        self.active_tab_ix = self.tabs.len().saturating_sub(1);
        self.show_home = false;
        self.show_settings = false;
        cx.notify();
    }

    /// Selects a workbook tab by index if within bounds and exits the Home view.
    pub fn select_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.tabs.len() {
            self.active_tab_ix = ix;
            self.show_home = false;
            self.show_settings = false;
            cx.notify();
        }
    }

    /// Closes the workbook tab at zero-based index `ix`. When the last tab closes, shows Home.
    pub fn close_tab(&mut self, ix: usize, _window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        let active_id = self.tabs.get(self.active_tab_ix).map(WorkbookTab::id);
        self.tabs.remove(ix);
        self.sheet_views.remove(ix);

        if self.tabs.is_empty() {
            self.active_tab_ix = 0;
            self.show_home = true;
        } else if let Some(prev_active) = active_id {
            if let Some(new_ix) = self.tabs.iter().position(|t| t.id() == prev_active) {
                self.active_tab_ix = new_ix;
            } else {
                self.active_tab_ix = ix.min(self.tabs.len() - 1);
            }
        }
        cx.notify();
    }

    /// Closes a workbook tab by its stable [`WorkbookTab::id`].
    pub fn close_tab_by_id(&mut self, tab_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.tabs.iter().position(|t| t.id() == tab_id) {
            self.close_tab(ix, window, cx);
        }
    }

    /// Closes the currently active workbook tab (or closes the window if already on an empty Home screen).
    pub fn close_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_showing_home() && !self.tabs.is_empty() {
            self.close_tab(self.active_tab_ix, window, cx);
        }
    }

    /// Reorders a workbook tab from `from_ix` to `to_ix`, keeping the active tab anchored by ID.
    pub fn move_tab(&mut self, from_ix: usize, to_ix: usize, cx: &mut Context<Self>) {
        if from_ix >= self.tabs.len() || to_ix >= self.tabs.len() || from_ix == to_ix {
            return;
        }
        let active_id = self.tabs.get(self.active_tab_ix).map(WorkbookTab::id);
        let tab = self.tabs.remove(from_ix);
        let view = self.sheet_views.remove(from_ix);
        self.tabs.insert(to_ix, tab);
        self.sheet_views.insert(to_ix, view);
        if let Some(aid) = active_id
            && let Some(new_ix) = self.tabs.iter().position(|t| t.id() == aid)
        {
            self.active_tab_ix = new_ix;
        }
        cx.notify();
    }

    fn render_workspace_content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let showing_settings = self.show_settings;
        let showing_home = self.is_showing_home();
        let active_sheet = self.active_sheet_view().cloned();

        h_flex()
            .h_full()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .bg(cx.theme().background)
            .when(showing_settings, |el| {
                el.child(self.render_settings_screen(cx))
            })
            .when(!showing_settings && showing_home, |el| {
                el.child(self.render_home_screen(cx))
            })
            .when(!showing_settings && !showing_home, |el| {
                el.when_some(active_sheet, |row, sheet_view| row.child(sheet_view))
                    .when(self.inspector_open, |row| {
                        row.child(self.render_inspector_slot(cx))
                    })
            })
    }
}

impl Focusable for MainWindow {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for MainWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let showing_home_or_settings = self.is_showing_home() || self.show_settings;
        #[cfg(not(target_os = "macos"))]
        let win_menu = self.app_menu_bar.clone();

        let root = v_flex()
            .id("ordia-main-window")
            .relative()
            .key_context("OrdiaMainWindow")
            .track_focus(&self.focus_handle)
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.open_dropped_paths(paths.paths(), window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenSettings, _, cx| {
                this.open_settings_screen(cx);
            }))
            .on_action(cx.listener(|this, _: &OpenCommandPalette, window, cx| {
                if this.palette_open {
                    this.close_command_palette(window, cx);
                } else {
                    this.open_command_palette(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &SaveWorkbook, _, cx| {
                this.save_active_workbook(cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleTheme, window, cx| {
                this.toggle_theme(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.toggle_sidebar(cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleInspector, _, cx| {
                this.toggle_inspector(cx);
            }))
            .on_action(cx.listener(|this, _: &NewWorkbookTab, window, cx| {
                this.add_workbook_tab(window, cx);
            }))
            .on_action(cx.listener(|this, _: &CloseActiveTab, window, cx| {
                this.close_active_tab(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ShowHome, _, cx| {
                this.show_home_screen(cx);
            }))
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_title_bar(cx));

        #[cfg(not(target_os = "macos"))]
        let root = root.child(
            h_flex()
                .w_full()
                .px_2()
                .border_b_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().popover)
                .child(win_menu),
        );

        root.when(!showing_home_or_settings, |el| {
            el.child(self.render_toolbar_slot(cx))
        })
        .when(
            self.loading_banner.is_some() || self.open_error_banner.is_some(),
            |el| el.child(self.render_open_banners(cx)),
        )
        .child(
            h_flex()
                .items_stretch()
                .flex_1()
                .min_h_0()
                .size_full()
                .child(self.render_sidebar(cx))
                .child(self.render_workspace_content(cx)),
        )
        .child(self.render_status_bar(cx))
        .when(self.palette_open, |el| {
            el.child(self.render_command_palette_overlay(cx))
        })
    }
}
