//! Menu widget hierarchy (`src/menus/widgets.js`).
//!
//! Ports `Widget`, `Rows`, `List`, `Button`, `Text`, `Panel`, `HorizontalButtons`,
//! `Tabs`, `Options`, and `OptionRow` from `src/menus/widgets.js`.

use crate::flashy::TitleIntro;
use crate::options::OptionKind;
use crate::space::{Color, WHITE_COLOR};

/// Action identifier or callback token triggered by a widget interaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WidgetAction {
    /// Open one of the option sub-pages (`"game"`, `"video"`, `"audio"`, `"controls"`).
    OpenPage(String),
    /// Pop the active screen or popup (`"back"`).
    Back,
    /// Reset the current option page to its default values (`"reset"`).
    ResetPage(String),
    /// Open the key rebind popup for `(tab, action_id, label)`.
    StartRebind {
        tab: String,
        action: String,
        label: String,
    },
    /// Switch the active tab in `CONTROL OPTIONS` (`"infantry"`, `"vehicle"`, `"starfighter"`).
    SelectTab(String),
    /// Custom named action (`"online"`, `"integrations"`, `"credits"`, `"audioPanel"`, `"yes"`, `"no"`, `"ok"`, etc.).
    Named(String),
}

/// Layout configuration for an angled stack of [`RowsWidget`] bars
/// (`Rows.layout` in `src/menus/widgets.js`, `TOP_LAYOUT` in `src/menus/option-pages.js`,
/// and `RowsLayout` in `rust/bfe-shell/src/render.rs`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowsLayout {
    pub at: [f64; 2],
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub rot_x: f64,
    pub rot_y: f64,
    pub rot_z: f64,
    pub xyz_order: bool,
    pub font: &'static str,
    pub y_spacing: f64,
    pub list_count: Option<usize>,
}

impl Default for RowsLayout {
    fn default() -> Self {
        Self {
            at: [0.5, 0.5],
            x: 0.0,
            y: 20.0,
            width: 350.0,
            rot_x: 0.0,
            rot_y: 35.0,
            rot_z: 0.0,
            xyz_order: false,
            font: "gamefont_medium",
            y_spacing: 5.0,
            list_count: None,
        }
    }
}

/// One row item inside a [`RowsWidget`] (`{ id, label, disabled, hidden, action }` in `src/menus/widgets.js`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowItem {
    pub id: String,
    pub label: String,
    pub disabled: bool,
    pub hidden: bool,
    pub action: Option<WidgetAction>,
}

impl RowItem {
    /// Creates an enabled visible row item.
    pub fn enabled(
        id: impl Into<String>,
        label: impl Into<String>,
        action: Option<WidgetAction>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            disabled: false,
            hidden: false,
            action,
        }
    }

    /// Creates a disabled visible row item.
    pub fn disabled(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            disabled: true,
            hidden: false,
            action: None,
        }
    }
}

/// Angled stack of menu rows under an orange header bar (`Rows` in `src/menus/widgets.js`).
#[derive(Clone, Debug)]
pub struct RowsWidget {
    pub id: String,
    pub title: String,
    pub rows: Vec<RowItem>,
    pub selected: usize,
    pub layout: RowsLayout,
    pub intro: TitleIntro,
    pub hidden: bool,
    pub disabled: bool,
}

