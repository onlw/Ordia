//! Engine-independent spreadsheet domain types.
//!
//! All coordinates in Ordia's public API are **zero-based**: `(row: 0, col: 0)` corresponds to
//! Excel's `A1`, and `(row: 1_048_575, col: 16_383)` corresponds to `XFD1048576`.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Maximum number of rows supported in a worksheet (`1,048,576`, matching Excel OOXML).
pub const MAX_ROWS: u32 = 1_048_576;

/// Maximum number of columns supported in a worksheet (`16,384` / `XFD`, matching Excel OOXML).
pub const MAX_COLS: u32 = 16_384;

/// Default logical row height in pixels (`25.0`, corresponding to `16pt` in IronCalc/OOXML).
pub const DEFAULT_ROW_HEIGHT_PX: f32 = 25.0;

/// Default logical column width in pixels (`90.0`, corresponding to `10` char units).
pub const DEFAULT_COL_WIDTH_PX: f32 = 90.0;

/// Zero-based worksheet cell coordinate where `(0, 0)` is `A1`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub struct CellCoord {
    /// Zero-based row index (`0..MAX_ROWS`).
    pub row: u32,
    /// Zero-based column index (`0..MAX_COLS`).
    pub col: u32,
}

impl CellCoord {
    /// Constructs a zero-based cell coordinate without bounds checking.
    #[must_use]
    pub const fn new(row: u32, col: u32) -> Self {
        Self { row, col }
    }

    /// Returns `true` if `row < MAX_ROWS` and `col < MAX_COLS`.
    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.row < MAX_ROWS && self.col < MAX_COLS
    }

    /// Parses an `A1`-style cell reference (ignoring optional `$` absolute markers), e.g. `"B12"` or `"$AA$1"`.
    #[must_use]
    pub fn parse_a1(input: &str) -> Option<Self> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return None;
        }
        let bytes = trimmed.as_bytes();
        let mut idx = 0;
        if idx < bytes.len() && bytes[idx] == b'$' {
            idx += 1;
        }
        let col_start = idx;
        while idx < bytes.len() && bytes[idx].is_ascii_alphabetic() {
            idx += 1;
        }
        let col_letters = &trimmed[col_start..idx];
        if col_letters.is_empty() || col_letters.len() > 3 {
            return None;
        }
        let col_1based = letters_to_col_1based(col_letters)?;

        if idx < bytes.len() && bytes[idx] == b'$' {
            idx += 1;
        }
        let row_digits = &trimmed[idx..];
        if row_digits.is_empty() || row_digits.starts_with('0') {
            return None;
        }
        let row_1based = row_digits.parse::<u32>().ok()?;
        if row_1based == 0 || row_1based > MAX_ROWS || col_1based == 0 || col_1based > MAX_COLS {
            return None;
        }
        Some(Self {
            row: row_1based - 1,
            col: col_1based - 1,
        })
    }

    /// Formats this coordinate as an Excel `A1` reference (e.g. `(0, 0)` -> `"A1"`).
    #[must_use]
    pub fn to_a1(self) -> String {
        format!("{}{}", col_to_letters(self.col), self.row + 1)
    }
}

impl fmt::Display for CellCoord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", col_to_letters(self.col), self.row + 1)
    }
}

/// Converts a zero-based column index (`0` -> `"A"`, `25` -> `"Z"`, `26` -> `"AA"`) to Excel letters.
#[must_use]
pub fn col_to_letters(col: u32) -> String {
    let mut n = col + 1;
    let mut buf = [0u8; 8];
    let mut pos = buf.len();
    while n > 0 {
        let rem = ((n - 1) % 26) as u8;
        pos -= 1;
        buf[pos] = b'A' + rem;
        n = (n - 1) / 26;
    }
    String::from_utf8_lossy(&buf[pos..]).into_owned()
}

/// Converts Excel column letters (`"A"` -> `1`, `"Z"` -> `26`, `"AA"` -> `27`) to a 1-based column index.
#[must_use]
pub fn letters_to_col_1based(letters: &str) -> Option<u32> {
    if letters.is_empty() || letters.len() > 3 {
        return None;
    }
    let mut acc: u32 = 0;
    for b in letters.bytes() {
        let upper = b.to_ascii_uppercase();
        if !upper.is_ascii_uppercase() {
            return None;
        }
        acc = acc
            .checked_mul(26)?
            .checked_add(u32::from(upper - b'A' + 1))?;
    }
    if acc == 0 || acc > MAX_COLS {
        None
    } else {
        Some(acc)
    }
}

