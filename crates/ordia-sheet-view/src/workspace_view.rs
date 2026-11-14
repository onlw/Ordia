//! Stateful spreadsheet workspace view: formula bar, virtualized grid, GPUI Kit `Input` inline editor, IME bridge, and frame-time overlay.

use crate::frame_stats::{FrameTimeSnapshot, FrameTimeTracker};
use crate::grid_element::SheetGridElement;
use crate::text_cache::ShapedLineCache;
use gpui_kit::TestSupportExt as _;
use gpui_kit::component::{
    ActiveTheme as _, IconName, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Enter as InputEnter, Escape as InputEscape, Input, InputEvent, InputState},
    tab::{Tab, TabBar},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Bounds, Context, Entity, EntityInputHandler, FocusHandle, Focusable,
    Hsla, InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _, Pixels, Point,
    Render, ShapedLine, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    UTF16Selection, Window, div, px,
};
use ordia_sheet_layout::{
    DEFAULT_COL_HEADER_HEIGHT_PX, DEFAULT_ROW_HEADER_WIDTH_PX, GridAxisIndex, LogicalRect,
    ViewportGeometry,
};
use ordia_sheet_model::{
    CellCoord, CellRange, ClipboardPayload, FreezePane, MAX_COLS, MAX_ROWS, SheetDocument,
    parse_clipboard_payload,
};
use std::ops::Range;
use std::sync::{Arc, Mutex};

/// Abstraction over platform rich-clipboard (HTML + TSV) I/O injected by `ordia-app::platform`.
pub trait SheetClipboardBridge: Send + Sync {
    /// Writes dual-flavor spreadsheet clipboard data (`text/html` + `text/plain` TSV).
    fn write_payload(&self, payload: &ClipboardPayload);
    /// Reads `(optional_html, optional_tsv)` from the clipboard.
    fn read_payload(&self) -> (Option<String>, Option<String>);
}

/// Isolated in-memory clipboard bridge used by default in headless UI tests.
#[derive(Default)]
pub struct InMemoryClipboardBridge {
    last: Mutex<Option<ClipboardPayload>>,
}

impl SheetClipboardBridge for InMemoryClipboardBridge {
    fn write_payload(&self, payload: &ClipboardPayload) {
        if let Ok(mut guard) = self.last.lock() {
            *guard = Some(payload.clone());
        }
    }

    fn read_payload(&self) -> (Option<String>, Option<String>) {
        if let Ok(guard) = self.last.lock()
            && let Some(p) = guard.as_ref()
        {
            return (Some(p.html.clone()), Some(p.tsv.clone()));
        }
        (None, None)
    }
}

/// Keyboard navigation and editing actions scoped to [`SheetWorkspaceView`].
#[allow(missing_docs)]
pub mod sheet_actions {
    gpui_kit::actions!(
        ordia_sheet_view,
        [
            MoveUp,
            MoveDown,
            MoveLeft,
            MoveRight,
            MovePageUp,
            MovePageDown,
            MoveRowStart,
            MoveRowEnd,
            StartEdit,
            CancelEdit,
            CommitEdit,
            ClearActiveCell,
            TogglePerfOverlay,
            UndoEdit,
            RedoEdit,
            CopySelection,
            PasteClipboard,
        ]
    );
}
pub use sheet_actions::*;

