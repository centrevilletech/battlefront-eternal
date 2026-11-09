//! Option menu and its four sub-pages (`GAME`, `VIDEO`, `AUDIO`, `CONTROL OPTIONS`)
//! (`src/menus/option-pages.js`).
//!
//! Ports `TOP_LAYOUT`, `TOP_COUNT`, `optionsMenu`, `gamePage`, `videoPage`,
//! `audioPage`, `controlsPage`, `controlsWindow`, `CONTROLS_TABS`, and `bindingsLook`
//! from `src/menus/option-pages.js`, plus [`OptionsController`] for full keyboard/mouse
//! navigation, live setting updates, key-capture popup rebinding, and per-frame
//! screen state synchronization.

use crate::fonts::{FONT_LARGE, FONT_MEDIUM, FONT_TINY};
use crate::hotkeys::{
    default_flight_bindings, flat_bindings, flight_bindings, hotkey_rows, player_bindings, rebind,
    TABS,
};
use crate::input::{MenuInput, MenuInputEvent};
use crate::options::{
    format_option_value, option_entry, option_note, options_for, reset_page, say, step_option,
    OptionKind, PAGES,
};
use crate::popup::{key_popup, ok_popup, KeyCaptureOutcome};
use crate::screen::MenuScreen;
use crate::settings::{SettingValue, Settings};
use crate::stack::ScreenStack;
use crate::widgets::{
    ButtonWidget, ListColumn, ListEntry, ListLook, ListWidget, OptionsItem, OptionsLayoutKind,
    OptionsWidget, PanelWidget, RowItem, RowsLayout, RowsWidget, TabItem, TabsWidget, TextLook,
    TextWidget, Widget, WidgetAction,
};
use std::collections::HashMap;

/// Original `ifs_opt_top` layout (`TOP_LAYOUT` in `src/menus/option-pages.js`):
/// 8 rows in its list, turned 40 degrees around Y, `gamefont_large`, `width: 360`.
///
/// Used both in the front-end shell and over a paused battle (`in_battle: true`).
pub const TOP_LAYOUT: RowsLayout = RowsLayout {
    // Normalized safe-area anchor (`at: [0.6, 0.55]` in `src/menus/option-pages.js`).
    at: [0.6, 0.55],
    x: 0.0,
    // Downward offset in interface pixels (`y: 20` in `src/menus/option-pages.js`).
    y: 20.0,
    // Row pill width (`width: 360` in `src/menus/option-pages.js`).
    width: 360.0,
    rot_x: 0.0,
    // 3D Y-axis tilt matching `ifs_opt_top` (`rotY: 40`).
    rot_y: 40.0,
    rot_z: 0.0,
    xyz_order: false,
    font: FONT_LARGE,
    y_spacing: 5.0,
    // Fixed 8-slot centering count (`listCount: TOP_COUNT`).
    list_count: Some(TOP_COUNT),
};

/// Row count used for `rowsShift` on `ifs_opt_top` (`TOP_COUNT = 8` in `src/menus/option-pages.js`).
///
/// The 2004 game's `ifs_opt_top` reserved 8 slots (including 2 console-only rows
/// hidden on PC) when centering the vertical row stack.
pub const TOP_COUNT: usize = 8;

/// `bfe-shell` `rowScreen` layout (`at: [0.5, 0.5], y: 20, width: 350, rotY: 35, font: "gamefont_medium"`).
///
/// Alternative top-level menu layout matching the standalone shell's row screen proportions.
pub const SHELL_OPTIONS_LAYOUT: RowsLayout = RowsLayout {
    // Centered safe-area anchor (`[0.5, 0.5]`).
    at: [0.5, 0.5],
    x: 0.0,
    // Vertical offset (`y = 20.0`).
    y: 20.0,
    // Pill width (`350.0` interface units).
    width: 350.0,
    rot_x: 0.0,
    // 35-degree Y-axis tilt.
    rot_y: 35.0,
    rot_z: 0.0,
    xyz_order: false,
    // Medium menu font (`gamefont_medium`).
    font: FONT_MEDIUM,
    y_spacing: 5.0,
    // Uses actual visible row count when centering.
    list_count: None,
};

/// Safe-area width in interface coordinates (`SAFE_W = 0.9 * 1024 = 921.6` in `src/menus/option-pages.js`).
///
/// Forms the horizontal reference width for the `CONTROL OPTIONS` tab bar, outer
/// window frame, and scrollable bindings list.
pub const SAFE_W: f64 = 0.9 * 1024.0;

/// Tab bar geometry for `CONTROL OPTIONS` (`CONTROLS_TABS` in `src/menus/option-pages.js`):
/// `{ at: [0, 0], x: 16, y: 79, width: 0.30, spacing: 1/3, font: "gamefont_tiny" }`.
///
/// Each tuple stores `(tab_id, x_offset, tab_width, flipped)` in 1024x768 safe-area units.
pub const CONTROLS_TABS: [(&str, f64, f64, bool); 3] = [
    // Left tab: `INFANTRY` at `x = 16.0`, width `30%` of `SAFE_W`.
    ("infantry", 16.0, 0.30 * SAFE_W, false),
    // Middle tab: `VEHICLE` offset by `1/3` of `SAFE_W`.
    ("vehicle", 16.0 + (1.0 / 3.0) * SAFE_W, 0.30 * SAFE_W, false),
    // Right tab: `STARFIGHTER` offset by `2/3` of `SAFE_W`.
    (
        "starfighter",
        16.0 + (2.0 / 3.0) * SAFE_W,
        0.30 * SAFE_W,
        false,
    ),
];