/// Normalized inclusive rectangular cell range (`start.row <= end.row && start.col <= end.col`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct CellRange {
    /// Top-left cell coordinate (inclusive).
    pub start: CellCoord,
    /// Bottom-right cell coordinate (inclusive).
    pub end: CellCoord,
}

impl CellRange {
    /// Creates a normalized inclusive range between two corner coordinates.
    #[must_use]
    pub fn new(a: CellCoord, b: CellCoord) -> Self {
        Self {
            start: CellCoord::new(a.row.min(b.row), a.col.min(b.col)),
            end: CellCoord::new(a.row.max(b.row), a.col.max(b.col)),
        }
    }

    /// Creates a 1x1 range containing a single cell.
    #[must_use]
    pub const fn single(coord: CellCoord) -> Self {
        Self {
            start: coord,
            end: coord,
        }
    }

    /// Parses an `A1` or `A1:C10` range string.
    #[must_use]
    pub fn parse_a1(input: &str) -> Option<Self> {
        if let Some((left, right)) = input.split_once(':') {
            Some(Self::new(
                CellCoord::parse_a1(left)?,
                CellCoord::parse_a1(right)?,
            ))
        } else {
            Some(Self::single(CellCoord::parse_a1(input)?))
        }
    }

    /// Formats this range in `A1` or `A1:B2` notation.
    #[must_use]
    pub fn to_a1(self) -> String {
        if self.start == self.end {
            self.start.to_a1()
        } else {
            format!("{}:{}", self.start.to_a1(), self.end.to_a1())
        }
    }

    /// Number of columns spanned (`>= 1`).
    #[must_use]
    pub const fn width(self) -> u32 {
        self.end.col - self.start.col + 1
    }

    /// Number of rows spanned (`>= 1`).
    #[must_use]
    pub const fn height(self) -> u32 {
        self.end.row - self.start.row + 1
    }

    /// Returns `true` if `coord` lies within this range.
    #[must_use]
    pub const fn contains(self, coord: CellCoord) -> bool {
        coord.row >= self.start.row
            && coord.row <= self.end.row
            && coord.col >= self.start.col
            && coord.col <= self.end.col
    }

    /// Returns `true` if `self` and `other` share at least one cell.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.start.row <= other.end.row
            && self.end.row >= other.start.row
            && self.start.col <= other.end.col
            && self.end.col >= other.start.col
    }
}

impl fmt::Display for CellRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_a1())
    }
}

/// Standard Excel formula evaluation error codes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FormulaError {
    /// `#DIV/0!`
    Div0,
    /// `#NAME?`
    Name,
    /// `#VALUE!`
    Value,
    /// `#REF!`
    Ref,
    /// `#N/A`
    Na,
    /// `#NUM!`
    Num,
    /// `#NULL!`
    Null,
    /// `#SPILL!`
    Spill,
    /// `#CALC!`
    Calc,
    /// `#GETTING_DATA`
    GettingData,
    /// `#CIRC!` (circular dependency detected)
    Circular,
    /// `#ERROR!` (syntax / general formula error)
    InvalidFormula,
    /// `#N/IMPL!` (function not implemented in engine)
    NotImplemented,
    /// Unrecognized error token preserved verbatim.
    Other(String),
}

impl FormulaError {
    /// Parses an Excel/IronCalc error display string into [`FormulaError`].
    #[must_use]
    pub fn from_display_str(s: &str) -> Option<Self> {
        match s {
            "#DIV/0!" => Some(Self::Div0),
            "#NAME?" => Some(Self::Name),
            "#VALUE!" => Some(Self::Value),
            "#REF!" => Some(Self::Ref),
            "#N/A" => Some(Self::Na),
            "#NUM!" => Some(Self::Num),
            "#NULL!" => Some(Self::Null),
            "#SPILL!" => Some(Self::Spill),
            "#CALC!" => Some(Self::Calc),
            "#GETTING_DATA" => Some(Self::GettingData),
            "#CIRC!" => Some(Self::Circular),
            "#ERROR!" => Some(Self::InvalidFormula),
            "#N/IMPL!" => Some(Self::NotImplemented),
            _ => None,
        }
    }

    /// Returns the canonical Excel error display token.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Div0 => "#DIV/0!",
            Self::Name => "#NAME?",
            Self::Value => "#VALUE!",
            Self::Ref => "#REF!",
            Self::Na => "#N/A",
            Self::Num => "#NUM!",
            Self::Null => "#NULL!",
            Self::Spill => "#SPILL!",
            Self::Calc => "#CALC!",
            Self::GettingData => "#GETTING_DATA",
            Self::Circular => "#CIRC!",
            Self::InvalidFormula => "#ERROR!",
            Self::NotImplemented => "#N/IMPL!",
            Self::Other(raw) => raw.as_str(),
        }
    }
}

