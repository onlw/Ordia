//! Domain data structures for the Ordia application shell (`M1-02`).

use std::path::PathBuf;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    Context, IntoElement, ParentElement as _, Render, SharedString, Styled as _, Window, div, px,
};

/// Identifies navigation sections in the shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarSection {
    /// Home / recent workbooks and quick-open list.
    Recent,
    /// Built-in spreadsheet templates.
    Templates,
    /// Application preferences.
    Settings,
}

/// Metadata for an open document tab in [`crate::MainWindow`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkbookTab {
    id: u64,
    title: SharedString,
    summary: SharedString,
    path: Option<PathBuf>,
    dirty: bool,
}

impl WorkbookTab {
    /// Creates a workbook tab descriptor with a stable numeric identifier.
    pub fn new(id: u64, title: impl Into<SharedString>, summary: impl Into<SharedString>) -> Self {
        Self {
            id,
            title: title.into(),
            summary: summary.into(),
            path: None,
            dirty: false,
        }
    }

    /// Attaches an on-disk file path to the tab descriptor.
    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Marks whether the tab has unsaved modifications.
    pub fn with_dirty(mut self, dirty: bool) -> Self {
        self.dirty = dirty;
        self
    }

    /// Stable numeric identifier for `ElementId` keying across tab reordering.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Display title shown in the tab bar.
    pub fn title(&self) -> &SharedString {
        &self.title
    }

    /// Updates the display title of the tab.
    pub fn set_title(&mut self, title: impl Into<SharedString>) {
        self.title = title.into();
    }

    /// Description rendered in the inspector and status bar.
    pub fn summary(&self) -> &SharedString {
        &self.summary
    }

    /// Backing file path on disk, if saved or opened from disk.
    pub fn path(&self) -> Option<&PathBuf> {
        self.path.as_ref()
    }

    /// Whether the document has unsaved edits.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}

/// Recent file item displayed on the Home screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentFileEntry {
    title: SharedString,
    path: PathBuf,
    last_opened_label: SharedString,
}

impl RecentFileEntry {
    /// Creates a recent file entry for the Home screen.
    pub fn new(
        title: impl Into<SharedString>,
        path: impl Into<PathBuf>,
        last_opened_label: impl Into<SharedString>,
    ) -> Self {
        Self {
            title: title.into(),
            path: path.into(),
            last_opened_label: last_opened_label.into(),
        }
    }

    /// Display file name.
    pub fn title(&self) -> &SharedString {
        &self.title
    }

    /// File path on disk.
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Human-readable timestamp / location label.
    pub fn last_opened_label(&self) -> &SharedString {
        &self.last_opened_label
    }
}

/// Drag payload carried when reordering workbook tabs in the title bar.
#[derive(Clone, Debug)]
pub(crate) struct DraggedTab {
    pub tab_id: u64,
    pub title: SharedString,
}

/// Floating ghost preview rendered while dragging a [`DraggedTab`].
pub(crate) struct DraggedTabPreview {
    pub title: SharedString,
}

impl Render for DraggedTabPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_1()
            .rounded(px(6.0))
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().primary)
            .text_xs()
            .text_color(cx.theme().foreground)
            .child(self.title.clone())
    }
}
