//! Headless UI integration tests for the Ordia application shell (`M1-02`).

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, Bounds, TestAppContext, WindowBounds, WindowOptions, px, size};
use ordia_app::MainWindow;

static UI_GLOBAL_LOCK: Mutex<()> = Mutex::new(());

fn lock_ui_globals() -> MutexGuard<'static, ()> {
    UI_GLOBAL_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[gpui_kit::test]
fn shell_new_switch_close_reorder_tabs_home_inspector_and_multiwindow(cx: &mut TestAppContext) {
    let _serial = lock_ui_globals();
    cx.update(|cx| {
        gpui_kit::init(cx);
        ordia_app::register_bindings_and_menus(cx);
    });

    let (handle, view) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1200.), px(780.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| MainWindow::new(window, cx)),
        )
        .expect("open headless main window")
    });

    let initial_dark = cx.update(|cx| cx.theme().mode.is_dark());

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);

        // 1. Theme toggle round-trip.
        window.click("toggle-theme", cx);
        assert_eq!(cx.theme().mode.is_dark(), !initial_dark);
        window.click("toggle-theme", cx);
        assert_eq!(cx.theme().mode.is_dark(), initial_dark);

        // 2. New tab via '+' button (`Book4.xlsx`, id=4).
        window.click("new-workbook-tab", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).tabs().len(), 4);
        assert_eq!(view.read(cx).active_tab_ix(), 3);
        assert_eq!(
            view.read(cx).active_tab().map(|t| t.title().as_ref()),
            Some("Book4.xlsx")
        );

        // 3. Switch tab by clicking tab pill `workbook-tab-2` (`Sales_Pipeline.xlsx`).
        window.click("workbook-tab-2", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).active_tab_ix(), 1);
        assert_eq!(
            view.read(cx).active_tab().map(|t| t.title().as_ref()),
            Some("Sales_Pipeline.xlsx")
        );

        // 4. Drag-reorder tabs: move `Budget_2026.xlsx` (ix 0) to ix 2; active tab `Sales_Pipeline.xlsx` (id=2) shifts to ix 0!
        view.update(cx, |this, cx| {
            this.move_tab(0, 2, cx);
        });
        window.render_frame(cx);
        assert_eq!(view.read(cx).active_tab_ix(), 0);
        assert_eq!(
            view.read(cx).active_tab().map(|t| t.title().as_ref()),
            Some("Sales_Pipeline.xlsx")
        );

        // 5. Right contextual property inspector slot toggle & close button.
        assert!(!view.read(cx).is_inspector_open());
        window.click("toggle-inspector", cx);
        window.render_frame(cx);
        assert!(view.read(cx).is_inspector_open());
        window.click("close-inspector", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).is_inspector_open());

        // 6. Close tab via per-tab close button (`close-tab-4`), then close remaining tabs to enter Home view.
        window.click("close-tab-4", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).tabs().len(), 3);

        view.update(cx, |this, cx| {
            while !this.tabs().is_empty() {
                this.close_tab(0, window, cx);
            }
        });
        window.render_frame(cx);
        assert!(view.read(cx).is_showing_home());
        assert!(view.read(cx).active_tab().is_none());

        // 7. Create a new workbook directly from the Home screen (`home-new-blank`) and drop an external `.xlsx` file.
        window.click("home-new-blank", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).is_showing_home());
        assert_eq!(view.read(cx).tabs().len(), 1);

        let sample_xlsx = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/public/m0_budget_sample.xlsx");
        view.update(cx, |this, cx| {
            this.open_dropped_paths(&[sample_xlsx], window, cx);
        });
        window.render_frame(cx);
        assert_eq!(view.read(cx).tabs().len(), 2);
        assert_eq!(
            view.read(cx).active_tab().map(|t| t.title().as_ref()),
            Some("m0_budget_sample.xlsx")
        );
    })
    .expect("drive main window interactions");

    // 8. Multi-window: open a second independent MainWindow instance.
    let (_handle2, view2) = cx.update(|cx| MainWindow::open(cx).expect("open second window"));
    cx.update(|cx| {
        assert_eq!(view.read(cx).tabs().len(), 2);
        assert_eq!(view2.read(cx).tabs().len(), 3);
    });
}

