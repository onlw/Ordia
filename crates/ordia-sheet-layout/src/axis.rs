//! Prefix-sum 1D coordinate index for virtualized row/column positioning and `O(log N)` hit-testing.

use ordia_sheet_model::{
    ColSpanMetric, DEFAULT_COL_WIDTH_PX, DEFAULT_ROW_HEIGHT_PX, MAX_COLS, MAX_ROWS, RowMetric,
};
use std::ops::RangeInclusive;

/// Prefix-sum coordinate index over `N` rows or columns (`offsets.len() == N + 1`).
///
/// `offsets[i]` stores the cumulative logical-pixel start coordinate of index `i`, and
/// `offsets[i + 1] - offsets[i]` equals the logical-pixel span of index `i` (`0.0` if hidden).
/// Uses `f64` prefix sums so `1,048,576 * 25.0 = 26,214,400.0` retains sub-pixel precision.
#[derive(Debug, Clone)]
pub struct GridAxisIndex {
    offsets: Vec<f64>,
}

impl GridAxisIndex {
    /// Builds a uniform axis index with `count` spans of `default_size_px`.
    #[must_use]
    pub fn uniform(count: u32, default_size_px: f32) -> Self {
        let n = count as usize;
        let step = f64::from(default_size_px.max(0.0));
        let mut offsets = Vec::with_capacity(n + 1);
        for i in 0..=n {
            offsets.push((i as f64) * step);
        }
        Self { offsets }
    }

    /// Builds a row coordinate index for `row_count` rows (`<= MAX_ROWS`) applying sparse `RowMetric` overrides.
    #[must_use]
    pub fn from_row_metrics(row_count: u32, overrides: &[RowMetric]) -> Self {
        let count = row_count.clamp(1, MAX_ROWS) as usize;
        let default_h = DEFAULT_ROW_HEIGHT_PX.max(0.0);
        let mut sizes = vec![default_h; count];
        for metric in overrides {
            let r = metric.row as usize;
            if r < count {
                sizes[r] = if metric.hidden {
                    0.0
                } else {
                    metric.height_px.max(0.0)
                };
            }
        }
        Self::from_sizes(&sizes)
    }

    /// Builds a column coordinate index for `col_count` columns (`<= MAX_COLS`) applying sparse `ColSpanMetric` overrides.
    #[must_use]
    pub fn from_col_metrics(col_count: u32, overrides: &[ColSpanMetric]) -> Self {
        let count = col_count.clamp(1, MAX_COLS) as usize;
        let default_w = DEFAULT_COL_WIDTH_PX.max(0.0);
        let mut sizes = vec![default_w; count];
        for span in overrides {
            let start = (span.start_col as usize).min(count);
            let end_excl = (span.end_col as usize).saturating_add(1).min(count);
            let w = if span.hidden {
                0.0
            } else {
                span.width_px.max(0.0)
            };
            for slot in &mut sizes[start..end_excl] {
                *slot = w;
            }
        }
        Self::from_sizes(&sizes)
    }

    /// Builds an axis index from explicit per-item sizes in logical pixels.
    #[must_use]
    pub fn from_sizes(sizes: &[f32]) -> Self {
        let mut offsets = Vec::with_capacity(sizes.len() + 1);
        let mut acc = 0.0_f64;
        offsets.push(0.0);
        for &sz in sizes {
            acc += f64::from(sz.max(0.0));
            offsets.push(acc);
        }
        Self { offsets }
    }

    /// Number of indexed spans (`rows` or `cols`).
    #[must_use]
    pub fn count(&self) -> u32 {
        self.offsets.len().saturating_sub(1) as u32
    }

    /// Total pixel extent from index `0` to `count`.
    #[must_use]
    pub fn total_extent(&self) -> f64 {
        self.offsets.last().copied().unwrap_or(0.0)
    }

    /// Cumulative logical-pixel start offset of `index` in `O(1)` time.
    #[must_use]
    pub fn index_to_offset(&self, index: u32) -> f64 {
        let ix = (index as usize).min(self.offsets.len().saturating_sub(1));
        self.offsets[ix]
    }

    /// Cumulative logical-pixel end offset of `index` (`index_to_offset(index + 1)`) in `O(1)` time.
    #[must_use]
    pub fn index_end_offset(&self, index: u32) -> f64 {
        let next = (index as usize)
            .saturating_add(1)
            .min(self.offsets.len().saturating_sub(1));
        self.offsets[next]
    }

    /// Logical-pixel size of `index` (`0.0` if hidden or out of bounds) in `O(1)` time.
    #[must_use]
    pub fn span_size(&self, index: u32) -> f32 {
        let i = index as usize;
        if i + 1 < self.offsets.len() {
            (self.offsets[i + 1] - self.offsets[i]) as f32
        } else {
            0.0
        }
    }

    /// Pixel extent of `[start_index..end_index_exclusive]` in `O(1)` time.
    #[must_use]
    pub fn range_extent(&self, start_index: u32, end_index_exclusive: u32) -> f64 {
        if end_index_exclusive <= start_index {
            return 0.0;
        }
        (self.index_to_offset(end_index_exclusive) - self.index_to_offset(start_index)).max(0.0)
    }

    /// Maps a cumulative pixel coordinate `px_offset` to its containing span index in `O(log N)` time.
    ///
    /// If `px_offset <= 0`, returns the first visible span (or `0`); if `px_offset >= total_extent`,
    /// returns `count - 1`. Hidden (`0.0`-size) spans are skipped forward to the first positive-size span
    /// sharing that boundary when available.
    #[must_use]
    pub fn offset_to_index(&self, px_offset: f64) -> u32 {
        let count = self.count();
        if count == 0 {
            return 0;
        }
        if px_offset <= 0.0 {
            return self.skip_hidden_forward(0);
        }
        let total = self.total_extent();
        if px_offset >= total {
            return count - 1;
        }
        // Find the largest `i` in `0..count` where `offsets[i] <= px_offset`.
        // `partition_point(|&off| off <= px_offset)` gives the first index `p` where `offsets[p] > px_offset`,
        // so `p - 1` is the exact visible span containing `px_offset` (naturally skipping zero-width spans!).
        let p = self.offsets.partition_point(|&off| off <= px_offset);
        let idx = p.saturating_sub(1).min((count - 1) as usize);
        idx as u32
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
        // Subtract a tiny epsilon so exact cell boundary `end_px == offsets[k]` doesn't pull in an extra invisible cell,
        // while still clamping >= start_px.
        let probe_end = (end_px - 1e-4).max(start_px);
        let last = self.offset_to_index(probe_end).max(first).min(count - 1);
        Some(first..=last)
    }

    fn skip_hidden_forward(&self, mut index: u32) -> u32 {
        let count = self.count();
        while index + 1 < count && self.span_size(index) <= 0.0 {
            index += 1;
        }
        index
    }
}
