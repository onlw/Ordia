//! Reusable Ordia UI component wrappers built over GPUI Kit (`PLAN.md` §5.1–§5.2).
//!
//! Provides compact, accessible desktop controls standardized for Ordia's single-row contextual
//! toolbar and right property inspector:
//! - [`ToolbarButton`]: compact icon/label command or toggle button
//! - [`ToolbarSelect`]: compact single-choice dropdown backed by [`ToolbarSelectState`]
//! - [`ToolbarColorPicker`]: compact spreadsheet color swatch & HSLA picker backed by [`ColorPickerState`]
//! - [`SegmentedControl`]: compact mutually exclusive option group (alignment, view mode, theme)

use std::rc::Rc;

use gpui_kit::component::{
    Disableable as _, Icon, IndexPath, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    color_picker::{ColorPicker, ColorPickerState},
    select::{SearchableVec, Select, SelectState},
    tab::{Tab, TabBar},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, ClickEvent, ElementId, Entity, Hsla, IntoElement, Pixels, RenderOnce,
    SharedString, Styled as _, Window,
};

use crate::icons::OrdiaIcon;
use crate::theme::spreadsheet_color_swatches;

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
type SegmentSelectHandler = Rc<dyn Fn(usize, &mut Window, &mut App) + 'static>;

/// Retained state type alias for [`ToolbarSelect`].
pub type ToolbarSelectState = SelectState<SearchableVec<SharedString>>;

/// Creates a retained [`ToolbarSelectState`] entity populated with `items` and optional `selected_ix`.
pub fn new_toolbar_select_state(
    items: impl IntoIterator<Item = impl Into<SharedString>>,
    selected_ix: Option<usize>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<ToolbarSelectState> {
    let vec: Vec<SharedString> = items.into_iter().map(Into::into).collect();
    let index_path = selected_ix.map(IndexPath::new);
    cx.new(|cx| SelectState::new(SearchableVec::new(vec), index_path, window, cx))
}

/// Creates a retained [`ColorPickerState`] entity with an optional initial `default_color`.
pub fn new_toolbar_color_picker_state(
    default_color: Option<Hsla>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<ColorPickerState> {
    cx.new(|cx| {
        let state = ColorPickerState::new(window, cx);
        match default_color {
            Some(color) => state.default_value(color),
            None => state,
        }
    })
}

/// Compact command or toggle button standardized for Ordia toolbars and inspectors.
///
/// Uses `Size::Small` and `ghost` styling at rest, elevating to selected emphasis when
/// `selected(true)` is set (e.g. active Bold / Italic / Filter toggle).
#[derive(IntoElement)]
pub struct ToolbarButton {
    id: ElementId,
    icon: Option<OrdiaIcon>,
    label: Option<SharedString>,
    tooltip: Option<SharedString>,
    selected: bool,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl ToolbarButton {
    /// Creates a compact toolbar button with a stable domain `id`.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            icon: None,
            label: None,
            tooltip: None,
            selected: false,
            disabled: false,
            on_click: None,
        }
    }

    /// Sets the leading semantic [`OrdiaIcon`].
    pub fn icon(mut self, icon: OrdiaIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Sets a visible text label alongside (or instead of) the icon.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets hover tooltip text (also used as screen-reader accessibility label when icon-only).
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Marks the button as toggled/active (e.g. Bold enabled on the active cell).
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Disables pointer and keyboard activation.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Registers a click activation callback.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for ToolbarButton {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let a11y = self.tooltip.clone();
        let mut btn = Button::new(self.id)
            .small()
            .ghost()
            .selected(self.selected)
            .disabled(self.disabled);

        if let Some(icon) = self.icon {
            btn = btn.icon(Icon::new(icon));
        }
        if let Some(label) = self.label {
            btn = btn.label(label);
        } else if let Some(a11y_name) = a11y {
            btn = btn.accessibility_label(a11y_name);
        }
        if let Some(tip) = self.tooltip {
            btn = btn.tooltip(tip);
        }
        if let Some(handler) = self.on_click {
            btn = btn.on_click(move |ev, window, cx| handler(ev, window, cx));
        }
        btn
    }
}

/// Compact dropdown selector for toolbar and inspector fields (font family, font size, number format).
#[derive(IntoElement)]
pub struct ToolbarSelect {
    id: ElementId,
    state: Entity<ToolbarSelectState>,
    placeholder: Option<SharedString>,
    width: Option<Pixels>,
    menu_width: Option<Pixels>,
    disabled: bool,
}

impl ToolbarSelect {
    /// Creates a compact select bound to a retained [`ToolbarSelectState`].
    pub fn new(id: impl Into<ElementId>, state: &Entity<ToolbarSelectState>) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            placeholder: None,
            width: None,
            menu_width: None,
            disabled: false,
        }
    }

    /// Sets placeholder text displayed when no option is selected.
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// Sets explicit trigger width so neighboring toolbar controls do not jitter when values change.
    pub fn with_width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    /// Sets explicit popup menu width when option labels are wider than the compact trigger.
    pub fn with_menu_width(mut self, width: Pixels) -> Self {
        self.menu_width = Some(width);
        self
    }

    /// Disables opening or changing the select.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for ToolbarSelect {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        Select::new(&self.state)
            .id(self.id)
            .small()
            .disabled(self.disabled)
            .when_some(self.placeholder, |el, p| el.placeholder(p))
            .when_some(self.width, |el, w| el.w(w))
            .when_some(self.menu_width, |el, mw| el.menu_width(mw))
    }
}

