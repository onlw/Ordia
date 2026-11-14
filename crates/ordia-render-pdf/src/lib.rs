//! PDF rendering backend translating [`ordia_render::DisplayList`] commands into PDF documents
//! via `krilla`, with automatic CJK font resolution and OpenType/TrueType glyph subsetting.

mod sheet_export;

pub use sheet_export::{PdfExportOptions, build_sheet_range_display_list};

use std::collections::HashMap;
use std::io::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use krilla::Document;
use krilla::color::rgb;
use krilla::geom::{Path as KrillaPath, PathBuilder, Point as KrillaPoint};
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{Fill, FillRule, Stroke, StrokeDash};
use krilla::text::{Font as KrillaFont, TextDirection};
use ordia_render::{Color, DisplayList, FontWeight, RectF, RenderCommand, StrokeStyle, TextHAlign};
use ordia_sheet_model::{CellRange, EngineError, SheetDocument};
use ordia_text::{FontCatalog, TextError, contains_cjk};
use serde::Serialize;
use thiserror::Error;

static PDF_TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Errors emitted while rendering [`DisplayList`] pages to PDF.
#[derive(Debug, Error)]
pub enum PdfRenderError {
    /// Unbalanced `PushClip` / `PopClip` commands in a `DisplayList`.
    #[error("DisplayList has unbalanced PushClip / PopClip commands")]
    UnbalancedClipStack,
    /// Invalid non-positive page dimensions.
    #[error("invalid PDF page dimensions ({width} x {height})")]
    InvalidPageDimensions {
        /// Page width in points.
        width: f32,
        /// Page height in points.
        height: f32,
    },
    /// Font resolution failure.
    #[error(transparent)]
    Text(#[from] TextError),
    /// `krilla` failed to construct a `Font` from the resolved face bytes.
    #[error("failed to parse OpenType/TrueType face '{0}' for PDF subsetting")]
    InvalidFontFace(String),
    /// `krilla` PDF serialization failure.
    #[error("PDF serialization failed: {0:?}")]
    KrillaSerialize(String),
    /// Spreadsheet model read error when exporting a sheet range.
    #[error(transparent)]
    Engine(#[from] EngineError),
    /// Filesystem I/O failure during atomic PDF export.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Deterministic structural summary of a generated PDF document (suitable for snapshot tests
/// independent of host OS font file paths).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PdfStructureSummary {
    /// PDF header line (e.g. `%PDF-1.7`).
    pub header_version: String,
    /// Whether the file ends with a valid `%%EOF` trailer marker.
    pub has_eof_marker: bool,
    /// Number of `/Type /Page` leaf objects in the PDF.
    pub page_count: usize,
    /// Number of embedded font descriptor objects (`/Type /FontDescriptor`).
    pub font_descriptor_count: usize,
    /// Whether at least one `/ToUnicode` CMap stream is present (required for selectable/searchable text).
    pub has_to_unicode_cmap: bool,
    /// Whether CID-keyed composite font subsetting (`/Subtype /CIDFontType0` or `/CIDFontType2`) was emitted.
    pub has_cid_font_subset: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PdfFontKey {
    family_name: String,
    face_index: u32,
}

/// Renders one or more [`DisplayList`] pages into an in-memory PDF byte vector with subsetted fonts.
pub fn render_display_lists_to_pdf(
    pages: &[DisplayList],
    fonts: &FontCatalog,
) -> Result<Vec<u8>, PdfRenderError> {
    let mut doc = Document::new();
    let mut font_cache: HashMap<PdfFontKey, KrillaFont> = HashMap::new();

    for page_dl in pages {
        if !page_dl.has_balanced_clips() {
            return Err(PdfRenderError::UnbalancedClipStack);
        }
        let settings = PageSettings::from_wh(page_dl.width, page_dl.height).ok_or(
            PdfRenderError::InvalidPageDimensions {
                width: page_dl.width,
                height: page_dl.height,
            },
        )?;
        let mut page = doc.start_page_with(settings);
        let mut surface = page.surface();

        for cmd in &page_dl.commands {
            match cmd {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius: _,
                } => {
                    if let Some(path) = build_rect_path(*rect) {
                        surface.set_fill(Some(to_krilla_fill(*color)));
                        surface.set_stroke(None);
                        surface.draw_path(&path);
                    }
                }
                RenderCommand::StrokeRect { rect, stroke } => {
                    if let Some(path) = build_rect_path(*rect) {
                        surface.set_fill(None);
                        surface.set_stroke(Some(to_krilla_stroke(*stroke)));
                        surface.draw_path(&path);
                    }
                }
                RenderCommand::DrawLine { start, end, stroke } => {
                    let mut pb = PathBuilder::new();
                    pb.move_to(start.x, start.y);
                    pb.line_to(end.x, end.y);
                    if let Some(path) = pb.finish() {
                        surface.set_fill(None);
                        surface.set_stroke(Some(to_krilla_stroke(*stroke)));
                        surface.draw_path(&path);
                    }
                }
                RenderCommand::DrawText {
                    origin,
                    text,
                    style,
                } => {
                    if text.is_empty() {
                        continue;
                    }
                    let bold = matches!(style.weight, FontWeight::Bold | FontWeight::Medium);
                    let need_cjk = contains_cjk(text);
                    let resolved = fonts.resolve_font(
                        style.font_family.as_deref(),
                        bold,
                        style.italic,
                        need_cjk,
                    )?;
                    let key = PdfFontKey {
                        family_name: resolved.family_name.clone(),
                        face_index: resolved.face_index,
                    };
                    let k_font = if let Some(existing) = font_cache.get(&key) {
                        existing.clone()
                    } else {
                        let parsed = KrillaFont::new(resolved.bytes.into(), resolved.face_index)
                            .ok_or(PdfRenderError::InvalidFontFace(resolved.family_name))?;
                        font_cache.insert(key, parsed.clone());
                        parsed
                    };

                    let est_w: f32 = text
                        .chars()
                        .map(|ch| {
                            if ch.is_ascii() {
                                style.font_size * 0.54
                            } else {
                                style.font_size
                            }
                        })
                        .sum();
                    let draw_x = match (style.h_align, style.max_width) {
                        (TextHAlign::Right, Some(mw)) => (origin.x + mw - est_w).max(origin.x),
                        (TextHAlign::Center, Some(mw)) => {
                            (origin.x + (mw - est_w) * 0.5).max(origin.x)
                        }
                        _ => origin.x,
                    };

                    surface.set_fill(Some(to_krilla_fill(style.color)));
                    surface.set_stroke(None);
                    surface.draw_text(
                        KrillaPoint::from_xy(draw_x, origin.y),
                        k_font,
                        style.font_size,
                        text,
                        false,
                        TextDirection::LeftToRight,
                    );
                    if style.underline {
                        let uy = origin.y + style.font_size * 0.12;
                        let mut pb = PathBuilder::new();
                        pb.move_to(draw_x, uy);
                        pb.line_to(draw_x + est_w, uy);
                        if let Some(p) = pb.finish() {
                            surface.set_fill(None);
                            surface.set_stroke(Some(to_krilla_stroke(StrokeStyle::solid(
                                (style.font_size * 0.07).max(0.6),
                                style.color,
                            ))));
                            surface.draw_path(&p);
                        }
                    }
                    if style.strikethrough {
                        let sy = origin.y - style.font_size * 0.28;
                        let mut pb = PathBuilder::new();
                        pb.move_to(draw_x, sy);
                        pb.line_to(draw_x + est_w, sy);
                        if let Some(p) = pb.finish() {
                            surface.set_fill(None);
                            surface.set_stroke(Some(to_krilla_stroke(StrokeStyle::solid(
                                (style.font_size * 0.07).max(0.6),
                                style.color,
                            ))));
                            surface.draw_path(&p);
                        }
                    }
                }
                RenderCommand::PushClip { rect } => {
                    let clip_rect =
                        RectF::new(rect.x, rect.y, rect.width.max(0.01), rect.height.max(0.01));
                    if let Some(path) = build_rect_path(clip_rect) {
                        surface.push_clip_path(&path, &FillRule::NonZero);
                    }
                }
                RenderCommand::PopClip => {
                    surface.pop();
                }
            }
        }

        surface.finish();
        page.finish();
    }

    doc.finish()
        .map_err(|err| PdfRenderError::KrillaSerialize(format!("{err:?}")))
}

/// Exports `range` from the active sheet of `doc` into an in-memory PDF byte vector.
pub fn export_sheet_range_to_pdf_bytes(
    doc: &SheetDocument,
    range: CellRange,
    options: &PdfExportOptions,
    fonts: &FontCatalog,
) -> Result<Vec<u8>, PdfRenderError> {
    let dl = build_sheet_range_display_list(doc, range, options)?;
    render_display_lists_to_pdf(&[dl], fonts)
}

/// Atomically writes `range` of `doc` as a PDF file to `path` (temp sibling + `sync_all` + `rename`).
pub fn export_sheet_range_to_pdf_file(
    doc: &SheetDocument,
    range: CellRange,
    options: &PdfExportOptions,
    fonts: &FontCatalog,
    path: &Path,
) -> Result<(), PdfRenderError> {
    let bytes = export_sheet_range_to_pdf_bytes(doc, range, options, fonts)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let stem = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "export.pdf".to_string());
    let pid = std::process::id();
    let seq = PDF_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = parent.join(format!(".{stem}.{pid}.{seq}.tmp"));

    let write_res = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        Ok(())
    })();
    if let Err(e) = write_res {
        let _ = std::fs::remove_file(&tmp);
        return Err(PdfRenderError::Io(e));
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(PdfRenderError::Io(e));
    }
    Ok(())
}