/// Configuration passed to [`options_menu`] and [`OptionsController`].
///
/// Captures the localization dictionary, whether the menu is opened over an active
/// battle (`in_battle`), optional camera viewpoint state, and which front-end or
/// desktop-only rows (`online`, `integrations`, `credits`, `audioPanel`) are enabled.
#[derive(Clone, Debug)]
pub struct OptionsContext {
    /// Localized string table (`common.json` keys mapped to translated labels).
    pub strings: HashMap<String, String>,
    /// True when the options menu is opened over a paused battle rather than the front end.
    pub in_battle: bool,
    /// Optional battle camera viewpoint (`Some(true)` for 1st person, `Some(false)` for 3rd person).
    pub viewpoint: Option<bool>,
    /// When true, appends the `SPEAKER TEST` / `AUDIO DETAILS` action row on `AUDIO OPTIONS`.
    pub has_audio_panel: bool,
    /// When true, shows the `ONLINE OPTIONS` row on the top `OPTIONS` screen.
    pub has_online: bool,
    /// When true, shows the desktop `STEAM & DISCORD` integrations row on the top `OPTIONS` screen.
    pub has_integrations: bool,
    /// When true, shows the `CREDITS` row on the top `OPTIONS` screen.
    pub has_credits: bool,
    /// Row layout applied to the top-level `OPTIONS` menu screen.
    pub top_layout: RowsLayout,
    /// Virtual surface height in interface units (typically `768.0` at 4:3 or scaled for widescreen).
    pub surface_height: f64,
    /// Current physical viewport pixel dimensions `[width, height]` used by `renderScale` notes.
    pub viewport: Option<[u32; 2]>,
}

impl Default for OptionsContext {
    /// Returns default front-end context (`1024x768` surface, `TOP_LAYOUT`, online and credits enabled).
    fn default() -> Self {
        Self {
            // Empty localization dictionary (falls back to built-in English strings).
            strings: HashMap::new(),
            // Front-end shell mode by default (`in_battle: false`).
            in_battle: false,
            // No active battle camera viewpoint toggle in the front-end shell.
            viewpoint: None,
            // Speaker test panel row is omitted unless explicitly enabled.
            has_audio_panel: false,
            // Front-end shell shows `ONLINE OPTIONS` and `CREDITS`.
            has_online: true,
            has_integrations: false,
            has_credits: true,
            // Classic 40-degree `ifs_opt_top` row layout on a `1024x768` surface.
            top_layout: TOP_LAYOUT,
            surface_height: 768.0,
            viewport: Some([1024, 768]),
        }
    }
}

/// Builds the bottom-left `BACK` button (`backButton(strings)` in `src/menus/option-pages.js`).
///
/// Placed in the `"back"` slot at the bottom-left corner of every option screen,
/// dispatching [`WidgetAction::Back`] when clicked or triggered via `Escape`.
fn back_button(strings: &HashMap<String, String>) -> ButtonWidget {
    ButtonWidget::new(
        "back",
        "back",
        say(strings, Some("common.back"), "BACK"),
        Some(WidgetAction::Back),
    )
}

/// Builds the bottom-center `RESET TO DEFAULTS` button (`resetButton` in `src/menus/option-pages.js`).
///
/// Placed in the `"middle"` slot on `VIDEO`, `AUDIO`, and `CONTROL OPTIONS`,
/// dispatching [`WidgetAction::ResetPage`] for the given `page`.
fn reset_button(page: &str, strings: &HashMap<String, String>) -> ButtonWidget {
    ButtonWidget::new(
        "reset",
        "middle",
        say(strings, Some("common.reset"), "RESET TO DEFAULTS"),
        Some(WidgetAction::ResetPage(page.to_string())),
    )
}

/// Builds the top header title banner for `page` (`title(page, strings, y)` in `src/menus/option-pages.js`).
///
/// Resolves the page title key from [`PAGES`] (`ifs.GameOpt.title`, `ifs.VideoOpt.title`,
/// `ifs.SoundOpt.title`, or `ifs.GameOpt.pc_optionstitle`) and positions it at `y`.
fn page_title(page: &str, strings: &HashMap<String, String>, y: f64) -> TextWidget {
    let entry = PAGES.iter().find(|p| p.id == page).unwrap_or(&PAGES[0]);
    TextWidget::title("title", say(strings, Some(entry.title_key), entry.title), y)
}

/// Builds the top-level `OPTIONS` [`MenuScreen`] (`optionsMenu` in `src/menus/option-pages.js`).
///
/// Includes the four sub-page rows (`GAME OPTIONS`, `VIDEO OPTIONS`, `AUDIO OPTIONS`,
/// `CONTROL OPTIONS`) followed by `ONLINE OPTIONS`, `STEAM & DISCORD`, and `CREDITS`
/// (each hidden when its corresponding flag in `ctx` is false).
pub fn options_menu(ctx: &OptionsContext) -> MenuScreen {
    let strings = &ctx.strings;
    // Core sub-page navigation rows always shown in both front-end and in-battle menus.
    let mut row_items = vec![
        // Row 1: opens `GAME OPTIONS` (`gamePage` in `src/menus/option-pages.js`).
        RowItem::enabled(
            "general",
            say(strings, Some("ifs.GameOpt.title"), "GAME OPTIONS"),
            Some(WidgetAction::OpenPage("game".to_string())),
        ),
        // Row 2: opens `VIDEO OPTIONS` (`videoPage` in `src/menus/option-pages.js`).
        RowItem::enabled(
            "video",
            say(strings, Some("ifs.VideoOpt.title"), "VIDEO OPTIONS"),
            Some(WidgetAction::OpenPage("video".to_string())),
        ),
        // Row 3: opens `AUDIO OPTIONS` (`audioPage` in `src/menus/option-pages.js`).
        RowItem::enabled(
            "sound",
            say(strings, Some("ifs.SoundOpt.title"), "AUDIO OPTIONS"),
            Some(WidgetAction::OpenPage("audio".to_string())),
        ),
        // Row 4: opens `CONTROL OPTIONS` (`controlsPage` in `src/menus/option-pages.js`).
        RowItem::enabled(
            "controls",
            say(
                strings,
                Some("ifs.GameOpt.pc_optionstitle"),
                "CONTROL OPTIONS",
            ),
            Some(WidgetAction::OpenPage("controls".to_string())),
        ),
    ];

    // Optional front-end and desktop rows (hidden during an active battle when disabled in `ctx`).
    // Row 5: `ONLINE OPTIONS` (`ifs.onlineopt.title`), hidden during an active battle.
    let mut online_row = RowItem::enabled(
        "online",
        say(strings, Some("ifs.onlineopt.title"), "ONLINE OPTIONS"),
        Some(WidgetAction::Named("online".to_string())),
    );
    online_row.hidden = !ctx.has_online;
    row_items.push(online_row);

    // Row 6: desktop `STEAM & DISCORD` integrations (`src/shell/integrations.js`).
    let mut int_row = RowItem::enabled(
        "integrations",
        "STEAM & DISCORD",
        Some(WidgetAction::Named("integrations".to_string())),
    );
    int_row.hidden = !ctx.has_integrations;
    row_items.push(int_row);

    // Row 7: `CREDITS` (`ifs.credits.title`), hidden during an active battle.
    let mut credits_row = RowItem::enabled(
        "credits",
        say(strings, Some("ifs.credits.title"), "CREDITS"),
        Some(WidgetAction::Named("credits".to_string())),
    );
    credits_row.hidden = !ctx.has_credits;
    row_items.push(credits_row);

    let rows = RowsWidget::new(
        "options",
        say(strings, Some("ifs.Main.options"), "OPTIONS"),
        row_items,
        ctx.top_layout,
    );
    MenuScreen::new(
        "options",
        vec![Widget::Rows(rows), Widget::Button(back_button(strings))],
    )
}

