//! Menu widget hierarchy (`src/menus/widgets.js`).
//!
//! Ports `Widget`, `Rows`, `List`, `Button`, `Text`, `Panel`, `HorizontalButtons`,
//! `Tabs`, `Options`, and `OptionRow` from `src/menus/widgets.js`.
//! Each widget stores its logical state and layout parameters while [`crate::theme::ClassicTheme`]
//! paints it into a [`crate::surface::Batch`] and records its 3D hotspots.

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
    /// Normalized safe-area anchor `[ax, ay]` (`[0.5, 0.5]` by default).
    pub at: [f64; 2],
    /// Horizontal offset in interface units added to `rows_shift(width, pitch, count)`.
    pub x: f64,
    /// Vertical offset in interface units from the anchor point.
    pub y: f64,
    /// Base width of the top and bottom row bars before chevron pitch stepping.
    pub width: f64,
    /// Local X-axis Euler rotation in degrees.
    pub rot_x: f64,
    /// Local Y-axis Euler rotation in degrees (`35.0` degrees on the top-level options menu).
    pub rot_y: f64,
    /// Local Z-axis Euler rotation in degrees.
    pub rot_z: f64,
    /// When `true`, applies Euler rotations in `XYZ` order; when `false`, uses `YXZ` (`classic/index.js`).
    pub xyz_order: bool,
    /// Bitmap font name for the row labels and header bar (`"gamefont_medium"`).
    pub font: &'static str,
    /// Vertical gap in interface units added to the font line height to form the row pitch `p`.
    pub y_spacing: f64,
    /// Optional total row count (including hidden console/PC rows) used by `rows_shift`
    /// (`listCount` in `src/menus/widgets.js`, replicating `AddVerticalButtons`).
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
    /// Row identifier (`"game"`, `"video"`, `"audio"`, `"controls"`, `"online"`, etc.).
    pub id: String,
    /// Display text rendered on the blue 3-piece button bar.
    pub label: String,
    /// When `true`, renders the label in [`crate::theme::GREY`] and skips it during focus navigation.
    pub disabled: bool,
    /// When `true`, omits the row from `visible_rows()` (though `list_count` may still count it for centering).
    pub hidden: bool,
    /// Action emitted when the row is activated via `Enter`, `Space`, or left click.
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
    /// Widget identifier (`"menu"`).
    pub id: String,
    /// Header title rendered in black on the orange `headerbutton*` bar above the rows.
    pub title: String,
    /// Complete list of row items (both visible and hidden).
    pub rows: Vec<RowItem>,
    /// Index within `visible_rows()` of the currently highlighted row.
    pub selected: usize,
    /// 3D placement, rotation, bar width, and font metrics (`Rows.layout` in `widgets.js`).
    pub layout: RowsLayout,
    /// Flashy intro timer for the row stack.
    pub intro: TitleIntro,
    /// When `true`, the entire row stack is hidden and unfocusable.
    pub hidden: bool,
    /// When `true`, the entire row stack is disabled and cannot receive focus.
    pub disabled: bool,
}

impl RowsWidget {
    /// Creates a new [`RowsWidget`] with `selected` initialized to the first enabled visible row.
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

    /// Returns the visible (`!hidden`) rows (`Rows.visibleRows` in `src/menus/widgets.js`).
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

    /// Moves selection by `dy` (`-1` or `+1`) across visible enabled rows, wrapping around
    /// (`MenuScreen.move` over `Rows.items` in `src/menus/screen.js` and `src/menus/widgets.js`).
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
        // Step through visible indices modulo `n` until an enabled row is reached.
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

    /// Activates the currently selected visible row (`Rows.activate` in `src/menus/widgets.js`).
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
    /// Horizontal offset of the column as a fraction (`0.0..1.0`) of the inner list width (`column.x` in `classic/index.js`).
    pub width: f64,
    /// Horizontal text alignment (`0` = left, `1` = right, `2` = center).
    pub align: u32,
}

