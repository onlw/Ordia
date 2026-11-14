//! Internationalization and localization catalog for Ordia (`zh-CN` and `en`).
//!
//! Embeds both `locales/zh-CN.toml` and `locales/en.toml` at compile time, detects the host
//! system language on startup, supports zero-restart runtime switching (`M1-04`, `M1-05`),
//! and exposes zero-allocation [`tr`] lookups plus `{placeholder}` interpolation via [`t!`].

use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

const EN_TOML: &str = include_str!("../locales/en.toml");
const ZH_CN_TOML: &str = include_str!("../locales/zh-CN.toml");

const LOCALE_UNSET: u8 = 0;
const LOCALE_EN: u8 = 1;
const LOCALE_ZH_CN: u8 = 2;

static ACTIVE_LOCALE: AtomicU8 = AtomicU8::new(LOCALE_UNSET);

/// Supported UI display locales in Ordia (`PLAN.md` §5, `AGENTS.md` §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Locale {
    /// Simplified Chinese (`zh-CN`).
    ZhCn,
    /// English (`en`).
    En,
}

impl Locale {
    /// BCP-47 language tag (`"zh-CN"` or `"en"`).
    pub const fn code(self) -> &'static str {
        match self {
            Self::ZhCn => "zh-CN",
            Self::En => "en",
        }
    }

    /// Parses a BCP-47 / POSIX locale tag prefix (`"zh*"` -> [`Self::ZhCn`], `"en*"` -> [`Self::En`]).
    pub fn from_tag(tag: &str) -> Option<Self> {
        let lower = tag.trim().to_ascii_lowercase();
        if lower.starts_with("zh") {
            Some(Self::ZhCn)
        } else if lower.starts_with("en") {
            Some(Self::En)
        } else {
            None
        }
    }

    const fn to_u8(self) -> u8 {
        match self {
            Self::En => LOCALE_EN,
            Self::ZhCn => LOCALE_ZH_CN,
        }
    }
}

/// User-configurable language preference (`System`, `zh-CN`, or `en`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LanguagePreference {
    /// Follow the operating system's locale (`default`).
    #[default]
    System,
    /// Pin interface language to Simplified Chinese (`zh-CN`).
    ZhCn,
    /// Pin interface language to English (`en`).
    En,
}

impl LanguagePreference {
    /// Resolves this preference into a concrete [`Locale`].
    pub fn resolve(self) -> Locale {
        match self {
            Self::System => detect_system_locale(),
            Self::ZhCn => Locale::ZhCn,
            Self::En => Locale::En,
        }
    }
}

/// Errors produced when parsing or validating TOML locale catalogs.
#[derive(Debug, thiserror::Error)]
pub enum I18nError {
    /// TOML syntax error in a locale file.
    #[error("failed to parse locale TOML ({locale}): {source}")]
    TomlParse {
        /// Locale identifier being parsed.
        locale: &'static str,
        /// Underlying TOML parser error.
        source: toml::de::Error,
    },
    /// Non-string leaf value encountered in a locale table.
    #[error("locale `{locale}` key `{key}` must be a string")]
    NonStringLeaf {
        /// Locale identifier.
        locale: &'static str,
        /// Dotted translation key.
        key: String,
    },
}

struct Catalog {
    en: BTreeMap<String, &'static str>,
    zh_cn: BTreeMap<String, &'static str>,
}

fn flatten_table(
    locale: &'static str,
    prefix: &str,
    table: &toml::Table,
    out: &mut BTreeMap<String, &'static str>,
) -> Result<(), I18nError> {
    for (k, v) in table {
        let full_key = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        match v {
            toml::Value::Table(sub) => flatten_table(locale, &full_key, sub, out)?,
            toml::Value::String(s) => {
                let leaked: &'static str = Box::leak(s.clone().into_boxed_str());
                out.insert(full_key, leaked);
            }
            _ => {
                return Err(I18nError::NonStringLeaf {
                    locale,
                    key: full_key,
                });
            }
        }
    }
    Ok(())
}

fn parse_catalog_str(
    locale: &'static str,
    raw: &str,
) -> Result<BTreeMap<String, &'static str>, I18nError> {
    let table: toml::Table =
        toml::from_str(raw).map_err(|source| I18nError::TomlParse { locale, source })?;
    let mut map = BTreeMap::new();
    flatten_table(locale, "", &table, &mut map)?;
    Ok(map)
}

fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let en = parse_catalog_str("en", EN_TOML)
            .expect("embedded crates/ordia-i18n/locales/en.toml must be valid TOML");
        let zh_cn = parse_catalog_str("zh-CN", ZH_CN_TOML)
            .expect("embedded crates/ordia-i18n/locales/zh-CN.toml must be valid TOML");
        Catalog { en, zh_cn }
    })
}