/// Builds the [`OptionsItem`] rows for `page` (`"game"`, `"video"`, or `"audio"`),
/// mirroring `gamePage`, `videoPage`, and `audioPage` in `src/menus/option-pages.js`.
fn build_option_items(page: &str, settings: &Settings, ctx: &OptionsContext) -> Vec<OptionsItem> {
    let mut items = Vec::new();
    // When paused in a battle with a live camera viewpoint callback, prepend the `VIEWPOINT` row.
    if page == "game" {
        if let Some(fp) = ctx.viewpoint {
            let val_text = if fp {
                say(&ctx.strings, Some("ifs.GameOpt.cam_cockpit"), "1ST PERSON")
            } else {
                say(&ctx.strings, Some("ifs.GameOpt.cam_forward"), "3RD PERSON")
            };
            items.push(OptionsItem {
                id: "viewpoint".to_string(),
                label: say(&ctx.strings, Some("ifs.gameopt.camera"), "VIEWPOINT"),
                value_text: val_text,
                kind: OptionKind::Choice,
                ratio: 0.0,
                min: 0.0,
                max: 1.0,
                step: 1.0,
                disabled: false,
                dim: false,
                hidden: false,
                note: String::new(),
                action: Some(WidgetAction::Named("toggle_viewpoint".to_string())),
            });
        }
    }

    // Convert every schema entry belonging to `page` into a formatted `OptionsItem` row.
    for spec in options_for(page) {
        let label = say(&ctx.strings, spec.label_key, spec.label);
        let value_text = format_option_value(spec, settings, &ctx.strings);
        // Compute normalized `[0.0, 1.0]` fill ratio for slider bars.
        let ratio = if spec.kind == OptionKind::Slider {
            let v = settings.get_f64(spec.key);
            let denom = (spec.max - spec.min).max(1e-6);
            ((v - spec.min) / denom).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let dim = spec.key == "enhancedSound" && settings.get_string("soundEngine") != "rebuilt";
        let note = option_note(
            spec.key,
            settings,
            &ctx.strings,
            ctx.in_battle,
            ctx.viewport,
        );
        items.push(OptionsItem {
            id: spec.key.to_string(),
            label,
            value_text,
            kind: spec.kind,
            ratio,
            min: spec.min,
            max: spec.max,
            step: spec.step,
            disabled: spec.kind == OptionKind::Fixed,
            dim,
            hidden: false,
            note,
            action: None,
        });
    }

    // `GAME OPTIONS` places `RESET TO DEFAULTS` as its final list row rather than a bottom button.
    if page == "game" {
        items.push(OptionsItem {
            id: "reset".to_string(),
            label: String::new(),
            value_text: say(&ctx.strings, Some("common.reset"), "RESET TO DEFAULTS"),
            kind: OptionKind::Choice,
            ratio: 0.0,
            min: 0.0,
            max: 1.0,
            step: 1.0,
            disabled: false,
            dim: false,
            hidden: false,
            note: String::new(),
            action: Some(WidgetAction::ResetPage("game".to_string())),
        });
    } else if page == "audio" && ctx.has_audio_panel {
        // When an audio test panel is available, append `SPEAKER TEST` / `AUDIO DETAILS`.
        items.push(OptionsItem {
            id: "panel".to_string(),
            label: "SPEAKER TEST".to_string(),
            value_text: "AUDIO DETAILS".to_string(),
            kind: OptionKind::Choice,
            ratio: 0.0,
            min: 0.0,
            max: 1.0,
            step: 1.0,
            disabled: false,
            dim: false,
            hidden: false,
            note: String::new(),
            action: Some(WidgetAction::Named("audioPanel".to_string())),
        });
    }
    items
}

/// Builds the `GAME OPTIONS` [`MenuScreen`] (`gamePage` in `src/menus/option-pages.js`).
///
/// Dims the background behind the screen, places the title at `y = 10.0`, and includes
/// the `GAME` option rows (with `RESET TO DEFAULTS` as the last row) and `BACK`.
pub fn game_page(settings: &Settings, ctx: &OptionsContext) -> MenuScreen {
    let items = build_option_items("game", settings, ctx);
    let options =
        OptionsWidget::with_layout("rows", OptionsLayoutKind::Game, items, ctx.surface_height);
    let mut screen = MenuScreen::new(
        "options-game",
        vec![
            Widget::Text(page_title("game", &ctx.strings, 10.0)),
            Widget::Options(options),
            Widget::Button(back_button(&ctx.strings)),
        ],
    );
    screen.dim_behind = true;
    screen
}

/// Builds the `VIDEO OPTIONS` [`MenuScreen`] (`videoPage` in `src/menus/option-pages.js`).
///
/// Dims the background, places the title at `y = -30.0`, and provides a scrollable list
/// of all 31 rendering settings along with `BACK` and a middle `RESET TO DEFAULTS` button.
pub fn video_page(settings: &Settings, ctx: &OptionsContext) -> MenuScreen {
    let items = build_option_items("video", settings, ctx);
    let options =
        OptionsWidget::with_layout("rows", OptionsLayoutKind::Video, items, ctx.surface_height);
    let mut screen = MenuScreen::new(
        "options-video",
        vec![
            Widget::Text(page_title("video", &ctx.strings, -30.0)),
            Widget::Options(options),
            Widget::Button(back_button(&ctx.strings)),
            Widget::Button(reset_button("video", &ctx.strings)),
        ],
    );
    screen.dim_behind = true;
    screen
}

/// Builds the `AUDIO OPTIONS` [`MenuScreen`] (`audioPage` in `src/menus/option-pages.js`).
///
/// Dims the background, places the title at `y = 0.0`, and lists volume sliders,
/// sound engine/output/dynamics options, `BACK`, and `RESET TO DEFAULTS`.
pub fn audio_page(settings: &Settings, ctx: &OptionsContext) -> MenuScreen {
    let items = build_option_items("audio", settings, ctx);
    let options =
        OptionsWidget::with_layout("rows", OptionsLayoutKind::Audio, items, ctx.surface_height);
    let mut screen = MenuScreen::new(
        "options-audio",
        vec![
            Widget::Text(page_title("audio", &ctx.strings, 0.0)),
            Widget::Options(options),
            Widget::Button(back_button(&ctx.strings)),
            Widget::Button(reset_button("audio", &ctx.strings)),
        ],
    );
    screen.dim_behind = true;
    screen
}

/// Window metrics for `CONTROL OPTIONS` (`controlsWindow` in `src/menus/option-pages.js`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControlsWindow {
    /// Outer panel `[x, y, w, h]` anchored at `[0.5, 0.0]` (`controlsWindow(surface)` in `option-pages.js`).
    pub window_box: [f64; 4],
    /// Vertical position of the `"CONFIG BINDINGS"` section heading (`186.0`).
    pub heading_y: f64,
    /// Vertical position of the `INFANTRY` / `VEHICLE` / `STARFIGHTER` tab bar (`79.0`).
    pub tab_y: f64,
    /// Bindings list box `[x, y, w, h]` anchored at `[0.5, 0.0]` (`bindingsLook(surface, extra)` in `option-pages.js`).
    pub list_box: [f64; 4],
}