impl RowsWidget {
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        rows: Vec<RowItem>,
        layout: RowsLayout,
    ) -> Self {
        let mut w = Self {
            id: id.into(),
            title: title.into(),
            rows,
            selected: 0,
            layout,
            intro: TitleIntro::completed(),
            hidden: false,
            disabled: false,
        };
        w.selected = w.first_enabled().unwrap_or(0);
        w
    }

    /// Returns the visible (`!hidden`) rows.
    pub fn visible_rows(&self) -> impl Iterator<Item = &RowItem> {
        self.rows.iter().filter(|r| !r.hidden)
    }

    /// Returns the number of visible (`!hidden`) rows.
    pub fn visible_count(&self) -> usize {
        self.rows.iter().filter(|r| !r.hidden).count()
    }

    /// Returns the index (within `visible_rows()`) of the first enabled row.
    pub fn first_enabled(&self) -> Option<usize> {
        self.visible_rows().position(|r| !r.disabled)
    }

    /// Moves selection by `dy` (`-1` or `+1`) across visible enabled rows (`Rows.move` in `widgets.js`).
    pub fn move_selection(&mut self, dy: i32) -> bool {
        let vis: Vec<(usize, bool)> = self
            .visible_rows()
            .enumerate()
            .map(|(i, r)| (i, !r.disabled))
            .collect();
        let n = vis.len();
        if n == 0 || !vis.iter().any(|&(_, en)| en) {
            return false;
        }
        let step = if dy < 0 { -1isize } else { 1isize };
        let mut i = (self.selected.min(n - 1)) as isize;
        for _ in 0..n {
            i = (i + step).rem_euclid(n as isize);
            if vis[i as usize].1 {
                let changed = self.selected != (i as usize);
                self.selected = i as usize;
                return changed;
            }
        }
        false
    }

    /// Activates the currently selected visible row (`Rows.accept` in `widgets.js`).
    pub fn accept(&self) -> Option<WidgetAction> {
        let row = self.visible_rows().nth(self.selected)?;
        if row.disabled {
            None
        } else {
            row.action.clone()
        }
    }
}

/// Column definition inside a [`ListWidget`] (`columns` in `List` in `src/menus/widgets.js`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ListColumn {
    pub width: f64,
    pub align: u32,
}

/// Visual layout for a [`ListWidget`] (`List.look` in `src/menus/widgets.js`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ListLook {
    pub at: [f64; 2],
    pub x: f64,
    pub y: f64,
    pub rot_y: f64,
    pub box_x: f64,
    pub box_y: f64,
    pub box_w: f64,
    pub box_h: f64,
    pub font: &'static str,
    pub pitch: f64,
    pub inset: f64,
    pub pad_left: f64,
    pub pad_right: f64,
    pub show_frame: bool,
}

impl Default for ListLook {
    fn default() -> Self {
        Self {
            at: [0.5, 0.5],
            x: 0.0,
            y: 0.0,
            rot_y: 0.0,
            box_x: 0.0,
            box_y: -15.0,
            box_w: 800.0,
            box_h: 442.0,
            font: "gamefont_medium",
            pitch: 26.0,
            inset: 15.0,
            pad_left: 15.0,
            pad_right: 15.0,
            show_frame: true,
        }
    }
}

/// One row inside a [`ListWidget`] (`src/menus/widgets.js`).
#[derive(Clone, Debug, PartialEq)]
pub struct ListEntry {
    pub id: String,
    pub cells: [String; 2],
    pub cell_count: usize,
    pub disabled: bool,
    /// Optional slider metadata `(ratio_0_to_1)` when this row is a slider option.
    pub slider_ratio: Option<f64>,
    /// Optional trailing icon texture names (`["rep_icon", "cis_icon"]`).
    pub icons: Vec<&'static str>,
    pub action: Option<WidgetAction>,
}

impl ListEntry {
    /// Creates a two-column `label | value` list row.
    pub fn two_col(
        id: impl Into<String>,
        left: impl Into<String>,
        right: impl Into<String>,
        disabled: bool,
        action: Option<WidgetAction>,
    ) -> Self {
        Self {
            id: id.into(),
            cells: [left.into(), right.into()],
            cell_count: 2,
            disabled,
            slider_ratio: None,
            icons: Vec::new(),
            action,
        }
    }

    /// Creates a single-column list row.
    pub fn one_col(
        id: impl Into<String>,
        label: impl Into<String>,
        disabled: bool,
        action: Option<WidgetAction>,
    ) -> Self {
        Self {
            id: id.into(),
            cells: [label.into(), String::new()],
            cell_count: 1,
            disabled,
            slider_ratio: None,
            icons: Vec::new(),
            action,
        }
    }
}