/// Registers keyboard shortcuts for [`SheetWorkspaceView`] navigation and editing.
pub fn register_sheet_keybindings(cx: &mut App) {
    #[cfg(target_os = "macos")]
    let platform_undo = [
        KeyBinding::new("cmd-z", UndoEdit, Some("OrdiaSheetGrid && !Input")),
        KeyBinding::new("cmd-shift-z", RedoEdit, Some("OrdiaSheetGrid && !Input")),
        KeyBinding::new("cmd-c", CopySelection, Some("OrdiaSheetGrid && !Input")),
        KeyBinding::new("cmd-v", PasteClipboard, Some("OrdiaSheetGrid && !Input")),
    ];
    #[cfg(not(target_os = "macos"))]
    let platform_undo = [
        KeyBinding::new("ctrl-z", UndoEdit, Some("OrdiaSheetGrid && !Input")),
        KeyBinding::new("ctrl-y", RedoEdit, Some("OrdiaSheetGrid && !Input")),
        KeyBinding::new("ctrl-c", CopySelection, Some("OrdiaSheetGrid && !Input")),
        KeyBinding::new("ctrl-v", PasteClipboard, Some("OrdiaSheetGrid && !Input")),
    ];

    cx.bind_keys(platform_undo);
    cx.bind_keys([
        KeyBinding::new("up", MoveUp, Some("OrdiaSheetGrid && !Input")),
        KeyBinding::new("down", MoveDown, Some("OrdiaSheetGrid && !Input")),
        KeyBinding::new("left", MoveLeft, Some("OrdiaSheetGrid && !Input")),
        KeyBinding::new("right", MoveRight, Some("OrdiaSheetGrid && !Input")),
        KeyBinding::new("tab", MoveRight, Some("OrdiaSheetGrid && !Input")),
        KeyBinding::new("shift-tab", MoveLeft, Some("OrdiaSheetGrid && !Input")),
        KeyBinding::new("tab", MoveRight, Some("OrdiaCellEditor")),
        KeyBinding::new("shift-tab", MoveLeft, Some("OrdiaCellEditor")),
        KeyBinding::new("pageup", MovePageUp, Some("OrdiaSheetGrid && !Input")),
        KeyBinding::new("pagedown", MovePageDown, Some("OrdiaSheetGrid && !Input")),
        KeyBinding::new("home", MoveRowStart, Some("OrdiaSheetGrid && !Input")),
        KeyBinding::new("end", MoveRowEnd, Some("OrdiaSheetGrid && !Input")),
        KeyBinding::new("f2", StartEdit, Some("OrdiaSheetGrid && !Input")),
        KeyBinding::new("enter", StartEdit, Some("OrdiaSheetGrid && !Input")),
        KeyBinding::new("escape", CancelEdit, Some("OrdiaSheetGrid && !Input")),
        KeyBinding::new(
            "backspace",
            ClearActiveCell,
            Some("OrdiaSheetGrid && !Input"),
        ),
        KeyBinding::new("delete", ClearActiveCell, Some("OrdiaSheetGrid && !Input")),
        KeyBinding::new("f12", TogglePerfOverlay, Some("OrdiaSheetWorkspace")),
    ]);
}

/// Complete spreadsheet document view owning workbook state, coordinate indices, editors, and perf metrics.
pub struct SheetWorkspaceView {
    grid_focus: FocusHandle,
    document: SheetDocument,
    row_index: GridAxisIndex,
    col_index: GridAxisIndex,
    cached_freeze: FreezePane,
    cached_layout_rev: u64,
    scroll_x: f64,
    scroll_y: f64,
    last_canvas_bounds: Bounds<Pixels>,
    last_geometry: Option<ViewportGeometry>,
    pointer_dragging: bool,
    editing_cell: Option<CellCoord>,
    syncing_formula_bar: bool,
    cell_editor: Entity<InputState>,
    formula_editor: Entity<InputState>,
    show_perf_overlay: bool,
    frame_tracker: FrameTimeTracker,
    text_cache: ShapedLineCache,
    clipboard_bridge: Arc<dyn SheetClipboardBridge>,
    _subscriptions: Vec<Subscription>,
}