/// Computes `controlsWindow(surface)` (`src/menus/option-pages.js`) for ground/vehicle tabs (`extra = 0.0`).
pub fn controls_window(surface_height: f64) -> ControlsWindow {
    controls_window_for_tab(surface_height, false)
}

/// Computes `controlsWindow(surface)` and `bindingsLook(surface, extra)` (`src/menus/option-pages.js`).
///
/// When `flying` is true (`"starfighter"` tab), shifts the bindings list top down by
/// `extra = 24.5` units to make room for the third top option row (`flightResponseCurve`).
pub fn controls_window_for_tab(surface_height: f64, flying: bool) -> ControlsWindow {
    // Outer window top starts at `y = 89` (just under the `y = 79` tab bar) and reaches `surface.height`.
    let top = 89.0;
    let bottom = surface_height;
    // Starfighter tab displays 3 top rows instead of 2, pushing the list down by `24.5` units.
    let extra = if flying { 24.5 } else { 0.0 };
    // Safe-area vertical span (`0.9 * surface.height`).
    let h = 0.9 * surface_height;
    // Bindings list height (`0.95 * h - 240 - extra`), clamped to at least `120.0` units.
    let list_h = (0.95 * h - 240.0 - extra).max(120.0);
    ControlsWindow {
        // Center-anchored `[x, y_center, width, height]` box for the outer frame panel.
        window_box: [0.0, (top + bottom) * 0.5, SAFE_W + 42.0, bottom - top],
        heading_y: 186.0,
        tab_y: 79.0,
        // Center-anchored `[x, y_center, width, height]` box for the bindings list.
        list_box: [0.0, 220.0 + extra + list_h * 0.5, SAFE_W, list_h],
    }
}

/// Computes `bindingsLook(surface)` (`src/menus/option-pages.js`) for ground/vehicle tabs (`extra = 0.0`).
pub fn bindings_look(surface_height: f64) -> ListLook {
    bindings_look_for_tab(surface_height, false)
}

/// Computes `bindingsLook(surface, extra)` (`src/menus/option-pages.js`).
///
/// Configures the two-column bindings list with `gamefont_tiny`, `pitch: 22.6`,
/// `inset: 14.0`, `pad_left: 16.0`, and `pad_right: 20.0`.
pub fn bindings_look_for_tab(surface_height: f64, flying: bool) -> ListLook {
    let cw = controls_window_for_tab(surface_height, flying);
    ListLook {
        // Horizontally centered at top safe-area anchor (`at: [0.5, 0]`).
        at: [0.5, 0.0],
        x: 0.0,
        y: 0.0,
        rot_y: 0.0,
        box_x: cw.list_box[0],
        box_y: cw.list_box[1],
        box_w: cw.list_box[2],
        box_h: cw.list_box[3],
        // Compact font (`gamefont_tiny`) and `22.6` row pitch matching `bindingsLook` in JS.
        font: FONT_TINY,
        pitch: 22.6,
        // Vertical frame inset and horizontal column padding (`inset: 14, padLeft: 16, padRight: 20`).
        inset: 14.0,
        pad_left: 16.0,
        pad_right: 20.0,
        show_frame: true,
    }
}

