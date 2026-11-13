//! Main executable entry point for the Ordia desktop client.

use std::time::Duration;

fn main() {
    let smoke_test = std::env::args().any(|arg| arg == "--smoke-test");

    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            ordia_app::register_bindings_and_menus(cx);

            if let Err(err) = ordia_app::MainWindow::open(cx) {
                eprintln!("failed to open Ordia main window: {err}");
                cx.quit();
                return;
            }

            if smoke_test {
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
