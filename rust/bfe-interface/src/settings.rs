//! Persistent player, video, audio, and control settings (`src/settings.js`,
//! `src/gpu-info.js`, `src/gpu-policy.json`, `src/cpu-info.js`).
//!
//! Ports all 59 keys of `DEFAULTS`, `BATTLE_SIZES`, `PHYSICS_RATES`,
//! `RENDER_SCALE_MIN`, `WORLD_SETTINGS`, `PERFORMANCE_PRESETS`, `PRESET_KEYS`,
//! `ANTIALIASING`, `WATER_QUALITIES`, `VIEW_DISTANCES`, `SHADOW_PRESETS`,
//! `Settings` (`get`, `get_saved`, `set`, `override_value`, `clear_overrides`,
//! `reset`, `reset_keys`, `adopt_hardware_defaults`, `mouse_scale`, desktop
//! `{ complete, values }` and JSON file persistence), and the GPU/CPU hardware
//! classification rules (`classify_gpu`, `is_low_end_cpu`).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Minimum render scale slider value (`RENDER_SCALE_MIN = 0.25` in `src/settings.js`).
pub const RENDER_SCALE_MIN: f64 = 0.25;

/// Supported simulation tick rates (`PHYSICS_RATES = ["60"]` in `src/settings.js`).
pub const PHYSICS_RATES: [&str; 1] = ["60"];

/// Settings that belong to the shared battle world rather than an individual client
/// (`WORLD_SETTINGS = ["friendlyFire"]` in `src/settings.js`).
pub const WORLD_SETTINGS: [&str; 1] = ["friendlyFire"];

/// Antialiasing mode identifiers (`ANTIALIASING` in `src/settings.js`).
pub const ANTIALIASING: [&str; 6] = ["off", "fxaa-fast", "fxaa", "2x", "4x", "8x"];

/// Water quality tier identifiers (`WATER_QUALITIES` in `src/settings.js`).
pub const WATER_QUALITIES: [&str; 3] = ["low", "high", "ultra"];

/// View distance multipliers (`VIEW_DISTANCES` in `src/settings.js`).
pub const VIEW_DISTANCES: [(&str, u32); 3] = [("original", 1), ("far", 2), ("max", 4)];

/// Battle size unit caps per team (`BATTLE_SIZES` in `src/settings.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BattleSizeCap {
    /// Preset identifier (`"full"`, `"large"`, or `"small"`).
    pub id: &'static str,
    /// Active unit cap per team in Clone Wars (`cw`) era missions.
    pub cw: u32,
    /// Active unit cap per team in Galactic Civil War (`gcw`) era missions.
    pub gcw: u32,
}

/// Battle size presets (`BATTLE_SIZES` in `src/settings.js`).
pub const BATTLE_SIZES: [BattleSizeCap; 3] = [
    // Full 2004 unit count (32 active units per team).
    BattleSizeCap {
        id: "full",
        cw: 32,
        gcw: 32,
    },
    // Reduced battle size (16 active units per team).
    BattleSizeCap {
        id: "large",
        cw: 16,
        gcw: 16,
    },
    // Small battle size for older CPUs (8 active units per team).
    BattleSizeCap {
        id: "small",
        cw: 8,
        gcw: 8,
    },
];

/// Shadow quality preset parameters (`SHADOW_PRESETS` in `src/settings.js`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowPreset {
    /// Preset tier identifier (`"off"`, `"low"`, `"medium"`, `"high"`, or `"ultra"`).
    pub id: &'static str,
    /// Whether shadow map rendering is active.
    pub enabled: bool,
    /// Shadow map resolution in texels (`0`, `512`, `1024`, `2048`, or `4096`).
    pub map_size: u32,
    /// PCF filtering kernel radius multiplier.
    pub radius: f64,
    /// Whether terrain receives and casts cascaded shadows.
    pub terrain: bool,
    /// Whether static scene props cast full-resolution shadows.
    pub objects: bool,
}

/// Shadow quality presets (`SHADOW_PRESETS` in `src/settings.js`).
pub const SHADOW_PRESETS: [ShadowPreset; 5] = [
    // Shadows disabled (`null` in `src/settings.js`).
    ShadowPreset {
        id: "off",
        enabled: false,
        map_size: 0,
        radius: 1.0,
        terrain: false,
        objects: false,
    },
    // Low shadow tier (`512` map size, unit/vehicle casters only).
    ShadowPreset {
        id: "low",
        enabled: true,
        map_size: 512,
        radius: 0.75,
        terrain: false,
        objects: false,
    },
    // Medium shadow tier (`1024` map size, includes terrain shadows).
    ShadowPreset {
        id: "medium",
        enabled: true,
        map_size: 1024,
        radius: 1.0,
        terrain: true,
        objects: false,
    },
    // High shadow tier (`2048` map size, wider filter radius).
    ShadowPreset {
        id: "high",
        enabled: true,
        map_size: 2048,
        radius: 1.5,
        terrain: true,
        objects: false,
    },
    // Ultra shadow tier (`4096` map size, terrain + static object casters).
    ShadowPreset {
        id: "ultra",
        enabled: true,
        map_size: 4096,
        radius: 2.5,
        terrain: true,
        objects: true,
    },
];

/// Quality preset bundle (`PERFORMANCE_PRESETS` in `src/settings.js`).
///
/// Selecting a named preset in [`Settings::set`] cascades all 27 fields below
/// onto their corresponding keys in [`PRESET_KEYS`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PerformancePreset {
    /// Preset identifier (`"minimum"`, `"performance"`, `"balanced"`, or `"quality"`).
    pub id: &'static str,
    /// 3D viewport resolution multiplier (`renderScale`).
    pub render_scale: f64,
    /// Upscaling algorithm (`upscaler`: `"off"`, `"bilinear"`, `"spatial"`, `"temporal"`).
    pub upscaler: &'static str,
    /// Upscaler sharpening strength (`upscaleSharpness`: `0.0..1.0`).
    pub upscale_sharpness: f64,
    /// Antialiasing mode (`antialiasing`: `"off"`, `"fxaa-fast"`, `"fxaa"`, `"4x"`).
    pub antialiasing: &'static str,
    /// Shadow map quality tier (`shadows`: `"off"`, `"low"`, `"medium"`, `"high"`).
    pub shadows: &'static str,
    /// Texture anisotropic filtering level (`anisotropy`: `1`, `2`, `8`, `16`).
    pub anisotropy: i64,
    /// Normal-mapped surface relief (`bumpMapping`).
    pub bump_mapping: bool,
    /// 2004 map soft-focus blur pass (`softFocus`).
    pub soft_focus: bool,
    /// HDR emissive bloom (`bloom`).
    pub bloom: bool,
    /// High-speed vehicle radial blur (`vehicleMotionBlur`).
    pub vehicle_motion_blur: bool,
    /// Depth-feathered soft particles (`softParticles`).
    pub soft_particles: bool,
    /// Dynamic point lights on characters (`lampLighting`).
    pub lamp_lighting: bool,
    /// Water reflection quality (`waterQuality`: `"low"`, `"high"`, `"ultra"`).
    pub water_quality: &'static str,
    /// Tree foliage detail (`treeDetail`: `"original"` or `"enhanced"`).
    pub tree_detail: &'static str,
    /// Droideka shield shader (`droidekaEffects`: `"classic"` or `"enhanced"`).
    pub droideka_effects: &'static str,
    /// Droid/vehicle destruction debris (`destructionEffects`: `"classic"` or `"enhanced"`).
    pub destruction_effects: &'static str,
    /// Jetpack thruster plumes (`jetpackEffects`: `"classic"` or `"enhanced"`).
    pub jetpack_effects: &'static str,
    /// Water splashes and steam (`environmentEffects`: `"classic"` or `"enhanced"`).
    pub environment_effects: &'static str,
    /// Weather rain streaks (`rainEffects`: `"classic"` or `"enhanced"`).
    pub rain_effects: &'static str,
    /// Ocean and sky horizon shaders (`oceanEffects`: `"classic"` or `"enhanced"`).
    pub ocean_effects: &'static str,
    /// Blaster bolt rendering (`boltEffects`: `"classic"` or `"enhanced"`).
    pub bolt_effects: &'static str,
    /// Arc caster lightning (`arcCasterEffects`: `"classic"` or `"enhanced"`).
    pub arc_caster_effects: &'static str,
    /// Sniper beam trails (`sniperEffects`: `"classic"` or `"enhanced"`).
    pub sniper_effects: &'static str,
    /// Terrain sun-and-slope shading (`terrainDetail`: `"original"` or `"enhanced"`).
    pub terrain_detail: &'static str,
    /// Post-death camera framing (`deathCamera`: `"classic"` or `"enhanced"`).
    pub death_camera: &'static str,
    /// Near-scene full-detail view distance (`viewDistance`: `"original"`, `"far"`, `"max"`).
    pub view_distance: &'static str,
    /// Distant terrain triangle density (`farTerrain`: `"full"` or `"reduced"`).
    pub far_terrain: &'static str,
}