/// Visual layout for a [`ListWidget`] (`List.look` in `src/menus/widgets.js` and `paintList` in `classic/index.js`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ListLook {
    /// Normalized safe-area anchor `[ax, ay]`.
    pub at: [f64; 2],
    /// Local X translation from `at`.
    pub x: f64,
    /// Local Y translation from `at`.
    pub y: f64,
    /// Local Y-axis rotation in degrees.
    pub rot_y: f64,
    /// Center X coordinate of the 9-slice list box in local widget space (`look.box.x`).
    pub box_x: f64,
    /// Center Y coordinate of the 9-slice list box in local widget space (`look.box.y`).
    pub box_y: f64,
    /// Total width of the list box in interface units (`look.box.w`).
    pub box_w: f64,
    /// Total height of the list box in interface units (`look.box.h`).
    pub box_h: f64,
    /// Bitmap font name for list cell text (`"gamefont_tiny"` on `CONTROL OPTIONS`).
    pub font: &'static str,
    /// Vertical distance between row centers (`look.pitch`, default `23` or `26` units).
    pub pitch: f64,
    /// Top and bottom vertical padding inside `box_h` (`look.inset`, default `15.0`).
    pub inset: f64,
    /// Left horizontal padding inside `box_w` (`look.padLeft`, default `15.0`).
    pub pad_left: f64,
    /// Right horizontal padding inside `box_w` (`look.padRight`, default `15.0`).
    pub pad_right: f64,
    /// Whether to draw the outer `border_3_pieces` 9-slice window (`look.window !== false` in `classic/index.js`).
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
    /// Row identifier (e.g., action id `"forward"` or map id).
    pub id: String,
    /// Up to two column text strings (`[action_label, bound_keys]` on `CONTROL OPTIONS`).
    pub cells: [String; 2],
    /// Number of active text columns (`1` or `2`).
    pub cell_count: usize,
    /// When `true`, renders cell text in [`crate::theme::GREY`].
    pub disabled: bool,
    /// Optional slider metadata `(ratio_0_to_1)` when this row is a slider option.
    pub slider_ratio: Option<f64>,
    /// Optional trailing icon texture names (`["rep_icon", "cis_icon"]`).
    pub icons: Vec<&'static str>,
    /// Action emitted when the row is activated (`WidgetAction::StartRebind` on `CONTROL OPTIONS`).
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
    /// Widget identifier (`"bindings"` on `CONTROL OPTIONS`).
    pub id: String,
    /// Rows displayed inside the list box.
    pub rows: Vec<ListEntry>,
    /// Column positions and alignments (`look.columns` in `classic/index.js`).
    pub columns: Vec<ListColumn>,
    /// Index of the currently selected row (`0..rows.len()`).
    pub selected: usize,
    /// Index of the row currently hovered by the pointer, drawn with `listbox_hilight`.
    pub hover_row: Option<usize>,
    /// Index of the top visible row in the scroll window (`0..=rows.len().saturating_sub(shown)`).
    pub first: usize,
    /// Number of rows that fit vertically inside `look.box_h - 2 * look.inset`.
    pub shown: usize,
    /// Box geometry, padding, and font configuration.
    pub look: ListLook,
    /// When `true`, the list is not drawn or focusable.
    pub hidden: bool,
    /// When `true`, the list cannot receive focus.
    pub disabled: bool,
}

impl ListWidget {
    /// Creates a new [`ListWidget`] and computes `shown` and `first` via [`Self::reveal`].
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

    /// Moves the selection by `dy` (`-1` or `+1`), wrapping around (`List.key` / `Screen.move` in `src/menus/widgets.js`).
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

    /// Scrolls the selection by `dy` rows without wrapping (`List.wheel` / `PageUp` / `PageDown` in `src/menus/widgets.js`).
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
    /// Button identifier (`"back"`, `"reset"`, `"audioPanel"`, `"accept"`, etc.).
    pub id: String,
    /// Standard bottom-bar placement (`"back"` = bottom-left, `"middle"` = bottom-center, `"accept"` = bottom-right).
    pub place: &'static str,
    /// Localized button label rendered in `gamefont_medium`.
    pub text: String,
    /// Optional custom safe-area anchor `[ax, ay]` overriding `BUTTON_PLACES[place]`.
    pub at: Option<[f64; 2]>,
    /// Optional custom X offset overriding `BUTTON_INSET`.
    pub x: Option<f64>,
    /// Optional custom Y offset overriding `-12.0`.
    pub y: Option<f64>,
    /// Optional custom horizontal alignment (`0` = left, `1` = right, `2` = center).
    pub align: Option<u32>,
    /// Bitmap font name (`"gamefont_medium"` by default).
    pub font: &'static str,
    /// When `true`, renders in [`crate::theme::GREY`] and ignores activation.
    pub disabled: bool,
    /// When `true`, omits the button from rendering and hit-testing.
    pub hidden: bool,
    /// Action emitted when activated (`None` on `place == "back"` defaults to [`WidgetAction::Back`]).
    pub action: Option<WidgetAction>,
}

