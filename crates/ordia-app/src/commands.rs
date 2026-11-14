//! Unified `CommandRegistry`, shortcut conflict detector, and `Cmd/Ctrl+K` Command Palette (`M1-03`).
//!
//! Generates global keybindings, native macOS / Windows menus, contextual toolbar buttons,
//! and Command Palette items from a single authoritative command catalog.

use std::collections::BTreeMap;
use std::rc::Rc;

use gpui_kit::component::{Disableable as _, Icon, command::CommandItem};
use gpui_kit::{Action, App, Context, KeyBinding, Menu, MenuItem, SharedString, Window};
use ordia_sheet_view::{RedoEdit, UndoEdit};
use ordia_ui::OrdiaIcon;

use crate::pinyin::fuzzy_or_pinyin_score;
use crate::{
    CloseActiveTab, MainWindow, NewWindow, NewWorkbookTab, OpenAbout, OpenCommandPalette,
    OpenSettings, Quit, SaveWorkbook, ShowHome, ToggleInspector, ToggleSidebar, ToggleTheme,
};

type EnabledPredicate = Rc<dyn Fn(&MainWindow, &App) -> bool>;
type ActionFactory = Rc<dyn Fn() -> Box<dyn Action>>;
type CommandExecutor = Rc<dyn Fn(&mut MainWindow, &mut Window, &mut Context<MainWindow>)>;

/// Top-level menu / command grouping category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CommandCategory {
    /// Application-level lifecycle commands (`Ordia` menu on macOS).
    App,
    /// Document creation, opening, saving, and tab management.
    File,
    /// Undo, redo, clipboard, and editing commands.
    Edit,
    /// Appearance, panels, navigation, and palette commands.
    View,
    /// Documentation, logs, diagnostics, and About Ordia commands.
    Help,
}

impl CommandCategory {
    /// Localized display title of the menu group.
    pub fn menu_title(self) -> &'static str {
        match self {
            Self::App => ordia_i18n::t!("menu.app"),
            Self::File => ordia_i18n::t!("menu.file"),
            Self::Edit => ordia_i18n::t!("menu.edit"),
            Self::View => ordia_i18n::t!("menu.view"),
            Self::Help => ordia_i18n::t!("menu.help"),
        }
    }
}

/// Declarative specification of a single command in [`CommandRegistry`].
#[derive(Clone)]
pub struct CommandSpec {
    id: &'static str,
    category: CommandCategory,
    i18n_key: &'static str,
    icon: Option<OrdiaIcon>,
    shortcut_macos: Option<&'static str>,
    shortcut_other: Option<&'static str>,
    key_context: Option<&'static str>,
    in_toolbar: bool,
    is_enabled: EnabledPredicate,
    make_action: ActionFactory,
    execute: CommandExecutor,
}

