//! Option pages, schema, notes, and row builders (`src/menus/options.js`).
//!
//! Ports `PAGES`, `OPTIONS` (all 57 entries in exact order), `OPTION_KEYS`,
//! `PROFILE_KEYS`, `THEME_KEYS`, `optionsFor`, `optionEntry`, `applies`, `NOTES`,
//! `renderResolution`, `soundNote`, `enhancedSoundNote`, `optionRow`, and `resetPage`
//! from `src/menus/options.js`.

use crate::settings::{SettingValue, Settings, PERFORMANCE_PRESETS};
use std::collections::HashMap;

/// Option page descriptor (`PAGES` in `src/menus/options.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageDef {
    pub id: &'static str,
    pub title: &'static str,
    pub title_key: &'static str,
}

/// The four option sub-pages (`PAGES` in `src/menus/options.js`).
pub const PAGES: [PageDef; 4] = [
    PageDef {
        id: "game",
        title: "GAME OPTIONS",
        title_key: "ifs.GameOpt.title",
    },
    PageDef {
        id: "video",
        title: "VIDEO OPTIONS",
        title_key: "ifs.VideoOpt.title",
    },
    PageDef {
        id: "audio",
        title: "AUDIO OPTIONS",
        title_key: "ifs.SoundOpt.title",
    },
    PageDef {
        id: "controls",
        title: "CONTROL OPTIONS",
        title_key: "ifs.GameOpt.pc_optionstitle",
    },
];

/// Widget control kind for an entry in [`OPTIONS`] (`kind` in `src/menus/options.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionKind {
    Choice,
    Toggle,
    Slider,
    Fixed,
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
    Text(&'static str),
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

    /// Returns true if `val` matches this choice literal.
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
    pub value: ChoiceVal,
    pub label: &'static str,
    pub label_key: Option<&'static str>,
}

impl ChoiceItem {
    const fn s(value: &'static str, label: &'static str) -> Self {
        Self {
            value: ChoiceVal::Text(value),
            label,
            label_key: None,
        }
    }