#[gpui_kit::test]
fn command_palette_pinyin_search_and_registry_toolbar_execution(cx: &mut TestAppContext) {
    let _serial = lock_ui_globals();
    cx.update(|cx| {
        gpui_kit::init(cx);
        ordia_app::register_bindings_and_menus(cx);
    });

    let (handle, view) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1200.), px(780.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| MainWindow::new(window, cx)),
        )
        .expect("open headless main window")
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(!view.read(cx).is_command_palette_open());
        assert_eq!(view.read(cx).saved_count(), 0);

        // Execute `file.save` via the toolbar button generated from `CommandRegistry`.
        window.click("toolbar-file.save", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).saved_count(), 1);

        // Open the `Cmd+K` Command Palette, verify Pinyin query `"bc"` surfaces `"file.save"` (`保存`),
        // and verify closing the palette restores clean state.
        window.click("open-command-palette", cx);
        window.render_frame(cx);
        assert!(view.read(cx).is_command_palette_open());

        let hits = view.read(cx).command_registry().search("bc");
        assert!(!hits.is_empty());
        assert_eq!(hits[0].id(), "file.save");
        assert_eq!(hits[0].label_zh(), "保存");

        view.update(cx, |this, cx| {
            this.close_command_palette(window, cx);
        });
        window.render_frame(cx);
        assert!(!view.read(cx).is_command_palette_open());
    })
    .expect("drive command palette UI test");
}

#[gpui_kit::test]
fn settings_and_window_state_persist_across_restart(cx: &mut TestAppContext) {
    let _serial = lock_ui_globals();
    use ordia_core::{
        AppearanceMode, EnterMoveDirection, LanguageSetting, PersistedWindowBounds, SettingsStore,
    };

    cx.update(|cx| {
        gpui_kit::init(cx);
        ordia_app::register_bindings_and_menus(cx);
    });

    let temp_dir = tempfile::tempdir().expect("create temporary config dir");
    let store = SettingsStore::with_dir(temp_dir.path());

    // Session 1: open window backed by `store`, open Settings page, customize preferences,
    // toggle panels, drop a recent file, and record custom window geometry.
    let (handle1, view1) = cx.update(|cx| {
        MainWindow::open_with_store(store.clone(), cx).expect("open session 1 main window")
    });

    cx.update_window(handle1, |_, window, cx| {
        window.render_frame(cx);
        view1.update(cx, |this, cx| {
            this.open_settings_screen(cx);
        });
        window.render_frame(cx);
        assert!(view1.read(cx).is_showing_settings());

        // Toggle "reopen last workbooks" button directly inside the Settings UI!
        window.click("settings-toggle-reopen", cx);
        window.render_frame(cx);
        assert!(!view1.read(cx).settings().general.reopen_last_workbooks);

        // Update appearance, language, Enter direction, panels, recent files, and window bounds.
        let sample_xlsx = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/public/m0_budget_sample.xlsx");
        view1.update(cx, |this, cx| {
            this.set_appearance_mode(AppearanceMode::Dark, window, cx);
            this.set_language_setting(LanguageSetting::ZhCn, cx);
            this.set_enter_move_direction(EnterMoveDirection::Right, cx);
            this.toggle_sidebar(cx);
            this.toggle_inspector(cx);
            this.open_dropped_paths(&[sample_xlsx], window, cx);
            this.record_window_bounds(PersistedWindowBounds::new(
                140.0, 90.0, 1360.0, 840.0, false,
            ));
        });
        window.render_frame(cx);
        assert!(cx.theme().mode.is_dark());
        assert_eq!(ordia_i18n::current_locale(), ordia_i18n::Locale::ZhCn);
    })
    .expect("drive session 1 settings mutations");

    // Reset in-memory globals to Light + En before Session 2 so we prove Session 2 restores them from TOML!
    cx.update(|cx| {
        ordia_i18n::set_locale(ordia_i18n::Locale::En);
        ordia_ui::apply_ordia_theme(gpui_kit::component::ThemeMode::Light, None, cx);
    });
    assert_eq!(ordia_i18n::current_locale(), ordia_i18n::Locale::En);

    // Session 2 (simulated restart): open a fresh MainWindow from the same `SettingsStore`.
    let (handle2, view2) = cx.update(|cx| {
        MainWindow::open_with_store(store, cx).expect("reopen session 2 main window from disk")
    });

    cx.update_window(handle2, |_, window, cx| {
        window.render_frame(cx);
        let win = view2.read(cx);
        assert!(!win.settings().general.reopen_last_workbooks);
        assert_eq!(win.settings().appearance.theme, AppearanceMode::Dark);
        assert_eq!(win.settings().language, LanguageSetting::ZhCn);
        assert_eq!(
            win.settings().editing.enter_move_direction,
            EnterMoveDirection::Right
        );
        assert!(cx.theme().mode.is_dark());
        assert_eq!(ordia_i18n::current_locale(), ordia_i18n::Locale::ZhCn);
        assert!(win.is_sidebar_collapsed());
        assert!(win.is_inspector_open());
        assert_eq!(
            win.persisted_window_bounds(),
            Some(PersistedWindowBounds::new(
                140.0, 90.0, 1360.0, 840.0, false
            ))
        );
        assert_eq!(
            win.recent_files().first().map(|r| r.title().as_ref()),
            Some("m0_budget_sample.xlsx")
        );

        // Restore clean defaults at end of test.
        ordia_i18n::set_locale(ordia_i18n::Locale::En);
        ordia_ui::apply_ordia_theme(gpui_kit::component::ThemeMode::Light, Some(window), cx);
    })
    .expect("verify session 2 restored state");
}

