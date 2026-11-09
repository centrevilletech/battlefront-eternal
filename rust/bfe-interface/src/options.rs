//! Option pages, schema, notes, and row builders (`src/menus/options.js`).
//!
//! Ports `PAGES`, `OPTIONS` (all 57 entries in exact order), `OPTION_KEYS`,
//! `PROFILE_KEYS`, `THEME_KEYS`, `optionsFor`, `optionEntry`, `applies`, `NOTES`,
//! `renderResolution`, `soundNote`, `enhancedSoundNote`, `optionRow`, and `resetPage`
//! from `src/menus/options.js`.
//!
//! Every setting in [`crate::settings::DEFAULTS`] is accounted for either in [`OPTIONS`]
//! (57 entries across `game`, `video`, `audio`, and `controls`), [`PROFILE_KEYS`]
//! (`playerName`), or [`THEME_KEYS`] (`menuTheme`), verified at test time against
//! `src/settings.js` and `src/menus/options.js`.

use crate::settings::{SettingValue, Settings, PERFORMANCE_PRESETS};
use std::collections::HashMap;

/// Option page descriptor (`PAGES` in `src/menus/options.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageDef {
    /// Internal page identifier (`"game"`, `"video"`, `"audio"`, or `"controls"`).
    pub id: &'static str,
    /// Fallback uppercase English page title.
    pub title: &'static str,
    /// Localization string key looked up in `common.json` (`strings`).
    pub title_key: &'static str,
}

/// The four option sub-pages (`PAGES` in `src/menus/options.js`).
pub const PAGES: [PageDef; 4] = [
    // Gameplay rules, bot difficulty/mode/size, HUD stats line, and 60 Hz simulation rate.
    PageDef {
        id: "game",
        title: "GAME OPTIONS",
        title_key: "ifs.GameOpt.title",
    },
    // Rendering preset, frame pacing, upscaling, shadows, water, terrain, and visual effects.
    PageDef {
        id: "video",
        title: "VIDEO OPTIONS",
        title_key: "ifs.VideoOpt.title",
    },
    // Master/category volume sliders and sound stage/output/dynamics settings.
    PageDef {
        id: "audio",
        title: "AUDIO OPTIONS",
        title_key: "ifs.SoundOpt.title",
    },
    // Mouse/starfighter sensitivity, Y-axis inversion, and key binding tables.
    PageDef {
        id: "controls",
        title: "CONTROL OPTIONS",
        title_key: "ifs.GameOpt.pc_optionstitle",
    },
];

/// Widget control kind for an entry in [`OPTIONS`] (`kind` in `src/menus/options.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionKind {
    /// Multi-value selector cycled with Left/Right arrows or mouse clicks (`"choice"`).
    Choice,
    /// Boolean two-state selector (`"toggle"`, displaying `OFF`/`ON` or `NO`/`YES`).
    Toggle,
    /// Numeric range bar stepped with arrows or dragged with the mouse (`"slider"`).
    Slider,
    /// Non-interactive informational or externally edited setting (`"fixed"`).
    Fixed,
    /// Clickable command row such as `RESET TO DEFAULTS` or `SPEAKER TEST` (`"action"`).
    Action,
}

impl OptionKind {
    /// Returns the `src/menus/options.js` string identifier (`"choice"`, `"toggle"`, `"slider"`, `"fixed"`, `"action"`).
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Choice => "choice",
            Self::Toggle => "toggle",
            Self::Slider => "slider",
            Self::Fixed => "fixed",
            Self::Action => "action",
        }
    }
}

/// When an option change takes effect (`apply` in `src/menus/options.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyMode {
    /// Applies immediately (`"live"`).
    Live,
    /// Applies on the next map load (`"map"`).
    Map,
    /// Applies on the next battle / mission restart (`"battle"`).
    Battle,
    /// Fixed constant that does not change (`"fixed"`).
    Fixed,
}

impl ApplyMode {
    /// Returns the `src/menus/options.js` string identifier (`"live"`, `"map"`, `"battle"`, `"fixed"`).
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Map => "map",
            Self::Battle => "battle",
            Self::Fixed => "fixed",
        }
    }
}

/// Value stored in a [`ChoiceItem`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChoiceVal {
    /// String setting value (used by most `choice` options).
    Text(&'static str),
    /// Integer setting value (used by `anisotropy`: `1`, `2`, `4`, `8`, `16`).
    Int(i64),
}

impl ChoiceVal {
    /// Converts this choice literal into a [`SettingValue`].
    #[inline]
    pub fn to_setting_value(self) -> SettingValue {
        match self {
            Self::Text(s) => SettingValue::Text(s.to_string()),
            Self::Int(i) => SettingValue::Int(i),
        }
    }

    /// Returns true if `val` matches this choice literal (comparing `Int` and whole `Number` values).
    #[inline]
    pub fn matches(self, val: &SettingValue) -> bool {
        match (self, val) {
            (Self::Text(a), SettingValue::Text(b)) => a == b,
            (Self::Int(a), SettingValue::Int(b)) => a == *b,
            (Self::Int(a), SettingValue::Number(b)) => (a as f64 - *b).abs() <= 1e-9,
            _ => false,
        }
    }
}

/// One selectable value in a `Choice` option (`choices` in `src/menus/options.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChoiceItem {
    /// Stored setting value when this choice is selected.
    pub value: ChoiceVal,
    /// Fallback uppercase English label displayed for this choice.
    pub label: &'static str,
    /// Optional localization key in `common.json` (`strings`).
    pub label_key: Option<&'static str>,
}

impl ChoiceItem {
    /// Constructs a string choice item without a localization key.
    const fn s(value: &'static str, label: &'static str) -> Self {
        Self {
            value: ChoiceVal::Text(value),
            label,
            label_key: None,
        }
    }

    /// Constructs a string choice item with a `common.json` localization key.
    const fn sk(value: &'static str, label: &'static str, label_key: &'static str) -> Self {
        Self {
            value: ChoiceVal::Text(value),
            label,
            label_key: Some(label_key),
        }
    }

    /// Constructs an integer choice item without a localization key.
    const fn i(value: i64, label: &'static str) -> Self {
        Self {
            value: ChoiceVal::Int(value),
            label,
            label_key: None,
        }
    }
}

/// Numeric formatting style for a `Slider` option (`format` in `src/menus/options.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SliderFormat {
    /// `format: percent` -> `"{round(v * 100)}%"`.
    Percent,
    /// `format: (v) => "${v.toFixed(2)}X"`.
    ScaleX,
    /// `format: (v) => v.toFixed(1)`.
    OneDecimal,
}

impl SliderFormat {
    /// Formats a slider value `v` according to `src/menus/options.js`.
    pub fn format(self, v: f64) -> String {
        match self {
            Self::Percent => format!("{}%", (v * 100.0).round() as i64),
            Self::ScaleX => format!("{v:.2}X"),
            Self::OneDecimal => format!("{v:.1}"),
        }
    }
}

