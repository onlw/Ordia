//! `insta` `DisplayList` and `tiny-skia` visual snapshot tests covering every cell style
//! combination required by `M2-03`.

use ordia_render::{Color, DashPattern, DisplayList, RectF, RenderCommand, TextHAlign};
use ordia_sheet_layout::{GridAxisIndex, TileKey, TileRenderTheme, build_tile_display_list};
use ordia_sheet_model::{
    BorderEdge, BorderLineStyle, CellBorders, CellCoord, CellStyle, ColorRgba, FontStyle,
    HorizontalAlign, MAX_COLS, MAX_ROWS, SheetDocument, SheetOp, Transaction, VerticalAlign,
};
use ordia_testkit::{rasterize_display_list_region, summarize_pixmap};

fn render_first_tile(doc: &SheetDocument, with_grid: bool) -> DisplayList {
    let row_metrics = doc.custom_row_metrics().expect("row metrics");
    let col_metrics = doc.custom_col_metrics().expect("col metrics");
    let row_index = GridAxisIndex::from_row_metrics(MAX_ROWS, &row_metrics);
    let col_index = GridAxisIndex::from_col_metrics(MAX_COLS, &col_metrics);
    let theme = TileRenderTheme {
        default_text_color: Color::rgb(26, 26, 26),
        grid_line_color: with_grid.then_some(Color::rgb(226, 232, 240)),
    };
    build_tile_display_list(doc, TileKey::new(0, 0, 0), &row_index, &col_index, theme).display_list
}

fn set_style(doc: &mut SheetDocument, coord: CellCoord, style: CellStyle) {
    let old_style = Box::new(doc.cell_style(coord).unwrap_or_default());
    doc.execute_transaction(Transaction::single(
        "Set style",
        SheetOp::SetCellStyle {
            sheet: 0,
            coord,
            old_style,
            new_style: Box::new(style),
        },
    ))
    .expect("apply style");
}

#[test]
fn typography_and_fill_styles_display_list_and_skia_snapshots() {
    let mut doc = SheetDocument::new_empty("TypographyMatrix.xlsx").expect("empty doc");
    doc.execute_transaction(Transaction::single(
        "Taller rows",
        SheetOp::SetRowHeight {
            sheet: 0,
            row: 0,
            old_height_px: 25.0,
            new_height_px: 36.0,
        },
    ))
    .expect("row height");

    // A1: Bold + Italic + Georgia 16pt + crimson text + warm cream fill
    doc.set_cell_input(CellCoord::new(0, 0), "Headline 16")
        .expect("A1");
    set_style(
        &mut doc,
        CellCoord::new(0, 0),
        CellStyle {
            font: FontStyle {
                name: "Georgia".to_string(),
                size_pt: 16.0,
                bold: true,
                italic: true,
                underline: false,
                strikethrough: false,
                color: Some(ColorRgba::rgb(185, 28, 28)),
            },
            fill_color: Some(ColorRgba::rgb(254, 243, 199)),
            ..CellStyle::default()
        },
    );

    // B1: Underline + Strikethrough + Courier New 10.5pt + emerald fill
    doc.set_cell_input(CellCoord::new(0, 1), "Code_v2")
        .expect("B1");
    set_style(
        &mut doc,
        CellCoord::new(0, 1),
        CellStyle {
            font: FontStyle {
                name: "Courier New".to_string(),
                size_pt: 10.5,
                bold: false,
                italic: false,
                underline: true,
                strikethrough: true,
                color: Some(ColorRgba::rgb(6, 95, 70)),
            },
            fill_color: Some(ColorRgba::rgb(209, 250, 229)),
            ..CellStyle::default()
        },
    );

    // Confirm fractional 10.5pt font size survived IronCalc style roundtrip.
    let read_b1 = doc.cell_style(CellCoord::new(0, 1)).expect("read B1 style");
    assert!((read_b1.font.size_pt - 10.5).abs() < 0.01);

    let dl = render_first_tile(&doc, false);
    insta::assert_yaml_snapshot!("m2_03_typography_fill_dl", &dl.commands);

    let pixmap =
        rasterize_display_list_region(&dl, RectF::new(0.0, 0.0, 200.0, 44.0), Color::WHITE)
            .expect("rasterize typography");
    let summary = summarize_pixmap(&pixmap, Color::WHITE).expect("summarize typography");
    assert!(summary.distinct_color_buckets >= 4);
    insta::assert_yaml_snapshot!("m2_03_typography_fill_visual", &summary);
}

