//! macOS-specific platform integrations (`NSPasteboard` dual HTML + TSV clipboard & `Info.plist` document types).

use ordia_sheet_model::ClipboardPayload;
use ordia_sheet_view::SheetClipboardBridge;

/// Native macOS `NSPasteboard` clipboard bridge writing and reading both `public.html`
/// and `public.utf8-plain-text` flavors for bidirectional Excel / WPS / Numbers interop.
#[derive(Default)]
pub struct MacosClipboardBridge;

impl SheetClipboardBridge for MacosClipboardBridge {
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

/// Returns the Apple bundle `Info.plist` XML containing `CFBundleDocumentTypes` for `.xlsx`, `.xlsm`, `.csv`, `.tsv`.
#[must_use]
pub const fn info_plist_xml() -> &'static str {
    super::macos_info_plist_template()
}