/// Schema specification for one entry in [`OPTIONS`] (`src/menus/options.js`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OptionSpec {
    /// Setting key in [`crate::settings::DEFAULTS`].
    pub key: &'static str,
    /// Owning sub-page id (`"game"`, `"video"`, `"audio"`, or `"controls"`).
    pub page: &'static str,
    /// Fallback uppercase English label shown on the left side of the row.
    pub label: &'static str,
    /// Optional localization key in `common.json` (`strings`).
    pub label_key: Option<&'static str>,
    /// Interactive control kind (`Choice`, `Toggle`, `Slider`, `Fixed`, `Action`).
    pub kind: OptionKind,
    /// When changes to this setting take effect (`Live`, `Map`, `Battle`, `Fixed`).
    pub apply: ApplyMode,
    /// Optional URL query parameter name that overrides this setting for a single session.
    pub url: Option<&'static str>,
    /// Allowed choice items when `kind == OptionKind::Choice`.
    pub choices: &'static [ChoiceItem],
    /// Fallback `[false_label, true_label]` strings when `kind == OptionKind::Toggle`.
    pub toggle_labels: [&'static str; 2],
    /// Optional `[false_key, true_key]` localization keys for toggle labels.
    pub toggle_label_keys: [Option<&'static str>; 2],
    /// Minimum slider value when `kind == OptionKind::Slider`.
    pub min: f64,
    /// Maximum slider value when `kind == OptionKind::Slider`.
    pub max: f64,
    /// Discrete arrow-key step increment when `kind == OptionKind::Slider`.
    pub step: f64,
    /// Display formatting rule for slider values.
    pub format: SliderFormat,
    /// Optional static display string for `OptionKind::Fixed` rows (such as `"60 HZ"`).
    pub display: Option<&'static str>,
}

/// Empty choice slice used by non-`Choice` option specs.
const CHOICES_EMPTY: [ChoiceItem; 0] = [];

/// `CLASSIC` (`[["classic", "CLASSIC"], ["enhanced", "ENHANCED"]]` in `src/menus/options.js`).
///
/// Used by visual effect options (`environmentEffects`, `oceanEffects`, `rainEffects`,
/// `boltEffects`, `arcCasterEffects`, `sniperEffects`, `jetpackEffects`, `droidekaEffects`,
/// `destructionEffects`, `deathCamera`).
const CHOICES_CLASSIC: [ChoiceItem; 2] = [
    // 2004 Battlefront visual representation.
    ChoiceItem::s("classic", "CLASSIC"),
    // Rebuilt shader/particle visual representation.
    ChoiceItem::s("enhanced", "ENHANCED"),
];

/// `ORIGINAL` (`[["original", "CLASSIC"], ["enhanced", "ENHANCED"]]` in `src/menus/options.js`).
///
/// Used by `terrainDetail` and `treeDetail`, whose classic setting value is `"original"`.
const CHOICES_ORIGINAL: [ChoiceItem; 2] = [
    // Original 2004 terrain/foliage detail (`"original"` displayed as `"CLASSIC"`).
    ChoiceItem::s("original", "CLASSIC"),
    // Enhanced slope shading and foliage detail (`"enhanced"`).
    ChoiceItem::s("enhanced", "ENHANCED"),
];

/// Bot difficulty choices (`aiDifficulty` in `src/menus/options.js`).
const CHOICES_DIFFICULTY: [ChoiceItem; 3] = [
    // Easy difficulty (`ifs.difficulty.easy`).
    ChoiceItem::sk("easy", "EASY", "ifs.difficulty.easy"),
    // Default balanced difficulty (`"medium"` -> `"NORMAL"`).
    ChoiceItem::s("medium", "NORMAL"),
    // Highest bot difficulty (`"hard"` -> `"ELITE"`).
    ChoiceItem::s("hard", "ELITE"),
];

/// Battle bot mode choices (`aiMode` in `src/menus/options.js`).
const CHOICES_AI_MODE: [ChoiceItem; 3] = [
    // No autonomous bot combat (`"off"`).
    ChoiceItem::sk("off", "OFF", "common.off"),
    // Recovered 2004 bot behavior (`"classic"`).
    ChoiceItem::s("classic", "CLASSIC"),
    // Rebalanced objective focus and fairer aim (`"enhanced"`).
    ChoiceItem::s("enhanced", "ENHANCED"),
];

/// Single-player team unit count scale choices (`battleSize` in `src/menus/options.js`).
const CHOICES_BATTLE_SIZE: [ChoiceItem; 3] = [
    // Full 2004 unit count (scale `1.0`, up to 32 per team).
    ChoiceItem::s("full", "FULL"),
    // Reduced 3/4 unit count (`"large"` displayed as `"REDUCED"`).
    ChoiceItem::s("large", "REDUCED"),
    // Half unit count for older CPUs (`"small"`).
    ChoiceItem::s("small", "SMALL"),
];

/// Rendering quality preset choices (`performancePreset` in `src/menus/options.js`).
const CHOICES_PERFORMANCE_PRESET: [ChoiceItem; 5] = [
    // Selected automatically whenever an individual `PRESET_KEYS` setting is customized.
    ChoiceItem::s("custom", "CUSTOM"),
    // Half-resolution preset for legacy integrated GPUs (`"minimum"`).
    ChoiceItem::s("minimum", "MINIMUM"),
    // 0.75x scale + fast FXAA preset for older PCs (`"performance"`).
    ChoiceItem::s("performance", "PERFORMANCE"),
    // 1.0x temporal + medium shadows preset (`"balanced"`).
    ChoiceItem::s("balanced", "BALANCED"),
    // High-quality 4x MSAA + high shadows preset (`"quality"`).
    ChoiceItem::s("quality", "HIGH QUALITY"),
];

/// Frame rate cap choices (`frameLimit` / `FPS("DISPLAY")` in `src/menus/options.js`).
const CHOICES_FRAME_LIMIT: [ChoiceItem; 6] = [
    // Paced to the display's native refresh rate (`"0"` -> `"DISPLAY"`).
    ChoiceItem::s("0", "DISPLAY"),
    ChoiceItem::s("30", "30 FPS"),
    ChoiceItem::s("60", "60 FPS"),
    ChoiceItem::s("120", "120 FPS"),
    ChoiceItem::s("144", "144 FPS"),
    ChoiceItem::s("240", "240 FPS"),
];

/// Adaptive resolution target frame rate choices (`dynamicResolution` / `FPS("OFF")` in `src/menus/options.js`).
const CHOICES_DYNAMIC_RESOLUTION: [ChoiceItem; 6] = [
    // Adaptive resolution disabled (`"0"` -> `"OFF"`).
    ChoiceItem::s("0", "OFF"),
    ChoiceItem::s("30", "30 FPS"),
    ChoiceItem::s("60", "60 FPS"),
    ChoiceItem::s("120", "120 FPS"),
    ChoiceItem::s("144", "144 FPS"),
    ChoiceItem::s("240", "240 FPS"),
];

/// 3D frame upscaler mode choices (`upscaler` in `src/menus/options.js`).
const CHOICES_UPSCALER: [ChoiceItem; 4] = [
    // Browser canvas stretch (`"off"`).
    ChoiceItem::s("off", "OFF"),
    // Linear texture stretch keeping the 2D HUD at native resolution (`"bilinear"`).
    ChoiceItem::s("bilinear", "BILINEAR"),
    // Single-pass spatial upscale + sharpening (`"spatial"`).
    ChoiceItem::s("spatial", "SPATIAL"),
    // Multi-frame jittered temporal reconstruction (`"temporal"`).
    ChoiceItem::s("temporal", "TEMPORAL"),
];

/// Edge antialiasing choices (`antialiasing` in `src/menus/options.js`).
const CHOICES_ANTIALIASING: [ChoiceItem; 6] = [
    ChoiceItem::s("off", "OFF"),
    // FXAA 3.11 console path for older GPUs (`"fxaa-fast"`).
    ChoiceItem::s("fxaa-fast", "FXAA (FAST)"),
    // Full FXAA 3.11 quality path (`"fxaa"`).
    ChoiceItem::s("fxaa", "FXAA"),
    // Hardware multisampling + FXAA (`"2x"`, `"4x"`, `"8x"`).
    ChoiceItem::s("2x", "2X MSAA"),
    ChoiceItem::s("4x", "4X MSAA"),
    ChoiceItem::s("8x", "8X MSAA"),
];

/// Cascaded shadow map quality choices (`shadows` in `src/menus/options.js`).
const CHOICES_SHADOWS: [ChoiceItem; 5] = [
    ChoiceItem::s("off", "OFF"),
    ChoiceItem::s("low", "LOW"),
    ChoiceItem::s("medium", "MEDIUM"),
    ChoiceItem::s("high", "HIGH"),
    ChoiceItem::s("ultra", "ULTRA"),
];

/// Texture filtering anisotropy choices (`anisotropy` in `src/menus/options.js`).
const CHOICES_ANISOTROPY: [ChoiceItem; 5] = [
    // Isotropic mipmapped trilinear filtering (`1`).
    ChoiceItem::i(1, "TRILINEAR"),
    ChoiceItem::i(2, "2X ANISOTROPIC"),
    ChoiceItem::i(4, "4X ANISOTROPIC"),
    ChoiceItem::i(8, "8X ANISOTROPIC"),
    ChoiceItem::i(16, "16X ANISOTROPIC"),
];

/// Water reflection quality choices (`waterQuality` in `src/menus/options.js`).
const CHOICES_WATER_QUALITY: [ChoiceItem; 3] = [
    // Original 2004 water without scene reflections (`"low"`).
    ChoiceItem::s("low", "LOW"),
    // Half-resolution screen-space reflections (`"high"`).
    ChoiceItem::s("high", "HIGH"),
    // Full-resolution planar reflections (`"ultra"`).
    ChoiceItem::s("ultra", "ULTRA"),
];

/// Near-scene full-detail view distance multiplier choices (`viewDistance` in `src/menus/options.js`).
const CHOICES_VIEW_DISTANCE: [ChoiceItem; 3] = [
    // Map's native 2004 near-scene range (`"original"`).
    ChoiceItem::s("original", "ORIGINAL"),
    // Twice the map's near-scene range (`"far"`).
    ChoiceItem::s("far", "FAR (2X)"),
    // Four times the map's near-scene range (`"max"`).
    ChoiceItem::s("max", "MAXIMUM (4X)"),
];

/// Far-scene terrain mesh density choices (`farTerrain` in `src/menus/options.js`).
const CHOICES_FAR_TERRAIN: [ChoiceItem; 2] = [
    // Every distant terrain triangle (`"full"`).
    ChoiceItem::s("full", "FULL"),
    // Quarter triangle count for weak GPUs (`"reduced"`).
    ChoiceItem::s("reduced", "REDUCED"),
];

/// Sound stage engine quality choices (`audioQuality` in `src/menus/options.js`).
const CHOICES_AUDIO_QUALITY: [ChoiceItem; 2] = [
    // 3D positional sound stage with distance and room acoustics (`"enhanced"`).
    ChoiceItem::s("enhanced", "ENHANCED"),
    // Lightweight 2004 stereo mix (`"original"` displayed as `"CLASSIC"`).
    ChoiceItem::s("original", "CLASSIC"),
];

/// Speaker/headphone channel layout choices (`audioOutput` in `src/menus/options.js`).
const CHOICES_AUDIO_OUTPUT: [ChoiceItem; 6] = [
    // Queries the output device layout automatically (`"auto"`).
    ChoiceItem::s("auto", "AUTOMATIC"),
    // Binaural headphone panning (`"headphones"`).
    ChoiceItem::s("headphones", "HEADPHONES"),
    // 2.0 stereo speakers (`"stereo"`).
    ChoiceItem::s("stereo", "STEREO"),
    // Multichannel surround speaker layouts (`"5.1"`, `"7.1"`, `"7.1.4"`).
    ChoiceItem::s("5.1", "5.1"),
    ChoiceItem::s("7.1", "7.1"),
    ChoiceItem::s("7.1.4", "7.1.4"),
];

/// Dynamic range compression profile choices (`audioDynamics` in `src/menus/options.js`).
const CHOICES_AUDIO_DYNAMICS: [ChoiceItem; 3] = [
    // Wide dynamic range for home theater speakers (`"home"`).
    ChoiceItem::s("home", "HOME THEATER"),
    // Balanced desktop dynamic range (`"normal"`).
    ChoiceItem::s("normal", "NORMAL"),
    // Compressed dynamic range for quiet listening (`"night"`).
    ChoiceItem::s("night", "NIGHT"),
];

impl OptionSpec {
    /// Constructs a boolean `OFF` / `ON` toggle row in the 57-entry [`OPTIONS`] schema table.
    const fn toggle(
        key: &'static str,
        page: &'static str,
        label: &'static str,
        label_key: Option<&'static str>,
        apply: ApplyMode,
        url: Option<&'static str>,
    ) -> Self {
        Self {
            key,
            page,
            label,
            label_key,
            kind: OptionKind::Toggle,
            apply,
            url,
            choices: &CHOICES_EMPTY,
            toggle_labels: ["OFF", "ON"],
            toggle_label_keys: [Some("common.off"), Some("common.on")],
            min: 0.0,
            max: 1.0,
            step: 1.0,
            format: SliderFormat::Percent,
            display: None,
        }
    }

    /// Constructs a boolean `NO` / `YES` toggle row (used by `invertMouse`).
    const fn toggle_yes_no(
        key: &'static str,
        page: &'static str,
        label: &'static str,
        label_key: Option<&'static str>,
        apply: ApplyMode,
        url: Option<&'static str>,
    ) -> Self {
        Self {
            key,
            page,
            label,
            label_key,
            kind: OptionKind::Toggle,
            apply,
            url,
            choices: &CHOICES_EMPTY,
            toggle_labels: ["NO", "YES"],
            toggle_label_keys: [Some("common.no"), Some("common.yes")],
            min: 0.0,
            max: 1.0,
            step: 1.0,
            format: SliderFormat::Percent,
            display: None,
        }
    }

    /// Constructs a multi-value `Choice` row in the 57-entry [`OPTIONS`] schema table.
    const fn choice(
        key: &'static str,
        page: &'static str,
        label: &'static str,
        label_key: Option<&'static str>,
        apply: ApplyMode,
        url: Option<&'static str>,
        choices: &'static [ChoiceItem],
    ) -> Self {
        Self {
            key,
            page,
            label,
            label_key,
            kind: OptionKind::Choice,
            apply,
            url,
            choices,
            toggle_labels: ["OFF", "ON"],
            toggle_label_keys: [Some("common.off"), Some("common.on")],
            min: 0.0,
            max: 1.0,
            step: 1.0,
            format: SliderFormat::Percent,
            display: None,
        }
    }

    // Constructs `Slider` rows of the 57-entry `OPTIONS` schema table with range, step, and format metadata.
    #[allow(clippy::too_many_arguments)]
    const fn slider(
        key: &'static str,
        page: &'static str,
        label: &'static str,
        label_key: Option<&'static str>,
        apply: ApplyMode,
        url: Option<&'static str>,
        min: f64,
        max: f64,
        step: f64,
        format: SliderFormat,
    ) -> Self {
        Self {
            key,
            page,
            label,
            label_key,
            kind: OptionKind::Slider,
            apply,
            url,
            choices: &CHOICES_EMPTY,
            toggle_labels: ["OFF", "ON"],
            toggle_label_keys: [Some("common.off"), Some("common.on")],
            min,
            max,
            step,
            format,
            display: None,
        }
    }

    /// Constructs a read-only or externally bound `Fixed` row in the 57-entry [`OPTIONS`] schema table.
    const fn fixed(
        key: &'static str,
        page: &'static str,
        label: &'static str,
        apply: ApplyMode,
        display: Option<&'static str>,
    ) -> Self {
        Self {
            key,
            page,
            label,
            label_key: None,
            kind: OptionKind::Fixed,
            apply,
            url: None,
            choices: &CHOICES_EMPTY,
            toggle_labels: ["OFF", "ON"],
            toggle_label_keys: [Some("common.off"), Some("common.on")],
            min: 0.0,
            max: 1.0,
            step: 1.0,
            format: SliderFormat::Percent,
            display,
        }
    }
}

/// All 57 option specifications in exact `src/menus/options.js` order.
pub const OPTIONS: [OptionSpec; 57] = [
    // 1..6: Starfighter & key bindings
    // 1. `flightFirstPerson`: starfighter cockpit camera preference (`game` page, live toggle).
    // Default: `false` (3rd-person chase camera); toggles cockpit view in starfighters.
    // Displayed as the first row on `GAME OPTIONS` (`page: "game"`, `apply: "live"`).
    OptionSpec::toggle(
        "flightFirstPerson",
        "game",
        "STARFIGHTER COCKPIT VIEW",
        None,
        ApplyMode::Live,
        None,
    ),
    // 2. `keyBindings`: infantry and vehicle custom key map (`controls` page, edited via `hotkey_rows`).
    // Default: `{}` (uses `flat_bindings()`); stores custom ground/vehicle key bindings.
    // Edited via the `INFANTRY` and `VEHICLE` tabs on `CONTROL OPTIONS`.
    OptionSpec::fixed("keyBindings", "controls", "KEY BINDINGS", ApplyMode::Live, None),
    // 3. `flightBindings`: starfighter custom key map (`controls` page, edited on `STARFIGHTER` tab).
    // Default: `{}` (uses `default_flight_bindings()`); stores custom starfighter bindings.
    // Edited via the `STARFIGHTER` tab on `CONTROL OPTIONS`.
    OptionSpec::fixed(
        "flightBindings",
        "controls",
        "STARFIGHTER BINDINGS",
        ApplyMode::Live,
        None,
    ),
    // 4. `flightSensitivity`: starfighter pitch/yaw turn sensitivity multiplier (`0.2..3.0`).
    // Default: `1.0`, range `0.2..3.0` in `0.1` steps (`SliderFormat::OneDecimal`).
    // Displayed at the top of `CONTROL OPTIONS` when the `STARFIGHTER` tab is active.
    OptionSpec::slider(
        "flightSensitivity",
        "controls",
        "STARFIGHTER SENSITIVITY",
        None,
        ApplyMode::Live,
        None,
        0.2,
        3.0,
        0.1,
        SliderFormat::OneDecimal,
    ),
    // 5. `flightInvertY`: inverts vertical pitch mouse/stick input when piloting a starfighter.
    // Default: `false` (`OFF` / `ON`); flips vertical pitch steering in starfighters.
    // Displayed as the second top row on the `STARFIGHTER` tab of `CONTROL OPTIONS`.
    OptionSpec::toggle(
        "flightInvertY",
        "controls",
        "STARFIGHTER INVERT Y",
        None,
        ApplyMode::Live,
        None,
    ),
    // 6. `flightResponseCurve`: exponent shaping small vs. large starfighter steering inputs (`1.0..3.0`).
    // Default: `1.35`, range `1.0..3.0` in `0.1` steps (`SliderFormat::OneDecimal`).
    // Displayed as the third top row on the `STARFIGHTER` tab of `CONTROL OPTIONS`.
    OptionSpec::slider(
        "flightResponseCurve",
        "controls",
        "STARFIGHTER RESPONSE",
        None,
        ApplyMode::Live,
        None,
        1.0,
        3.0,
        0.1,
        SliderFormat::OneDecimal,
    ),
    // 7..13: Game options
    // 7. `friendlyFire`: whether weapons damage human teammates (`WORLD_SETTINGS`, live toggle).
    // Default: `true` (`ON`, matching the 2004 game); belongs to `WORLD_SETTINGS`.
    // Controls whether friendly human fire and self-splash deal damage.
    OptionSpec::toggle(
        "friendlyFire",
        "game",
        "FRIENDLY FIRE",
        Some("ifs.gameopt.friendlyfire"),
        ApplyMode::Live,
        None,
    ),
    // 8. `aiHeroes`: spawns faction hero NPCs in Instant Action battles (applies on next battle, `?heroes=`).
    // Default: `false` (`OFF`); spawns faction hero NPCs (such as Luke or Vader) in Instant Action.
    // Takes effect when a battle starts (`ApplyMode::Battle`, URL override `?heroes=`).
    OptionSpec::toggle(
        "aiHeroes",
        "game",
        "INSTANT ACTION HEROES",
        Some("ifs.gameopt.hero"),
        ApplyMode::Battle,
        Some("heroes"),
    ),
    // 9. `aiDifficulty`: bot skill tier (`easy`, `medium`, `hard`; applies on next battle, `?tier=`).
    // Default: `"medium"` (`NORMAL`); chooses between `EASY`, `NORMAL`, and `ELITE`.
    // Takes effect when a battle starts (`ApplyMode::Battle`, URL override `?tier=`).
    OptionSpec::choice(
        "aiDifficulty",
        "game",
        "DIFFICULTY",
        Some("ifs.gameopt.diff"),
        ApplyMode::Battle,
        Some("tier"),
        &CHOICES_DIFFICULTY,
    ),
    // 10. `aiMode`: bot behavior mode (`off`, `classic`, `enhanced`; applies on next battle, `?ai=`).
    // Default: `"off"`; selects `OFF`, `CLASSIC` (2004 AI), or `ENHANCED` (objective/fair-aim AI).
    // Takes effect when a battle starts (`ApplyMode::Battle`, URL override `?ai=`).
    OptionSpec::choice(
        "aiMode",
        "game",
        "BATTLE AI",
        None,
        ApplyMode::Battle,
        Some("ai"),
        &CHOICES_AI_MODE,
    ),
    // 11. `battleSize`: active unit count per team (`full` 32, `large` reduced, `small`; `?size=`).
    // Default: `"full"` (32 units per team); scales team unit caps (`FULL`, `REDUCED`, `SMALL`).
    // Takes effect when a battle starts (`ApplyMode::Battle`, URL override `?size=`).
    OptionSpec::choice(
        "battleSize",
        "game",
        "BATTLE SIZE (MAX 32 PER TEAM)",
        None,
        ApplyMode::Battle,
        Some("size"),
        &CHOICES_BATTLE_SIZE,
    ),
    // 12. `showStats`: toggles the top-right FPS / frame-timing telemetry line.
    // Default: `true` (`ON`); shows or hides the live performance telemetry line.
    // Applies immediately (`ApplyMode::Live`) on the `GAME OPTIONS` page.
    OptionSpec::toggle("showStats", "game", "STATS LINE", None, ApplyMode::Live, None),
    // 13. `physicsRate`: simulation tick rate locked at `"60 HZ"`.
    // Default: `"60"` (`60 HZ`); read-only fixed row indicating the 60 Hz world simulation tick.
    // Cannot be modified (`OptionKind::Fixed`, `ApplyMode::Fixed`).
    OptionSpec::fixed(
        "physicsRate",
        "game",
        "SIMULATION RATE",
        ApplyMode::Fixed,
        Some("60 HZ"),
    ),
    // 14..44: Video options
    // 14. `performancePreset`: master quality preset (`custom`, `minimum`, `performance`, `balanced`, `quality`).
    // Default: `"custom"`; cascades across all 27 `PRESET_KEYS` when a named preset is chosen.
    // Automatically reverts to `"custom"` when any individual preset key is modified.
    OptionSpec::choice(
        "performancePreset",
        "video",
        "QUALITY PRESET",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_PERFORMANCE_PRESET,
    ),
    // 15. `frameLimit`: frame pacing cap (`0` display refresh, `30`, `60`, `120`, `144`, `240` FPS).
    // Default: `"0"` (`DISPLAY`); caps frame presentation rate or follows display refresh.
    // Applies immediately (`ApplyMode::Live`) on `VIDEO OPTIONS`.
    OptionSpec::choice(
        "frameLimit",
        "video",
        "FRAME LIMIT",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_FRAME_LIMIT,
    ),
    // 16. `dynamicResolution`: target frame rate for automatic render scale adjustment.
    // Default: `"0"` (`OFF`); dynamically lowers 3D render scale to maintain target FPS.
    // Applies immediately (`ApplyMode::Live`) on `VIDEO OPTIONS`.
    OptionSpec::choice(
        "dynamicResolution",
        "video",
        "ADAPTIVE RESOLUTION",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_DYNAMIC_RESOLUTION,
    ),
    // 17. `renderScale`: 3D viewport resolution multiplier (`0.25X..2.00X` in `0.05` steps, `?scale=`).
    // Default: `1.0` (`1.00X`), range `0.25..2.0` in `0.05` steps (URL override `?scale=`).
    // Footer note displays the resulting pixel dimensions (`render_resolution`).
    OptionSpec::slider(
        "renderScale",
        "video",
        "RENDER SCALE",
        None,
        ApplyMode::Live,
        Some("scale"),
        0.25,
        2.0,
        0.05,
        SliderFormat::ScaleX,
    ),
    // 18. `upscaler`: reconstruction pass used when `renderScale < 1.0` (`off`, `bilinear`, `spatial`, `temporal`).
    // Default: `"temporal"`; selects how sub-native 3D frames are reconstructed to window size.
    // Keeps the 2D HUD at native window resolution whenever an upscaler is active.
    OptionSpec::choice(
        "upscaler",
        "video",
        "UPSCALER",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_UPSCALER,
    ),
    // 19. `upscaleSharpness`: contrast-adaptive sharpening strength (`0%..100%`).
    // Default: `0.4` (`40%`), range `0.0..1.0` in `0.05` steps (`SliderFormat::Percent`).
    // Adjusts sharpening pass strength for the `spatial` and `temporal` upscalers.
    OptionSpec::slider(
        "upscaleSharpness",
        "video",
        "SHARPENING",
        None,
        ApplyMode::Live,
        None,
        0.0,
        1.0,
        0.05,
        SliderFormat::Percent,
    ),
    // 20. `antialiasing`: edge smoothing mode (`off`, `fxaa-fast`, `fxaa`, `2x`, `4x`, `8x` MSAA).
    // Default: `"4x"` (`4X MSAA`); chooses `OFF`, `FXAA (FAST)`, `FXAA`, or `2X`/`4X`/`8X` MSAA.
    // Part of `PRESET_KEYS` and applies immediately (`ApplyMode::Live`).
    OptionSpec::choice(
        "antialiasing",
        "video",
        "ANTIALIASING",
        Some("ifs.videopt.multisample"),
        ApplyMode::Live,
        None,
        &CHOICES_ANTIALIASING,
    ),
    // 21. `shadows`: cascaded shadow map preset (`off`, `low`, `medium`, `high`, `ultra`, `?shadows=`).
    // Default: `"high"`; configures cascaded shadow map resolution and cascade distance (`?shadows=`).
    // Part of `PRESET_KEYS` and applies immediately (`ApplyMode::Live`).
    OptionSpec::choice(
        "shadows",
        "video",
        "SHADOW QUALITY",
        Some("ifs.videopt.shadows"),
        ApplyMode::Live,
        Some("shadows"),
        &CHOICES_SHADOWS,
    ),
    // 22. `anisotropy`: texture filtering sample count (`1` trilinear up to `16`x anisotropic).
    // Default: `16` (`16X ANISOTROPIC`); selects texture filtering sample count (`1..16`).
    // Part of `PRESET_KEYS` and applies immediately (`ApplyMode::Live`).
    OptionSpec::choice(
        "anisotropy",
        "video",
        "TEXTURE FILTERING",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_ANISOTROPY,
    ),
    // 23. `textureCompression`: GPU-compressed KTX2/Basis textures (applies on next map load).
    // Default: `true` (`ON`); uploads KTX2/Basis GPU-compressed textures to save VRAM bandwidth.
    // Takes effect on the next map load (`ApplyMode::Map`).
    OptionSpec::toggle(
        "textureCompression",
        "video",
        "COMPRESSED TEXTURES",
        None,
        ApplyMode::Map,
        None,
    ),
    // 24. `waterQuality`: water surface reflection tier (`low`, `high` screen-space, `ultra` planar).
    // Default: `"high"` (screen-space reflections); chooses `LOW`, `HIGH`, or `ULTRA` (planar).
    // Part of `PRESET_KEYS` and applies immediately (`ApplyMode::Live`).
    OptionSpec::choice(
        "waterQuality",
        "video",
        "WATER QUALITY",
        Some("ifs.videoopt.waterquality"),
        ApplyMode::Live,
        None,
        &CHOICES_WATER_QUALITY,
    ),
    // 25. `viewDistance`: near-scene full-detail draw distance multiplier (`original`, `far` 2x, `max` 4x).
    // Default: `"original"` (1x near-scene range); chooses `ORIGINAL`, `FAR (2X)`, or `MAXIMUM (4X)`.
    // Part of `PRESET_KEYS` and applies immediately (`ApplyMode::Live`).
    OptionSpec::choice(
        "viewDistance",
        "video",
        "VIEW DISTANCE",
        Some("ifs.videopt.viewdistance"),
        ApplyMode::Live,
        None,
        &CHOICES_VIEW_DISTANCE,
    ),
    // 26. `farTerrain`: distant terrain mesh density (`full` or `reduced` quarter-triangle LOD).
    // Default: `"full"`; selects `FULL` or `REDUCED` (quarter-triangle) distant terrain patches.
    // Part of `PRESET_KEYS` and applies immediately (`ApplyMode::Live`).
    OptionSpec::choice(
        "farTerrain",
        "video",
        "FAR TERRAIN",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_FAR_TERRAIN,
    ),
    // 27. `bumpMapping`: normal-mapped surface detail on models and terrain.
    // Default: `true` (`ON`); enables normal-map surface relief on characters, vehicles, and ground.
    // Part of `PRESET_KEYS` and applies immediately (`ApplyMode::Live`).
    OptionSpec::toggle(
        "bumpMapping",
        "video",
        "BUMPMAPPING",
        Some("ifs.videopt.bumpmapping"),
        ApplyMode::Live,
        None,
    ),
    // 28. `softFocus`: original 2004 map-configured soft-focus post-process blur.
    // Default: `true` (`ON`); preserves the 2004 map-authored soft-focus glow pass.
    // Part of `PRESET_KEYS` and applies immediately (`ApplyMode::Live`).
    OptionSpec::toggle("softFocus", "video", "SOFT FOCUS", None, ApplyMode::Live, None),
    // 29. `bloom`: emissive bloom and weapon/explosion glow post-processing.
    // Default: `true` (`ON`); enables HDR emissive bloom around blaster fire, sabers, and lamps.
    // Part of `PRESET_KEYS` and applies immediately (`ApplyMode::Live`).
    OptionSpec::toggle(
        "bloom",
        "video",
        "BLOOM AND GLOW",
        None,
        ApplyMode::Live,
        None,
    ),
    // 30. `vehicleMotionBlur`: high-speed radial blur when boosting in vehicles/starfighters.
    // Default: `true` (`ON`); enables speed-dependent motion blur while piloting vehicles.
    // Part of `PRESET_KEYS` and applies immediately (`ApplyMode::Live`).
    OptionSpec::toggle(
        "vehicleMotionBlur",
        "video",
        "MOTION BLUR",
        Some("ifs.videoopt.motionblur"),
        ApplyMode::Live,
        None,
    ),
    // 31. `softParticles`: depth-feathered particle billboards against scene geometry.
    // Default: `true` (`ON`); softens particle intersections against depth buffer geometry.
    // Part of `PRESET_KEYS` and applies immediately (`ApplyMode::Live`).
    OptionSpec::toggle(
        "softParticles",
        "video",
        "SOFT PARTICLES",
        None,
        ApplyMode::Live,
        None,
    ),
    // 32. `lampLighting`: dynamic point-light illumination on characters from nearby lamps/ordnance.
    // Default: `true` (`ON`); illuminates nearby soldiers and droids with dynamic point lights.
    // Part of `PRESET_KEYS` and applies immediately (`ApplyMode::Live`).
    OptionSpec::toggle(
        "lampLighting",
        "video",
        "LAMP LIGHT ON CHARACTERS",
        None,
        ApplyMode::Live,
        None,
    ),
    // 33. `terrainDetail`: sun-and-slope terrain shading (`original` vs. `enhanced`).
    // Default: `"enhanced"`; switches between `CLASSIC` (`"original"`) and `ENHANCED` slope shading.
    // Part of `PRESET_KEYS` and applies immediately (`ApplyMode::Live`).
    OptionSpec::choice(
        "terrainDetail",
        "video",
        "TERRAIN",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_ORIGINAL,
    ),
    // 34. `treeDetail`: foliage and canopy rendering fidelity (`original` vs. `enhanced`).
    // Default: `"enhanced"`; switches between `CLASSIC` (`"original"`) and `ENHANCED` foliage.
    // Part of `PRESET_KEYS` and applies immediately (`ApplyMode::Live`).
    OptionSpec::choice(
        "treeDetail",
        "video",
        "TREES",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_ORIGINAL,
    ),
    // 35. `environmentEffects`: water splashes, mist, and steam emitters (`classic` vs. `enhanced`).
    // Default: `"enhanced"`; switches water splashes and steam between `CLASSIC` and `ENHANCED`.
    // Part of `PRESET_KEYS` and applies immediately (`ApplyMode::Live`).
    OptionSpec::choice(
        "environmentEffects",
        "video",
        "WATER AND STEAM",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_CLASSIC,
    ),
    // 36. `oceanEffects`: ocean wave and sky horizon shaders (`classic` vs. `enhanced`).
    // Default: `"enhanced"`; switches Kamino/Naboo ocean and sky shaders (`CLASSIC` vs. `ENHANCED`).
    // Part of `PRESET_KEYS` and applies immediately (`ApplyMode::Live`).
    OptionSpec::choice(
        "oceanEffects",
        "video",
        "OCEAN AND SKY",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_CLASSIC,
    ),
    // 37. `rainEffects`: Kamino/weather rain streaks and ripples (`classic` vs. `enhanced`).
    // Default: `"enhanced"`; switches weather rain streaks and splashes (`CLASSIC` vs. `ENHANCED`).
    // Part of `PRESET_KEYS` and applies immediately (`ApplyMode::Live`).
    OptionSpec::choice(
        "rainEffects",
        "video",
        "RAIN",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_CLASSIC,
    ),
    // 38. `boltEffects`: blaster bolt rendering (`classic` strips vs. `enhanced` lit plasma bolts).
    // Default: `"enhanced"`; switches blaster bolts between 2004 strips and lit plasma bolts.
    // Part of `PRESET_KEYS` and applies immediately (`ApplyMode::Live`).
    OptionSpec::choice(
        "boltEffects",
        "video",
        "BLASTER BOLTS",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_CLASSIC,
    ),
    // 39. `arcCasterEffects`: Dark Trooper arc caster lightning (`classic` ribbons vs. `enhanced` arcs).
    // Default: `"enhanced"`; switches DN bolt caster arcs between 2004 ribbons and chaining lightning.
    // Part of `PRESET_KEYS` and applies immediately (`ApplyMode::Live`).
    OptionSpec::choice(
        "arcCasterEffects",
        "video",
        "BOLT CASTER",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_CLASSIC,
    ),
    // 40. `sniperEffects`: sniper beam trails and impact sparks (`classic` vs. `enhanced`).
    // Default: `"enhanced"`; switches sniper trails between 2004 streaks and ion rail traces.
    // Part of `PRESET_KEYS` and applies immediately (`ApplyMode::Live`).
    OptionSpec::choice(
        "sniperEffects",
        "video",
        "SNIPER SHOTS",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_CLASSIC,
    ),
    // 41. `jetpackEffects`: jetpack thruster plumes and exhaust lighting (`classic` vs. `enhanced`).
    // Default: `"enhanced"`; switches jetpack exhaust plumes (`CLASSIC` vs. `ENHANCED`).
    // Part of `PRESET_KEYS` and applies immediately (`ApplyMode::Live`).
    OptionSpec::choice(
        "jetpackEffects",
        "video",
        "JETPACKS",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_CLASSIC,
    ),
    // 42. `droidekaEffects`: droideka bubble shield refraction and shimmer (`classic` vs. `enhanced`).
    // Default: `"enhanced"`; switches droideka personal shield envelopes (`CLASSIC` vs. `ENHANCED`).
    // Part of `PRESET_KEYS` and applies immediately (`ApplyMode::Live`).
    OptionSpec::choice(
        "droidekaEffects",
        "video",
        "DROIDEKAS",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_CLASSIC,
    ),
    // 43. `destructionEffects`: droid/vehicle debris and electrical shorting (`classic` vs. `enhanced`).
    // Default: `"enhanced"`; switches droid/vehicle destruction sparks and hot debris.
    // Part of `PRESET_KEYS` and applies immediately (`ApplyMode::Live`).
    OptionSpec::choice(
        "destructionEffects",
        "video",
        "DESTRUCTION",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_CLASSIC,
    ),
    // 44. `deathCamera`: post-death camera framing (`classic` vs. `enhanced` kill-shot replay).
    // Default: `"enhanced"`; switches post-death camera between 2004 framing and kill-shot replay.
    // Part of `PRESET_KEYS` and applies immediately (`ApplyMode::Live`).
    OptionSpec::choice(
        "deathCamera",
        "video",
        "DEATH CAMERA",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_CLASSIC,
    ),
    // 45..55: Audio options
    // 45. `masterVolume`: overall audio output gain (`0%..100%`, default `80%`).
    // Default: `0.8` (`80%`), range `0.0..1.0` in `0.05` steps (`SliderFormat::Percent`).
    // Scales overall audio output across all buses (`ifs.soundopt.mastervol`).
    OptionSpec::slider(
        "masterVolume",
        "audio",
        "MASTER VOLUME",
        Some("ifs.soundopt.mastervol"),
        ApplyMode::Live,
        None,
        0.0,
        1.0,
        0.05,
        SliderFormat::Percent,
    ),
    // 46. `musicVolume`: battle and menu soundtrack gain (`0%..100%`).
    // Default: `1.0` (`100%`), range `0.0..1.0` in `0.05` steps (`SliderFormat::Percent`).
    // Controls John Williams soundtrack gain (`ifs.soundopt.musicvol`).
    OptionSpec::slider(
        "musicVolume",
        "audio",
        "MUSIC VOLUME",
        Some("ifs.soundopt.musicvol"),
        ApplyMode::Live,
        None,
        0.0,
        1.0,
        0.05,
        SliderFormat::Percent,
    ),
    // 47. `effectsVolume`: weapon, explosion, and vehicle SFX gain (`0%..100%`).
    // Default: `1.0` (`100%`), range `0.0..1.0` in `0.05` steps (`SliderFormat::Percent`).
    // Controls blasters, explosions, footsteps, and vehicle engines (`ifs.soundopt.sfxvol`).
    OptionSpec::slider(
        "effectsVolume",
        "audio",
        "SFX VOLUME",
        Some("ifs.soundopt.sfxvol"),
        ApplyMode::Live,
        None,
        0.0,
        1.0,
        0.05,
        SliderFormat::Percent,
    ),
    // 48. `voiceVolume`: unit command chatter and announcer speech gain (`0%..100%`).
    // Default: `1.0` (`100%`), range `0.0..1.0` in `0.05` steps (`SliderFormat::Percent`).
    // Controls faction command post announcements and soldier chatter (`ifs.soundopt.speechvol`).
    OptionSpec::slider(
        "voiceVolume",
        "audio",
        "SPEECH VOLUME",
        Some("ifs.soundopt.speechvol"),
        ApplyMode::Live,
        None,
        0.0,
        1.0,
        0.05,
        SliderFormat::Percent,
    ),
    // 49. `ambienceVolume`: environmental background beds and wind/crowd gain (`0%..100%`).
    // Default: `1.0` (`100%`), range `0.0..1.0` in `0.05` steps (`SliderFormat::Percent`).
    // Controls environmental wind, wildlife, and distant battle ambience.
    OptionSpec::slider(
        "ambienceVolume",
        "audio",
        "AMBIENCE VOLUME",
        None,
        ApplyMode::Live,
        None,
        0.0,
        1.0,
        0.05,
        SliderFormat::Percent,
    ),
    // 50. `interfaceVolume`: menu navigation and HUD beep gain (`0%..100%`).
    // Default: `1.0` (`100%`), range `0.0..1.0` in `0.05` steps (`SliderFormat::Percent`).
    // Controls front-end and pause menu button/selection sound effects.
    OptionSpec::slider(
        "interfaceVolume",
        "audio",
        "INTERFACE VOLUME",
        None,
        ApplyMode::Live,
        None,
        0.0,
        1.0,
        0.05,
        SliderFormat::Percent,
    ),
    // 51. `audioQuality`: sound engine mode (`enhanced` 3D acoustics vs. `original` 2004 stereo mix).
    // Default: `"enhanced"` (or `"original"` on low-end CPUs via `adopt_hardware_defaults`).
    // Footer note explains the active sound stage (`sound_note`).
    OptionSpec::choice(
        "audioQuality",
        "audio",
        "SOUND",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_AUDIO_QUALITY,
    ),
    // 52. `audioOutput`: speaker/headphone layout (`auto`, `headphones`, `stereo`, `5.1`, `7.1`, `7.1.4`).
    // Default: `"auto"` (`AUTOMATIC`); selects speaker/headphone channel layout.
    // Footer note warns when `audioQuality` is set to `"original"` (`enhanced_sound_note`).
    OptionSpec::choice(
        "audioOutput",
        "audio",
        "OUTPUT MODE",
        Some("ifs.soundopt.outputmode.title"),
        ApplyMode::Live,
        None,
        &CHOICES_AUDIO_OUTPUT,
    ),
    // 53. `audioDynamics`: dynamic range profile (`home` theater, `normal`, `night`).
    // Default: `"normal"` (`NORMAL`); selects `HOME THEATER`, `NORMAL`, or `NIGHT` dynamics.
    // Footer note warns when `audioQuality` is set to `"original"` (`enhanced_sound_note`).
    OptionSpec::choice(
        "audioDynamics",
        "audio",
        "DYNAMICS",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_AUDIO_DYNAMICS,
    ),
    // 54. `audioMono`: folds all output channels down to mono.
    // Default: `false` (`OFF`); downmixes stereo/surround output to a single mono channel.
    // Applies immediately (`ApplyMode::Live`, `ifs.soundopt.outputmode.mono`).
    OptionSpec::toggle(
        "audioMono",
        "audio",
        "MONO",
        Some("ifs.soundopt.outputmode.mono"),
        ApplyMode::Live,
        None,
    ),
    // 55. `audioClarity`: enables the high-frequency clarity stage in enhanced sound mode.
    // Default: `true` (`ON`); enables high-frequency clarity processing in enhanced audio.
    // Footer note warns when `audioQuality` is set to `"original"` (`enhanced_sound_note`).
    OptionSpec::toggle("audioClarity", "audio", "CLARITY", None, ApplyMode::Live, None),
    // 56..57: Controls options
    // 56. `mouseSensitivity`: ground/vehicle mouse turn sensitivity (`0.5..10.0` in tenths, `?sens=`).
    // Default: `5.0`, range `0.5..10.0` in `0.1` steps (`Settings::mouse_scale`, `?sens=`).
    // Each `0.1` step changes turn rate by `2^(0.1/5) - 1` (~1.4%).
    OptionSpec::slider(
        "mouseSensitivity",
        "controls",
        "MOUSE SENSITIVITY",
        Some("ifs.GameOpt.pc_mousesens"),
        ApplyMode::Live,
        Some("sens"),
        0.5,
        10.0,
        0.1,
        SliderFormat::OneDecimal,
    ),
    // 57. `invertMouse`: inverts vertical mouse look on foot and in ground vehicles (`NO`/`YES`, `?invert=`).
    // Default: `false` (`NO`); toggles `NO` / `YES` (`common.no` / `common.yes`, `?invert=`).
    // Inverts vertical mouse look on foot and in ground vehicles.
    OptionSpec::toggle_yes_no(
        "invertMouse",
        "controls",
        "INVERT Y AXIS",
        Some("ifs.gameopt.pc_invert"),
        ApplyMode::Live,
        Some("invert"),
    ),
];

/// Keys in [`OPTIONS`] (`OPTION_KEYS` in `src/menus/options.js`), in exact table order.
pub const OPTION_KEYS: [&str; 57] = [
    // 1..6: Starfighter camera, key binding maps, and flight controls.
    "flightFirstPerson",
    "keyBindings",
    "flightBindings",
    "flightSensitivity",
    "flightInvertY",
    "flightResponseCurve",
    // 7..13: Game rules, Instant Action bots, battle size, stats line, and 60 Hz tick rate.
    "friendlyFire",
    "aiHeroes",
    "aiDifficulty",
    "aiMode",
    "battleSize",
    "showStats",
    "physicsRate",
    // 14..26: Video quality preset, frame pacing, upscaling, shadows, textures, water, and distance.
    "performancePreset",
    "frameLimit",
    "dynamicResolution",
    "renderScale",
    "upscaler",
    "upscaleSharpness",
    "antialiasing",
    "shadows",
    "anisotropy",
    "textureCompression",
    "waterQuality",
    "viewDistance",
    "farTerrain",
    // 27..32: Surface shading, post-processing, particles, and dynamic character lamp lighting.
    "bumpMapping",
    "softFocus",
    "bloom",
    "vehicleMotionBlur",
    "softParticles",
    "lampLighting",
    // 33..44: Classic vs. Enhanced terrain, foliage, weather, weapons, shields, and death camera.
    "terrainDetail",
    "treeDetail",
    "environmentEffects",
    "oceanEffects",
    "rainEffects",
    "boltEffects",
    "arcCasterEffects",
    "sniperEffects",
    "jetpackEffects",
    "droidekaEffects",
    "destructionEffects",
    "deathCamera",
    // 45..55: Master and category volume sliders plus sound engine, output, dynamics, mono, clarity.
    "masterVolume",
    "musicVolume",
    "effectsVolume",
    "voiceVolume",
    "ambienceVolume",
    "interfaceVolume",
    "audioQuality",
    "audioOutput",
    "audioDynamics",
    "audioMono",
    "audioClarity",
    // 56..57: Ground/vehicle mouse sensitivity and Y-axis inversion.
    "mouseSensitivity",
    "invertMouse",
];

/// Settings keys edited by the profile screen (`PROFILE_KEYS` in `src/menus/options.js`).
pub const PROFILE_KEYS: [&str; 1] = ["playerName"];

/// Settings keys edited by the menu theme selector (`THEME_KEYS` in `src/menus/options.js`).
pub const THEME_KEYS: [&str; 1] = ["menuTheme"];

/// Returns all [`OptionSpec`]s belonging to `page` (`optionsFor(page)` in `src/menus/options.js`).
///
/// Preserves the declaration order in [`OPTIONS`] for `"game"`, `"video"`, `"audio"`, and `"controls"`.
pub fn options_for(page: &str) -> Vec<&'static OptionSpec> {
    OPTIONS.iter().filter(|o| o.page == page).collect()
}

