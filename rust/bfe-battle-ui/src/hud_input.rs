// Rust HUD input publication layer.
//
// Ports `src/hud-input.js` (`HudInput`, `relationCode`, `MAX_POSTS`, `MAX_UNITS`, `MAX_TEAMS`)
// and re-exports `crate::chat`.
//
// Architectural boundary (`src/hud-input.js` and `src/hud-layout.js`):
// - `HudInputPublisher` owns a flat `f64` staging buffer of length `WORDS` (`INPUT_LEN`),
//   laid out in fixed contiguous regions: `HudFrame` (`H`), 4 `HudWeapon` rows (`WEAPONS`),
//   `MAX_POSTS` (`128`) command posts (`POSTS`), `MAX_POSTS * MAX_TEAMS` post count rows
//   (`POST_COUNTS`), `MAX_UNITS` (`2048`) radar unit rows (`UNITS`), `MAX_TEAMS` (`32`) team
//   rows (`TEAMS`), retained state slots (`STATES`), spawn-map draw metadata (`MAP_DRAW`),
//   and `MAX_RINGS` (`128`) selection rings (`RINGS`).
// - Strings and textures are interned once into 1-based numeric handles (`0` = empty/absent)
//   so per-frame serialization never allocates strings, glyph tables, or texture descriptors.
// - Publication (`publish_to_slice`) copies only active regions into `HudModel::input` and
//   compares words via `f64::to_bits` (matching JS `Object.is`) so `NaN` sentinels (such as
//   an absent countdown timer) and signed zero never falsely dirty unchanged words.

use std::collections::HashMap;

use bfe_render::hud_model::layout::{
    HudCommand as C, HudFrame as H, HudMapDraw as M, HudPost as P, HudPostCount as PC,
    HudRing as R, HudState as S, HudTeam as T, HudUnit as U, HudWeapon as HW,
};
use bfe_render::hud_model::{
    HudModel, INPUT_LEN, MAP_DRAW, MAX_POSTS, MAX_TEAMS, MAX_UNITS, POSTS, POST_COUNTS, RINGS,
    STATES, TEAMS, UNITS, WEAPONS,
};

pub use crate::chat::*;

/// Maximum number of registered HUD texture assets (`MAX_ASSETS = 4096` in `src/hud-input.js`).
///
/// Requests beyond 4096 clip to `0` (the no-art handle) just as the legacy batch renderer
/// skipped unavailable textures.
pub const MAX_ASSETS: usize = 4096;

/// Maximum number of spawn map selection rings (`128` in `bfe_render::hud_model`).
pub const MAX_RINGS: usize = 128;

/// Total scalar length of the HUD input buffer (`INPUT_LEN` in `bfe_render::hud_model`).
pub const WORDS: usize = INPUT_LEN;

/// Fixed HUD art slot names in `HudArt` order (`Object.keys(HudArt).slice(1, -1)` in `src/hud-input.js`).
///
/// Pre-registered during `HudInputPublisher::new` so that fixed battle-art slots (`1..=29`)
/// retain deterministic asset handles even when a map omits one of the textures, followed
/// immediately by the solid white fallback slot (`"__solid__"` at index `30`).
pub const HUD_ART_NAMES: [&str; 29] = [
    "hud_objective_icon",
    "hud_DST_spawn_icon",
    "hud_enemy_icon",
    "hud_compass",
    "hud_health_trooper_icon",
    "hud_health_droideka_icon",
    "hud_flag_icon",
    "hud_circle",
    "hud_dots",
    "hud_Burst_bar",
    "hud_Clip_bar",
    "hud_Center_In_view",
    "hud_Not_in_view",
    "hud_white_semi_circle",
    "hud_vehicle_ seats",
    "hud_ Reinforcements_bar",
    "hud_reinforcements_icon",
    "hud_bar_fill",
    "hud_ map_cove",
    "hud_target_lock_on2",
    "hud_pain_direction",
    "weapon_scope3",
    "reticule_00",
    "reticule_01",
    "reticule_rifle",
    "reticule_rocket",
    "reticule_sniper",
    "hud_crosshair_ friend",
    "map_cone",
];

/// Converts a relation value (`1` friendly, `-1` enemy, `0` neutral) or label into the HUD relation code
/// (`relationCode` in `src/hud-input.js`).
///
/// Encodes `4.0` for `None`/null, `3.0` for indigenous locals (`team > 2`), `1.0` for friendly
/// (`relation > 0`), `2.0` for enemy (`relation < 0`), and `0.0` for neutral.
#[inline]
pub fn relation_code(relation: i32, is_locals: bool, is_none: bool) -> f64 {
    if is_none {
        4.0
    } else if is_locals {
        3.0
    } else if relation > 0 {
        1.0
    } else if relation < 0 {
        2.0
    } else {
        0.0
    }
}