/// Detects the preferred [`Locale`] from environment variables (`ORDIA_LOCALE`, `LC_ALL`,
/// `LC_MESSAGES`, `LANG`), defaulting to [`Locale::En`] when unset or unrecognized.
pub fn detect_system_locale() -> Locale {
    for var in ["ORDIA_LOCALE", "LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(val) = std::env::var(var)
            && let Some(loc) = Locale::from_tag(&val)
        {
            return loc;
        }
    }
    Locale::En
}

/// Returns the currently active UI [`Locale`], initializing from [`detect_system_locale`] on first read.
pub fn current_locale() -> Locale {
    match ACTIVE_LOCALE.load(Ordering::Relaxed) {
        LOCALE_EN => Locale::En,
        LOCALE_ZH_CN => Locale::ZhCn,
        _ => {
            let detected = detect_system_locale();
            let _ = ACTIVE_LOCALE.compare_exchange(
                LOCALE_UNSET,
                detected.to_u8(),
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
            match ACTIVE_LOCALE.load(Ordering::Relaxed) {
                LOCALE_ZH_CN => Locale::ZhCn,
                _ => Locale::En,
            }
        }
    }
}

/// Switches the active UI [`Locale`] at runtime (`zh-CN` or `en`).
pub fn set_locale(locale: Locale) {
    ACTIVE_LOCALE.store(locale.to_u8(), Ordering::Relaxed);
}

/// Applies a [`LanguagePreference`] (`System`, `ZhCn`, or `En`) and returns the resolved [`Locale`].
pub fn set_language_preference(pref: LanguagePreference) -> Locale {
    let resolved = pref.resolve();
    set_locale(resolved);
    resolved
}

/// Looks up `key` in the explicit `locale` catalog.
pub fn tr_in(locale: Locale, key: &str) -> Option<&'static str> {
    let cat = catalog();
    match locale {
        Locale::En => cat.en.get(key).copied(),
        Locale::ZhCn => cat.zh_cn.get(key).copied(),
    }
}

/// Translates a static dotted key in the currently active [`Locale`].
///
/// Falls back to English and then `key` itself in release builds so a missing key never crashes
/// the user's session; asserts key existence in debug/test builds.
pub fn tr(key: &'static str) -> &'static str {
    let loc = current_locale();
    if let Some(val) = tr_in(loc, key) {
        return val;
    }
    debug_assert!(
        false,
        "missing ordia-i18n translation key `{key}` for locale `{}`",
        loc.code()
    );
    tr_in(Locale::En, key).unwrap_or(key)
}

/// Translates `key` in the active [`Locale`] and substitutes `{name}` placeholders from `args`.
pub fn tr_args(key: &'static str, args: &[(&str, &str)]) -> String {
    let template = tr(key);
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after_open = &rest[open + 1..];
        if let Some(close) = after_open.find('}') {
            let token = &after_open[..close];
            if let Some((_, val)) = args.iter().find(|(k, _)| *k == token) {
                out.push_str(val);
            } else {
                out.push('{');
                out.push_str(token);
                out.push('}');
            }
            rest = &after_open[close + 1..];
        } else {
            out.push('{');
            rest = after_open;
            break;
        }
    }
    out.push_str(rest);
    out
}

/// Translates a dotted localization key, optionally interpolating `"param" => value` pairs.
#[macro_export]
macro_rules! t {
    ($key:literal) => {
        $crate::tr($key)
    };
    ($key:literal $(, $param:literal => $val:expr)+ $(,)?) => {
        $crate::tr_args($key, &[ $( ($param, ::std::convert::AsRef::<str>::as_ref(&$val)) ),+ ])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalogs_have_identical_keys_and_non_empty_strings() {
        let cat = catalog();
        let en_keys: Vec<_> = cat.en.keys().collect();
        let zh_keys: Vec<_> = cat.zh_cn.keys().collect();
        assert_eq!(
            en_keys, zh_keys,
            "en.toml and zh-CN.toml key sets must match"
        );
        for (k, v) in &cat.en {
            assert!(!v.trim().is_empty(), "empty en translation for {k}");
        }
        for (k, v) in &cat.zh_cn {
            assert!(!v.trim().is_empty(), "empty zh-CN translation for {k}");
        }
    }

    #[test]
    fn runtime_locale_switching_and_placeholder_interpolation_work() {
        let prev = current_locale();

        set_locale(Locale::ZhCn);
        assert_eq!(t!("cmd.file.save"), "保存");
        assert_eq!(t!("status.recent_count", "count" => "3"), "3 个最近工作簿");

        set_locale(Locale::En);
        assert_eq!(t!("cmd.file.save"), "Save");
        assert_eq!(
            t!("status.recent_count", "count" => "3"),
            "3 recent workbooks"
        );

        set_locale(prev);
    }
}
