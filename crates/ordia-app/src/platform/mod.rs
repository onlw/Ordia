//! Cross-platform OS integration dispatch (`macos` / `windows` / fallback).

use std::sync::Arc;

use ordia_sheet_view::SheetClipboardBridge;

/// macOS platform integrations.
#[cfg(target_os = "macos")]
pub mod macos;

/// Windows platform integrations.
#[cfg(target_os = "windows")]
pub mod windows;

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