impl PerformancePreset {
    /// Returns the preset value for `key` if `key` is one of [`PRESET_KEYS`].
    pub fn value_for(&self, key: &str) -> Option<SettingValue> {
        Some(match key {
            "renderScale" => SettingValue::Number(self.render_scale),
            "upscaler" => SettingValue::Text(self.upscaler.to_string()),
            "upscaleSharpness" => SettingValue::Number(self.upscale_sharpness),
            "antialiasing" => SettingValue::Text(self.antialiasing.to_string()),
            "shadows" => SettingValue::Text(self.shadows.to_string()),
            "anisotropy" => SettingValue::Int(self.anisotropy),
            "bumpMapping" => SettingValue::Bool(self.bump_mapping),
            "softFocus" => SettingValue::Bool(self.soft_focus),
            "bloom" => SettingValue::Bool(self.bloom),
            "vehicleMotionBlur" => SettingValue::Bool(self.vehicle_motion_blur),
            "softParticles" => SettingValue::Bool(self.soft_particles),
            "lampLighting" => SettingValue::Bool(self.lamp_lighting),
            "waterQuality" => SettingValue::Text(self.water_quality.to_string()),
            "treeDetail" => SettingValue::Text(self.tree_detail.to_string()),
            "droidekaEffects" => SettingValue::Text(self.droideka_effects.to_string()),
            "destructionEffects" => SettingValue::Text(self.destruction_effects.to_string()),
            "jetpackEffects" => SettingValue::Text(self.jetpack_effects.to_string()),
            "environmentEffects" => SettingValue::Text(self.environment_effects.to_string()),
            "rainEffects" => SettingValue::Text(self.rain_effects.to_string()),
            "oceanEffects" => SettingValue::Text(self.ocean_effects.to_string()),
            "boltEffects" => SettingValue::Text(self.bolt_effects.to_string()),
            "arcCasterEffects" => SettingValue::Text(self.arc_caster_effects.to_string()),
            "sniperEffects" => SettingValue::Text(self.sniper_effects.to_string()),
            "terrainDetail" => SettingValue::Text(self.terrain_detail.to_string()),
            "deathCamera" => SettingValue::Text(self.death_camera.to_string()),
            "viewDistance" => SettingValue::Text(self.view_distance.to_string()),
            "farTerrain" => SettingValue::Text(self.far_terrain.to_string()),
            _ => return None,
        })
    }
}

/// All four named quality presets (`minimum`, `performance`, `balanced`, `quality`)
/// from `PERFORMANCE_PRESETS` in `src/settings.js`.
pub const PERFORMANCE_PRESETS: [PerformancePreset; 4] = [
    // 1. `minimum`: for pre-Ivy Bridge integrated GPUs (`0.5` render scale, no upscaler/AA/shadows,
    // classic effects, and reduced far terrain). Adopted automatically on old Intel GPUs.
    PerformancePreset {
        id: "minimum",
        render_scale: 0.5,
        upscaler: "off",
        upscale_sharpness: 0.0,
        antialiasing: "off",
        shadows: "off",
        anisotropy: 1,
        bump_mapping: false,
        soft_focus: false,
        bloom: false,
        vehicle_motion_blur: false,
        soft_particles: false,
        lamp_lighting: false,
        water_quality: "low",
        tree_detail: "original",
        droideka_effects: "classic",
        destruction_effects: "classic",
        jetpack_effects: "classic",
        environment_effects: "classic",
        rain_effects: "classic",
        ocean_effects: "classic",
        bolt_effects: "classic",
        arc_caster_effects: "classic",
        sniper_effects: "classic",
        terrain_detail: "original",
        death_camera: "classic",
        view_distance: "original",
        far_terrain: "reduced",
    },
    // 2. `performance`: for older PCs (`0.75` render scale, fast FXAA, low shadows, classic effects).
    PerformancePreset {
        id: "performance",
        render_scale: 0.75,
        upscaler: "spatial",
        upscale_sharpness: 0.35,
        antialiasing: "fxaa-fast",
        shadows: "low",
        anisotropy: 2,
        bump_mapping: false,
        soft_focus: false,
        bloom: false,
        vehicle_motion_blur: false,
        soft_particles: false,
        lamp_lighting: false,
        water_quality: "low",
        tree_detail: "original",
        droideka_effects: "classic",
        destruction_effects: "classic",
        jetpack_effects: "classic",
        environment_effects: "classic",
        rain_effects: "classic",
        ocean_effects: "classic",
        bolt_effects: "classic",
        arc_caster_effects: "classic",
        sniper_effects: "classic",
        terrain_detail: "original",
        death_camera: "classic",
        view_distance: "original",
        far_terrain: "reduced",
    },
    // 3. `balanced`: native `1.0` scale with temporal upscaler, FXAA, medium shadows, and enhanced effects.
    PerformancePreset {
        id: "balanced",
        render_scale: 1.0,
        upscaler: "temporal",
        upscale_sharpness: 0.35,
        antialiasing: "fxaa",
        shadows: "medium",
        anisotropy: 8,
        bump_mapping: true,
        soft_focus: true,
        bloom: true,
        vehicle_motion_blur: false,
        soft_particles: true,
        lamp_lighting: true,
        water_quality: "high",
        tree_detail: "enhanced",
        droideka_effects: "enhanced",
        destruction_effects: "enhanced",
        jetpack_effects: "enhanced",
        environment_effects: "enhanced",
        rain_effects: "enhanced",
        ocean_effects: "enhanced",
        bolt_effects: "enhanced",
        arc_caster_effects: "enhanced",
        sniper_effects: "enhanced",
        terrain_detail: "enhanced",
        death_camera: "enhanced",
        view_distance: "original",
        far_terrain: "full",
    },
    // 4. `quality`: full `1.0` scale with 4x MSAA, high shadows, 16x anisotropy, and all enhanced effects.
    PerformancePreset {
        id: "quality",
        render_scale: 1.0,
        upscaler: "temporal",
        upscale_sharpness: 0.4,
        antialiasing: "4x",
        shadows: "high",
        anisotropy: 16,
        bump_mapping: true,
        soft_focus: true,
        bloom: true,
        vehicle_motion_blur: true,
        soft_particles: true,
        lamp_lighting: true,
        water_quality: "high",
        tree_detail: "enhanced",
        droideka_effects: "enhanced",
        destruction_effects: "enhanced",
        jetpack_effects: "enhanced",
        environment_effects: "enhanced",
        rain_effects: "enhanced",
        ocean_effects: "enhanced",
        bolt_effects: "enhanced",
        arc_caster_effects: "enhanced",
        sniper_effects: "enhanced",
        terrain_detail: "enhanced",
        death_camera: "enhanced",
        view_distance: "original",
        far_terrain: "full",
    },
];

