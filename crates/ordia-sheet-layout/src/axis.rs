//! Fenwick-tree (Binary Indexed Tree) 1D coordinate index for virtualized row/column positioning,
//! dynamic `O(log N)` height/width/visibility updates, and `O(log N)` pixel ↔ row/column conversion.

use ordia_sheet_model::{
    ColSpanMetric, DEFAULT_COL_WIDTH_PX, DEFAULT_ROW_HEIGHT_PX, MAX_COLS, MAX_ROWS, RowMetric,
};
use std::ops::RangeInclusive;

/// Dynamic `O(log N)` Fenwick-tree coordinate index over `N` rows or columns (`M2-01`).
///
/// Stores each span's unhidden pixel size (`base_sizes`) and visibility (`hidden`) alongside a
/// 1-based `f64` Fenwick tree (`bit`) of effective visible extents (`0.0` when hidden).
/// Using `f64` prefix sums ensures `1,048,576 * 25.0 = 26,214,400.0` retains exact sub-pixel
/// precision while supporting single-cell or range height/width/hide mutations in `O(log N)`
/// without reallocating or rebuilding the 1M-row array.
#[derive(Debug, Clone)]
pub struct GridAxisIndex {
    bit: Vec<f64>,
    base_sizes: Vec<f32>,
    hidden: Vec<bool>,
    total: f64,
    top_bit: usize,
}

impl GridAxisIndex {
    /// Builds a uniform axis index with `count` visible spans of `default_size_px` in `O(N)` time.
    #[must_use]
    pub fn uniform(count: u32, default_size_px: f32) -> Self {
        let n = count as usize;
        let sz = default_size_px.max(0.0);
        Self::from_parts(vec![sz; n], vec![false; n])
    }

    /// Builds a row coordinate index for `row_count` rows (`<= MAX_ROWS`) applying sparse `RowMetric` overrides.
    #[must_use]
    pub fn from_row_metrics(row_count: u32, overrides: &[RowMetric]) -> Self {
        let count = row_count.clamp(1, MAX_ROWS) as usize;
        let default_h = DEFAULT_ROW_HEIGHT_PX.max(0.0);
        let mut base_sizes = vec![default_h; count];
        let mut hidden = vec![false; count];
        for metric in overrides {
            let r = metric.row as usize;
            if r < count {
                base_sizes[r] = metric.height_px.max(0.0);
                hidden[r] = metric.hidden;
            }
        }
        Self::from_parts(base_sizes, hidden)
    }

    /// Builds a column coordinate index for `col_count` columns (`<= MAX_COLS`) applying sparse `ColSpanMetric` overrides.
    #[must_use]
    pub fn from_col_metrics(col_count: u32, overrides: &[ColSpanMetric]) -> Self {
        let count = col_count.clamp(1, MAX_COLS) as usize;
        let default_w = DEFAULT_COL_WIDTH_PX.max(0.0);
        let mut base_sizes = vec![default_w; count];
        let mut hidden = vec![false; count];
        for span in overrides {
            let start = (span.start_col as usize).min(count);
            let end_excl = (span.end_col as usize).saturating_add(1).min(count);
            let w = span.width_px.max(0.0);
            for slot in &mut base_sizes[start..end_excl] {
                *slot = w;
            }
            for flag in &mut hidden[start..end_excl] {
                *flag = span.hidden;
            }
        }
        Self::from_parts(base_sizes, hidden)
    }

    /// Builds an axis index from explicit per-item visible sizes in logical pixels (`O(N)`).
    #[must_use]
    pub fn from_sizes(sizes: &[f32]) -> Self {
        let base_sizes: Vec<f32> = sizes.iter().map(|&s| s.max(0.0)).collect();
        let hidden = vec![false; base_sizes.len()];
        Self::from_parts(base_sizes, hidden)
    }

    /// Builds an axis index from explicit base sizes and hidden flags (`O(N)`).
    #[must_use]
    pub fn from_sizes_and_hidden(sizes: &[f32], hidden_flags: &[bool]) -> Self {
        let n = sizes.len();
        let base_sizes: Vec<f32> = sizes.iter().map(|&s| s.max(0.0)).collect();
        let mut hidden = vec![false; n];
        let copy_len = n.min(hidden_flags.len());
        hidden[..copy_len].copy_from_slice(&hidden_flags[..copy_len]);
        Self::from_parts(base_sizes, hidden)
    }

    fn from_parts(base_sizes: Vec<f32>, hidden: Vec<bool>) -> Self {
        let n = base_sizes.len();
        let mut bit = vec![0.0_f64; n + 1];
        let mut total = 0.0_f64;
        for i in 0..n {
            let eff = if hidden[i] {
                0.0
            } else {
                f64::from(base_sizes[i].max(0.0))
            };
            total += eff;
            let idx1 = i + 1;
            bit[idx1] += eff;
            let parent = idx1 + idx1.isolate_lowest_one();
            if parent <= n {
                bit[parent] += bit[idx1];
            }
        }
        let top_bit = highest_pow2_le(n);
        Self {
            bit,
            base_sizes,
            hidden,
            total,
            top_bit,
        }
    }