/// Cold authored unit/weapon definition facts cached once on first observation (`definition` in `src/hud-input.js`).
///
/// Because ODF class definitions are static authored policy rather than per-frame mutable state,
/// their texture handles (`icon`, `reticle`, `map`, `health`), localized `label` handle, missile-lock
/// capability (`lock`), and sensor radii (`scan`, `transmit`) are resolved once and reused across frames.
#[derive(Clone, Debug, Default)]
pub struct HudDefinitionFacts {
    /// 1-based asset handle for `IconTexture` (`0` if unauthored).
    pub icon: u32,
    /// 1-based interned text handle for the weapon or unit display `label`.
    pub label: u32,
    /// 1-based asset handle for `ReticuleTexture` (`0` if unauthored).
    pub reticle: u32,
    /// 1-based asset handle for `MapTexture` (`0` if unauthored).
    pub map: u32,
    /// 1-based asset handle for `HealthTexture` (`0` if unauthored).
    pub health: u32,
    /// Whether the weapon definition enables missile target lock (`hasMissileLock(def)`).
    pub lock: bool,
    /// Whether `MapTexture != null` on the authored unit definition (`hasMap` in `src/hud-input.js`).
    pub has_map: bool,
    /// Authored `MapScale` (`NaN` when omitted so `HudModel` applies its class default).
    pub scale: f64,
    /// Radar sensor `ScanningRange` in world meters (`0.0` if none).
    pub scan: f64,
    /// Radar sensor `TransmitRange` in world meters (`0.0` if none).
    pub transmit: f64,
}

/// Scalar weapon state published into one of the 4 HUD weapon rows (`weapon` in `src/hud-input.js`).
///
/// Rows `0` and `1` describe the bottom-right weapon status panel (primary and secondary arsenal slots),
/// while rows `2` and `3` describe the active reticle weapons (which can differ from the soldier's
/// personal arsenal during mounted vehicle or turret previews).
#[derive(Clone, Debug, Default)]
pub struct HudWeaponFact {
    /// Stable 1-based identity of the weapon instance (`weaponId` in `src/hud-input.js`);
    /// changing `id` triggers the weapon name fade-in even between equal ODF definitions.
    pub id: u32,
    pub def: HudDefinitionFacts,
    pub lock_target_id: i32,
    pub lock_elapsed: f64,
    pub lock_time: f64,
    pub charging: bool,
    pub held: f64,
    pub max_pressed: f64,
    pub reloading: bool,
    pub reload_left: f64,
    pub reload_time: f64,
    pub bottomless: bool,
    pub heat_per_shot: f64,
    pub heat: f64,
    pub overheated: bool,
    pub loaded: f64,
    pub spare: f64,
    pub reserve: f64,
    pub rounds_per_clip: f64,
    pub wait: f64,
    pub shot_delay: f64,
    pub lock_off_angle: f64,
    pub kick: f64,
    pub still_spread: f64,
    pub yaw_spread: f64,
    pub pitch_spread: f64,
}

/// Scalar command post fact published into the HUD posts table (`posts` in `src/hud-input.js`).
///
/// All objectives—including inactive or unmapped posts—are published as raw game facts so the
/// native `HudModel` can evaluate map eligibility, capture progress bars, and contested blink colors.
#[derive(Clone, Debug, Default)]
pub struct HudPostFact {
    pub x: f64,
    pub z: f64,
    pub mapped: bool,
    pub active: bool,
    pub team: i32,
    pub relation: f64,
    pub neutralize: f64,
    pub progress: f64,
    pub toward: i32,
    pub toward_relation: f64,
    pub contested: bool,
    /// Insertion-ordered `(team, unit_count, relation_code)` entries inside the capture zone.
    pub counts: Vec<(i32, i32, f64)>,
    pub texture: u32,
    pub scale: f64,
    pub label: u32,
}

/// Scalar unit fact published into the HUD radar/units table (`units` in `src/hud-input.js`).
///
/// Separates simulation-rate facts (`x`, `y`, `z`, `removed`, `seated`, `dead`, `team`, sensor ranges)
/// from render-rate presentation pose (`visual_x`, `visual_z`, `yaw`, `quaternion`, `forward`).
#[derive(Clone, Debug)]
pub struct HudUnitFact {
    /// Stable caller key for the entity facade.
    pub entity_key: u64,
    /// Entity spawn/reuse generation from the simulation; bumping this allocates a new HUD lease
    /// generation so radar discovery and recent-shot memory from a previous occupant are cleared.
    pub source_generation: u32,
    pub def: HudDefinitionFacts,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub visual_x: f64,
    pub visual_z: f64,
    pub has_position: bool,
    pub removed: bool,
    pub seated: bool,
    pub dead: bool,
    pub kind: f64,
    pub soldier: bool,
    pub hero: bool,
    pub alive: bool,
    pub team: i32,
    pub yaw: f64,
    pub quaternion: Option<[f64; 4]>,
    pub forward: Option<[f64; 2]>,
}

impl Default for HudUnitFact {
    fn default() -> Self {
        Self {
            entity_key: 0,
            source_generation: 0,
            def: HudDefinitionFacts::default(),
            x: 0.0,
            y: 0.0,
            z: 0.0,
            visual_x: 0.0,
            visual_z: 0.0,
            has_position: true,
            removed: false,
            seated: false,
            dead: false,
            kind: f64::NAN,
            soldier: true,
            hero: false,
            alive: true,
            team: 0,
            yaw: 0.0,
            quaternion: None,
            forward: None,
        }
    }
}

