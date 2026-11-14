//! `64×16` spreadsheet tile partitioning, `DisplayList` tile cache, fine-grained invalidation,
//! and 1-screen viewport prefetch planning (`M2-02`, `PLAN.md` §4.3).

use crate::axis::GridAxisIndex;
use crate::cell_style_render::build_styled_tile;
use crate::merged::MergedRangeIndex;
use crate::viewport::{PaneRegion, ViewportGeometry};
use ordia_render::{Color, DisplayList};
use ordia_sheet_model::{CellCoord, CellRange, MAX_COLS, MAX_ROWS, SheetDamage, SheetDocument};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

/// Number of worksheet rows grouped into a single render tile (`PLAN.md` §4.3).
pub const TILE_ROWS: u32 = 64;

/// Number of worksheet columns grouped into a single render tile (`PLAN.md` §4.3).
pub const TILE_COLS: u32 = 16;

/// Default maximum number of `64×16` tiles retained in [`SheetTileCache`].
pub const DEFAULT_MAX_CACHED_TILES: usize = 256;

/// Identifies a `64×16` cell block on a specific worksheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TileKey {
    /// Zero-based worksheet index.
    pub sheet: u32,
    /// Tile row coordinate (`row / 64`).
    pub tile_row: u32,
    /// Tile column coordinate (`col / 16`).
    pub tile_col: u32,
}

impl TileKey {
    /// Constructs a tile key directly.
    #[must_use]
    pub const fn new(sheet: u32, tile_row: u32, tile_col: u32) -> Self {
        Self {
            sheet,
            tile_row,
            tile_col,
        }
    }

    /// Maps a single cell coordinate on `sheet` to its owning `64×16` [`TileKey`].
    #[must_use]
    pub const fn from_cell(sheet: u32, coord: CellCoord) -> Self {
        Self {
            sheet,
            tile_row: coord.row / TILE_ROWS,
            tile_col: coord.col / TILE_COLS,
        }
    }

    /// Inclusive [`CellRange`] covered by this `64×16` tile (clamped to Excel sheet bounds).
    #[must_use]
    pub fn cell_range(self) -> CellRange {
        let start_row = (self.tile_row * TILE_ROWS).min(MAX_ROWS - 1);
        let end_row = (start_row + TILE_ROWS - 1).min(MAX_ROWS - 1);
        let start_col = (self.tile_col * TILE_COLS).min(MAX_COLS - 1);
        let end_col = (start_col + TILE_COLS - 1).min(MAX_COLS - 1);
        CellRange::new(
            CellCoord::new(start_row, start_col),
            CellCoord::new(end_row, end_col),
        )
    }
}

/// Theme colors supplied to [`build_tile_display_list`] for UI-dependent hairlines and default text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileRenderTheme {
    /// Default cell text color when no authored font color is specified.
    pub default_text_color: Color,
    /// Optional 1px gridline color (`None` when worksheet gridlines are hidden).
    pub grid_line_color: Option<Color>,
}

/// Pre-built tile-local [`DisplayList`] anchored at `(sheet_origin_x, sheet_origin_y)` in sheet space.
#[derive(Debug, Clone)]
pub struct CachedTile {
    /// Owning `64×16` tile key.
    pub key: TileKey,
    /// Sheet-space X coordinate of the tile's left edge.
    pub sheet_origin_x: f64,
    /// Sheet-space Y coordinate of the tile's top edge.
    pub sheet_origin_y: f64,
    /// Tile-local drawing commands (`(0, 0)` corresponds to `(sheet_origin_x, sheet_origin_y)`).
    pub display_list: DisplayList,
}

/// Visible and 1-screen-ring prefetch tile plan for a single viewport frame.
#[derive(Debug, Clone, Default)]
pub struct TilePlan {
    /// Tiles intersecting at least one visible quadrant (`top_left`, `top`, `left`, `main`).
    pub visible: Vec<TileKey>,
    /// Surrounding 1-screen prefetch tiles around the scrollable quadrant (`PLAN.md` §4.3).
    pub prefetch: Vec<TileKey>,
}

/// Bounded `DisplayList` tile cache with fine-grained cell/range/axis invalidation (`M2-02`).
#[derive(Debug)]
pub struct SheetTileCache {
    max_tiles: usize,
    entries: HashMap<TileKey, Arc<CachedTile>>,
    fifo: VecDeque<TileKey>,
    hits: u64,
    misses: u64,
    invalidations: u64,
}

impl Default for SheetTileCache {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_CACHED_TILES)
    }
}