impl SheetWorkspaceView {
    /// Constructs a new [`SheetWorkspaceView`] wrapping `document`.
    pub fn new(document: SheetDocument, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let grid_focus = cx.focus_handle().tab_stop(true);
        let cell_editor = cx.new(|cx| InputState::new(window, cx));
        let formula_editor = cx.new(|cx| InputState::new(window, cx));

        let initial_raw = document
            .cell_input(document.active_cell())
            .unwrap_or_default();
        formula_editor.update(cx, |state, cx| {
            state.set_value(initial_raw, window, cx);
        });

        let sub_cell = cx.subscribe_in(
            &cell_editor,
            window,
            |this, _emitter, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    if this.editing_cell.is_some() {
                        let val = this.cell_editor.read(cx).value();
                        this.syncing_formula_bar = true;
                        this.formula_editor.update(cx, |f_state, cx| {
                            f_state.set_value(val, window, cx);
                        });
                        this.syncing_formula_bar = false;
                        cx.notify();
                    }
                }
                InputEvent::PressEnter { .. } | InputEvent::Focus | InputEvent::Blur => {}
            },
        );

        let sub_formula = cx.subscribe_in(
            &formula_editor,
            window,
            |this, _emitter, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    if this.syncing_formula_bar
                        || !this
                            .formula_editor
                            .read(cx)
                            .focus_handle(cx)
                            .is_focused(window)
                    {
                        return;
                    }
                    let val = this.formula_editor.read(cx).value();
                    if this.editing_cell.is_none() {
                        this.editing_cell = Some(this.document.active_cell());
                    }
                    this.cell_editor.update(cx, |c_state, cx| {
                        c_state.set_value(val, window, cx);
                    });
                    cx.notify();
                }
                InputEvent::PressEnter { .. } | InputEvent::Focus | InputEvent::Blur => {}
            },
        );

        let row_overrides = document.custom_row_metrics().unwrap_or_default();
        let col_overrides = document.custom_col_metrics().unwrap_or_default();
        let cached_freeze = document.freeze_pane().unwrap_or_default();
        let cached_layout_rev = document.layout_revision();

        Self {
            grid_focus,
            document,
            row_index: GridAxisIndex::from_row_metrics(MAX_ROWS, &row_overrides),
            col_index: GridAxisIndex::from_col_metrics(MAX_COLS, &col_overrides),
            cached_freeze,
            cached_layout_rev,
            scroll_x: 0.0,
            scroll_y: 0.0,
            last_canvas_bounds: Bounds::default(),
            last_geometry: None,
            pointer_dragging: false,
            editing_cell: None,
            syncing_formula_bar: false,
            cell_editor,
            formula_editor,
            show_perf_overlay: false,
            frame_tracker: FrameTimeTracker::new(),
            text_cache: ShapedLineCache::new(),
            clipboard_bridge: Arc::new(InMemoryClipboardBridge::default()),
            _subscriptions: vec![sub_cell, sub_formula],
        }
    }

    /// Injects a custom [`SheetClipboardBridge`] (e.g. native macOS/Windows HTML+TSV clipboard).
    pub fn set_clipboard_bridge(&mut self, bridge: Arc<dyn SheetClipboardBridge>) {
        self.clipboard_bridge = bridge;
    }

    /// Copies the active selection to the configured [`SheetClipboardBridge`].
    pub fn copy_selection(&self) {
        if let Ok(payload) = self.document.copy_selection_clipboard() {
            self.clipboard_bridge.write_payload(&payload);
        }
    }

    /// Pastes clipboard contents (HTML `<table>` or TSV fallback) anchored at the active cell.
    pub fn paste_clipboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (html, tsv) = self.clipboard_bridge.read_payload();
        let grid = parse_clipboard_payload(html.as_deref(), tsv.as_deref());
        if !grid.is_empty() {
            let anchor = self.document.active_cell();
            let _ = self.document.paste_clipboard_grid(anchor, &grid);
            self.sync_formula_bar_to_active(window, cx);
            cx.notify();
        }
    }

    /// Reference to the underlying [`SheetDocument`].
    #[must_use]
    pub fn document(&self) -> &SheetDocument {
        &self.document
    }

    /// Mutable reference to the underlying [`SheetDocument`].
    pub fn document_mut(&mut self) -> &mut SheetDocument {
        &mut self.document
    }

    /// Returns the grid's keyboard [`FocusHandle`].
    #[must_use]
    pub fn grid_focus_handle(&self) -> &FocusHandle {
        &self.grid_focus
    }

    /// Returns the inline cell [`InputState`] entity.
    #[must_use]
    pub fn cell_editor(&self) -> &Entity<InputState> {
        &self.cell_editor
    }

    /// Returns the formula bar [`InputState`] entity.
    #[must_use]
    pub fn formula_editor(&self) -> &Entity<InputState> {
        &self.formula_editor
    }

    /// Currently inline-edited cell coordinate, if in edit mode.
    #[must_use]
    pub fn editing_cell(&self) -> Option<CellCoord> {
        self.editing_cell
    }

    /// Current horizontal scroll offset in logical pixels.
    #[must_use]
    pub fn scroll_x(&self) -> f64 {
        self.scroll_x
    }

    /// Current vertical scroll offset in logical pixels.
    #[must_use]
    pub fn scroll_y(&self) -> f64 {
        self.scroll_y
    }

    /// Snapshot of rolling frame-time metrics (`last`, `avg`, `p50`, `p95`, `p99`, `max`).
    #[must_use]
    pub fn frame_stats(&self) -> FrameTimeSnapshot {
        self.frame_tracker.snapshot()
    }

    /// Resets rolling frame-time samples before an isolated scroll benchmark.
    pub fn reset_frame_stats(&mut self) {
        self.frame_tracker.reset();
    }

    /// Toggles visibility of the frame-time debug overlay.
    pub fn toggle_perf_overlay(&mut self, cx: &mut Context<Self>) {
        self.show_perf_overlay = !self.show_perf_overlay;
        cx.notify();
    }

    /// Returns whether the frame-time debug overlay is visible.
    #[must_use]
    pub fn is_perf_overlay_visible(&self) -> bool {
        self.show_perf_overlay
    }

    pub(crate) fn row_index(&self) -> &GridAxisIndex {
        &self.row_index
    }

    pub(crate) fn col_index(&self) -> &GridAxisIndex {
        &self.col_index
    }

    pub(crate) fn freeze_pane_cached(&self) -> FreezePane {
        self.cached_freeze
    }

    pub(crate) fn is_pointer_dragging(&self) -> bool {
        self.pointer_dragging
    }

    pub(crate) fn set_pointer_dragging(&mut self, dragging: bool) {
        self.pointer_dragging = dragging;
    }

    pub(crate) fn ensure_layout_indices(&mut self) {
        let rev = self.document.layout_revision();
        if self.cached_layout_rev != rev {
            let row_overrides = self.document.custom_row_metrics().unwrap_or_default();
            let col_overrides = self.document.custom_col_metrics().unwrap_or_default();
            self.row_index = GridAxisIndex::from_row_metrics(MAX_ROWS, &row_overrides);
            self.col_index = GridAxisIndex::from_col_metrics(MAX_COLS, &col_overrides);
            self.cached_freeze = self.document.freeze_pane().unwrap_or_default();
            self.cached_layout_rev = rev;
        }
    }

    pub(crate) fn record_resolved_viewport(
        &mut self,
        bounds: Bounds<Pixels>,
        geom: &ViewportGeometry,
    ) {
        self.last_canvas_bounds = bounds;
        self.scroll_x = geom.scroll_x;
        self.scroll_y = geom.scroll_y;
        self.last_geometry = Some(geom.clone());
    }

    pub(crate) fn record_frame_time_ms(&mut self, ms: f32) {
        self.frame_tracker.record_ms(ms);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn shape_cached_line(
        &mut self,
        window: &Window,
        text: SharedString,
        font_family: SharedString,
        font_size: Pixels,
        bold: bool,
        italic: bool,
        color: Hsla,
    ) -> ShapedLine {
        self.text_cache
            .shape_or_get(window, text, font_family, font_size, bold, italic, color)
    }

    /// Scrolls the viewport by `(dx, dy)` logical pixels and schedules a frame redraw.
    pub fn scroll_by(&mut self, dx: f64, dy: f64, cx: &mut Context<Self>) {
        self.ensure_layout_indices();
        let max_x = self
            .last_geometry
            .as_ref()
            .map_or_else(|| self.col_index.total_extent(), |g| g.max_scroll_x);
        let max_y = self
            .last_geometry
            .as_ref()
            .map_or_else(|| self.row_index.total_extent(), |g| g.max_scroll_y);
        self.scroll_x = (self.scroll_x + dx).clamp(0.0, max_x);
        self.scroll_y = (self.scroll_y + dy).clamp(0.0, max_y);
        cx.notify();
    }

    /// Scrolls directly to `(scroll_x, scroll_y)` in logical pixels.
    pub fn set_scroll_offset(&mut self, scroll_x: f64, scroll_y: f64, cx: &mut Context<Self>) {
        self.ensure_layout_indices();
        self.scroll_x = scroll_x.max(0.0);
        self.scroll_y = scroll_y.max(0.0);
        cx.notify();
    }

    /// Selects a single cell, syncs the formula bar, ensures visibility, and focuses the grid.
    pub fn select_cell(&mut self, coord: CellCoord, window: &mut Window, cx: &mut Context<Self>) {
        self.document.set_active_cell(coord);
        self.sync_formula_bar_to_active(window, cx);
        if let Some(geom) = &self.last_geometry {
            let (nx, ny) = geom.scroll_into_view(coord, &self.row_index, &self.col_index);
            self.scroll_x = nx;
            self.scroll_y = ny;
        }
        self.grid_focus.focus(window, cx);
        cx.notify();
    }

    /// Sets a multi-cell selection between `anchor` and `focus_end`.
    pub fn set_selection_range(
        &mut self,
        anchor: CellCoord,
        focus_end: CellCoord,
        cx: &mut Context<Self>,
    ) {
        self.document.set_active_cell(anchor);
        self.document
            .set_selection(CellRange::new(anchor, focus_end));
        cx.notify();
    }

    /// Moves the active cell by `(delta_row, delta_col)` and scrolls it into view.
    pub fn move_selection_by(
        &mut self,
        delta_row: i32,
        delta_col: i32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = self.document.move_active_cell(delta_row, delta_col);
        self.sync_formula_bar_to_active(window, cx);
        if let Some(geom) = &self.last_geometry {
            let (nx, ny) = geom.scroll_into_view(next, &self.row_index, &self.col_index);
            self.scroll_x = nx;
            self.scroll_y = ny;
        }
        cx.notify();
    }

    /// Enters inline edit mode on `coord` seeded with the cell's existing input/formula (`F2` / double-click).
    pub fn begin_edit_existing(
        &mut self,
        coord: CellCoord,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.document.set_active_cell(coord);
        if let Some(geom) = &self.last_geometry {
            let (nx, ny) = geom.scroll_into_view(coord, &self.row_index, &self.col_index);
            self.scroll_x = nx;
            self.scroll_y = ny;
        }
        let existing = self.document.cell_input(coord).unwrap_or_default();
        self.editing_cell = Some(coord);
        self.cell_editor.update(cx, |state, cx| {
            state.set_value(existing.clone(), window, cx);
            state.focus(window, cx);
        });
        self.syncing_formula_bar = true;
        self.formula_editor.update(cx, |state, cx| {
            state.set_value(existing, window, cx);
        });
        self.syncing_formula_bar = false;
        cx.notify();
    }

    /// Commits the current inline edit to `SheetDocument` (via `Transaction`) and returns focus to grid if `refocus_grid`.
    pub fn commit_edit(&mut self, refocus_grid: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(coord) = self.editing_cell.take() {
            let new_input = self.cell_editor.read(cx).value().to_string();
            let _ = self.document.set_cell_input(coord, &new_input);
            self.sync_formula_bar_to_active(window, cx);
            if refocus_grid {
                self.grid_focus.focus(window, cx);
            }
            cx.notify();
        }
    }

    /// Cancels inline editing without mutating `SheetDocument` and restores focus to the grid.
    pub fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing_cell.take().is_some() {
            self.sync_formula_bar_to_active(window, cx);
            self.grid_focus.focus(window, cx);
            cx.notify();
        }
    }

    fn sync_formula_bar_to_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let raw = self
            .document
            .cell_input(self.document.active_cell())
            .unwrap_or_default();
        self.syncing_formula_bar = true;
        self.formula_editor.update(cx, |state, cx| {
            state.set_value(raw, window, cx);
        });
        self.syncing_formula_bar = false;
    }
}