/// Looks up a [`PerformancePreset`] by id (`"minimum"`, `"performance"`, `"balanced"`, `"quality"`).
#[inline]
pub fn performance_preset(id: &str) -> Option<&'static PerformancePreset> {
    PERFORMANCE_PRESETS.iter().find(|p| p.id == id)
}

/// Keys governed by [`PERFORMANCE_PRESETS`] (`PRESET_KEYS` in `src/settings.js`).
///
/// Modifying any key in this list while `performancePreset` is set to a named
/// preset (`minimum`, `performance`, `balanced`, `quality`) flips `performancePreset`
/// back to `"custom"`.
pub const PRESET_KEYS: [&str; 27] = [
    // Resolution scaling, upscaling, antialiasing, shadows, and texture filtering.
    "renderScale",
    "upscaler",
    "upscaleSharpness",
    "antialiasing",
    "shadows",
    "anisotropy",
    // Surface normal mapping, post-process blur/bloom, soft particles, and character lights.
    "bumpMapping",
    "softFocus",
    "bloom",
    "vehicleMotionBlur",
    "softParticles",
    "lampLighting",
    // Water reflections, foliage, shields, destruction, jetpacks, and weather/sky shaders.
    "waterQuality",
    "treeDetail",
    "droidekaEffects",
    "destructionEffects",
    "jetpackEffects",
    "environmentEffects",
    "rainEffects",
    "oceanEffects",
    // Weapon traces, terrain shading, death camera, near view distance, and far terrain.
    "boltEffects",
    "arcCasterEffects",
    "sniperEffects",
    "terrainDetail",
    "deathCamera",
    "viewDistance",
    "farTerrain",
];

/// Static default value in [`DEFAULTS`] (`src/settings.js`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DefaultVal {
    /// Boolean default (`true` or `false`).
    Bool(bool),
    /// Integer default (such as `anisotropy: 16`).
    Int(i64),
    /// Floating-point default (such as `renderScale: 1.0` or `masterVolume: 0.8`).
    Number(f64),
    /// Static string literal default (such as `"custom"`, `"high"`, or `"PLAYER 1"`).
    Text(&'static str),
    /// Empty custom key-binding map (`{}` in `src/settings.js`).
    Bindings,
}

impl DefaultVal {
    /// Converts this static default into an owned [`SettingValue`].
    #[inline]
    pub fn to_setting_value(self) -> SettingValue {
        match self {
            Self::Bool(v) => SettingValue::Bool(v),
            Self::Int(v) => SettingValue::Int(v),
            Self::Number(v) => SettingValue::Number(v),
            Self::Text(v) => SettingValue::Text(v.to_string()),
            Self::Bindings => SettingValue::Bindings(BTreeMap::new()),
        }
    }
}

/// Entry in [`DEFAULTS`] pairing a setting key with its default value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SettingDefault {
    /// Setting identifier in `src/settings.js` `DEFAULTS`.
    pub key: &'static str,
    /// Typed compile-time default value.
    pub value: DefaultVal,
}

impl SettingDefault {
    /// Constructs a boolean setting default.
    const fn b(key: &'static str, v: bool) -> Self {
        Self {
            key,
            value: DefaultVal::Bool(v),
        }
    }
    /// Constructs an integer setting default.
    const fn i(key: &'static str, v: i64) -> Self {
        Self {
            key,
            value: DefaultVal::Int(v),
        }
    }
    /// Constructs a floating-point setting default.
    const fn n(key: &'static str, v: f64) -> Self {
        Self {
            key,
            value: DefaultVal::Number(v),
        }
    }
    /// Constructs a string setting default.
    const fn s(key: &'static str, v: &'static str) -> Self {
        Self {
            key,
            value: DefaultVal::Text(v),
        }
    }
    /// Constructs an empty bindings-map setting default (`{}`).
    const fn map(key: &'static str) -> Self {
        Self {
            key,
            value: DefaultVal::Bindings,
        }
    }
}

