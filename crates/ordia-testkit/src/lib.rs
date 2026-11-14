//! Test harness utilities for deterministic `tiny-skia` software rasterization, visual snapshot
//! summaries, corpus round-trip validation, and formula recalculation diffing.

use std::collections::HashSet;

use ordia_render::{
    Color, DisplayList, FontWeight, Point, RectF, RenderCommand, StrokeStyle, TextHAlign,
    TextRunStyle,
};
use ordia_text::is_cjk_char;
use serde::Serialize;
use thiserror::Error;
use tiny_skia::{
    Color as SkiaColor, FillRule, LineCap, LineJoin, Mask, Paint, Path, PathBuilder, Pixmap,
    Stroke, StrokeDash, Transform,
};

/// Errors emitted while rasterizing a [`DisplayList`] with `tiny-skia`.
#[derive(Debug, Error)]
pub enum RasterError {
    /// Non-positive or excessive surface dimensions.
    #[error("invalid raster surface dimensions ({width} x {height})")]
    InvalidDimensions {
        /// Requested width in pixels.
        width: u32,
        /// Requested height in pixels.
        height: u32,
    },
    /// Unbalanced `PushClip` / `PopClip` commands.
    #[error("DisplayList has unbalanced PushClip / PopClip commands")]
    UnbalancedClips,
    /// PNG encoding failed.
    #[error("PNG encoding failed: {0}")]
    PngEncode(String),
}

/// Deterministic, human-inspectable summary of a rasterized `tiny-skia` [`Pixmap`] for `insta`
/// visual regression snapshots (`M2-03`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct VisualSnapshotSummary {
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// Deterministic CRC32 over raw premultiplied RGBA pixels (`8` uppercase hex chars).
    pub rgba_crc32_hex: String,
    /// Byte length of the encoded PNG image.
    pub png_byte_len: usize,
    /// Number of pixels differing from the background color.
    pub ink_pixel_count: usize,
    /// Inclusive `[min_x, min_y, max_x, max_y]` bounding box of non-background pixels.
    pub ink_bbox: Option<[u32; 4]>,
    /// Number of distinct quantized `(r>>4, g>>4, b>>4)` buckets present.
    pub distinct_color_buckets: usize,
    /// Downsampled ASCII luminance/ink preview grid for human diff inspection.
    pub ascii_preview: Vec<String>,
}

/// Rasterizes the full surface of `dl` onto a `tiny-skia` [`Pixmap`].
pub fn rasterize_display_list(dl: &DisplayList, background: Color) -> Result<Pixmap, RasterError> {
    let region = RectF::new(0.0, 0.0, dl.width, dl.height);
    rasterize_display_list_region(dl, region, background)
}

