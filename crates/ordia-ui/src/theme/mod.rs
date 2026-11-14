//! Ordia design tokens (colors, radii, spacing, elevation shadows, typography, motion)
//! and Light / Dark theme adapters mapped onto GPUI Kit's [`Theme`].

use std::time::Duration;

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, BoxShadow, Hsla, Pixels, SharedString, Window, point, px, rgba};

/// Packs `0xRRGGBB` into an opaque [`Hsla`] value.
pub fn hex_rgb(hex: u32) -> Hsla {
    Hsla::from(rgba((hex << 8) | 0xFF))
}

/// Packs `0xRRGGBB` with `alpha` (`0..=255`) into [`Hsla`].
pub fn hex_rgba(hex: u32, alpha: u8) -> Hsla {
    Hsla::from(rgba((hex << 8) | u32::from(alpha)))
}

/// Corner-radius scale tokens (`PLAN.md` §5.2).
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RadiusScale {
    /// Sharp cell / hairline radius (`0px`).
    pub none: Pixels,
    /// Subtle badge / kbd radius (`4px`).
    pub xs: Pixels,
    /// Compact toolbar control / input radius (`6px`).
    pub sm: Pixels,
    /// Standard popover / card radius (`8px`).
    pub md: Pixels,
    /// Dialog / floating palette radius (`12px`).
    pub lg: Pixels,
    /// Pill / circular swatch radius (`9999px`).
    pub full: Pixels,
}

impl Default for RadiusScale {
    fn default() -> Self {
        Self {
            none: px(0.0),
            xs: px(4.0),
            sm: px(6.0),
            md: px(8.0),
            lg: px(12.0),
            full: px(9999.0),
        }
    }
}

/// 4px-grid spacing tokens (`PLAN.md` §5.2 — compact professional density with generous hit targets).
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpacingScale {
    /// `2px` hairline / inline icon gap.
    pub xxs: Pixels,
    /// `4px` tight toolbar button cluster gap.
    pub xs: Pixels,
    /// `8px` toolbar group gap / compact padding.
    pub sm: Pixels,
    /// `12px` panel row padding.
    pub md: Pixels,
    /// `16px` section padding.
    pub lg: Pixels,
    /// `24px` dialog / gallery card padding.
    pub xl: Pixels,
}

impl Default for SpacingScale {
    fn default() -> Self {
        Self {
            xxs: px(2.0),
            xs: px(4.0),
            sm: px(8.0),
            md: px(12.0),
            lg: px(16.0),
            xl: px(24.0),
        }
    }
}

/// Typography scale tokens (`PLAN.md` §5.2).
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub struct TypographyScale {
    /// UI sans-serif font family (`".SystemUIFont"` on macOS, `"Segoe UI"` on Windows).
    pub ui_sans: SharedString,
    /// Monospace font family for formulas, cell references, and diagnostics.
    pub ui_mono: SharedString,
    /// `11px` status bar / row-column header text.
    pub caption: Pixels,
    /// `12px` compact toolbar / inspector label.
    pub sm: Pixels,
    /// `13px` primary desktop UI body size.
    pub base: Pixels,
    /// `15px` dialog / panel section heading.
    pub md: Pixels,
    /// `18px` page / home title.
    pub lg: Pixels,
    /// `22px` hero display title.
    pub xl: Pixels,
}

impl Default for TypographyScale {
    fn default() -> Self {
        #[cfg(target_os = "windows")]
        let (sans, mono) = ("Segoe UI", "Consolas");
        #[cfg(target_os = "macos")]
        let (sans, mono) = (".SystemUIFont", "SF Mono");
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let (sans, mono) = ("Inter", "JetBrains Mono");

        Self {
            ui_sans: SharedString::from(sans),
            ui_mono: SharedString::from(mono),
            caption: px(11.0),
            sm: px(12.0),
            base: px(13.0),
            md: px(15.0),
            lg: px(18.0),
            xl: px(22.0),
        }
    }
}

/// Elevation shadow tokens (`PLAN.md` §5.2).
#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct ElevationScale {
    /// Subtle raised card / segmented thumb shadow.
    pub sm: Vec<BoxShadow>,
    /// Popover / dropdown menu elevation.
    pub md: Vec<BoxShadow>,
    /// Modal dialog / command palette elevation.
    pub lg: Vec<BoxShadow>,
}

