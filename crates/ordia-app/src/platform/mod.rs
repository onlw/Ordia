//! Cross-platform OS integration dispatch (`macos` / `windows` / fallback) and file associations (`M1-06`).

use std::path::Path;
use std::sync::Arc;

use ordia_sheet_view::SheetClipboardBridge;

/// macOS platform integrations (`NSPasteboard` clipboard + `Info.plist` UTI document types).
#[cfg(target_os = "macos")]
pub mod macos;

/// Windows platform integrations (`CF_HTML` clipboard + `HKCU\Software\Classes` ProgID registration).
#[cfg(target_os = "windows")]
pub mod windows;

/// Descriptor for an operating-system document file association registered by Ordia.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct FileAssociationSpec {
    /// Lowercase extension without leading dot (`"xlsx"`, `"xlsm"`, `"csv"`, `"tsv"`).
    pub extension: &'static str,
    /// Standard IANA MIME type.
    pub mime_type: &'static str,
    /// macOS Uniform Type Identifier (`LSItemContentTypes`).
    pub macos_uti: &'static str,
    /// Windows ProgID (`HKCU\Software\Classes\<ProgID>`).
    pub windows_prog_id: &'static str,
    /// Human-readable English type description for OS registries.
    pub description: &'static str,
}

/// Authoritative list of file extensions associated with Ordia Spreadsheets on macOS and Windows.
pub const SUPPORTED_FILE_ASSOCIATIONS: &[FileAssociationSpec] = &[
    FileAssociationSpec {
        extension: "xlsx",
        mime_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        macos_uti: "org.openxmlformats.spreadsheetml.sheet",
        windows_prog_id: "Ordia.Spreadsheet.xlsx",
        description: "Ordia OpenXML Spreadsheet (.xlsx)",
    },
    FileAssociationSpec {
        extension: "xlsm",
        mime_type: "application/vnd.ms-excel.sheet.macroEnabled.12",
        macos_uti: "org.openxmlformats.spreadsheetml.sheet.macroenabled",
        windows_prog_id: "Ordia.Spreadsheet.xlsm",
        description: "Ordia Macro-Enabled Spreadsheet (.xlsm)",
    },
    FileAssociationSpec {
        extension: "csv",
        mime_type: "text/csv",
        macos_uti: "public.comma-separated-values-text",
        windows_prog_id: "Ordia.Spreadsheet.csv",
        description: "Ordia Comma-Separated Values (.csv)",
    },
    FileAssociationSpec {
        extension: "tsv",
        mime_type: "text/tab-separated-values",
        macos_uti: "public.tab-separated-values-text",
        windows_prog_id: "Ordia.Spreadsheet.tsv",
        description: "Ordia Tab-Separated Values (.tsv)",
    },
];

/// Embedded macOS `Info.plist` template declaring `CFBundleDocumentTypes` for `.xlsx`, `.xlsm`, `.csv`, `.tsv`.
#[must_use]
pub const fn macos_info_plist_template() -> &'static str {
    include_str!("../../../../assets/macos/Info.plist")
}

/// Generates a Windows `HKCU\Software\Classes` `.reg` script binding [`SUPPORTED_FILE_ASSOCIATIONS`]
/// to `exe_path` without requiring Administrator privileges.
#[must_use]
pub fn generate_windows_hkcu_reg_script(exe_path: &Path) -> String {
    let escaped_exe = exe_path.to_string_lossy().replace('\\', "\\\\");
    let mut out = String::from("Windows Registry Editor Version 5.00\r\n\r\n");
    for spec in SUPPORTED_FILE_ASSOCIATIONS {
        out.push_str(&format!(
            "[HKEY_CURRENT_USER\\Software\\Classes\\{}]\r\n@=\"{}\"\r\n",
            spec.windows_prog_id, spec.description
        ));
        out.push_str(&format!(
            "[HKEY_CURRENT_USER\\Software\\Classes\\{}\\shell\\open\\command]\r\n@=\"\\\"{}\\\" \\\"%1\\\"\"\r\n\r\n",
            spec.windows_prog_id, escaped_exe
        ));
        out.push_str(&format!(
            "[HKEY_CURRENT_USER\\Software\\Classes\\.{}\\OpenWithProgids]\r\n\"{}\"=\"\"\r\n\r\n",
            spec.extension, spec.windows_prog_id
        ));
    }
    out
}

/// Opens `dir` in the host operating system's native file manager (Finder / Explorer).
pub fn reveal_directory_in_os(dir: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        macos::reveal_directory(dir)
    }
    #[cfg(target_os = "windows")]
    {
        windows::reveal_directory(dir)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(dir).spawn()?;
        Ok(())
    }
}

/// Constructs the native OS [`SheetClipboardBridge`] for the active host platform.
pub fn system_clipboard_bridge() -> Arc<dyn SheetClipboardBridge> {
    #[cfg(target_os = "macos")]
    {
        Arc::new(macos::MacosClipboardBridge)
    }
    #[cfg(target_os = "windows")]
    {
        Arc::new(windows::WindowsClipboardBridge)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Arc::new(ordia_sheet_view::InMemoryClipboardBridge::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_plist_and_windows_registry_cover_xlsx_and_csv() {
        let plist = macos_info_plist_template();
        assert!(plist.contains("<string>xlsx</string>"));
        assert!(plist.contains("<string>csv</string>"));
        assert!(plist.contains("org.openxmlformats.spreadsheetml.sheet"));
        assert!(plist.contains("public.comma-separated-values-text"));

        let reg =
            generate_windows_hkcu_reg_script(Path::new(r"C:\Program Files\Ordia\ordia-app.exe"));
        assert!(reg.contains(r"[HKEY_CURRENT_USER\Software\Classes\.xlsx\OpenWithProgids]"));
        assert!(reg.contains(r"[HKEY_CURRENT_USER\Software\Classes\.csv\OpenWithProgids]"));
        assert!(reg.contains(r#"\"C:\\Program Files\\Ordia\\ordia-app.exe\" \"%1\""#));
    }
}