impl ButtonWidget {
    /// Creates a new [`ButtonWidget`] at `place` (`"back"`, `"middle"`, or `"accept"`).
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

/// Visual style preset for a [`TextWidget`] (`Text.look` in `src/menus/widgets.js` and `paintText` in `classic/index.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextLook {
    /// Screen title at top-center in `gamefont_large`.
    Title,
    /// Section heading in `gamefont_large`.
    Heading,
    /// Wrapped `260`-unit-wide modal question centered at `y = -74` in `gamefont_medium`.
    Popup,
    /// Secondary heading in `gamefont_medium`.
    Subtitle,
    /// Footer or hint text in `gamefont_small`.
    Note,
    /// Arbitrary positioned label in `gamefont_medium`.
    Label,
}

/// Static or flashy title/note label (`Text` in `src/menus/widgets.js`).
#[derive(Clone, Debug)]
pub struct TextWidget {
    /// Widget identifier (`"title"`, `"message"`, `"note"`, etc.).
    pub id: String,
    /// Preset typography and layout mode (`paintText` in `src/menus/themes/classic/index.js`).
    pub look: TextLook,
    /// String content (may contain `\n` line breaks).
    pub text: String,
    /// Normalized anchor `[ax, ay]`.
    pub at: [f64; 2],
    /// Whether `at` is inside the 90% safe area (`true`) or full viewport (`false`).
    pub safe: bool,
    /// Horizontal offset from `at` in interface units.
    pub x: f64,
    /// Vertical offset of the first line from `at` in interface units.
    pub y: f64,
    /// Horizontal alignment (`0` = left, `1` = right, `2` = center).
    pub align: u32,
    /// Optional explicit font name overriding the `look` default.
    pub font: Option<&'static str>,
    /// Optional explicit RGBA tint color overriding [`crate::theme::WHITE`].
    pub color: Option<Color>,
    /// Opacity multiplier (`0.0..=1.0`).
    pub alpha: f64,
    /// Whether `intro` advances on `update(dt)`.
    pub flashy: bool,
    /// Five-stage flashy reveal state (`TitleIntro` in `src/interface/flashy.js`).
    pub intro: TitleIntro,
    /// When `true`, the text widget is not drawn.
    pub hidden: bool,
}

impl TextWidget {
    /// Creates a top-center screen title (`new Text({ id: "title", look: "title", text, y })` in `widgets.js`).
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

    /// Creates a centered note line (`new Text({ id: "note", look: "note", text, y })` in `widgets.js`).
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
    /// Widget identifier (`"window"`).
    pub id: String,
    /// Optional title string drawn centered above the top border of the panel.
    pub title: String,
    /// Normalized anchor `[ax, ay]`.
    pub at: [f64; 2],
    /// Whether `at` is mapped into the 90% safe area (`true`) or full screen (`false` for popups).
    pub safe: bool,
    /// Center X coordinate of the panel relative to `at`.
    pub x: f64,
    /// Center Y coordinate of the panel relative to `at`.
    pub y: f64,
    /// Total panel width in interface units (`300.0` for `YES`/`NO` and `OK` popups, `360.0` for key capture).
    pub w: f64,
    /// Total panel height in interface units (`180.0` for modal popups).
    pub h: f64,
    /// 9-slice texture name (`"border_3_pieces"`).
    pub texture: &'static str,
    /// RGBA tint applied to the 9-slice frame quads.
    pub color: Color,
    /// When `true`, the panel is not drawn.
    pub hidden: bool,
}

impl PanelWidget {
    /// Creates a centered `"border_3_pieces"` 9-slice [`PanelWidget`] of size `w x h` at `(x, y)`.
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
    /// Button identifier (`"yes"`, `"no"`, `"ok"`).
    pub id: String,
    /// Localized label rendered in `gamefont_medium`.
    pub label: String,
    /// When `true`, renders in [`crate::theme::GREY`] and cannot be activated.
    pub disabled: bool,
    /// Action emitted when activated.
    pub action: Option<WidgetAction>,
}

/// Centered horizontal row of text buttons (`HorizontalButtons` in `src/menus/widgets.js`,
/// porting the 2004 `AddHorizontalButtons` used on `Popup_YesNo` and `Popup_Ok`).
#[derive(Clone, Debug)]
pub struct HorizontalButtonsWidget {
    /// Widget identifier (`"answer"` or `"buttons"`).
    pub id: String,
    /// Ordered list of horizontal button items.
    pub buttons: Vec<HorizontalButtonItem>,
    /// Index of the currently highlighted button (`1` = `"NO"` by default on `confirm_popup`).
    pub selected: usize,
    /// Vertical offset from the screen center (`63.0` units below center on popups).
    pub y: f64,
    /// Horizontal center-to-center spacing between adjacent buttons (`85.0` units in `classic/index.js`).
    pub spacing: f64,
    /// Bitmap font name (`"gamefont_medium"`).
    pub font: &'static str,
    /// When `true`, the button row is hidden.
    pub hidden: bool,
    /// When `true`, the button row cannot receive focus.
    pub disabled: bool,
}