/// Framed scrollable list of rows (`List` in `src/menus/widgets.js`).
#[derive(Clone, Debug)]
pub struct ListWidget {
    pub id: String,
    pub rows: Vec<ListEntry>,
    pub columns: Vec<ListColumn>,
    pub selected: usize,
    pub hover_row: Option<usize>,
    pub first: usize,
    pub shown: usize,
    pub look: ListLook,
    pub hidden: bool,
    pub disabled: bool,
}

impl ListWidget {
    pub fn new(
        id: impl Into<String>,
        rows: Vec<ListEntry>,
        columns: Vec<ListColumn>,
        look: ListLook,
    ) -> Self {
        let shown = (((look.box_h - 2.0 * look.inset) / look.pitch).floor() as usize).max(1);
        let mut w = Self {
            id: id.into(),
            rows,
            columns,
            selected: 0,
            hover_row: None,
            first: 0,
            shown,
            look,
            hidden: false,
            disabled: false,
        };
        w.reveal();
        w
    }

    /// Updates `shown` from `look.box_h`, `look.inset`, and `look.pitch` and scrolls so
    /// `selected` is within `[first, first + shown)` (`List.reveal()` in `src/menus/widgets.js`).
    pub fn reveal(&mut self) {
        self.shown =
            (((self.look.box_h - 2.0 * self.look.inset) / self.look.pitch).floor() as usize).max(1);
        let n = self.rows.len();
        if n == 0 {
            self.selected = 0;
            self.first = 0;
            return;
        }
        self.selected = self.selected.min(n - 1);
        if self.selected < self.first {
            self.first = self.selected;
        } else if self.selected >= self.first + self.shown {
            self.first = self.selected + 1 - self.shown;
        }
        let max_first = n.saturating_sub(self.shown);
        self.first = self.first.min(max_first);
    }

    /// Moves the selection by `dy` (`-1` or `+1`), wrapping around (`List.move` in `src/menus/widgets.js`).
    pub fn move_selection(&mut self, dy: i32) -> bool {
        let n = self.rows.len();
        if n == 0 {
            return false;
        }
        let step = if dy < 0 { -1isize } else { 1isize };
        let next = ((self.selected as isize) + step).rem_euclid(n as isize) as usize;
        let changed = next != self.selected;
        self.selected = next;
        self.reveal();
        changed
    }

    /// Scrolls the visible window by `dy` rows (`List.scroll` in `src/menus/widgets.js`).
    pub fn scroll(&mut self, dy: i32) -> bool {
        let n = self.rows.len();
        if n == 0 {
            return false;
        }
        let next = ((self.selected as isize) + (dy as isize)).clamp(0, (n as isize) - 1) as usize;
        let changed = next != self.selected;
        self.selected = next;
        self.reveal();
        changed
    }
}

/// Bottom-edge or positioned action button (`Button` in `src/menus/widgets.js`).
#[derive(Clone, Debug)]
pub struct ButtonWidget {
    pub id: String,
    pub place: &'static str,
    pub text: String,
    pub at: Option<[f64; 2]>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub align: Option<u32>,
    pub font: &'static str,
    pub disabled: bool,
    pub hidden: bool,
    pub action: Option<WidgetAction>,
}

impl ButtonWidget {
    pub fn new(
        id: impl Into<String>,
        place: &'static str,
        text: impl Into<String>,
        action: Option<WidgetAction>,
    ) -> Self {
        Self {
            id: id.into(),
            place,
            text: text.into(),
            at: None,
            x: None,
            y: None,
            align: None,
            font: "gamefont_medium",
            disabled: false,
            hidden: false,
            action,
        }
    }
}

/// Visual style preset for a [`TextWidget`] (`Text.look` in `src/menus/widgets.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextLook {
    Title,
    Heading,
    Popup,
    Subtitle,
    Note,
    Label,
}