/// Team radar/ticket table published once per update (`teams` in `src/hud-input.js`).
///
/// Providing team relations, ticket counts, and optional living-unit overrides as a 32-row table
/// avoids per-entity callback overhead during radar sensor sweeps.
#[derive(Clone, Copy, Debug)]
pub struct HudTeamFact {
    pub relation: f64,
    pub locals: bool,
    pub tickets: f64,
    pub living: f64,
}

impl Default for HudTeamFact {
    fn default() -> Self {
        Self {
            relation: 0.0,
            locals: false,
            tickets: f64::INFINITY,
            living: f64::NAN,
        }
    }
}

/// Internal lease record mapping an `entity_key` to a reusable radar slot (`1..=MAX_UNITS`)
/// and monotonically increasing `generation` (`unitIds` WeakMap in `src/hud-input.js`).
#[derive(Clone, Debug, Default)]
struct UnitLeaseRow {
    id: usize,
    generation: u32,
    source_generation: u32,
    def: HudDefinitionFacts,
    fact: HudUnitFact,
}

/// Metadata for a registered HUD texture asset (`assets` in `src/hud-input.js`).
///
/// Retaining a stable 1-based asset slot across GPU raster atlas upgrades allows `HudModel` rows
/// to keep their numeric texture handles even when backing GPU storage is resized.
#[derive(Clone, Debug, Default)]
pub struct HudAssetSlot {
    pub name: String,
    pub handle: i32,
    pub width: i32,
    pub height: i32,
    pub ready: bool,
}

/// Zero-allocation-per-frame HUD input staging and publication layer (`HudInput` in `src/hud-input.js`).
///
/// Retained for the lifetime of a battle. Setters write into `self.f` (`WORDS` `f64` scalars)
/// and `publish_to_slice` copies only active, modified sub-ranges into `HudModel::input`.
pub struct HudInputPublisher {
    /// Staging scalar buffer (`WORDS` f64 elements).
    pub f: Vec<f64>,
    /// 1-based interned text table (`index 0` is reserved for the empty string).
    pub text_values: Vec<String>,
    texts: HashMap<String, u32>,
    /// 1-based registered texture asset table (`index 0` is reserved for no-art).
    pub assets: Vec<HudAssetSlot>,
    asset_ids: HashMap<String, u32>,
    /// Bitmask of `HudCommand` state kinds observed at least once (`observedStates` in `src/hud-input.js`).
    observed_states: u32,
    next_generation: u32,
    max_id: usize,
    /// `owners[slot_id]` stores the `entity_key` currently occupying radar slot `1..=MAX_UNITS`.
    owners: Vec<Option<u64>>,
    /// LIFO stack of available radar slot IDs (`1..=MAX_UNITS`, initialized so slot `1` is popped first).
    free: Vec<usize>,
    free_count: usize,
    /// Stamp of the last `set_units(true, ...)` pass that touched each slot (`1..=MAX_UNITS`).
    used: Vec<u32>,
    unit_leases: HashMap<u64, UnitLeaseRow>,
    /// Ordered `entity_key` list for the current update's active radar rows (`0..row_count`).
    rows: Vec<u64>,
    pub row_count: usize,
    /// Per-slot deduplication stamp array (`seen[slot_id] == stamp`) preventing duplicate rows
    /// when the same entity appears in both the unit registry and the extra objects list.
    seen: Vec<u32>,
    stamp: u32,
    /// Count of immediate events dispatched.
    pub event_count: usize,
    /// Recorded `(kind, a, b, c, d)` samples observed by `state()`.
    pub observed_samples: Vec<(u32, f64, f64, f64, f64)>,
}

impl Default for HudInputPublisher {
    fn default() -> Self {
        Self::new()
    }
}