impl fmt::Display for FormulaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Evaluated value of a spreadsheet cell.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub enum CellValue {
    /// Empty cell with no value.
    #[default]
    Empty,
    /// IEEE-754 floating-point number (including dates/times in Excel serial format).
    Number(f64),
    /// Plain string value.
    Text(String),
    /// Logical `TRUE` or `FALSE`.
    Boolean(bool),
    /// Formula or literal error value.
    Error(FormulaError),
}

/// 8-bit sRGB color with alpha used for document cell styles and tab colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ColorRgba {
    /// Red channel (`0..=255`).
    pub r: u8,
    /// Green channel (`0..=255`).
    pub g: u8,
    /// Blue channel (`0..=255`).
    pub b: u8,
    /// Alpha channel (`0..=255`, `255` = opaque).
    pub a: u8,
}

impl ColorRgba {
    /// Constructs an opaque RGB color.
    #[must_use]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Parses `#RRGGBB` or `RRGGBB` hex string.
    #[must_use]
    pub fn from_hex_rgb(hex: &str) -> Option<Self> {
        let s = hex.strip_prefix('#').unwrap_or(hex);
        if s.len() != 6 {
            return None;
        }
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        Some(Self::rgb(r, g, b))
    }

    /// Formats as uppercase `#RRGGBB`.
    #[must_use]
    pub fn to_hex_rgb(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

/// Horizontal text alignment inside a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum HorizontalAlign {
    /// Type-dependent alignment (numbers right, text left, booleans/errors centered).
    #[default]
    General,
    /// Left-aligned.
    Left,
    /// Horizontally centered.
    Center,
    /// Right-aligned.
    Right,
    /// Repeat value to fill cell width.
    Fill,
    /// Justify wrapped lines.
    Justify,
    /// Center across selection without merging.
    CenterContinuous,
    /// Distributed evenly across width.
    Distributed,
}

/// Vertical text alignment inside a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum VerticalAlign {
    /// Top-aligned.
    Top,
    /// Vertically centered.
    Center,
    /// Bottom-aligned (Excel default).
    #[default]
    Bottom,
    /// Justified vertically.
    Justify,
    /// Distributed vertically.
    Distributed,
}

/// OOXML cell border stroke style (`ST_BorderStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BorderLineStyle {
    /// Thin solid border (`1px`).
    Thin,
    /// Medium solid border (`2px`).
    Medium,
    /// Thick solid border (`3px`).
    Thick,
    /// Double-line parallel border.
    Double,
    /// Ultra-fine hairline / fine-dot border.
    Hair,
    /// Dotted border.
    Dotted,
    /// Thin dashed border.
    Dashed,
    /// Thin dash-dot border.
    DashDot,
    /// Thin dash-dot-dot border.
    DashDotDot,
    /// Medium dashed border.
    MediumDashed,
    /// Medium dash-dot border.
    MediumDashDot,
    /// Medium dash-dot-dot border.
    MediumDashDotDot,
    /// Slanted dash-dot border.
    SlantDashDot,
}

/// Single border edge specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BorderEdge {
    /// Stroke line style.
    pub style: BorderLineStyle,
    /// Resolved RGB color (`None` means automatic / default grid foreground).
    pub color: Option<ColorRgba>,
}

/// Four-sided + diagonal borders of a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct CellBorders {
    /// Top border edge.
    pub top: Option<BorderEdge>,
    /// Right border edge.
    pub right: Option<BorderEdge>,
    /// Bottom border edge.
    pub bottom: Option<BorderEdge>,
    /// Left border edge.
    pub left: Option<BorderEdge>,
    /// Diagonal stroke style/color when `diagonal_up` or `diagonal_down` is true.
    pub diagonal: Option<BorderEdge>,
    /// Bottom-left to top-right diagonal line.
    pub diagonal_up: bool,
    /// Top-left to bottom-right diagonal line.
    pub diagonal_down: bool,
}

/// Font attributes of a cell style.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontStyle {
    /// Typeface family name (e.g. `"Calibri"`, `"Inter"`, `"Microsoft YaHei"`).
    pub name: String,
    /// Font size in points.
    pub size_pt: f32,
    /// Bold weight.
    pub bold: bool,
    /// Italic posture.
    pub italic: bool,
    /// Single underline.
    pub underline: bool,
    /// Strikethrough line.
    pub strikethrough: bool,
    /// Resolved text color (`None` means automatic text color).
    pub color: Option<ColorRgba>,
}