/// Static or flashy title/note label (`Text` in `src/menus/widgets.js`).
#[derive(Clone, Debug)]
pub struct TextWidget {
    pub id: String,
    pub look: TextLook,
    pub text: String,
    pub at: [f64; 2],
    pub safe: bool,
    pub x: f64,
    pub y: f64,
    pub align: u32,
    pub font: Option<&'static str>,
    pub color: Option<Color>,
    pub alpha: f64,
    pub flashy: bool,
    pub intro: TitleIntro,
    pub hidden: bool,
}

impl TextWidget {
    /// Creates a top-center screen title (`Text({ id: "title", look: "title", text, y })`).
    pub fn title(id: impl Into<String>, text: impl Into<String>, y: f64) -> Self {
        Self {
            id: id.into(),
            look: TextLook::Title,
            text: text.into(),
            at: [0.5, 0.0],
            safe: true,
            x: 0.0,
            y,
            align: 2,
            font: None,
            color: None,
            alpha: 1.0,
            flashy: false,
            intro: TitleIntro::completed(),
            hidden: false,
        }
    }

    /// Creates a centered note line (`Text({ id: "note", look: "note", text, y })`).
    pub fn note(id: impl Into<String>, text: impl Into<String>, y: f64) -> Self {
        Self {
            id: id.into(),
            look: TextLook::Note,
            text: text.into(),
            at: [0.5, 0.0],
            safe: true,
            x: 0.0,
            y,
            align: 2,
            font: None,
            color: None,
            alpha: 1.0,
            flashy: false,
            intro: TitleIntro::completed(),
            hidden: false,
        }
    }
}

/// Framed 9-slice window panel (`Panel` in `src/menus/widgets.js`).
#[derive(Clone, Debug)]
pub struct PanelWidget {
    pub id: String,
    pub title: String,
    pub at: [f64; 2],
    pub safe: bool,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub texture: &'static str,
    pub color: Color,
    pub hidden: bool,
}

impl PanelWidget {
    pub fn new(id: impl Into<String>, x: f64, y: f64, w: f64, h: f64) -> Self {
        Self {
            id: id.into(),
            title: String::new(),
            at: [0.5, 0.5],
            safe: true,
            x,
            y,
            w,
            h,
            texture: "border_3_pieces",
            color: WHITE_COLOR,
            hidden: false,
        }
    }
}

/// One button inside [`HorizontalButtonsWidget`] (`src/menus/widgets.js`).
#[derive(Clone, Debug)]
pub struct HorizontalButtonItem {
    pub id: String,
    pub label: String,
    pub disabled: bool,
    pub action: Option<WidgetAction>,
}

/// Centered horizontal row of text buttons (`HorizontalButtons` in `src/menus/widgets.js`).
#[derive(Clone, Debug)]
pub struct HorizontalButtonsWidget {
    pub id: String,
    pub buttons: Vec<HorizontalButtonItem>,
    pub selected: usize,
    pub y: f64,
    pub spacing: f64,
    pub font: &'static str,
    pub hidden: bool,
    pub disabled: bool,
}

impl HorizontalButtonsWidget {
    pub fn new(id: impl Into<String>, y: f64, buttons: Vec<HorizontalButtonItem>) -> Self {
        Self {
            id: id.into(),
            buttons,
            selected: 0,
            y,
            spacing: 85.0,
            font: "gamefont_medium",
            hidden: false,
            disabled: false,
        }
    }

    pub fn step(&mut self, dx: i32) -> bool {
        let n = self.buttons.len();
        if n == 0 {
            return false;
        }
        let dir = if dx < 0 { -1isize } else { 1isize };
        let next = ((self.selected as isize) + dir).rem_euclid(n as isize) as usize;
        let changed = next != self.selected;
        self.selected = next;
        changed
    }
}