/// All 59 keys and defaults from `DEFAULTS` in `src/settings.js`, in exact source order.
pub const DEFAULTS: [SettingDefault; 59] = [
    // Simulation rate (`"60"` Hz fixed tick) and video preset / pacing / upscaling.
    // 1. Fixed 60 Hz gameplay simulation tick rate.
    SettingDefault::s("physicsRate", "60"),
    // 2. Active rendering preset (`"custom"`, `"minimum"`, `"performance"`, `"balanced"`, `"quality"`).
    SettingDefault::s("performancePreset", "custom"),
    // 3. Frame pacing limit (`"0"` follows display refresh rate).
    SettingDefault::s("frameLimit", "0"),
    // 4. Adaptive resolution target frame rate (`"0"` disables dynamic scaling).
    SettingDefault::s("dynamicResolution", "0"),
    // 5. 3D viewport render resolution multiplier (`0.25..2.0`).
    SettingDefault::n("renderScale", 1.0),
    // 6. Sub-native 3D frame upscaler (`"off"`, `"bilinear"`, `"spatial"`, `"temporal"`).
    SettingDefault::s("upscaler", "temporal"),
    // 7. Contrast-adaptive sharpening strength (`0.0..1.0`).
    SettingDefault::n("upscaleSharpness", 0.4),
    // Texture compression, antialiasing, shadow maps, and anisotropic filtering.
    // 8. KTX2/Basis GPU texture compression on map load.
    SettingDefault::b("textureCompression", true),
    // 9. Edge antialiasing mode (`"off"`, `"fxaa-fast"`, `"fxaa"`, `"2x"`, `"4x"`, `"8x"`).
    SettingDefault::s("antialiasing", "4x"),
    // 10. Cascaded shadow map quality tier (`"off"`, `"low"`, `"medium"`, `"high"`, `"ultra"`).
    SettingDefault::s("shadows", "high"),
    // 11. Texture filtering anisotropy level (`1`, `2`, `4`, `8`, `16`).
    SettingDefault::i("anisotropy", 16),
    // Surface shading, post-processing blur/bloom, soft particles, and character lamp lights.
    // 12. Normal-mapped surface relief on models and terrain.
    SettingDefault::b("bumpMapping", true),
    // 13. Original 2004 map soft-focus post-process blur.
    SettingDefault::b("softFocus", true),
    // 14. Emissive HDR bloom and glow post-processing.
    SettingDefault::b("bloom", true),
    // 15. High-speed radial motion blur in vehicles and starfighters.
    SettingDefault::b("vehicleMotionBlur", true),
    // 16. Depth-feathered soft particle billboards.
    SettingDefault::b("softParticles", true),
    // 17. Dynamic point-light illumination on characters.
    SettingDefault::b("lampLighting", true),
    // Water reflections, foliage, shields, destruction, weather, and weapon/camera effects.
    // 18. Water reflection tier (`"low"`, `"high"` screen-space, `"ultra"` planar).
    SettingDefault::s("waterQuality", "high"),
    // 19. Tree and foliage detail (`"original"` vs. `"enhanced"`).
    SettingDefault::s("treeDetail", "enhanced"),
    // 20. Droideka shield refraction shader (`"classic"` vs. `"enhanced"`).
    SettingDefault::s("droidekaEffects", "enhanced"),
    // 21. Droid/vehicle destruction sparks and hot debris (`"classic"` vs. `"enhanced"`).
    SettingDefault::s("destructionEffects", "enhanced"),
    // 22. Jetpack thruster plumes and lighting (`"classic"` vs. `"enhanced"`).
    SettingDefault::s("jetpackEffects", "enhanced"),
    // 23. Water splashes, mist, and steam emitters (`"classic"` vs. `"enhanced"`).
    SettingDefault::s("environmentEffects", "enhanced"),
    // 24. Weather rain streaks and surface ripples (`"classic"` vs. `"enhanced"`).
    SettingDefault::s("rainEffects", "enhanced"),
    // 25. Ocean wave and sky horizon shaders (`"classic"` vs. `"enhanced"`).
    SettingDefault::s("oceanEffects", "enhanced"),
    // 26. Blaster bolt rendering (`"classic"` strips vs. `"enhanced"` lit plasma).
    SettingDefault::s("boltEffects", "enhanced"),
    // 27. Arc caster lightning (`"classic"` ribbons vs. `"enhanced"` chaining arcs).
    SettingDefault::s("arcCasterEffects", "enhanced"),
    // 28. Sniper rifle beam trails and impact sparks (`"classic"` vs. `"enhanced"`).
    SettingDefault::s("sniperEffects", "enhanced"),
    // 29. Terrain sun-and-slope shading (`"original"` vs. `"enhanced"`).
    SettingDefault::s("terrainDetail", "enhanced"),
    // 30. Post-death camera (`"classic"` framing vs. `"enhanced"` kill-shot replay).
    SettingDefault::s("deathCamera", "enhanced"),
    // Starfighter steering tuning, cockpit camera preference, and custom key binding maps.
    // 31. Starfighter steering sensitivity multiplier (`0.2..3.0`).
    SettingDefault::n("flightSensitivity", 1.0),
    // 32. Starfighter vertical pitch axis inversion.
    SettingDefault::b("flightInvertY", false),
    // 33. Starfighter steering response curve exponent (`1.0..3.0`).
    SettingDefault::n("flightResponseCurve", 1.35),
    // 34. Starfighter cockpit first-person camera preference.
    SettingDefault::b("flightFirstPerson", false),
    // 35. Player-overridden infantry and vehicle key bindings (`{}`).
    SettingDefault::map("keyBindings"),
    // 36. Player-overridden starfighter key bindings (`{}`).
    SettingDefault::map("flightBindings"),
    // Near-scene draw distance, distant terrain LOD, mouse look tuning, and HUD stats line.
    // 37. Near-scene full-detail view distance (`"original"`, `"far"`, `"max"`).
    SettingDefault::s("viewDistance", "original"),
    // 38. Distant terrain mesh density (`"full"` or `"reduced"`).
    SettingDefault::s("farTerrain", "full"),
    // 39. Ground/vehicle mouse sensitivity slider (`0.5..10.0`, neutral `5.0`).
    SettingDefault::n("mouseSensitivity", 5.0),
    // 40. Ground/vehicle vertical mouse look inversion.
    SettingDefault::b("invertMouse", false),
    // 41. Top-right FPS and frame-timing stats line visibility.
    SettingDefault::b("showStats", true),
    // Profile name (`PROFILE MANAGEMENT`) and front-end visual theme (`"bf1"`, `"bf2"`, `"eternal"`).
    // 42. Profile display name (`PROFILE MANAGEMENT`).
    SettingDefault::s("playerName", "PLAYER 1"),
    // 43. Front-end menu color theme (`"bf1"`, `"bf2"`, or `"eternal"`).
    SettingDefault::s("menuTheme", "eternal"),
    // Battle rules (`friendlyFire`), Instant Action bots (`aiMode`, `aiDifficulty`, `aiHeroes`), and `battleSize`.
    // 44. Friendly fire damage rule (`WORLD_SETTINGS`).
    SettingDefault::b("friendlyFire", true),
    // 45. Instant Action bot mode (`"off"`, `"classic"`, `"enhanced"`).
    SettingDefault::s("aiMode", "off"),
    // 46. Instant Action bot difficulty (`"easy"`, `"medium"`, `"hard"`).
    SettingDefault::s("aiDifficulty", "medium"),
    // 47. Instant Action faction hero NPC spawning.
    SettingDefault::b("aiHeroes", false),
    // 48. Single-player unit cap scale (`"full"`, `"large"`, `"small"`).
    SettingDefault::s("battleSize", "full"),
    // Audio engine stage, speaker layout, dynamic range, mono fold, clarity, and 6 volume buses.
    // 49. Sound engine stage (`"enhanced"` 3D acoustics vs. `"original"` stereo mix).
    SettingDefault::s("audioQuality", "enhanced"),
    // 50. Speaker/headphone channel layout (`"auto"`, `"headphones"`, `"stereo"`, `"5.1"`, `"7.1"`, `"7.1.4"`).
    SettingDefault::s("audioOutput", "auto"),
    // 51. Audio dynamic range profile (`"home"`, `"normal"`, `"night"`).
    SettingDefault::s("audioDynamics", "normal"),
    // 52. Mono channel downmix toggle.
    SettingDefault::b("audioMono", false),
    // 53. High-frequency clarity stage in enhanced audio mode.
    SettingDefault::b("audioClarity", true),
    // 54. Master audio bus gain (`0.0..1.0`, default `0.8`).
    SettingDefault::n("masterVolume", 0.8),
    // 55. Sound effects bus gain (`0.0..1.0`).
    SettingDefault::n("effectsVolume", 1.0),
    // 56. Music soundtrack bus gain (`0.0..1.0`).
    SettingDefault::n("musicVolume", 1.0),
    // 57. Voice and announcer speech bus gain (`0.0..1.0`).
    SettingDefault::n("voiceVolume", 1.0),
    // 58. Environmental ambience bus gain (`0.0..1.0`).
    SettingDefault::n("ambienceVolume", 1.0),
    // 59. Menu and HUD interface sound bus gain (`0.0..1.0`).
    SettingDefault::n("interfaceVolume", 1.0),
];

/// Looks up the default value for `key` in [`DEFAULTS`].
#[inline]
pub fn default_for(key: &str) -> Option<DefaultVal> {
    DEFAULTS.iter().find(|d| d.key == key).map(|d| d.value)
}