impl CommandSpec {
    /// Unique stable command ID (e.g. `"file.save"`).
    pub const fn id(&self) -> &'static str {
        self.id
    }

    /// Top-level menu category.
    pub const fn category(&self) -> CommandCategory {
        self.category
    }

    /// Localization catalog key (`ordia-i18n`).
    pub const fn i18n_key(&self) -> &'static str {
        self.i18n_key
    }

    /// Display name in the currently active [`ordia_i18n::Locale`].
    pub fn label(&self) -> &'static str {
        ordia_i18n::tr_in(ordia_i18n::current_locale(), self.i18n_key).unwrap_or(self.i18n_key)
    }

    /// Simplified Chinese display name (`zh-CN`).
    pub fn label_zh(&self) -> &'static str {
        ordia_i18n::tr_in(ordia_i18n::Locale::ZhCn, self.i18n_key).unwrap_or(self.i18n_key)
    }

    /// English display name (`en`).
    pub fn label_en(&self) -> &'static str {
        ordia_i18n::tr_in(ordia_i18n::Locale::En, self.i18n_key).unwrap_or(self.i18n_key)
    }

    /// Combined bilingual label shown in the Command Palette.
    pub fn palette_label(&self) -> SharedString {
        SharedString::from(format!("{} · {}", self.label_zh(), self.label_en()))
    }

    /// Optional semantic icon.
    pub const fn icon(&self) -> Option<OrdiaIcon> {
        self.icon
    }

    /// Active shortcut chord on the current target OS, if any.
    pub const fn shortcut_for_current_os(&self) -> Option<&'static str> {
        #[cfg(target_os = "macos")]
        {
            self.shortcut_macos
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.shortcut_other
        }
    }

    /// Whether this command is exposed in the contextual toolbar slot.
    pub const fn is_in_toolbar(&self) -> bool {
        self.in_toolbar
    }

    /// Evaluates whether the command is currently enabled for `window_view`.
    pub fn is_enabled(&self, window_view: &MainWindow, cx: &App) -> bool {
        (self.is_enabled)(window_view, cx)
    }

    /// Instantiates the GPUI [`Action`] bound to this command.
    pub fn boxed_action(&self) -> Box<dyn Action> {
        (self.make_action)()
    }

    /// Executes the command against `window_view`.
    pub fn execute(
        &self,
        window_view: &mut MainWindow,
        window: &mut Window,
        cx: &mut Context<MainWindow>,
    ) {
        if (self.is_enabled)(window_view, cx) {
            (self.execute)(window_view, window, cx);
        }
    }
}

/// Describes a detected keyboard shortcut collision between two distinct commands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutConflict {
    platform: &'static str,
    normalized_shortcut: String,
    key_context: Option<&'static str>,
    first_command_id: &'static str,
    second_command_id: &'static str,
}

impl ShortcutConflict {
    /// Platform where the collision occurs (`"macos"` or `"windows/linux"`).
    pub fn platform(&self) -> &'static str {
        self.platform
    }

    /// Canonicalized modifier+key string (e.g. `"cmd-shift-s"`).
    pub fn normalized_shortcut(&self) -> &str {
        &self.normalized_shortcut
    }

    /// First conflicting command ID.
    pub fn first_command_id(&self) -> &'static str {
        self.first_command_id
    }

    /// Second conflicting command ID.
    pub fn second_command_id(&self) -> &'static str {
        self.second_command_id
    }
}

