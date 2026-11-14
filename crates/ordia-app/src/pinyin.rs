//! Fuzzy subsequence matcher and Chinese Hanzi Pinyin-initials matcher (`M1-03`).
//!
//! Uses the GB2312 Level-1 Hanzi block (`0xB0A1..=0xD7F9` via `encoding_rs::GBK`, which orders
//! all 3,755 common Simplified Chinese characters alphabetically by Pinyin) plus polyphone
//! overrides for UI terms like `重做` (`cz`/`zz`), `行高` (`hg`/`xg`), and `调整` (`tz`/`dz`).

use encoding_rs::GBK;

/// GB2312 Level-1 Pinyin initial boundary table (`0xB0A1..=0xD7F9`).
const GB2312_PINYIN_BOUNDARIES: &[(u16, char)] = &[
    (0xB0A1, 'a'),
    (0xB0C5, 'b'),
    (0xB2C1, 'c'),
    (0xB4EE, 'd'),
    (0xB6EA, 'e'),
    (0xB7A2, 'f'),
    (0xB8C1, 'g'),
    (0xB9FE, 'h'),
    (0xBBF7, 'j'),
    (0xBFA6, 'k'),
    (0xC0AC, 'l'),
    (0xC2E8, 'm'),
    (0xC4C3, 'n'),
    (0xC5B6, 'o'),
    (0xC5BE, 'p'),
    (0xC6DA, 'q'),
    (0xC8BB, 'r'),
    (0xC8F6, 's'),
    (0xCBFA, 't'),
    (0xCDDA, 'w'),
    (0xCEF4, 'x'),
    (0xD1B9, 'y'),
    (0xD4D1, 'z'),
];

/// Returns candidate lowercase ASCII Pinyin initial letters for a character `ch`.
pub fn char_pinyin_initials(ch: char) -> &'static [char] {
    if ch.is_ascii_alphanumeric() {
        return &[];
    }
    // Common UI polyphones (`重做` chong/zhong, `行高` hang/xing, `调整` tiao/diao).
    match ch {
        '重' => return &['c', 'z'],
        '行' => return &['h', 'x'],
        '调' => return &['t', 'd'],
        '长' => return &['c', 'z'],
        '藏' => return &['c', 'z'],
        _ => {}
    }

    let mut utf8_buf = [0u8; 4];
    let s = ch.encode_utf8(&mut utf8_buf);
    let (cow, _, had_errors) = GBK.encode(s);
    if had_errors || cow.len() != 2 {
        return &[];
    }
    let code = (u16::from(cow[0]) << 8) | u16::from(cow[1]);
    if !(0xB0A1..=0xD7F9).contains(&code) {
        return &[];
    }

    let mut matched = 'a';
    for &(start, letter) in GB2312_PINYIN_BOUNDARIES {
        if code >= start {
            matched = letter;
        } else {
            break;
        }
    }
    static LETTERS: [char; 26] = [
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r',
        's', 't', 'u', 'v', 'w', 'x', 'y', 'z',
    ];
    let ix = (matched as usize).saturating_sub('a' as usize);
    &LETTERS[ix..=ix]
}

/// Extracts the primary lowercase Pinyin initial string for `text` (keeping ASCII alphanumerics).
pub fn pinyin_initials_string(text: &str) -> String {
    let mut out = String::with_capacity(text.chars().count());
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if let Some(&initial) = char_pinyin_initials(ch).first() {
            out.push(initial);
        }
    }
    out
}

/// Scores `query` against `candidate` using exact/prefix/substring, fuzzy subsequence,
/// and polyphone-aware Chinese Pinyin initials. Higher score means better match (`None` = no match).
pub fn fuzzy_or_pinyin_score(query: &str, candidate: &str) -> Option<u32> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some(1);
    }
    let cand_lower = candidate.to_lowercase();

    // 1. Direct literal match (exact > prefix > substring).
    if cand_lower == q {
        return Some(1_000);
    }
    if cand_lower.starts_with(&q) {
        return Some(850);
    }
    if cand_lower.contains(&q) {
        return Some(700);
    }

    // 2. Polyphone-aware Pinyin initials match (for ASCII queries against CJK text).
    if q.is_ascii()
        && let Some(py_score) = score_pinyin_initials(q.as_bytes(), candidate)
    {
        return Some(py_score);
    }

    // 3. Fuzzy ordered subsequence match with word-boundary / consecutive bonus.
    score_subsequence(&q, &cand_lower)
}

fn score_pinyin_initials(q_bytes: &[u8], candidate: &str) -> Option<u32> {
    let chars: Vec<char> = candidate.chars().collect();
    if chars.is_empty() {
        return None;
    }

    fn dfs(q: &[u8], chars: &[char], qi: usize, ci: usize, consecutive: u32) -> Option<u32> {
        if qi == q.len() {
            return Some(500 + consecutive * 25);
        }
        if ci >= chars.len() {
            return None;
        }
        let target = q[qi] as char;
        let ch = chars[ci];
        let matches_here = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase() == target
        } else {
            char_pinyin_initials(ch).contains(&target)
        };

        let take = if matches_here {
            dfs(q, chars, qi + 1, ci + 1, consecutive + 1)
                .map(|s| if ci == 0 && qi == 0 { s + 120 } else { s })
        } else {
            None
        };
        let skip = dfs(q, chars, qi, ci + 1, 0);
        match (take, skip) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    }

    dfs(q_bytes, &chars, 0, 0, 0)
}

fn score_subsequence(q: &str, cand_lower: &str) -> Option<u32> {
    let q_chars: Vec<char> = q.chars().collect();
    let mut qi = 0usize;
    let mut bonus = 0u32;
    let mut prev_matched = false;
    let mut prev_sep = true;

    for ch in cand_lower.chars() {
        if qi < q_chars.len() && ch == q_chars[qi] {
            qi += 1;
            if prev_matched {
                bonus += 20;
            }
            if prev_sep {
                bonus += 25;
            }
            prev_matched = true;
        } else {
            prev_matched = false;
        }
        prev_sep = matches!(ch, ' ' | '_' | '-' | '.' | '/');
    }

    (qi == q_chars.len()).then_some(300 + bonus)
}
