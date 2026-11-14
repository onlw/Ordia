//! Font discovery, metric-compatible substitution, CJK fallback chains, and face loading for Ordia.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use fontdb::{Database, Family, Query, Style, Weight};
use thiserror::Error;

/// Errors returned by [`FontCatalog`] font discovery and loading operations.
#[derive(Debug, Error)]
pub enum TextError {
    /// No usable font face was found in the catalog.
    #[error("no usable font face found for requested family '{0}'")]
    FontNotFound(String),
    /// Failed to load raw bytes for a matched font face.
    #[error("failed to load font binary data for face '{0}'")]
    FaceDataUnavailable(String),
}

/// Resolved TrueType/OpenType face binary data ready for shaping or PDF subsetting.
#[derive(Clone, Debug)]
pub struct ResolvedFont {
    /// Matched font family name.
    pub family_name: String,
    /// Shared raw `.ttf` / `.otf` / `.ttc` bytes.
    pub bytes: Arc<Vec<u8>>,
    /// Face index inside a `.ttc` collection (`0` for single-face `.ttf`/`.otf`).
    pub face_index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CacheKey {
    family: Option<String>,
    bold: bool,
    italic: bool,
    cjk: bool,
}

/// System and bundled font catalog with Office metric-compatible substitution and CJK fallback.
pub struct FontCatalog {
    db: Database,
    cache: Mutex<HashMap<CacheKey, ResolvedFont>>,
}

impl std::fmt::Debug for FontCatalog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontCatalog")
            .field("face_count", &self.db.len())
            .finish()
    }
}

impl FontCatalog {
    /// Discovers system fonts on macOS, Windows, and Linux (including modern macOS `PingFang.ttc`
    /// in `MobileAsset` directories when present).
    pub fn from_system() -> Self {
        let mut db = Database::new();
        db.load_system_fonts();
        load_macos_pingfang_asset(&mut db);
        Self {
            db,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Creates an isolated catalog populated only with explicit font binary buffers (useful for
    /// deterministic tests).
    pub fn from_font_bytes(buffers: impl IntoIterator<Item = Vec<u8>>) -> Self {
        let mut db = Database::new();
        for buf in buffers {
            db.load_font_data(buf);
        }
        Self {
            db,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Number of indexed font faces in the catalog.
    pub fn face_count(&self) -> usize {
        self.db.len()
    }

    /// Resolves the best matching font face for `preferred_family`, weight, slant, and CJK requirement.
    ///
    /// Loaded font binaries are cached in memory by `(family, bold, italic, cjk)` so repeated
    /// `DrawText` commands on a page share a single `Arc<Vec<u8>>`.
    pub fn resolve_font(
        &self,
        preferred_family: Option<&str>,
        bold: bool,
        italic: bool,
        requires_cjk: bool,
    ) -> Result<ResolvedFont, TextError> {
        let key = CacheKey {
            family: preferred_family.map(str::to_string),
            bold,
            italic,
            cjk: requires_cjk,
        };
        if let Ok(guard) = self.cache.lock()
            && let Some(hit) = guard.get(&key)
        {
            return Ok(hit.clone());
        }

        let candidates = build_candidate_chain(preferred_family, requires_cjk);
        let weight = if bold { Weight::BOLD } else { Weight::NORMAL };
        let style = if italic { Style::Italic } else { Style::Normal };

        let mut matched_id = None;
        for name in &candidates {
            let families = [Family::Name(name.as_str())];
            let query = Query {
                families: &families,
                weight,
                style,
                ..Query::default()
            };
            if let Some(id) = self.db.query(&query) {
                matched_id = Some(id);
                break;
            }
        }

        if matched_id.is_none() {
            let generic = [Family::SansSerif, Family::Serif, Family::Monospace];
            let query = Query {
                families: &generic,
                weight,
                style,
                ..Query::default()
            };
            matched_id = self
                .db
                .query(&query)
                .or_else(|| self.db.faces().next().map(|f| f.id));
        }

        let label = preferred_family.unwrap_or("<default>").to_string();
        let id = matched_id.ok_or_else(|| TextError::FontNotFound(label.clone()))?;
        let family_name = self
            .db
            .face(id)
            .and_then(|f| f.families.first().map(|(n, _)| n.clone()))
            .unwrap_or_else(|| label.clone());

        let (bytes, face_index) = self
            .db
            .with_face_data(id, |raw, idx| (Arc::new(raw.to_vec()), idx))
            .ok_or(TextError::FaceDataUnavailable(family_name.clone()))?;

        let resolved = ResolvedFont {
            family_name,
            bytes,
            face_index,
        };

        if let Ok(mut guard) = self.cache.lock() {
            guard.insert(key, resolved.clone());
        }
        Ok(resolved)
    }
}

/// Returns `true` if `text` contains any CJK Unified Ideographs, CJK punctuation, Fullwidth forms,
/// Hiragana, Katakana, or Hangul characters that require a CJK-capable font face.
pub fn contains_cjk(text: &str) -> bool {
    text.chars().any(is_cjk_char)
}

fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x2E80..=0x2FFF
            | 0x3000..=0x30FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xAC00..=0xD7AF
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFFEF
            | 0x20000..=0x2FA1F
    )
}

/// Builds the ordered fallback family list honoring PLAN.md §5.3 metric-compatible substitutions.
pub fn build_candidate_chain(preferred: Option<&str>, requires_cjk: bool) -> Vec<String> {
    let mut chain = Vec::with_capacity(14);

    if requires_cjk {
        if let Some(pref) = preferred
            && is_known_cjk_family(pref)
        {
            chain.push(pref.to_string());
        }
        for cjk in [
            "PingFang SC",
            "Microsoft YaHei",
            "Microsoft YaHei UI",
            "Hiragino Sans GB",
            "Heiti SC",
            "STHeiti",
            "Noto Sans CJK SC",
            "Noto Sans SC",
            "Source Han Sans SC",
            "WenQuanYi Micro Hei",
            "SimHei",
            "SimSun",
            "Songti SC",
            "Arial Unicode MS",
        ] {
            if !chain
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(cjk))
            {
                chain.push(cjk.to_string());
            }
        }
    }

    if let Some(pref) = preferred {
        if !chain
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(pref))
        {
            chain.push(pref.to_string());
        }
        for sub in metric_substitutes(pref) {
            if !chain
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(sub))
            {
                chain.push((*sub).to_string());
            }
        }
    }

