//! Non-blocking background document loader, progress banner, and friendly localized error display (`M1-06`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _, SharedString, Styled as _,
    Window, div, px,
};
use ordia_core::{
    Document as _, DocumentError, JournalRecoveryManager, LoadProgress, LoadStage,
    UncleanSessionInfo,
};
use ordia_sheet_model::{SheetDocument, Transaction};

use crate::{MainWindow, RecentFileEntry, WorkbookTab};

/// Files at or above `2 MiB` automatically load on GPUI's background worker pool when dropped or opened
/// so the UI thread never stalls on multi-megabyte / 1M-row workbooks.
pub const ASYNC_LOAD_THRESHOLD_BYTES: u64 = 2 * 1024 * 1024;

/// Active non-blocking background file-loading banner state.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct LoadingBannerState {
    /// Display file name being loaded.
    pub file_name: SharedString,
    /// Full path being loaded.
    pub path: PathBuf,
    /// Current pipeline stage.
    pub stage: LoadStage,
    /// Estimated completion percentage (`0..=100`).
    pub percent: u8,
}

/// Dismissible, localized error banner shown when opening a damaged or unsupported file fails.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct OpenErrorBanner {
    /// File name that failed to open.
    pub file_name: SharedString,
    /// Localized user-friendly explanation.
    pub message: SharedString,
}

/// Formats a [`DocumentError`] into a localized, user-friendly explanation using `ordia-i18n`.
#[must_use]
pub fn format_friendly_open_error(file_name: &str, err: &DocumentError) -> SharedString {
    match err {
        DocumentError::EmptyFile { .. } => {
            SharedString::from(ordia_i18n::t!("open_flow.err_empty", "name" => file_name))
        }
        DocumentError::CorruptArchive { .. } => {
            SharedString::from(ordia_i18n::t!("open_flow.err_corrupt", "name" => file_name))
        }
        DocumentError::LegacyBinaryXls { .. } => {
            SharedString::from(ordia_i18n::t!("open_flow.err_legacy_xls", "name" => file_name))
        }
        DocumentError::UnsupportedFormat { detail, .. } => SharedString::from(
            ordia_i18n::t!("open_flow.err_unsupported", "name" => file_name, "detail" => detail),
        ),
        DocumentError::Io { source, .. } => SharedString::from(
            ordia_i18n::t!("open_flow.err_io", "name" => file_name, "detail" => source.to_string()),
        ),
        DocumentError::ParseError { detail, .. } | DocumentError::SaveFailed { detail, .. } => {
            SharedString::from(
                ordia_i18n::t!("open_flow.err_parse", "name" => file_name, "detail" => detail),
            )
        }
        _ => SharedString::from(ordia_i18n::t!("open_flow.err_corrupt", "name" => file_name)),
    }
}

impl MainWindow {
    /// Returns the active background loading progress banner, if a file is currently loading asynchronously.
    #[must_use]
    pub fn loading_banner(&self) -> Option<&LoadingBannerState> {
        self.loading_banner.as_ref()
    }

    /// Returns the currently displayed friendly file-open error banner, if any.
    #[must_use]
    pub fn open_error_banner(&self) -> Option<&OpenErrorBanner> {
        self.open_error_banner.as_ref()
    }

    /// Dismisses the file-open error banner.
    pub fn dismiss_open_error(&mut self, cx: &mut Context<Self>) {
        if self.open_error_banner.take().is_some() {
            cx.notify();
        }
    }

    /// Lists unclosed crash-recovery sessions detected on startup (`M1-07`).
    #[must_use]
    pub fn unclean_recovery_sessions(&self) -> &[UncleanSessionInfo] {
        &self.recovery_sessions
    }

