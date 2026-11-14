//! Platform-independent `DisplayList` rendering intermediate representation for Ordia.
//!
//! Kernel layout crates emit `DisplayList` commands without touching GPUI or PDF backends.
//! Backends (`ordia-render-gpui`, `ordia-render-pdf`, and `tiny-skia` test harnesses)
//! consume the same `DisplayList` deterministically.

use serde::{Deserialize, Serialize};

/// 2D point in logical points / pixels (origin at top-left, +x right, +y down).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// Horizontal coordinate.
    pub x: f32,
    /// Vertical coordinate.
    pub y: f32,
}

impl Point {
    /// Creates a point at `(x, y)`.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// 2D size in logical points / pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Size {
    /// Width extent.
    pub width: f32,
    /// Height extent.
    pub height: f32,
}

impl Size {
    /// Creates a size `(width, height)`.
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// Axis-aligned rectangle in logical points / pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RectF {
    /// Left coordinate.
    pub x: f32,
    /// Top coordinate.
    pub y: f32,
    /// Horizontal span.
    pub width: f32,
    /// Vertical span.
    pub height: f32,
}

impl RectF {
    /// Creates an axis-aligned rectangle `(x, y, width, height)`.
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Right edge (`x + width`).
    pub fn right(self) -> f32 {
        self.x + self.width
    }

    /// Bottom edge (`y + height`).
    pub fn bottom(self) -> f32 {
        self.y + self.height
    }
}

/// Straight-alpha sRGB color (`0..=255` per channel).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Color {
    /// Red component.
    pub r: u8,
    /// Green component.
    pub g: u8,
    /// Blue component.
    pub b: u8,
    /// Alpha component (`255` = opaque).
    pub a: u8,
}

impl Color {
    /// Opaque black (`#000000`).
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    /// Opaque white (`#FFFFFF`).
    pub const WHITE: Self = Self::rgb(255, 255, 255);

    /// Creates an opaque RGB color.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Creates an RGBA color.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::BLACK
    }
}

/// Stroke style for lines and rectangle outlines.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StrokeStyle {
    /// Stroke thickness in points / pixels.
    pub width: f32,
    /// Stroke color.
    pub color: Color,
    /// Optional `[dash_len, gap_len]` pattern (`None` = solid).
    pub dash: Option<[f32; 2]>,
}

impl StrokeStyle {
    /// Solid stroke with given width and color.
    pub const fn solid(width: f32, color: Color) -> Self {
        Self {
            width,
            color,
            dash: None,
        }
    }
}

/// Font weight classification for `DrawText` runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FontWeight {
    /// Normal weight (`400`).
    #[default]
    Regular,
    /// Medium weight (`500`).
    Medium,
    /// Bold weight (`700`).
    Bold,
}

/// Text styling attributes for [`RenderCommand::DrawText`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextRunStyle {
    /// Preferred font family name (`None` = default UI/spreadsheet font).
    pub font_family: Option<String>,
    /// Font size in points / pixels.
    pub font_size: f32,
    /// Font weight.
    pub weight: FontWeight,
    /// Italic style flag.
    pub italic: bool,
    /// Text fill color.
    pub color: Color,
}

impl Default for TextRunStyle {
    fn default() -> Self {
        Self {
            font_family: None,
            font_size: 11.0,
            weight: FontWeight::Regular,
            italic: false,
            color: Color::BLACK,
        }
    }
}

/// Single drawing command inside a [`DisplayList`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RenderCommand {
    /// Fills a solid (optionally rounded) rectangle.
    FillRect {
        /// Destination bounds.
        rect: RectF,
        /// Fill color.
        color: Color,
        /// Corner radius (`0.0` for sharp corners).
        corner_radius: f32,
    },
    /// Strokes the outline of a rectangle.
    StrokeRect {
        /// Outline bounds.
        rect: RectF,
        /// Stroke style.
        stroke: StrokeStyle,
    },
    /// Draws a straight line segment between `start` and `end`.
    DrawLine {
        /// Segment start point.
        start: Point,
        /// Segment end point.
        end: Point,
        /// Stroke style.
        stroke: StrokeStyle,
    },
    /// Draws a single-line text fragment anchored by its baseline origin.
    DrawText {
        /// Baseline start point `(x, baseline_y)`.
        origin: Point,
        /// UTF-8 text run.
        text: String,
        /// Font and fill attributes.
        style: TextRunStyle,
    },
    /// Intersects the active clip region with `rect`. Must be paired with [`Self::PopClip`].
    PushClip {
        /// Clip rectangle.
        rect: RectF,
    },
    /// Restores the clip region pushed by the matching [`Self::PushClip`].
    PopClip,
}