impl Focusable for SheetWorkspaceView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.grid_focus.clone()
    }
}

/// Bridges OS IME (`NSTextInputClient` on macOS, `TSF` on Windows) and direct character input
/// when standing on a selected grid cell, seamlessly transitioning into inline `InputState` editing
/// so candidate popups follow the exact cell bounds from the very first keystroke.
impl EntityInputHandler for SheetWorkspaceView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        if self.editing_cell.is_some() {
            self.cell_editor.update(cx, |state, cx| {
                EntityInputHandler::text_for_range(state, range, adjusted_range, window, cx)
            })
        } else {
            *adjusted_range = Some(0..0);
            Some(String::new())
        }
    }

    fn selected_text_range(
        &mut self,
        ignore_disabled_input: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        if self.editing_cell.is_some() {
            self.cell_editor.update(cx, |state, cx| {
                EntityInputHandler::selected_text_range(state, ignore_disabled_input, window, cx)
            })
        } else {
            Some(UTF16Selection {
                range: 0..0,
                reversed: false,
            })
        }
    }

    fn marked_text_range(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        if self.editing_cell.is_some() {
            self.cell_editor.update(cx, |state, cx| {
                EntityInputHandler::marked_text_range(state, window, cx)
            })
        } else {
            None
        }
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing_cell.is_some() {
            self.cell_editor.update(cx, |state, cx| {
                EntityInputHandler::unmark_text(state, window, cx)
            });
        }
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if text.is_empty() && self.editing_cell.is_none() {
            return;
        }
        if self.editing_cell.is_none() {
            let active = self.document.active_cell();
            self.editing_cell = Some(active);
            self.cell_editor.update(cx, |state, cx| {
                state.set_value("", window, cx);
                state.focus(window, cx);
            });
        }
        self.cell_editor.update(cx, |state, cx| {
            EntityInputHandler::replace_text_in_range(state, range, text, window, cx);
        });
        let val = self.cell_editor.read(cx).value();
        self.syncing_formula_bar = true;
        self.formula_editor.update(cx, |f_state, cx| {
            f_state.set_value(val, window, cx);
        });
        self.syncing_formula_bar = false;
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editing_cell.is_none() {
            let active = self.document.active_cell();
            self.editing_cell = Some(active);
            self.cell_editor.update(cx, |state, cx| {
                state.set_value("", window, cx);
                state.focus(window, cx);
            });
        }
        self.cell_editor.update(cx, |state, cx| {
            EntityInputHandler::replace_and_mark_text_in_range(
                state,
                range,
                new_text,
                new_selected_range,
                window,
                cx,
            );
        });
        let val = self.cell_editor.read(cx).value();
        self.syncing_formula_bar = true;
        self.formula_editor.update(cx, |f_state, cx| {
            f_state.set_value(val, window, cx);
        });
        self.syncing_formula_bar = false;
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        if self.editing_cell.is_some() {
            self.cell_editor
                .update(cx, |state, cx| {
                    EntityInputHandler::bounds_for_range(
                        state,
                        range_utf16,
                        element_bounds,
                        window,
                        cx,
                    )
                })
                .or(Some(element_bounds))
        } else {
            Some(element_bounds)
        }
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        if self.editing_cell.is_some() {
            self.cell_editor.update(cx, |state, cx| {
                EntityInputHandler::character_index_for_point(state, point, window, cx)
            })
        } else {
            Some(0)
        }
    }
}