/// Builds the top option rows above the `CONFIG BINDINGS` list on `CONTROL OPTIONS`
/// (`rows()` in `controlsPage` in `src/menus/option-pages.js`).
///
/// Shows `mouseSensitivity` and `invertMouse` on `INFANTRY` / `VEHICLE` tabs, or
/// `flightSensitivity`, `flightInvertY`, and `flightResponseCurve` on `STARFIGHTER`.
fn build_controls_top_items(
    flying: bool,
    settings: &Settings,
    strings: &HashMap<String, String>,
) -> Vec<OptionsItem> {
    // Starfighter tab exposes 3 flight tuning controls; infantry/vehicle tabs expose 2 mouse controls.
    let keys: &[&str] = if flying {
        &["flightSensitivity", "flightInvertY", "flightResponseCurve"]
    } else {
        &["mouseSensitivity", "invertMouse"]
    };
    let mut items = Vec::with_capacity(keys.len());
    for &key in keys {
        if let Some(spec) = option_entry(key) {
            // Format current setting value and compute `[0.0, 1.0]` slider fill ratio.
            let val_text = format_option_value(spec, settings, strings);
            let v = settings.get_f64(spec.key);
            let denom = (spec.max - spec.min).max(1e-6);
            let ratio = if spec.kind == OptionKind::Slider {
                ((v - spec.min) / denom).clamp(0.0, 1.0)
            } else {
                0.0
            };
            items.push(OptionsItem {
                id: spec.key.to_string(),
                label: say(strings, spec.label_key, spec.label),
                value_text: val_text,
                kind: spec.kind,
                ratio,
                min: spec.min,
                max: spec.max,
                step: spec.step,
                disabled: false,
                dim: false,
                hidden: false,
                note: String::new(),
                action: None,
            });
        }
    }
    items
}

/// Builds the two-column [`ListEntry`] rows for `tab` in the `CONTROL OPTIONS` bindings list
/// (`bindings()` in `controlsPage` in `src/menus/option-pages.js`).
fn build_controls_binding_entries(
    tab: &str,
    settings: &Settings,
    strings: &HashMap<String, String>,
) -> Vec<ListEntry> {
    // Select `flightBindings` on the `starfighter` tab and `keyBindings` on `infantry`/`vehicle`.
    let bindings = if tab == "starfighter" {
        let saved_val = settings.get("flightBindings");
        flight_bindings(saved_val.as_bindings())
    } else {
        let saved_val = settings.get("keyBindings");
        player_bindings(saved_val.as_bindings())
    };
    hotkey_rows(tab, &bindings, strings)
        .into_iter()
        .map(|row| {
            // Rebindable rows open the key capture popup when accepted (`Enter` or double-click).
            let act = row.action.as_ref().map(|a| WidgetAction::StartRebind {
                tab: tab.to_string(),
                action: a.clone(),
                label: row.label.clone(),
            });
            let id = row.action.unwrap_or_else(|| row.label.clone());
            ListEntry::two_col(id, row.label, row.keys, row.disabled, act)
        })
        .collect()
}

/// Builds the `CONTROL OPTIONS` [`MenuScreen`] (`controlsPage` in `src/menus/option-pages.js`).
///
/// Mirrors the 2004 game's `ifs_opt_pccontrols` layout: outer window panel, three-tab
/// bar (`INFANTRY`, `VEHICLE`, `STARFIGHTER`), top sensitivity/invert rows,
/// `"CONFIG BINDINGS"` heading, two-column scrollable bindings list, `BACK`, and
/// `RESET TO DEFAULTS`.
pub fn controls_page(settings: &Settings, ctx: &OptionsContext, active_tab: &str) -> MenuScreen {
    let flying = active_tab == "starfighter";
    let cw = controls_window_for_tab(ctx.surface_height, flying);

    // Outer frame panel spanning from below the tab bar to the bottom of the surface.
    let mut window = PanelWidget::new(
        "window",
        cw.window_box[0],
        cw.window_box[1],
        cw.window_box[2],
        cw.window_box[3],
    );
    window.at = [0.5, 0.0];

    // Three-tab selector bar (`INFANTRY`, `VEHICLE`, `STARFIGHTER`) along the top of the window.
    let tab_items: Vec<TabItem> = CONTROLS_TABS
        .iter()
        .map(|&(id, x, width, flipped)| {
            let entry = TABS.iter().find(|t| t.id == id).unwrap_or(&TABS[0]);
            TabItem {
                id: id.to_string(),
                label: say(&ctx.strings, Some(entry.label_key), entry.label),
                x,
                y: cw.tab_y,
                width,
                flipped,
            }
        })
        .collect();
    let tabs = TabsWidget::new("tabs", tab_items, active_tab, cw.tab_y, 24.0, FONT_TINY);

    // Sensitivity and Y-invert option rows above the bindings list.
    let top_opts = OptionsWidget::with_layout(
        "rows",
        OptionsLayoutKind::Controls,
        build_controls_top_items(flying, settings, &ctx.strings),
        ctx.surface_height,
    );

    // Static `"CONFIG BINDINGS"` heading centered above the bindings list box.
    let heading = TextWidget {
        id: "heading".to_string(),
        look: TextLook::Heading,
        text: say(
            &ctx.strings,
            Some("ifs.GameOpt.pc_configbindings"),
            "CONFIG BINDINGS",
        ),
        at: [0.5, 0.0],
        safe: true,
        x: 0.0,
        y: cw.heading_y,
        align: 2,
        font: Some(FONT_LARGE),
        color: None,
        alpha: 1.0,
        flashy: false,
        intro: crate::flashy::TitleIntro::completed(),
        hidden: false,
    };

    // Two-column bindings list (`x = 0.0` action name, `x = 0.4` bound key labels).
    let list_entries = build_controls_binding_entries(active_tab, settings, &ctx.strings);
    let list = ListWidget::new(
        "bindings",
        list_entries,
        vec![
            ListColumn {
                width: 0.0,
                align: 0,
            },
            ListColumn {
                width: 0.4,
                align: 0,
            },
        ],
        bindings_look_for_tab(ctx.surface_height, flying),
    );

    // Assemble the 8 widgets in exact `controlsPage` draw order.
    let mut screen = MenuScreen::new(
        "options-controls",
        vec![
            // 1. Page title (`CONTROL OPTIONS` at `y = 5`).
            Widget::Text(page_title("controls", &ctx.strings, 5.0)),
            // 2. Outer frame window (`controlsWindow`).
            Widget::Panel(window),
            // 3. `INFANTRY` / `VEHICLE` / `STARFIGHTER` tab strip.
            Widget::Tabs(tabs),
            // 4. Top sensitivity and invert rows (`id: "rows"`).
            Widget::Options(top_opts),
            // 5. `"CONFIG BINDINGS"` section heading (`y = 186`).
            Widget::Text(heading),
            // 6. Scrollable two-column key bindings list (`id: "bindings"`).
            Widget::List(list),
            // 7. Bottom-left `BACK` button.
            Widget::Button(back_button(&ctx.strings)),
            // 8. Bottom-center `RESET TO DEFAULTS` button.
            Widget::Button(reset_button("controls", &ctx.strings)),
        ],
    );
    // Dim the underlying menu/battle view and start keyboard focus on the top option rows.
    screen.dim_behind = true;
    screen.set_focus_by_id("rows");
    screen
}