impl HudInputPublisher {
    /// Creates a new `HudInputPublisher` and pre-reserves the fixed `HudArt` slots and default strings.
    ///
    /// Matches the `HudInput` constructor in `src/hud-input.js`:
    /// - Initializes `f[H::COUNTDOWN]` to `NaN` so no countdown timer renders until set.
    /// - Fills the `free` slot stack in reverse (`MAX_UNITS` down to `1`) so `--free_count` pops `1` first.
    /// - Registers all 29 `HUD_ART_NAMES` plus `"__solid__"`, then interns the default victory,
    ///   defeat, leaving-battlefield, and 4 kill-feed template strings.
    pub fn new() -> Self {
        let mut f = vec![0.0; WORDS];
        f[H::COUNTDOWN] = f64::NAN;

        let mut free = vec![0usize; MAX_UNITS];
        for (i, slot) in free.iter_mut().enumerate() {
            *slot = MAX_UNITS - i;
        }

        let mut pub_layer = Self {
            f,
            text_values: vec![String::new()], // index 0 is null
            texts: HashMap::with_capacity(64),
            assets: vec![HudAssetSlot::default()], // index 0 is null
            asset_ids: HashMap::with_capacity(64),
            observed_states: 0,
            next_generation: 1,
            max_id: 0,
            owners: vec![None; MAX_UNITS + 1],
            free,
            free_count: MAX_UNITS,
            used: vec![0; MAX_UNITS + 1],
            unit_leases: HashMap::with_capacity(256),
            rows: Vec::with_capacity(MAX_UNITS),
            row_count: 0,
            seen: vec![0; MAX_UNITS + 1],
            stamp: 0,
            event_count: 0,
            observed_samples: Vec::new(),
        };

        for name in HUD_ART_NAMES {
            pub_layer.asset(name);
        }
        // Solid white texture slot right after fixed art.
        pub_layer.register_unique_asset("__solid__");

        // Pre-populate localized banner and kill-feed template text handles in `HudFrame`
        // (`H.victory`, `H.defeat`, `H.leavingText`, `H.killTemplates` in `src/hud-input.js`).
        pub_layer.f[H::VICTORY] = pub_layer.text("", false) as f64;
        pub_layer.f[H::DEFEAT] = pub_layer.text("", false) as f64;
        pub_layer.f[H::LEAVING_TEXT] = pub_layer.text("LEAVING BATTLEFIELD", false) as f64;
        pub_layer.f[H::KILL_TEMPLATES] = pub_layer.text("%s DIED", true) as f64;
        pub_layer.f[H::KILL_TEMPLATES + 1] = pub_layer.text("%s KILLED HIMSELF", true) as f64;
        pub_layer.f[H::KILL_TEMPLATES + 2] = pub_layer.text("%s KILLED TEAMMATE %s", true) as f64;
        pub_layer.f[H::KILL_TEMPLATES + 3] = pub_layer.text("%s KILLED %s", true) as f64;

        pub_layer
    }

    /// Interns a text string and returns its 1-based handle (`0` for empty strings, `text` in `src/hud-input.js`).
    ///
    /// When `uppercase` is `true`, the string is uppercased before lookup/insertion so kill-feed
    /// templates and command post labels share canonical uppercase handles.
    pub fn text(&mut self, value: &str, uppercase: bool) -> u32 {
        if value.is_empty() {
            return 0;
        }
        let key = if uppercase {
            value.to_uppercase()
        } else {
            value.to_string()
        };
        if let Some(&id) = self.texts.get(&key) {
            return id;
        }
        let id = self.text_values.len() as u32;
        self.text_values.push(key.clone());
        self.texts.insert(key, id);
        id
    }

    /// Interns a named art texture and returns its 1-based asset handle (`0` if empty or over `MAX_ASSETS = 4096`).
    pub fn asset(&mut self, name: &str) -> u32 {
        if name.is_empty() {
            return 0;
        }
        if let Some(&id) = self.asset_ids.get(name) {
            return id;
        }
        let id = self.register_unique_asset(name);
        if id != 0 {
            self.asset_ids.insert(name.to_string(), id);
        }
        id
    }

    /// Appends a new `HudAssetSlot` with initial unready sentinel metadata (`handle = -2`, `width = -1`, `height = -1`).
    fn register_unique_asset(&mut self, name: &str) -> u32 {
        let id = self.assets.len();
        if id >= MAX_ASSETS {
            return 0;
        }
        self.assets.push(HudAssetSlot {
            name: name.to_string(),
            handle: -2,
            width: -1,
            height: -1,
            ready: false,
        });
        id as u32
    }

    /// Updates retained state for `kind` (`C::HEALTH`, `C::HULL`, `C::STATS`, `C::SECOND`, `C::SUPPLY`, etc.)
    /// (`state` in `src/hud-input.js`).
    ///
    /// Health, hull, and stats transitions are observed immediately when their values change
    /// (because a simulation preview can change twice before a frame is drawn, and damage-flash /
    /// stat-delta indicators depend on seeing each transition), while repeated identical values
    /// and secondary bar fills stay local in the staging buffer without queuing commands.
    pub fn state(
        &mut self,
        kind: u32,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        hud_model: Option<&mut HudModel>,
    ) {
        let o = STATES + (kind as usize) * S::WORDS;
        let is_transition_kind = kind == C::HEALTH || kind == C::HULL || kind == C::STATS;
        let bit = 1u32 << kind;
        let changed = (self.observed_states & bit) == 0
            || self.f[o] != a
            || self.f[o + 1] != b
            || self.f[o + 2] != c
            || self.f[o + 3] != d;

        if is_transition_kind && changed {
            if let Some(hm) = hud_model {
                hm.event(kind, a, b, c, d);
            }
            self.observed_samples.push((kind, a, b, c, d));
            self.observed_states |= bit;
        }

        // Overwrite the retained state slot and mark its dirty bit in `H::STATES`.
        self.f[o + S::VALUES] = a;
        self.f[o + S::VALUES + 1] = b;
        self.f[o + S::VALUES + 2] = c;
        self.f[o + S::VALUES + 3] = d;
        let prev_states = self.f[H::STATES] as u32;
        self.f[H::STATES] = (prev_states | bit) as f64;
    }

    /// Dispatches an immediate HUD event (`C::MESSAGE`, `C::KILL`, `C::HIT`, `C::SHOT`, etc.)
    /// (`event` in `src/hud-input.js`).
    ///
    /// Real events enter `HudModel` immediately at its current visual clock so a stalled render
    /// or long asset load never accumulates an unbounded host backlog.
    pub fn event(
        &mut self,
        kind: u32,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        hud_model: Option<&mut HudModel>,
    ) {
        self.event_count += 1;
        if let Some(hm) = hud_model {
            hm.event(kind, a, b, c, d);
        }
    }