/// Rasterizes a sub-rectangle `region` of `dl` (translated so `(region.x, region.y)` maps to `(0, 0)`).
pub fn rasterize_display_list_region(
    dl: &DisplayList,
    region: RectF,
    background: Color,
) -> Result<Pixmap, RasterError> {
    if !dl.has_balanced_clips() {
        return Err(RasterError::UnbalancedClips);
    }
    let w = region.width.ceil().max(0.0) as u32;
    let h = region.height.ceil().max(0.0) as u32;
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        return Err(RasterError::InvalidDimensions {
            width: w,
            height: h,
        });
    }
    let mut pixmap = Pixmap::new(w, h).ok_or(RasterError::InvalidDimensions {
        width: w,
        height: h,
    })?;
    pixmap.fill(to_skia_color(background));

    let dx = -region.x;
    let dy = -region.y;
    let surface_bounds = RectF::new(0.0, 0.0, w as f32, h as f32);
    let mut clip_stack: Vec<RectF> = Vec::new();
    let mut active_mask: Option<Mask> = None;

    for cmd in &dl.commands {
        match cmd {
            RenderCommand::PushClip { rect } => {
                let shifted = RectF::new(rect.x + dx, rect.y + dy, rect.width, rect.height);
                let combined = clip_stack.last().copied().map_or_else(
                    || intersect_rect(surface_bounds, shifted),
                    |top| intersect_rect(top, shifted),
                );
                clip_stack.push(combined);
                active_mask = build_clip_mask(w, h, combined);
            }
            RenderCommand::PopClip => {
                let _ = clip_stack.pop();
                active_mask = clip_stack
                    .last()
                    .copied()
                    .and_then(|r| build_clip_mask(w, h, r));
            }
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } => {
                if clip_stack.last().is_some() && active_mask.is_none() {
                    continue;
                }
                let shifted = RectF::new(rect.x + dx, rect.y + dy, rect.width, rect.height);
                if let Some(path) = build_rect_or_round_path(shifted, *corner_radius) {
                    let paint = to_paint(*color);
                    pixmap.fill_path(
                        &path,
                        &paint,
                        FillRule::Winding,
                        Transform::identity(),
                        active_mask.as_ref(),
                    );
                }
            }
            RenderCommand::StrokeRect { rect, stroke } => {
                if clip_stack.last().is_some() && active_mask.is_none() {
                    continue;
                }
                let shifted = RectF::new(rect.x + dx, rect.y + dy, rect.width, rect.height);
                if let Some(path) = build_rect_or_round_path(shifted, 0.0) {
                    paint_stroked_path(&mut pixmap, &path, *stroke, active_mask.as_ref());
                }
            }
            RenderCommand::DrawLine { start, end, stroke } => {
                if clip_stack.last().is_some() && active_mask.is_none() {
                    continue;
                }
                let mut pb = PathBuilder::new();
                pb.move_to(start.x + dx, start.y + dy);
                pb.line_to(end.x + dx, end.y + dy);
                if let Some(path) = pb.finish() {
                    paint_stroked_path(&mut pixmap, &path, *stroke, active_mask.as_ref());
                }
            }
            RenderCommand::DrawText {
                origin,
                text,
                style,
            } => {
                if text.is_empty() || (clip_stack.last().is_some() && active_mask.is_none()) {
                    continue;
                }
                rasterize_deterministic_text(
                    &mut pixmap,
                    Point::new(origin.x + dx, origin.y + dy),
                    text,
                    style,
                    active_mask.as_ref(),
                );
            }
        }
    }

    Ok(pixmap)
}

/// Encodes `pixmap` as PNG bytes.
pub fn encode_pixmap_png(pixmap: &Pixmap) -> Result<Vec<u8>, RasterError> {
    pixmap
        .encode_png()
        .map_err(|e| RasterError::PngEncode(e.to_string()))
}

/// Computes a deterministic [`VisualSnapshotSummary`] (CRC32, PNG verification, bounding box,
/// color bucket count, and ASCII preview grid) from `pixmap`.
pub fn summarize_pixmap(
    pixmap: &Pixmap,
    background: Color,
) -> Result<VisualSnapshotSummary, RasterError> {
    let png = encode_pixmap_png(pixmap)?;
    let w = pixmap.width();
    let h = pixmap.height();
    let raw = pixmap.data();
    let crc = crc32fast::hash(raw);

    let bg_px = to_skia_color(background).premultiply().to_color_u8();
    let mut ink_count = 0usize;
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut buckets: HashSet<(u8, u8, u8)> = HashSet::new();

    for (idx, px_val) in pixmap.pixels().iter().enumerate() {
        buckets.insert((px_val.red() >> 4, px_val.green() >> 4, px_val.blue() >> 4));
        if *px_val != bg_px {
            ink_count += 1;
            let x = (idx as u32) % w;
            let y = (idx as u32) / w;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }

    let cols = 48u32.min(w);
    let rows = 18u32.min(h);
    let mut ascii_preview = Vec::with_capacity(rows as usize);
    for gy in 0..rows {
        let y0 = (gy * h) / rows;
        let y1 = (((gy + 1) * h) / rows).max(y0 + 1).min(h);
        let mut row_str = String::with_capacity(cols as usize);
        for gx in 0..cols {
            let x0 = (gx * w) / cols;
            let x1 = (((gx + 1) * w) / cols).max(x0 + 1).min(w);
            let mut max_diff = 0u32;
            for py in y0..y1 {
                for px_i in x0..x1 {
                    if let Some(p) = pixmap.pixel(px_i, py) {
                        let dr = u32::from(p.red().abs_diff(bg_px.red()));
                        let dg = u32::from(p.green().abs_diff(bg_px.green()));
                        let db = u32::from(p.blue().abs_diff(bg_px.blue()));
                        max_diff = max_diff.max(dr + dg + db);
                    }
                }
            }
            let ch = match max_diff {
                0..=8 => '.',
                9..=80 => ':',
                81..=220 => '+',
                221..=450 => '#',
                _ => '@',
            };
            row_str.push(ch);
        }
        ascii_preview.push(row_str);
    }

    Ok(VisualSnapshotSummary {
        width: w,
        height: h,
        rgba_crc32_hex: format!("{crc:08X}"),
        png_byte_len: png.len(),
        ink_pixel_count: ink_count,
        ink_bbox: (ink_count > 0).then_some([min_x, min_y, max_x, max_y]),
        distinct_color_buckets: buckets.len(),
        ascii_preview,
    })
}

fn to_skia_color(c: Color) -> SkiaColor {
    SkiaColor::from_rgba8(c.r, c.g, c.b, c.a)
}

fn to_paint(c: Color) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(to_skia_color(c));
    paint.anti_alias = true;
    paint
}

