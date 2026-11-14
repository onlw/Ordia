//! Main executable entry point for the Ordia desktop client.

use clap::Parser;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Command-line flags for the Ordia desktop application.
#[derive(Debug, Parser)]
#[command(name = "ordia-app", about = "Ordia cross-platform office suite")]
struct CliArgs {
    /// Positional spreadsheet path passed by OS file associations (`"%1"` on Windows / CLI).
    #[arg(value_name = "FILE")]
    file: Option<PathBuf>,

    /// Optional `.xlsx`/`.csv` workbook path to open immediately on startup.
    #[arg(long)]
    open: Option<PathBuf>,

    /// Renders initial frames and exits cleanly (used for CI smoke checks).
    #[arg(long)]
    smoke_test: bool,

    /// Runs an automated scroll benchmark across `<FRAMES>` frames on the open workbook and exits.
    #[arg(long)]
    bench_scroll: Option<usize>,

    /// Exports the active worksheet's used range to `<EXPORT_PDF>` with subsetted CJK fonts and exits.
    #[arg(long)]
    export_pdf: Option<PathBuf>,

    /// Opens the interactive Ordia Design System Gallery window (Light + Dark tokens and components).
    #[arg(long)]
    gallery: bool,
}

fn file_url_to_path(url: &str) -> Option<PathBuf> {
    let stripped = url.strip_prefix("file://")?;
    let host_stripped = stripped.strip_prefix("localhost").unwrap_or(stripped);
    Some(PathBuf::from(host_stripped))
}

