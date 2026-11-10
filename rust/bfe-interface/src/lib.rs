//! Battlefront Eternal 2D/3D-tilted interface kit, menu framework, classic theme,
//! options schema, and persistent settings (`src/interface/`, `src/menus/`, `src/settings.js`).
//!
//! Draws every menu screen and modal popup through [`bfe_render::interface::Batch`] and
//! [`bfe_render::hud::HudPass`] in the `1024 x H` classic Battlefront coordinate space
//! (`H = 768 + (h / w - 0.75) * 768`, `768` at 4:3, `576` at 16:9).
//!
//! Module groups:
//! - **2D/3D interface primitives** (`flashy`, `fonts`, `shapes`, `space`, `surface`):
//!   virtual `1024 x H` camera projection, 9-slice geometry, bitmap font quads, and flashy text intros.
//! - **Menu stack & widgets** (`input`, `popup`, `screen`, `sounds`, `stack`, `widgets`, `theme`):
//!   retained screen navigation stack, keyboard/pointer hit-testing, and zero-allocation classic rendering.
//! - **Options & settings** (`hotkeys`, `option_pages`, `options`, `settings`):
//!   declarative `GAME`/`VIDEO`/`AUDIO`/`CONTROLS` schemas, GPU preset classification, and keybindings.

// 2D/3D interface primitives (`src/interface/`), menu framework (`src/menus/`), and settings (`src/settings.js`).
pub mod flashy;
pub mod fonts;
pub mod gpu_note;
pub mod hotkeys;
pub mod input;
pub mod option_pages;
pub mod options;
pub mod popup;
pub mod screen;
pub mod settings;
pub mod shapes;
pub mod sounds;
pub mod space;
pub mod stack;
pub mod surface;
pub mod theme;
pub mod widgets;

// Re-export flashy text intro animation types (`src/interface/flashy.js`) and bitmap fonts (`src/interface/fonts.js`).
pub use flashy::{FlashyBar, FlashyFrame, FlashyPieces, FlashyText, TitleIntro, INTRO, INTRO_LENGTH};
pub use fonts::{
    font_draw_text, font_text_width, Font, Fonts, Glyph, TextAlign, TextDraw, FONT_LARGE,
    FONT_MEDIUM, FONT_SMALL, FONT_TINY,
};
// Re-export keybinding tables (`src/menus/hotkeys.js`) and menu input routing (`src/menus/input.js`).
pub use hotkeys::{
    bindable, binding_codes, default_flight_bindings, flat_bindings, flight_bindings, hotkey_rows,
    key_name, keys_text, player_bindings, rebind, FlightBindingDef, HotkeyRow, HotkeyRowSpec,
    HotkeyTab, FLIGHT_BINDINGS, INFANTRY_ROWS, INPUT_CODES, RESERVED, TABS, VEHICLE_ROWS,
    WORLD_AXES,
};
pub use input::{MenuInput, MenuInputEvent};
// Re-export option page builders (`src/menus/option-pages.js`) and declarative option specs (`src/menus/options.js`).
pub use option_pages::{
    audio_page, bindings_look, bindings_look_for_tab, controls_page, controls_window,
    controls_window_for_tab, game_page, options_menu, video_page, ControlsWindow, OptionsContext,
    OptionsController, CONTROLS_TABS, SAFE_W, SHELL_OPTIONS_LAYOUT, TOP_COUNT, TOP_LAYOUT,
};
pub use options::{
    all_performance_presets_in_choices, applies, format_option_value, option_entry, option_note,
    options_for, render_resolution, reset_page, say, sound_note, step_option, ApplyMode,
    ChoiceItem, ChoiceVal, OptionKind, OptionSpec, PageDef, SliderFormat, OPTIONS, OPTION_KEYS,
    PAGES, PROFILE_KEYS, THEME_KEYS,
};
// Re-export modal popups (`src/menus/popup.js`), screens (`src/menus/screen.js`), and settings (`src/settings.js`).
pub use popup::{confirm_popup, key_popup, ok_popup, KeyCaptureModal, KeyCaptureOutcome};
pub use screen::{MenuScreen, ScreenFocus};
pub use settings::{
    classify_gpu, default_for, performance_preset, BattleSizeCap, CpuInfo, DefaultVal,
    DesktopSettingsSnapshot, GpuInfo, PerformancePreset, SettingDefault, SettingValue, Settings,
    ShadowPreset, ANTIALIASING, BATTLE_SIZES, DEFAULTS, PERFORMANCE_PRESETS, PHYSICS_RATES,
    PRESET_KEYS, RENDER_SCALE_MIN, SHADOW_PRESETS, VIEW_DISTANCES, WATER_QUALITIES, WORLD_SETTINGS,
};
// Re-export geometric shapes, sound cues, 3D interface coordinate space, stack, surface, theme, and widgets.
pub use shapes::{inside_quad, nine_slice, nine_slice_fixed, wrap_lines, NineSlicePiece};
pub use sounds::{sound_asset, SoundCue, SoundEvent, SoundEvents, SOUNDS};
pub use space::{
    alpha, anchor, anchor_matrix, interface_height, local_point, mix, mul, plane_point, rotate_x,
    rotate_y, rotate_z, scale, to_interface, to_interface_size, translate, Color, Matrix, Point,
    FOV, IDENTITY, IH, IW, PLANE, UV, WHITE_COLOR,
};
pub use stack::ScreenStack;
pub use surface::{common_texture_url, Batch, Draw, InterfaceSurface, Surface, QUADS};
pub use theme::{
    cursor_pulse, draw_bar, draw_frame, draw_scroll_bar, draw_slider_bar, draw_str, fallback_color,
    menu_pulse, piece_quad, rotation_euler, row_bar_at, rows_shift, scaled_color, selected_color,
    str_width, tint_color, ClassicTheme, HitRegion, HitTarget, BAR_X, BLACK, BUTTON_INSET, DIM,
    DIMMED, GREY, NOTE_YELLOW, SLIDER_DIM, TEXT_X, THUMB_ALPHA, TRACK_ALPHA, WHITE,
};
pub use widgets::{
    ButtonWidget, HorizontalButtonItem, HorizontalButtonsWidget, ListColumn, ListEntry, ListLook,
    ListWidget, OptionRowWidget, OptionsItem, OptionsLayoutKind, OptionsWidget, PanelWidget,
    RowItem, RowsLayout, RowsWidget, TabItem, TabsWidget, TextLook, TextWidget, Widget,
    WidgetAction,
};