fn intersect_rect(a: RectF, b: RectF) -> RectF {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = a.right().min(b.right());
    let y1 = a.bottom().min(b.bottom());
    RectF::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
}

fn build_clip_mask(w: u32, h: u32, rect: RectF) -> Option<Mask> {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    let mut mask = Mask::new(w, h)?;
    let path = build_rect_or_round_path(rect, 0.0)?;
    mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
    Some(mask)
}

fn build_rect_or_round_path(rect: RectF, radius: f32) -> Option<Path> {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    let r = radius.clamp(0.0, rect.width.min(rect.height) * 0.5);
    let mut pb = PathBuilder::new();
    if r <= 0.05 {
        pb.move_to(rect.x, rect.y);
        pb.line_to(rect.right(), rect.y);
        pb.line_to(rect.right(), rect.bottom());
        pb.line_to(rect.x, rect.bottom());
        pb.close();
    } else {
        let x0 = rect.x;
        let y0 = rect.y;
        let x1 = rect.right();
        let y1 = rect.bottom();
        pb.move_to(x0 + r, y0);
        pb.line_to(x1 - r, y0);
        pb.quad_to(x1, y0, x1, y0 + r);
        pb.line_to(x1, y1 - r);
        pb.quad_to(x1, y1, x1 - r, y1);
        pb.line_to(x0 + r, y1);
        pb.quad_to(x0, y1, x0, y1 - r);
        pb.line_to(x0, y0 + r);
        pb.quad_to(x0, y0, x0 + r, y0);
        pb.close();
    }
    pb.finish()
}

fn paint_stroked_path(pixmap: &mut Pixmap, path: &Path, style: StrokeStyle, mask: Option<&Mask>) {
    let paint = to_paint(style.color);
    let stroke = Stroke {
        width: style.width.max(0.4),
        line_cap: LineCap::Butt,
        line_join: LineJoin::Miter,
        dash: style
            .dash
            .and_then(|pat| StrokeDash::new(pat.as_slice().to_vec(), 0.0)),
        ..Stroke::default()
    };
    pixmap.stroke_path(path, &paint, &stroke, Transform::identity(), mask);
}

