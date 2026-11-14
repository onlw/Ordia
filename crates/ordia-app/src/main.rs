//! Main executable entry point for the Ordia desktop client.

use clap::Parser;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Command-line flags for the Ordia desktop application.
#[derive(Debug, Parser)]
#[command(name = "ordia-app", about = "Ordia cross-platform office suite")]
struct CliArgs {
    /// Optional `.xlsx` workbook path to open immediately on startup.
    #[arg(long)]
    open: Option<PathBuf>,

    /// Renders initial frames and exits cleanly (used for CI smoke checks).
    #[arg(long)]
    smoke_test: bool,

    /// Runs an automated scroll benchmark across `<FRAMES>` frames on the open workbook and exits.
    #[arg(long)]
    bench_scroll: Option<usize>,
}

fn main() {
    let args = CliArgs::parse();

    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            ordia_app::register_bindings_and_menus(cx);

            let t0 = Instant::now();
            let (_handle, main_window) =
                match ordia_app::MainWindow::open_with_path(args.open.as_deref(), cx) {
                    Ok(opened) => opened,
                    Err(err) => {
                        eprintln!("failed to open Ordia main window: {err}");
                        cx.quit();
                        return;
                    }
                };
            cx.activate(true);
            let open_elapsed = t0.elapsed();

            if let Some(frames) = args.bench_scroll {
                let sheet_entity = main_window.read(cx).active_sheet_view().cloned();
                cx.spawn(async move |cx| {
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
