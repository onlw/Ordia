//! Zero-allocation rolling frame-time ring buffer for the grid performance overlay.

/// Number of recent frame samples kept in the ring buffer.
const RING_CAPACITY: usize = 240;

/// Summary statistics over recent grid layout + paint frame durations in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub struct FrameTimeSnapshot {
    /// Most recent frame layout + paint time in milliseconds.
    pub last_ms: f32,
    /// Rolling arithmetic mean in milliseconds.
    pub avg_ms: f32,
    /// Rolling 50th percentile (median) in milliseconds.
    pub p50_ms: f32,
    /// Rolling 95th percentile in milliseconds.
    pub p95_ms: f32,
    /// Rolling 99th percentile in milliseconds.
    pub p99_ms: f32,
    /// Rolling maximum in milliseconds.
    pub max_ms: f32,
    /// Total number of recorded frames since tracker creation or reset.
    pub total_frames: u64,
}

/// Fixed-size ring buffer tracking recent grid layout + paint durations without heap allocation.
#[derive(Debug, Clone)]
pub struct FrameTimeTracker {
    samples_ms: [f32; RING_CAPACITY],
    write_ix: usize,
    filled: usize,
    total_frames: u64,
    last_ms: f32,
}

impl Default for FrameTimeTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameTimeTracker {
    /// Creates an empty tracker.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            samples_ms: [0.0; RING_CAPACITY],
            write_ix: 0,
            filled: 0,
            total_frames: 0,
            last_ms: 0.0,
        }
    }

    /// Records a single frame's layout + paint duration in milliseconds.
    pub fn record_ms(&mut self, duration_ms: f32) {
        let clamped = duration_ms.max(0.0);
        self.samples_ms[self.write_ix] = clamped;
        self.write_ix = (self.write_ix + 1) % RING_CAPACITY;
        if self.filled < RING_CAPACITY {
            self.filled += 1;
        }
        self.total_frames += 1;
        self.last_ms = clamped;
    }

    /// Resets all accumulated samples (used before starting an isolated scroll benchmark).
    pub fn reset(&mut self) {
        self.samples_ms = [0.0; RING_CAPACITY];
        self.write_ix = 0;
        self.filled = 0;
        self.total_frames = 0;
        self.last_ms = 0.0;
    }

    /// Computes summary metrics (`last`, `avg`, `p50`, `p95`, `p99`, `max`) using a stack-allocated sort buffer.
    #[must_use]
    pub fn snapshot(&self) -> FrameTimeSnapshot {
        if self.filled == 0 {
            return FrameTimeSnapshot::default();
        }
        let mut scratch = [0.0_f32; RING_CAPACITY];
        let slice = &mut scratch[..self.filled];
        slice.copy_from_slice(&self.samples_ms[..self.filled]);
        slice.sort_by(f32::total_cmp);

        let sum: f32 = slice.iter().copied().sum();
        let avg_ms = sum / (self.filled as f32);
        let p50_ix = (self.filled - 1) / 2;
        let p95_ix = ((self.filled - 1) * 95) / 100;
        let p99_ix = ((self.filled - 1) * 99) / 100;
        let max_ms = slice[self.filled - 1];

        FrameTimeSnapshot {
            last_ms: self.last_ms,
            avg_ms,
            p50_ms: slice[p50_ix],
            p95_ms: slice[p95_ix],
            p99_ms: slice[p99_ix],
            max_ms,
            total_frames: self.total_frames,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracker_computes_percentiles_without_panicking() {
        let mut tracker = FrameTimeTracker::new();
        for i in 1..=100 {
            tracker.record_ms(i as f32 * 0.1);
        }
        let snap = tracker.snapshot();
        assert_eq!(snap.total_frames, 100);
        assert!((snap.last_ms - 10.0).abs() < 1e-4);
        assert!((snap.p50_ms - 5.0).abs() < 0.2);
        assert!((snap.p95_ms - 9.5).abs() < 0.2);
        assert!((snap.p99_ms - 9.9).abs() < 0.2);
        assert!((snap.max_ms - 10.0).abs() < 1e-4);
    }
}
