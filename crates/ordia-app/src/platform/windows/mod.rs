//! Windows-specific platform integrations (`CF_HTML` + `CF_UNICODETEXT` clipboard & `HKCU` file associations).

use std::path::Path;
use std::process::Command;

use ordia_sheet_model::ClipboardPayload;
use ordia_sheet_view::SheetClipboardBridge;

use super::SUPPORTED_FILE_ASSOCIATIONS;

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

/// Registers per-user (`HKCU\Software\Classes`) OpenWith ProgIDs for `.xlsx`, `.xlsm`, `.csv`, `.tsv`.
pub fn register_user_file_associations(exe_path: &Path) -> std::io::Result<()> {
    let cmd_val = format!("\"{}\" \"%1\"", exe_path.display());
    for spec in SUPPORTED_FILE_ASSOCIATIONS {
        let prog_key = format!(r"HKCU\Software\Classes\{}", spec.windows_prog_id);
        let open_key = format!(r"{prog_key}\shell\open\command");
        let ext_key = format!(r"HKCU\Software\Classes\.{}\OpenWithProgids", spec.extension);
        let _ = Command::new("reg")
            .args(["add", &prog_key, "/ve", "/d", spec.description, "/f"])
            .status()?;
        let _ = Command::new("reg")
            .args(["add", &open_key, "/ve", "/d", &cmd_val, "/f"])
            .status()?;
        let _ = Command::new("reg")
            .args([
                "add",
                &ext_key,
                "/v",
                spec.windows_prog_id,
                "/t",
                "REG_NONE",
                "/f",
            ])
            .status()?;
    }
    Ok(())
}

/// Opens `dir` inside Windows File Explorer (`explorer.exe <dir>`).
pub fn reveal_directory(dir: &Path) -> std::io::Result<()> {
    let _ = Command::new("explorer").arg(dir).spawn()?;
    Ok(())
}