    /// Convenience helper for `BattleHud.setPlayer` (`C::HEALTH`).
    pub fn set_player(&mut self, health: f64, hud_model: Option<&mut HudModel>) {
        self.state(C::HEALTH, health, 0.0, 0.0, 0.0, hud_model);
    }

    /// Convenience helper for `BattleHud.setSecondBar` (`C::SECOND`).
    pub fn set_second_bar(&mut self, kind_code: f64, fill: f64, hud_model: Option<&mut HudModel>) {
        self.state(C::SECOND, kind_code, fill, 0.0, 0.0, hud_model);
    }

    /// Convenience helper for `BattleHud.setHull` (`C::HULL`).
    pub fn set_hull(&mut self, kind_code: f64, fill: f64, hud_model: Option<&mut HudModel>) {
        self.state(C::HULL, kind_code, fill, 0.0, 0.0, hud_model);
    }

    /// Convenience helper for `BattleHud.setSupply` (`C::SUPPLY`).
    pub fn set_supply(&mut self, health: bool, ammo: bool, hud_model: Option<&mut HudModel>) {
        self.state(
            C::SUPPLY,
            if health { 1.0 } else { 0.0 },
            if ammo { 1.0 } else { 0.0 },
            0.0,
            0.0,
            hud_model,
        );
    }

    /// Convenience helper for `BattleHud.stats` (`C::STATS`).
    pub fn set_stats(
        &mut self,
        kills: i32,
        deaths: i32,
        captures: i32,
        hud_model: Option<&mut HudModel>,
    ) {
        self.state(
            C::STATS,
            kills as f64,
            deaths as f64,
            captures as f64,
            0.0,
            hud_model,
        );
    }

    /// Writes a weapon row (`0..4`) into the staging buffer (`weapon` in `src/hud-input.js`).
    ///
    /// Passing `None` zeros out all `HW::WORDS` (`29`) scalars for that row. Otherwise copies
    /// the cached `HudDefinitionFacts` handles along with live lock, charge, reload, heat,
    /// clip/reserve ammo, and stance-spread scalars.
    pub fn set_weapon(&mut self, row: usize, weapon: Option<&HudWeaponFact>) {
        if row >= 4 {
            return;
        }
        let o = WEAPONS + row * HW::WORDS;
        let Some(w) = weapon else {
            self.f[o..o + HW::WORDS].fill(0.0);
            return;
        };
        self.f[o + HW::ID] = w.id as f64;
        self.f[o + HW::ICON] = w.def.icon as f64;
        self.f[o + HW::LABEL] = w.def.label as f64;
        self.f[o + HW::RETICLE] = w.def.reticle as f64;
        self.f[o + HW::LOCK] = if w.def.lock { 1.0 } else { 0.0 };
        self.f[o + HW::LOCK_TARGET] = w.lock_target_id as f64;
        self.f[o + HW::LOCK_ELAPSED] = w.lock_elapsed;
        self.f[o + HW::LOCK_TIME] = w.lock_time;
        self.f[o + HW::CHARGING] = if w.charging { 1.0 } else { 0.0 };
        self.f[o + HW::HELD] = w.held;
        self.f[o + HW::MAX_PRESSED] = w.max_pressed;
        self.f[o + HW::RELOADING] = if w.reloading { 1.0 } else { 0.0 };
        self.f[o + HW::RELOAD_LEFT] = w.reload_left;
        self.f[o + HW::RELOAD_TIME] = w.reload_time;
        self.f[o + HW::BOTTOMLESS] = if w.bottomless { 1.0 } else { 0.0 };
        self.f[o + HW::HEAT_PER_SHOT] = w.heat_per_shot;
        self.f[o + HW::HEAT] = w.heat;
        self.f[o + HW::OVERHEATED] = if w.overheated { 1.0 } else { 0.0 };
        self.f[o + HW::LOADED] = w.loaded;
        self.f[o + HW::SPARE] = w.spare;
        self.f[o + HW::RESERVE] = w.reserve;
        self.f[o + HW::ROUNDS_PER_CLIP] = w.rounds_per_clip;
        self.f[o + HW::WAIT] = w.wait;
        self.f[o + HW::SHOT_DELAY] = w.shot_delay;
        self.f[o + HW::LOCK_OFF_ANGLE] = w.lock_off_angle;
        // Spread facts represent the authored stance minima (`StandStillSpread`, `YawSpread`,
        // `PitchSpread`) combined with live recoil `kick` (`weapon` in `src/hud-input.js`).
        self.f[o + HW::KICK] = w.kick;
        self.f[o + HW::STILL_SPREAD] = w.still_spread;
        self.f[o + HW::YAW_SPREAD] = w.yaw_spread;
        self.f[o + HW::PITCH_SPREAD] = w.pitch_spread;
    }

