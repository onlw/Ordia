//! GPUI virtualized spreadsheet grid element, inline cell editor, formula bar, and worksheet tab bar.

mod frame_stats;
mod grid_element;
mod text_cache;
mod workspace_view;

pub use frame_stats::{FrameTimeSnapshot, FrameTimeTracker};
pub use grid_element::SheetGridElement;
pub use workspace_view::{
    CancelEdit, ClearActiveCell, CommitEdit, CopySelection, DEFAULT_ZOOM_PERCENT,
    InMemoryClipboardBridge, MAX_ZOOM_PERCENT, MIN_ZOOM_PERCENT, MoveDown, MoveLeft, MovePageDown,
    MovePageUp, MoveRight, MoveRowEnd, MoveRowStart, MoveUp, PasteClipboard, RedoEdit,
    SheetClipboardBridge, SheetWorkspaceView, StartEdit, ToggleGridlines, TogglePerfOverlay,
    UndoEdit, ZoomIn, ZoomOut, ZoomReset, register_sheet_keybindings,
};