#[gpui_kit::test]
fn corrupt_files_show_friendly_error_and_async_open_never_blocks_ui(cx: &mut TestAppContext) {
    let _serial = lock_ui_globals();
    cx.update(|cx| {
        gpui_kit::init(cx);
        ordia_app::register_bindings_and_menus(cx);
    });

    let temp_dir = tempfile::tempdir().expect("temp dir");
    let corrupt_xlsx = temp_dir.path().join("damaged_archive.xlsx");
    std::fs::write(&corrupt_xlsx, b"PK\x03\x04\x14\x00truncated-corrupt-zip")
        .expect("write corrupt xlsx");

    let legacy_xls_as_xlsx = temp_dir.path().join("legacy_biff8.xlsx");
    std::fs::write(
        &legacy_xls_as_xlsx,
        [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0x00, 0x01],
    )
    .expect("write ole2 header");

    let (handle, view) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1200.), px(780.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| MainWindow::new(window, cx)),
        )
        .expect("open headless window")
    });

    // 1. Opening a corrupt `.xlsx` and a disguised legacy `.xls` never crashes; shows dismissible localized banner.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let initial_tabs = view.read(cx).tabs().len();

        view.update(cx, |this, cx| {
            this.open_dropped_paths(&[corrupt_xlsx], window, cx);
        });
        window.render_frame(cx);
        assert_eq!(view.read(cx).tabs().len(), initial_tabs);
        let err = view
            .read(cx)
            .open_error_banner()
            .expect("error banner shown for corrupt xlsx");
        assert!(err.message.contains("damaged_archive.xlsx"));

        // Dismiss via UI button!
        window.click("dismiss-open-error", cx);
        window.render_frame(cx);
        assert!(view.read(cx).open_error_banner().is_none());

        // Disguised `.xls` binary -> friendly `.xls` conversion hint!
        view.update(cx, |this, cx| {
            this.open_dropped_paths(&[legacy_xls_as_xlsx], window, cx);
        });
        window.render_frame(cx);
        let xls_err = view
            .read(cx)
            .open_error_banner()
            .expect("error banner shown for legacy xls");
        assert!(xls_err.message.contains(".xls"));
    })
    .expect("verify corrupt and legacy file handling");

    // 2. Asynchronous background loading (`open_file_async`) returns immediately on the UI thread,
    // displays the non-blocking loading progress banner while UI frames continue rendering,
    // and opens the workbook tab once background parsing completes.
    let sample_xlsx =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/public/m0_budget_sample.xlsx");
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |this, cx| {
            this.open_file_async(sample_xlsx.clone(), window, cx);
        });
        // Immediately after `open_file_async`, the UI thread is free to render frames with the progress banner!
        assert!(view.read(cx).loading_banner().is_some());
        window.render_frame(cx);
        window.click("toggle-inspector", cx);
        window.render_frame(cx);
        assert!(view.read(cx).is_inspector_open());
    })
    .expect("verify non-blocking UI frame rendering during async load");

    cx.executor().run_until_parked();

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).loading_banner().is_none());
        assert_eq!(
            view.read(cx).active_tab().map(|t| t.title().as_ref()),
            Some("m0_budget_sample.xlsx")
        );
    })
    .expect("verify async load completion");
}