    /// Publishes command post facts (`posts` in `src/hud-input.js`), clipping at `MAX_POSTS` (`128`)
    /// and `MAX_TEAMS` (`32`).
    ///
    /// Preserves the insertion order of `post.counts` inside `POST_COUNTS`: the original game
    /// selects the first opposing team in iteration order and distinguishes a present key with
    /// count `0` (during neutralization) from a positive count (contested neutral capture).
    pub fn set_posts(&mut self, posts: &[HudPostFact], capture_post: Option<usize>) {
        self.f[H::CAPTURE_POST] = capture_post.map(|idx| idx as f64).unwrap_or(-1.0);
        let mut count = 0usize;
        for post in posts.iter().take(MAX_POSTS) {
            let index = count;
            count += 1;
            let o = POSTS + index * P::WORDS;
            self.f[o + P::X] = post.x;
            self.f[o + P::Z] = post.z;
            self.f[o + P::MAPPED] = if post.mapped { 1.0 } else { 0.0 };
            self.f[o + P::ACTIVE] = if post.active { 1.0 } else { 0.0 };
            self.f[o + P::TEAM] = post.team as f64;
            self.f[o + P::RELATION] = post.relation;
            self.f[o + P::NEUTRALIZE] = post.neutralize;
            self.f[o + P::PROGRESS] = post.progress;
            self.f[o + P::TOWARD] = post.toward as f64;
            self.f[o + P::TOWARD_RELATION] = post.toward_relation;
            self.f[o + P::CONTESTED] = if post.contested { 1.0 } else { 0.0 };

            // Each post owns a stride of `MAX_TEAMS * PC::WORDS` in `POST_COUNTS`.
            let start = POST_COUNTS + index * MAX_TEAMS * PC::WORDS;
            self.f[o + P::COUNTS] = start as f64;
            let mut n = 0usize;
            for &(team, cnt, rel) in post.counts.iter().take(MAX_TEAMS) {
                let row = start + n * PC::WORDS;
                n += 1;
                self.f[row + PC::TEAM] = team as f64;
                self.f[row + PC::COUNT] = cnt as f64;
                self.f[row + PC::RELATION] = rel;
            }
            self.f[o + P::COUNTS_LENGTH] = n as f64;
            self.f[o + P::TEXTURE] = post.texture as f64;
            // Default command post map icon scale is `2.5` when unauthored (`0.0`).
            self.f[o + P::SCALE] = if post.scale == 0.0 { 2.5 } else { post.scale };
            self.f[o + P::LABEL] = post.label as f64;
        }
        self.f[H::POSTS] = count as f64;
    }

    /// Publishes team relation/ticket/living tables (`teams` in `src/hud-input.js`).
    ///
    /// Missing team entries default to neutral relation (`0.0`), `locals = team > 2`,
    /// infinite tickets (`f64::INFINITY`), and `NaN` living count (telling native radar to
    /// compute living counts from the unit table).
    pub fn set_teams(&mut self, teams: &[HudTeamFact]) {
        for team in 0..MAX_TEAMS {
            let o = TEAMS + team * T::WORDS;
            let tf = teams.get(team).copied().unwrap_or(HudTeamFact {
                relation: 0.0,
                locals: team > 2,
                tickets: f64::INFINITY,
                living: f64::NAN,
            });
            self.f[o + T::RELATION] = tf.relation;
            self.f[o + T::LOCALS] = if tf.locals { 1.0 } else { 0.0 };
            self.f[o + T::TICKETS] = tf.tickets;
            self.f[o + T::LIVING] = tf.living;
        }
    }

    /// Acquires or refreshes the 1-based radar slot (`1..=MAX_UNITS`) and lease generation for `unit`
    /// (`unitId` in `src/hud-input.js`).
    ///
    /// If the entity has no slot, lost its slot to recycling while absent, or changed its
    /// `source_generation`, a fresh `next_generation` is assigned so `HudModel` discards stale
    /// radar discovery and weapon-fire state from the previous occupant.
    fn acquire_unit_slot(&mut self, unit: &HudUnitFact) -> usize {
        if unit.entity_key == 0 {
            return 0;
        }
        let mut needs_new_slot = false;
        let mut needs_new_gen = false;
        if let Some(existing) = self.unit_leases.get(&unit.entity_key) {
            if existing.id == 0 || self.owners[existing.id] != Some(unit.entity_key) {
                needs_new_slot = true;
            } else if existing.source_generation != unit.source_generation {
                needs_new_gen = true;
            }
        } else {
            needs_new_slot = true;
        }

        // Allocate a free radar slot from the LIFO stack and bump `next_generation` so the
        // native radar model treats this slot as a newly discovered entity facade.
        if needs_new_slot {
            if self.free_count == 0 {
                return 0;
            }
            self.free_count -= 1;
            let slot_id = self.free[self.free_count];
            self.owners[slot_id] = Some(unit.entity_key);
            self.max_id = self.max_id.max(slot_id);
            let gen = self.next_generation;
            self.next_generation += 1;
            self.unit_leases.insert(
                unit.entity_key,
                UnitLeaseRow {
                    id: slot_id,
                    generation: gen,
                    source_generation: unit.source_generation,
                    def: unit.def.clone(),
                    fact: unit.clone(),
                },
            );
            self.used[slot_id] = self.stamp;
            return slot_id;
        }

        let Some(entry) = self.unit_leases.get_mut(&unit.entity_key) else {
            return 0;
        };
        // If the underlying entity respawned or changed `source_generation` in the same slot,
        // advance `generation` without reallocating the slot index.
        if needs_new_gen {
            entry.source_generation = unit.source_generation;
            entry.generation = self.next_generation;
            self.next_generation += 1;
        }
        entry.def = unit.def.clone();
        entry.fact = unit.clone();
        let slot_id = entry.id;
        self.used[slot_id] = self.stamp;
        slot_id
    }