/// Dynamic setting value stored in [`Settings`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SettingValue {
    /// Boolean toggle setting (`true` / `false`).
    Bool(bool),
    /// Integer choice setting (`anisotropy`).
    Int(i64),
    /// Floating-point slider setting (`renderScale`, volumes, sensitivities).
    Number(f64),
    /// String choice or profile/theme identifier.
    Text(String),
    /// Action-to-keycodes override map (`keyBindings` and `flightBindings`).
    Bindings(BTreeMap<String, Vec<String>>),
}

impl SettingValue {
    /// Returns `self` as a `bool` if it is a boolean.
    #[inline]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Returns `self` as an `f64` if it is numeric (`Int` or `Number`).
    #[inline]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Int(i) => Some(*i as f64),
            Self::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// Returns `self` as an `i64` if it is an integer or whole number.
    #[inline]
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            Self::Number(n) => Some(n.round() as i64),
            _ => None,
        }
    }

    /// Returns `self` as a `&str` if it is text.
    #[inline]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Returns `self` as a bindings map if it is `Bindings`.
    #[inline]
    pub fn as_bindings(&self) -> Option<&BTreeMap<String, Vec<String>>> {
        match self {
            Self::Bindings(m) => Some(m),
            _ => None,
        }
    }

    /// Converts this value to a [`serde_json::Value`].
    ///
    /// Whole floating-point numbers (such as `1.0` or `5.0`) are serialized as JSON
    /// integers so desktop snapshots match JavaScript `JSON.stringify` output.
    pub fn to_json(&self) -> Value {
        match self {
            Self::Bool(b) => Value::Bool(*b),
            Self::Int(i) => Value::from(*i),
            Self::Number(n) => {
                // Emit integer JSON tokens for exact whole numbers (`1.0` -> `1`).
                if n.fract() == 0.0 && *n >= (i64::MIN as f64) && *n <= (i64::MAX as f64) {
                    Value::from(*n as i64)
                } else {
                    Value::from(*n)
                }
            }
            Self::Text(s) => Value::String(s.clone()),
            // Serialize custom bindings as `{ "<action>": ["<code1>", ...] }`.
            Self::Bindings(map) => {
                let mut out = Map::new();
                for (k, v) in map {
                    out.insert(
                        k.clone(),
                        Value::Array(v.iter().cloned().map(Value::String).collect()),
                    );
                }
                Value::Object(out)
            }
        }
    }

    /// Parses a [`serde_json::Value`] into a [`SettingValue`] matching the expected type of `default`.
    ///
    /// Returns `None` if `val` does not match the variant of `default`, keeping
    /// corrupted or mismatched storage values from overwriting valid defaults.
    pub fn from_json_typed(val: &Value, default: DefaultVal) -> Option<Self> {
        match default {
            // Boolean settings accept only JSON booleans.
            DefaultVal::Bool(_) => val.as_bool().map(Self::Bool),
            // Integer settings accept JSON integers or rounded floats.
            DefaultVal::Int(_) => val
                .as_i64()
                .or_else(|| val.as_f64().map(|n| n.round() as i64))
                .map(Self::Int),
            // Floating-point settings accept any JSON number.
            DefaultVal::Number(_) => val.as_f64().map(Self::Number),
            // Text settings accept only JSON strings.
            DefaultVal::Text(_) => val.as_str().map(|s| Self::Text(s.to_string())),
            // Binding maps accept JSON objects mapping action names to string arrays.
            DefaultVal::Bindings => {
                let obj = val.as_object()?;
                let mut map = BTreeMap::new();
                for (k, v) in obj {
                    if let Some(arr) = v.as_array() {
                        let codes: Vec<String> = arr
                            .iter()
                            .filter_map(|item| item.as_str().map(str::to_string))
                            .collect();
                        map.insert(k.clone(), codes);
                    }
                }
                Some(Self::Bindings(map))
            }
        }
    }

    /// Compares two [`SettingValue`]s for logical equality (treating `Int(1)` and `Number(1.0)` as equal).
    pub fn same_value(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Number(a), Self::Number(b)) => (a - b).abs() <= 1e-9,
            (Self::Int(a), Self::Number(b)) | (Self::Number(b), Self::Int(a)) => {
                ((*a as f64) - b).abs() <= 1e-9
            }
            (Self::Text(a), Self::Text(b)) => a == b,
            (Self::Bindings(a), Self::Bindings(b)) => a == b,
            _ => false,
        }
    }
}

// Conversions from primitive Rust types into `SettingValue` for ergonomic `Settings::set` calls.
impl From<bool> for SettingValue {
    /// Wraps a `bool` into [`SettingValue::Bool`].
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

impl From<i64> for SettingValue {
    /// Wraps an `i64` into [`SettingValue::Int`].
    fn from(v: i64) -> Self {
        Self::Int(v)
    }
}

impl From<f64> for SettingValue {
    /// Wraps an `f64` into [`SettingValue::Number`].
    fn from(v: f64) -> Self {
        Self::Number(v)
    }
}

impl From<&str> for SettingValue {
    /// Clones a `&str` slice into [`SettingValue::Text`].
    fn from(v: &str) -> Self {
        Self::Text(v.to_string())
    }
}

impl From<String> for SettingValue {
    /// Wraps an owned `String` into [`SettingValue::Text`].
    fn from(v: String) -> Self {
        Self::Text(v)
    }
}

impl From<BTreeMap<String, Vec<String>>> for SettingValue {
    /// Wraps an action-to-keycodes `BTreeMap` into [`SettingValue::Bindings`].
    fn from(v: BTreeMap<String, Vec<String>>) -> Self {
        Self::Bindings(v)
    }
}

/// GPU classification result (`oldGpu` / `gpuState` in `src/gpu-info.js`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GpuInfo {
    /// Trimmed WebGL/graphics renderer string.
    pub renderer: String,
    /// Trimmed GPU vendor string.
    pub vendor: String,
    /// True for pre-Ivy Bridge Intel GPUs (`oldGpu` in `src/gpu-info.js`).
    pub is_old_intel: bool,
    /// True when the GPU requires the `minimum` preset on first launch.
    pub is_blocklisted: bool,
    /// Human-readable classification reason when `is_old_intel` is true.
    pub reason: Option<String>,
}

