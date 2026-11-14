//! Design system, theme tokens, semantic icons, component wrappers, and interactive gallery
//! for Ordia (`PLAN.md` §5.2).

/// Reusable Ordia UI wrappers over GPUI Kit (`ToolbarButton`, `ToolbarSelect`, `ToolbarColorPicker`, `SegmentedControl`).
pub mod components;
/// Interactive component and token gallery view (`cargo run -p ordia-app -- --gallery`).
pub mod gallery;
/// Semantic Lucide icon wrapper (`OrdiaIcon`).
pub mod icons;
/// Semantic design tokens and Light / Dark GPUI Kit theme adapters.
pub mod theme;

pub use components::{
    SegmentOption, SegmentedControl, ToolbarButton, ToolbarColorPicker, ToolbarSelect,
    ToolbarSelectState, new_toolbar_color_picker_state, new_toolbar_select_state,
};
pub use gallery::DesignGalleryView;
pub use icons::OrdiaIcon;
pub use theme::{
    ElevationScale, MotionScale, OrdiaColorTokens, OrdiaTokens, RadiusScale, SpacingScale,
    TypographyScale, apply_ordia_theme, spreadsheet_color_swatches,
};