impl HorizontalButtonsWidget {
    /// Creates a new [`HorizontalButtonsWidget`] at vertical offset `y` with `85.0`-unit button spacing.
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

    /// Steps the selected button index left (`dx < 0`) or right (`dx > 0`), wrapping around
    /// (`HorizontalButtons.key` in `src/menus/widgets.js`).
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
    /// Tab identifier (`"infantry"`, `"vehicle"`, `"starfighter"`).
    pub id: String,
    /// Localized uppercase label drawn on the orange `headerbutton*` bar.
    pub label: String,
    /// Left X offset in interface units.
    pub x: f64,
    /// Vertical center Y offset in interface units.
    pub y: f64,
    /// Width of the tab bar in interface units.
    pub width: f64,
    /// Whether the right triangle cap is vertically flipped.
    pub flipped: bool,
}

/// Horizontal tab bar (`Tabs` in `src/menus/widgets.js`, used for `INFANTRY` / `VEHICLE` / `STARFIGHTER`).
#[derive(Clone, Debug)]
pub struct TabsWidget {
    /// Widget identifier (`"tabs"`).
    pub id: String,
    /// Tab items along the header bar.
    pub tabs: Vec<TabItem>,
    /// Identifier of the currently active tab (`"infantry"`, `"vehicle"`, or `"starfighter"`).
    pub active: String,
    /// Index of the currently selected/active tab (`0..tabs.len()`).
    pub selected: usize,
    /// Vertical center Y position in safe-area units (`58.0` on `CONTROL OPTIONS`).
    pub y: f64,
    /// Height of each tab bar (`24.0` units: `y - 13.0..y + 11.0` in `paintTabs`).
    pub height: f64,
    /// Bitmap font name (`"gamefont_tiny"`).
    pub font: &'static str,
    /// When `true`, the tab bar is hidden.
    pub hidden: bool,
    /// When `true`, the tab bar cannot receive focus.
    pub disabled: bool,
}

impl TabsWidget {
    /// Creates a new [`TabsWidget`] with `selected` matching `active`.
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

    /// Steps to the previous or next tab (`Tabs.step` in `src/menus/widgets.js`) and returns the new active tab id.
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
    /// Setting key in `OPTIONS` (e.g., `"fov"`, `"renderScale"`, `"masterVolume"`, `"sensitivity"`).
    pub id: String,
    /// Localized uppercase option label drawn in the left column.
    pub label: String,
    /// Formatted current value string drawn in the right column or next to a slider.
    pub value_text: String,
    /// Option interaction kind (`Choice`, `Toggle`, `Slider`, `Action`, `Fixed`).
    pub kind: OptionKind,
    /// Normalized slider position (`0.0..=1.0`, `OptionRow.fraction()` in `widgets.js`).
    pub ratio: f64,
    /// Minimum numeric value for slider options.
    pub min: f64,
    /// Maximum numeric value for slider options.
    pub max: f64,
    /// Step increment for Left/Right arrow adjustments on slider options.
    pub step: f64,
    /// When `true`, renders the value in [`crate::theme::GREY`] and skips focus.
    pub disabled: bool,
    /// When `true` on a `Fixed` row, dims the value text to [`crate::theme::GREY`] (`row.dim` in `classic/index.js`).
    pub dim: bool,
    /// When `true`, hides the row from `paintOptions` and focus navigation.
    pub hidden: bool,
    /// Yellow footer note (`NOTE_YELLOW`) displayed below the rows while this item is focused.
    pub note: String,
    /// Optional action triggered on `Enter` / click for `OptionKind::Action` rows.
    pub action: Option<WidgetAction>,
}

