//! GPUI rendering backend translating platform-independent [`ordia_render::DisplayList`] commands
//! into GPU quads, paths, content masks, and cached shaped text runs (`M2-02`, `M2-03`).

use std::collections::HashMap;

use gpui_kit::{
    App, BorderStyle, Bounds, ContentMask, Font, FontStyle as GpuiFontStyle,
    FontWeight as GpuiFontWeight, Hsla, PathBuilder, Pixels, Point as GpuiPoint, ShapedLine,
    SharedString, StrikethroughStyle, TextAlign, TextRun, UnderlineStyle, Window, fill, outline,
    point, px, rgba, size,
};
use ordia_render::{
    Color, DisplayList, FontWeight, RectF, RenderCommand, TextHAlign, TextRunStyle,
};

const MAX_CACHED_SHAPED_LINES: usize = 4_096;

#[derive(Clone, PartialEq, Eq, Hash)]
struct ShapedKey {
    text: SharedString,
    family: SharedString,
    size_bits: u32,
    weight: FontWeight,
    italic: bool,
    underline: bool,
    strikethrough: bool,
    rgba_packed: u32,
}

/// Two-generation `ShapedLine` cache accelerating [`paint_display_list_cached`] during rapid scrolling.
#[derive(Default)]
pub struct ShapedTextCache {
    current: HashMap<ShapedKey, ShapedLine>,
    previous: HashMap<ShapedKey, ShapedLine>,
    last_rem_bits: u32,
}

impl ShapedTextCache {
    /// Creates an empty two-generation shaped-line cache.
    #[must_use]
    pub fn new() -> Self {
        Self {
            current: HashMap::with_capacity(512),
            previous: HashMap::with_capacity(512),
            last_rem_bits: 0,
        }
    }

    /// Shapes `text` or returns an already-shaped [`ShapedLine`].
    pub fn shape_or_get(
        &mut self,
        window: &Window,
        text: SharedString,
        family: SharedString,
        style: &TextRunStyle,
    ) -> ShapedLine {
        let rem_bits = f32::from(window.rem_size()).to_bits();
        if self.last_rem_bits != rem_bits {
            self.current.clear();
            self.previous.clear();
            self.last_rem_bits = rem_bits;
        }
        let packed =
            u32::from_be_bytes([style.color.r, style.color.g, style.color.b, style.color.a]);
        let key = ShapedKey {
            text: text.clone(),
            family: family.clone(),
            size_bits: style.font_size.to_bits(),
            weight: style.weight,
            italic: style.italic,
            underline: style.underline,
            strikethrough: style.strikethrough,
            rgba_packed: packed,
        };
        if let Some(hit) = self.current.get(&key) {
            return hit.clone();
        }
        if let Some(promoted) = self.previous.remove(&key) {
            if self.current.len() >= MAX_CACHED_SHAPED_LINES {
                std::mem::swap(&mut self.previous, &mut self.current);
                self.current.clear();
            }
            self.current.insert(key, promoted.clone());
            return promoted;
        }
        if self.current.len() >= MAX_CACHED_SHAPED_LINES {
            std::mem::swap(&mut self.previous, &mut self.current);
            self.current.clear();
        }
        let shaped = shape_uncached_line(window, text, family, style);
        self.current.insert(key, shaped.clone());
        shaped
    }
}

fn shape_uncached_line(
    window: &Window,
    text: SharedString,
    family: SharedString,
    style: &TextRunStyle,
) -> ShapedLine {
    let gpui_weight = match style.weight {
        FontWeight::Regular => GpuiFontWeight::NORMAL,
        FontWeight::Medium => GpuiFontWeight::MEDIUM,
        FontWeight::Bold => GpuiFontWeight::BOLD,
    };
    let hsla_color = to_hsla(style.color);
    let run = TextRun {
        len: text.len(),
        font: Font {
            family,
            features: Default::default(),
            fallbacks: None,
            weight: gpui_weight,
            style: if style.italic {
                GpuiFontStyle::Italic
            } else {
                GpuiFontStyle::Normal
            },
        },
        color: hsla_color,
        background_color: None,
        underline: style.underline.then_some(UnderlineStyle {
            thickness: px(1.0),
            color: Some(hsla_color),
            wavy: false,
        }),
        strikethrough: style.strikethrough.then_some(StrikethroughStyle {
            thickness: px(1.0),
            color: Some(hsla_color),
        }),
    };
    window
        .text_system()
        .shape_line(text, px(style.font_size), &[run], None)
}

fn to_hsla(c: Color) -> Hsla {
    Hsla::from(rgba(u32::from_be_bytes([c.r, c.g, c.b, c.a])))
}

fn to_bounds_scaled(origin: GpuiPoint<Pixels>, r: RectF, s: f32) -> Bounds<Pixels> {
    Bounds::new(
        point(origin.x + px(r.x * s), origin.y + px(r.y * s)),
        size(px((r.width * s).max(0.0)), px((r.height * s).max(0.0))),
    )
}

/// Paints `list` onto `window` anchored at `origin` without an external shaped-line cache.
pub fn paint_display_list(
    list: &DisplayList,
    origin: GpuiPoint<Pixels>,
    default_font_family: &SharedString,
    window: &mut Window,
    cx: &mut App,
) {
    paint_slice(
        &list.commands,
        origin,
        1.0,
        default_font_family,
        None,
        window,
        cx,
    );
}

