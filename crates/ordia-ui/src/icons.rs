//! Semantic icon set for Ordia (`PLAN.md` §5.2), backed by GPUI Kit's Lucide assets.

use gpui_kit::SharedString;
use gpui_kit::assets::{IconName, IconNamed};
use gpui_kit::component::Icon;

/// Product-semantic icons used across Ordia's shell, toolbars, inspectors, and spreadsheet views.
///
/// Implements [`IconNamed`] so any `OrdiaIcon` can be passed directly to `Icon::new(icon)` or
/// converted via [`OrdiaIcon::into_icon`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OrdiaIcon {
    /// Undo last transaction (`Cmd/Ctrl+Z`).
    Undo,
    /// Redo last undone transaction (`Cmd/Ctrl+Shift+Z` / `Ctrl+Y`).
    Redo,
    /// Save active workbook (`Cmd/Ctrl+S`).
    Save,
    /// Open workbook from disk (`Cmd/Ctrl+O`).
    Open,
    /// Find / search (`Cmd/Ctrl+F`).
    Search,
    /// Open command palette (`Cmd/Ctrl+K`).
    CommandPalette,
    /// Toggle bold font weight (`Cmd/Ctrl+B`).
    Bold,
    /// Toggle italic font style (`Cmd/Ctrl+I`).
    Italic,
    /// Toggle underline decoration (`Cmd/Ctrl+U`).
    Underline,
    /// Align cell content to leading/left edge.
    AlignLeft,
    /// Center cell content horizontally.
    AlignCenter,
    /// Align cell content to trailing/right edge.
    AlignRight,
    /// Cell background fill color bucket.
    FillColor,
    /// Cell foreground text color palette.
    TextColor,
    /// Cell border configuration.
    Borders,
    /// Insert function / formula bar (`Σ` / `fx`).
    Formula,
    /// Insert or configure chart.
    Chart,
    /// AutoFilter column dropdown.
    Filter,
    /// Sort range ascending / descending.
    Sort,
    /// Open application preferences (`Cmd/Ctrl+,`).
    Settings,
    /// Switch to Light appearance.
    LightMode,
    /// Switch to Dark appearance.
    DarkMode,
    /// Create new workbook or worksheet (`+`).
    Add,
    /// Close tab, panel, or banner (`×`).
    Close,
    /// Toggle right contextual inspector panel.
    Inspector,
    /// Spreadsheet workbook file glyph.
    Spreadsheet,
    /// Export active document to PDF.
    ExportPdf,
}

impl OrdiaIcon {
    /// Complete ordered catalog of Ordia semantic icons for gallery inspection and tests.
    pub const ALL: &'static [Self] = &[
        Self::Undo,
        Self::Redo,
        Self::Save,
        Self::Open,
        Self::Search,
        Self::CommandPalette,
        Self::Bold,
        Self::Italic,
        Self::Underline,
        Self::AlignLeft,
        Self::AlignCenter,
        Self::AlignRight,
        Self::FillColor,
        Self::TextColor,
        Self::Borders,
        Self::Formula,
        Self::Chart,
        Self::Filter,
        Self::Sort,
        Self::Settings,
        Self::LightMode,
        Self::DarkMode,
        Self::Add,
        Self::Close,
        Self::Inspector,
        Self::Spreadsheet,
        Self::ExportPdf,
    ];

    /// Maps this semantic icon to its underlying Lucide [`IconName`].
    pub const fn asset_icon(self) -> IconName {
        match self {
            Self::Undo => IconName::Undo,
            Self::Redo => IconName::Redo,
            Self::Save => IconName::Save,
            Self::Open => IconName::FolderOpen,
            Self::Search => IconName::Search,
            Self::CommandPalette => IconName::Command,
            Self::Bold => IconName::Bold,
            Self::Italic => IconName::Italic,
            Self::Underline => IconName::Underline,
            Self::AlignLeft => IconName::AlignHorizontalJustifyStart,
            Self::AlignCenter => IconName::AlignHorizontalJustifyCenter,
            Self::AlignRight => IconName::AlignHorizontalJustifyEnd,
            Self::FillColor => IconName::PaintBucket,
            Self::TextColor => IconName::Palette,
            Self::Borders => IconName::Grid3x3,
            Self::Formula => IconName::Sigma,
            Self::Chart => IconName::ChartColumn,
            Self::Filter => IconName::ListFilter,
            Self::Sort => IconName::ArrowUpDown,
            Self::Settings => IconName::Settings,
            Self::LightMode => IconName::Sun,
            Self::DarkMode => IconName::Moon,
            Self::Add => IconName::Plus,
            Self::Close => IconName::X,
            Self::Inspector => IconName::PanelRight,
            Self::Spreadsheet => IconName::FileSpreadsheet,
            Self::ExportPdf => IconName::Download,
        }
    }

    /// Stable identifier name of the semantic icon.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::Save => "save",
            Self::Open => "open",
            Self::Search => "search",
            Self::CommandPalette => "command-palette",
            Self::Bold => "bold",
            Self::Italic => "italic",
            Self::Underline => "underline",
            Self::AlignLeft => "align-left",
            Self::AlignCenter => "align-center",
            Self::AlignRight => "align-right",
            Self::FillColor => "fill-color",
            Self::TextColor => "text-color",
            Self::Borders => "borders",
            Self::Formula => "formula",
            Self::Chart => "chart",
            Self::Filter => "filter",
            Self::Sort => "sort",
            Self::Settings => "settings",
            Self::LightMode => "light-mode",
            Self::DarkMode => "dark-mode",
            Self::Add => "add",
            Self::Close => "close",
            Self::Inspector => "inspector",
            Self::Spreadsheet => "spreadsheet",
            Self::ExportPdf => "export-pdf",
        }
    }

    /// Builds a GPUI Kit [`Icon`] element from this semantic icon.
    pub fn into_icon(self) -> Icon {
        Icon::new(self)
    }
}

impl IconNamed for OrdiaIcon {
    fn path(self) -> SharedString {
        self.asset_icon().path()
    }
}
