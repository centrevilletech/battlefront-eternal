//! Option menu and its four sub-pages (`GAME`, `VIDEO`, `AUDIO`, `CONTROL OPTIONS`)
//! (`src/menus/option-pages.js`).
//!
//! Ports `TOP_LAYOUT`, `TOP_COUNT`, `optionsMenu`, `gamePage`, `videoPage`,
//! `audioPage`, `controlsPage`, `controlsWindow`, `CONTROLS_TABS`, and `bindingsLook`
//! from `src/menus/option-pages.js`, plus [`OptionsController`] for full keyboard/mouse
//! navigation and zero-allocation per-frame rendering.

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
/// 8 rows in its list, turned 40 degrees, `gamefont_large`, `width: 360`.
pub const TOP_LAYOUT: RowsLayout = RowsLayout {
    at: [0.6, 0.55],
    x: 0.0,
    y: 20.0,
    width: 360.0,
    rot_x: 0.0,
    rot_y: 40.0,
    rot_z: 0.0,
    xyz_order: false,
    font: FONT_LARGE,
    y_spacing: 5.0,
    list_count: Some(TOP_COUNT),
};

/// Row count used for `rowsShift` on `ifs_opt_top` (`TOP_COUNT = 8` in `src/menus/option-pages.js`).
pub const TOP_COUNT: usize = 8;

/// `bfe-shell` `rowScreen` layout (`at: [0.5, 0.5], y: 20, width: 350, rotY: 35, font: "gamefont_medium"`).
pub const SHELL_OPTIONS_LAYOUT: RowsLayout = RowsLayout {
    at: [0.5, 0.5],
    x: 0.0,
    y: 20.0,
    width: 350.0,
    rot_x: 0.0,
    rot_y: 35.0,
    rot_z: 0.0,
    xyz_order: false,
    font: FONT_MEDIUM,
    y_spacing: 5.0,
    list_count: None,
};

/// Safe-area width in interface coordinates (`SAFE_W = 0.9 * 1024 = 921.6` in `src/menus/option-pages.js`).
pub const SAFE_W: f64 = 0.9 * 1024.0;

/// Tab bar geometry for `CONTROL OPTIONS` (`CONTROLS_TABS` in `src/menus/option-pages.js`):
/// `{ at: [0, 0], x: 16, y: 79, width: 0.30, spacing: 1/3, font: "gamefont_tiny" }`.
pub const CONTROLS_TABS: [(&str, f64, f64, bool); 3] = [
    ("infantry", 16.0, 0.30 * SAFE_W, false),
    ("vehicle", 16.0 + (1.0 / 3.0) * SAFE_W, 0.30 * SAFE_W, false),
    ("starfighter", 16.0 + (2.0 / 3.0) * SAFE_W, 0.30 * SAFE_W, false),
];

/// Configuration passed to [`options_menu`] and [`OptionsController`].
#[derive(Clone, Debug)]
pub struct OptionsContext {
    pub strings: HashMap<String, String>,
    pub in_battle: bool,
    pub viewpoint: Option<bool>,
    pub has_audio_panel: bool,
    pub has_online: bool,
    pub has_integrations: bool,
    pub has_credits: bool,
    pub top_layout: RowsLayout,
    pub surface_height: f64,
    pub viewport: Option<[u32; 2]>,
}

impl Default for OptionsContext {
    fn default() -> Self {
        Self {
            strings: HashMap::new(),
            in_battle: false,
            viewpoint: None,
            has_audio_panel: false,
            has_online: true,
            has_integrations: false,
            has_credits: true,
            top_layout: TOP_LAYOUT,
            surface_height: 768.0,
            viewport: Some([1024, 768]),
        }
    }
}

fn back_button(strings: &HashMap<String, String>) -> ButtonWidget {
    ButtonWidget::new(
        "back",
        "back",
        say(strings, Some("common.back"), "BACK"),
        Some(WidgetAction::Back),
    )
}

fn reset_button(page: &str, strings: &HashMap<String, String>) -> ButtonWidget {
    ButtonWidget::new(
        "reset",
        "middle",
        say(strings, Some("common.reset"), "RESET TO DEFAULTS"),
        Some(WidgetAction::ResetPage(page.to_string())),
    )
}

fn page_title(page: &str, strings: &HashMap<String, String>, y: f64) -> TextWidget {
    let entry = PAGES.iter().find(|p| p.id == page).unwrap_or(&PAGES[0]);
    TextWidget::title(
        "title",
        say(strings, Some(entry.title_key), entry.title),
        y,
    )
}