    /// Number of indexed spans (`rows` or `cols`).
    #[inline]
    #[must_use]
    pub fn count(&self) -> u32 {
        self.base_sizes.len() as u32
    }

    /// Total visible pixel extent across `0..count` in `O(1)` time.
    #[inline]
    #[must_use]
    pub fn total_extent(&self) -> f64 {
        self.total.max(0.0)
    }

    /// Effective logical-pixel size of `index` (`0.0` if hidden or out of bounds) in `O(1)` time.
    #[inline]
    #[must_use]
    pub fn span_size(&self, index: u32) -> f32 {
        let i = index as usize;
        if i < self.base_sizes.len() && !self.hidden[i] {
            self.base_sizes[i]
        } else {
            0.0
        }
    }

    /// Underlying unhidden pixel size of `index` (preserved while hidden) in `O(1)` time.
    #[inline]
    #[must_use]
    pub fn unhidden_size(&self, index: u32) -> f32 {
        self.base_sizes.get(index as usize).copied().unwrap_or(0.0)
    }

    /// Returns `true` if `index` is marked hidden (`O(1)`).
    #[inline]
    #[must_use]
    pub fn is_hidden(&self, index: u32) -> bool {
        self.hidden.get(index as usize).copied().unwrap_or(false)
    }

    /// Dynamically updates the unhidden pixel size of `index` in `O(log N)` time.
    pub fn set_span_size(&mut self, index: u32, size_px: f32) {
        let i = index as usize;
        if i >= self.base_sizes.len() {
            return;
        }
        let old_eff = f64::from(self.span_size(index));
        self.base_sizes[i] = size_px.max(0.0);
        let new_eff = f64::from(self.span_size(index));
        self.add_delta(i + 1, new_eff - old_eff);
    }

    /// Dynamically hides or unhides `index` in `O(log N)` time (preserving its `unhidden_size`).
    pub fn set_hidden(&mut self, index: u32, hide: bool) {
        let i = index as usize;
        if i >= self.hidden.len() || self.hidden[i] == hide {
            return;
        }
        let old_eff = f64::from(self.span_size(index));
        self.hidden[i] = hide;
        let new_eff = f64::from(self.span_size(index));
        self.add_delta(i + 1, new_eff - old_eff);
    }

    /// Updates the unhidden pixel size of every span in `range` (`O(|range| log N)`).
    pub fn set_range_size(&mut self, range: RangeInclusive<u32>, size_px: f32) {
        let count = self.count();
        if count == 0 || range.is_empty() {
            return;
        }
        let end = (*range.end()).min(count - 1);
        for idx in *range.start()..=end {
            self.set_span_size(idx, size_px);
        }
    }

    /// Hides or unhides every span in `range` (`O(|range| log N)`).
    pub fn set_range_hidden(&mut self, range: RangeInclusive<u32>, hide: bool) {
        let count = self.count();
        if count == 0 || range.is_empty() {
            return;
        }
        let end = (*range.end()).min(count - 1);
        for idx in *range.start()..=end {
            self.set_hidden(idx, hide);
        }
    }

    fn add_delta(&mut self, mut idx1: usize, delta: f64) {
        if delta == 0.0 {
            return;
        }
        self.total = (self.total + delta).max(0.0);
        let limit = self.base_sizes.len();
        while idx1 <= limit {
            self.bit[idx1] = (self.bit[idx1] + delta).max(0.0);
            idx1 += idx1.isolate_lowest_one();
        }
    }

    /// Cumulative logical-pixel start offset of `index` (`sum(0..index)`) in `O(log N)` time.
    #[inline]
    #[must_use]
    pub fn index_to_offset(&self, index: u32) -> f64 {
        let n = self.base_sizes.len();
        let mut idx = (index as usize).min(n);
        if idx == n {
            return self.total_extent();
        }
        let mut sum = 0.0_f64;
        while idx > 0 {
            sum += self.bit[idx];
            idx &= idx - 1;
        }
        sum
    }

    /// Cumulative logical-pixel end offset of `index` (`index_to_offset(index) + span_size(index)`) in `O(log N)` time.
    #[inline]
    #[must_use]
    pub fn index_end_offset(&self, index: u32) -> f64 {
        let i = index as usize;
        if i >= self.base_sizes.len() {
            self.total_extent()
        } else {
            self.index_to_offset(index) + f64::from(self.span_size(index))
        }
    }