/// Looks up an [`OptionSpec`] by setting `key` (`optionEntry(key)` in `src/menus/options.js`).
#[inline]
pub fn option_entry(key: &str) -> Option<&'static OptionSpec> {
    OPTIONS.iter().find(|o| o.key == key)
}

/// Returns when changes to `key` apply (`applies(key)` in `src/menus/options.js`).
#[inline]
pub fn applies(key: &str) -> Option<ApplyMode> {
    option_entry(key).map(|o| o.apply)
}

/// Resolves a localized string from `strings`, collapsing newlines (`say` in `src/menus/options.js`).
///
/// Falls back to `fallback` when `key` is `None` or absent from `strings`.
pub fn say(strings: &HashMap<String, String>, key: Option<&str>, fallback: &str) -> String {
    let raw = key
        .and_then(|k| strings.get(k))
        .map(String::as_str)
        .unwrap_or(fallback);
    raw.replace("\r\n", " ").replace('\n', " ").trim().to_string()
}

/// Computes the scaled render resolution string (`"WIDTHxHEIGHT"`, e.g. `"1024x768"`)
/// for `renderScale` (`renderResolution` in `src/menus/options.js`).
///
/// Multiplies the viewport dimensions `[vw, vh]` by the current `renderScale` setting,
/// clamping each dimension to at least `1` pixel.
pub fn render_resolution(settings: &Settings, viewport: Option<[u32; 2]>) -> Option<String> {
    let [vw, vh] = viewport?;
    if vw == 0 || vh == 0 {
        return None;
    }
    let scale = settings.get_f64("renderScale");
    let s = if scale.is_finite() { scale } else { 1.0 };
    let w = ((vw as f64) * s).round().max(1.0) as u32;
    let h = ((vh as f64) * s).round().max(1.0) as u32;
    Some(format!("{w}x{h}"))
}