/// Inspects raw PDF bytes and extracts deterministic structural properties for verification.
pub fn inspect_pdf_structure(pdf_bytes: &[u8]) -> PdfStructureSummary {
    let lossy = String::from_utf8_lossy(pdf_bytes);
    let header_version = lossy.lines().next().unwrap_or_default().trim().to_string();
    let tail_window = &pdf_bytes[pdf_bytes.len().saturating_sub(64)..];
    let has_eof_marker = String::from_utf8_lossy(tail_window).contains("%%EOF");

    // Count `/Type/Page` or `/Type /Page` (excluding `/Pages` and `/PageLabels`)
    let mut page_count = 0usize;
    for pat in ["/Type/Page", "/Type /Page"] {
        for (idx, _) in lossy.match_indices(pat) {
            let after = &lossy[idx + pat.len()..];
            if !after.starts_with(|c: char| c.is_ascii_alphabetic()) {
                page_count += 1;
            }
        }
    }

    let font_descriptor_count = lossy.matches("/Type/FontDescriptor").count()
        + lossy.matches("/Type /FontDescriptor").count();
    let has_to_unicode_cmap = lossy.contains("/ToUnicode");
    let has_cid_font_subset = lossy.contains("/Subtype/CIDFontType0")
        || lossy.contains("/Subtype /CIDFontType0")
        || lossy.contains("/Subtype/CIDFontType2")
        || lossy.contains("/Subtype /CIDFontType2");

    PdfStructureSummary {
        header_version,
        has_eof_marker,
        page_count,
        font_descriptor_count,
        has_to_unicode_cmap,
        has_cid_font_subset,
    }
}

