//! Bounded single-line `ShapedLine` cache keyed by text, font attributes, color, and `rem` scale.

use gpui_kit::{
    Font, FontStyle as GpuiFontStyle, FontWeight, Hsla, Pixels, ShapedLine, SharedString, TextRun,
    Window,
};
use std::collections::HashMap;

/// Maximum number of shaped text lines retained before resetting the cache.
const MAX_CACHED_LINES: usize = 4_096;

#[derive(Clone, PartialEq, Eq, Hash)]
struct LineCacheKey {
    text: SharedString,
    font_family: SharedString,
    font_size_bits: u32,
    bold: bool,
    italic: bool,
    color_h_bits: u32,
    color_s_bits: u32,
    color_l_bits: u32,
    color_a_bits: u32,
}

/// Window-local shaped text cache avoiding repeated HarfBuzz/CoreText shaping during rapid scrolling.
///
/// Uses two-generation aging (`current` + `previous`) so eviction never causes sudden cold-cache
/// frame spikes, and automatically invalidates whenever `window.rem_size()` changes.
#[derive(Default)]
pub struct ShapedLineCache {
    current: HashMap<LineCacheKey, ShapedLine>,
    previous: HashMap<LineCacheKey, ShapedLine>,
    last_rem_bits: u32,
}

impl ShapedLineCache {
    /// Creates an empty shaped-line cache.
    #[must_use]
    pub fn new() -> Self {
        Self {
            current: HashMap::with_capacity(512),
            previous: HashMap::with_capacity(512),
            last_rem_bits: 0,
        }
    }

    /// Shapes `text` or returns an already-shaped [`ShapedLine`] for matching style and `rem` size.
    #[allow(clippy::too_many_arguments)]
    pub fn shape_or_get(
        &mut self,
        window: &Window,
        text: SharedString,
        font_family: SharedString,
        font_size: Pixels,
        bold: bool,
        italic: bool,
        color: Hsla,
    ) -> ShapedLine {
        let rem_bits = f32::from(window.rem_size()).to_bits();
        if self.last_rem_bits != rem_bits {
            self.current.clear();
            self.previous.clear();
            self.last_rem_bits = rem_bits;
        }

        let key = LineCacheKey {
            text: text.clone(),
            font_family: font_family.clone(),
            font_size_bits: f32::from(font_size).to_bits(),
            bold,
            italic,
            color_h_bits: color.h.to_bits(),
            color_s_bits: color.s.to_bits(),
            color_l_bits: color.l.to_bits(),
            color_a_bits: color.a.to_bits(),
        };

        if let Some(existing) = self.current.get(&key) {
            return existing.clone();
        }
        if let Some(promoted) = self.previous.remove(&key) {
            if self.current.len() >= MAX_CACHED_LINES {
                std::mem::swap(&mut self.previous, &mut self.current);
                self.current.clear();
            }
            self.current.insert(key, promoted.clone());
            return promoted;
        }

        if self.current.len() >= MAX_CACHED_LINES {
            std::mem::swap(&mut self.previous, &mut self.current);
            self.current.clear();
        }

        let run = TextRun {
            len: text.len(),
            font: Font {
                family: font_family,
                features: Default::default(),
                fallbacks: None,
                weight: if bold {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                },
                style: if italic {
                    GpuiFontStyle::Italic
                } else {
                    GpuiFontStyle::Normal
                },
            },
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };

        let shaped = window
            .text_system()
            .shape_line(text, font_size, &[run], None);
        self.current.insert(key, shaped.clone());
        shaped
    }
}