/// Builds the top-level `OPTIONS` [`MenuScreen`] (`optionsMenu` in `src/menus/option-pages.js`).
pub fn options_menu(ctx: &OptionsContext) -> MenuScreen {
    let strings = &ctx.strings;
    let mut row_items = vec![
        RowItem::enabled(
            "general",
            say(strings, Some("ifs.GameOpt.title"), "GAME OPTIONS"),
            Some(WidgetAction::OpenPage("game".to_string())),
        ),
        RowItem::enabled(
            "video",
            say(strings, Some("ifs.VideoOpt.title"), "VIDEO OPTIONS"),
            Some(WidgetAction::OpenPage("video".to_string())),
        ),
        RowItem::enabled(
            "sound",
            say(strings, Some("ifs.SoundOpt.title"), "AUDIO OPTIONS"),
            Some(WidgetAction::OpenPage("audio".to_string())),
        ),
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

    let mut online_row = RowItem::enabled(
        "online",
        say(strings, Some("ifs.onlineopt.title"), "ONLINE OPTIONS"),
        Some(WidgetAction::Named("online".to_string())),
    );
    online_row.hidden = !ctx.has_online;
    row_items.push(online_row);

    let mut int_row = RowItem::enabled(
        "integrations",
        "STEAM & DISCORD",
        Some(WidgetAction::Named("integrations".to_string())),
    );
    int_row.hidden = !ctx.has_integrations;
    row_items.push(int_row);

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

fn build_option_items(
    page: &str,
    settings: &Settings,
    ctx: &OptionsContext,
) -> Vec<OptionsItem> {
    let mut items = Vec::new();
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

    for spec in options_for(page) {
        let label = say(&ctx.strings, spec.label_key, spec.label);
        let value_text = format_option_value(spec, settings, &ctx.strings);
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
pub fn game_page(settings: &Settings, ctx: &OptionsContext) -> MenuScreen {
    let items = build_option_items("game", settings, ctx);
    let options = OptionsWidget::with_layout(
        "rows",
        OptionsLayoutKind::Game,
        items,
        ctx.surface_height,
    );
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
pub fn video_page(settings: &Settings, ctx: &OptionsContext) -> MenuScreen {
    let items = build_option_items("video", settings, ctx);
    let options = OptionsWidget::with_layout(
        "rows",
        OptionsLayoutKind::Video,
        items,
        ctx.surface_height,
    );
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
pub fn audio_page(settings: &Settings, ctx: &OptionsContext) -> MenuScreen {
    let items = build_option_items("audio", settings, ctx);
    let options = OptionsWidget::with_layout(
        "rows",
        OptionsLayoutKind::Audio,
        items,
        ctx.surface_height,
    );
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
    pub heading_y: f64,
    pub tab_y: f64,
    /// Bindings list box `[x, y, w, h]` anchored at `[0.5, 0.0]` (`bindingsLook(surface, extra)` in `option-pages.js`).
    pub list_box: [f64; 4],
}

/// Computes `controlsWindow(surface)` (`src/menus/option-pages.js`).
pub fn controls_window(surface_height: f64) -> ControlsWindow {
    controls_window_for_tab(surface_height, false)
}

/// Computes `controlsWindow(surface)` and `bindingsLook(surface, extra)` (`src/menus/option-pages.js`).
pub fn controls_window_for_tab(surface_height: f64, flying: bool) -> ControlsWindow {
    let top = 89.0;
    let bottom = surface_height;
    let extra = if flying { 24.5 } else { 0.0 };
    let h = 0.9 * surface_height;
    let list_h = (0.95 * h - 240.0 - extra).max(120.0);
    ControlsWindow {
        window_box: [0.0, (top + bottom) * 0.5, SAFE_W + 42.0, bottom - top],
        heading_y: 186.0,
        tab_y: 79.0,
        list_box: [0.0, 220.0 + extra + list_h * 0.5, SAFE_W, list_h],
    }
}

/// Computes `bindingsLook(surface)` (`src/menus/option-pages.js`).
pub fn bindings_look(surface_height: f64) -> ListLook {
    bindings_look_for_tab(surface_height, false)
}

/// Computes `bindingsLook(surface, extra)` (`src/menus/option-pages.js`).
pub fn bindings_look_for_tab(surface_height: f64, flying: bool) -> ListLook {
    let cw = controls_window_for_tab(surface_height, flying);
    ListLook {
        at: [0.5, 0.0],
        x: 0.0,
        y: 0.0,
        rot_y: 0.0,
        box_x: cw.list_box[0],
        box_y: cw.list_box[1],
        box_w: cw.list_box[2],
        box_h: cw.list_box[3],
        font: FONT_TINY,
        pitch: 22.6,
        inset: 14.0,
        pad_left: 16.0,
        pad_right: 20.0,
        show_frame: true,
    }
}

fn build_controls_top_items(
    flying: bool,
    settings: &Settings,
    strings: &HashMap<String, String>,
) -> Vec<OptionsItem> {
    let keys: &[&str] = if flying {
        &["flightSensitivity", "flightInvertY", "flightResponseCurve"]
    } else {
        &["mouseSensitivity", "invertMouse"]
    };
    let mut items = Vec::with_capacity(keys.len());
    for &key in keys {
        if let Some(spec) = option_entry(key) {
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

fn build_controls_binding_entries(
    tab: &str,
    settings: &Settings,
    strings: &HashMap<String, String>,
) -> Vec<ListEntry> {
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
pub fn controls_page(
    settings: &Settings,
    ctx: &OptionsContext,
    active_tab: &str,
) -> MenuScreen {
    let flying = active_tab == "starfighter";
    let cw = controls_window_for_tab(ctx.surface_height, flying);

    let mut window = PanelWidget::new(
        "window",
        cw.window_box[0],
        cw.window_box[1],
        cw.window_box[2],
        cw.window_box[3],
    );
    window.at = [0.5, 0.0];

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

    let top_opts = OptionsWidget::with_layout(
        "rows",
        OptionsLayoutKind::Controls,
        build_controls_top_items(flying, settings, &ctx.strings),
        ctx.surface_height,
    );

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

    let mut screen = MenuScreen::new(
        "options-controls",
        vec![
            Widget::Text(page_title("controls", &ctx.strings, 5.0)),
            Widget::Panel(window),
            Widget::Tabs(tabs),
            Widget::Options(top_opts),
            Widget::Text(heading),
            Widget::List(list),
            Widget::Button(back_button(&ctx.strings)),
            Widget::Button(reset_button("controls", &ctx.strings)),
        ],
    );
    screen.dim_behind = true;
    screen.set_focus_by_id("rows");
    screen
}

/// Complete stateful controller for the `OPTIONS` menu and its four sub-pages
/// (`GAME`, `VIDEO`, `AUDIO`, `CONTROL OPTIONS`).
#[derive(Clone, Debug)]
pub struct OptionsController {
    pub stack: ScreenStack,
    pub settings: Settings,
    pub context: OptionsContext,
    pub input: MenuInput,
    pub controls_tab: String,
    pub last_named_action: Option<String>,
}

impl OptionsController {
    /// Creates a new [`OptionsController`] starting on the top `OPTIONS` screen.
    pub fn new(settings: Settings, context: OptionsContext) -> Self {
        let mut stack = ScreenStack::new();
        stack.push(options_menu(&context));
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
        let screen = match page {
            "game" => game_page(&self.settings, &self.context),
            "video" => video_page(&self.settings, &self.context),
            "audio" => audio_page(&self.settings, &self.context),
            "controls" => controls_page(&self.settings, &self.context, &self.controls_tab),
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
        if let Some(p) = page {
            let new_items = build_option_items(p, &self.settings, &self.context);
            if let Some(Widget::Options(opts)) = top.widget_mut("rows") {
                let sel = opts.selected.min(new_items.len().saturating_sub(1));
                let first = opts.first;
                opts.items = new_items;
                opts.selected = sel;
                opts.first = first;
                opts.update_shown(self.context.surface_height);
                opts.reveal();
            }
        } else if screen_id == "options-controls" {
            let flying = self.controls_tab == "starfighter";
            let new_top = build_controls_top_items(flying, &self.settings, &self.context.strings);
            if let Some(Widget::Options(opts)) = top.widget_mut("rows") {
                let sel = opts.selected.min(new_top.len().saturating_sub(1));
                opts.items = new_top;
                opts.selected = sel;
            }
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
                self.refresh_active_screen();
            }
            MenuInputEvent::StepOption { key, dir } => {
                if let Some(spec) = option_entry(&key) {
                    if step_option(spec, &mut self.settings, dir) {
                        self.refresh_active_screen();
                    }
                }
            }
            MenuInputEvent::SetOptionRatio { key, ratio } => {
                if let Some(spec) = option_entry(&key) {
                    if spec.kind == OptionKind::Slider {
                        let raw = spec.min + ratio.clamp(0.0, 1.0) * (spec.max - spec.min);
                        let stepped =
                            ((raw / spec.step).round() * spec.step * 1000.0).round() / 1000.0;
                        self.settings.set(spec.key, stepped.clamp(spec.min, spec.max));
                        self.refresh_active_screen();
                    }
                }
            }
            MenuInputEvent::Action(action) => match action {
                WidgetAction::OpenPage(page) => self.open_page(&page),
                WidgetAction::Back => {
                    self.stack.pop();
                    self.refresh_active_screen();
                }
                WidgetAction::ResetPage(page) => {
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
                WidgetAction::StartRebind { tab, action, label } => {
                    let popup = key_popup(&tab, &action, &label, &self.context.strings);
                    self.stack.push(popup);
                }
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
                WidgetAction::Named(name) => {
                    if name == "toggle_viewpoint" {
                        if let Some(vp) = &mut self.context.viewpoint {
                            *vp = !*vp;
                        }
                        self.refresh_active_screen();
                    } else if name == "credits" {
                        let popup = ok_popup(
                            "BATTLEFRONT ETERNAL\nA fan-made mod for the Steam versions of\nStar Wars: Battlefront (2004) and Battlefront II (2005).\nBattlefront II support is planned.",
                            &self.context.strings,
                        );
                        self.stack.push(popup);
                    } else {
                        self.last_named_action = Some(name);
                    }
                }
            },
            MenuInputEvent::KeyCapture(outcome) => match outcome {
                KeyCaptureOutcome::Cancelled => {
                    self.stack.pop();
                }
                KeyCaptureOutcome::Bound { tab, action, code } => {
                    self.apply_rebind(&tab, &action, Some(&code));
                    self.stack.pop();
                    self.refresh_active_screen();
                }
                KeyCaptureOutcome::Cleared { tab, action } => {
                    self.apply_rebind(&tab, &action, None);
                    self.stack.pop();
                    self.refresh_active_screen();
                }
                KeyCaptureOutcome::Ignored => {}
            },
        }
    }

    fn apply_rebind(&mut self, tab: &str, action: &str, code: Option<&str>) {
        if tab == "starfighter" {
            let defaults = default_flight_bindings();
            let cur = self.settings.get("flightBindings");
            let next = rebind(&defaults, cur.as_bindings(), action, code);
            self.settings
                .set("flightBindings", SettingValue::Bindings(next));
        } else {
            let defaults = flat_bindings();
            let cur = self.settings.get("keyBindings");
            let next = rebind(&defaults, cur.as_bindings(), action, code);
            self.settings
                .set("keyBindings", SettingValue::Bindings(next));
        }
    }

    /// Processes a keyboard key press.
    pub fn on_key_down(&mut self, code: &str, shift: bool) {
        let ev = self.input.on_key_down(&mut self.stack, code, shift);
        self.apply_event(ev);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_pages_build_and_navigate_all_four_pages() {
        let settings = Settings::new();
        let ctx = OptionsContext::default();
        let mut ctrl = OptionsController::new(settings, ctx);

        assert_eq!(ctrl.stack.top().unwrap().id, "options");

        // Open GAME OPTIONS
        ctrl.open_page("game");
        assert_eq!(ctrl.stack.top().unwrap().id, "options-game");
        if let Some(Widget::Options(opts)) = ctrl.stack.top().unwrap().widget("rows") {
            // 8 game options + 1 RESET TO DEFAULTS row = 9 rows
            assert_eq!(opts.items.len(), 9);
            assert_eq!(opts.items[0].id, "flightFirstPerson");
            assert_eq!(opts.items[8].id, "reset");
        } else {
            panic!("expected Options widget on gamePage");
        }
        ctrl.on_key_down("Escape", false);
        assert_eq!(ctrl.stack.top().unwrap().id, "options");

        // Open VIDEO OPTIONS
        ctrl.open_page("video");
        assert_eq!(ctrl.stack.top().unwrap().id, "options-video");
        if let Some(Widget::Options(opts)) = ctrl.stack.top().unwrap().widget("rows") {
            assert_eq!(opts.items.len(), 31);
            assert_eq!(opts.items[0].id, "performancePreset");
            assert_eq!(opts.items[30].id, "deathCamera");
        } else {
            panic!("expected Options widget on videoPage");
        }
        ctrl.on_key_down("Escape", false);
        assert_eq!(ctrl.stack.top().unwrap().id, "options");

        // Open AUDIO OPTIONS
        ctrl.open_page("audio");
        assert_eq!(ctrl.stack.top().unwrap().id, "options-audio");
        if let Some(Widget::Options(opts)) = ctrl.stack.top().unwrap().widget("rows") {
            assert_eq!(opts.items.len(), 11);
            assert_eq!(opts.items[0].id, "masterVolume");
            assert_eq!(opts.items[10].id, "audioClarity");
        } else {
            panic!("expected Options widget on audioPage");
        }
        ctrl.on_key_down("Escape", false);
        assert_eq!(ctrl.stack.top().unwrap().id, "options");

        // Open CONTROL OPTIONS
        ctrl.open_page("controls");
        assert_eq!(ctrl.stack.top().unwrap().id, "options-controls");
        if let Some(Widget::List(list)) = ctrl.stack.top().unwrap().widget("bindings") {
            assert!(!list.rows.is_empty());
        } else {
            panic!("expected bindings list on controlsPage");
        }
    }
}