/// Classifies a GPU `renderer` and `vendor` string against `src/gpu-policy.json`
/// (`getGpuInfo` in `src/gpu-info.js`).
///
/// Old pre-Ivy Bridge Intel GPUs (`Intel GMA`, `Intel 945/965`, `G33/G41/G45`,
/// `Mobile Intel 4 Series`, `Intel HD Graphics 2000/3000`, `Intel Ironlake`)
/// set `is_old_intel = true` and `is_blocklisted = true`.
pub fn classify_gpu(renderer: &str, vendor: &str) -> GpuInfo {
    let r = renderer.trim();
    let v = vendor.trim();
    if r.is_empty() && v.is_empty() {
        return GpuInfo::default();
    }
    let rl = r.to_ascii_lowercase();
    let vl = v.to_ascii_lowercase();
    let mut is_old_intel = false;
    let mut reason = None;

    // 1. Intel GMA / 945 / 965 / 3100 / X3100 / G31 / G33 / G35 / G41 / G43 / G45 / Q35 / Q45
    if rl.contains("intel")
        && (rl.contains("gma")
            || rl.contains("945")
            || rl.contains("965")
            || rl.contains("3100")
            || rl.contains("x3100")
            || rl.contains("g31")
            || rl.contains("g33")
            || rl.contains("g35")
            || rl.contains("g41")
            || rl.contains("g43")
            || rl.contains("g45")
            || rl.contains("q35")
            || rl.contains("q45")
            || rl.contains("4 series")
            || rl.contains("ironlake"))
    {
        is_old_intel = true;
        reason = Some("Legacy Intel GMA / 4-Series / Ironlake GPU".to_string());
    }

    // 2. Sandy Bridge (`Intel(R) HD Graphics 2000` / `3000` or bare `Intel(R) HD Graphics`)
    if !is_old_intel && (rl.contains("intel") || vl.contains("intel")) && rl.contains("hd graphics")
    {
        let after_hd = rl
            .split("hd graphics")
            .nth(1)
            .unwrap_or("")
            .trim_start_matches(|c: char| !c.is_ascii_alphanumeric());
        if after_hd.starts_with("2000") || after_hd.starts_with("3000") {
            is_old_intel = true;
            reason = Some("Sandy Bridge (2011) GPU".to_string());
        } else if after_hd.is_empty()
            || (!after_hd.starts_with(char::is_numeric)
                && !after_hd.starts_with('p')
                && !rl.contains("iris")
                && !rl.contains("uhd"))
        {
            is_old_intel = true;
            reason = Some("Unnumbered Intel HD Graphics (2010-2011)".to_string());
        }
    }

    GpuInfo {
        renderer: r.to_string(),
        vendor: v.to_string(),
        is_old_intel,
        is_blocklisted: is_old_intel,
        reason,
    }
}

/// CPU hardware probe result (`getCpuInfo` in `src/cpu-info.js`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CpuInfo {
    /// Logical processor count (`navigator.hardwareConcurrency`, default `8`).
    pub threads: u32,
    /// Approximate RAM in GiB (`navigator.deviceMemory`, default `8.0`).
    pub memory: f64,
    /// True when `threads <= 4 && memory <= 4.0` (`lowEndCpu` in `src/cpu-info.js`).
    pub low_end_cpu: bool,
}

impl Default for CpuInfo {
    fn default() -> Self {
        Self::new(8, 8.0)
    }
}

impl CpuInfo {
    /// Evaluates `low_end_cpu` (`threads <= 4 && memory <= 4.0`) from `threads` and `memory`
    /// (`getCpuInfo` in `src/cpu-info.js`).
    #[inline]
    pub fn new(threads: u32, memory: f64) -> Self {
        let t = if threads > 0 { threads } else { 8 };
        let m = if memory.is_finite() && memory > 0.0 {
            memory
        } else {
            8.0
        };
        Self {
            threads: t,
            memory: m,
            low_end_cpu: t <= 4 && m <= 4.0,
        }
    }
}

/// Desktop player settings snapshot (`{ complete, values }` in the desktop host
/// and `src/settings.js`).
///
/// When `complete` is true, `values` holds the authoritative player settings across
/// all keys; when `complete` is false (written by a shadowed update process),
/// `values` contains only the keys explicitly modified during that session.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DesktopSettingsSnapshot {
    /// Whether `values` represents the complete settings record (`record.complete`).
    pub complete: bool,
    /// Setting key-value map serialized to `player-settings.json` (`record.values`).
    pub values: Map<String, Value>,
}

/// Persistent and session-overridden settings container (`Settings` in `src/settings.js`).
#[derive(Clone, Debug)]
pub struct Settings {
    /// Current effective saved values (`this.values` in `src/settings.js`).
    pub values: BTreeMap<String, SettingValue>,
    /// Transient session overrides (`this.overrides` in `src/settings.js`, e.g. URL query params).
    pub overrides: BTreeMap<String, SettingValue>,
    /// Keys that already had a saved value when loaded (`this.saved` in `src/settings.js`).
    pub saved: BTreeMap<String, bool>,
    /// Keys modified during this session via [`Self::set`] or [`Self::reset`] (`this.changed` in `src/settings.js`).
    pub changed: BTreeMap<String, bool>,
    /// True if no setting was previously saved (`this.fresh` in `src/settings.js`).
    pub fresh: bool,
    /// True if the backing desktop record is complete (`this.complete` in `src/settings.js`).
    pub complete: bool,
    /// Monotonic counter incremented whenever settings are mutated, allowing zero-allocation polling.
    pub revision: u64,
    /// Optional backing file path for automatic persistence.
    file_path: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self::new()
    }
}

impl Settings {
    /// Creates a fresh [`Settings`] initialized to [`DEFAULTS`] with no saved entries.
    pub fn new() -> Self {
        let mut values = BTreeMap::new();
        for entry in DEFAULTS {
            values.insert(entry.key.to_string(), entry.value.to_setting_value());
        }
        Self {
            values,
            overrides: BTreeMap::new(),
            saved: BTreeMap::new(),
            changed: BTreeMap::new(),
            fresh: true,
            complete: true,
            revision: 0,
            file_path: None,
        }
    }

    /// Loads [`Settings`] from a `localStorage`-style string map (`bfe_setting_<key>` -> JSON)
    /// and an optional desktop `{ complete, values }` snapshot, matching `Settings.load()` in
    /// `src/settings.js`.
    pub fn from_storage_and_desktop(
        storage: &BTreeMap<String, String>,
        desktop: Option<&DesktopSettingsSnapshot>,
    ) -> Self {
        let mut settings = Self::new();
        // Record whether the desktop snapshot is complete or a partial shadowed diff.
        if let Some(d) = desktop {
            settings.complete = d.complete;
        }
        for entry in DEFAULTS {
            let key = entry.key;
            let from_desktop = desktop.and_then(|d| d.values.get(key));
            let storage_key = format!("bfe_setting_{key}");
            let raw_storage = storage.get(&storage_key);

            // Mark the session as non-fresh whenever any recognized setting was previously saved.
            if from_desktop.is_some() || raw_storage.is_some() {
                settings.saved.insert(key.to_string(), true);
                settings.fresh = false;
            }

            // Desktop snapshot values take priority over browser `localStorage` entries.
            if let Some(dv) = from_desktop {
                if let Some(parsed) = SettingValue::from_json_typed(dv, entry.value) {
                    settings.values.insert(key.to_string(), parsed);
                    continue;
                }
            }
            // Fall back to `localStorage` (`bfe_setting_<key>`) when not present in `desktop`.
            if let Some(raw) = raw_storage {
                if let Ok(jv) = serde_json::from_str::<Value>(raw) {
                    if let Some(parsed) = SettingValue::from_json_typed(&jv, entry.value) {
                        settings.values.insert(key.to_string(), parsed);
                    }
                }
            }
        }
        // `physicsRate` is fixed at 60 Hz (`src/settings.js`).
        settings
            .values
            .insert("physicsRate".to_string(), SettingValue::from("60"));
        settings
    }

    /// Loads [`Settings`] from a JSON file on disk (either `{ "complete": ..., "values": { ... } }`
    /// or a flat `{ "<key>": <value> }` object).
    pub fn load_from_file(path: impl AsRef<Path>) -> Self {
        let path_buf = path.as_ref().to_path_buf();
        let Ok(raw) = fs::read_to_string(&path_buf) else {
            let mut s = Self::new();
            s.file_path = Some(path_buf);
            return s;
        };
        let mut s = Self::from_json_str(&raw);
        s.file_path = Some(path_buf);
        s
    }

