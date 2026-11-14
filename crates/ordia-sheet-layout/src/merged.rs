//! Augmented interval-tree index over worksheet merged cell ranges (`M2-05`).
//!
//! Stores merged [`CellRange`] rectangles sorted by `(start.row, start.col)` along with an
//! implicit midpoint-split binary search tree's `subtree_max_end_row` array so both single-cell
//! containment (`containing`) and viewport/tile intersection (`intersecting`) prune non-overlapping
//! subtrees in `O(log M)` time without pointer allocations.

use ordia_sheet_model::{CellCoord, CellRange};

/// `O(log M)` spatial index over merged cell ranges on a worksheet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergedRangeIndex {
    ranges: Vec<CellRange>,
    subtree_max_end_row: Vec<u32>,
}

impl MergedRangeIndex {
    /// Builds a sorted, augmented merged-range index from `raw_ranges` in `O(M log M)` time.
    #[must_use]
    pub fn from_ranges(raw_ranges: &[CellRange]) -> Self {
        if raw_ranges.is_empty() {
            return Self::default();
        }
        let mut ranges: Vec<CellRange> = raw_ranges
            .iter()
            .copied()
            .filter(|r| r.start != r.end)
            .collect();
        ranges.sort_unstable_by_key(|r| (r.start.row, r.start.col, r.end.row, r.end.col));
        ranges.dedup();
        let n = ranges.len();
        let mut subtree_max_end_row = vec![0_u32; n];
        if n > 0 {
            Self::populate_max_end(&ranges, &mut subtree_max_end_row, 0, n);
        }
        Self {
            ranges,
            subtree_max_end_row,
        }
    }

    fn populate_max_end(ranges: &[CellRange], max_end: &mut [u32], lo: usize, hi: usize) -> u32 {
        let mid = lo + (hi - lo) / 2;
        let mut best = ranges[mid].end.row;
        if lo < mid {
            best = best.max(Self::populate_max_end(ranges, max_end, lo, mid));
        }
        if mid + 1 < hi {
            best = best.max(Self::populate_max_end(ranges, max_end, mid + 1, hi));
        }
        max_end[mid] = best;
        best
    }

    /// Number of multi-cell merged ranges indexed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    /// Returns `true` if the worksheet has no multi-cell merged ranges.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    /// Ordered slice of indexed merged ranges.
    #[must_use]
    pub fn ranges(&self) -> &[CellRange] {
        &self.ranges
    }

    /// Finds the merged [`CellRange`] covering `coord` in `O(log M)` time, if any.
    #[must_use]
    pub fn containing(&self, coord: CellCoord) -> Option<CellRange> {
        if self.ranges.is_empty() {
            return None;
        }
        self.find_containing(0, self.ranges.len(), coord)
    }

    fn find_containing(&self, lo: usize, hi: usize, coord: CellCoord) -> Option<CellRange> {
        if lo >= hi {
            return None;
        }
        let mid = lo + (hi - lo) / 2;
        if self.subtree_max_end_row[mid] < coord.row {
            return None;
        }
        let cur = self.ranges[mid];
        if cur.contains(coord) {
            return Some(cur);
        }
        if lo < mid
            && let Some(hit) = self.find_containing(lo, mid, coord)
        {
            return Some(hit);
        }
        // Only right-hand entries can satisfy `start.row <= coord.row` if `cur.start.row <= coord.row`
        // (and when `cur.start.row == coord.row`, only if `cur.start.col <= coord.col`).
        if mid + 1 < hi
            && (cur.start.row < coord.row
                || (cur.start.row == coord.row && cur.start.col <= coord.col))
        {
            return self.find_containing(mid + 1, hi, coord);
        }
        None
    }

    /// Maps `coord` to the top-left anchor cell of its enclosing merged range, or returns `coord` unchanged.
    #[must_use]
    pub fn resolve_anchor(&self, coord: CellCoord) -> CellCoord {
        self.containing(coord).map_or(coord, |r| r.start)
    }

    /// Appends all merged ranges intersecting `query` into `out` in `O(log M + k)` time.
    pub fn intersecting(&self, query: CellRange, out: &mut Vec<CellRange>) {
        if !self.ranges.is_empty() {
            self.collect_intersecting(0, self.ranges.len(), query, out);
        }
    }

    /// Returns all merged ranges intersecting `query`.
    #[must_use]
    pub fn intersecting_ranges(&self, query: CellRange) -> Vec<CellRange> {
        let mut out = Vec::new();
        self.intersecting(query, &mut out);
        out
    }

    fn collect_intersecting(
        &self,
        lo: usize,
        hi: usize,
        query: CellRange,
        out: &mut Vec<CellRange>,
    ) {
        if lo >= hi {
            return;
        }
        let mid = lo + (hi - lo) / 2;
        if self.subtree_max_end_row[mid] < query.start.row {
            return;
        }
        if lo < mid {
            self.collect_intersecting(lo, mid, query, out);
        }
        let cur = self.ranges[mid];
        if cur.intersects(query) {
            out.push(cur);
        }
        if mid + 1 < hi && cur.start.row <= query.end.row {
            self.collect_intersecting(mid + 1, hi, query, out);
        }
    }
}