    /// Replays all detected crash-recovery journals into fresh/reopened workbook tabs and clears their locks.
    pub fn recover_unclean_sessions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mgr = JournalRecoveryManager::with_data_dir(self.settings_store.dir());
        let sessions = std::mem::take(&mut self.recovery_sessions);
        for sess in sessions {
            if let Ok(rec) = mgr.recover_session::<Transaction>(&sess.meta.session_id) {
                let mut doc = sess
                    .meta
                    .source_path
                    .as_deref()
                    .and_then(|p| SheetDocument::open_auto(p).ok())
                    .or_else(|| SheetDocument::new_empty(&sess.meta.document_title).ok())
                    .unwrap_or_else(|| {
                        SheetDocument::new_empty("Recovered.xlsx")
                            .expect("create fallback recovered workbook")
                    });
                let op_count = rec.operations.len().to_string();
                for (_seq, tx) in rec.operations {
                    let _ = doc.execute_transaction(tx);
                }
                let id = self.next_tab_id;
                self.next_tab_id += 1;
                let mut tab = WorkbookTab::new(
                    id,
                    sess.meta.document_title.clone(),
                    ordia_i18n::t!("recovery.recovered_summary", "ops" => op_count),
                );
                if let Some(p) = sess.meta.source_path {
                    tab = tab.with_path(p);
                }
                tab.set_dirty(true);
                self.push_document_tab(tab, doc, window, cx);
            }
            let _ = mgr.discard_session(&sess.meta.session_id);
        }
        cx.notify();
    }

    /// Discards all pending crash-recovery sessions without replaying them.
    pub fn discard_unclean_sessions(&mut self, cx: &mut Context<Self>) {
        let mgr = JournalRecoveryManager::with_data_dir(self.settings_store.dir());
        for sess in std::mem::take(&mut self.recovery_sessions) {
            let _ = mgr.discard_session(&sess.meta.session_id);
        }
        cx.notify();
    }

    /// Opens `path` synchronously on the caller thread (suitable for small files), surfacing a friendly
    /// localized error banner instead of crashing if the file is corrupt or unsupported.
    pub fn open_file_sync(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(existing_ix) = self
            .tabs
            .iter()
            .position(|t| t.path().is_some_and(|p| p == path))
        {
            self.select_tab(existing_ix, cx);
            return;
        }
        let file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("Workbook.xlsx")
            .to_string();
        match SheetDocument::open_auto(path) {
            Ok(doc) => {
                self.open_error_banner = None;
                self.accept_opened_document(file_name, path.to_path_buf(), doc, window, cx);
            }
            Err(err) => {
                let msg = format_friendly_open_error(&file_name, &err);
                self.open_error_banner = Some(OpenErrorBanner {
                    file_name: SharedString::from(file_name),
                    message: msg,
                });
                cx.notify();
            }
        }
    }
}

impl MainWindow {
    /// Opens `path` asynchronously on GPUI's background worker pool, immediately displaying a non-blocking
    /// progress banner so 1,000,000-row workbooks load without freezing UI frames.
    pub fn open_file_async(
        &mut self,
        path: impl Into<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path_buf = path.into();
        if let Some(existing_ix) = self
            .tabs
            .iter()
            .position(|t| t.path().is_some_and(|p| *p == path_buf))
        {
            self.select_tab(existing_ix, cx);
            return;
        }

        let file_name = path_buf
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("Workbook.xlsx")
            .to_string();

        self.load_ticket = self.load_ticket.wrapping_add(1);
        let ticket = self.load_ticket;
        self.open_error_banner = None;
        self.loading_banner = Some(LoadingBannerState {
            file_name: SharedString::from(file_name.clone()),
            path: path_buf.clone(),
            stage: LoadStage::DetectingFormat,
            percent: 5,
        });
        cx.notify();

        let shared_prog = Arc::new(Mutex::new(LoadProgress::new(
            LoadStage::DetectingFormat,
            0,
            None,
        )));
        let prog_writer = shared_prog.clone();
        let path_for_bg = path_buf.clone();
        let bg_task = cx.background_executor().spawn(async move {
            SheetDocument::open_from_path(&path_for_bg, &mut |p| {
                if let Ok(mut guard) = prog_writer.lock() {
                    *guard = p;
                }
            })
        });

        let prog_reader = shared_prog;
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(25))
                    .await;
                let snap = prog_reader
                    .lock()
                    .map(|g| *g)
                    .unwrap_or_else(|_| LoadProgress::new(LoadStage::ParsingWorkbook, 0, None));
                let keep_going = this
                    .update_in(cx, |win, _, cx| {
                        if win.load_ticket != ticket || win.loading_banner.is_none() {
                            return false;
                        }
                        if let Some(banner) = win.loading_banner.as_mut()
                            && (banner.stage != snap.stage || banner.percent != snap.percent())
                        {
                            banner.stage = snap.stage;
                            banner.percent = snap.percent();
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        })
        .detach();

        self.active_load_task = Some(cx.spawn_in(window, async move |this, cx| {
            let res = bg_task.await;
            let _ = this.update_in(cx, |win, window, cx| {
                if win.load_ticket != ticket {
                    return;
                }
                win.loading_banner = None;
                match res {
                    Ok(doc) => {
                        win.accept_opened_document(file_name, path_buf, doc, window, cx);
                    }
                    Err(err) => {
                        let msg = format_friendly_open_error(&file_name, &err);
                        win.open_error_banner = Some(OpenErrorBanner {
                            file_name: SharedString::from(file_name),
                            message: msg,
                        });
                        cx.notify();
                    }
                }
            });
        }));
    }