/// Note shown under `audioQuality` (`soundNote` in `src/menus/options.js`).
///
/// Describes either the classic 2004 stereo mix or the 3D positional sound stage.
pub fn sound_note(settings: &Settings, strings: &HashMap<String, String>, in_battle: bool) -> String {
    let restart = if in_battle {
        "RESTART MISSION"
    } else {
        "NEXT BATTLE"
    };
    if settings.get_string("audioQuality") == "original" {
        say(
            strings,
            Some("bfe.audio.classicSoundNote"),
            &format!("CLASSIC SOUND: STEREO MIX, LIGHTER CPU USE ({restart})"),
        )
    } else {
        say(
            strings,
            Some("bfe.audio.enhancedSoundNote"),
            "3D POSITIONAL SOUND, DISTANCE AND ROOM ACOUSTICS",
        )
    }
}

/// Note shown under `audioOutput`, `audioDynamics`, and `audioClarity` when `audioQuality == "original"`
/// (`enhancedSoundNote` in `src/menus/options.js`).
pub fn enhanced_sound_note(settings: &Settings, strings: &HashMap<String, String>) -> String {
    if settings.get_string("audioQuality") == "original" {
        say(
            strings,
            Some("bfe.audio.usedWithEnhanced"),
            "USED WHEN SOUND IS SET TO ENHANCED",
        )
    } else {
        String::new()
    }
}

