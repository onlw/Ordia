//! Complete Excel OOXML number-formatting engine (`M2-04`):
//! General, Number, Currency, Accounting, Date & Time (1900 serials including Lotus 1-2-3
//! `1900-02-29` leap-year compatibility), Percentage, Fraction (continued-fraction rational
//! approximation + fixed denominators), Scientific, Text `@`, up to 4 `;`-delimited sections,
//! conditional colors/predicates (`[Red]`, `[>=100]`), and Chinese date/numeral tokens
//! (`yyyy"年"m"月"d"日"`, `aaaa`/`aaa` weekday, `上午/下午`, `[DBNum1]`/`[DBNum2]`).

use crate::types::{CellValue, ColorRgba};

/// Formatted display string paired with an optional conditional color override (e.g. `[Red]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormattedValue {
    /// Display string rendered inside the cell.
    pub text: String,
    /// Optional color specified by a bracketed format modifier such as `[Red]` or `[Color10]`.
    pub color: Option<ColorRgba>,
}

impl FormattedValue {
    /// Creates a plain formatted value with no color override.
    #[must_use]
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            color: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CmpOp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

impl CmpOp {
    fn matches(self, lhs: f64, rhs: f64) -> bool {
        match self {
            Self::Lt => lhs < rhs,
            Self::Le => lhs <= rhs,
            Self::Gt => lhs > rhs,
            Self::Ge => lhs >= rhs,
            Self::Eq => (lhs - rhs).abs() <= 1e-12,
            Self::Ne => (lhs - rhs).abs() > 1e-12,
        }
    }