#[gpui_kit::test]
fn startup_detects_unclean_journal_and_recovers_last_flushed_transactions(cx: &mut TestAppContext) {
    let _serial = lock_ui_globals();
    use ordia_core::{JournalFlushPolicy, JournalSessionMeta, JournalWriter, SettingsStore};
    use ordia_sheet_model::{CellCoord, SheetOp, Transaction};
    use std::io::Write as _;
    use std::time::Duration;

    cx.update(|cx| {
        gpui_kit::init(cx);
        ordia_app::register_bindings_and_menus(cx);
    });

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let store = SettingsStore::with_dir(temp_dir.path());

    // Simulate a previous session where 5 transactions were appended (`flush_every_n_ops = 4`, so 1..=4 flushed,
    // 5 stayed in memory), a torn write hit the tail of `ops.journal`, and the process died abruptly!
    {
        let meta = JournalSessionMeta::new("crash-sess-1", "Crash_Budget.xlsx", None);
        let policy = JournalFlushPolicy::new(4, Duration::from_secs(3600));
        let mut writer =
            JournalWriter::<Transaction>::open(store.dir(), &meta, policy).expect("open journal");
        for row in 0..5u32 {
            let tx = Transaction::new(
                "Edit cell",
                vec![SheetOp::SetCellInput {
                    sheet: 0,
                    coord: CellCoord::new(row, 0),
                    old_input: String::new(),
                    new_input: format!("Committed-{}", row + 1),
                }],
            );
            writer.append(&tx).expect("append tx");
        }
        assert_eq!(writer.flushed_seq(), 4);
        let jpath = writer.journal_path().to_path_buf();
        drop(writer);

        let mut torn = std::fs::OpenOptions::new()
            .append(true)
            .open(jpath)
            .expect("open journal tail");
        torn.write_all(b"\x99\x88\x77torn-tail")
            .expect("append torn tail");
    }

    // Restart Ordia: MainWindow detects the unclean journal and shows the recovery prompt banner!
    let (handle, view) = cx.update(|cx| {
        MainWindow::open_with_store(store, cx).expect("open main window after simulated crash")
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(view.read(cx).unclean_recovery_sessions().len(), 1);

        // Click the "Recover" button in the crash-recovery banner!
        window.click("recover-sessions-btn", cx);
        window.render_frame(cx);

        assert!(view.read(cx).unclean_recovery_sessions().is_empty());
        assert_eq!(
            view.read(cx).active_tab().map(|t| t.title().as_ref()),
            Some("Crash_Budget.xlsx")
        );
        let sheet_entity = view
            .read(cx)
            .active_sheet_view()
            .expect("recovered sheet view")
            .clone();
        let doc = sheet_entity.read(cx).document();
        assert_eq!(
            doc.cell_display_text(CellCoord::new(0, 0)).expect("A1"),
            "Committed-1"
        );
        assert_eq!(
            doc.cell_display_text(CellCoord::new(3, 0)).expect("A4"),
            "Committed-4"
        );
        // Unflushed 5th operation was never synced before the simulated crash.
        assert_eq!(doc.cell_display_text(CellCoord::new(4, 0)).expect("A5"), "");
    })
    .expect("verify startup crash recovery UI flow");
}

