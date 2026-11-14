//! Windows-specific platform integrations (`CF_HTML` + `CF_UNICODETEXT` clipboard).

use ordia_sheet_model::ClipboardPayload;
use ordia_sheet_view::SheetClipboardBridge;

/// Native Windows clipboard bridge writing and reading both `HTML Format` (`CF_HTML`)
/// and `CF_UNICODETEXT` flavors for bidirectional Excel / WPS interop.
#[derive(Default)]
pub struct WindowsClipboardBridge;

impl SheetClipboardBridge for WindowsClipboardBridge {
    fn write_payload(&self, payload: &ClipboardPayload) {
        if let Ok(mut cb) = arboard::Clipboard::new() {
            let _ = cb
                .set()
                .html(payload.html.clone(), Some(payload.tsv.clone()));
        }
    }

    fn read_payload(&self) -> (Option<String>, Option<String>) {
        let Ok(mut cb) = arboard::Clipboard::new() else {
            return (None, None);
        };
        let html = cb.get().html().ok();
        let tsv = cb.get_text().ok();
        (html, tsv)
    }
}