/// Normalizes a GPUI keystroke string so `"shift-cmd-S"` and `"cmd-shift-s"` canonicalize identically.
pub fn normalize_shortcut(raw: &str) -> String {
    raw.split_whitespace()
        .map(|chord| {
            let parts: Vec<String> = chord
                .split('-')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_ascii_lowercase())
                .collect();
            if parts.len() <= 1 {
                return parts.join("-");
            }
            let key = parts.last().cloned().unwrap_or_default();
            let mut mods: Vec<String> = parts[..parts.len() - 1].to_vec();
            mods.sort();
            mods.dedup();
            mods.push(key);
            mods.join("-")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Authoritative registry of all Ordia application commands.
#[derive(Clone, Default)]
pub struct CommandRegistry {
    commands: Vec<CommandSpec>,
}

impl CommandRegistry {
    /// Creates an empty command registry.
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    /// Registers a [`CommandSpec`].
    pub fn register(&mut self, spec: CommandSpec) {
        self.commands.push(spec);
    }

    /// Returns all registered commands in registration order.
    pub fn commands(&self) -> &[CommandSpec] {
        &self.commands
    }

    /// Looks up a command by its stable ID (e.g. `"file.save"`).
    pub fn get(&self, id: &str) -> Option<&CommandSpec> {
        self.commands.iter().find(|c| c.id == id)
    }

    /// Builds the default production Ordia command registry.
    pub fn default_ordia() -> Self {
        let mut reg = Self::new();
        let always: EnabledPredicate = Rc::new(|_, _| true);
        let when_workbook_open: EnabledPredicate = Rc::new(|win, _| !win.is_showing_home());

        reg.register(CommandSpec {
            id: "app.settings",
            category: CommandCategory::App,
            i18n_key: "cmd.app.settings",
            icon: Some(OrdiaIcon::Settings),
            shortcut_macos: Some("cmd-,"),
            shortcut_other: Some("ctrl-,"),
            key_context: None,
            in_toolbar: false,
            is_enabled: always.clone(),
            make_action: Rc::new(|| Box::new(OpenSettings)),
            execute: Rc::new(|win, _, cx| win.open_settings_screen(cx)),
        });

        reg.register(CommandSpec {
            id: "app.quit",
            category: CommandCategory::App,
            i18n_key: "cmd.app.quit",
            icon: Some(OrdiaIcon::Close),
            shortcut_macos: Some("cmd-q"),
            shortcut_other: Some("ctrl-q"),
            key_context: None,
            in_toolbar: false,
            is_enabled: always.clone(),
            make_action: Rc::new(|| Box::new(Quit)),
            execute: Rc::new(|_, _, cx| cx.quit()),
        });

        reg.register(CommandSpec {
            id: "file.save",
            category: CommandCategory::File,
            i18n_key: "cmd.file.save",
            icon: Some(OrdiaIcon::Save),
            shortcut_macos: Some("cmd-s"),
            shortcut_other: Some("ctrl-s"),
            key_context: None,
            in_toolbar: true,
            is_enabled: when_workbook_open.clone(),
            make_action: Rc::new(|| Box::new(SaveWorkbook)),
            execute: Rc::new(|win, _, cx| win.save_active_workbook(cx)),
        });

        reg.register(CommandSpec {
            id: "file.new_workbook",
            category: CommandCategory::File,
            i18n_key: "cmd.file.new_workbook",
            icon: Some(OrdiaIcon::Add),
            shortcut_macos: Some("cmd-t"),
            shortcut_other: Some("ctrl-t"),
            key_context: None,
            in_toolbar: false,
            is_enabled: always.clone(),
            make_action: Rc::new(|| Box::new(NewWorkbookTab)),
            execute: Rc::new(|win, window, cx| win.add_workbook_tab(window, cx)),
        });

        reg.register(CommandSpec {
            id: "file.new_window",
            category: CommandCategory::File,
            i18n_key: "cmd.file.new_window",
            icon: Some(OrdiaIcon::Open),
            shortcut_macos: Some("cmd-shift-n"),
            shortcut_other: Some("ctrl-shift-n"),
            key_context: None,
            in_toolbar: false,
            is_enabled: always.clone(),
            make_action: Rc::new(|| Box::new(NewWindow)),
            execute: Rc::new(|_, _, cx| {
                let _ = MainWindow::open(cx);
            }),
        });

        reg.register(CommandSpec {
            id: "file.close_tab",
            category: CommandCategory::File,
            i18n_key: "cmd.file.close_tab",
            icon: Some(OrdiaIcon::Close),
            shortcut_macos: Some("cmd-w"),
            shortcut_other: Some("ctrl-w"),
            key_context: None,
            in_toolbar: false,
            is_enabled: when_workbook_open.clone(),
            make_action: Rc::new(|| Box::new(CloseActiveTab)),
            execute: Rc::new(|win, window, cx| win.close_active_tab(window, cx)),
        });

        reg.register(CommandSpec {
            id: "edit.undo",
            category: CommandCategory::Edit,
            i18n_key: "cmd.edit.undo",
            icon: Some(OrdiaIcon::Undo),
            shortcut_macos: None,
            shortcut_other: None,
            key_context: Some("OrdiaSheetWorkspace"),
            in_toolbar: true,
            is_enabled: when_workbook_open.clone(),
            make_action: Rc::new(|| Box::new(UndoEdit)),
            execute: Rc::new(|win, _, cx| {
                if let Some(sheet) = win.active_sheet_view().cloned() {
                    sheet.update(cx, |s, cx| {
                        let _ = s.document_mut().undo();
                        cx.notify();
                    });
                }
            }),
        });

        reg.register(CommandSpec {
            id: "edit.redo",
            category: CommandCategory::Edit,
            i18n_key: "cmd.edit.redo",
            icon: Some(OrdiaIcon::Redo),
            shortcut_macos: None,
            shortcut_other: None,
            key_context: Some("OrdiaSheetWorkspace"),
            in_toolbar: true,
            is_enabled: when_workbook_open.clone(),
            make_action: Rc::new(|| Box::new(RedoEdit)),
            execute: Rc::new(|win, _, cx| {
                if let Some(sheet) = win.active_sheet_view().cloned() {
                    sheet.update(cx, |s, cx| {
                        let _ = s.document_mut().redo();
                        cx.notify();
                    });
                }
            }),
        });

        reg.register(CommandSpec {
            id: "view.command_palette",
            category: CommandCategory::View,
            i18n_key: "cmd.view.command_palette",
            icon: Some(OrdiaIcon::CommandPalette),
            shortcut_macos: Some("cmd-k"),
            shortcut_other: Some("ctrl-k"),
            key_context: None,
            in_toolbar: false,
            is_enabled: always.clone(),
            make_action: Rc::new(|| Box::new(OpenCommandPalette)),
            execute: Rc::new(|win, window, cx| win.open_command_palette(window, cx)),
        });

        reg.register(CommandSpec {
            id: "view.home",
            category: CommandCategory::View,
            i18n_key: "cmd.view.home",
            icon: Some(OrdiaIcon::Spreadsheet),
            shortcut_macos: Some("cmd-shift-h"),
            shortcut_other: Some("ctrl-shift-h"),
            key_context: None,
            in_toolbar: false,
            is_enabled: always.clone(),
            make_action: Rc::new(|| Box::new(ShowHome)),
            execute: Rc::new(|win, _, cx| win.show_home_screen(cx)),
        });

        reg.register(CommandSpec {
            id: "view.toggle_inspector",
            category: CommandCategory::View,
            i18n_key: "cmd.view.toggle_inspector",
            icon: Some(OrdiaIcon::Inspector),
            shortcut_macos: Some("cmd-alt-i"),
            shortcut_other: Some("ctrl-alt-i"),
            key_context: None,
            in_toolbar: true,
            is_enabled: when_workbook_open,
            make_action: Rc::new(|| Box::new(ToggleInspector)),
            execute: Rc::new(|win, _, cx| win.toggle_inspector(cx)),
        });

        reg.register(CommandSpec {
            id: "view.toggle_sidebar",
            category: CommandCategory::View,
            i18n_key: "cmd.view.toggle_sidebar",
            icon: Some(OrdiaIcon::Settings),
            shortcut_macos: Some("cmd-b"),
            shortcut_other: Some("ctrl-b"),
            key_context: None,
            in_toolbar: false,
            is_enabled: always.clone(),
            make_action: Rc::new(|| Box::new(ToggleSidebar)),
            execute: Rc::new(|win, _, cx| win.toggle_sidebar(cx)),
        });

        reg.register(CommandSpec {
            id: "view.toggle_theme",
            category: CommandCategory::View,
            i18n_key: "cmd.view.toggle_theme",
            icon: Some(OrdiaIcon::DarkMode),
            shortcut_macos: Some("cmd-shift-d"),
            shortcut_other: Some("ctrl-shift-d"),
            key_context: None,
            in_toolbar: false,
            is_enabled: always.clone(),
            make_action: Rc::new(|| Box::new(ToggleTheme)),
            execute: Rc::new(|win, window, cx| win.toggle_theme(window, cx)),
        });

        reg.register(CommandSpec {
            id: "help.about",
            category: CommandCategory::Help,
            i18n_key: "cmd.help.about",
            icon: Some(OrdiaIcon::Spreadsheet),
            shortcut_macos: None,
            shortcut_other: None,
            key_context: None,
            in_toolbar: false,
            is_enabled: always,
            make_action: Rc::new(|| Box::new(OpenAbout)),
            execute: Rc::new(|win, _, cx| win.open_about_screen(cx)),
        });

        reg
    }
}

impl CommandRegistry {
    /// Searches registered commands by English/Chinese text, fuzzy subsequence, or Chinese Pinyin initials.
    ///
    /// For example, querying `"bc"` returns `"file.save"` (`保存`).
    pub fn search(&self, query: &str) -> Vec<&CommandSpec> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return self.commands.iter().collect();
        }
        let mut scored: Vec<(u32, usize, &CommandSpec)> = self
            .commands
            .iter()
            .enumerate()
            .filter_map(|(ix, spec)| {
                let s_zh = fuzzy_or_pinyin_score(trimmed, spec.label_zh());
                let s_en = fuzzy_or_pinyin_score(trimmed, spec.label_en());
                let s_id = fuzzy_or_pinyin_score(trimmed, spec.id);
                let best = [s_zh, s_en, s_id].into_iter().flatten().max()?;
                Some((best, ix, spec))
            })
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        scored.into_iter().map(|(_, _, spec)| spec).collect()
    }

    /// Detects duplicate normalized keyboard shortcuts within the same key context across platforms.
    pub fn detect_shortcut_conflicts(&self) -> Vec<ShortcutConflict> {
        let mut conflicts = Vec::new();
        for (platform, pick) in [
            (
                "macos",
                (|s: &CommandSpec| s.shortcut_macos) as fn(&CommandSpec) -> Option<&'static str>,
            ),
            (
                "windows/linux",
                (|s: &CommandSpec| s.shortcut_other) as fn(&CommandSpec) -> Option<&'static str>,
            ),
        ] {
            let mut seen: BTreeMap<(Option<&'static str>, String), &'static str> = BTreeMap::new();
            for spec in &self.commands {
                let Some(raw) = pick(spec) else {
                    continue;
                };
                let norm = normalize_shortcut(raw);
                if norm.is_empty() {
                    continue;
                }
                let key = (spec.key_context, norm.clone());
                if let Some(&existing_id) = seen.get(&key) {
                    if existing_id != spec.id {
                        conflicts.push(ShortcutConflict {
                            platform,
                            normalized_shortcut: norm,
                            key_context: spec.key_context,
                            first_command_id: existing_id,
                            second_command_id: spec.id,
                        });
                    }
                } else {
                    seen.insert(key, spec.id);
                }
            }
        }
        conflicts
    }

    /// Generates GPUI [`KeyBinding`] definitions for all commands with shortcuts on the current OS.
    pub fn build_keybindings(&self, cx: &App) -> Vec<KeyBinding> {
        self.commands
            .iter()
            .filter_map(|spec| {
                let chord = spec.shortcut_for_current_os()?;
                let ctx_pred = spec.key_context.and_then(|c| {
                    gpui_kit::KeyBindingContextPredicate::parse(c)
                        .ok()
                        .map(Rc::new)
                });
                KeyBinding::load(chord, spec.boxed_action(), ctx_pred, false, None, cx).ok()
            })
            .collect()
    }

    /// Generates native macOS / Windows application [`Menu`] trees directly from the registry.
    pub fn build_menus(&self) -> Vec<Menu> {
        let mut grouped: BTreeMap<CommandCategory, Vec<MenuItem>> = BTreeMap::new();
        for spec in &self.commands {
            grouped
                .entry(spec.category)
                .or_default()
                .push(MenuItem::Action {
                    name: spec.label().into(),
                    action: spec.boxed_action(),
                    os_action: None,
                    checked: false,
                    disabled: false,
                });
        }
        grouped
            .into_iter()
            .map(|(cat, items)| Menu {
                name: cat.menu_title().into(),
                items,
                disabled: false,
            })
            .collect()
    }

    /// Builds filtered [`CommandItem`] rows for GPUI Kit's [`gpui_kit::component::command::Command`].
    pub fn build_palette_items(
        &self,
        query: &str,
        window_view: &MainWindow,
        cx: &App,
    ) -> Vec<CommandItem> {
        self.search(query)
            .into_iter()
            .map(|spec| {
                let enabled = spec.is_enabled(window_view, cx);
                let mut item = CommandItem::new()
                    .label(spec.palette_label())
                    .action(spec.boxed_action())
                    .disabled(!enabled);
                if let Some(icon) = spec.icon {
                    item = item.icon(Icon::new(icon));
                }
                item
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinyin_search_bc_finds_save_command() {
        let reg = CommandRegistry::default_ordia();

        // Core acceptance test: Pinyin initials "bc" finds "保存" ("file.save").
        let bc_hits = reg.search("bc");
        assert!(!bc_hits.is_empty(), "expected 'bc' to match 保存");
        assert_eq!(bc_hits[0].id(), "file.save");
        assert_eq!(bc_hits[0].label_zh(), "保存");

        // Additional Pinyin initial and polyphone checks.
        assert_eq!(reg.search("xjgzb")[0].id(), "file.new_workbook");
        assert_eq!(reg.search("cx")[0].id(), "edit.undo");
        assert_eq!(reg.search("cz")[0].id(), "edit.redo");
        assert_eq!(reg.search("qhwg")[0].id(), "view.toggle_theme");

        // English fuzzy subsequence check ("swapp" -> "Switch Appearance").
        assert_eq!(reg.search("swapp")[0].id(), "view.toggle_theme");
    }

    #[test]
    fn shortcut_conflict_detection_catches_duplicate_normalized_shortcuts() {
        let default_reg = CommandRegistry::default_ordia();
        assert!(
            default_reg.detect_shortcut_conflicts().is_empty(),
            "production CommandRegistry must have zero shortcut conflicts"
        );

        let mut custom = CommandRegistry::new();
        let always: EnabledPredicate = Rc::new(|_, _| true);
        custom.register(CommandSpec {
            id: "cmd.alpha",
            category: CommandCategory::File,
            i18n_key: "cmd.file.save",
            icon: None,
            shortcut_macos: Some("shift-cmd-S"),
            shortcut_other: Some("ctrl-shift-s"),
            key_context: None,
            in_toolbar: false,
            is_enabled: always.clone(),
            make_action: Rc::new(|| Box::new(SaveWorkbook)),
            execute: Rc::new(|_, _, _| {}),
        });
        custom.register(CommandSpec {
            id: "cmd.beta_conflict",
            category: CommandCategory::Edit,
            i18n_key: "cmd.edit.undo",
            icon: None,
            shortcut_macos: Some("cmd-shift-s"),
            shortcut_other: Some("shift-ctrl-S"),
            key_context: None,
            in_toolbar: false,
            is_enabled: always.clone(),
            make_action: Rc::new(|| Box::new(SaveWorkbook)),
            execute: Rc::new(|_, _, _| {}),
        });
        custom.register(CommandSpec {
            id: "cmd.gamma_scoped_ok",
            category: CommandCategory::View,
            i18n_key: "cmd.view.home",
            icon: None,
            shortcut_macos: Some("cmd-shift-s"),
            shortcut_other: Some("ctrl-shift-s"),
            key_context: Some("OrdiaScopedEditor"),
            in_toolbar: false,
            is_enabled: always,
            make_action: Rc::new(|| Box::new(SaveWorkbook)),
            execute: Rc::new(|_, _, _| {}),
        });

        let conflicts = custom.detect_shortcut_conflicts();
        assert_eq!(
            conflicts.len(),
            2,
            "expected 1 macOS + 1 Windows/Linux conflict"
        );
        assert_eq!(conflicts[0].first_command_id(), "cmd.alpha");
        assert_eq!(conflicts[0].second_command_id(), "cmd.beta_conflict");
        assert_eq!(conflicts[0].normalized_shortcut(), "cmd-shift-s");
    }
}
