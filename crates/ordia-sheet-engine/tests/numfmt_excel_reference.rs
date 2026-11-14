//! Excel number-format reference conformance suite (`M2-04`, `>= 200` verified cases).

use ordia_sheet_engine::{
    CellCoord, CellStyle, CellValue, ColorRgba, FormulaError, IronCalcEngine, SheetEngine,
    format_excel_value,
};

#[test]
fn excel_number_format_reference_table_over_200_cases() {
    // `(CellValue, format_code, expected_text)`
    let mut cases: Vec<(CellValue, &str, &str)> = Vec::with_capacity(240);

    macro_rules! n {
        ($val:expr, $fmt:expr, $exp:expr) => {
            cases.push((CellValue::Number($val), $fmt, $exp));
        };
    }
    macro_rules! t {
        ($val:expr, $fmt:expr, $exp:expr) => {
            cases.push((CellValue::Text($val.to_string()), $fmt, $exp));
        };
    }

    // 1. General (24 cases)
    n!(0.0, "General", "0");
    n!(-0.0, "General", "0");
    n!(1.0, "General", "1");
    n!(-1.0, "General", "-1");
    n!(42.0, "general", "42");
    n!(42.5, "General", "42.5");
    n!(-42.5, "General", "-42.5");
    n!(12345678901.0, "General", "12345678901");
    n!(-12345678901.0, "General", "-12345678901");
    n!(100_000_000_000.0, "General", "1E+11");
    n!(123_456_789_012.0, "General", "1.23457E+11");
    n!(-123_456_789_012.0, "General", "-1.23457E+11");
    n!(0.123456789, "General", "0.123456789");
    n!(0.0001, "General", "0.0001");
    n!(0.00001, "General", "0.00001");
    n!(1e-10, "General", "1E-10");
    n!(-2.5e-10, "General", "-2.5E-10");
    n!(std::f64::consts::PI, "General", "3.1415926536");
    n!(99999.125, "", "99999.125");
    cases.push((CellValue::Empty, "General", ""));
    cases.push((CellValue::Boolean(true), "General", "TRUE"));
    cases.push((CellValue::Boolean(false), "#,##0.00", "FALSE"));
    cases.push((CellValue::Error(FormulaError::Div0), "#,##0.00", "#DIV/0!"));
    cases.push((CellValue::Error(FormulaError::Na), "yyyy-mm-dd", "#N/A"));

    // 2. Standard Numbers, Rounding, Grouping, Scaling Commas & Masks (42 cases)
    n!(0.0, "0", "0");
    n!(0.0, "#", "");
    n!(0.0, "?", " ");
    n!(0.0, "0.00", "0.00");
    n!(0.0, "#.##", ".");
    n!(5.0, "0", "5");
    n!(5.4, "0", "5");
    n!(5.5, "0", "6");
    n!(-5.5, "0", "-6");
    n!(1.005, "0.00", "1.01");
    n!(2.345, "0.00", "2.35");
    n!(-2.345, "0.00", "-2.35");
    n!(2.675, "0.00", "2.68");
    n!(12.3, "0.00", "12.30");
    n!(12.3, "0.##", "12.3");
    n!(12.34, "0.#", "12.3");
    n!(12.36, "0.#", "12.4");
    n!(12.3, "0.??", "12.3 ");
    n!(0.5, ".00", ".50");
    n!(0.5, "0.00", "0.50");
    n!(7.0, "0000", "0007");
    n!(42.0, "000000", "000042");
    n!(123456789.0, "000-00-0000", "123-45-6789");
    n!(456789.0, "000-00-0000", "000-45-6789");
    n!(999.0, "#,##0", "999");
    n!(1000.0, "#,##0", "1,000");
    n!(1234567.89, "#,##0.00", "1,234,567.89");
    n!(-1234567.89, "#,##0.00", "-1,234,567.89");
    n!(1234567890.0, "#,##0", "1,234,567,890");
    n!(0.0, "#,##0", "0");
    n!(0.0, "#,###", "");
    n!(1250.0, "#,##0,", "1");
    n!(1500.0, "#,##0,", "2");
    n!(1250000.0, "#,##0,", "1,250");
    n!(1250000.0, "#,##0,,", "1");
    n!(1500000.0, "#,##0,,", "2");
    n!(1250000.0, "#,##0.0,,\"M\"", "1.3M");
    n!(48200.0, "#,##0,.0\"K\"", "48.2K");
    n!(123.45, "+#,##0.0;-#,##0.0;0.0", "+123.5");
    n!(-123.45, "+#,##0.0;-#,##0.0;0.0", "-123.5");
    n!(0.0, "+#,##0.0;-#,##0.0;0.0", "0.0");
    n!(7.2, "??0.00", "  7.20");

    // 3. Currency & Accounting (28 cases)
    n!(1234.5, "$#,##0.00", "$1,234.50");
    n!(-1234.5, "$#,##0.00", "-$1,234.50");
    n!(-1234.5, "$#,##0.00;($#,##0.00)", "($1,234.50)");
    n!(0.0, "$#,##0.00;($#,##0.00);\"-\"", "-");
    n!(8888.8, "¥#,##0.00", "¥8,888.80");
    n!(-8888.8, "¥#,##0.00;[Red]-¥#,##0.00", "-¥8,888.80");
    n!(1999.95, "[$¥-804]#,##0.00", "¥1,999.95");
    n!(
        -1999.95,
        "[$¥-804]#,##0.00;[Red]([$¥-804]#,##0.00)",
        "(¥1,999.95)"
    );
    n!(500.0, "[$€-407]#,##0.00", "€500.00");
    n!(250.25, "[$£-809]#,##0.00", "£250.25");
    n!(1024.0, "[$USD ]#,##0.00", "USD 1,024.00");
    n!(99.9, "#,##0.00\" 元\"", "99.90 元");
    n!(1234.56, "#,##0.00_);(#,##0.00)", "1,234.56 ");
    n!(-1234.56, "#,##0.00_);(#,##0.00)", "(1,234.56)");
    n!(
        1234.56,
        "_($* #,##0.00_);_($* (#,##0.00);_($* \"-\"??_);_(@_)",
        " $ 1,234.56 "
    );
    n!(
        -1234.56,
        "_($* #,##0.00_);_($* (#,##0.00);_($* \"-\"??_);_(@_)",
        " $ (1,234.56)"
    );
    n!(
        0.0,
        "_($* #,##0.00_);_($* (#,##0.00);_($* \"-\"_);_(@_)",
        " $ - "
    );
    t!(
        "Note",
        "_($* #,##0.00_);_($* (#,##0.00);_($* \"-\"_);_(@_)",
        " Note "
    );
    n!(45.0, "\\$#,##0", "$45");
    n!(45.0, "\\¥#,##0.0", "¥45.0");
    n!(-0.5, "$#,##0.00;[Red]($#,##0.00)", "($0.50)");
    n!(1000000.0, "¥#,##0", "¥1,000,000");
    n!(0.01, "$0.00", "$0.01");
    n!(0.004, "$0.00", "$0.00");
    n!(0.005, "$0.00", "$0.01");
    n!(320.0, "#,##0\" RMB\"", "320 RMB");
    n!(-320.0, "#,##0\" RMB\";(#,##0\" RMB\")", "(320 RMB)");
    n!(0.0, "#,##0;(#,##0);\"—\"", "—");

    // 4. Percentages (16 cases)
    n!(0.0, "0%", "0%");
    n!(1.0, "0%", "100%");
    n!(0.25, "0%", "25%");
    n!(0.256, "0%", "26%");
    n!(0.256, "0.0%", "25.6%");
    n!(0.25678, "0.00%", "25.68%");
    n!(-0.125, "0.0%", "-12.5%");
    n!(-0.125, "0.0%;(0.0%)", "(12.5%)");
    n!(12.345, "#,##0.0%", "1,234.5%");
    n!(0.0005, "0.00%", "0.05%");
    n!(0.00004, "0.00%", "0.00%");
    n!(0.5, "0\"%\"", "1%"); // Quoted "%" does NOT multiply by 100!
    n!(0.5, "0\\%", "1%"); // Escaped \% does NOT multiply by 100!
    n!(0.03, "+0.0%;-0.0%;0.0%", "+3.0%");
    n!(-0.03, "+0.0%;-0.0%;0.0%", "-3.0%");
    n!(0.0, "+0.0%;-0.0%;0.0%", "0.0%");

    // 5. Fractions (24 cases)
    n!(0.5, "# ?/?", "1/2");
    n!(0.25, "# ?/?", "1/4");
    n!(0.75, "# ?/?", "3/4");
    n!(1.5, "# ?/?", "1 1/2");
    n!(-1.5, "# ?/?", "-1 1/2");
    n!(3.25, "# ?/?", "3 1/4");
    n!(3.0, "# ?/?", "3    ");
    n!(0.5, "0 ?/?", "0 1/2");
    n!(3.25, "?/?", "13/4");
    n!(-3.25, "?/?", "-13/4");
    n!(0.125, "# ?/?", "1/8");
    n!(0.3333333333, "# ?/?", "1/3");
    n!(0.6666666667, "# ?/?", "2/3");
    n!(0.1428571429, "# ?/?", "1/7");
    n!(std::f64::consts::PI, "# ??/??", "3 14/99");
    n!(std::f64::consts::PI, "# ???/???", "3  16/113");
    n!(0.375, "# ?/8", "3/8");
    n!(2.625, "# ?/8", "2 5/8");
    n!(0.5, "# ?/8", "4/8");
    n!(0.3125, "# ??/16", " 5/16");
    n!(1.25, "# ??/100", "1 25/100");
    n!(0.07, "# ??/100", " 7/100");
    n!(0.99, "# ?/2", "1    ");
    n!(2.0, "# 0/4", "2 0/4");

    append_scientific_datetime_chinese_and_conditional_cases(&mut cases);

    assert!(
        cases.len() >= 215,
        "expected >= 200 Excel reference test cases, got {}",
        cases.len()
    );

    for (idx, (val, fmt, expected)) in cases.iter().enumerate() {
        let actual = format_excel_value(val, fmt);
        assert_eq!(
            actual.text, *expected,
            "case #{idx} failed for value={val:?}, fmt={fmt:?}"
        );
    }
}