impl Default for FontStyle {
    fn default() -> Self {
        Self {
            name: "Inter".to_string(),
            size_pt: 11.0,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            color: None,
        }
    }
}

/// Complete resolved formatting attributes of a spreadsheet cell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellStyle {
    /// Font typography and foreground color.
    pub font: FontStyle,
    /// Solid background fill color (`None` means transparent / default sheet background).
    pub fill_color: Option<ColorRgba>,
    /// Cell border edges.
    pub borders: CellBorders,
    /// Horizontal alignment.
    pub horizontal_align: HorizontalAlign,
    /// Vertical alignment.
    pub vertical_align: VerticalAlign,
    /// Whether text wraps within column width.
    pub wrap_text: bool,
    /// Horizontal indentation level (`0..=250`, where each unit indents by ~3 space widths).
    #[serde(default)]
    pub indent: u8,
    /// Excel number format string (e.g. `"general"`, `"#,##0.00"`, `"yyyy-mm-dd"`).
    pub number_format: String,
    /// Leading apostrophe text-prefix indicator (`'123`).
    pub quote_prefix: bool,
}

impl Default for CellStyle {
    fn default() -> Self {
        Self {
            font: FontStyle::default(),
            fill_color: None,
            borders: CellBorders::default(),
            horizontal_align: HorizontalAlign::General,
            vertical_align: VerticalAlign::Bottom,
            wrap_text: false,
            indent: 0,
            number_format: "general".to_string(),
            quote_prefix: false,
        }
    }
}

/// Frozen top rows and left columns configuration for a worksheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct FreezePane {
    /// Number of frozen rows pinned at the top (`0` = none).
    pub frozen_rows: u32,
    /// Number of frozen columns pinned on the left (`0` = none).
    pub frozen_cols: u32,
}

/// Visibility state of a worksheet tab (`ST_SheetState` in OOXML).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum SheetVisibility {
    /// Normal visible tab.
    #[default]
    Visible,
    /// Hidden via UI (can be unhidden by user).
    Hidden,
    /// Hidden programmatically (`veryHidden` in VBA/OOXML).
    VeryHidden,
}

/// Summary metadata for a worksheet in a workbook.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SheetInfo {
    /// Zero-based position in the workbook's tab order.
    pub index: u32,
    /// Stable OOXML `sheetId` identifier.
    pub sheet_id: u32,
    /// User-visible worksheet title.
    pub name: String,
    /// Visibility state.
    pub visibility: SheetVisibility,
    /// Optional custom tab accent color.
    pub tab_color: Option<ColorRgba>,
    /// Whether gridlines are enabled on this worksheet.
    pub show_grid_lines: bool,
    /// Bounding box of populated cells on this worksheet, if non-empty.
    pub used_range: Option<CellRange>,
}

/// Non-default row height / visibility override for fast coordinate indexing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RowMetric {
    /// Zero-based row index.
    pub row: u32,
    /// Unhidden height in logical pixels.
    pub height_px: f32,
    /// Whether this row has an explicit custom height.
    pub custom_height: bool,
    /// Whether this row is hidden.
    pub hidden: bool,
}

/// Non-default column span width / visibility override for fast coordinate indexing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ColSpanMetric {
    /// Inclusive start zero-based column index.
    pub start_col: u32,
    /// Inclusive end zero-based column index.
    pub end_col: u32,
    /// Unhidden width in logical pixels.
    pub width_px: f32,
    /// Whether this span has an explicit custom width.
    pub custom_width: bool,
    /// Whether columns in this span are hidden.
    pub hidden: bool,
}

/// Workbook or worksheet-scoped defined name (`<definedName>`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefinedNameInfo {
    /// Defined name identifier.
    pub name: String,
    /// `None` for workbook-global scope, or `Some(sheet_index)` for sheet-local scope.
    pub scope_sheet: Option<u32>,
    /// Formula or range reference assigned to this name.
    pub formula: String,
}

/// Cell note / comment attached to a worksheet coordinate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellComment {
    /// Cell coordinate holding the comment.
    pub coord: CellCoord,
    /// Author display name.
    pub author: String,
    /// Plain-text comment body.
    pub text: String,
}

/// Hyperlink metadata attached to a cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellHyperlink {
    /// Cell coordinate holding the hyperlink.
    pub coord: CellCoord,
    /// External URL/URI or internal workbook reference (`Sheet2!A1`).
    pub target: String,
    /// `true` if `target` refers to an internal workbook location rather than an external URI.
    pub is_internal: bool,
    /// Optional hover tooltip text.
    pub tooltip: Option<String>,
}
