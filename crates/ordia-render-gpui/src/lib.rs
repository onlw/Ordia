//! GPUI rendering backend translating platform-independent [`ordia_render::DisplayList`] commands
//! into GPU quads, paths, content masks, and shaped text runs.

use gpui_kit::{
    App, BorderStyle, Bounds, ContentMask, Font, FontStyle as GpuiFontStyle,
    FontWeight as GpuiFontWeight, Hsla, PathBuilder, Pixels, Point as GpuiPoint, SharedString,
    TextAlign, TextRun, Window, fill, outline, point, px, rgba, size,
};
use ordia_render::{Color, DisplayList, FontWeight, RectF, RenderCommand};

fn to_hsla(c: Color) -> Hsla {
    Hsla::from(rgba(u32::from_be_bytes([c.r, c.g, c.b, c.a])))
}

fn to_bounds(origin: GpuiPoint<Pixels>, r: RectF) -> Bounds<Pixels> {
    Bounds::new(
        point(origin.x + px(r.x), origin.y + px(r.y)),
        size(px(r.width.max(0.0)), px(r.height.max(0.0))),
    )
}

/// Paints `list` onto `window` anchored at `origin`.
pub fn paint_display_list(
    list: &DisplayList,
    origin: GpuiPoint<Pixels>,
    default_font_family: &SharedString,
    window: &mut Window,
    cx: &mut App,
) {
    paint_slice(&list.commands, origin, default_font_family, window, cx);
}

fn paint_slice(
    commands: &[RenderCommand],
    origin: GpuiPoint<Pixels>,
    default_font_family: &SharedString,
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
                let mut quad = fill(to_bounds(origin, *rect), to_hsla(*color));
                if *corner_radius > 0.0 {
                    quad.corner_radii = px(*corner_radius).into();
                }
                window.paint_quad(quad);
                idx += 1;
            }
            RenderCommand::StrokeRect { rect, stroke } => {
                let mut quad = outline(
                    to_bounds(origin, *rect),
                    to_hsla(stroke.color),
                    if stroke.dash.is_some() {
                        BorderStyle::Dashed
                    } else {
                        BorderStyle::Solid
                    },
                );
                quad.border_widths = px(stroke.width.max(0.5)).into();
                window.paint_quad(quad);
                idx += 1;
            }
            RenderCommand::DrawLine { start, end, stroke } => {
                let mut pb = PathBuilder::stroke(px(stroke.width.max(0.5)));
                if let Some([dash_len, gap_len]) = stroke.dash {
                    pb = pb.dash_array(&[px(dash_len), px(gap_len)]);
                }
                pb.move_to(point(origin.x + px(start.x), origin.y + px(start.y)));
                pb.line_to(point(origin.x + px(end.x), origin.y + px(end.y)));
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
                let gpui_weight = match style.weight {
                    FontWeight::Regular => GpuiFontWeight::NORMAL,
                    FontWeight::Medium => GpuiFontWeight::MEDIUM,
                    FontWeight::Bold => GpuiFontWeight::BOLD,
                };
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
                    color: to_hsla(style.color),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let shaped = window.text_system().shape_line(
                    SharedString::from(text.clone()),
                    px(style.font_size),
                    &[run],
                    None,
                );
                let top_y = text_origin.y - (style.font_size * 0.82);
                let _ = shaped.paint(
                    point(origin.x + px(text_origin.x), origin.y + px(top_y)),
                    px(style.font_size * 1.2),
                    TextAlign::Left,
                    None,
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
                    bounds: to_bounds(origin, *rect),
                };
                window.with_content_mask(Some(mask), |window| {
                    paint_slice(sub, origin, default_font_family, window, cx);
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