/// Ordered sequence of platform-independent drawing primitives for a surface or page.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DisplayList {
    /// Surface or page width in points / pixels.
    pub width: f32,
    /// Surface or page height in points / pixels.
    pub height: f32,
    /// Ordered drawing commands.
    pub commands: Vec<RenderCommand>,
}

impl DisplayList {
    /// Creates an empty display list with the given dimensions.
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            commands: Vec::new(),
        }
    }

    /// Appends a filled rectangle command.
    pub fn fill_rect(&mut self, rect: RectF, color: Color) {
        self.commands.push(RenderCommand::FillRect {
            rect,
            color,
            corner_radius: 0.0,
        });
    }

    /// Appends a rounded filled rectangle command.
    pub fn fill_rounded_rect(&mut self, rect: RectF, color: Color, corner_radius: f32) {
        self.commands.push(RenderCommand::FillRect {
            rect,
            color,
            corner_radius,
        });
    }

    /// Appends a stroked rectangle command.
    pub fn stroke_rect(&mut self, rect: RectF, stroke: StrokeStyle) {
        self.commands
            .push(RenderCommand::StrokeRect { rect, stroke });
    }

    /// Appends a line segment command.
    pub fn draw_line(&mut self, start: Point, end: Point, stroke: StrokeStyle) {
        self.commands
            .push(RenderCommand::DrawLine { start, end, stroke });
    }

    /// Appends a text fragment command positioned by `(x, baseline_y)`.
    pub fn draw_text(&mut self, origin: Point, text: impl Into<String>, style: TextRunStyle) {
        let text = text.into();
        if text.is_empty() {
            return;
        }
        self.commands.push(RenderCommand::DrawText {
            origin,
            text,
            style,
        });
    }

    /// Pushes a rectangular clip region.
    pub fn push_clip(&mut self, rect: RectF) {
        self.commands.push(RenderCommand::PushClip { rect });
    }

    /// Pops the most recently pushed clip region.
    pub fn pop_clip(&mut self) {
        self.commands.push(RenderCommand::PopClip);
    }

    /// Executes `f` bracketed by matched `PushClip` and `PopClip` commands.
    pub fn with_clip(&mut self, rect: RectF, f: impl FnOnce(&mut Self)) {
        self.push_clip(rect);
        f(self);
        self.pop_clip();
    }

    /// Verifies that every `PushClip` has a matching `PopClip` and clip depth never underflows.
    pub fn has_balanced_clips(&self) -> bool {
        let mut depth: i32 = 0;
        for cmd in &self.commands {
            match cmd {
                RenderCommand::PushClip { .. } => depth += 1,
                RenderCommand::PopClip => {
                    depth -= 1;
                    if depth < 0 {
                        return false;
                    }
                }
                _ => {}
            }
        }
        depth == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_list_clip_stack_validation() {
        let mut dl = DisplayList::new(400.0, 300.0);
        dl.fill_rect(RectF::new(0.0, 0.0, 400.0, 300.0), Color::WHITE);
        dl.with_clip(RectF::new(10.0, 10.0, 120.0, 24.0), |inner| {
            inner.draw_text(
                Point::new(14.0, 26.0),
                "预算汇总",
                TextRunStyle {
                    weight: FontWeight::Bold,
                    ..TextRunStyle::default()
                },
            );
        });
        assert!(dl.has_balanced_clips());
        assert_eq!(dl.commands.len(), 4);

        dl.pop_clip();
        assert!(!dl.has_balanced_clips());
    }
}