/// One tab item inside [`TabsWidget`] (`src/menus/widgets.js`).
#[derive(Clone, Debug)]
pub struct TabItem {
    pub id: String,
    pub label: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub flipped: bool,
}

/// Horizontal tab bar (`Tabs` in `src/menus/widgets.js`, used for `INFANTRY` / `VEHICLE` / `STARFIGHTER`).
#[derive(Clone, Debug)]
pub struct TabsWidget {
    pub id: String,
    pub tabs: Vec<TabItem>,
    pub active: String,
    pub selected: usize,
    pub y: f64,
    pub height: f64,
    pub font: &'static str,
    pub hidden: bool,
    pub disabled: bool,
}

impl TabsWidget {
    pub fn new(
        id: impl Into<String>,
        tabs: Vec<TabItem>,
        active: impl Into<String>,
        y: f64,
        height: f64,
        font: &'static str,
    ) -> Self {
        let active_str = active.into();
        let selected = tabs.iter().position(|t| t.id == active_str).unwrap_or(0);
        Self {
            id: id.into(),
            tabs,
            active: active_str,
            selected,
            y,
            height,
            font,
            hidden: false,
            disabled: false,
        }
    }

    pub fn step(&mut self, dx: i32) -> Option<String> {
        let n = self.tabs.len();
        if n == 0 {
            return None;
        }
        let dir = if dx < 0 { -1isize } else { 1isize };
        self.selected = ((self.selected as isize) + dir).rem_euclid(n as isize) as usize;
        self.active = self.tabs[self.selected].id.clone();
        Some(self.active.clone())
    }
}

/// Layout selector for [`OptionsWidget`] (`OPTION_LOOKS` in `src/menus/themes/classic/index.js`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OptionsLayoutKind {
    /// `GAME OPTIONS`: centered `0.5` split (`labels: [0.5, -15], values: [0.5, 15], top: 150, pitch: 40`).
    #[default]
    Game,
    /// `VIDEO OPTIONS`: centered `0.5` split (`labels: [0.5, -15], values: [0.5, 15], top: 32, pitch: 30, bottom: 60, slider width 110, scroll [0.5, 380]`).
    Video,
    /// `AUDIO OPTIONS`: `0.4` split (`labels: [0.4, 0], values: [0.4, 35], top: 55, pitch: 35, slider [0.4, 20]..[1, -176.8], number [1, 0]`).
    Audio,
    /// `CONTROL OPTIONS` top rows (`labels: [0.15, 0], top: 121, pitch: 24.5, slider [0.4, 0] width 0.25*safeW, number [1, -2]`).
    Controls,
}

/// One option row inside an [`OptionsWidget`] (`OptionRow` in `src/menus/widgets.js`).
#[derive(Clone, Debug, PartialEq)]
pub struct OptionsItem {
    pub id: String,
    pub label: String,
    pub value_text: String,
    pub kind: OptionKind,
    pub ratio: f64,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub disabled: bool,
    pub dim: bool,
    pub hidden: bool,
    pub note: String,
    pub action: Option<WidgetAction>,
}

/// Two-column `label | value` options widget (`Options` in `src/menus/widgets.js` and `paintOptions` in `classic/index.js`).
#[derive(Clone, Debug)]
pub struct OptionsWidget {
    pub id: String,
    pub layout: OptionsLayoutKind,
    pub items: Vec<OptionsItem>,
    pub selected: usize,
    pub first: usize,
    pub shown: usize,
    pub at: [f64; 2],
    pub safe: bool,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub pitch: f64,
    pub font: &'static str,
    pub hidden: bool,
    pub disabled: bool,
}