/// Compact color picker pre-populated with Ordia's theme-invariant spreadsheet swatches.
#[derive(IntoElement)]
pub struct ToolbarColorPicker {
    state: Entity<ColorPickerState>,
    icon: Option<OrdiaIcon>,
    label: Option<SharedString>,
    accessibility_label: Option<SharedString>,
}

impl ToolbarColorPicker {
    /// Creates a compact color picker bound to a retained [`ColorPickerState`].
    pub fn new(state: &Entity<ColorPickerState>) -> Self {
        Self {
            state: state.clone(),
            icon: None,
            label: None,
            accessibility_label: None,
        }
    }

    /// Displays a semantic [`OrdiaIcon`] on the picker trigger button instead of a plain color square.
    pub fn icon(mut self, icon: OrdiaIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Sets a visible label next to the trigger.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the screen-reader accessible label for icon-only or swatch-only triggers.
    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }
}

impl RenderOnce for ToolbarColorPicker {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        ColorPicker::new(&self.state)
            .small()
            .featured_colors(spreadsheet_color_swatches())
            .when_some(self.icon, |el, icon| el.icon(Icon::new(icon)))
            .when_some(self.label, |el, label| el.label(label))
            .when_some(self.accessibility_label, |el, a11y| {
                el.accessibility_label(a11y)
            })
    }
}

/// Single option descriptor inside a [`SegmentedControl`].
pub struct SegmentOption {
    icon: Option<OrdiaIcon>,
    label: Option<SharedString>,
    tooltip: Option<SharedString>,
}

impl SegmentOption {
    /// Creates a text-labeled segment option.
    pub fn label(label: impl Into<SharedString>) -> Self {
        Self {
            icon: None,
            label: Some(label.into()),
            tooltip: None,
        }
    }

    /// Creates an icon-only segment option with a required tooltip / accessible name.
    pub fn icon(icon: OrdiaIcon, tooltip: impl Into<SharedString>) -> Self {
        Self {
            icon: Some(icon),
            label: None,
            tooltip: Some(tooltip.into()),
        }
    }

    /// Creates an icon + label segment option.
    pub fn icon_and_label(icon: OrdiaIcon, label: impl Into<SharedString>) -> Self {
        Self {
            icon: Some(icon),
            label: Some(label.into()),
            tooltip: None,
        }
    }
}

/// Compact segmented control for mutually exclusive choices (e.g. Left/Center/Right alignment,
/// Light/Dark appearance, Normal/Page-Break view).
#[derive(IntoElement)]
pub struct SegmentedControl {
    id: ElementId,
    options: Vec<SegmentOption>,
    selected_ix: usize,
    on_select: Option<SegmentSelectHandler>,
}

impl SegmentedControl {
    /// Creates a compact segmented control with a stable `id`.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            options: Vec::new(),
            selected_ix: 0,
            on_select: None,
        }
    }

    /// Appends a [`SegmentOption`] to the segmented control.
    pub fn option(mut self, option: SegmentOption) -> Self {
        self.options.push(option);
        self
    }

    /// Appends multiple [`SegmentOption`] items.
    pub fn options(mut self, options: impl IntoIterator<Item = SegmentOption>) -> Self {
        self.options.extend(options);
        self
    }

    /// Sets the zero-based active segment index.
    pub fn selected_index(mut self, ix: usize) -> Self {
        self.selected_ix = ix;
        self
    }

    /// Registers a callback invoked with the newly selected zero-based index.
    pub fn on_select(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for SegmentedControl {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut bar = TabBar::new(self.id)
            .segmented()
            .small()
            .selected_index(self.selected_ix);

        if let Some(handler) = self.on_select {
            bar = bar.on_click(move |ix: &usize, window, cx| {
                handler(*ix, window, cx);
            });
        }

        for opt in self.options {
            let mut tab = Tab::new();
            if let Some(icon) = opt.icon {
                tab = tab.icon(Icon::new(icon));
            }
            if let Some(label) = opt.label {
                tab = tab.label(label);
            }
            if let Some(tip) = opt.tooltip {
                tab = tab.aria_label(tip);
            }
            bar = bar.child(tab);
        }

        bar
    }
}