/// Complete stateful controller for the `OPTIONS` menu and its four sub-pages
/// (`GAME`, `VIDEO`, `AUDIO`, `CONTROL OPTIONS`).
#[derive(Clone, Debug)]
pub struct OptionsController {
    /// Active menu screen stack (root `options` screen, open sub-page, and any modal popup).
    pub stack: ScreenStack,
    /// Live player settings modified by option rows, sliders, resets, and key rebinding.
    pub settings: Settings,
    /// Display, localization, and feature-availability context for building screens.
    pub context: OptionsContext,
    /// Keyboard and mouse input state machine driving [`Self::stack`].
    pub input: MenuInput,
    /// Currently selected tab id (`"infantry"`, `"vehicle"`, or `"starfighter"`) on `CONTROL OPTIONS`.
    pub controls_tab: String,
    /// Most recent host-handled named action (`"online"`, `"integrations"`, `"audioPanel"`).
    pub last_named_action: Option<String>,
}

impl OptionsController {
    /// Creates a new [`OptionsController`] starting on the top `OPTIONS` screen.
    pub fn new(settings: Settings, context: OptionsContext) -> Self {
        let mut stack = ScreenStack::new();
        stack.push(options_menu(&context));
        // Clear the initial push sound so constructing the controller is silent.
        stack.sounds.drain();
        Self {
            stack,
            settings,
            context,
            input: MenuInput::new(),
            controls_tab: "infantry".to_string(),
            last_named_action: None,
        }
    }

    /// Opens a specific sub-page (`"game"`, `"video"`, `"audio"`, `"controls"`).
    pub fn open_page(&mut self, page: &str) {
        // Construct the requested sub-page screen from current settings and context.
        let screen = match page {
            // `GAME OPTIONS` (`options-game`).
            "game" => game_page(&self.settings, &self.context),
            // `VIDEO OPTIONS` (`options-video`).
            "video" => video_page(&self.settings, &self.context),
            // `AUDIO OPTIONS` (`options-audio`).
            "audio" => audio_page(&self.settings, &self.context),
            // `CONTROL OPTIONS` (`options-controls`), opening on the last active controls tab.
            "controls" => controls_page(&self.settings, &self.context, &self.controls_tab),
            // Ignore unknown page identifiers.
            _ => return,
        };
        self.stack.push(screen);
    }

    /// Synchronizes the active screen's option values and footer note after a setting change.
    pub fn refresh_active_screen(&mut self) {
        let Some(top) = self.stack.top_mut() else {
            return;
        };
        let screen_id = top.id.clone();
        let page = match screen_id.as_str() {
            "options-game" => Some("game"),
            "options-video" => Some("video"),
            "options-audio" => Some("audio"),
            _ => None,
        };
        // Rebuild `OptionsItem` rows on `GAME`, `VIDEO`, or `AUDIO` while preserving scroll & selection.
        if let Some(p) = page {
            let new_items = build_option_items(p, &self.settings, &self.context);
            if let Some(Widget::Options(opts)) = top.widget_mut("rows") {
                // Clamp selection index in case row count changed and keep visible scroll window.
                let sel = opts.selected.min(new_items.len().saturating_sub(1));
                let first = opts.first;
                opts.items = new_items;
                opts.selected = sel;
                opts.first = first;
                // Recompute visible row capacity for `surface_height` and ensure `selected` is in view.
                opts.update_shown(self.context.surface_height);
                opts.reveal();
            }
        } else if screen_id == "options-controls" {
            // On `CONTROL OPTIONS`, refresh the top sensitivity rows, the bindings list, and tab state.
            let flying = self.controls_tab == "starfighter";
            // Rebuild the 2 mouse rows (`infantry`/`vehicle`) or 3 flight rows (`starfighter`).
            let new_top = build_controls_top_items(flying, &self.settings, &self.context.strings);
            if let Some(Widget::Options(opts)) = top.widget_mut("rows") {
                let sel = opts.selected.min(new_top.len().saturating_sub(1));
                opts.items = new_top;
                opts.selected = sel;
            }
            // Rebuild the two-column hotkey rows for the active tab and adjust list box height (`extra`).
            let new_bindings = build_controls_binding_entries(
                &self.controls_tab,
                &self.settings,
                &self.context.strings,
            );
            if let Some(Widget::List(list)) = top.widget_mut("bindings") {
                let sel = list.selected.min(new_bindings.len().saturating_sub(1));
                let first = list.first;
                list.rows = new_bindings;
                list.look = bindings_look_for_tab(self.context.surface_height, flying);
                list.selected = sel;
                list.first = first;
                list.reveal();
            }
            // Synchronize the `TabsWidget` active id and highlighted tab index.
            if let Some(Widget::Tabs(tabs)) = top.widget_mut("tabs") {
                tabs.active = self.controls_tab.clone();
                if let Some(pos) = tabs.tabs.iter().position(|t| t.id == self.controls_tab) {
                    tabs.selected = pos;
                }
            }
        }
    }