impl OptionsWidget {
    /// Creates an [`OptionsWidget`] for one of the standard option page layouts (`game`, `video`, `audio`, `controls`).
    pub fn with_layout(
        id: impl Into<String>,
        layout: OptionsLayoutKind,
        items: Vec<OptionsItem>,
        surface_height: f64,
    ) -> Self {
        let mut w = Self {
            id: id.into(),
            layout,
            items,
            selected: 0,
            first: 0,
            shown: usize::MAX,
            at: [0.5, 0.0],
            safe: true,
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            pitch: match layout {
                OptionsLayoutKind::Game => 40.0,
                OptionsLayoutKind::Video => 30.0,
                OptionsLayoutKind::Audio => 35.0,
                OptionsLayoutKind::Controls => 24.5,
            },
            font: match layout {
                OptionsLayoutKind::Controls => "gamefont_tiny",
                _ => "gamefont_medium",
            },
            hidden: false,
            disabled: false,
        };
        w.selected = w.first_enabled().unwrap_or(0);
        w.update_shown(surface_height);
        w.reveal();
        w
    }

    /// Returns true if `item` at `index` is enabled for focus (`Options.enabled(item)` in `src/menus/widgets.js`).
    pub fn is_item_enabled(&self, index: usize) -> bool {
        if self.disabled {
            return false;
        }
        let Some(item) = self.items.get(index) else {
            return false;
        };
        !item.hidden && !item.disabled && item.kind != OptionKind::Fixed
    }

    /// Returns the first enabled item index.
    pub fn first_enabled(&self) -> Option<usize> {
        (0..self.items.len()).find(|&i| self.is_item_enabled(i))
    }

    /// Recomputes `shown` from `surface_height` (`paintOptions` in `src/menus/themes/classic/index.js`).
    pub fn update_shown(&mut self, surface_height: f64) {
        if self.layout == OptionsLayoutKind::Video {
            let safe_h = 0.9 * surface_height;
            let room = safe_h - 32.0 - 60.0;
            self.shown = ((room / 30.0).floor() as usize).max(1);
        } else {
            self.shown = usize::MAX;
        }
    }

    /// Scrolls `first` so `selected` is within `[first, first + shown)` (`Options.reveal` in `src/menus/widgets.js`).
    pub fn reveal(&mut self) {
        let visible: Vec<usize> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, it)| (!it.hidden).then_some(i))
            .collect();
        if visible.is_empty() {
            self.first = 0;
            return;
        }
        if self.shown == usize::MAX {
            self.first = 0;
            return;
        }
        if let Some(at) = visible.iter().position(|&i| i == self.selected) {
            if at < self.first {
                self.first = at;
            } else if at >= self.first + self.shown {
                self.first = at + 1 - self.shown;
            }
        }
        let max_first = visible.len().saturating_sub(self.shown);
        self.first = self.first.min(max_first);
    }

    /// Moves selection by `dy` (`-1` or `+1`) across enabled items, returning `None` when
    /// `dy` steps past the first/last item so `MenuScreen` can advance to the next widget on the ring.
    pub fn move_selection(&mut self, dy: i32, wrap: bool) -> bool {
        let enabled: Vec<usize> = (0..self.items.len())
            .filter(|&i| self.is_item_enabled(i))
            .collect();
        if enabled.is_empty() {
            return false;
        }
        let cur_pos = enabled
            .iter()
            .position(|&i| i == self.selected)
            .unwrap_or(0) as isize;
        let step = if dy < 0 { -1isize } else { 1isize };
        let next_pos = cur_pos + step;
        let resolved = if wrap {
            next_pos.rem_euclid(enabled.len() as isize) as usize
        } else if next_pos >= 0 && (next_pos as usize) < enabled.len() {
            next_pos as usize
        } else {
            return false;
        };
        let next_idx = enabled[resolved];
        let changed = next_idx != self.selected;
        self.selected = next_idx;
        self.reveal();
        changed
    }

    /// Scrolls the visible window by `delta` rows (`Options.scrollBy` in `src/menus/widgets.js`).
    pub fn scroll_by(&mut self, delta: i32) -> bool {
        if self.shown == usize::MAX {
            return false;
        }
        let count = self.items.iter().filter(|it| !it.hidden).count();
        let max_first = count.saturating_sub(self.shown) as isize;
        let next = ((self.first as isize) + (delta as isize)).clamp(0, max_first) as usize;
        if next == self.first {
            return false;
        }
        self.first = next;
        true
    }
}