    fn handles_negative(self, rhs: f64) -> bool {
        matches!(self, Self::Lt | Self::Le if rhs <= 0.0) || matches!(self, Self::Eq if rhs < 0.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DbNumMode {
    None,
    /// `[DBNum1]` Chinese lowercase numerals (`〇一二三四五六七八九十百千万`).
    Lower,
    /// `[DBNum2]` Chinese uppercase financial numerals (`零壹贰叁肆伍陆柒捌玖拾佰仟万`).
    Upper,
}

#[derive(Debug, Clone)]
enum SectionToken {
    Literal(String),
    SkipWidth,
    RepeatSpace,
    BracketRaw(String),
    RawCode(char),
}

#[derive(Debug, Clone)]
struct ParsedSection {
    color: Option<ColorRgba>,
    condition: Option<(CmpOp, f64)>,
    db_num: DbNumMode,
    zh_locale: bool,
    tokens: Vec<SectionToken>,
}

/// Formats `value` according to an Excel number-format string `format_code`.
#[must_use]
pub fn format_excel_value(value: &CellValue, format_code: &str) -> FormattedValue {
    match value {
        CellValue::Empty => FormattedValue::plain(""),
        CellValue::Boolean(b) => FormattedValue::plain(if *b { "TRUE" } else { "FALSE" }),
        CellValue::Error(err) => FormattedValue::plain(err.as_str()),
        CellValue::Text(s) => format_text_cell(s, format_code),
        CellValue::Number(n) => format_number_cell(*n, format_code),
    }
}

/// Convenience helper formatting an `f64` directly to its display string.
#[must_use]
pub fn format_excel_number_str(n: f64, format_code: &str) -> String {
    format_excel_value(&CellValue::Number(n), format_code).text
}

fn is_general_code(code: &str) -> bool {
    let t = code.trim();
    t.is_empty() || t.eq_ignore_ascii_case("general") || t.eq_ignore_ascii_case("standard")
}

/// Formats an IEEE-754 `f64` using Excel's `General` rules (up to 11 significant digits,
/// stripping trailing decimal zeros, switching to scientific notation for `|x| >= 1e11` or tiny values).
#[must_use]
pub fn format_general_number(n: f64) -> String {
    if !n.is_finite() {
        return "#NUM!".to_string();
    }
    if n == 0.0 {
        return "0".to_string();
    }
    let abs = n.abs();
    let sign = if n < 0.0 { "-" } else { "" };

    if abs >= 1e11 || abs < 1e-9 {
        return format_general_scientific(n);
    }
    if abs < 1e-4 {
        let fixed = trim_decimal_zeros(&format!("{abs:.10}"));
        if fixed.len() > 11 || fixed == "0" {
            return format_general_scientific(n);
        }
        return format!("{sign}{fixed}");
    }

    let int_digits = (abs.floor().log10().floor() as i32 + 1).max(1);
    if (abs - abs.round()).abs() < 1e-12 && int_digits <= 11 {
        return format!("{sign}{:.0}", abs.round());
    }
    let frac_prec = (11 - int_digits).clamp(0, 10) as usize;
    let rounded = round_half_away(abs, frac_prec as i32);
    let formatted = trim_decimal_zeros(&format!("{rounded:.frac_prec$}"));
    if formatted == "0" {
        "0".to_string()
    } else {
        format!("{sign}{formatted}")
    }
}

fn format_general_scientific(n: f64) -> String {
    let abs = n.abs();
    let sign = if n < 0.0 { "-" } else { "" };
    let exp = abs.log10().floor() as i32;
    let mant = abs / 10_f64.powi(exp);
    let mut rounded_mant = round_half_away(mant, 5);
    let mut adj_exp = exp;
    if rounded_mant >= 10.0 {
        rounded_mant /= 10.0;
        adj_exp += 1;
    }
    let mant_str = trim_decimal_zeros(&format!("{rounded_mant:.5}"));
    let exp_sign = if adj_exp >= 0 { '+' } else { '-' };
    format!("{sign}{mant_str}E{exp_sign}{:02}", adj_exp.unsigned_abs())
}

fn trim_decimal_zeros(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

fn round_half_away(abs_val: f64, decimals: i32) -> f64 {
    if !abs_val.is_finite() {
        return abs_val;
    }
    if decimals >= 0 {
        let factor = 10_f64.powi(decimals.min(15));
        let scaled = abs_val * factor;
        // Compensate 1-ULP binary float representation (e.g., `1.005 * 100 = 100.49999999999999`).
        ((scaled + 0.5 + scaled * 1e-14).floor()) / factor
    } else {
        let div = 10_f64.powi((-decimals).min(15));
        (((abs_val / div) + 0.5 + 1e-12).floor()) * div
    }
}

fn split_format_sections(code: &str) -> Vec<&str> {
    let mut sections = Vec::with_capacity(4);
    let mut in_quotes = false;
    let mut in_brackets = false;
    let mut escaped = false;
    let mut start = 0usize;

    for (idx, ch) in code.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_quotes {
            if ch == '"' {
                in_quotes = false;
            }
            continue;
        }
        if in_brackets {
            if ch == ']' {
                in_brackets = false;
            }
            continue;
        }
        match ch {
            '\\' | '_' | '*' => escaped = true,
            '"' => in_quotes = true,
            '[' => in_brackets = true,
            ';' => {
                sections.push(&code[start..idx]);
                start = idx + 1;
                if sections.len() == 3 {
                    // Up to 4 sections total; rest belongs to section 4.
                    break;
                }
            }
            _ => {}
        }
    }
    sections.push(&code[start..]);
    sections
}

fn parse_section(raw: &str) -> ParsedSection {
    let mut color = None;
    let mut condition = None;
    let mut db_num = DbNumMode::None;
    let mut zh_locale = false;
    let mut tokens = Vec::new();

    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        match chars[i] {
            '"' => {
                i += 1;
                let mut lit = String::new();
                while i < chars.len() && chars[i] != '"' {
                    lit.push(chars[i]);
                    i += 1;
                }
                if i < chars.len() {
                    i += 1;
                }
                if !lit.is_empty() {
                    tokens.push(SectionToken::Literal(lit));
                }
            }
            '\\' => {
                if i + 1 < chars.len() {
                    tokens.push(SectionToken::Literal(chars[i + 1].to_string()));
                    i += 2;
                } else {
                    i += 1;
                }
            }
            '_' => {
                if i + 1 < chars.len() {
                    tokens.push(SectionToken::SkipWidth);
                    i += 2;
                } else {
                    i += 1;
                }
            }
            '*' => {
                if i + 1 < chars.len() {
                    if chars[i + 1] == ' ' {
                        tokens.push(SectionToken::RepeatSpace);
                    }
                    i += 2;
                } else {
                    i += 1;
                }
            }
            '[' => {
                i += 1;
                let mut inner = String::new();
                while i < chars.len() && chars[i] != ']' {
                    inner.push(chars[i]);
                    i += 1;
                }
                if i < chars.len() {
                    i += 1;
                }
                let trimmed = inner.trim();
                if let Some(rest) = inner.strip_prefix('$') {
                    let (sym, loc) = rest.split_once('-').unwrap_or((rest, ""));
                    if loc.contains("804") || loc.contains("404") {
                        zh_locale = true;
                    }
                    if !sym.is_empty() {
                        tokens.push(SectionToken::Literal(sym.to_string()));
                    }
                } else if let Some(c) = parse_bracket_color(trimmed) {
                    color = Some(c);
                } else if let Some(cond) = parse_bracket_condition(trimmed) {
                    condition = Some(cond);
                } else if trimmed.eq_ignore_ascii_case("DBNum1") {
                    db_num = DbNumMode::Lower;
                } else if trimmed.eq_ignore_ascii_case("DBNum2") {
                    db_num = DbNumMode::Upper;
                } else {
                    // Elapsed time `[h]`, `[mm]`, `[ss]`, etc.
                    tokens.push(SectionToken::BracketRaw(trimmed.to_string()));
                }
            }
            c => {
                tokens.push(SectionToken::RawCode(c));
                i += 1;
            }
        }
    }

    ParsedSection {
        color,
        condition,
        db_num,
        zh_locale,
        tokens,
    }
}

fn parse_bracket_color(s: &str) -> Option<ColorRgba> {
    let lower = s.to_ascii_lowercase();
    match lower.as_str() {
        "black" | "黑色" => Some(ColorRgba::rgb(0, 0, 0)),
        "white" | "白色" => Some(ColorRgba::rgb(255, 255, 255)),
        "red" | "红色" => Some(ColorRgba::rgb(255, 0, 0)),
        "green" | "绿色" => Some(ColorRgba::rgb(0, 128, 0)),
        "blue" | "蓝色" => Some(ColorRgba::rgb(0, 0, 255)),
        "yellow" | "黄色" => Some(ColorRgba::rgb(255, 255, 0)),
        "magenta" | "洋红" => Some(ColorRgba::rgb(255, 0, 255)),
        "cyan" | "青色" => Some(ColorRgba::rgb(0, 255, 255)),
        _ => {
            let num_str = lower.strip_prefix("color")?.trim();
            let idx = num_str.parse::<u8>().ok()?;
            match idx {
                1 => Some(ColorRgba::rgb(0, 0, 0)),
                2 => Some(ColorRgba::rgb(255, 255, 255)),
                3 => Some(ColorRgba::rgb(255, 0, 0)),
                4 => Some(ColorRgba::rgb(0, 255, 0)),
                5 => Some(ColorRgba::rgb(0, 0, 255)),
                6 => Some(ColorRgba::rgb(255, 255, 0)),
                7 => Some(ColorRgba::rgb(255, 0, 255)),
                8 => Some(ColorRgba::rgb(0, 255, 255)),
                9 => Some(ColorRgba::rgb(128, 0, 0)),
                10 => Some(ColorRgba::rgb(0, 128, 0)),
                11 => Some(ColorRgba::rgb(0, 0, 128)),
                12..=56 => Some(ColorRgba::rgb(
                    idx.wrapping_mul(37),
                    idx.wrapping_mul(73),
                    idx.wrapping_mul(109),
                )),
                _ => None,
            }
        }
    }
}

fn parse_bracket_condition(s: &str) -> Option<(CmpOp, f64)> {
    let (op, rest) = if let Some(r) = s.strip_prefix(">=") {
        (CmpOp::Ge, r)
    } else if let Some(r) = s.strip_prefix("<=") {
        (CmpOp::Le, r)
    } else if let Some(r) = s.strip_prefix("<>") {
        (CmpOp::Ne, r)
    } else if let Some(r) = s.strip_prefix('>') {
        (CmpOp::Gt, r)
    } else if let Some(r) = s.strip_prefix('<') {
        (CmpOp::Lt, r)
    } else {
        let r = s.strip_prefix('=')?;
        (CmpOp::Eq, r)
    };
    let val = rest.trim().parse::<f64>().ok()?;
    Some((op, val))
}

fn section_contains_at(sec: &ParsedSection) -> bool {
    sec.tokens
        .iter()
        .any(|t| matches!(t, SectionToken::RawCode('@')))
}

fn format_text_section(text: &str, sec: &ParsedSection) -> FormattedValue {
    let mut out = String::new();
    for tok in &sec.tokens {
        match tok {
            SectionToken::Literal(s) => out.push_str(s),
            SectionToken::SkipWidth | SectionToken::RepeatSpace => out.push(' '),
            SectionToken::RawCode('@') => out.push_str(text),
            SectionToken::RawCode(c) => out.push(*c),
            SectionToken::BracketRaw(_) => {}
        }
    }
    FormattedValue {
        text: out,
        color: sec.color,
    }
}

fn format_text_cell(text: &str, format_code: &str) -> FormattedValue {
    if is_general_code(format_code) {
        return FormattedValue::plain(text);
    }
    let raw_sections = split_format_sections(format_code);
    let parsed: Vec<ParsedSection> = raw_sections.into_iter().map(parse_section).collect();
    if parsed.len() >= 4 {
        return format_text_section(text, &parsed[3]);
    }
    if parsed.len() == 1 && section_contains_at(&parsed[0]) {
        return format_text_section(text, &parsed[0]);
    }
    FormattedValue::plain(text)
}

fn format_number_cell(n: f64, format_code: &str) -> FormattedValue {
    if !n.is_finite() {
        return FormattedValue::plain("#NUM!");
    }
    if is_general_code(format_code) {
        return FormattedValue::plain(format_general_number(n));
    }

    let raw_sections = split_format_sections(format_code);
    let parsed: Vec<ParsedSection> = raw_sections.into_iter().map(parse_section).collect();
    let num_sections = &parsed[..parsed.len().min(3)];
    let any_condition = num_sections.iter().any(|s| s.condition.is_some());

    // Select active section + whether minus sign should be auto-prepended.
    let (sec, auto_minus) = if any_condition {
        let mut chosen: Option<(&ParsedSection, bool)> = None;
        for s in num_sections {
            if let Some((op, rhs)) = s.condition
                && op.matches(n, rhs)
            {
                let show_minus = n < 0.0 && !op.handles_negative(rhs);
                chosen = Some((s, show_minus));
                break;
            }
        }
        if let Some(pair) = chosen {
            pair
        } else if let Some(fallback) = num_sections.iter().find(|s| s.condition.is_none()) {
            let explicit_sign = fallback
                .tokens
                .iter()
                .any(|t| matches!(t, SectionToken::RawCode('-' | '(')));
            (fallback, n < 0.0 && !explicit_sign)
        } else {
            return FormattedValue::plain(format_general_number(n));
        }
    } else {
        match parsed.len() {
            1 => (&parsed[0], n < 0.0),
            2 => {
                if n < 0.0 {
                    (&parsed[1], false)
                } else {
                    (&parsed[0], false)
                }
            }
            _ => {
                if n > 0.0 {
                    (&parsed[0], false)
                } else if n < 0.0 {
                    (&parsed[1], false)
                } else {
                    (&parsed[2], false)
                }
            }
        }
    };

    if sec.tokens.is_empty() {
        return FormattedValue {
            text: String::new(),
            color: sec.color,
        };
    }

    // Check if section literally says `General`
    let raw_letters: String = sec
        .tokens
        .iter()
        .filter_map(|t| match t {
            SectionToken::RawCode(c) => Some(*c),
            _ => None,
        })
        .collect();
    if raw_letters.trim().eq_ignore_ascii_case("general") {
        let val = if auto_minus { n } else { n.abs() };
        let mut s = format_general_number(val);
        if n < 0.0 && auto_minus && !s.starts_with('-') {
            s.insert(0, '-');
        }
        return FormattedValue {
            text: s,
            color: sec.color,
        };
    }

    if is_datetime_section(sec) {
        if n < 0.0 {
            // Negative serial dates cannot be displayed in Excel 1900 date system -> `#####`.
            return FormattedValue {
                text: "#####".to_string(),
                color: sec.color,
            };
        }
        return FormattedValue {
            text: render_datetime_section(n, sec),
            color: sec.color,
        };
    }

    let rendered = render_numeric_section(n.abs(), auto_minus && n < 0.0, sec);
    FormattedValue {
        text: rendered,
        color: sec.color,
    }
}

#[derive(Debug, Clone, Copy)]
struct ExcelDateTime {
    year: i32,
    month: u32,
    day: u32,
    /// `0` = Sunday .. `6` = Saturday.
    weekday: u32,
    hour: u32,
    minute: u32,
    second: u32,
    millis: u32,
    total_hours: u64,
    total_minutes: u64,
    total_seconds: u64,
}

/// Converts a non-negative Excel 1900 serial number into calendar/clock components,
/// faithfully reproducing Excel's Lotus 1-2-3 `1900-02-29` (`serial == 60`) compatibility!
fn serial_to_excel_datetime(serial: f64, subsec_digits: u32) -> ExcelDateTime {
    let precision_ms: u64 = match subsec_digits {
        0 => 1_000,
        1 => 100,
        2 => 10,
        _ => 1,
    };
    let total_ms_raw = (serial * 86_400_000.0 + 0.5).floor().max(0.0) as u64;
    let rounded_ms = ((total_ms_raw + precision_ms / 2) / precision_ms) * precision_ms;

    let day_serial = rounded_ms / 86_400_000;
    let rem_ms = (rounded_ms % 86_400_000) as u32;
    let hour = rem_ms / 3_600_000;
    let minute = (rem_ms % 3_600_000) / 60_000;
    let second = (rem_ms % 60_000) / 1_000;
    let millis = rem_ms % 1_000;

    // Excel weekday: 1900-01-01 (serial 1) is Sunday (0); serial 60 (1900-02-29) is Wednesday (3);
    // serial 61 (1900-03-01) is Thursday (4)! Wait: 1 -> Sun(0) .. 59 -> Tue(2), 60 -> Wed(3), 61 -> Thu(4):
    // Notice `(day_serial - 1) % 7` gives `0` for `1`, `2` for `59`, `3` for `60`, `4` for `61`!
    // Wait: let's check real calendar 2026-01-01:
    // Days from 1899-12-31 to 2026-01-01 = 46,022 real days; Excel serial = 46,023!
    // 2026-01-01 is Thursday (4)! `(46023 - 1) % 7 = 46022 % 7 = 4` (Thursday)!!
    //Wait: for serial 1..=59 (before the fake Feb 29): 1900-01-01 in real life was Monday,
    // while Excel's `WEEKDAY(1)` / `TEXT(1, "dddd")` returns `"Sunday"` (`day_serial % 7 == 1 -> Sun`? Wait:
    // `46023 % 7 = 5`, and Thursday is index 4 when Sun=0! Why? Because `46022 % 7`:
    // `46022 = 6574 * 7 + 4`! So `(46023 - 1) % 7 == 4` (Thursday)!
    // That means for `serial >= 61`, `(day_serial - 1) % 7` is Sun=0, Mon=1, Tue=2, Wed=3, Thu=4, Fri=5, Sat=6!
    // And for `serial == 0` (`1900-01-00`), Excel returns `"Saturday"` (`6`)!
    let weekday = if day_serial == 0 {
        6
    } else {
        ((day_serial - 1) % 7) as u32
    };

    let (year, month, day) = if day_serial == 0 {
        (1900, 1, 0)
    } else if day_serial == 60 {
        (1900, 2, 29)
    } else {
        let adj_days = if day_serial < 60 {
            day_serial as i64
        } else {
            (day_serial - 1) as i64
        };
        // `adj_days == 1` is `1900-01-01`. Civil days since `1970-01-01`: `1970-01-01` is `25568` days after `1899-12-31`.
        civil_from_days(adj_days - 25_568)
    };

    ExcelDateTime {
        year,
        month,
        day,
        weekday,
        hour,
        minute,
        second,
        millis,
        total_hours: rounded_ms / 3_600_000,
        total_minutes: rounded_ms / 60_000,
        total_seconds: rounded_ms / 1_000,
    }
}

/// Howard Hinnant's proleptic Gregorian `civil_from_days` (`z == 0` -> `1970-01-01`).
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = (y + i64::from(m <= 2)) as i32;
    (year, m as u32, d as u32)
}

#[derive(Debug, Clone)]
enum DtPiece {
    Lit(String),
    Year(usize),
    Month(usize),
    Day(usize),
    WeekdayEn(usize),
    WeekdayZh(usize),
    Hour(usize),
    Minute(usize),
    Second(usize),
    SubSec(usize),
    ElapsedHour(usize),
    ElapsedMinute(usize),
    ElapsedSecond(usize),
    AmPmUpper,
    AmPmLower,
    APUpper,
    APLower,
    AmPmZh,
}

fn is_datetime_section(sec: &ParsedSection) -> bool {
    let mut has_num_placeholder = false;
    for tok in &sec.tokens {
        match tok {
            SectionToken::BracketRaw(b) => {
                let bl = b.to_ascii_lowercase();
                if !bl.is_empty()
                    && (bl.chars().all(|c| c == 'h')
                        || bl.chars().all(|c| c == 'm')
                        || bl.chars().all(|c| c == 's'))
                {
                    return true;
                }
            }
            SectionToken::RawCode(c) => {
                if matches!(c, '0' | '#' | '?') {
                    has_num_placeholder = true;
                }
            }
            _ => {}
        }
    }

    // Collect raw unquoted runs to inspect date/time letters without confusing `E+` scientific.
    let mut raw = String::new();
    for tok in &sec.tokens {
        if let SectionToken::RawCode(c) = tok {
            raw.push(*c);
        } else {
            raw.push(' ');
        }
    }
    let lower = raw.to_ascii_lowercase();
    if lower.contains("e+") || lower.contains("e-") {
        return false;
    }
    if lower.contains("上午/下午")
        || lower.contains("am/pm")
        || lower.contains("a/p")
        || lower.contains("aaa")
    {
        return true;
    }
    let has_dt_char = lower
        .chars()
        .any(|c| matches!(c, 'y' | 'm' | 'd' | 'h' | 's' | 'e'));
    // If `#` or `?` appears alongside no date letters, it's numeric; if `y/m/d/h/s` appears without `#/?`, it's datetime
    // (note `.00` sub-seconds uses `0` right after `s.`).
    has_dt_char && !raw.chars().any(|c| matches!(c, '#' | '?'))
        || (has_dt_char && !has_num_placeholder)
}

fn tokenize_datetime(sec: &ParsedSection) -> (Vec<DtPiece>, bool, u32) {
    let mut pieces: Vec<DtPiece> = Vec::new();
    let mut has_12h = false;
    let mut subsec_digits = 0u32;

    // Flatten RawCode runs while preserving Literal/SkipWidth/BracketRaw boundaries.
    let mut i = 0usize;
    while i < sec.tokens.len() {
        match &sec.tokens[i] {
            SectionToken::Literal(s) => {
                pieces.push(DtPiece::Lit(s.clone()));
                i += 1;
            }
            SectionToken::SkipWidth | SectionToken::RepeatSpace => {
                pieces.push(DtPiece::Lit(" ".to_string()));
                i += 1;
            }
            SectionToken::BracketRaw(b) => {
                let bl = b.to_ascii_lowercase();
                if !bl.is_empty() && bl.chars().all(|c| c == 'h') {
                    pieces.push(DtPiece::ElapsedHour(bl.len()));
                } else if !bl.is_empty() && bl.chars().all(|c| c == 'm') {
                    pieces.push(DtPiece::ElapsedMinute(bl.len()));
                } else if !bl.is_empty() && bl.chars().all(|c| c == 's') {
                    pieces.push(DtPiece::ElapsedSecond(bl.len()));
                }
                i += 1;
            }
            SectionToken::RawCode(_) => {
                let mut run = String::new();
                while i < sec.tokens.len() {
                    if let SectionToken::RawCode(c) = sec.tokens[i] {
                        run.push(c);
                        i += 1;
                    } else {
                        break;
                    }
                }
                parse_dt_raw_run(&run, &mut pieces, &mut has_12h, &mut subsec_digits);
            }
        }
    }

    // Context-sensitive disambiguation: `Month(n)` becomes `Minute(n)` when preceded by Hour
    // or followed by Second (ignoring pure literal separators like `:` or whitespace).
    for idx in 0..pieces.len() {
        if let DtPiece::Month(len) = pieces[idx]
            && len <= 2
        {
            let prev_is_hour = pieces[..idx]
                .iter()
                .rev()
                .find(|p| !matches!(p, DtPiece::Lit(_)))
                .is_some_and(|p| matches!(p, DtPiece::Hour(_) | DtPiece::ElapsedHour(_)));
            let next_is_sec = pieces[idx + 1..]
                .iter()
                .find(|p| !matches!(p, DtPiece::Lit(_)))
                .is_some_and(|p| matches!(p, DtPiece::Second(_) | DtPiece::ElapsedSecond(_)));
            if prev_is_hour || next_is_sec {
                pieces[idx] = DtPiece::Minute(len);
            }
        }
    }

    (pieces, has_12h, subsec_digits)
}

fn parse_dt_raw_run(
    run: &str,
    pieces: &mut Vec<DtPiece>,
    has_12h: &mut bool,
    subsec_digits: &mut u32,
) {
    let chars: Vec<char> = run.chars().collect();
    let mut k = 0usize;
    while k < chars.len() {
        let rest: String = chars[k..].iter().collect();
        if rest.starts_with("上午/下午") {
            *has_12h = true;
            pieces.push(DtPiece::AmPmZh);
            k += "上午/下午".chars().count();
            continue;
        }
        let rest_lower = rest.to_ascii_lowercase();
        if rest_lower.starts_with("am/pm") {
            *has_12h = true;
            if rest.starts_with("AM") {
                pieces.push(DtPiece::AmPmUpper);
            } else {
                pieces.push(DtPiece::AmPmLower);
            }
            k += 5;
            continue;
        }
        if rest_lower.starts_with("a/p") {
            *has_12h = true;
            if rest.starts_with('A') {
                pieces.push(DtPiece::APUpper);
            } else {
                pieces.push(DtPiece::APLower);
            }
            k += 3;
            continue;
        }

        // Sub-seconds `.0`, `.00`, `.000` immediately following `Second` or `ElapsedSecond`
        if chars[k] == '.'
            && k + 1 < chars.len()
            && chars[k + 1] == '0'
            && matches!(
                pieces.last(),
                Some(DtPiece::Second(_) | DtPiece::ElapsedSecond(_))
            )
        {
            k += 1;
            let mut zeros = 0usize;
            while k < chars.len() && chars[k] == '0' && zeros < 3 {
                zeros += 1;
                k += 1;
            }
            *subsec_digits = (*subsec_digits).max(zeros as u32);
            pieces.push(DtPiece::SubSec(zeros));
            continue;
        }

        let ch = chars[k];
        let low = ch.to_ascii_lowercase();
        if matches!(low, 'y' | 'e' | 'm' | 'd' | 'a' | 'h' | 's') {
            let mut count = 1usize;
            while k + count < chars.len() && chars[k + count].to_ascii_lowercase() == low {
                count += 1;
            }
            match low {
                'y' | 'e' => pieces.push(DtPiece::Year(if low == 'e' { 4 } else { count })),
                'm' => pieces.push(DtPiece::Month(count)),
                'd' => {
                    if count >= 3 {
                        pieces.push(DtPiece::WeekdayEn(count));
                    } else {
                        pieces.push(DtPiece::Day(count));
                    }
                }
                'a' => {
                    if count >= 3 {
                        pieces.push(DtPiece::WeekdayZh(count));
                    } else {
                        pieces.push(DtPiece::Lit(chars[k..k + count].iter().collect()));
                    }
                }
                'h' => pieces.push(DtPiece::Hour(count)),
                's' => pieces.push(DtPiece::Second(count)),
                _ => {}
            }
            k += count;
        } else {
            pieces.push(DtPiece::Lit(ch.to_string()));
            k += 1;
        }
    }
}

const EN_MONTHS_SHORT: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const EN_MONTHS_LONG: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const ZH_MONTHS: [&str; 12] = [
    "一月",
    "二月",
    "三月",
    "四月",
    "五月",
    "六月",
    "七月",
    "八月",
    "九月",
    "十月",
    "十一月",
    "十二月",
];
const EN_WD_SHORT: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const EN_WD_LONG: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const ZH_WD_SHORT: [&str; 7] = ["日", "一", "二", "三", "四", "五", "六"];
const ZH_WD_LONG: [&str; 7] = [
    "星期日",
    "星期一",
    "星期二",
    "星期三",
    "星期四",
    "星期五",
    "星期六",
];

fn render_datetime_section(serial: f64, sec: &ParsedSection) -> String {
    let (pieces, has_12h, subsec_digits) = tokenize_datetime(sec);
    let dt = serial_to_excel_datetime(serial, subsec_digits);
    let mut out = String::new();

    for piece in pieces {
        match piece {
            DtPiece::Lit(s) => out.push_str(&s),
            DtPiece::Year(len) => {
                if sec.db_num != DbNumMode::None {
                    let y_str = format!("{:04}", dt.year);
                    out.push_str(&digits_to_chinese(&y_str, sec.db_num));
                } else if len <= 2 {
                    out.push_str(&format!("{:02}", dt.year.rem_euclid(100)));
                } else {
                    out.push_str(&format!("{:04}", dt.year));
                }
            }
            DtPiece::Month(len) => {
                let m_idx = (dt.month.saturating_sub(1) as usize).min(11);
                if sec.db_num != DbNumMode::None {
                    out.push_str(&small_int_to_chinese(u64::from(dt.month), sec.db_num));
                } else if len == 1 {
                    out.push_str(&dt.month.to_string());
                } else if len == 2 {
                    out.push_str(&format!("{:02}", dt.month));
                } else if len == 3 {
                    if sec.zh_locale {
                        out.push_str(ZH_MONTHS[m_idx]);
                    } else {
                        out.push_str(EN_MONTHS_SHORT[m_idx]);
                    }
                } else if len == 4 {
                    if sec.zh_locale {
                        out.push_str(ZH_MONTHS[m_idx]);
                    } else {
                        out.push_str(EN_MONTHS_LONG[m_idx]);
                    }
                } else {
                    out.push(EN_MONTHS_LONG[m_idx].chars().next().unwrap_or('J'));
                }
            }
            DtPiece::Day(len) => {
                if sec.db_num != DbNumMode::None {
                    out.push_str(&small_int_to_chinese(u64::from(dt.day), sec.db_num));
                } else if len == 1 {
                    out.push_str(&dt.day.to_string());
                } else {
                    out.push_str(&format!("{:02}", dt.day));
                }
            }
            DtPiece::WeekdayEn(len) => {
                let w = (dt.weekday as usize) % 7;
                out.push_str(if len == 3 {
                    EN_WD_SHORT[w]
                } else {
                    EN_WD_LONG[w]
                });
            }
            DtPiece::WeekdayZh(len) => {
                let w = (dt.weekday as usize) % 7;
                out.push_str(if len == 3 {
                    ZH_WD_SHORT[w]
                } else {
                    ZH_WD_LONG[w]
                });
            }
            DtPiece::Hour(len) => {
                let h = if has_12h {
                    match dt.hour % 12 {
                        0 => 12,
                        v => v,
                    }
                } else {
                    dt.hour
                };
                if len == 1 {
                    out.push_str(&h.to_string());
                } else {
                    out.push_str(&format!("{h:02}"));
                }
            }
            DtPiece::Minute(len) => {
                if len == 1 {
                    out.push_str(&dt.minute.to_string());
                } else {
                    out.push_str(&format!("{:02}", dt.minute));
                }
            }
            DtPiece::Second(len) => {
                if len == 1 {
                    out.push_str(&dt.second.to_string());
                } else {
                    out.push_str(&format!("{:02}", dt.second));
                }
            }
            DtPiece::SubSec(digits) => {
                out.push('.');
                match digits {
                    1 => out.push_str(&format!("{:01}", dt.millis / 100)),
                    2 => out.push_str(&format!("{:02}", dt.millis / 10)),
                    _ => out.push_str(&format!("{:03}", dt.millis)),
                }
            }
            DtPiece::ElapsedHour(len) => {
                let s = dt.total_hours.to_string();
                if s.len() < len {
                    for _ in 0..(len - s.len()) {
                        out.push('0');
                    }
                }
                out.push_str(&s);
            }
            DtPiece::ElapsedMinute(len) => {
                let s = dt.total_minutes.to_string();
                if s.len() < len {
                    for _ in 0..(len - s.len()) {
                        out.push('0');
                    }
                }
                out.push_str(&s);
            }
            DtPiece::ElapsedSecond(len) => {
                let s = dt.total_seconds.to_string();
                if s.len() < len {
                    for _ in 0..(len - s.len()) {
                        out.push('0');
                    }
                }
                out.push_str(&s);
            }
            DtPiece::AmPmUpper => out.push_str(if dt.hour < 12 { "AM" } else { "PM" }),
            DtPiece::AmPmLower => out.push_str(if dt.hour < 12 { "am" } else { "pm" }),
            DtPiece::APUpper => out.push(if dt.hour < 12 { 'A' } else { 'P' }),
            DtPiece::APLower => out.push(if dt.hour < 12 { 'a' } else { 'p' }),
            DtPiece::AmPmZh => out.push_str(if dt.hour < 12 { "上午" } else { "下午" }),
        }
    }

    out
}

fn render_numeric_section(mut val: f64, prepend_minus: bool, sec: &ParsedSection) -> String {
    // Detect scientific `E+` / `E-` or fraction `/` outside literals.
    let mut raw_codes = String::new();
    for tok in &sec.tokens {
        if let SectionToken::RawCode(c) = tok {
            raw_codes.push(*c);
        }
    }
    let upper = raw_codes.to_ascii_uppercase();
    if upper.contains("E+") || upper.contains("E-") {
        return render_scientific(val, prepend_minus, sec);
    }
    if raw_codes.contains('/') && raw_codes.chars().any(|c| matches!(c, '?' | '#' | '0')) {
        return render_fraction(val, prepend_minus, sec);
    }

    let pct_count = raw_codes.chars().filter(|&c| c == '%').count();
    for _ in 0..pct_count {
        val *= 100.0;
    }

    render_decimal_placeholders(val, prepend_minus, sec)
}

fn render_scientific(val: f64, prepend_minus: bool, sec: &ParsedSection) -> String {
    // Split tokens at `E`/`e` followed by `+` or `-`.
    let mut exp_pos = None;
    let mut exp_always_sign = true;
    let mut exp_upper = true;
    for i in 0..sec.tokens.len().saturating_sub(1) {
        if let (SectionToken::RawCode(e_ch), SectionToken::RawCode(s_ch)) =
            (&sec.tokens[i], &sec.tokens[i + 1])
            && matches!(e_ch, 'E' | 'e')
            && matches!(s_ch, '+' | '-')
        {
            exp_pos = Some(i);
            exp_upper = *e_ch == 'E';
            exp_always_sign = *s_ch == '+';
            break;
        }
    }
    let Some(e_idx) = exp_pos else {
        return format_general_number(if prepend_minus { -val } else { val });
    };

    let mant_sec = ParsedSection {
        color: sec.color,
        condition: sec.condition,
        db_num: DbNumMode::None,
        zh_locale: sec.zh_locale,
        tokens: sec.tokens[..e_idx].to_vec(),
    };
    let exp_tokens = &sec.tokens[e_idx + 2..];
    let exp_min_zeros = exp_tokens
        .iter()
        .filter(|t| matches!(t, SectionToken::RawCode('0')))
        .count()
        .max(1);

    // Count integer placeholders in mantissa (supports `0.00E+00` -> 1 and engineering `##0.0E+0` -> 3).
    let mut int_slots = 0usize;
    for tok in &mant_sec.tokens {
        match tok {
            SectionToken::RawCode('.') => break,
            SectionToken::RawCode('0' | '#' | '?') => int_slots += 1,
            _ => {}
        }
    }
    let group = int_slots.max(1) as i32;
    let frac_slots = mant_sec
        .tokens
        .iter()
        .skip_while(|t| !matches!(t, SectionToken::RawCode('.')))
        .skip(1)
        .filter(|t| matches!(t, SectionToken::RawCode('0' | '#' | '?')))
        .count() as i32;

    let (mant, exp) = if val == 0.0 {
        (0.0, 0)
    } else {
        let raw_exp = val.log10().floor() as i32;
        let aligned_exp = if group > 1 {
            raw_exp.div_euclid(group) * group
        } else {
            raw_exp
        };
        let mut m = val / 10_f64.powi(aligned_exp);
        m = round_half_away(m, frac_slots);
        let limit = 10_f64.powi(group);
        if m >= limit {
            (m / limit, aligned_exp + group)
        } else {
            (m, aligned_exp)
        }
    };

    let mant_str = render_decimal_placeholders(mant, prepend_minus, &mant_sec);
    let e_char = if exp_upper { 'E' } else { 'e' };
    let sign_str = if exp < 0 {
        "-"
    } else if exp_always_sign {
        "+"
    } else {
        ""
    };
    let abs_exp = exp.unsigned_abs().to_string();
    let padded_exp = if abs_exp.len() < exp_min_zeros {
        format!("{}{abs_exp}", "0".repeat(exp_min_zeros - abs_exp.len()))
    } else {
        abs_exp
    };
    format!("{mant_str}{e_char}{sign_str}{padded_exp}")
}

fn best_rational_approx(x: f64, max_den: u64) -> (u64, u64) {
    if max_den <= 1 || x <= 0.0 {
        return (x.round() as u64, 1);
    }
    let mut p0: u64 = 0;
    let mut q0: u64 = 1;
    let mut p1: u64 = 1;
    let mut q1: u64 = 0;
    let mut rem = x;

    for _ in 0..32 {
        let a = rem.floor() as u64;
        let p2 = a.saturating_mul(p1).saturating_add(p0);
        let q2 = a.saturating_mul(q1).saturating_add(q0);
        if q2 > max_den {
            let k = (max_den.saturating_sub(q0)) / q1.max(1);
            let cand_p = k.saturating_mul(p1).saturating_add(p0);
            let cand_q = k.saturating_mul(q1).saturating_add(q0);
            let err1 = (x - (p1 as f64) / (q1.max(1) as f64)).abs();
            let err_cand = (x - (cand_p as f64) / (cand_q.max(1) as f64)).abs();
            return if cand_q >= 1 && err_cand < err1 {
                (cand_p, cand_q)
            } else {
                (p1, q1.max(1))
            };
        }
        p0 = p1;
        q0 = q1;
        p1 = p2;
        q1 = q2;
        let frac = rem - (a as f64);
        if frac.abs() < 1e-11 {
            break;
        }
        rem = 1.0 / frac;
    }
    (p1, q1.max(1))
}

fn render_fraction(val: f64, prepend_minus: bool, sec: &ParsedSection) -> String {
    // Flatten raw specification around `/`: optional integer part (`#`/`0` followed by space) + numerator + `/` + denominator.
    let mut raw = String::new();
    for tok in &sec.tokens {
        match tok {
            SectionToken::RawCode(c) => raw.push(*c),
            SectionToken::Literal(s) => raw.push_str(s),
            SectionToken::SkipWidth | SectionToken::RepeatSpace => raw.push(' '),
            SectionToken::BracketRaw(_) => {}
        }
    }
    let Some((left, right)) = raw.split_once('/') else {
        return format_general_number(val);
    };
    let sign = if prepend_minus { "-" } else { "" };

    let left_trim = left.trim_end();
    let (has_int_part, num_spec) = if let Some((int_part, num_part)) = left_trim.rsplit_once(' ') {
        let has_i = int_part.chars().any(|c| matches!(c, '#' | '0' | '?'));
        (has_i, num_part.trim())
    } else {
        (false, left_trim.trim())
    };
    let den_spec_token = right
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || matches!(c, '?' | '#' | '0'))
        .collect::<String>();
    let suffix: String = right
        .trim_start()
        .chars()
        .skip(den_spec_token.chars().count())
        .collect();