    /// Dispatches a [`MenuInputEvent`] produced by [`MenuInput`].
    pub fn apply_event(&mut self, event: MenuInputEvent) {
        match event {
            MenuInputEvent::None => {}
            MenuInputEvent::SelectionChanged => {
                // Selection and wheel handlers already updated their widgets.
                // Rebuilding here would restore classic row capacity and undo
                // wheel scrolling when its selected row stays off-screen.
            }
            // Left/Right arrow or click on a choice, toggle, or slider row.
            MenuInputEvent::StepOption { key, dir } => {
                if let Some(spec) = option_entry(&key) {
                    if step_option(spec, &mut self.settings, dir) {
                        self.refresh_active_screen();
                    }
                }
            }
            // Mouse drag or click along a slider bar.
            MenuInputEvent::SetOptionRatio { key, ratio } => {
                if let Some(spec) = option_entry(&key) {
                    if spec.kind == OptionKind::Slider {
                        let raw = spec.min + ratio.clamp(0.0, 1.0) * (spec.max - spec.min);
                        let stepped =
                            ((raw / spec.step).round() * spec.step * 1000.0).round() / 1000.0;
                        self.settings
                            .set(spec.key, stepped.clamp(spec.min, spec.max));
                        self.refresh_active_screen();
                    }
                }
            }
            // Button activation, row action, tab switch, or rebind trigger.
            MenuInputEvent::Action(action) => match action {
                // Push one of the four option sub-pages (`game`, `video`, `audio`, `controls`).
                WidgetAction::OpenPage(page) => self.open_page(&page),
                // Pop the current sub-page or modal popup and refresh the newly exposed top screen.
                WidgetAction::Back => {
                    self.stack.pop();
                    self.refresh_active_screen();
                }
                // Restore all settings on `page` back to `DEFAULTS`.
                WidgetAction::ResetPage(page) => {
                    // Resetting `controls` also restores the active tab's binding map to defaults.
                    if page == "controls" {
                        reset_page("controls", &mut self.settings);
                        let key = if self.controls_tab == "starfighter" {
                            "flightBindings"
                        } else {
                            "keyBindings"
                        };
                        self.settings.reset_keys([key]);
                    } else {
                        reset_page(&page, &mut self.settings);
                    }
                    self.refresh_active_screen();
                }
                // Open the `PRESS NEW KEY, ESC TO CANCEL` modal popup for a rebindable row.
                WidgetAction::StartRebind { tab, action, label } => {
                    let popup = key_popup(&tab, &action, &label, &self.context.strings);
                    self.stack.push(popup);
                }
                // Switch active `CONTROL OPTIONS` tab and reset bindings list scroll to row 0.
                WidgetAction::SelectTab(tab) => {
                    self.controls_tab = tab;
                    if let Some(top) = self.stack.top_mut() {
                        if let Some(Widget::List(list)) = top.widget_mut("bindings") {
                            list.selected = 0;
                            list.first = 0;
                        }
                    }
                    self.refresh_active_screen();
                }
                // Handle in-battle viewpoint toggle, credits modal popup, or external host actions.
                WidgetAction::Named(name) => {
                    if name == "toggle_viewpoint" {
                        if let Some(vp) = &mut self.context.viewpoint {
                            *vp = !*vp;
                        }
                        self.refresh_active_screen();
                    } else if name == "credits" {
                        let mut popup = ok_popup(
                            "BATTLEFRONT ETERNAL\nA fan-made mod for the Steam versions of\nStar Wars: Battlefront (2004) and Battlefront II (2005).\nBattlefront II support is planned.",
                            &self.context.strings,
                        );
                        // Credits overlays the hub, as ScreenView.buildPopup
                        // does, so its paragraph and OK button share a modal box.
                        popup.modal = true;
                        self.stack.push(popup);
                    } else {
                        self.last_named_action = Some(name);
                    }
                }
            },
            // Key capture popup result (`Escape` cancels, bindable key assigns, `Delete` clears).
            MenuInputEvent::KeyCapture(outcome) => match outcome {
                // `Escape` dismisses the key capture popup without changing bindings.
                KeyCaptureOutcome::Cancelled => {
                    self.stack.pop();
                }
                // A bindable key assigns `[code]` exclusively to `action` and closes the popup.
                KeyCaptureOutcome::Bound { tab, action, code } => {
                    self.apply_rebind(&tab, &action, Some(&code));
                    self.stack.pop();
                    self.refresh_active_screen();
                }
                // Clear gesture unbinds `action` (`[]`) and closes the popup.
                KeyCaptureOutcome::Cleared { tab, action } => {
                    self.apply_rebind(&tab, &action, None);
                    self.stack.pop();
                    self.refresh_active_screen();
                }
                // Reserved keys (such as `Tab` or `F12`) are ignored while the popup stays open.
                KeyCaptureOutcome::Ignored => {}
            },
        }
    }