/// Evaluates the footer note for `key` (`note` and `NOTES` in `src/menus/options.js`).
///
/// Checks option-specific dynamic notes first (`renderScale`, `upscaler`, `upscaleSharpness`,
/// `audioQuality`, `audioOutput`, `audioDynamics`, `audioClarity`) and falls back to the
/// [`ApplyMode`] timing note (`Map` or `Battle`) when empty.
pub fn option_note(
    key: &str,
    settings: &Settings,
    strings: &HashMap<String, String>,
    in_battle: bool,
    viewport: Option<[u32; 2]>,
) -> String {
    let Some(spec) = option_entry(key) else {
        return String::new();
    };
    // Option-specific dynamic notes take precedence over generic `ApplyMode` timing notes.
    let custom = match key {
        "renderScale" => render_resolution(settings, viewport).unwrap_or_default(),
        "upscaler" => {
            if settings.get_string("upscaler") == "temporal" {
                "ACCUMULATES DETAIL ACROSS FRAMES WHEN RENDER SCALE IS BELOW 1.00X".to_string()
            } else {
                String::new()
            }
        }
        "upscaleSharpness" => {
            let u = settings.get_string("upscaler");
            if u != "spatial" && u != "temporal" {
                "USED BY SPATIAL AND TEMPORAL UPSCALERS".to_string()
            } else {
                String::new()
            }
        }
        "audioQuality" => sound_note(settings, strings, in_battle),
        "audioOutput" | "audioDynamics" | "audioClarity" => enhanced_sound_note(settings, strings),
        _ => String::new(),
    };
    if !custom.is_empty() {
        return custom;
    }
    // Generic timing note based on whether the option applies on map load or battle start.
    match spec.apply {
        ApplyMode::Map => {
            if in_battle {
                "TAKES EFFECT AFTER RESTARTING THE MISSION".to_string()
            } else {
                "TAKES EFFECT ON THE NEXT MAP".to_string()
            }
        }
        ApplyMode::Battle => {
            if in_battle {
                "TAKES EFFECT AFTER RESTARTING THE MISSION".to_string()
            } else {
                "TAKES EFFECT IN THE NEXT BATTLE".to_string()
            }
        }
        ApplyMode::Live | ApplyMode::Fixed => String::new(),
    }
}