/// Standalone option row widget (`OptionRow` in `src/menus/widgets.js`).
#[derive(Clone, Debug)]
pub struct OptionRowWidget {
    pub id: String,
    pub label: String,
    pub value_text: String,
    pub kind: OptionKind,
    pub ratio: f64,
    pub at: [f64; 2],
    pub safe: bool,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub slider_width: f64,
    pub font: &'static str,
    pub disabled: bool,
    pub hidden: bool,
}

/// Polymorphic menu widget enum (`Widget` subclasses in `src/menus/widgets.js`).
#[derive(Clone, Debug)]
pub enum Widget {
    Rows(RowsWidget),
    List(ListWidget),
    Button(ButtonWidget),
    Text(TextWidget),
    Panel(PanelWidget),
    HorizontalButtons(HorizontalButtonsWidget),
    Tabs(TabsWidget),
    Options(OptionsWidget),
    OptionRow(OptionRowWidget),
}

impl Widget {
    /// Returns the widget's string identifier (`widget.id` in `src/menus/widgets.js`).
    pub fn id(&self) -> &str {
        match self {
            Self::Rows(w) => &w.id,
            Self::List(w) => &w.id,
            Self::Button(w) => &w.id,
            Self::Text(w) => &w.id,
            Self::Panel(w) => &w.id,
            Self::HorizontalButtons(w) => &w.id,
            Self::Tabs(w) => &w.id,
            Self::Options(w) => &w.id,
            Self::OptionRow(w) => &w.id,
        }
    }

    /// Returns true if the widget is hidden (`widget.isHidden()` in `src/menus/widgets.js`).
    pub fn is_hidden(&self) -> bool {
        match self {
            Self::Rows(w) => w.hidden,
            Self::List(w) => w.hidden,
            Self::Button(w) => w.hidden,
            Self::Text(w) => w.hidden,
            Self::Panel(w) => w.hidden,
            Self::HorizontalButtons(w) => w.hidden,
            Self::Tabs(w) => w.hidden,
            Self::Options(w) => w.hidden,
            Self::OptionRow(w) => w.hidden,
        }
    }

    /// Returns true if the widget is disabled (`widget.isDisabled()` in `src/menus/widgets.js`).
    pub fn is_disabled(&self) -> bool {
        match self {
            Self::Rows(w) => w.disabled,
            Self::List(w) => w.disabled,
            Self::Button(w) => w.disabled,
            Self::Text(_) | Self::Panel(_) => true,
            Self::HorizontalButtons(w) => w.disabled,
            Self::Tabs(w) => w.disabled,
            Self::Options(w) => w.disabled,
            Self::OptionRow(w) => w.disabled,
        }
    }

    /// Returns true if the widget can receive focus (`widget.canFocus()` in `src/menus/widgets.js`).
    pub fn can_focus(&self) -> bool {
        if self.is_hidden() || self.is_disabled() {
            return false;
        }
        match self {
            Self::Rows(w) => w.first_enabled().is_some(),
            Self::List(w) => !w.rows.is_empty(),
            Self::Button(_) => true,
            Self::Text(_) | Self::Panel(_) => false,
            Self::HorizontalButtons(w) => !w.buttons.is_empty(),
            Self::Tabs(w) => !w.tabs.is_empty(),
            Self::Options(w) => w.first_enabled().is_some(),
            Self::OptionRow(_) => true,
        }
    }

    /// Advances any flashy intro timers on the widget (`widget.update(dt)` in `src/menus/widgets.js`).
    pub fn update(&mut self, dt: f64) {
        match self {
            Self::Rows(w) => w.intro.step(dt),
            Self::Text(w) if w.flashy => w.intro.step(dt),
            _ => {}
        }
    }
}