    const fn sk(value: &'static str, label: &'static str, label_key: &'static str) -> Self {
        Self {
            value: ChoiceVal::Text(value),
            label,
            label_key: Some(label_key),
        }
    }

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
    pub key: &'static str,
    pub page: &'static str,
    pub label: &'static str,
    pub label_key: Option<&'static str>,
    pub kind: OptionKind,
    pub apply: ApplyMode,
    pub url: Option<&'static str>,
    pub choices: &'static [ChoiceItem],
    pub toggle_labels: [&'static str; 2],
    pub toggle_label_keys: [Option<&'static str>; 2],
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub format: SliderFormat,
    pub display: Option<&'static str>,
}

const CHOICES_EMPTY: [ChoiceItem; 0] = [];

const CHOICES_CLASSIC: [ChoiceItem; 2] = [
    ChoiceItem::s("classic", "CLASSIC"),
    ChoiceItem::s("enhanced", "ENHANCED"),
];

const CHOICES_ORIGINAL: [ChoiceItem; 2] = [
    ChoiceItem::s("original", "CLASSIC"),
    ChoiceItem::s("enhanced", "ENHANCED"),
];

const CHOICES_DIFFICULTY: [ChoiceItem; 3] = [
    ChoiceItem::sk("easy", "EASY", "ifs.difficulty.easy"),
    ChoiceItem::s("medium", "NORMAL"),
    ChoiceItem::s("hard", "ELITE"),
];

const CHOICES_AI_MODE: [ChoiceItem; 3] = [
    ChoiceItem::sk("off", "OFF", "common.off"),
    ChoiceItem::s("classic", "CLASSIC"),
    ChoiceItem::s("enhanced", "ENHANCED"),
];

const CHOICES_BATTLE_SIZE: [ChoiceItem; 3] = [
    ChoiceItem::s("full", "FULL"),
    ChoiceItem::s("large", "REDUCED"),
    ChoiceItem::s("small", "SMALL"),
];

const CHOICES_PERFORMANCE_PRESET: [ChoiceItem; 5] = [
    ChoiceItem::s("custom", "CUSTOM"),
    ChoiceItem::s("minimum", "MINIMUM"),
    ChoiceItem::s("performance", "PERFORMANCE"),
    ChoiceItem::s("balanced", "BALANCED"),
    ChoiceItem::s("quality", "HIGH QUALITY"),
];

const CHOICES_FRAME_LIMIT: [ChoiceItem; 6] = [
    ChoiceItem::s("0", "DISPLAY"),
    ChoiceItem::s("30", "30 FPS"),
    ChoiceItem::s("60", "60 FPS"),
    ChoiceItem::s("120", "120 FPS"),
    ChoiceItem::s("144", "144 FPS"),
    ChoiceItem::s("240", "240 FPS"),
];

const CHOICES_DYNAMIC_RESOLUTION: [ChoiceItem; 6] = [
    ChoiceItem::s("0", "OFF"),
    ChoiceItem::s("30", "30 FPS"),
    ChoiceItem::s("60", "60 FPS"),
    ChoiceItem::s("120", "120 FPS"),
    ChoiceItem::s("144", "144 FPS"),
    ChoiceItem::s("240", "240 FPS"),
];

const CHOICES_UPSCALER: [ChoiceItem; 4] = [
    ChoiceItem::s("off", "OFF"),
    ChoiceItem::s("bilinear", "BILINEAR"),
    ChoiceItem::s("spatial", "SPATIAL"),
    ChoiceItem::s("temporal", "TEMPORAL"),
];

const CHOICES_ANTIALIASING: [ChoiceItem; 6] = [
    ChoiceItem::s("off", "OFF"),
    ChoiceItem::s("fxaa-fast", "FXAA (FAST)"),
    ChoiceItem::s("fxaa", "FXAA"),
    ChoiceItem::s("2x", "2X MSAA"),
    ChoiceItem::s("4x", "4X MSAA"),
    ChoiceItem::s("8x", "8X MSAA"),
];

const CHOICES_SHADOWS: [ChoiceItem; 5] = [
    ChoiceItem::s("off", "OFF"),
    ChoiceItem::s("low", "LOW"),
    ChoiceItem::s("medium", "MEDIUM"),
    ChoiceItem::s("high", "HIGH"),
    ChoiceItem::s("ultra", "ULTRA"),
];

const CHOICES_ANISOTROPY: [ChoiceItem; 5] = [
    ChoiceItem::i(1, "TRILINEAR"),
    ChoiceItem::i(2, "2X ANISOTROPIC"),
    ChoiceItem::i(4, "4X ANISOTROPIC"),
    ChoiceItem::i(8, "8X ANISOTROPIC"),
    ChoiceItem::i(16, "16X ANISOTROPIC"),
];

const CHOICES_WATER_QUALITY: [ChoiceItem; 3] = [
    ChoiceItem::s("low", "LOW"),
    ChoiceItem::s("high", "HIGH"),
    ChoiceItem::s("ultra", "ULTRA"),
];

const CHOICES_VIEW_DISTANCE: [ChoiceItem; 3] = [
    ChoiceItem::s("original", "ORIGINAL"),
    ChoiceItem::s("far", "FAR (2X)"),
    ChoiceItem::s("max", "MAXIMUM (4X)"),
];

const CHOICES_FAR_TERRAIN: [ChoiceItem; 2] = [
    ChoiceItem::s("full", "FULL"),
    ChoiceItem::s("reduced", "REDUCED"),
];

const CHOICES_AUDIO_QUALITY: [ChoiceItem; 2] = [
    ChoiceItem::s("enhanced", "ENHANCED"),
    ChoiceItem::s("original", "CLASSIC"),
];

const CHOICES_AUDIO_OUTPUT: [ChoiceItem; 6] = [
    ChoiceItem::s("auto", "AUTOMATIC"),
    ChoiceItem::s("headphones", "HEADPHONES"),
    ChoiceItem::s("stereo", "STEREO"),
    ChoiceItem::s("5.1", "5.1"),
    ChoiceItem::s("7.1", "7.1"),
    ChoiceItem::s("7.1.4", "7.1.4"),
];

const CHOICES_AUDIO_DYNAMICS: [ChoiceItem; 3] = [
    ChoiceItem::s("home", "HOME THEATER"),
    ChoiceItem::s("normal", "NORMAL"),
    ChoiceItem::s("night", "NIGHT"),
];

impl OptionSpec {
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
    OptionSpec::toggle(
        "flightFirstPerson",
        "game",
        "STARFIGHTER COCKPIT VIEW",
        None,
        ApplyMode::Live,
        None,
    ),
    OptionSpec::fixed("keyBindings", "controls", "KEY BINDINGS", ApplyMode::Live, None),
    OptionSpec::fixed(
        "flightBindings",
        "controls",
        "STARFIGHTER BINDINGS",
        ApplyMode::Live,
        None,
    ),
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
    OptionSpec::toggle(
        "flightInvertY",
        "controls",
        "STARFIGHTER INVERT Y",
        None,
        ApplyMode::Live,
        None,
    ),
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
    OptionSpec::toggle(
        "friendlyFire",
        "game",
        "FRIENDLY FIRE",
        Some("ifs.gameopt.friendlyfire"),
        ApplyMode::Live,
        None,
    ),
    OptionSpec::toggle(
        "aiHeroes",
        "game",
        "INSTANT ACTION HEROES",
        Some("ifs.gameopt.hero"),
        ApplyMode::Battle,
        Some("heroes"),
    ),
    OptionSpec::choice(
        "aiDifficulty",
        "game",
        "DIFFICULTY",
        Some("ifs.gameopt.diff"),
        ApplyMode::Battle,
        Some("tier"),
        &CHOICES_DIFFICULTY,
    ),
    OptionSpec::choice(
        "aiMode",
        "game",
        "BATTLE AI",
        None,
        ApplyMode::Battle,
        Some("ai"),
        &CHOICES_AI_MODE,
    ),
    OptionSpec::choice(
        "battleSize",
        "game",
        "BATTLE SIZE (MAX 32 PER TEAM)",
        None,
        ApplyMode::Battle,
        Some("size"),
        &CHOICES_BATTLE_SIZE,
    ),
    OptionSpec::toggle("showStats", "game", "STATS LINE", None, ApplyMode::Live, None),
    OptionSpec::fixed(
        "physicsRate",
        "game",
        "SIMULATION RATE",
        ApplyMode::Fixed,
        Some("60 HZ"),
    ),
    // 14..44: Video options
    OptionSpec::choice(
        "performancePreset",
        "video",
        "QUALITY PRESET",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_PERFORMANCE_PRESET,
    ),
    OptionSpec::choice(
        "frameLimit",
        "video",
        "FRAME LIMIT",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_FRAME_LIMIT,
    ),
    OptionSpec::choice(
        "dynamicResolution",
        "video",
        "ADAPTIVE RESOLUTION",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_DYNAMIC_RESOLUTION,
    ),
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
    OptionSpec::choice(
        "upscaler",
        "video",
        "UPSCALER",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_UPSCALER,
    ),
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
    OptionSpec::choice(
        "antialiasing",
        "video",
        "ANTIALIASING",
        Some("ifs.videopt.multisample"),
        ApplyMode::Live,
        None,
        &CHOICES_ANTIALIASING,
    ),
    OptionSpec::choice(
        "shadows",
        "video",
        "SHADOW QUALITY",
        Some("ifs.videopt.shadows"),
        ApplyMode::Live,
        Some("shadows"),
        &CHOICES_SHADOWS,
    ),
    OptionSpec::choice(
        "anisotropy",
        "video",
        "TEXTURE FILTERING",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_ANISOTROPY,
    ),
    OptionSpec::toggle(
        "textureCompression",
        "video",
        "COMPRESSED TEXTURES",
        None,
        ApplyMode::Map,
        None,
    ),
    OptionSpec::choice(
        "waterQuality",
        "video",
        "WATER QUALITY",
        Some("ifs.videoopt.waterquality"),
        ApplyMode::Live,
        None,
        &CHOICES_WATER_QUALITY,
    ),
    OptionSpec::choice(
        "viewDistance",
        "video",
        "VIEW DISTANCE",
        Some("ifs.videopt.viewdistance"),
        ApplyMode::Live,
        None,
        &CHOICES_VIEW_DISTANCE,
    ),
    OptionSpec::choice(
        "farTerrain",
        "video",
        "FAR TERRAIN",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_FAR_TERRAIN,
    ),
    OptionSpec::toggle(
        "bumpMapping",
        "video",
        "BUMPMAPPING",
        Some("ifs.videopt.bumpmapping"),
        ApplyMode::Live,
        None,
    ),
    OptionSpec::toggle("softFocus", "video", "SOFT FOCUS", None, ApplyMode::Live, None),
    OptionSpec::toggle(
        "bloom",
        "video",
        "BLOOM AND GLOW",
        None,
        ApplyMode::Live,
        None,
    ),
    OptionSpec::toggle(
        "vehicleMotionBlur",
        "video",
        "MOTION BLUR",
        Some("ifs.videoopt.motionblur"),
        ApplyMode::Live,
        None,
    ),
    OptionSpec::toggle(
        "softParticles",
        "video",
        "SOFT PARTICLES",
        None,
        ApplyMode::Live,
        None,
    ),
    OptionSpec::toggle(
        "lampLighting",
        "video",
        "LAMP LIGHT ON CHARACTERS",
        None,
        ApplyMode::Live,
        None,
    ),
    OptionSpec::choice(
        "terrainDetail",
        "video",
        "TERRAIN",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_ORIGINAL,
    ),
    OptionSpec::choice(
        "treeDetail",
        "video",
        "TREES",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_ORIGINAL,
    ),
    OptionSpec::choice(
        "environmentEffects",
        "video",
        "WATER AND STEAM",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_CLASSIC,
    ),
    OptionSpec::choice(
        "oceanEffects",
        "video",
        "OCEAN AND SKY",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_CLASSIC,
    ),
    OptionSpec::choice(
        "rainEffects",
        "video",
        "RAIN",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_CLASSIC,
    ),
    OptionSpec::choice(
        "boltEffects",
        "video",
        "BLASTER BOLTS",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_CLASSIC,
    ),
    OptionSpec::choice(
        "arcCasterEffects",
        "video",
        "BOLT CASTER",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_CLASSIC,
    ),
    OptionSpec::choice(
        "sniperEffects",
        "video",
        "SNIPER SHOTS",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_CLASSIC,
    ),
    OptionSpec::choice(
        "jetpackEffects",
        "video",
        "JETPACKS",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_CLASSIC,
    ),
    OptionSpec::choice(
        "droidekaEffects",
        "video",
        "DROIDEKAS",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_CLASSIC,
    ),
    OptionSpec::choice(
        "destructionEffects",
        "video",
        "DESTRUCTION",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_CLASSIC,
    ),
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
    OptionSpec::choice(
        "audioQuality",
        "audio",
        "SOUND",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_AUDIO_QUALITY,
    ),
    OptionSpec::choice(
        "audioOutput",
        "audio",
        "OUTPUT MODE",
        Some("ifs.soundopt.outputmode.title"),
        ApplyMode::Live,
        None,
        &CHOICES_AUDIO_OUTPUT,
    ),
    OptionSpec::choice(
        "audioDynamics",
        "audio",
        "DYNAMICS",
        None,
        ApplyMode::Live,
        None,
        &CHOICES_AUDIO_DYNAMICS,
    ),
    OptionSpec::toggle(
        "audioMono",
        "audio",
        "MONO",
        Some("ifs.soundopt.outputmode.mono"),
        ApplyMode::Live,
        None,
    ),
    OptionSpec::toggle("audioClarity", "audio", "CLARITY", None, ApplyMode::Live, None),
    // 56..57: Controls options
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
    OptionSpec::toggle_yes_no(
        "invertMouse",
        "controls",
        "INVERT Y AXIS",
        Some("ifs.gameopt.pc_invert"),
        ApplyMode::Live,
        Some("invert"),
    ),
];

/// Keys in [`OPTIONS`] (`OPTION_KEYS` in `src/menus/options.js`).
pub const OPTION_KEYS: [&str; 57] = [
    "flightFirstPerson",
    "keyBindings",
    "flightBindings",
    "flightSensitivity",
    "flightInvertY",
    "flightResponseCurve",
    "friendlyFire",
    "aiHeroes",
    "aiDifficulty",
    "aiMode",
    "battleSize",
    "showStats",
    "physicsRate",
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
    "bumpMapping",
    "softFocus",
    "bloom",
    "vehicleMotionBlur",
    "softParticles",
    "lampLighting",
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
    "mouseSensitivity",
    "invertMouse",
];

/// Settings keys edited by the profile screen (`PROFILE_KEYS` in `src/menus/options.js`).
pub const PROFILE_KEYS: [&str; 1] = ["playerName"];

/// Settings keys edited by the menu theme selector (`THEME_KEYS` in `src/menus/options.js`).
pub const THEME_KEYS: [&str; 1] = ["menuTheme"];

/// Returns all [`OptionSpec`]s belonging to `page` (`optionsFor(page)` in `src/menus/options.js`).
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
pub fn say(strings: &HashMap<String, String>, key: Option<&str>, fallback: &str) -> String {
    let raw = key
        .and_then(|k| strings.get(k))
        .map(String::as_str)
        .unwrap_or(fallback);
    raw.replace("\r\n", " ").replace('\n', " ").trim().to_string()
}

/// Computes the scaled render resolution string (`"WIDTHxHEIGHT"`, e.g. `"1024x768"`)
/// for `renderScale` (`renderResolution` in `src/menus/options.js`).
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
        OptionKind::Toggle => {
            let idx = usize::from(val.as_bool().unwrap_or(false));
            say(strings, spec.toggle_label_keys[idx], spec.toggle_labels[idx])
        }
        OptionKind::Slider => spec.format.format(val.as_f64().unwrap_or(spec.min)),
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
    use crate::settings::{default_for, DefaultVal, DEFAULTS};

    // Gate test: asserts that every single key of `src/settings.js` `DEFAULTS` is present
    // in `bfe_interface::settings::DEFAULTS` with its exact default value and in
    // `bfe_interface::options::OPTIONS` (or `PROFILE_KEYS` / `THEME_KEYS`) with its exact page and order.
    #[test]
    fn every_settings_js_key_is_present_with_default_and_page() {
        let js_settings = include_str!("../../../src/settings.js");
        let js_options = include_str!("../../../src/menus/options.js");

        // Extract the `export const DEFAULTS = { ... };` block from `src/settings.js`.
        let defaults_body = js_settings
            .split("export const DEFAULTS = {")
            .nth(1)
            .expect("DEFAULTS block in src/settings.js")
            .split("};")
            .next()
            .expect("end of DEFAULTS block");

        let mut js_keys_and_vals: Vec<(&str, &str)> = Vec::new();
        for raw_line in defaults_body.lines() {
            let line = raw_line.split("//").next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if let Some((k, v)) = line.split_once(':') {
                let key = k.trim();
                let val = v.trim().trim_end_matches(',').trim();
                js_keys_and_vals.push((key, val));
            }
        }

        assert_eq!(
            js_keys_and_vals.len(),
            DEFAULTS.len(),
            "Rust DEFAULTS count must match src/settings.js DEFAULTS count"
        );

        for (idx, &(js_key, js_val)) in js_keys_and_vals.iter().enumerate() {
            assert_eq!(
                DEFAULTS[idx].key, js_key,
                "DEFAULTS key order mismatch at index {idx}"
            );
            let rust_def = default_for(js_key).expect("key must exist in Rust DEFAULTS");
            match rust_def {
                DefaultVal::Bool(b) => {
                    assert_eq!(if b { "true" } else { "false" }, js_val, "mismatch for {js_key}");
                }
                DefaultVal::Int(i) => {
                    assert_eq!(i.to_string(), js_val, "mismatch for {js_key}");
                }
                DefaultVal::Number(n) => {
                    let parsed: f64 = js_val.parse().expect("numeric default in settings.js");
                    assert!((n - parsed).abs() <= 1e-9, "mismatch for {js_key}");
                }
                DefaultVal::Text(s) => {
                    let unquoted = js_val.trim_matches('"');
                    assert_eq!(s, unquoted, "mismatch for {js_key}");
                }
                DefaultVal::Bindings => {
                    assert_eq!(js_val, "{}", "mismatch for {js_key}");
                }
            }

            // Every key of `DEFAULTS` must be covered by `OPTIONS`, `PROFILE_KEYS`, or `THEME_KEYS`.
            let in_options = option_entry(js_key).is_some();
            let in_profile = PROFILE_KEYS.contains(&js_key);
            let in_theme = THEME_KEYS.contains(&js_key);
            assert!(
                in_options || in_profile || in_theme,
                "key {js_key} from settings.js is missing from OPTIONS / PROFILE_KEYS / THEME_KEYS"
            );
        }

        // Also parse `export const OPTIONS = [ ... ];` from `src/menus/options.js` and verify
        // every single entry's key, page, kind, and apply order!
        let options_body = js_options
            .split("export const OPTIONS = [")
            .nth(1)
            .expect("OPTIONS block in src/menus/options.js")
            .split("\n];")
            .next()
            .expect("end of OPTIONS block");

        let mut js_option_entries: Vec<(&str, &str, &str, &str)> = Vec::new();
        for chunk in options_body.split("{ key:").skip(1) {
            let key = chunk
                .split('"')
                .nth(1)
                .unwrap_or("");
            let page = chunk
                .split("page:")
                .nth(1)
                .and_then(|s| s.split('"').nth(1))
                .unwrap_or("");
            let kind = chunk
                .split("kind:")
                .nth(1)
                .and_then(|s| s.split('"').nth(1))
                .unwrap_or("");
            let apply = chunk
                .split("apply:")
                .nth(1)
                .and_then(|s| s.split('"').nth(1))
                .unwrap_or("");
            js_option_entries.push((key, page, kind, apply));
        }

        assert_eq!(js_option_entries.len(), OPTIONS.len());
        for (i, &(k, p, kind, apply)) in js_option_entries.iter().enumerate() {
            assert_eq!(OPTIONS[i].key, k, "OPTIONS[{i}] key mismatch");
            assert_eq!(OPTIONS[i].page, p, "OPTIONS[{i}] ({k}) page mismatch");
            assert_eq!(
                OPTIONS[i].kind.as_str(),
                kind,
                "OPTIONS[{i}] ({k}) kind mismatch"
            );
            assert_eq!(
                OPTIONS[i].apply.as_str(),
                apply,
                "OPTIONS[{i}] ({k}) apply mismatch"
            );
        }

        assert!(all_performance_presets_in_choices());
    }
}