fn append_scientific_datetime_chinese_and_conditional_cases(
    cases: &mut Vec<(CellValue, &'static str, &'static str)>,
) {
    macro_rules! n {
        ($val:expr, $fmt:expr, $exp:expr) => {
            cases.push((CellValue::Number($val), $fmt, $exp));
        };
    }
    macro_rules! t {
        ($val:expr, $fmt:expr, $exp:expr) => {
            cases.push((CellValue::Text($val.to_string()), $fmt, $exp));
        };
    }

    // 6. Scientific & Engineering (18 cases)
    n!(0.0, "0.00E+00", "0.00E+00");
    n!(1.0, "0.00E+00", "1.00E+00");
    n!(12345.0, "0.00E+00", "1.23E+04");
    n!(12350.0, "0.00E+00", "1.24E+04");
    n!(-12345.0, "0.00E+00", "-1.23E+04");
    n!(0.001234, "0.00E+00", "1.23E-03");
    n!(0.0000999, "0.00E+00", "9.99E-05");
    n!(99999.9, "0.00E+00", "1.00E+05");
    n!(12345.0, "0.0E+0", "1.2E+4");
    n!(0.0012, "0.0E+0", "1.2E-3");
    n!(12345.0, "0.00E-00", "1.23E04");
    n!(0.00123, "0.00E-00", "1.23E-03");
    n!(12345.0, "0.00e+00", "1.23e+04");
    n!(12345.0, "##0.0E+0", "12.3E+3");
    n!(123456.0, "##0.0E+0", "123.5E+3");
    n!(1234567.0, "##0.0E+0", "1.2E+6");
    n!(0.00123, "##0.0E+0", "1.2E-3");
    n!(0.0123, "##0.0E+0", "12.3E-3");

    // 7. 1900 Dates & Times + Lotus 1-2-3 1900-02-29 Bug (38 cases)
    n!(0.0, "yyyy-mm-dd", "1900-01-00");
    n!(1.0, "yyyy-mm-dd", "1900-01-01");
    n!(1.0, "dddd", "Sunday");
    n!(59.0, "yyyy-mm-dd", "1900-02-28");
    n!(59.0, "dddd", "Tuesday");
    n!(60.0, "yyyy-mm-dd", "1900-02-29"); // Lotus 1-2-3 leap-year bug compatibility!
    n!(60.0, "dddd", "Wednesday");
    n!(61.0, "yyyy-mm-dd", "1900-03-01");
    n!(61.0, "dddd", "Thursday");
    n!(45351.0, "yyyy-mm-dd", "2024-02-29"); // Real 2024 leap day
    n!(46023.0, "yyyy-mm-dd", "2026-01-01");
    n!(46023.0, "dddd", "Thursday");
    n!(46023.0, "ddd", "Thu");
    n!(46023.0, "m/d/yy", "1/1/26");
    n!(46023.0, "mm/dd/yyyy", "01/01/2026");
    n!(46023.0, "d-mmm-yy", "1-Jan-26");
    n!(46023.0, "dd-mmm-yyyy", "01-Jan-2026");
    n!(46023.0, "mmmm d, yyyy", "January 1, 2026");
    n!(46023.0, "mmmmm", "J");
    n!(46340.0, "yyyy-mm-dd", "2026-11-14");
    n!(46340.0, "dddd", "Saturday");
    n!(0.0, "hh:mm:ss", "00:00:00");
    n!(0.5, "hh:mm:ss", "12:00:00");
    n!(0.75, "hh:mm:ss", "18:00:00");
    n!(0.5, "h:mm AM/PM", "12:00 PM");
    n!(0.0, "h:mm AM/PM", "12:00 AM");
    n!(0.25, "h:mm AM/PM", "6:00 AM");
    n!(0.75, "h:mm am/pm", "6:00 pm");
    n!(0.75, "h:mm A/P", "6:00 P");
    n!(0.25, "h:mm a/p", "6:00 a");
    n!(46023.5625, "yyyy-mm-dd hh:mm:ss", "2026-01-01 13:30:00");
    n!(0.5 + 1.25 / 86400.0, "hh:mm:ss.00", "12:00:01.25");
    n!(0.5 + 0.123 / 86400.0, "hh:mm:ss.000", "12:00:00.123");
    n!(1.5, "[h]:mm:ss", "36:00:00");
    n!(2.25, "[hh]:mm", "54:00");
    n!(0.5, "[mm]:ss", "720:00");
    n!(1.0 / 24.0, "[ss]", "3600");
    n!(-1.0, "yyyy-mm-dd", "#####");

    // 8. Chinese Dates, Weekdays & [DBNum1]/[DBNum2] Numerals (24 cases)
    n!(46023.0, "yyyy\"年\"m\"月\"d\"日\"", "2026年1月1日");
    n!(46023.0, "yyyy\"年\"mm\"月\"dd\"日\"", "2026年01月01日");
    n!(
        46340.0,
        "yyyy\"年\"m\"月\"d\"日\" aaaa",
        "2026年11月14日 星期六"
    );
    n!(46340.0, "yyyy/m/d (aaa)", "2026/11/14 (六)");
    n!(46023.0, "aaaa", "星期四");
    n!(46023.0, "aaa", "四");
    n!(1.0, "aaaa", "星期日");
    n!(60.0, "aaaa", "星期三");
    n!(46023.0, "[$-804]mmmm", "一月");
    n!(46340.0, "[$-804]mmmm", "十一月");
    n!(46023.25, "上午/下午h\"时\"mm\"分\"", "上午6时00分");
    n!(
        46023.75,
        "上午/下午h\"时\"mm\"分\"ss\"秒\"",
        "下午6时00分00秒"
    );
    n!(46023.0, "e\"年\"m\"月\"d\"日\"", "2026年1月1日");
    n!(
        46340.0,
        "[DBNum1]yyyy\"年\"m\"月\"d\"日\"",
        "二〇二六年十一月十四日"
    );
    n!(
        46023.0,
        "[DBNum1]yyyy\"年\"m\"月\"d\"日\"",
        "二〇二六年一月一日"
    );
    n!(0.0, "[DBNum1]0", "〇");
    n!(10.0, "[DBNum1]0", "十");
    n!(15.0, "[DBNum1]0", "十五");
    n!(108.0, "[DBNum1]0", "一百〇八");
    n!(12345.0, "[DBNum1]0", "一万二千三百四十五");
    n!(0.0, "[DBNum2]0", "零");
    n!(10.0, "[DBNum2]0", "壹拾");
    n!(108.0, "[DBNum2]0\"元整\"", "壹佰零捌元整");
    n!(10005.0, "[DBNum2]0\"元\"", "壹万零伍元");

    // 9. Multi-Section `;`, Text `@`, & Conditional Predicates `[>=100]` (20 cases)
    n!(50.0, ";;; ", "");
    n!(-50.0, ";;;", "");
    n!(0.0, ";;;", "");
    t!("Hidden", ";;;", "");
    t!("Alice", "@", "Alice");
    t!("Engineering", "\"Dept: \"@", "Dept: Engineering");
    t!("Q4", "[Blue]\"[\"@\"]\"", "[Q4]");
    t!("RawText", "#,##0.00", "RawText"); // Numeric-only format leaves text untouched
    t!("Wrapped", "0;-0;0;\"<\"@\">\"", "<Wrapped>");
    n!(
        150.0,
        "[>=100]\"High \"0;[>=50]\"Mid \"0;\"Low \"0",
        "High 150"
    );
    n!(
        100.0,
        "[>=100]\"High \"0;[>=50]\"Mid \"0;\"Low \"0",
        "High 100"
    );
    n!(
        75.0,
        "[>=100]\"High \"0;[>=50]\"Mid \"0;\"Low \"0",
        "Mid 75"
    );
    n!(
        12.0,
        "[>=100]\"High \"0;[>=50]\"Mid \"0;\"Low \"0",
        "Low 12"
    );
    n!(
        1500000.0,
        "[>=1000000]0.0,,\"M\";[>=1000]0.0,\"K\";0",
        "1.5M"
    );
    n!(4500.0, "[>=1000000]0.0,,\"M\";[>=1000]0.0,\"K\";0", "4.5K");
    n!(88.0, "[>=1000000]0.0,,\"M\";[>=1000]0.0,\"K\";0", "88");
    n!(0.0, "[=0]\"Zero\";[<0]\"Neg \"0;\"Pos \"0", "Zero");
    n!(-9.0, "[=0]\"Zero\";[<0]\"Neg \"0;\"Pos \"0", "Neg 9");
    n!(9.0, "[=0]\"Zero\";[<0]\"Neg \"0;\"Pos \"0", "Pos 9");
    n!(42.0, "[<>0]\"NonZero \"0;\"Zero\"", "NonZero 42");
}