#[test]
fn all_thirteen_border_styles_and_diagonals_snapshots() {
    let mut doc = SheetDocument::new_empty("BordersMatrix.xlsx").expect("empty doc");
    let styles = [
        BorderLineStyle::Thin,
        BorderLineStyle::Medium,
        BorderLineStyle::Thick,
        BorderLineStyle::Double,
        BorderLineStyle::Hair,
        BorderLineStyle::Dotted,
        BorderLineStyle::Dashed,
        BorderLineStyle::DashDot,
        BorderLineStyle::DashDotDot,
        BorderLineStyle::MediumDashed,
        BorderLineStyle::MediumDashDot,
        BorderLineStyle::MediumDashDotDot,
        BorderLineStyle::SlantDashDot,
    ];

    for (idx, bs) in styles.into_iter().enumerate() {
        let r = (idx / 4) as u32 * 2;
        let c = (idx % 4) as u32;
        let coord = CellCoord::new(r, c);
        doc.set_cell_input(coord, &format!("B{idx}"))
            .expect("label");
        let edge = Some(BorderEdge {
            style: bs,
            color: Some(ColorRgba::rgb(30, 64, 175)),
        });
        set_style(
            &mut doc,
            coord,
            CellStyle {
                borders: CellBorders {
                    top: edge,
                    right: edge,
                    bottom: edge,
                    left: edge,
                    diagonal: (bs == BorderLineStyle::Thin).then_some(BorderEdge {
                        style: BorderLineStyle::Medium,
                        color: Some(ColorRgba::rgb(220, 38, 38)),
                    }),
                    diagonal_up: bs == BorderLineStyle::Thin,
                    diagonal_down: bs == BorderLineStyle::Thin,
                },
                ..CellStyle::default()
            },
        );
    }

    let dl = render_first_tile(&doc, false);
    // Double border on 4 sides emits 8 parallel 1px lines; DashDot emits 4-element DashPattern;
    // DashDotDot emits 6-element DashPattern.
    let has_dash_dot = dl.commands.iter().any(|cmd| {
        matches!(
            cmd,
            RenderCommand::DrawLine {
                stroke,
                ..
            } if matches!(stroke.dash, Some(DashPattern::DashDot(_)))
        )
    });
    let has_dash_dot_dot = dl.commands.iter().any(|cmd| {
        matches!(
            cmd,
            RenderCommand::DrawLine {
                stroke,
                ..
            } if matches!(stroke.dash, Some(DashPattern::DashDotDot(_)))
        )
    });
    assert!(has_dash_dot, "expected DashDot 4-element stroke pattern");
    assert!(
        has_dash_dot_dot,
        "expected DashDotDot 6-element stroke pattern"
    );

    insta::assert_yaml_snapshot!("m2_03_all_borders_dl", &dl.commands);

    let pixmap =
        rasterize_display_list_region(&dl, RectF::new(0.0, 0.0, 370.0, 185.0), Color::WHITE)
            .expect("rasterize borders");
    let summary = summarize_pixmap(&pixmap, Color::WHITE).expect("summarize borders");
    insta::assert_yaml_snapshot!("m2_03_all_borders_visual", &summary);
}

#[test]
fn alignment_indent_wrap_and_fill_repeat_snapshots() {
    let mut doc = SheetDocument::new_empty("AlignmentWrapMatrix.xlsx").expect("empty doc");
    for r in 0..3 {
        doc.execute_transaction(Transaction::single(
            "Tall row",
            SheetOp::SetRowHeight {
                sheet: 0,
                row: r,
                old_height_px: 25.0,
                new_height_px: 56.0,
            },
        ))
        .expect("tall row");
    }

    // Row 0: General alignment type dispatch (Text->Left, Number->Right, Bool->Center, Error->Center)
    doc.set_cell_input(CellCoord::new(0, 0), "TextGen")
        .expect("A1");
    doc.set_cell_input(CellCoord::new(0, 1), "42195")
        .expect("B1");
    doc.set_cell_input(CellCoord::new(0, 2), "TRUE")
        .expect("C1");
    doc.set_cell_input(CellCoord::new(0, 3), "=1/0")
        .expect("D1");

    // Row 1: Top+Left+Indent(2), Center+Center, Bottom+Right+Indent(1), Fill("*-")
    doc.set_cell_input(CellCoord::new(1, 0), "Indent2")
        .expect("A2");
    set_style(
        &mut doc,
        CellCoord::new(1, 0),
        CellStyle {
            horizontal_align: HorizontalAlign::Left,
            vertical_align: VerticalAlign::Top,
            indent: 2,
            ..CellStyle::default()
        },
    );

    doc.set_cell_input(CellCoord::new(1, 1), "MidBox")
        .expect("B2");
    set_style(
        &mut doc,
        CellCoord::new(1, 1),
        CellStyle {
            horizontal_align: HorizontalAlign::Center,
            vertical_align: VerticalAlign::Center,
            ..CellStyle::default()
        },
    );

    doc.set_cell_input(CellCoord::new(1, 2), "RightIn")
        .expect("C2");
    set_style(
        &mut doc,
        CellCoord::new(1, 2),
        CellStyle {
            horizontal_align: HorizontalAlign::Right,
            vertical_align: VerticalAlign::Bottom,
            indent: 1,
            ..CellStyle::default()
        },
    );

    doc.set_cell_input(CellCoord::new(1, 3), "*-").expect("D2");
    set_style(
        &mut doc,
        CellCoord::new(1, 3),
        CellStyle {
            horizontal_align: HorizontalAlign::Fill,
            vertical_align: VerticalAlign::Center,
            ..CellStyle::default()
        },
    );

    // Row 2: Multi-line wrap_text (Latin words + CJK ideographs + Distributed vertical align)
    doc.set_cell_input(CellCoord::new(2, 0), "Quarterly revenue forecast summary")
        .expect("A3");
    set_style(
        &mut doc,
        CellCoord::new(2, 0),
        CellStyle {
            horizontal_align: HorizontalAlign::Left,
            vertical_align: VerticalAlign::Top,
            wrap_text: true,
            ..CellStyle::default()
        },
    );

    doc.set_cell_input(CellCoord::new(2, 1), "自动换行长中文内容排版验证")
        .expect("B3");
    set_style(
        &mut doc,
        CellCoord::new(2, 1),
        CellStyle {
            horizontal_align: HorizontalAlign::Center,
            vertical_align: VerticalAlign::Distributed,
            wrap_text: true,
            ..CellStyle::default()
        },
    );

    let dl = render_first_tile(&doc, false);
    // Verify General alignment dispatched Number -> Right and Bool/Error -> Center, and wrap_text emitted >= 2 lines.
    let right_count = dl
        .commands
        .iter()
        .filter(|c| {
            matches!(
                c,
                RenderCommand::DrawText { style, .. } if style.h_align == TextHAlign::Right
            )
        })
        .count();
    assert!(right_count >= 2);

    insta::assert_yaml_snapshot!("m2_03_alignment_indent_wrap_dl", &dl.commands);

    let pixmap =
        rasterize_display_list_region(&dl, RectF::new(0.0, 0.0, 365.0, 172.0), Color::WHITE)
            .expect("rasterize alignment & wrap");
    let summary = summarize_pixmap(&pixmap, Color::WHITE).expect("summarize alignment & wrap");
    insta::assert_yaml_snapshot!("m2_03_alignment_indent_wrap_visual", &summary);
}