/// Formats the current value of `spec` in `settings` as display text (`valueText` in `optionRow`
/// in `src/menus/options.js`).
pub fn format_option_value(
    spec: &OptionSpec,
    settings: &Settings,
    strings: &HashMap<String, String>,
) -> String {
    let val = settings.get(spec.key);
    match spec.kind {
        // Look up the matching choice item's localized label, or format raw value as fallback.
        OptionKind::Choice => {
            if let Some(choice) = spec.choices.iter().find(|c| c.value.matches(&val)) {
                say(strings, choice.label_key, choice.label)
            } else {
                match val {
                    SettingValue::Text(s) => s.to_ascii_uppercase(),
                    SettingValue::Int(i) => i.to_string(),
                    SettingValue::Number(n) => n.to_string(),
                    SettingValue::Bool(b) => if b { "ON" } else { "OFF" }.to_string(),
                    SettingValue::Bindings(_) => String::new(),
                }
            }
        }
        // Index `0` (`false`) or `1` (`true`) into the spec's toggle labels (`OFF`/`ON` or `NO`/`YES`).
        OptionKind::Toggle => {
            let idx = usize::from(val.as_bool().unwrap_or(false));
            say(strings, spec.toggle_label_keys[idx], spec.toggle_labels[idx])
        }
        // Format numeric slider values via `Percent`, `ScaleX`, or `OneDecimal`.
        OptionKind::Slider => spec.format.format(val.as_f64().unwrap_or(spec.min)),
        // Fixed rows use their static `display` text (e.g. `"60 HZ"`) when provided.
        OptionKind::Fixed => spec
            .display
            .map(str::to_string)
            .unwrap_or_else(|| match val {
                SettingValue::Text(s) => s,
                SettingValue::Int(i) => i.to_string(),
                SettingValue::Number(n) => n.to_string(),
                _ => String::new(),
            }),
        OptionKind::Action => String::new(),
    }
}