impl ElevationScale {
    /// Builds shadow tokens calibrated for `is_dark` surfaces.
    pub fn for_mode(is_dark: bool) -> Self {
        let (a_sm, a_md, a_lg) = if is_dark {
            (72, 110, 148)
        } else {
            (16, 28, 42)
        };
        Self {
            sm: vec![BoxShadow {
                color: hex_rgba(0x09090B, a_sm),
                offset: point(px(0.0), px(1.0)),
                blur_radius: px(2.0),
                spread_radius: px(0.0),
                inset: false,
            }],
            md: vec![BoxShadow {
                color: hex_rgba(0x09090B, a_md),
                offset: point(px(0.0), px(4.0)),
                blur_radius: px(12.0),
                spread_radius: px(-2.0),
                inset: false,
            }],
            lg: vec![BoxShadow {
                color: hex_rgba(0x09090B, a_lg),
                offset: point(px(0.0), px(12.0)),
                blur_radius: px(28.0),
                spread_radius: px(-4.0),
                inset: false,
            }],
        }
    }
}

/// Motion timing tokens (`PLAN.md` §5.2: `120–200ms` snappy transitions).
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionScale {
    /// Fast hover / selection feedback (`120ms`).
    pub fast: Duration,
    /// Standard panel / tab / popover transition (`160ms`).
    pub normal: Duration,
    /// Gentle sheet / dialog entrance (`200ms`).
    pub gentle: Duration,
}

impl Default for MotionScale {
    fn default() -> Self {
        Self {
            fast: Duration::from_millis(120),
            normal: Duration::from_millis(160),
            gentle: Duration::from_millis(200),
        }
    }
}

/// Semantic color tokens for UI chrome and spreadsheet workspace.
///
/// Note (`PLAN.md` §5.2): `sheet_paper_bg` stays paper-white (`#FFFFFF`) even in dark mode so
/// authored spreadsheet cell fills and font colors remain 100% faithful (matching Apple Numbers).
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrdiaColorTokens {
    /// Window canvas / shell background.
    pub background: Hsla,
    /// Elevated surface (toolbar, inspector, popover).
    pub surface: Hsla,
    /// Subtle recessed surface (tab bar well, status bar, formula bar prefix).
    pub surface_muted: Hsla,
    /// Primary text color.
    pub foreground: Hsla,
    /// Secondary / muted caption color.
    pub muted_foreground: Hsla,
    /// Brand emerald-teal accent (`#0D9488` light / `#14B8A6` dark).
    pub primary: Hsla,
    /// Text color on `primary`.
    pub primary_foreground: Hsla,
    /// Hover / subtle active highlight background.
    pub accent_subtle: Hsla,
    /// Text color on `accent_subtle`.
    pub accent_foreground: Hsla,
    /// Hairline border color.
    pub border: Hsla,
    /// Input control border color.
    pub input_border: Hsla,
    /// Focus ring color.
    pub focus_ring: Hsla,
    /// Danger / destructive action color.
    pub danger: Hsla,
    /// Warning / preservation-hazard banner color.
    pub warning: Hsla,
    /// Success / recalculated badge color.
    pub success: Hsla,
    /// Spreadsheet document paper background (always `#FFFFFF` per `PLAN.md` §5.2).
    pub sheet_paper_bg: Hsla,
    /// Active cell selection frame color.
    pub selection_border: Hsla,
}

/// Complete Ordia design token bundle for a [`ThemeMode`].
#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct OrdiaTokens {
    /// Active theme mode (`Light` or `Dark`).
    pub mode: ThemeMode,
    /// Semantic color tokens.
    pub colors: OrdiaColorTokens,
    /// Corner radii.
    pub radius: RadiusScale,
    /// 4px spacing scale.
    pub spacing: SpacingScale,
    /// Font families and sizes.
    pub typography: TypographyScale,
    /// Elevation box shadows.
    pub elevation: ElevationScale,
    /// Transition durations.
    pub motion: MotionScale,
}

impl OrdiaTokens {
    /// Curated Light mode token set (clean Linear / Numbers zinc-and-emerald aesthetic).
    pub fn light() -> Self {
        Self {
            mode: ThemeMode::Light,
            colors: OrdiaColorTokens {
                background: hex_rgb(0xFAFAFB),
                surface: hex_rgb(0xFFFFFF),
                surface_muted: hex_rgb(0xF4F4F6),
                foreground: hex_rgb(0x18181B),
                muted_foreground: hex_rgb(0x71717A),
                primary: hex_rgb(0x0F766E),
                primary_foreground: hex_rgb(0xFFFFFF),
                accent_subtle: hex_rgb(0xCCFBF1),
                accent_foreground: hex_rgb(0x115E59),
                border: hex_rgb(0xE4E4E7),
                input_border: hex_rgb(0xD4D4D8),
                focus_ring: hex_rgba(0x0F766E, 96),
                danger: hex_rgb(0xDC2626),
                warning: hex_rgb(0xD97706),
                success: hex_rgb(0x16A34A),
                sheet_paper_bg: hex_rgb(0xFFFFFF),
                selection_border: hex_rgb(0x0F766E),
            },
            radius: RadiusScale::default(),
            spacing: SpacingScale::default(),
            typography: TypographyScale::default(),
            elevation: ElevationScale::for_mode(false),
            motion: MotionScale::default(),
        }
    }

