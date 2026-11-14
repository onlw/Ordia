//! Localization resource parity and hardcoded UI text scanner (`cargo xtask i18n-check`, `M1-04`).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

const UI_CRATE_SRC_DIRS: &[&str] = &[
    "crates/ordia-app/src",
    "crates/ordia-ui/src",
    "crates/ordia-sheet-view/src",
];

const FORBIDDEN_LITERAL_CALLS: &[&str] = &[
    ".label(\"",
    ".tooltip(\"",
    ".placeholder(\"",
    ".accessibility_label(\"",
    "SidebarGroup::new(\"",
    "SidebarMenuItem::new(\"",
    "MenuItem::action(\"",
];

/// Runs the full `i18n-check` pipeline on `root`.
pub fn run(root: &Path) -> Result<()> {
    let locales_dir = root.join("crates/ordia-i18n/locales");
    let zh_path = locales_dir.join("zh-CN.toml");
    let en_path = locales_dir.join("en.toml");
    if !zh_path.exists() || !en_path.exists() {
        bail!(
            "missing locale files in {}: expected both zh-CN.toml and en.toml",
            locales_dir.display()
        );
    }

    let zh_raw = fs::read_to_string(&zh_path)
        .with_context(|| format!("failed to read {}", zh_path.display()))?;
    let en_raw = fs::read_to_string(&en_path)
        .with_context(|| format!("failed to read {}", en_path.display()))?;

    let zh_map = flatten_toml("zh-CN", &zh_raw)?;
    let en_map = flatten_toml("en", &en_raw)?;

    let mut violations = validate_catalog_parity(&zh_map, &en_map);

    let known_keys: BTreeSet<String> = en_map.keys().cloned().collect();
    for rel_dir in UI_CRATE_SRC_DIRS {
        let dir = root.join(rel_dir);
        if !dir.exists() {
            continue;
        }
        let mut rs_files = Vec::new();
        collect_rs_files(&dir, &mut rs_files)?;
        for file in rs_files {
            let src = fs::read_to_string(&file)
                .with_context(|| format!("failed to read {}", file.display()))?;
            let display_path = file
                .strip_prefix(root)
                .unwrap_or(&file)
                .to_string_lossy()
                .replace('\\', "/");
            violations.extend(scan_rust_ui_source(&display_path, &src, &known_keys));
        }
    }

    if !violations.is_empty() {
        let mut msg = String::from("i18n-check violations detected:\n");
        for v in &violations {
            msg.push_str("  - ");
            msg.push_str(v);
            msg.push('\n');
        }
        bail!("{msg}");
    }

    println!(
        "i18n-check ok: {} translation keys verified across zh-CN/en and UI crates.",
        known_keys.len()
    );
    Ok(())
}

pub(crate) fn flatten_toml(locale: &str, raw: &str) -> Result<BTreeMap<String, String>> {
    let table: toml::Table =
        toml::from_str(raw).with_context(|| format!("failed to parse {locale}.toml"))?;
    let mut out = BTreeMap::new();
    flatten_inner(locale, "", &table, &mut out)?;
    Ok(out)
}

fn flatten_inner(
    locale: &str,
    prefix: &str,
    table: &toml::Table,
    out: &mut BTreeMap<String, String>,
) -> Result<()> {
    for (k, v) in table {
        let key = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        match v {
            toml::Value::Table(sub) => flatten_inner(locale, &key, sub, out)?,
            toml::Value::String(s) => {
                out.insert(key, s.clone());
            }
            _ => bail!("locale `{locale}` key `{key}` must be a string"),
        }
    }
    Ok(())
}

fn placeholders(s: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    let mut rest = s;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        if let Some(close) = after.find('}') {
            set.insert(after[..close].to_string());
            rest = &after[close + 1..];
        } else {
            break;
        }
    }
    set
}

fn contains_cjk(s: &str) -> bool {
    s.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c))
}

pub(crate) fn validate_catalog_parity(
    zh: &BTreeMap<String, String>,
    en: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut errors = Vec::new();
    for k in zh.keys() {
        if !en.contains_key(k) {
            errors.push(format!(
                "key `{k}` exists in zh-CN.toml but is missing in en.toml"
            ));
        }
    }
    for k in en.keys() {
        if !zh.contains_key(k) {
            errors.push(format!(
                "key `{k}` exists in en.toml but is missing in zh-CN.toml"
            ));
        }
    }
    for (k, en_val) in en {
        if en_val.trim().is_empty() {
            errors.push(format!("en.toml key `{k}` has an empty translation"));
        }
        if contains_cjk(en_val) {
            errors.push(format!(
                "en.toml key `{k}` contains unexpected CJK characters: `{en_val}`"
            ));
        }
        if let Some(zh_val) = zh.get(k) {
            if zh_val.trim().is_empty() {
                errors.push(format!("zh-CN.toml key `{k}` has an empty translation"));
            }
            let en_ph = placeholders(en_val);
            let zh_ph = placeholders(zh_val);
            if en_ph != zh_ph {
                errors.push(format!(
                    "placeholder mismatch for `{k}`: en={en_ph:?}, zh-CN={zh_ph:?}"
                ));
            }
        }
    }
    errors
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    out.sort();
    Ok(())
}