fn main() {
    let args = CliArgs::parse();
    let app_data_dir = ordia_core::SettingsStore::default_app_data_dir();
    let _log_guard = ordia_core::init_rolling_file_logger(&app_data_dir, 7);
    ordia_core::install_panic_crash_hook(&app_data_dir);
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        "starting Ordia desktop client"
    );
    let initial_open = args.open.clone().or_else(|| args.file.clone());

    if let Some(pdf_out) = args.export_pdf.as_ref() {
        let doc = match initial_open.as_deref() {
            Some(p) => {
                ordia_sheet_model::SheetDocument::open_auto(p).map_err(|e| anyhow::anyhow!("{e}"))
            }
            None => ordia_sheet_model::SheetDocument::new_demo("Budget_2026")
                .map_err(|e| anyhow::anyhow!("{e}")),
        }
        .unwrap_or_else(|err| {
            eprintln!("failed to open workbook for PDF export: {err}");
            std::process::exit(1);
        });
        let range = doc
            .active_sheet_info()
            .and_then(|s| s.used_range)
            .unwrap_or_else(|| {
                ordia_sheet_model::CellRange::new(
                    ordia_sheet_model::CellCoord::new(0, 0),
                    ordia_sheet_model::CellCoord::new(10, 5),
                )
            });
        let fonts = ordia_text::FontCatalog::from_system();
        let opts = ordia_render_pdf::PdfExportOptions::default();
        if let Err(err) =
            ordia_render_pdf::export_sheet_range_to_pdf_file(&doc, range, &opts, &fonts, pdf_out)
        {
            eprintln!("PDF export failed: {err}");
            std::process::exit(1);
        }
        return;
    }

    let finder_urls: std::sync::Arc<std::sync::Mutex<Vec<PathBuf>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let finder_urls_writer = finder_urls.clone();

    let app = gpui_kit::application().with_assets(gpui_kit::assets::AllAssets);
    app.on_open_urls(move |urls| {
        if let Ok(mut queue) = finder_urls_writer.lock() {
            for u in urls {
                if let Some(p) = file_url_to_path(&u) {
                    queue.push(p);
                }
            }
        }
    });
    app.run(move |cx| {
            gpui_kit::init(cx);
            ordia_app::register_bindings_and_menus(cx);

            if args.gallery {
                if let Err(err) = ordia_app::open_design_gallery(cx) {
                    eprintln!("failed to open Ordia design gallery: {err}");
                    cx.quit();
                    return;
                }
                cx.activate(true);
                if args.smoke_test {
                    cx.spawn(async move |cx| {
                        cx.background_executor()
                            .timer(Duration::from_millis(250))
                            .await;
                        cx.update(|cx| cx.quit());
                    })
                    .detach();
                }
                return;
            }

            let queued_from_os = finder_urls
                .lock()
                .ok()
                .and_then(|mut q| if q.is_empty() { None } else { Some(q.remove(0)) });
            let startup_path = initial_open.or(queued_from_os);

            let t0 = Instant::now();
            let (_handle, main_window) =
                match ordia_app::MainWindow::open_with_path(startup_path.as_deref(), cx) {
                    Ok(opened) => opened,
                    Err(err) => {
                        eprintln!("failed to open Ordia main window: {err}");
                        cx.quit();
                        return;
                    }
                };
            cx.activate(true);

            if let Some(frames) = args.bench_scroll {
                cx.spawn(async move |cx| {
                    // Wait for background workbook loading (if `>= 2 MiB`) to complete while UI stays live.
                    loop {
                        let still_loading = cx.update(|cx| main_window.read(cx).loading_banner().is_some());
                        if !still_loading {
                            break;
                        }
                        cx.background_executor()
                            .timer(Duration::from_millis(20))
                            .await;
                    }
                    let open_elapsed = t0.elapsed();
                    let sheet_entity = cx.update(|cx| main_window.read(cx).active_sheet_view().cloned());

                    // Allow initial window presentation to settle.
                    cx.background_executor()
                        .timer(Duration::from_millis(200))
                        .await;

                    if let Some(ref view) = sheet_entity {
                        cx.update(|cx| {
                            view.update(cx, |state, cx| {
                                state.toggle_perf_overlay(cx);
                                state.set_scroll_offset(0.0, 500_000.0 * 25.0, cx);
                            });
                        });
                        cx.background_executor()
                            .timer(Duration::from_millis(50))
                            .await;
                        cx.update(|cx| {
                            view.update(cx, |state, _cx| {
                                state.reset_frame_stats();
                            });
                        });

                        // Phase 1: Rapid continuous trackpad flick (+150 px/frame = 6 rows/frame at row 500,000).
                        for i in 0..frames {
                            let target_y = 500_000.0 * 25.0 + ((i + 1) as f64) * 150.0;
                            cx.update(|cx| {
                                view.update(cx, |state, cx| {
                                    state.set_scroll_offset(0.0, target_y, cx);
                                });
                            });
                            cx.background_executor()
                                .timer(Duration::from_millis(16))
                                .await;
                        }
                        cx.background_executor()
                            .timer(Duration::from_millis(40))
                            .await;
                        cx.update(|cx| {
                            let stats = view.read(cx).frame_stats();
                            println!(
                                "BENCH_SCROLL_FLICK open_ms={:.2} samples={} avg_ms={:.3} p50_ms={:.3} p95_ms={:.3} p99_ms={:.3} max_ms={:.3}",
                                open_elapsed.as_secs_f64() * 1000.0,
                                stats.total_frames,
                                stats.avg_ms,
                                stats.p50_ms,
                                stats.p95_ms,
                                stats.p99_ms,
                                stats.max_ms,
                            );
                            view.update(cx, |state, _cx| {
                                state.reset_frame_stats();
                            });
                        });

                        // Phase 2: Full-workbook scrollbar scrub across 1,000,000 rows (+122,500 px/frame).
                        let total_px = 24_500_000.0_f64;
                        let step_y = total_px / (frames.max(1) as f64);
                        for i in 0..frames {
                            let target_y = (i as f64 + 1.0) * step_y;
                            cx.update(|cx| {
                                view.update(cx, |state, cx| {
                                    state.set_scroll_offset(0.0, target_y, cx);
                                });
                            });
                            cx.background_executor()
                                .timer(Duration::from_millis(16))
                                .await;
                        }

                        // Wait one final frame so the last paint records its duration.
                        cx.background_executor()
                            .timer(Duration::from_millis(50))
                            .await;

                        cx.update(|cx| {
                            let stats = view.read(cx).frame_stats();
                            let final_y = view.read(cx).scroll_y();
                            println!(
                                "BENCH_SCROLL_SCRUB open_ms={:.2} samples={} avg_ms={:.3} p50_ms={:.3} p95_ms={:.3} p99_ms={:.3} max_ms={:.3} final_scroll_y={:.1}",
                                open_elapsed.as_secs_f64() * 1000.0,
                                stats.total_frames,
                                stats.avg_ms,
                                stats.p50_ms,
                                stats.p95_ms,
                                stats.p99_ms,
                                stats.max_ms,
                                final_y,
                            );
                        });
                    }
                    cx.update(|cx| cx.quit());
                })
                .detach();
            } else if args.smoke_test {
                cx.spawn(async move |cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(250))
                        .await;
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
}