    /// Parses [`Settings`] from a JSON string (supporting both `{ complete, values }` and flat objects).
    pub fn from_json_str(raw: &str) -> Self {
        let Ok(val) = serde_json::from_str::<Value>(raw) else {
            return Self::new();
        };
        let Some(obj) = val.as_object() else {
            return Self::new();
        };
        if let Some(values_obj) = obj.get("values").and_then(Value::as_object) {
            let complete = obj.get("complete").and_then(Value::as_bool).unwrap_or(true);
            let snapshot = DesktopSettingsSnapshot {
                complete,
                values: values_obj.clone(),
            };
            Self::from_storage_and_desktop(&BTreeMap::new(), Some(&snapshot))
        } else {
            let snapshot = DesktopSettingsSnapshot {
                complete: true,
                values: obj.clone(),
            };
            Self::from_storage_and_desktop(&BTreeMap::new(), Some(&snapshot))
        }
    }

    /// Serializes [`Settings`] as a desktop snapshot (`{ complete, values }`), matching
    /// `Settings.persist()` in `src/settings.js` and the desktop host.
    pub fn to_desktop_snapshot(&self) -> DesktopSettingsSnapshot {
        let mut values = Map::new();
        // Complete records write all 59 keys in `DEFAULTS` order.
        if self.complete {
            for entry in DEFAULTS {
                if let Some(v) = self.values.get(entry.key) {
                    values.insert(entry.key.to_string(), v.to_json());
                }
            }
        } else {
            // Incomplete (shadowed) sessions write only the keys modified during this run.
            for key in self.changed.keys() {
                if let Some(v) = self.values.get(key) {
                    values.insert(key.clone(), v.to_json());
                }
            }
        }
        DesktopSettingsSnapshot {
            complete: self.complete,
            values,
        }
    }

    /// Loads [`Settings`] from a JSON file on disk, returning an error naming the file or
    /// malformed JSON field when reading or parsing fails.
    pub fn try_load_from_file(path: impl AsRef<Path>) -> Result<Self, String> {
        let path_buf = path.as_ref().to_path_buf();
        let raw = fs::read_to_string(&path_buf)
            .map_err(|e| format!("{}: {e}", path_buf.display()))?;
        let mut s = Self::try_from_json_str(&raw, &path_buf.display().to_string())?;
        s.file_path = Some(path_buf);
        Ok(s)
    }

    /// Parses [`Settings`] from a JSON string, returning an error naming `source` and any
    /// invalid top-level or `values` field.
    pub fn try_from_json_str(raw: &str, source: &str) -> Result<Self, String> {
        let val = serde_json::from_str::<Value>(raw)
            .map_err(|e| format!("{source}: invalid settings JSON: {e}"))?;
        let obj = val
            .as_object()
            .ok_or_else(|| format!("{source}: settings root must be a JSON object"))?;
        if let Some(values_val) = obj.get("values") {
            let values_obj = values_val
                .as_object()
                .ok_or_else(|| format!("{source}: field `values` must be a JSON object"))?;
            let complete = obj.get("complete").and_then(Value::as_bool).unwrap_or(true);
            let snapshot = DesktopSettingsSnapshot {
                complete,
                values: values_obj.clone(),
            };
            Ok(Self::from_storage_and_desktop(&BTreeMap::new(), Some(&snapshot)))
        } else {
            let snapshot = DesktopSettingsSnapshot {
                complete: true,
                values: obj.clone(),
            };
            Ok(Self::from_storage_and_desktop(&BTreeMap::new(), Some(&snapshot)))
        }
    }