impl SheetWorkspaceView {
    fn render_formula_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let sel = self.document.selection();
        let name_label = SharedString::from(sel.to_a1());

        h_flex()
            .w_full()
            .px_2()
            .py_1()
            .gap_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(
                div()
                    .id("name-box")
                    .test_support()
                    .aria_label(name_label.clone())
                    .w_20()
                    .px_2()
                    .py_0p5()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary)
                    .text_xs()
                    .font_medium()
                    .text_color(cx.theme().foreground)
                    .child(name_label),
            )
            .child(
                div()
                    .px_1()
                    .text_xs()
                    .italic()
                    .font_semibold()
                    .text_color(cx.theme().muted_foreground)
                    .child("fx"),
            )
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&self.formula_editor)
                        .id("formula-bar-input")
                        .small()
                        .cleanable(false),
                ),
            )
            .child(
                Button::new("toggle-perf-overlay")
                    .ghost()
                    .xsmall()
                    .icon(IconName::ChartPie)
                    .tooltip("Toggle frame-time overlay (F12)")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_perf_overlay(cx);
                    })),
            )
    }

    fn render_inline_editor_overlay(&self) -> Option<impl IntoElement> {
        let coord = self.editing_cell?;
        let rect = if let Some(geom) = &self.last_geometry {
            geom.cell_viewport_rect(coord, &self.row_index, &self.col_index)
        } else {
            Some(LogicalRect::new(
                DEFAULT_ROW_HEADER_WIDTH_PX + (coord.col as f32) * 90.0,
                DEFAULT_COL_HEADER_HEIGHT_PX + (coord.row as f32) * 25.0,
                90.0,
                25.0,
            ))
        }?;

        let editor_w = rect.width.max(140.0);
        let editor_h = rect.height.max(25.0);

        Some(
            div()
                .id("cell-editor-overlay-box")
                .key_context("OrdiaCellEditor")
                .absolute()
                .left(px(rect.x))
                .top(px(rect.y))
                .w(px(editor_w))
                .h(px(editor_h))
                .child(
                    Input::new(&self.cell_editor)
                        .id("cell-inline-editor")
                        .small()
                        .h_full()
                        .cleanable(false),
                ),
        )
    }

    fn render_perf_overlay(&self, cx: &App) -> impl IntoElement {
        let stats = self.frame_tracker.snapshot();
        let fps = if stats.avg_ms > 0.05 {
            (1000.0 / stats.avg_ms).min(999.0)
        } else {
            0.0
        };
        let top_row = self
            .row_index
            .offset_to_index(f64::from(self.cached_freeze.frozen_rows) * 25.0 + self.scroll_y)
            + 1;
        let summary = SharedString::from(format!(
            "FPS {:.0} · Last {:.2}ms · P50 {:.2}ms · P95 {:.2}ms · P99 {:.2}ms · Max {:.2}ms · Row {}/{}",
            fps,
            stats.last_ms,
            stats.p50_ms,
            stats.p95_ms,
            stats.p99_ms,
            stats.max_ms,
            top_row,
            self.row_index.count()
        ));

        div()
            .id("perf-overlay")
            .test_support()
            .aria_label(summary.clone())
            .absolute()
            .top_2()
            .right_3()
            .px_2p5()
            .py_1()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().popover)
            .text_xs()
            .text_color(cx.theme().popover_foreground)
            .child(summary)
    }

    fn render_bottom_sheet_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let sheets = self.document.sheets();
        let active_ix = self.document.active_sheet() as usize;
        let tabs: Vec<Tab> = sheets
            .iter()
            .map(|s| Tab::new().label(SharedString::from(s.name.clone())))
            .collect();

        let active_val = self
            .document
            .cell_display_text(self.document.active_cell())
            .unwrap_or_default();
        let dirty_tag = if self.document.is_dirty() {
            "Modified"
        } else {
            "Saved"
        };
        let status_label = SharedString::from(format!(
            "{} = {} · {}",
            self.document.active_cell(),
            if active_val.is_empty() {
                "∅"
            } else {
                &active_val
            },
            dirty_tag
        ));

        h_flex()
            .w_full()
            .px_2()
            .py_0p5()
            .justify_between()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary)
            .child(
                TabBar::new("worksheet-bottom-tabs")
                    .pill()
                    .xsmall()
                    .selected_index(active_ix)
                    .children(tabs)
                    .on_click(cx.listener(|this, ix, window, cx| {
                        if this.editing_cell.is_some() {
                            this.commit_edit(false, window, cx);
                        }
                        if this.document.set_active_sheet(*ix as u32) {
                            this.scroll_x = 0.0;
                            this.scroll_y = 0.0;
                            this.sync_formula_bar_to_active(window, cx);
                            cx.notify();
                        }
                    })),
            )
            .child(
                div()
                    .id("sheet-status-readout")
                    .test_support()
                    .aria_label(status_label.clone())
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(status_label),
            )
    }
}