#[gpui_kit::test]
fn about_screen_displays_version_license_and_third_party_notices(cx: &mut TestAppContext) {
    let _serial = lock_ui_globals();
    cx.update(|cx| {
        gpui_kit::init(cx);
        ordia_app::register_bindings_and_menus(cx);
    });

    assert_eq!(ordia_app::about_view::app_version(), "0.1.0");
    assert_eq!(ordia_app::about_view::app_license(), "Apache-2.0");
    let notices = ordia_app::about_view::third_party_notice_lines();
    assert!(
        notices.len() >= 100,
        "expected comprehensive third-party notices, got {}",
        notices.len()
    );
    assert!(notices.iter().any(|l| l.starts_with("ironcalc |")));

    let (handle, view) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1200.), px(780.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| MainWindow::new(window, cx)),
        )
        .expect("open headless window")
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(!view.read(cx).is_showing_about());

        // Trigger `"help.about"` command from CommandRegistry (also accessible via Pinyin `"gy"` -> `"关于 Ordia"`!).
        let about_hits = view.read(cx).command_registry().search("gy");
        assert!(about_hits.iter().any(|c| c.id() == "help.about"));

        let spec = view
            .read(cx)
            .command_registry()
            .get("help.about")
            .expect("help.about registered")
            .clone();
        view.update(cx, |this, cx| {
            spec.execute(this, window, cx);
        });
        window.render_frame(cx);
        assert!(view.read(cx).is_showing_about());

        window.click("about-close", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).is_showing_about());
    })
    .expect("verify About Ordia screen flow");
}

#[gpui_kit::test]
fn crash_report_banner_is_opt_in_and_open_log_dir_command_works(cx: &mut TestAppContext) {
    let _serial = lock_ui_globals();
    cx.update(|cx| {
        gpui_kit::init(cx);
        ordia_app::register_bindings_and_menus(cx);
    });

    let dir = tempfile::tempdir().expect("tempdir");
    ordia_core::install_panic_crash_hook(dir.path());
    let canary_cell = "SECRET_CLIENT_REVENUE_999888777";
    let _ = std::thread::spawn(move || {
        panic!("simulated crash touching {canary_cell}");
    })
    .join();

    let store = ordia_core::SettingsStore::with_dir(dir.path());
    let (handle, view) =
        cx.update(|cx| MainWindow::open_with_store(store, cx).expect("open window with crash"));

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let reports = view.read(cx).pending_crash_reports();
        assert_eq!(reports.len(), 1);
        let raw_url = reports[0].1.github_issue_url();
        assert!(raw_url.starts_with("https://github.com/onlw/Ordia/issues/new?"));
        assert!(!raw_url.contains(canary_cell));

        // Verify Help -> Open Log Directory is registered and searchable by Pinyin ("rz").
        let hits = view.read(cx).command_registry().search("rz");
        assert!(hits.iter().any(|c| c.id() == "help.open_log_dir"));

        let log_dir = view.update(cx, |this, cx| this.open_log_directory(false, cx));
        assert!(log_dir.is_dir());
        assert_eq!(view.read(cx).opened_log_dir_count(), 1);
        window.render_frame(cx);

        // Dismissing crash report banner deletes local crash metadata without network upload.
        window.click("dismiss-crash-report-btn", cx);
        window.render_frame(cx);
        assert!(view.read(cx).pending_crash_reports().is_empty());
        assert!(ordia_core::list_pending_crash_reports(dir.path()).is_empty());
    })
    .expect("verify crash report banner and log dir UI flow");
}