/// Steps `spec` in `settings` by `dir` (`-1` or `+1`), matching `optionRow.step` in `src/menus/options.js`.
///
/// - `Choice`: wraps around `spec.choices` modulo `choices.len()`.
/// - `Toggle`: negates the current boolean value.
/// - `Slider`: increments by `dir * spec.step`, rounds to the nearest step, and clamps to `[min, max]`.
/// - `Fixed` / `Action`: no-op (`returns false`).
pub fn step_option(spec: &OptionSpec, settings: &mut Settings, dir: i32) -> bool {
    let step_dir = if dir < 0 { -1 } else { 1 };
    match spec.kind {
        OptionKind::Choice => {
            if spec.choices.is_empty() {
                return false;
            }
            let current = settings.get(spec.key);
            let idx = spec
                .choices
                .iter()
                .position(|c| c.value.matches(&current))
                .unwrap_or(0) as isize;
            let len = spec.choices.len() as isize;
            let next = (idx + (step_dir as isize)).rem_euclid(len) as usize;
            settings.set(spec.key, spec.choices[next].value.to_setting_value());
            true
        }
        OptionKind::Toggle => {
            let cur = settings.get_bool(spec.key);
            settings.set(spec.key, !cur);
            true
        }
        OptionKind::Slider => {
            let cur = settings.get_f64(spec.key);
            let next = (cur + (step_dir as f64) * spec.step).clamp(spec.min, spec.max);
            let rounded = ((next / spec.step).round() * spec.step * 1000.0).round() / 1000.0;
            let clamped = rounded.clamp(spec.min, spec.max);
            settings.set(spec.key, clamped);
            true
        }
        OptionKind::Fixed | OptionKind::Action => false,
    }
}