impl Render for SheetWorkspaceView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view_entity = cx.entity();

        v_flex()
            .id("ordia-sheet-workspace")
            .key_context("OrdiaSheetWorkspace")
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(cx.theme().background)
            .on_action(cx.listener(|this, _: &MoveUp, window, cx| {
                if this.editing_cell.is_none() {
                    this.move_selection_by(-1, 0, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &MoveDown, window, cx| {
                if this.editing_cell.is_none() {
                    this.move_selection_by(1, 0, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &MoveLeft, window, cx| {
                if this.editing_cell.is_some() {
                    this.commit_edit(true, window, cx);
                }
                this.move_selection_by(0, -1, window, cx);
            }))
            .on_action(cx.listener(|this, _: &MoveRight, window, cx| {
                if this.editing_cell.is_some() {
                    this.commit_edit(true, window, cx);
                }
                this.move_selection_by(0, 1, window, cx);
            }))
            .on_action(cx.listener(|this, _: &MovePageUp, window, cx| {
                if this.editing_cell.is_none() {
                    this.move_selection_by(-25, 0, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &MovePageDown, window, cx| {
                if this.editing_cell.is_none() {
                    this.move_selection_by(25, 0, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &MoveRowStart, window, cx| {
                if this.editing_cell.is_none() {
                    let row = this.document.active_cell().row;
                    this.select_cell(CellCoord::new(row, 0), window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &MoveRowEnd, window, cx| {
                if this.editing_cell.is_none() {
                    let row = this.document.active_cell().row;
                    let last_col = this
                        .document
                        .active_sheet_info()
                        .and_then(|i| i.used_range.map(|r| r.end.col))
                        .unwrap_or(19);
                    this.select_cell(CellCoord::new(row, last_col), window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &StartEdit, window, cx| {
                if this.editing_cell.is_none() {
                    this.begin_edit_existing(this.document.active_cell(), window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &CancelEdit, window, cx| {
                this.cancel_edit(window, cx);
            }))
            .on_action(cx.listener(|this, _: &InputEscape, window, cx| {
                this.cancel_edit(window, cx);
            }))
            .on_action(cx.listener(|this, action: &InputEnter, window, cx| {
                if this
                    .formula_editor
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
                {
                    let text = this.formula_editor.read(cx).value().to_string();
                    let target = this
                        .editing_cell
                        .take()
                        .unwrap_or_else(|| this.document.active_cell());
                    let _ = this.document.set_cell_input(target, &text);
                    let delta = if action.shift { -1 } else { 1 };
                    this.move_selection_by(delta, 0, window, cx);
                    this.grid_focus.focus(window, cx);
                    cx.notify();
                } else if this.editing_cell.is_some() {
                    this.commit_edit(true, window, cx);
                    let delta = if action.shift { -1 } else { 1 };
                    this.move_selection_by(delta, 0, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &CommitEdit, window, cx| {
                this.commit_edit(true, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ClearActiveCell, window, cx| {
                if this.editing_cell.is_none() {
                    let coord = this.document.active_cell();
                    let _ = this.document.set_cell_input(coord, "");
                    this.sync_formula_bar_to_active(window, cx);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &UndoEdit, window, cx| {
                if this.editing_cell.is_none() {
                    let _ = this.document.undo();
                    this.sync_formula_bar_to_active(window, cx);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &RedoEdit, window, cx| {
                if this.editing_cell.is_none() {
                    let _ = this.document.redo();
                    this.sync_formula_bar_to_active(window, cx);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &CopySelection, _, _| {
                if this.editing_cell.is_none() {
                    this.copy_selection();
                }
            }))
            .on_action(cx.listener(|this, _: &PasteClipboard, window, cx| {
                if this.editing_cell.is_none() {
                    this.paste_clipboard(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &TogglePerfOverlay, _, cx| {
                this.toggle_perf_overlay(cx);
            }))
            .child(self.render_formula_bar(cx))
            .child(
                div()
                    .id("sheet-grid-surface")
                    .key_context("OrdiaSheetGrid")
                    .test_support()
                    .track_focus(&self.grid_focus)
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .size_full()
                    .overflow_hidden()
                    .child(SheetGridElement::new("sheet-grid-canvas", view_entity))
                    .when_some(self.render_inline_editor_overlay(), |el, overlay| {
                        el.child(overlay)
                    })
                    .when(self.show_perf_overlay, |el| {
                        el.child(self.render_perf_overlay(cx))
                    }),
            )
            .child(self.render_bottom_sheet_bar(cx))
    }
}