    let fixed_den = den_spec_token.parse::<u64>().ok().filter(|&d| d > 0);
    let max_den = fixed_den.unwrap_or_else(|| {
        let digits = den_spec_token.len().clamp(1, 5) as u32;
        10_u64.pow(digits) - 1
    });

    if has_int_part {
        let mut whole = val.floor() as u64;
        let frac = val - (whole as f64);
        let (mut num, den) = if let Some(fd) = fixed_den {
            (((frac * (fd as f64)) + 0.5).floor() as u64, fd)
        } else {
            best_rational_approx(frac, max_den)
        };
        if num >= den {
            whole += num / den;
            num %= den;
        }
        if num == 0 {
            let blank_w = num_spec.len() + 1 + den_spec_token.len();
            if num_spec.contains('0') {
                format!("{sign}{whole} 0/{den}{suffix}")
            } else {
                format!("{sign}{whole}{}{suffix}", " ".repeat(1 + blank_w))
            }
        } else {
            let num_pad = if num_spec.starts_with('?') && num.to_string().len() < num_spec.len() {
                format!(
                    "{}{num}",
                    " ".repeat(num_spec.len() - num.to_string().len())
                )
            } else {
                num.to_string()
            };
            let den_str = den.to_string();
            let den_pad = if fixed_den.is_none()
                && den_spec_token.ends_with('?')
                && den_str.len() < den_spec_token.len()
            {
                format!(
                    "{den_str}{}",
                    " ".repeat(den_spec_token.len() - den_str.len())
                )
            } else {
                den_str
            };
            if whole == 0 && !left_trim.starts_with('0') {
                format!("{sign}{num_pad}/{den_pad}{suffix}")
            } else {
                format!("{sign}{whole} {num_pad}/{den_pad}{suffix}")
            }
        }
    } else {
        let (num, den) = if let Some(fd) = fixed_den {
            (((val * (fd as f64)) + 0.5).floor() as u64, fd)
        } else {
            best_rational_approx(val, max_den)
        };
        format!("{sign}{num}/{den}{suffix}")
    }
}

fn render_decimal_placeholders(mut val: f64, prepend_minus: bool, sec: &ParsedSection) -> String {
    let is_ph = |t: &SectionToken| matches!(t, SectionToken::RawCode('0' | '#' | '?'));
    let first_ph = sec.tokens.iter().position(is_ph);
    let last_ph = sec.tokens.iter().rposition(is_ph);
    let Some((first_idx, last_idx)) = first_ph.zip(last_ph) else {
        // Pure literal section (e.g. `"-"` in section 3 for zero).
        let mut lit = String::new();
        if prepend_minus {
            lit.push('-');
        }
        for tok in &sec.tokens {
            match tok {
                SectionToken::Literal(s) => lit.push_str(s),
                SectionToken::SkipWidth | SectionToken::RepeatSpace => lit.push(' '),
                SectionToken::RawCode(c) => lit.push(*c),
                SectionToken::BracketRaw(_) => {}
            }
        }
        return lit;
    };

    // Find decimal point `.` inside `[first_idx..=last_idx]` (or immediately before `first_idx`, e.g. `.00`).
    let mut dot_idx = None;
    for (idx, tok) in sec.tokens.iter().enumerate() {
        if matches!(tok, SectionToken::RawCode('.')) && idx <= last_idx {
            dot_idx = Some(idx);
            break;
        }
    }

    // Trailing commas immediately after the last integer/decimal placeholder scale by `1/1000` each!
    let mut trailing_commas = 0usize;
    let mut scan_after = last_idx + 1;
    while scan_after < sec.tokens.len()
        && matches!(sec.tokens[scan_after], SectionToken::RawCode(','))
    {
        trailing_commas += 1;
        scan_after += 1;
    }
    // Also handle `#,##0,.0` where scaling comma(s) sit immediately before `.`!
    let mut pre_dot_trailing_commas = 0usize;
    if let Some(d_i) = dot_idx {
        let mut back = d_i;
        while back > first_idx && matches!(sec.tokens[back - 1], SectionToken::RawCode(',')) {
            pre_dot_trailing_commas += 1;
            back -= 1;
        }
    }
    let scale_commas = trailing_commas + pre_dot_trailing_commas;
    for _ in 0..scale_commas {
        val /= 1_000.0;
    }

    let int_end = dot_idx.map_or(last_idx + 1, |d| d - pre_dot_trailing_commas);
    let int_slice = if int_end > first_idx {
        &sec.tokens[first_idx..int_end]
    } else {
        &[]
    };
    let mut use_grouping = false;
    let mut int_phs = Vec::new();
    for (idx, tok) in int_slice.iter().enumerate() {
        match tok {
            SectionToken::RawCode(c @ ('0' | '#' | '?')) => int_phs.push(*c),
            SectionToken::RawCode(',') => {
                // Internal comma between two integer placeholders enables thousands grouping.
                let has_left = int_slice[..idx].iter().any(is_ph);
                let has_right = int_slice[idx + 1..].iter().any(is_ph);
                if has_left && has_right {
                    use_grouping = true;
                }
            }
            _ => {}
        }
    }

    let mut frac_phs = Vec::new();
    if let Some(d_i) = dot_idx {
        for tok in &sec.tokens[d_i + 1..=last_idx] {
            if let SectionToken::RawCode(c @ ('0' | '#' | '?')) = tok {
                frac_phs.push(*c);
            }
        }
    }

    let rounded = round_half_away(val, frac_phs.len() as i32);
    if sec.db_num != DbNumMode::None && frac_phs.is_empty() {
        let cn = uint_to_chinese(rounded.round() as u64, sec.db_num);
        let mut out = String::new();
        if prepend_minus {
            out.push('-');
        }
        append_non_ph_slice(&sec.tokens[..first_idx], &mut out);
        out.push_str(&cn);
        append_non_ph_slice(&sec.tokens[scan_after..], &mut out);
        return out;
    }

    let frac_len = frac_phs.len();
    let fixed_str = format!("{rounded:.frac_len$}");
    let (raw_int_digits, raw_frac_digits) = fixed_str
        .split_once('.')
        .unwrap_or((fixed_str.as_str(), ""));
    let sig_int = raw_int_digits.trim_start_matches('0');

    // Format integer digits against `int_phs` right-to-left.
    let mut rev_int_chars: Vec<char> = Vec::new();
    let sig_bytes = sig_int.as_bytes();
    let max_i = sig_bytes.len().max(int_phs.len());
    let mut emitted_digit_pos = 0usize;
    for r in 0..max_i {
        let ph = if r < int_phs.len() {
            int_phs[int_phs.len() - 1 - r]
        } else {
            '#'
        };
        if r < sig_bytes.len() {
            if use_grouping && emitted_digit_pos > 0 && emitted_digit_pos.is_multiple_of(3) {
                rev_int_chars.push(',');
            }
            rev_int_chars.push(sig_bytes[sig_bytes.len() - 1 - r] as char);
            emitted_digit_pos += 1;
        } else {
            match ph {
                '0' => {
                    if use_grouping && emitted_digit_pos > 0 && emitted_digit_pos.is_multiple_of(3)
                    {
                        rev_int_chars.push(',');
                    }
                    rev_int_chars.push('0');
                    emitted_digit_pos += 1;
                }
                '?' => rev_int_chars.push(' '),
                _ => {}
            }
        }
    }
    rev_int_chars.reverse();
    let int_out: String = rev_int_chars.into_iter().collect();

    // Format fractional digits against `frac_phs` left-to-right.
    let mut frac_out = String::new();
    let sig_frac_trimmed = raw_frac_digits.trim_end_matches('0');
    for (i, &ph) in frac_phs.iter().enumerate() {
        let d_ch = raw_frac_digits
            .as_bytes()
            .get(i)
            .map(|&b| b as char)
            .unwrap_or('0');
        if i < sig_frac_trimmed.len() {
            frac_out.push(d_ch);
        } else {
            match ph {
                '0' => frac_out.push('0'),
                '?' => frac_out.push(' '),
                _ => {}
            }
        }
    }

    let mut out = String::new();
    let prefix_end = dot_idx.filter(|&d| d < first_idx).unwrap_or(first_idx);
    if prepend_minus {
        out.push('-');
    }
    append_non_ph_slice(&sec.tokens[..prefix_end], &mut out);
    out.push_str(&int_out);

    if has_embedded_separators(int_slice) && !use_grouping {
        out.clear();
        if prepend_minus {
            out.push('-');
        }
        append_non_ph_slice(&sec.tokens[..prefix_end], &mut out);
        out.push_str(&render_masked_integer(sig_int, int_slice));
    }

    if dot_idx.is_some() {
        out.push('.');
        out.push_str(&frac_out);
    }
    append_non_ph_slice(&sec.tokens[scan_after..], &mut out);
    out
}

fn has_embedded_separators(slice: &[SectionToken]) -> bool {
    slice.iter().any(|t| match t {
        SectionToken::Literal(_) | SectionToken::SkipWidth => true,
        SectionToken::RawCode(c) => !matches!(c, '0' | '#' | '?' | ','),
        _ => false,
    })
}

fn render_masked_integer(sig_int: &str, slice: &[SectionToken]) -> String {
    let total_phs = slice
        .iter()
        .filter(|t| matches!(t, SectionToken::RawCode('0' | '#' | '?')))
        .count();
    let padded = if sig_int.len() < total_phs {
        format!("{}{sig_int}", "0".repeat(total_phs - sig_int.len()))
    } else {
        sig_int.to_string()
    };
    let extra_lead = padded.len().saturating_sub(total_phs);
    let mut out = padded[..extra_lead].to_string();
    let mut d_idx = extra_lead;
    for tok in slice {
        match tok {
            SectionToken::RawCode(ph @ ('0' | '#' | '?')) => {
                let ch = padded
                    .as_bytes()
                    .get(d_idx)
                    .map(|&b| b as char)
                    .unwrap_or('0');
                let is_leading_pad = d_idx + sig_int.len() < padded.len();
                if is_leading_pad {
                    match ph {
                        '0' => out.push('0'),
                        '?' => out.push(' '),
                        _ => {}
                    }
                } else {
                    out.push(ch);
                }
                d_idx += 1;
            }
            SectionToken::RawCode(',') => {}
            SectionToken::RawCode(c) => out.push(*c),
            SectionToken::Literal(s) => out.push_str(s),
            SectionToken::SkipWidth | SectionToken::RepeatSpace => out.push(' '),
            SectionToken::BracketRaw(_) => {}
        }
    }
    out
}

fn append_non_ph_slice(slice: &[SectionToken], out: &mut String) {
    for tok in slice {
        match tok {
            SectionToken::Literal(s) => out.push_str(s),
            SectionToken::SkipWidth | SectionToken::RepeatSpace => out.push(' '),
            SectionToken::RawCode(c) => out.push(*c),
            SectionToken::BracketRaw(_) => {}
        }
    }
}

fn digits_to_chinese(digits: &str, mode: DbNumMode) -> String {
    let table = match mode {
        DbNumMode::Lower | DbNumMode::None => {
            ['〇', '一', '二', '三', '四', '五', '六', '七', '八', '九']
        }
        DbNumMode::Upper => ['零', '壹', '贰', '叁', '肆', '伍', '陆', '柒', '捌', '玖'],
    };
    digits
        .chars()
        .map(|c| {
            c.to_digit(10)
                .and_then(|d| table.get(d as usize).copied())
                .unwrap_or(c)
        })
        .collect()
}

fn small_int_to_chinese(n: u64, mode: DbNumMode) -> String {
    let (digits, ten) = match mode {
        DbNumMode::Lower | DbNumMode::None => (
            ['〇', '一', '二', '三', '四', '五', '六', '七', '八', '九'],
            '十',
        ),
        DbNumMode::Upper => (
            ['零', '壹', '贰', '叁', '肆', '伍', '陆', '柒', '捌', '玖'],
            '拾',
        ),
    };
    if n < 10 {
        digits[n as usize].to_string()
    } else if n == 10 {
        if mode == DbNumMode::Upper {
            format!("壹{ten}")
        } else {
            ten.to_string()
        }
    } else if n < 20 {
        if mode == DbNumMode::Upper {
            format!("壹{ten}{}", digits[(n % 10) as usize])
        } else {
            format!("{ten}{}", digits[(n % 10) as usize])
        }
    } else if n.is_multiple_of(10) && n < 100 {
        format!("{}{ten}", digits[(n / 10) as usize])
    } else if n < 100 {
        format!(
            "{}{ten}{}",
            digits[(n / 10) as usize],
            digits[(n % 10) as usize]
        )
    } else {
        uint_to_chinese(n, mode)
    }
}

fn uint_to_chinese(n: u64, mode: DbNumMode) -> String {
    if n < 100 {
        return small_int_to_chinese(n, mode);
    }
    let (digits, units) = match mode {
        DbNumMode::Lower | DbNumMode::None => (
            ['〇', '一', '二', '三', '四', '五', '六', '七', '八', '九'],
            ["", "十", "百", "千"],
        ),
        DbNumMode::Upper => (
            ['零', '壹', '贰', '叁', '肆', '伍', '陆', '柒', '捌', '玖'],
            ["", "拾", "佰", "仟"],
        ),
    };
    let big_units = ["", "万", "亿", "兆"];
    let mut rem = n;
    let mut groups = Vec::new();
    while rem > 0 {
        groups.push((rem % 10_000) as u16);
        rem /= 10_000;
    }
    let mut out = String::new();
    let mut need_zero = false;
    for (g_idx, &grp) in groups.iter().enumerate().rev() {
        if grp == 0 {
            if !out.is_empty() {
                need_zero = true;
            }
            continue;
        }
        if need_zero || (!out.is_empty() && grp < 1_000) {
            out.push(digits[0]);
            need_zero = false;
        }
        let d = [grp / 1_000, (grp / 100) % 10, (grp / 10) % 10, grp % 10];
        let mut prev_zero = false;
        let mut started = false;
        for (i, &digit) in d.iter().enumerate() {
            let unit_pos = 3 - i;
            if digit == 0 {
                if started {
                    prev_zero = true;
                }
            } else {
                if prev_zero {
                    out.push(digits[0]);
                    prev_zero = false;
                }
                if !(mode != DbNumMode::Upper
                    && out.is_empty()
                    && !started
                    && unit_pos == 1
                    && digit == 1)
                {
                    out.push(digits[digit as usize]);
                }
                out.push_str(units[unit_pos]);
                started = true;
            }
        }
        if let Some(&bu) = big_units.get(g_idx) {
            out.push_str(bu);
        }
    }
    out
}