    /// Pixel extent of `[start_index..end_index_exclusive]` in `O(log N)` time.
    #[inline]
    #[must_use]
    pub fn range_extent(&self, start_index: u32, end_index_exclusive: u32) -> f64 {
        if end_index_exclusive <= start_index {
            return 0.0;
        }
        (self.index_to_offset(end_index_exclusive) - self.index_to_offset(start_index)).max(0.0)
    }

    /// Maps a cumulative pixel coordinate `px_offset` to its containing span index in `O(log N)` time
    /// via Fenwick-tree binary lifting.
    ///
    /// Hidden (`0.0`-size) spans are automatically skipped forward to the first positive-size span
    /// sharing that boundary when available; if `px_offset >= total_extent`, returns `count - 1`.
    #[must_use]
    pub fn offset_to_index(&self, px_offset: f64) -> u32 {
        let n = self.base_sizes.len();
        if n == 0 {
            return 0;
        }
        let total = self.total_extent();
        if total <= 0.0 {
            return (n - 1) as u32;
        }
        if px_offset <= 0.0 {
            return self.lift_largest_le(0.0).min(n - 1) as u32;
        }
        if px_offset >= total {
            return (n - 1) as u32;
        }

        let mut idx = self.lift_largest_le(px_offset).min(n - 1) as u32;
        // Guard against 1-ULP floating-point rounding between top-down lifting and bottom-up prefix sums.
        while idx > 0 && self.index_to_offset(idx) > px_offset {
            idx -= 1;
        }
        while (idx as usize) + 1 < n && self.index_to_offset(idx + 1) <= px_offset {
            idx += 1;
        }
        idx
    }

    /// Fenwick binary lifting: returns the largest `pos` in `0..=N` whose prefix sum is `<= target`.
    #[inline]
    fn lift_largest_le(&self, target: f64) -> usize {
        let n = self.base_sizes.len();
        let mut pos = 0usize;
        let mut acc = 0.0_f64;
        let mut step = self.top_bit;
        while step > 0 {
            let next = pos + step;
            if next <= n && acc + self.bit[next] <= target {
                pos = next;
                acc += self.bit[next];
            }
            step >>= 1;
        }
        pos
    }

    /// Computes the inclusive index range `[first..=last]` intersecting `[window_start_px, window_end_px)`
    /// within the unfrozen region `[min_index..count)` in `O(log N)` time.
    #[must_use]
    pub fn visible_range(
        &self,
        min_index: u32,
        window_start_px: f64,
        viewport_extent_px: f64,
    ) -> Option<RangeInclusive<u32>> {
        let count = self.count();
        if min_index >= count || viewport_extent_px <= 0.0 {
            return None;
        }
        let base_px = self.index_to_offset(min_index);
        let start_px = window_start_px.max(base_px);
        let end_px = (start_px + viewport_extent_px).min(self.total_extent());
        if end_px <= base_px {
            return None;
        }
        let first = self.offset_to_index(start_px).max(min_index);
        let probe_end = (end_px - 1e-4).max(start_px);
        let last = self.offset_to_index(probe_end).max(first).min(count - 1);
        Some(first..=last)
    }

    /// Returns `true` if one or more hidden (`0.0`-size) spans immediately precede `index` (`O(1)`).
    #[inline]
    #[must_use]
    pub fn has_hidden_before(&self, index: u32) -> bool {
        let i = index as usize;
        i > 0 && i <= self.base_sizes.len() && self.span_size(index - 1) <= 0.0
    }

    /// Returns the contiguous inclusive range of hidden (`0.0`-size) spans immediately preceding `index`, if any.
    #[must_use]
    pub fn hidden_gap_before(&self, index: u32) -> Option<RangeInclusive<u32>> {
        if !self.has_hidden_before(index) {
            return None;
        }
        let end = index - 1;
        let mut start = end;
        while start > 0 && self.span_size(start - 1) <= 0.0 {
            start -= 1;
        }
        Some(start..=end)
    }

    /// Finds the next visible (`span_size > 0`) index strictly greater than `index`, if any.
    #[must_use]
    pub fn next_visible(&self, index: u32) -> Option<u32> {
        let count = self.count();
        let mut cur = index.checked_add(1)?;
        while cur < count {
            if self.span_size(cur) > 0.0 {
                return Some(cur);
            }
            cur += 1;
        }
        None
    }

    /// Finds the previous visible (`span_size > 0`) index strictly less than `index`, if any.
    #[must_use]
    pub fn prev_visible(&self, index: u32) -> Option<u32> {
        let mut cur = index.checked_sub(1)?;
        loop {
            if self.span_size(cur) > 0.0 {
                return Some(cur);
            }
            cur = cur.checked_sub(1)?;
        }
    }
}

#[inline]
const fn highest_pow2_le(n: usize) -> usize {
    if n == 0 {
        0
    } else {
        1usize << (usize::BITS - 1 - n.leading_zeros())
    }
}