    pub(crate) fn accept_opened_document(
        &mut self,
        file_name: String,
        path: PathBuf,
        doc: SheetDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let sheet_count = doc.sheet_count().to_string();
        let tab = WorkbookTab::new(
            id,
            file_name.clone(),
            ordia_i18n::t!("shell.summary_local_sheets", "count" => sheet_count),
        )
        .with_path(path.clone());
        self.recent_files.retain(|r| *r.path() != path);
        self.recent_files.insert(
            0,
            RecentFileEntry::new(file_name, path, ordia_i18n::t!("shell.recent_just_now")),
        );
        let max_len = self.settings.general.max_recent_files.max(1);
        if self.recent_files.len() > max_len {
            self.recent_files.truncate(max_len);
        }
        self.persist_shell_state();
        self.push_document_tab(tab, doc, window, cx);
    }

    pub(crate) fn render_open_banners(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let recovery = if self.recovery_sessions.is_empty() {
            None
        } else {
            let count = self.recovery_sessions.len().to_string();
            let names = self
                .recovery_sessions
                .iter()
                .map(|s| s.meta.document_title.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            Some(ordia_i18n::t!(
                "recovery.banner_message",
                "count" => count,
                "names" => names
            ))
        };
        let loading = self.loading_banner.clone();
        let err = self.open_error_banner.clone();

        v_flex()
            .w_full()
            .when_some(recovery, |col, msg| {
                col.child(
                    h_flex()
                        .id("crash-recovery-banner")
                        .w_full()
                        .justify_between()
                        .px_4()
                        .py_2()
                        .gap_3()
                        .border_b_1()
                        .border_color(cx.theme().primary)
                        .bg(cx.theme().secondary)
                        .text_xs()
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    div()
                                        .font_semibold()
                                        .text_color(cx.theme().primary)
                                        .child(ordia_i18n::t!("recovery.banner_title")),
                                )
                                .child(msg),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new("recover-sessions-btn")
                                        .primary()
                                        .small()
                                        .label(ordia_i18n::t!("recovery.recover_action"))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.recover_unclean_sessions(window, cx);
                                        })),
                                )
                                .child(
                                    Button::new("discard-sessions-btn")
                                        .outline()
                                        .small()
                                        .label(ordia_i18n::t!("recovery.discard_action"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.discard_unclean_sessions(cx);
                                        })),
                                ),
                        ),
                )
            })
            .when_some(loading, |col, b| {
                let pct_str = b.percent.to_string();
                let label = match b.stage {
                    LoadStage::DetectingFormat => ordia_i18n::t!(
                        "open_flow.progress_detecting",
                        "name" => b.file_name.as_ref(),
                        "percent" => pct_str
                    ),
                    LoadStage::ReadingBytes => ordia_i18n::t!(
                        "open_flow.progress_reading",
                        "name" => b.file_name.as_ref(),
                        "percent" => pct_str
                    ),
                    _ => ordia_i18n::t!(
                        "open_flow.progress_parsing",
                        "name" => b.file_name.as_ref(),
                        "percent" => pct_str
                    ),
                };
                col.child(
                    h_flex()
                        .id("open-loading-banner")
                        .w_full()
                        .px_4()
                        .py_1p5()
                        .gap_3()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().secondary)
                        .text_xs()
                        .child(label)
                        .child(
                            div()
                                .w(px(140.))
                                .h(px(6.))
                                .rounded_full()
                                .bg(cx.theme().border)
                                .child(
                                    div()
                                        .w(px(1.4 * f32::from(b.percent)))
                                        .h_full()
                                        .rounded_full()
                                        .bg(cx.theme().primary),
                                ),
                        ),
                )
            })
            .when_some(err, |col, e| {
                col.child(
                    h_flex()
                        .id("open-error-banner")
                        .w_full()
                        .justify_between()
                        .px_4()
                        .py_2()
                        .gap_3()
                        .border_b_1()
                        .border_color(cx.theme().danger)
                        .bg(cx.theme().popover)
                        .text_xs()
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    div()
                                        .font_semibold()
                                        .text_color(cx.theme().danger)
                                        .child(ordia_i18n::t!("open_flow.error_title")),
                                )
                                .child(e.message),
                        )
                        .child(
                            Button::new("dismiss-open-error")
                                .outline()
                                .small()
                                .label(ordia_i18n::t!("open_flow.error_dismiss"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.dismiss_open_error(cx);
                                })),
                        ),
                )
            })
    }
}