    /// Updates radar unit rows (`units(update)` in `src/hud-input.js`), recycling inactive slots
    /// and clipping at `MAX_UNITS` (`2048`).
    ///
    /// When `update` is `true` (simulation tick), slots not seen in the previous update or marked
    /// `removed` are returned to `self.free`, `self.stamp` advances, and simulation facts
    /// (`ID`, `GENERATION`, `X`/`Y`/`Z`, `TEAM`, sensor ranges) are frozen into `UNITS`.
    /// When `update` is `false` (render frame), only visual presentation pose (`VISUAL_X`, `VISUAL_Z`,
    /// `YAW`, `QUATERNION`, `FORWARD`) is refreshed on the frozen rows so a render-time pose update
    /// cannot trigger a second sensor query or add a marker before the next simulation tick.
    pub fn set_units(
        &mut self,
        update: bool,
        units: &[HudUnitFact],
        viewer_key: Option<u64>,
        controllable_key: Option<u64>,
        viewer_team: i32,
        easy: bool,
    ) {
        if update {
            // Reclaim slots that were not touched during the prior update or whose entity was removed.
            for id in 1..=self.max_id {
                if let Some(owner_key) = self.owners[id] {
                    let removed = self
                        .unit_leases
                        .get(&owner_key)
                        .map(|r| r.fact.removed)
                        .unwrap_or(true);
                    if self.used[id] != self.stamp || removed {
                        self.owners[id] = None;
                        self.free[self.free_count] = id;
                        self.free_count += 1;
                    }
                }
            }
            self.row_count = 0;
            self.rows.clear();
            // Advance the deduplication stamp (`>>> 0` wrap-around in JS resets `seen` to 0).
            self.stamp = self.stamp.wrapping_add(1);
            if self.stamp == 0 {
                self.seen.fill(0);
                self.stamp = 1;
            }
            for unit in units {
                let id = self.acquire_unit_slot(unit);
                if id == 0 || self.seen[id] == self.stamp {
                    continue;
                }
                self.seen[id] = self.stamp;
                if self.row_count >= MAX_UNITS {
                    continue;
                }
                self.rows.push(unit.entity_key);
                self.row_count += 1;
            }
        }

        for i in 0..self.row_count {
            let key = self.rows[i];
            let Some(record) = self.unit_leases.get(&key) else {
                continue;
            };
            let o = UNITS + i * U::WORDS;
            let u = &record.fact;
            // Simulation-rate unit properties (`id`, `generation`, world position, alive/dead/seated
            // flags, and radar definition metrics) are written only when `update` is `true`.
            if update {
                self.f[o + U::ID] = record.id as f64;
                self.f[o + U::GENERATION] = record.generation as f64;
                self.f[o + U::X] = u.x;
                self.f[o + U::Y] = u.y;
                self.f[o + U::Z] = u.z;
                self.f[o + U::REMOVED] = if u.removed { 1.0 } else { 0.0 };
                self.f[o + U::SEATED] = if u.seated { 1.0 } else { 0.0 };
                self.f[o + U::DEAD] = if u.dead { 1.0 } else { 0.0 };
                self.f[o + U::HAS_POSITION] = if u.has_position { 1.0 } else { 0.0 };
                self.f[o + U::KIND] = u.kind;
                self.f[o + U::SOLDIER] = if u.soldier { 1.0 } else { 0.0 };
                self.f[o + U::HERO] = if u.hero { 1.0 } else { 0.0 };
                self.f[o + U::ALIVE] = if u.alive { 1.0 } else { 0.0 };
                self.f[o + U::TEAM] = u.team as f64;
                self.f[o + U::TEXTURE] = record.def.map as f64;
                self.f[o + U::HAS_MAP_TEXTURE] = if record.def.has_map { 1.0 } else { 0.0 };
                self.f[o + U::SCALE] = record.def.scale;
                self.f[o + U::SCAN] = record.def.scan;
                self.f[o + U::TRANSMIT] = record.def.transmit;
            }
            // Presentation position and heading (`heading(unit, o)` in `src/hud-input.js`).
            self.f[o + U::VISUAL_X] = u.visual_x;
            self.f[o + U::VISUAL_Z] = u.visual_z;
            self.f[o + U::YAW] = u.yaw;
            // Hull quaternion takes precedence for vehicle/flyer heading (`heading` in `src/hud-input.js`),
            // falling back to a 2D forward vector `(forward_x, forward_z)` when no quaternion exists.
            if let Some(q) = u.quaternion {
                self.f[o + U::HAS_QUATERNION] = 1.0;
                self.f[o + U::QUATERNION] = q[0];
                self.f[o + U::QUATERNION + 1] = q[1];
                self.f[o + U::QUATERNION + 2] = q[2];
                self.f[o + U::QUATERNION + 3] = q[3];
                self.f[o + U::FORWARD] = 1.0;
            } else if let Some(fwd) = u.forward {
                self.f[o + U::HAS_QUATERNION] = 0.0;
                self.f[o + U::FORWARD] = 1.0;
                self.f[o + U::FORWARD_X] = fwd[0];
                self.f[o + U::FORWARD_Z] = fwd[1];
            } else {
                self.f[o + U::HAS_QUATERNION] = 0.0;
                self.f[o + U::FORWARD] = 0.0;
            }
        }

        // Publish viewer and controllable unit slot IDs and sensor origin.
        self.f[H::UNITS] = self.row_count as f64;
        let viewer_id = viewer_key
            .and_then(|k| self.unit_leases.get(&k).map(|r| r.id))
            .unwrap_or(0);
        let control_id = controllable_key
            .and_then(|k| self.unit_leases.get(&k).map(|r| r.id))
            .unwrap_or(0);
        self.f[H::VIEWER] = viewer_id as f64;
        self.f[H::CONTROLLABLE] = control_id as f64;
        self.f[H::HAS_VIEWER] = if viewer_id != 0 { 1.0 } else { 0.0 };
        self.f[H::EASY] = if easy { 1.0 } else { 0.0 };
        self.f[H::VIEWER_TEAM] = viewer_team as f64;
        if let Some(ctrl_row) = controllable_key.and_then(|k| self.unit_leases.get(&k)) {
            self.f[H::HAS_CONTROL] = if ctrl_row.fact.has_position { 1.0 } else { 0.0 };
            self.f[H::CONTROL_X] = ctrl_row.fact.x;
            self.f[H::CONTROL_Y] = ctrl_row.fact.y;
            self.f[H::CONTROL_Z] = ctrl_row.fact.z;
            self.f[H::CONTROL_SCAN] = ctrl_row.def.scan;
        } else {
            self.f[H::HAS_CONTROL] = 0.0;
            self.f[H::CONTROL_X] = 0.0;
            self.f[H::CONTROL_Y] = 0.0;
            self.f[H::CONTROL_Z] = 0.0;
            self.f[H::CONTROL_SCAN] = 0.0;
        }
    }