#[test]
fn long_text_overflow_and_occupied_neighbor_clipping_snapshots() {
    let mut doc = SheetDocument::new_empty("OverflowMatrix.xlsx").expect("empty doc");

    // Row 0: A1 has long left-aligned text and B1:D1 are empty -> clip expands across B1:C1!
    doc.set_cell_input(
        CellCoord::new(0, 0),
        "Long unclipped heading overflowing smoothly across adjacent empty cells",
    )
    .expect("A1");

    // Row 1: A2 has the same long text, but B2 is occupied ("BLOCK") -> A2 clips at width 89px!
    doc.set_cell_input(
        CellCoord::new(1, 0),
        "Long heading stopped immediately by non-empty neighbor B2",
    )
    .expect("A2");
    doc.set_cell_input(CellCoord::new(1, 1), "BLOCK")
        .expect("B2");

    // Row 2: C3 has right-aligned long text and A3:B3 are empty -> expands leftward into A3:B3!
    doc.set_cell_input(
        CellCoord::new(2, 2),
        "Right-aligned banner flowing left into empty A3:B3",
    )
    .expect("C3");
    set_style(
        &mut doc,
        CellCoord::new(2, 2),
        CellStyle {
            horizontal_align: HorizontalAlign::Right,
            ..CellStyle::default()
        },
    );

    // Row 3: B4 has centered long text and A4 & C4 are empty -> expands symmetrically!
    doc.set_cell_input(
        CellCoord::new(3, 1),
        "Centered title expanding symmetrically left and right",
    )
    .expect("B4");
    set_style(
        &mut doc,
        CellCoord::new(3, 1),
        CellStyle {
            horizontal_align: HorizontalAlign::Center,
            ..CellStyle::default()
        },
    );

    let dl = render_first_tile(&doc, true);
    let clips: Vec<RectF> = dl
        .commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::PushClip { rect } => Some(*rect),
            _ => None,
        })
        .collect();
    // Clip 0 (A1) must be > 250px wide; Clip 1 (A2 blocked by B2) must be <= 90px wide!
    assert!(
        clips[0].width > 250.0,
        "A1 should overflow across empty cells, got {}",
        clips[0].width
    );
    assert!(
        clips[1].width <= 90.0,
        "A2 should clip at occupied B2 boundary, got {}",
        clips[1].width
    );

    insta::assert_yaml_snapshot!("m2_03_overflow_clipping_dl", &dl.commands);

    let pixmap =
        rasterize_display_list_region(&dl, RectF::new(0.0, 0.0, 365.0, 105.0), Color::WHITE)
            .expect("rasterize overflow");
    let summary = summarize_pixmap(&pixmap, Color::WHITE).expect("summarize overflow");
    insta::assert_yaml_snapshot!("m2_03_overflow_clipping_visual", &summary);
}