#[test]
fn conditional_colors_and_engine_display_text_integration() {
    let fmt = "[Green]#,##0.00;[Red](#,##0.00);[Blue]\"-\"";
    let pos = format_excel_value(&CellValue::Number(1250.5), fmt);
    assert_eq!(pos.text, "1,250.50");
    assert_eq!(pos.color, Some(ColorRgba::rgb(0, 128, 0)));

    let neg = format_excel_value(&CellValue::Number(-1250.5), fmt);
    assert_eq!(neg.text, "(1,250.50)");
    assert_eq!(neg.color, Some(ColorRgba::rgb(255, 0, 0)));

    let zero = format_excel_value(&CellValue::Number(0.0), fmt);
    assert_eq!(zero.text, "-");
    assert_eq!(zero.color, Some(ColorRgba::rgb(0, 0, 255)));

    let col10 = format_excel_value(&CellValue::Number(7.0), "[Color10]0.0");
    assert_eq!(col10.text, "7.0");
    assert_eq!(col10.color, Some(ColorRgba::rgb(0, 128, 0)));

    // Verify `IronCalcEngine::cell_display_text` uses `format_excel_value` for Chinese dates & fractions.
    let mut engine = IronCalcEngine::new_empty("NumFmtEngine.xlsx").expect("engine");
    let a1 = CellCoord::new(0, 0);
    engine.set_cell_input(0, a1, "46340").expect("set 46340");
    engine
        .set_cell_style(
            0,
            a1,
            &CellStyle {
                number_format: "yyyy\"年\"m\"月\"d\"日\" aaaa".to_string(),
                ..CellStyle::default()
            },
        )
        .expect("set Chinese date format");
    assert_eq!(
        engine.cell_display_text(0, a1).expect("display A1"),
        "2026年11月14日 星期六"
    );
}