impl SheetTileCache {
    /// Creates an empty tile cache bounded to `max_tiles` entries.
    #[must_use]
    pub fn new(max_tiles: usize) -> Self {
        let cap = max_tiles.max(8);
        Self {
            max_tiles: cap,
            entries: HashMap::with_capacity(cap),
            fifo: VecDeque::with_capacity(cap),
            hits: 0,
            misses: 0,
            invalidations: 0,
        }
    }

    /// Number of currently cached tiles.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if no tiles are cached.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns `true` if `key` is currently cached without mutating hit/miss counters.
    #[must_use]
    pub fn contains(&self, key: TileKey) -> bool {
        self.entries.contains_key(&key)
    }

    /// Cumulative cache hit count.
    #[must_use]
    pub const fn hits(&self) -> u64 {
        self.hits
    }

    /// Cumulative cache miss count.
    #[must_use]
    pub const fn misses(&self) -> u64 {
        self.misses
    }

    /// Cumulative invalidated tile count.
    #[must_use]
    pub const fn invalidations(&self) -> u64 {
        self.invalidations
    }

    /// Fetches a cached tile without rebuilding if absent.
    #[must_use]
    pub fn peek(&self, key: TileKey) -> Option<&Arc<CachedTile>> {
        self.entries.get(&key)
    }

    /// Returns a cached tile or builds and caches it via `builder`.
    pub fn get_or_build(
        &mut self,
        key: TileKey,
        builder: impl FnOnce(TileKey) -> CachedTile,
    ) -> Arc<CachedTile> {
        if let Some(hit) = self.entries.get(&key) {
            self.hits += 1;
            return hit.clone();
        }
        self.misses += 1;
        while self.entries.len() >= self.max_tiles {
            let Some(oldest) = self.fifo.pop_front() else {
                break;
            };
            if self.entries.remove(&oldest).is_some() {
                break;
            }
        }
        let tile = Arc::new(builder(key));
        self.entries.insert(key, tile.clone());
        self.fifo.push_back(key);
        tile
    }

    /// Invalidates ONLY the single `64×16` tile containing `(sheet, coord)`, returning `1` if evicted.
    pub fn invalidate_cell(&mut self, sheet: u32, coord: CellCoord) -> usize {
        let key = TileKey::from_cell(sheet, coord);
        if self.entries.remove(&key).is_some() {
            self.invalidations += 1;
            1
        } else {
            0
        }
    }

    /// Invalidates all `64×16` tiles intersecting `range` on `sheet`, returning the evicted count.
    pub fn invalidate_range(&mut self, sheet: u32, range: CellRange) -> usize {
        let r0 = range.start.row / TILE_ROWS;
        let r1 = range.end.row / TILE_ROWS;
        let c0 = range.start.col / TILE_COLS;
        let c1 = range.end.col / TILE_COLS;
        let mut evicted = 0usize;
        for tr in r0..=r1 {
            for tc in c0..=c1 {
                if self.entries.remove(&TileKey::new(sheet, tr, tc)).is_some() {
                    evicted += 1;
                }
            }
        }
        self.invalidations += evicted as u64;
        evicted
    }

    /// Invalidates tiles on `sheet` whose `tile_row >= from_row / 64` (used on row resize/hide).
    pub fn invalidate_from_row(&mut self, sheet: u32, from_row: u32) -> usize {
        let min_tr = from_row / TILE_ROWS;
        let before = self.entries.len();
        self.entries
            .retain(|k, _| k.sheet != sheet || k.tile_row < min_tr);
        let evicted = before - self.entries.len();
        self.invalidations += evicted as u64;
        evicted
    }

    /// Invalidates tiles on `sheet` whose `tile_col >= from_col / 16` (used on column resize/hide).
    pub fn invalidate_from_col(&mut self, sheet: u32, from_col: u32) -> usize {
        let min_tc = from_col / TILE_COLS;
        let before = self.entries.len();
        self.entries
            .retain(|k, _| k.sheet != sheet || k.tile_col < min_tc);
        let evicted = before - self.entries.len();
        self.invalidations += evicted as u64;
        evicted
    }

    /// Invalidates all cached tiles belonging to `sheet`.
    pub fn invalidate_sheet(&mut self, sheet: u32) -> usize {
        let before = self.entries.len();
        self.entries.retain(|k, _| k.sheet != sheet);
        let evicted = before - self.entries.len();
        self.invalidations += evicted as u64;
        evicted
    }

    /// Clears all cached tiles across all sheets.
    pub fn clear(&mut self) {
        let evicted = self.entries.len();
        self.entries.clear();
        self.fifo.clear();
        self.invalidations += evicted as u64;
    }