/// Resets all settings on `page` back to their [`crate::settings::DEFAULTS`] values
/// (`resetPage(page, settings)` in `src/menus/options.js`).
pub fn reset_page(page: &str, settings: &mut Settings) {
    let keys: Vec<&'static str> = options_for(page).into_iter().map(|o| o.key).collect();
    settings.reset_keys(keys);
}

/// Returns true if every preset in `PERFORMANCE_PRESETS` is represented in the
/// `performancePreset` choice list (`src/menus/options.js` `PRESET_CHOICES`).
pub fn all_performance_presets_in_choices() -> bool {
    let Some(spec) = option_entry("performancePreset") else {
        return false;
    };
    PERFORMANCE_PRESETS.iter().all(|p| {
        spec.choices
            .iter()
            .any(|c| c.value == ChoiceVal::Text(p.id))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{default_for, DEFAULTS};

    #[test]
    fn every_settings_key_is_present_with_default_and_page() {
        assert_eq!(DEFAULTS.len(), 59);
        for def in DEFAULTS {
            assert!(
                default_for(def.key).is_some(),
                "missing default_for({})",
                def.key
            );
            let in_options = option_entry(def.key).is_some();
            let in_profile = PROFILE_KEYS.contains(&def.key);
            let in_theme = THEME_KEYS.contains(&def.key);
            assert!(
                in_options || in_profile || in_theme,
                "key {} is missing from OPTIONS / PROFILE_KEYS / THEME_KEYS",
                def.key
            );
        }
        assert!(!OPTIONS.is_empty());
        for opt in OPTIONS {
            assert!(
                default_for(opt.key).is_some(),
                "option {} missing in DEFAULTS",
                opt.key
            );
            assert!(!opt.page.is_empty());
            assert!(!opt.kind.as_str().is_empty());
            assert!(!opt.apply.as_str().is_empty());
        }
        assert!(all_performance_presets_in_choices());
    }
}