/// Two-column `label | value` options widget (`Options` in `src/menus/widgets.js` and `paintOptions` in `classic/index.js`).
#[derive(Clone, Debug)]
pub struct OptionsWidget {
    /// Widget identifier (`"options"` or `"top"`).
    pub id: String,
    /// Page layout preset (`Game`, `Video`, `Audio`, `Controls`) selecting `OPTION_LOOKS` geometry.
    pub layout: OptionsLayoutKind,
    /// Option rows belonging to this page.
    pub items: Vec<OptionsItem>,
    /// Index into `items` of the currently focused option row.
    pub selected: usize,
    /// Index within the visible (`!hidden`) items of the top row in the scroll window.
    pub first: usize,
    /// Maximum number of visible rows that fit above the bottom buttons (`usize::MAX` when unpaginated).
    pub shown: usize,
    /// Normalized anchor `[ax, ay]`.
    pub at: [f64; 2],
    /// Whether `at` uses safe-area coordinates.
    pub safe: bool,
    /// Local X offset.
    pub x: f64,
    /// Local Y offset.
    pub y: f64,
    /// Optional explicit width.
    pub w: f64,
    /// Optional explicit height.
    pub h: f64,
    /// Vertical distance between row centers (`40.0` Game, `30.0` Video, `35.0` Audio, `24.5` Controls).
    pub pitch: f64,
    /// Bitmap font name (`"gamefont_tiny"` on `Controls`, `"gamefont_medium"` on all other pages).
    pub font: &'static str,
    /// When `true`, the widget is hidden.
    pub hidden: bool,
    /// When `true`, the widget cannot receive focus.
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
    /// Read-only `OptionKind::Fixed` rows (such as `"RENDER RESOLUTION"`) are never focusable.
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
    /// Only `VIDEO OPTIONS` (`look.bottom = 60`, `look.top = 32`, `look.pitch = 30`) paginates its rows.
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

    /// Moves selection by `dy` (`-1` or `+1`) across enabled items, returning `false` when
    /// `wrap` is `false` and `dy` steps past the first/last item.
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
    /// Option identifier key.
    pub id: String,
    /// Left-aligned label text.
    pub label: String,
    /// Right-aligned formatted value text.
    pub value_text: String,
    /// Option kind (`Slider`, `Choice`, `Toggle`, etc.).
    pub kind: OptionKind,
    /// Normalized slider position (`0.0..=1.0`).
    pub ratio: f64,
    /// Normalized anchor `[ax, ay]`.
    pub at: [f64; 2],
    /// Whether `at` is inside the 90% safe area.
    pub safe: bool,
    /// Left X offset in interface units.
    pub x: f64,
    /// Vertical center Y offset in interface units.
    pub y: f64,
    /// Total row width from label left to value right.
    pub width: f64,
    /// Slider bar height in interface units.
    pub height: f64,
    /// Horizontal width of the slider track when `kind == OptionKind::Slider`.
    pub slider_width: f64,
    /// Bitmap font name.
    pub font: &'static str,
    /// When `true`, renders in [`crate::theme::GREY`] and cannot be modified.
    pub disabled: bool,
    /// When `true`, omits the row from rendering and focus.
    pub hidden: bool,
}

/// Polymorphic menu widget enum (`Widget` subclasses in `src/menus/widgets.js`).
#[derive(Clone, Debug)]
pub enum Widget {
    /// Angled 3D stack of menu rows under an orange header bar (`Rows` in `widgets.js`).
    Rows(RowsWidget),
    /// Scrollable 9-slice list box (`List` in `widgets.js`).
    List(ListWidget),
    /// Bottom-corner or standalone text button (`Button` in `widgets.js`).
    Button(ButtonWidget),
    /// Static or flashy title/popup/note text (`Text` in `widgets.js`).
    Text(TextWidget),
    /// 9-slice `border_3_pieces` window frame (`Panel` in `widgets.js`).
    Panel(PanelWidget),
    /// Centered horizontal row of popup buttons (`HorizontalButtons` in `widgets.js`).
    HorizontalButtons(HorizontalButtonsWidget),
    /// Orange header-bar tab selector (`Tabs` in `widgets.js`).
    Tabs(TabsWidget),
    /// Two-column `label | value` option page list (`Options` in `widgets.js`).
    Options(OptionsWidget),
    /// Standalone single option row (`OptionRow` in `widgets.js`).
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

    /// Returns true if the widget is hidden (`widget.hidden` in `src/menus/widgets.js`).
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

    /// Returns true if the widget is disabled (`!widget.enabled()` in `src/menus/widgets.js`).
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

    /// Returns true if the widget can receive focus (`MenuScreen.ring()` in `src/menus/screen.js`).
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

    /// Advances any flashy intro timers on the widget (`MenuScreen.update(dt)` in `src/menus/screen.js`).
    pub fn update(&mut self, dt: f64) {
        match self {
            Self::Rows(w) => w.intro.step(dt),
            Self::Text(w) if w.flashy => w.intro.step(dt),
            _ => {}
        }
    }
}