    /// Applies a batch of fine-grained [`SheetDamage`] records emitted by [`SheetDocument::drain_damage`].
    pub fn apply_damage(&mut self, damages: &[SheetDamage]) -> usize {
        let mut total = 0usize;
        for dmg in damages {
            match dmg {
                SheetDamage::Cell { sheet, coord } => {
                    total += self.invalidate_cell(*sheet, *coord);
                }
                SheetDamage::Range { sheet, range } => {
                    total += self.invalidate_range(*sheet, *range);
                }
                SheetDamage::FromRow { sheet, from_row } => {
                    total += self.invalidate_from_row(*sheet, *from_row);
                }
                SheetDamage::FromCol { sheet, from_col } => {
                    total += self.invalidate_from_col(*sheet, *from_col);
                }
                SheetDamage::Sheet { sheet } => {
                    total += self.invalidate_sheet(*sheet);
                }
                SheetDamage::All | _ => {
                    let before = self.entries.len();
                    self.clear();
                    total += before;
                }
            }
        }
        total
    }
}

/// Computes the visible `64×16` tiles across all active frozen/scrolled quadrants plus a 1-screen
/// surrounding prefetch ring (`PLAN.md` §4.3).
#[must_use]
pub fn plan_viewport_tiles(
    sheet: u32,
    geom: &ViewportGeometry,
    row_index: &GridAxisIndex,
    col_index: &GridAxisIndex,
) -> TilePlan {
    let mut visible = Vec::new();
    let mut seen = HashSet::new();

    let panes = [
        geom.top_left_pane.as_ref(),
        geom.top_pane.as_ref(),
        geom.left_pane.as_ref(),
        geom.main_pane.as_ref(),
    ];
    for pane in panes.into_iter().flatten() {
        collect_pane_tiles(sheet, pane, &mut visible, &mut seen);
    }

    let mut prefetch = Vec::new();
    if let Some(main) = geom.main_pane.as_ref() {
        let min_r = geom.freeze.frozen_rows;
        let min_c = geom.freeze.frozen_cols;
        let s64 = f64::from(geom.zoom.max(0.10));
        let w = f64::from(main.clip_rect.width.max(1.0)) / s64;
        let h = f64::from(main.clip_rect.height.max(1.0)) / s64;
        let pf_x = (main.sheet_origin_x - w).max(col_index.index_to_offset(min_c));
        let pf_y = (main.sheet_origin_y - h).max(row_index.index_to_offset(min_r));
        if let (Some(rows), Some(cols)) = (
            row_index.visible_range(min_r, pf_y, h * 3.0),
            col_index.visible_range(min_c, pf_x, w * 3.0),
        ) {
            for tr in (*rows.start() / TILE_ROWS)..=(*rows.end() / TILE_ROWS) {
                for tc in (*cols.start() / TILE_COLS)..=(*cols.end() / TILE_COLS) {
                    let key = TileKey::new(sheet, tr, tc);
                    if seen.insert(key) {
                        prefetch.push(key);
                    }
                }
            }
        }
    }

    TilePlan { visible, prefetch }
}

fn collect_pane_tiles(
    sheet: u32,
    pane: &PaneRegion,
    out: &mut Vec<TileKey>,
    seen: &mut HashSet<TileKey>,
) {
    let r0 = *pane.rows.start() / TILE_ROWS;
    let r1 = *pane.rows.end() / TILE_ROWS;
    let c0 = *pane.cols.start() / TILE_COLS;
    let c1 = *pane.cols.end() / TILE_COLS;
    for tr in r0..=r1 {
        for tc in c0..=c1 {
            let key = TileKey::new(sheet, tr, tc);
            if seen.insert(key) {
                out.push(key);
            }
        }
    }
}

/// Builds a tile-local [`CachedTile`] (`64×16` cells) directly into a [`DisplayList`],
/// automatically indexing any merged ranges on `key.sheet`.
#[must_use]
pub fn build_tile_display_list(
    doc: &SheetDocument,
    key: TileKey,
    row_index: &GridAxisIndex,
    col_index: &GridAxisIndex,
    theme: TileRenderTheme,
) -> CachedTile {
    let raw = doc.merged_ranges_on(key.sheet).unwrap_or_default();
    let merges = MergedRangeIndex::from_ranges(&raw);
    build_styled_tile(doc, key, row_index, col_index, &merges, theme)
}

/// Builds a tile-local [`CachedTile`] (`64×16` cells) reusing a prebuilt [`MergedRangeIndex`].
#[must_use]
pub fn build_tile_display_list_with_merges(
    doc: &SheetDocument,
    key: TileKey,
    row_index: &GridAxisIndex,
    col_index: &GridAxisIndex,
    merged_index: &MergedRangeIndex,
    theme: TileRenderTheme,
) -> CachedTile {
    build_styled_tile(doc, key, row_index, col_index, merged_index, theme)
}