/// Strips `#[cfg(test)]` tail modules and `//` line comments before scanning UI Rust code.
fn strip_test_module_and_comments(src: &str) -> String {
    let prod_slice = src.split("#[cfg(test)]").next().unwrap_or(src);
    let mut cleaned = String::with_capacity(prod_slice.len());
    for line in prod_slice.lines() {
        let code_part = match line.find("//") {
            Some(idx) => &line[..idx],
            None => line,
        };
        cleaned.push_str(code_part);
        cleaned.push('\n');
    }
    cleaned
}

pub(crate) fn scan_rust_ui_source(
    file_label: &str,
    src: &str,
    known_keys: &BTreeSet<String>,
) -> Vec<String> {
    let mut errors = Vec::new();
    let cleaned = strip_test_module_and_comments(src);
    let compact: String = cleaned.chars().filter(|c| !c.is_whitespace()).collect();

    for pat in FORBIDDEN_LITERAL_CALLS {
        if compact.contains(pat) {
            errors.push(format!(
                "{file_label}: hardcoded UI string literal in `{pat}...` (must use `ordia_i18n::t!`)"
            ));
        }
    }

    for (line_no, line) in cleaned.lines().enumerate() {
        // Check string literals on this line for hardcoded CJK ideographs.
        let mut rest = line;
        while let Some(q1) = rest.find('"') {
            let after_q1 = &rest[q1 + 1..];
            let mut end_idx = None;
            let mut escaped = false;
            for (i, ch) in after_q1.char_indices() {
                if escaped {
                    escaped = false;
                    continue;
                }
                if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    end_idx = Some(i);
                    break;
                }
            }
            if let Some(q2) = end_idx {
                let lit = &after_q1[..q2];
                if contains_cjk(lit) {
                    errors.push(format!(
                        "{file_label}:{}: hardcoded CJK string literal `\"{lit}\"` (move to locales/*.toml)",
                        line_no + 1
                    ));
                }
                rest = &after_q1[q2 + 1..];
            } else {
                break;
            }
        }
    }

    // Verify every `ordia_i18n::t!("key"` / standalone `t!("key"` and `i18n_key:"key"` exists in the catalog!
    let bytes = compact.as_bytes();
    for prefix in ["t!(\"", "i18n_key:\""] {
        let mut offset = 0usize;
        while let Some(rel) = compact[offset..].find(prefix) {
            let abs = offset + rel;
            let is_ident_continuation =
                abs > 0 && (bytes[abs - 1].is_ascii_alphanumeric() || bytes[abs - 1] == b'_');
            let after_start = abs + prefix.len();
            if is_ident_continuation {
                offset = after_start;
                continue;
            }
            let after = &compact[after_start..];
            if let Some(end_q) = after.find('"') {
                let key = &after[..end_q];
                if !known_keys.contains(key) {
                    errors.push(format!("{file_label}: references unknown i18n key `{key}`"));
                }
                offset = after_start + end_q + 1;
            } else {
                break;
            }
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catches_missing_keys_and_placeholder_mismatches() {
        let zh = flatten_toml(
            "zh-CN",
            "[cmd]\nsave = \"保存\"\ncount = \"{n} 个\"\nonly_zh = \"独有\"\n",
        )
        .expect("parse test zh-CN TOML");
        let en = flatten_toml("en", "[cmd]\nsave = \"Save\"\ncount = \"{count} items\"\n")
            .expect("parse test en TOML");
        let errs = validate_catalog_parity(&zh, &en);
        assert!(errs.iter().any(|e| e.contains("only_zh")));
        assert!(errs.iter().any(|e| e.contains("placeholder mismatch")));
    }

    #[test]
    fn catches_hardcoded_ui_literals_and_unknown_keys() {
        let mut keys = BTreeSet::new();
        keys.insert("cmd.file.save".to_string());

        let bad_code = r#"
            fn view() {
                Button::new("save").label("Save");
                let x = "硬编码中文";
                let y = ordia_i18n::t!("missing.key");
            }
        "#;
        let errs = scan_rust_ui_source("fake_view.rs", bad_code, &keys);
        assert_eq!(errs.len(), 3, "expected 3 violations, got: {errs:?}");
    }
}