    /// Applies a single-key rebind (or clear when `code` is `None`) to `flightBindings`
    /// on the `"starfighter"` tab or `keyBindings` on `"infantry"` / `"vehicle"`.
    ///
    /// Mirrors the `assign` callback in `controlsPage` (`src/menus/option-pages.js`),
    /// computing the diff against `default_flight_bindings()` or `flat_bindings()`
    /// via [`rebind`] and persisting the updated map in `self.settings`.
    fn apply_rebind(&mut self, tab: &str, action: &str, code: Option<&str>) {
        if tab == "starfighter" {
            // Starfighter tab stores its custom binding overrides under `flightBindings`.
            let defaults = default_flight_bindings();
            let cur = self.settings.get("flightBindings");
            let next = rebind(&defaults, cur.as_bindings(), action, code);
            self.settings
                .set("flightBindings", SettingValue::Bindings(next));
        } else {
            // Infantry and vehicle tabs share the ground/vehicle `keyBindings` setting.
            let defaults = flat_bindings();
            let cur = self.settings.get("keyBindings");
            let next = rebind(&defaults, cur.as_bindings(), action, code);
            self.settings
                .set("keyBindings", SettingValue::Bindings(next));
        }
    }

    /// Processes a keyboard key press through [`MenuInput`] and applies the resulting event.
    pub fn on_key_down(&mut self, code: &str, shift: bool) {
        let ev = self.input.on_key_down(&mut self.stack, code, shift);
        self.apply_event(ev);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_root_swallows_backspace_but_escape_and_back_button_leave() {
        let mut ctrl = OptionsController::new(Settings::new(), OptionsContext::default());
        ctrl.on_key_down("Backspace", false);
        assert_eq!(ctrl.stack.top().unwrap().id, "options");
        ctrl.open_page("game");
        ctrl.on_key_down("Backspace", false);
        assert_eq!(ctrl.stack.top().unwrap().id, "options-game");
        ctrl.on_key_down("Escape", false);
        assert_eq!(ctrl.stack.top().unwrap().id, "options");
        ctrl.on_key_down("Escape", false);
        assert!(ctrl.stack.is_empty());
        let mut ctrl = OptionsController::new(Settings::new(), OptionsContext::default());
        ctrl.apply_event(MenuInputEvent::Action(WidgetAction::Back));
        assert!(ctrl.stack.is_empty());
    }

    #[test]
    fn credits_is_modal_and_back_preserves_the_options_hub() {
        let mut ctrl = OptionsController::new(Settings::new(), OptionsContext::default());
        ctrl.stack.update(2.);
        ctrl.apply_event(MenuInputEvent::Action(WidgetAction::Named(
            "credits".into(),
        )));
        assert!(ctrl.stack.top().unwrap().modal);
        ctrl.on_key_down("Escape", false);
        assert_eq!(ctrl.stack.top().unwrap().id, "options");
        assert_eq!(ctrl.stack.top().unwrap().elapsed, 2.);
    }

    // Verifies that all four option sub-pages build with expected row counts and navigate back with Escape.
    #[test]
    fn option_pages_build_and_navigate_all_four_pages() -> Result<(), Box<dyn std::error::Error>> {
        let settings = Settings::new();
        let ctx = OptionsContext::default();
        let mut ctrl = OptionsController::new(settings, ctx);

        assert_eq!(ctrl.stack.top().ok_or("missing options top")?.id, "options");

        // Open GAME OPTIONS
        ctrl.open_page("game");
        let top = ctrl.stack.top().ok_or("missing options-game top")?;
        assert_eq!(top.id, "options-game");
        if let Some(Widget::Options(opts)) = top.widget("rows") {
            // 8 game options + 1 RESET TO DEFAULTS row = 9 rows
            assert_eq!(opts.items.len(), 9);
            assert_eq!(opts.items[0].id, "flightFirstPerson");
            assert_eq!(opts.items[8].id, "reset");
        } else {
            return Err("expected Options widget on gamePage".into());
        }
        ctrl.on_key_down("Escape", false);
        assert_eq!(ctrl.stack.top().ok_or("missing options top")?.id, "options");

        // Open VIDEO OPTIONS and verify all 31 rendering rows (`performancePreset`..`deathCamera`).
        ctrl.open_page("video");
        let top = ctrl.stack.top().ok_or("missing options-video top")?;
        assert_eq!(top.id, "options-video");
        if let Some(Widget::Options(opts)) = top.widget("rows") {
            // 31 video options in exact `src/menus/options.js` order.
            assert_eq!(opts.items.len(), 31);
            assert_eq!(opts.items[0].id, "performancePreset");
            assert_eq!(opts.items[30].id, "deathCamera");
        } else {
            return Err("expected Options widget on videoPage".into());
        }
        // Pressing `Escape` pops `options-video` and returns to `options`.
        ctrl.on_key_down("Escape", false);
        assert_eq!(ctrl.stack.top().ok_or("missing options top")?.id, "options");

        // Open AUDIO OPTIONS and verify all 11 audio rows (`masterVolume`..`audioClarity`).
        ctrl.open_page("audio");
        let top = ctrl.stack.top().ok_or("missing options-audio top")?;
        assert_eq!(top.id, "options-audio");
        if let Some(Widget::Options(opts)) = top.widget("rows") {
            // 11 audio options when `has_audio_panel` is false.
            assert_eq!(opts.items.len(), 11);
            assert_eq!(opts.items[0].id, "masterVolume");
            assert_eq!(opts.items[10].id, "audioClarity");
        } else {
            return Err("expected Options widget on audioPage".into());
        }
        // Pressing `Escape` pops `options-audio` and returns to `options`.
        ctrl.on_key_down("Escape", false);
        assert_eq!(ctrl.stack.top().ok_or("missing options top")?.id, "options");

        // Open CONTROL OPTIONS and verify the bindings list is populated.
        ctrl.open_page("controls");
        let top = ctrl.stack.top().ok_or("missing options-controls top")?;
        assert_eq!(top.id, "options-controls");
        // Default tab is `infantry`, which populates the two-column bindings list widget.
        if let Some(Widget::List(list)) = top.widget("bindings") {
            assert!(!list.rows.is_empty());
        } else {
            return Err("expected bindings list on controlsPage".into());
        }
        Ok(())
    }
}