fn rasterize_deterministic_text(
    pixmap: &mut Pixmap,
    origin: Point,
    text: &str,
    style: &TextRunStyle,
    mask: Option<&Mask>,
) {
    let fs = style.font_size.max(6.0);
    let bold = matches!(style.weight, FontWeight::Bold | FontWeight::Medium);
    let bold_scale = if bold { 1.06 } else { 1.0 };

    let total_w: f32 = text
        .chars()
        .map(|ch| {
            let em = if is_cjk_char(ch) {
                1.0
            } else if ch == ' ' {
                0.30
            } else {
                0.56
            };
            fs * em * bold_scale
        })
        .sum();

    let start_x = match (style.h_align, style.max_width) {
        (TextHAlign::Right, Some(mw)) => origin.x + mw - total_w,
        (TextHAlign::Center, Some(mw)) => origin.x + (mw - total_w) * 0.5,
        _ => origin.x,
    };

    let cap_h = fs * 0.68;
    let stroke_w = if bold {
        (fs * 0.145).max(1.4)
    } else {
        (fs * 0.085).max(0.95)
    };
    let slant = if style.italic { cap_h * 0.22 } else { 0.0 };

    let mut pb = PathBuilder::new();
    let mut pen_x = start_x;
    for ch in text.chars() {
        let adv = if is_cjk_char(ch) {
            fs * bold_scale
        } else if ch == ' ' {
            fs * 0.30 * bold_scale
        } else {
            fs * 0.56 * bold_scale
        };
        if ch != ' ' {
            let gx0 = pen_x + adv * 0.12;
            let gx1 = pen_x + adv * 0.88;
            let gy1 = origin.y;
            let gy0 = origin.y - cap_h;
            let mid_y = (gy0 + gy1) * 0.5;
            let mid_x = (gx0 + gx1) * 0.5;
            if is_cjk_char(ch) {
                // Crisp CJK ideograph box + central crossbars (distinguishes CJK glyphs deterministically).
                pb.move_to(gx0 + slant, gy0);
                pb.line_to(gx1 + slant, gy0);
                pb.line_to(gx1, gy1);
                pb.line_to(gx0, gy1);
                pb.close();
                pb.move_to(gx0 + slant * 0.5, mid_y);
                pb.line_to(gx1 + slant * 0.5, mid_y);
                pb.move_to(mid_x + slant, gy0);
                pb.line_to(mid_x, gy1);
            } else {
                // Deterministic character-code-modulated 7-segment vector glyph so different ASCII strings
                // produce distinct glyph shapes and pixel hashes across every OS!
                let bits = (ch as u32).wrapping_mul(0x9E37_79B9) ^ (ch as u32);
                if bits & 1 != 0 {
                    pb.move_to(gx0 + slant, gy0);
                    pb.line_to(gx1 + slant, gy0);
                }
                if bits & 2 != 0 {
                    pb.move_to(gx0 + slant * 0.5, mid_y);
                    pb.line_to(gx1 + slant * 0.5, mid_y);
                }
                if bits & 4 != 0 {
                    pb.move_to(gx0, gy1);
                    pb.line_to(gx1, gy1);
                }
                pb.move_to(gx0 + slant, gy0);
                pb.line_to(gx0, gy1);
                if bits & 8 != 0 {
                    pb.move_to(gx1 + slant, gy0);
                    pb.line_to(gx1, gy1);
                }
            }
        }
        pen_x += adv;
    }

    let paint = to_paint(style.color);
    if let Some(glyph_path) = pb.finish() {
        let stroke = Stroke {
            width: stroke_w,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default()
        };
        pixmap.stroke_path(&glyph_path, &paint, &stroke, Transform::identity(), mask);
    }

    let deco_w = (fs * 0.075).max(0.9);
    if style.underline && total_w > 0.5 {
        let uy = origin.y + fs * 0.14;
        let mut upb = PathBuilder::new();
        upb.move_to(start_x, uy);
        upb.line_to(start_x + total_w, uy);
        if let Some(p) = upb.finish() {
            paint_stroked_path(pixmap, &p, StrokeStyle::solid(deco_w, style.color), mask);
        }
    }
    if style.strikethrough && total_w > 0.5 {
        let sy = origin.y - cap_h * 0.45;
        let mut spb = PathBuilder::new();
        spb.move_to(start_x, sy);
        spb.line_to(start_x + total_w, sy);
        if let Some(p) = spb.finish() {
            paint_stroked_path(pixmap, &p, StrokeStyle::solid(deco_w, style.color), mask);
        }
    }
}