/// Paints `list` onto `window` anchored at `origin`, reusing shaped text lines in `cache`.
pub fn paint_display_list_cached(
    list: &DisplayList,
    origin: GpuiPoint<Pixels>,
    default_font_family: &SharedString,
    cache: &mut ShapedTextCache,
    window: &mut Window,
    cx: &mut App,
) {
    paint_slice(
        &list.commands,
        origin,
        1.0,
        default_font_family,
        Some(cache),
        window,
        cx,
    );
}

/// Paints `list` onto `window` anchored at `origin` with lossless vector `scale` (`0.10..=4.0`).
#[allow(clippy::too_many_arguments)]
pub fn paint_display_list_scaled(
    list: &DisplayList,
    origin: GpuiPoint<Pixels>,
    scale: f32,
    default_font_family: &SharedString,
    cache: &mut ShapedTextCache,
    window: &mut Window,
    cx: &mut App,
) {
    paint_slice(
        &list.commands,
        origin,
        scale.clamp(0.10, 4.0),
        default_font_family,
        Some(cache),
        window,
        cx,
    );
}

fn paint_slice(
    commands: &[RenderCommand],
    origin: GpuiPoint<Pixels>,
    scale: f32,
    default_font_family: &SharedString,
    mut cache: Option<&mut ShapedTextCache>,
    window: &mut Window,
    cx: &mut App,
) {
    let mut idx = 0;
    while idx < commands.len() {
        match &commands[idx] {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } => {
                let mut quad = fill(to_bounds_scaled(origin, *rect, scale), to_hsla(*color));
                if *corner_radius > 0.0 {
                    quad.corner_radii = px(*corner_radius * scale).into();
                }
                window.paint_quad(quad);
                idx += 1;
            }
            RenderCommand::StrokeRect { rect, stroke } => {
                let mut quad = outline(
                    to_bounds_scaled(origin, *rect, scale),
                    to_hsla(stroke.color),
                    if stroke.dash.is_some() {
                        BorderStyle::Dashed
                    } else {
                        BorderStyle::Solid
                    },
                );
                quad.border_widths = px((stroke.width * scale).max(0.5)).into();
                window.paint_quad(quad);
                idx += 1;
            }
            RenderCommand::DrawLine { start, end, stroke } => {
                let mut pb = PathBuilder::stroke(px((stroke.width * scale).max(0.5)));
                if let Some(pat) = stroke.dash {
                    let mut px_buf = [px(0.0); 6];
                    let slice = pat.as_slice();
                    for (dst, &v) in px_buf.iter_mut().zip(slice.iter()) {
                        *dst = px((v * scale).max(0.5));
                    }
                    pb = pb.dash_array(&px_buf[..slice.len()]);
                }
                pb.move_to(point(
                    origin.x + px(start.x * scale),
                    origin.y + px(start.y * scale),
                ));
                pb.line_to(point(
                    origin.x + px(end.x * scale),
                    origin.y + px(end.y * scale),
                ));
                if let Ok(path) = pb.build() {
                    window.paint_path(path, to_hsla(stroke.color));
                }
                idx += 1;
            }
            RenderCommand::DrawText {
                origin: text_origin,
                text,
                style,
            } => {
                let family = style
                    .font_family
                    .as_deref()
                    .map(|s| SharedString::from(s.to_string()))
                    .unwrap_or_else(|| default_font_family.clone());
                let shared_text = SharedString::from(text.clone());
                let mut eff_style = style.clone();
                eff_style.font_size = (style.font_size * scale).clamp(3.0, 240.0);
                eff_style.max_width = style.max_width.map(|w| w * scale);
                let shaped = if let Some(c) = cache.as_deref_mut() {
                    c.shape_or_get(window, shared_text, family, &eff_style)
                } else {
                    shape_uncached_line(window, shared_text, family, &eff_style)
                };
                let align = match eff_style.h_align {
                    TextHAlign::Left => TextAlign::Left,
                    TextHAlign::Center => TextAlign::Center,
                    TextHAlign::Right => TextAlign::Right,
                };
                let top_y = text_origin.y * scale - (eff_style.font_size * 0.82);
                let _ = shaped.paint(
                    point(origin.x + px(text_origin.x * scale), origin.y + px(top_y)),
                    px(eff_style.font_size * 1.2),
                    align,
                    eff_style.max_width.map(px),
                    window,
                    cx,
                );
                idx += 1;
            }
            RenderCommand::PushClip { rect } => {
                let close_idx = find_matching_pop_clip(&commands[idx + 1..])
                    .map(|rel| idx + 1 + rel)
                    .unwrap_or(commands.len());
                let sub = &commands[idx + 1..close_idx];
                let mask = ContentMask {
                    bounds: to_bounds_scaled(origin, *rect, scale),
                };
                window.with_content_mask(Some(mask), |window| {
                    paint_slice(
                        sub,
                        origin,
                        scale,
                        default_font_family,
                        cache.as_deref_mut(),
                        window,
                        cx,
                    );
                });
                idx = (close_idx + 1).min(commands.len());
            }
            RenderCommand::PopClip => {
                idx += 1;
            }
        }
    }
}

fn find_matching_pop_clip(slice: &[RenderCommand]) -> Option<usize> {
    let mut depth = 0usize;
    for (i, cmd) in slice.iter().enumerate() {
        match cmd {
            RenderCommand::PushClip { .. } => depth += 1,
            RenderCommand::PopClip => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}