    /// Copies `[start..end]` from `src` into `dst`, writing only elements whose bit pattern changed
    /// (`copy` in `src/hud-input.js`).
    #[inline]
    fn copy_slice(src: &[f64], dst: &mut [f64], start: usize, end: usize) {
        let len = src.len().min(dst.len());
        let s = start.min(len);
        let e = end.min(len);
        for i in s..e {
            // `to_bits` matches JS `Object.is` (preserving NaN sentinels and signed zero).
            if dst[i].to_bits() != src[i].to_bits() {
                dst[i] = src[i];
            }
        }
    }

    /// Publishes only the active regions of `self.f` into `target` (`HudModel::input` or a test slice)
    /// and clears `self.f[H::STATES]` (`publish` in `src/hud-input.js`).
    ///
    /// Skipping unused tail slots in `POSTS`, `POST_COUNTS`, `UNITS`, and `RINGS` avoids scanning
    /// the full `WORDS` buffer every frame when only a handful of posts and units are active.
    pub fn publish_to_slice(&mut self, target: &mut [f64]) {
        // 1. Copy `HudFrame` (`0..WEAPONS`), all 4 `WEAPONS` rows, and the active `POSTS` prefix.
        let post_count = (self.f[H::POSTS] as usize).min(MAX_POSTS);
        Self::copy_slice(&self.f, target, 0, POSTS + post_count * P::WORDS);
        for i in 0..post_count {
            let start = POST_COUNTS + i * MAX_TEAMS * PC::WORDS;
            let len = (self.f[POSTS + i * P::WORDS + P::COUNTS_LENGTH] as usize).min(MAX_TEAMS);
            Self::copy_slice(&self.f, target, start, start + len * PC::WORDS);
        }
        // 2. Copy active `UNITS` rows (`0..unit_count`), then all `TEAMS` and `STATES` up to `MAP_DRAW`.
        let unit_count = (self.f[H::UNITS] as usize).min(MAX_UNITS);
        Self::copy_slice(&self.f, target, UNITS, UNITS + unit_count * U::WORDS);
        Self::copy_slice(&self.f, target, TEAMS, MAP_DRAW);
        // 3. Copy `MAP_DRAW` and active spawn-map selection `RINGS`, then clear the frame dirty mask.
        let ring_count = (self.f[MAP_DRAW + M::RINGS] as usize).min(MAX_RINGS);
        Self::copy_slice(&self.f, target, MAP_DRAW, RINGS + ring_count * R::WORDS);
        self.f[H::STATES] = 0.0;
    }

    /// Publishes active regions directly into `hud_model.input`.
    pub fn publish(&mut self, hud_model: &mut HudModel) {
        self.publish_to_slice(&mut hud_model.input);
    }
}