fn build_rect_path(rect: RectF) -> Option<KrillaPath> {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    let mut pb = PathBuilder::new();
    pb.move_to(rect.x, rect.y);
    pb.line_to(rect.right(), rect.y);
    pb.line_to(rect.right(), rect.bottom());
    pb.line_to(rect.x, rect.bottom());
    pb.close();
    pb.finish()
}

fn to_krilla_fill(color: Color) -> Fill {
    let alpha = (f32::from(color.a) / 255.0).clamp(0.0, 1.0);
    Fill {
        paint: rgb::Color::new(color.r, color.g, color.b).into(),
        opacity: NormalizedF32::new(alpha).unwrap_or(NormalizedF32::ONE),
        rule: FillRule::NonZero,
    }
}

fn to_krilla_stroke(stroke: StrokeStyle) -> Stroke {
    let alpha = (f32::from(stroke.color.a) / 255.0).clamp(0.0, 1.0);
    let dash = stroke.dash.map(|pat| StrokeDash {
        array: pat.as_slice().to_vec(),
        offset: 0.0,
    });
    Stroke {
        paint: rgb::Color::new(stroke.color.r, stroke.color.g, stroke.color.b).into(),
        width: stroke.width.max(0.25),
        opacity: NormalizedF32::new(alpha).unwrap_or(NormalizedF32::ONE),
        dash,
        ..Stroke::default()
    }
}