    /// Curated Dark mode token set (Zed / Linear deep neutral slate chrome + invariant paper canvas).
    pub fn dark() -> Self {
        Self {
            mode: ThemeMode::Dark,
            colors: OrdiaColorTokens {
                background: hex_rgb(0x121316),
                surface: hex_rgb(0x1A1C20),
                surface_muted: hex_rgb(0x22252B),
                foreground: hex_rgb(0xF4F4F5),
                muted_foreground: hex_rgb(0xA1A1AA),
                primary: hex_rgb(0x14B8A6),
                primary_foreground: hex_rgb(0x042F2E),
                accent_subtle: hex_rgba(0x14B8A6, 44),
                accent_foreground: hex_rgb(0x5EEAD4),
                border: hex_rgb(0x2C3038),
                input_border: hex_rgb(0x383D47),
                focus_ring: hex_rgba(0x14B8A6, 110),
                danger: hex_rgb(0xF87171),
                warning: hex_rgb(0xFBBF24),
                success: hex_rgb(0x4ADE80),
                sheet_paper_bg: hex_rgb(0xFFFFFF),
                selection_border: hex_rgb(0x14B8A6),
            },
            radius: RadiusScale::default(),
            spacing: SpacingScale::default(),
            typography: TypographyScale::default(),
            elevation: ElevationScale::for_mode(true),
            motion: MotionScale::default(),
        }
    }

    /// Returns the token bundle matching `mode`.
    pub fn for_mode(mode: ThemeMode) -> Self {
        if mode.is_dark() {
            Self::dark()
        } else {
            Self::light()
        }
    }
}

/// Applies Ordia's semantic design tokens for `mode` onto GPUI Kit's global [`Theme`].
pub fn apply_ordia_theme(mode: ThemeMode, window: Option<&mut Window>, cx: &mut App) {
    Theme::change(mode, window, cx);
    let spec = OrdiaTokens::for_mode(mode);
    Theme::update(cx, |theme| {
        let mut sem = theme.semantic_tokens();
        sem.colors.background = spec.colors.background;
        sem.colors.foreground = spec.colors.foreground;
        sem.colors.surface = spec.colors.surface;
        sem.colors.surface_foreground = spec.colors.foreground;
        sem.colors.primary = spec.colors.primary;
        sem.colors.primary_foreground = spec.colors.primary_foreground;
        sem.colors.secondary = spec.colors.surface_muted;
        sem.colors.secondary_foreground = spec.colors.foreground;
        sem.colors.muted = spec.colors.surface_muted;
        sem.colors.muted_foreground = spec.colors.muted_foreground;
        sem.colors.accent = spec.colors.accent_subtle;
        sem.colors.accent_foreground = spec.colors.accent_foreground;
        sem.colors.destructive = spec.colors.danger;
        sem.colors.border = spec.colors.border;
        sem.colors.input = spec.colors.input_border;
        sem.colors.ring = spec.colors.focus_ring;
        sem.radius.sm = spec.radius.sm;
        sem.radius.md = spec.radius.sm;
        sem.radius.lg = spec.radius.lg;
        sem.typography.md.size = spec.typography.base;
        theme.apply_semantic_tokens(&sem);
    });
}

/// Default spreadsheet document color palette (theme-invariant sRGB swatches for fill/font pickers).
pub fn spreadsheet_color_swatches() -> Vec<Hsla> {
    [
        0x000000, 0x374151, 0x6B7280, 0x9CA3AF, 0xE5E7EB, 0xFFFFFF, // Neutrals
        0xDC2626, 0xEA580C, 0xD97706, 0x16A34A, 0x0F766E, 0x2563EB, // Vivid accents
        0xFEE2E2, 0xFFEDD5, 0xFEF3C7, 0xDCFCE7, 0xCCFBF1, 0xDBEAFE, // Pastel cell fills
    ]
    .into_iter()
    .map(hex_rgb)
    .collect()
}