    /// Saves [`Settings`] to `path` as a JSON `{ complete, values }` record.
    pub fn save_to_file(&self, path: impl AsRef<Path>) -> Result<(), String> {
        let path_ref = path.as_ref();
        let snapshot = self.to_desktop_snapshot();
        let json = serde_json::to_string_pretty(&snapshot)
            .map_err(|e| format!("{}: {e}", path_ref.display()))?;
        if let Some(parent) = path_ref.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("{}: {e}", parent.display()))?;
            }
        }
        fs::write(path_ref, json).map_err(|e| format!("{}: {e}", path_ref.display()))
    }

    fn persist_if_backed(&self) {
        if let Some(path) = &self.file_path {
            let _ = self.save_to_file(path);
        }
    }

    /// Returns the effective value of `key` (checking `overrides` first, then `values`, then `DEFAULTS`).
    /// Mirrors `Settings.get(key)` in `src/settings.js`.
    pub fn get(&self, key: &str) -> SettingValue {
        if let Some(v) = self.overrides.get(key) {
            return v.clone();
        }
        if let Some(v) = self.values.get(key) {
            return v.clone();
        }
        default_for(key)
            .map(DefaultVal::to_setting_value)
            .unwrap_or_else(|| SettingValue::Text(String::new()))
    }

    /// Returns the saved value of `key`, ignoring transient `overrides`.
    /// Mirrors `Settings.getSaved(key)` in `src/settings.js`.
    pub fn get_saved(&self, key: &str) -> SettingValue {
        if let Some(v) = self.values.get(key) {
            return v.clone();
        }
        default_for(key)
            .map(DefaultVal::to_setting_value)
            .unwrap_or_else(|| SettingValue::Text(String::new()))
    }

    /// Convenience getter for boolean settings.
    #[inline]
    pub fn get_bool(&self, key: &str) -> bool {
        self.get(key).as_bool().unwrap_or(false)
    }

    /// Convenience getter for numeric (`f64`) settings.
    #[inline]
    pub fn get_f64(&self, key: &str) -> f64 {
        self.get(key).as_f64().unwrap_or(0.0)
    }

    /// Convenience getter for integer (`i64`) settings.
    #[inline]
    pub fn get_i64(&self, key: &str) -> i64 {
        self.get(key).as_i64().unwrap_or(0)
    }

    /// Convenience getter for string settings.
    #[inline]
    pub fn get_string(&self, key: &str) -> String {
        self.get(key).as_str().unwrap_or("").to_string()
    }

    /// Borrows a `Bindings` map (`keyBindings` or `flightBindings`) without cloning.
    #[inline]
    pub fn get_bindings_ref(&self, key: &str) -> Option<&BTreeMap<String, Vec<String>>> {
        if let Some(v) = self.overrides.get(key) {
            return v.as_bindings();
        }
        self.values.get(key).and_then(SettingValue::as_bindings)
    }

    /// Sets `key` to `value` (clearing any session override on `key` and cascading
    /// `performancePreset` changes across [`PRESET_KEYS`]), matching `Settings.set(key, value)`
    /// in `src/settings.js`.
    pub fn set(&mut self, key: &str, value: impl Into<SettingValue>) {
        if key == "physicsRate" {
            return;
        }
        let val = value.into();
        self.revision = self.revision.wrapping_add(1);
        self.overrides.remove(key);
        self.values.insert(key.to_string(), val.clone());
        self.saved.insert(key.to_string(), true);
        self.changed.insert(key.to_string(), true);
        self.fresh = false;

        // Choosing a named `performancePreset` cascades all 27 `PRESET_KEYS` to that preset's values.
        if key == "performancePreset" {
            if let Some(preset_id) = val.as_str() {
                if let Some(preset) = performance_preset(preset_id) {
                    for &pk in &PRESET_KEYS {
                        if let Some(pv) = preset.value_for(pk) {
                            self.overrides.remove(pk);
                            self.values.insert(pk.to_string(), pv);
                            self.saved.insert(pk.to_string(), true);
                            self.changed.insert(pk.to_string(), true);
                        }
                    }
                }
            }
        } else if PRESET_KEYS.contains(&key) {
            // Changing any individual `PRESET_KEYS` entry away from the active preset's value
            // flips `performancePreset` back to `"custom"` (`src/settings.js`).
            let current_preset = self
                .values
                .get("performancePreset")
                .and_then(SettingValue::as_str)
                .unwrap_or("custom");
            if let Some(preset) = performance_preset(current_preset) {
                if let Some(expected) = preset.value_for(key) {
                    if !expected.same_value(&val) {
                        self.values.insert(
                            "performancePreset".to_string(),
                            SettingValue::from("custom"),
                        );
                        self.saved.insert("performancePreset".to_string(), true);
                        self.changed.insert("performancePreset".to_string(), true);
                    }
                }
            }
        }

        self.persist_if_backed();
    }

    /// Sets a session-only override for `key` without modifying persisted values.
    /// Mirrors `Settings.override(key, value)` in `src/settings.js`.
    pub fn override_value(&mut self, key: &str, value: impl Into<SettingValue>) {
        if key == "physicsRate" {
            return;
        }
        let val = value.into();
        self.revision = self.revision.wrapping_add(1);
        self.overrides.insert(key.to_string(), val.clone());
        if key == "performancePreset" {
            if let Some(preset_id) = val.as_str() {
                if let Some(preset) = performance_preset(preset_id) {
                    for &pk in &PRESET_KEYS {
                        if let Some(pv) = preset.value_for(pk) {
                            self.overrides.insert(pk.to_string(), pv);
                        }
                    }
                }
            }
        }
    }

    /// Clears all transient session overrides (`Settings.clearOverrides()` in `src/settings.js`).
    pub fn clear_overrides(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.overrides.clear();
    }

    /// Resets every setting to [`DEFAULTS`] (`Settings.reset()` in `src/settings.js`).
    ///
    /// Clears all session overrides, marks all 59 keys as saved and changed, marks
    /// the record as `complete = true`, and flushes to disk if file-backed.
    pub fn reset(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.overrides.clear();
        for entry in DEFAULTS {
            self.values
                .insert(entry.key.to_string(), entry.value.to_setting_value());
            self.saved.insert(entry.key.to_string(), true);
            self.changed.insert(entry.key.to_string(), true);
        }
        self.complete = true;
        self.persist_if_backed();
    }

    /// Resets only the specified `keys` back to their [`DEFAULTS`] values
    /// (used by [`crate::options::reset_page`] for per-page `RESET TO DEFAULTS`).
    pub fn reset_keys<I, S>(&mut self, keys: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for k in keys {
            let key = k.as_ref();
            // Ignore any unrecognized setting key not present in `DEFAULTS`.
            let Some(def) = default_for(key) else {
                continue;
            };
            self.overrides.remove(key);
            self.values.insert(key.to_string(), def.to_setting_value());
            self.saved.insert(key.to_string(), true);
            self.changed.insert(key.to_string(), true);
        }
        self.revision = self.revision.wrapping_add(1);
        self.persist_if_backed();
    }

    /// Adopts hardware-appropriate defaults on first run (`Settings.adoptHardwareDefaults(gpu, cpu)`
    /// in `src/settings.js`):
    /// - If `fresh && gpu.is_old_intel`, sets `performancePreset` to `"minimum"` (cascading all
    ///   27 [`PRESET_KEYS`] to their minimum preset values).
    /// - If `!saved["audioQuality"] && cpu.low_end_cpu`, sets `audioQuality` to `"original"`.
    pub fn adopt_hardware_defaults(&mut self, gpu: Option<&GpuInfo>, cpu: Option<&CpuInfo>) {
        // On a completely fresh profile with a pre-Ivy Bridge Intel GPU, adopt `"minimum"`.
        if self.fresh {
            if let Some(g) = gpu {
                if g.is_old_intel {
                    self.set("performancePreset", "minimum");
                }
            }
        }
        // When `audioQuality` has not been explicitly saved and the CPU is low-end, adopt `"original"`.
        if !self.saved.get("audioQuality").copied().unwrap_or(false) {
            if let Some(c) = cpu {
                if c.low_end_cpu {
                    self.set("audioQuality", "original");
                }
            }
        }
    }

    /// Converts the `mouseSensitivity` slider (`0.5..10.0`, default `5.0`) into the mouse look
    /// multiplier `2^((clamp(slider, 0.5, 10) - 5) / 5)` (`0.5 -> ~0.536x`, `5.0 -> 1.0x`, `10.0 -> 2.0x`),
    /// matching `Settings.mouseScale()` in `src/settings.js`.
    pub fn mouse_scale(&self) -> f64 {
        // Fall back to the neutral `5.0` midpoint if the stored value is non-finite.
        let raw = self.get_f64("mouseSensitivity");
        let n = if raw.is_finite() { raw } else { 5.0 };
        // Clamp to the slider range `[0.5, 10.0]` and evaluate `2^((slider - 5) / 5)`.
        let step = n.clamp(0.5, 10.0);
        2.0_f64.powf((step - 5.0) / 5.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verifies all 59 DEFAULTS keys, preset cascading, `physicsRate` lock, `mouseScale`, and `adoptHardwareDefaults`.
    #[test]
    fn settings_defaults_presets_and_hardware_rules_match_js() {
        assert_eq!(DEFAULTS.len(), 59);
        let mut s = Settings::new();
        assert!(s.fresh);
        assert_eq!(s.get_string("physicsRate"), "60");
        assert_eq!(s.get_string("performancePreset"), "custom");
        assert_eq!(s.get_string("playerName"), "PLAYER 1");
        assert_eq!(s.get_string("menuTheme"), "eternal");
        assert!((s.get_f64("masterVolume") - 0.8).abs() < 1e-9);
        assert!((s.mouse_scale() - 1.0).abs() < 1e-9);

        // `physicsRate` is locked to "60".
        s.set("physicsRate", "30");
        assert_eq!(s.get_string("physicsRate"), "60");

        // Adopting hardware defaults on an old Intel GPU + low-end CPU:
        let mut hw = Settings::new();
        let gpu = classify_gpu("Intel(R) HD Graphics 3000", "Intel");
        assert!(gpu.is_old_intel);
        let cpu = CpuInfo::new(4, 4.0);
        assert!(cpu.low_end_cpu);
        hw.adopt_hardware_defaults(Some(&gpu), Some(&cpu));
        assert_eq!(hw.get_string("performancePreset"), "minimum");
        assert!((hw.get_f64("renderScale") - 0.5).abs() < 1e-9);
        assert_eq!(hw.get_string("shadows"), "off");
        assert_eq!(hw.get_string("audioQuality"), "original");

        // Changing a preset key flips `performancePreset` back to `"custom"`.
        hw.set("shadows", "high");
        assert_eq!(hw.get_string("performancePreset"), "custom");
    }
}