    for latin in [
        "Carlito",
        "Inter",
        "Helvetica Neue",
        "Segoe UI",
        "Liberation Sans",
        "Arial",
        "Helvetica",
        "DejaVu Sans",
    ] {
        if !chain
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(latin))
        {
            chain.push(latin.to_string());
        }
    }

    chain
}

fn is_known_cjk_family(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("pingfang")
        || lower.contains("yahei")
        || lower.contains("heiti")
        || lower.contains("hiragino")
        || lower.contains("song")
        || lower.contains("simsun")
        || lower.contains("simhei")
        || lower.contains("kaiti")
        || lower.contains("fangsong")
        || lower.contains("cjk")
        || lower.contains("source han")
        || name.contains('黑')
        || name.contains('宋')
        || name.contains('楷')
        || name.contains('苹')
}

fn metric_substitutes(family: &str) -> &'static [&'static str] {
    if family.eq_ignore_ascii_case("calibri") {
        &["Carlito", "Helvetica Neue", "Arial"]
    } else if family.eq_ignore_ascii_case("cambria") {
        &["Caladea", "Times New Roman", "Georgia"]
    } else if family.eq_ignore_ascii_case("arial") {
        &["Liberation Sans", "Helvetica Neue", "Helvetica"]
    } else if family.eq_ignore_ascii_case("times new roman") {
        &["Liberation Serif", "Times", "Georgia"]
    } else if family.eq_ignore_ascii_case("consolas") || family.eq_ignore_ascii_case("courier new")
    {
        &["Liberation Mono", "Menlo", "Consolas", "Courier"]
    } else if family == "微软雅黑" || family.eq_ignore_ascii_case("microsoft yahei") {
        &[
            "PingFang SC",
            "Hiragino Sans GB",
            "STHeiti",
            "Noto Sans CJK SC",
        ]
    } else if family == "宋体" || family.eq_ignore_ascii_case("simsun") {
        &["Songti SC", "PingFang SC", "STHeiti", "Noto Serif CJK SC"]
    } else {
        &[]
    }
}

fn load_macos_pingfang_asset(db: &mut Database) {
    let direct = Path::new("/System/Library/Fonts/PingFang.ttc");
    if direct.is_file() {
        let _ = db.load_font_file(direct);
        return;
    }
    for dir in [
        "/System/Library/AssetsV2/com_apple_MobileAsset_Font7",
        "/System/Library/AssetsV2/com_apple_MobileAsset_Font8",
    ] {
        let root = Path::new(dir);
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let candidate = entry.path().join("AssetData/PingFang.ttc");
            if candidate.is_file() {
                let _ = db.load_font_file(&candidate);
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_cjk_ideographs_and_fullwidth_punctuation() {
        assert!(contains_cjk("2026年预算汇总"));
        assert!(contains_cjk("￥12,800"));
        assert!(!contains_cjk("Q1 Revenue $12,800.00"));
    }

    #[test]
    fn calibri_substitution_prioritizes_carlito() {
        let chain = build_candidate_chain(Some("Calibri"), false);
        assert_eq!(chain[0], "Calibri");
        assert_eq!(chain[1], "Carlito");
    }

    #[test]
    fn system_catalog_resolves_latin_and_cjk_faces() {
        let catalog = FontCatalog::from_system();
        assert!(catalog.face_count() > 0);

        let latin = catalog
            .resolve_font(Some("Calibri"), false, false, false)
            .expect("resolve Latin font");
        assert!(!latin.bytes.is_empty());

        let cjk = catalog
            .resolve_font(Some("Calibri"), true, false, true)
            .expect("resolve CJK fallback font");
        assert!(!cjk.bytes.is_empty());
    }
}
