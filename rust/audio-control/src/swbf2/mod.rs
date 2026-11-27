//! 2005 audio control state and per-frame dispatch, kept beside the 2004 path
//! and selected only when `Phase::family() == GameFamily::Swbf2`.

pub mod ambience;
pub mod director;
pub mod powers;
pub mod saber;
pub mod vehicles;
pub mod weapons;

use crate::engine::{Ask, Engine, Shape, AMBIENCE, CAPACITY, FOLEY, NONE, SILENT, VOICE, WEAPONS};
use crate::layout::{event as ev, frame as f};
use crate::phase::{index, Phase, RECORDS};
use crate::{Collision, Point};
use bfe_family::GameFamily;

/// Case-folded 32-bit FNV-1a hash used for 2005 audio, class, combo, state and
/// placed-object identifiers (ORs each byte with `0x20`; empty string hashes to `0`).
pub fn zero_hash(name: &str) -> u32 {
    if name.is_empty() {
        return 0;
    }
    name.bytes()
        .fold(0x811c_9dc5u32, |h, b| (h ^ ((b | 0x20) as u32)).wrapping_mul(0x0100_0193))
}

/// A sound property parameter that can be scaled per sound group at load or
/// from a mission script.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundParam {
    Gain,
    Pitch,
    MinDistance,
    MaxDistance,
    MuteDistance,
}

/// One delayed or immediate sound trigger attached to a melee combo state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ComboSoundTrigger {
    pub sound: usize,
    pub delay_s: f64,
}

/// Triggers belonging to a single hashed melee combo state (`zero_hash(state_name)`).
#[derive(Clone, Debug, Default)]
pub struct ComboStateSounds {
    pub state_hash: u32,
    pub triggers: Vec<ComboSoundTrigger>,
}

/// All combo-state sound triggers for one combo machine (`ComboMachine::name_hash`).
#[derive(Clone, Debug, Default)]
pub struct ComboSoundSet {
    pub combo_hash: u32,
    pub states: Vec<ComboStateSounds>,
}

/// Maximum condition tags stored per `VOSound` rule.
pub const MAX_VO_CONDITIONS: usize = 8;

/// One tagged voice-over rule parsed from a class's `VOSound` property.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoRule {
    pub sound: usize,
    pub event_hash: u32,
    /// Authored condition tag hashes (`+Tag` tokens after the event);
    /// the leading `condition_count` entries are active.
    pub conditions: [u32; MAX_VO_CONDITIONS],
    pub condition_count: u8,
}

impl VoRule {
    /// Constructs a `VoRule` from an authored condition list, ignoring zeros and duplicates.
    /// Returns `None` if more than `MAX_VO_CONDITIONS` distinct non-zero conditions are given.
    pub fn try_new(sound: usize, event_hash: u32, conds: &[u32]) -> Option<Self> {
        let mut conditions = [0u32; MAX_VO_CONDITIONS];
        let mut count = 0usize;
        for &c in conds {
            if c == 0 || conditions[..count].contains(&c) {
                continue;
            }
            if count >= MAX_VO_CONDITIONS {
                return None;
            }
            conditions[count] = c;
            count += 1;
        }
        Some(Self {
            sound,
            event_hash,
            conditions,
            condition_count: count as u8,
        })
    }

    /// Constructs a `VoRule` from an authored condition list, panicking if more than
    /// `MAX_VO_CONDITIONS` distinct non-zero conditions are provided.
    pub fn new(sound: usize, event_hash: u32, conds: &[u32]) -> Self {
        Self::try_new(sound, event_hash, conds).expect("VOSound rule exceeds MAX_VO_CONDITIONS")
    }

    /// Returns the active condition tag hashes for this rule.
    pub fn conditions(&self) -> &[u32] {
        &self.conditions[..usize::from(self.condition_count).min(MAX_VO_CONDITIONS)]
    }

    /// Returns true when the event matches and every authored condition is present in `active`.
    pub fn matches(&self, event_hash: u32, active: &[u32]) -> bool {
        self.event_hash == event_hash && self.conditions().iter().all(|c| active.contains(c))
    }
}

/// Cold per-class 2005 audio metadata loaded at battle admission.
#[derive(Clone, Debug)]
pub struct Swbf2Class {
    pub saber_turn_on: usize,
    pub saber_turn_off: usize,
    pub saber_on_hum: usize,
    pub saber_hit: usize,
    pub saber_deflect: usize,
    pub boost_cues: Vec<vehicles::BoostCue>,
    pub trick_sound: usize,
    pub flip_sound: usize,
    pub cockpit_1p: usize,
    pub cockpit_3p: usize,
    pub warm_up: usize,
    pub warm_up_time: f64,
    pub warm_up_start_pitch: f64,
    pub warm_up_fade_in: f64,
    pub warm_up_fade_out: f64,
    pub barrel: usize,
    pub barrel_fade_in: f64,
    pub barrel_fade_out: f64,
    pub building_sound: usize,
    pub damage_effect_sound: usize,
    pub door_open: usize,
    pub door_close: usize,
    pub door_locked: usize,
    pub soldier_music: usize,
    pub rep_music: usize,
    pub cis_music: usize,
    pub hero_spawned: usize,
    pub hero_defeated: usize,
    pub hero_killer: usize,
    pub vo_unit_type: u32,
    pub vo_rules: Vec<VoRule>,
    /// Linked ordnance class index resolved from `OrdnanceName` (`NONE` when absent).
    pub ordnance_class: usize,
    /// Linked explosion class index resolved from `ExplosionName` / `ExplosionDestruct` / `Explosion`.
    pub explosion_class: usize,
    /// Linked expiry explosion class index resolved from `ExplosionExpire`.
    pub expire_explosion_class: usize,
    /// Linked entity destruction explosion class index resolved from `ExplosionDestruct` / `ExplosionName`.
    pub destruct_explosion_class: usize,
}

impl Default for Swbf2Class {
    fn default() -> Self {
        Self {
            saber_turn_on: NONE,
            saber_turn_off: NONE,
            saber_on_hum: NONE,
            saber_hit: NONE,
            saber_deflect: NONE,
            boost_cues: Vec::new(),
            trick_sound: NONE,
            flip_sound: NONE,
            cockpit_1p: NONE,
            cockpit_3p: NONE,
            warm_up: NONE,
            warm_up_time: 0.5,
            warm_up_start_pitch: 0.5,
            warm_up_fade_in: 0.1,
            warm_up_fade_out: 0.1,
            barrel: NONE,
            barrel_fade_in: 0.05,
            barrel_fade_out: 0.1,
            building_sound: NONE,
            damage_effect_sound: NONE,
            door_open: NONE,
            door_close: NONE,
            door_locked: NONE,
            soldier_music: NONE,
            rep_music: NONE,
            cis_music: NONE,
            hero_spawned: NONE,
            hero_defeated: NONE,
            hero_killer: NONE,
            vo_unit_type: 0,
            vo_rules: Vec::new(),
            ordnance_class: NONE,
            explosion_class: NONE,
            expire_explosion_class: NONE,
            destruct_explosion_class: NONE,
        }
    }
}

/// Complete 2005 audio extension state carried on `Phase` (boxed, absent for 2004).
pub struct Swbf2State {
    pub active: bool,
    pub classes: Vec<Swbf2Class>,
    pub combos: Vec<ComboSoundSet>,
    /// Authored `Group` hash per sound index, for `scale_sound_parameter`.
    pub sound_groups: Vec<u32>,
    /// Lookup tables `(zero_hash(name), index)` for zero-allocation O(log N) resolution.
    class_hashes: Vec<(u32, usize)>,
    combo_hashes: Vec<(u32, usize)>,
    building_hashes: Vec<(u32, usize)>,
    lookups_sorted: bool,
    pub scoreboard_bus: usize,
    pub sabers: saber::SaberBank,
    pub powers: powers::Swbf2PowerBank,
    pub vehicles: vehicles::VehicleBank,
    pub weapons: weapons::Swbf2WeaponBank,
    pub ambience: ambience::AmbienceState,
    pub director: director::Swbf2Director,
}

impl Default for Swbf2State {
    fn default() -> Self {
        Self {
            active: false,
            classes: Vec::new(),
            combos: Vec::new(),
            sound_groups: Vec::new(),
            class_hashes: Vec::new(),
            combo_hashes: Vec::new(),
            building_hashes: Vec::new(),
            lookups_sorted: false,
            scoreboard_bus: NONE,
            sabers: saber::SaberBank::empty(),
            powers: powers::Swbf2PowerBank::default(),
            vehicles: vehicles::VehicleBank::empty(),
            weapons: weapons::Swbf2WeaponBank::default(),
            ambience: ambience::AmbienceState::default(),
            director: director::Swbf2Director::default(),
        }
    }
}

fn lookup_hash(table: &[(u32, usize)], sorted: bool, hash: u32) -> Option<usize> {
    if hash == 0 {
        return None;
    }
    if sorted {
        table.binary_search_by_key(&hash, |&(h, _)| h).ok().map(|i| table[i].1)
    } else {
        table.iter().find(|&&(h, _)| h == hash).map(|&(_, id)| id)
    }
}

impl Swbf2State {
    /// Preallocates all bounded runtime tables so steady-state ticks never allocate.
    pub fn new_active() -> Self {
        Self {
            active: true,
            classes: Vec::new(),
            combos: Vec::new(),
            sound_groups: Vec::new(),
            class_hashes: Vec::new(),
            combo_hashes: Vec::new(),
            building_hashes: Vec::new(),
            lookups_sorted: false,
            scoreboard_bus: NONE,
            sabers: saber::SaberBank::preallocated(RECORDS),
            powers: powers::Swbf2PowerBank::default(),
            vehicles: vehicles::VehicleBank::preallocated(RECORDS),
            weapons: weapons::Swbf2WeaponBank::default(),
            ambience: ambience::AmbienceState::default(),
            director: director::Swbf2Director::default(),
        }
    }

    pub fn register_class_hash(&mut self, class_hash: u32, class_id: usize) {
        if class_hash != 0 {
            self.class_hashes.push((class_hash, class_id));
            self.lookups_sorted = false;
        }
    }

    pub fn register_combo_hash(&mut self, combo_hash: u32, combo_id: usize) {
        if combo_hash != 0 {
            self.combo_hashes.push((combo_hash, combo_id));
            self.lookups_sorted = false;
        }
    }

    pub fn register_building_hash(&mut self, name_hash: u32, building_id: usize) {
        if name_hash != 0 {
            self.building_hashes.push((name_hash, building_id));
            self.lookups_sorted = false;
        }
    }

    /// Sorts all hash lookup tables once after load for O(log N) zero-allocation queries.
    pub fn finalize_lookups(&mut self) {
        self.class_hashes.sort_unstable_by_key(|&(h, _)| h);
        self.class_hashes.dedup_by_key(|entry| entry.0);
        self.combo_hashes.sort_unstable_by_key(|&(h, _)| h);
        self.combo_hashes.dedup_by_key(|entry| entry.0);
        self.building_hashes.sort_unstable_by_key(|&(h, _)| h);
        self.building_hashes.dedup_by_key(|entry| entry.0);
        self.lookups_sorted = true;
    }

    pub fn class_by_hash(&self, class_hash: u32) -> Option<usize> {
        lookup_hash(&self.class_hashes, self.lookups_sorted, class_hash)
    }

    pub fn combo_by_hash(&self, combo_hash: u32) -> Option<usize> {
        lookup_hash(&self.combo_hashes, self.lookups_sorted, combo_hash)
    }

    pub fn building_by_hash(&self, name_hash: u32) -> Option<usize> {
        lookup_hash(&self.building_hashes, self.lookups_sorted, name_hash)
    }
}

impl Phase {
    /// Constructs a control phase for the selected game family.
    /// `GameFamily::Swbf1` matches `Phase::new(seed)` identically.
    pub fn new_for_family(seed: u32, family: GameFamily) -> Self {
        let mut phase = Self::new(seed);
        if family == GameFamily::Swbf2 {
            phase.set_swbf2_family(Swbf2State::new_active());
            phase.streams.max_active = 6;
        }
        phase
    }

    pub fn swbf2_class_by_hash(&self, class_hash: u32) -> Option<usize> {
        self.swbf2()?.class_by_hash(class_hash)
    }

    pub fn swbf2_combo_by_hash(&self, combo_hash: u32) -> Option<usize> {
        self.swbf2()?.combo_by_hash(combo_hash)
    }

    pub fn swbf2_building_by_hash(&self, instance_hash: u32) -> Option<usize> {
        self.swbf2()?.building_by_hash(instance_hash)
    }

    pub fn swbf2_sound_by_hash(&self, sound_hash: u32) -> Option<usize> {
        if self.family() != GameFamily::Swbf2 || sound_hash == 0 {
            return None;
        }
        let id = self.named(sound_hash);
        if id == NONE { None } else { Some(id) }
    }

    /// Script-host API: sets the 2005 Doppler velocity scaling factor.
    pub fn set_doppler_factor(&mut self, factor: f64) {
        if self.family() != GameFamily::Swbf2 {
            return;
        }
        if factor.is_finite() && factor >= 0.0 {
            self.engine.doppler_factor = factor;
        }
    }

    /// Script-host API: scales a sound property across every sound belonging to `group_hash`.
    pub fn scale_sound_parameter(&mut self, group_hash: u32, param: SoundParam, scale: f64) {
        if self.family() != GameFamily::Swbf2 || !scale.is_finite() || scale < 0.0 {
            return;
        }
        let count = self.swbf2().map_or(0, |s| s.sound_groups.len());
        for id in 0..count {
            let group = self.swbf2().unwrap().sound_groups[id];
            if group != group_hash {
                continue;
            }
            if let Some(sound) = self.engine.sounds.get_mut(id) {
                match param {
                    SoundParam::Gain => sound.gain = (sound.gain * scale).clamp(0.0, 1.0),
                    SoundParam::Pitch => sound.pitch = (sound.pitch * scale).max(0.01),
                    SoundParam::MinDistance => sound.curve.min *= scale,
                    SoundParam::MaxDistance => sound.curve.max *= scale,
                    SoundParam::MuteDistance => sound.curve.mute *= scale,
                }
            }
        }
    }

    /// Script-host API: enables or disables a named `soundtrigger` world region (`zero_hash(name)`).
    pub fn trigger_sound_region_enable(&mut self, region_hash: u32, enabled: bool) {
        if let Some(swbf2) = self.swbf2_mut() {
            swbf2.ambience.set_trigger_enabled(region_hash, enabled);
        }
    }

    /// Script-host API: toggles hero `SoldierMusic` override playback.
    pub fn enable_hero_music(&mut self, enabled: bool) {
        if let Some(swbf2) = self.swbf2_mut() {
            swbf2.director.hero_music_enabled = enabled;
        }
    }

    /// Script-host API: toggles hero spawn, defeat and killer voice-over cues.
    pub fn enable_hero_vo(&mut self, enabled: bool) {
        if let Some(swbf2) = self.swbf2_mut() {
            swbf2.director.hero_vo_enabled = enabled;
        }
    }

    /// Script-host API: sets remaining score progress (`1.0` at start down to `0.0` at victory)
    /// for points-based modes whose ambient music stages use fraction thresholds.
    pub fn set_mode_score_progress(&mut self, team1_remaining: f64, team2_remaining: f64) {
        if let Some(swbf2) = self.swbf2_mut() {
            swbf2.director.mode_score_progress = Some([
                team1_remaining.clamp(0.0, 1.0),
                team2_remaining.clamp(0.0, 1.0),
            ]);
        }
    }

    /// Script-host API: queues a mission voice-over sound (`zero_hash` of sound name),
    /// optionally filtered to one team (`0` = all).
    pub fn broadcast_voice_over(&mut self, sound_hash: u32, team_filter: u8) {
        if self.family() != GameFamily::Swbf2 {
            return;
        }
        let sound = self.named(sound_hash);
        if sound != NONE {
            if let Some(swbf2) = self.swbf2_mut() {
                swbf2.director.push_broadcast(sound, team_filter);
            }
        }
    }

    /// Script-host API: plays a named music sound/track immediately on top of ambient stages.
    /// Unplayable requests are ignored without disturbing running music; repeated calls for the
    /// active request are idempotent.
    pub fn play_in_game_music(&mut self, sound_hash: u32) {
        if self.family() != GameFamily::Swbf2 {
            return;
        }
        let sound = self.named(sound_hash);
        let layout = if self.engine.enhanced { self.engine.channels } else { 0 };
        let d = &mut self.swbf2_mut().unwrap().director;
        d.script_input = Some(sound);
        if d.failed_script_input == Some((sound, layout)) || !self.streams.has_playable_segment(&self.engine, sound) {
            return;
        }
        if let Some(swbf2) = self.swbf2() {
            if swbf2.director.script_music_sound == Some(sound)
                && (!swbf2.director.script_music_started
                    || self.streams.playing(swbf2.director.script_music_voice))
            {
                return;
            }
        }
        let entries = &self.engine.sounds[sound].entries;
        if entries.iter().all(|entry| {
            (entry.interval != 0.0 || entry.dev != 0.0)
                && entries.iter().find(|other| other.id == entry.id).is_some_and(|gate| self.engine.time < gate.next)
        }) { return; }
        let old_voice = self.swbf2().map_or(SILENT, |s| s.director.script_music_voice);
        if old_voice != SILENT {
            self.streams.stop(&mut self.engine, old_voice, 0.25);
        }
        if let Some(swbf2) = self.swbf2_mut() {
            swbf2.director.script_music_sound = Some(sound);
            swbf2.director.script_music_voice = SILENT;
            swbf2.director.script_music_started = false;
        }
    }

    /// Script-host API: fades out any active script-requested music override.
    pub fn stop_in_game_music(&mut self, fade_s: f64) {
        if self.family() != GameFamily::Swbf2 {
            return;
        }
        let voice = self.swbf2().map_or(SILENT, |s| s.director.script_music_voice);
        if let Some(swbf2) = self.swbf2_mut() {
            swbf2.director.failed_script_input = None;
            swbf2.director.script_music_sound = None;
            swbf2.director.script_music_voice = SILENT;
            swbf2.director.script_music_started = false;
        }
        if voice != SILENT {
            self.streams.stop(&mut self.engine, voice, fade_s.max(0.0));
        }
    }

    /// Script-host API: registers a low-reinforcements warning line heard by `listener_team`
    /// when `watched_team`'s reinforcements fall to or below `threshold`.
    pub fn set_low_reinforcements_vo(
        &mut self,
        watched_team: usize,
        threshold: f64,
        sound_hash: u32,
        is_fraction: bool,
        listener_team: usize,
    ) {
        if self.family() != GameFamily::Swbf2
            || !(1..=2).contains(&watched_team)
            || !(1..=2).contains(&listener_team)
            || !threshold.is_finite()
        {
            return;
        }
        let sound = self.named(sound_hash);
        if sound != NONE {
            if let Some(swbf2) = self.swbf2_mut() {
                swbf2.director.low_reinforcements.push(director::LowReinforcementsRule {
                    watched_team,
                    listener_team,
                    threshold,
                    sound,
                    is_fraction,
                    fired: false,
                });
            }
        }
    }

    /// Script-host / combat API: queues a hero spawn, defeat, or killer announcement
    /// for `hero_class_hash` (`zero_hash` of the hero ODF class name).
    pub fn notify_hero_event(&mut self, hero_class_hash: u32, kind: director::HeroVoKind) {
        if let Some(swbf2) = self.swbf2_mut() {
            if let Some(hero_class) = swbf2.class_by_hash(hero_class_hash) {
                swbf2.director.push_hero_vo(hero_class, kind);
            }
        }
    }

    /// Plays a tagged `VOSound` cue for `class_hash` (`zero_hash` of ODF class name)
    /// matching `event_hash` and every authored rule condition against `conditions`.
    /// The highest matching tag count wins. Authored order on an equal score is provisional.
    pub fn play_vo_tag(
        &mut self,
        class_hash: u32,
        event_hash: u32,
        conditions: &[u32],
        position: Option<Point>,
        positional: bool,
    ) -> bool {
        let Some(swbf2) = self.swbf2() else {
            return false;
        };
        let Some(class) = swbf2.class_by_hash(class_hash) else {
            return false;
        };
        let Some(cls) = swbf2.classes.get(class) else {
            return false;
        };
        let mut best: Option<VoRule> = None;
        for &rule in &cls.vo_rules {
            if rule.matches(event_hash, conditions)
                && best.is_none_or(|b| rule.condition_count > b.condition_count)
            {
                best = Some(rule);
            }
        }
        let Some(found) = best.map(|r| r.sound).filter(|&s| s != NONE) else {
            return false;
        };
        self.swbf2_queue_vo(found, position, positional)
    }

    /// Melee interface: queues a lightsaber sound event (zero allocation).
    pub fn push_saber_event(&mut self, event: saber::SaberEvent) {
        if let Some(swbf2) = self.swbf2_mut() {
            swbf2.sabers.push(event);
        }
    }

    /// Vehicle interface: queues a starfighter trick or flip sound event (zero allocation).
    pub fn push_vehicle_event(&mut self, event: vehicles::VehicleEvent) {
        if let Some(swbf2) = self.swbf2_mut() {
            swbf2.vehicles.push_vehicle_event(event);
        }
    }

    /// World interface: queues a door open, close, or locked sound event (zero allocation).
    pub fn push_door_event(&mut self, event: vehicles::DoorEvent) {
        if let Some(swbf2) = self.swbf2_mut() {
            swbf2.vehicles.push_door_event(event);
        }
    }

    /// Chaingun interface: updates a weapon's spin-up and barrel loop state (zero allocation).
    /// Must be called on every tick for every active chaingun; omitting a weapon stops its loops.
    pub fn update_chaingun(&mut self, input: vehicles::ChaingunInput) {
        if let Some(swbf2) = self.swbf2_mut() {
            swbf2.vehicles.push_chaingun(input);
        }
    }

    /// Sets the local player's mounted vehicle class (`zero_hash` of vehicle ODF class name)
    /// and first-person state for cockpit audio and vehicle priority music.
    pub fn set_own_mount_swbf2(&mut self, vehicle_class_hash: Option<u32>, first_person: bool) {
        if let Some(swbf2) = self.swbf2_mut() {
            let class = vehicle_class_hash.and_then(|h| swbf2.class_by_hash(h));
            swbf2.vehicles.own_vehicle_class = class;
            swbf2.vehicles.own_first_person = first_person;
            swbf2.director.own_vehicle_class = class;
        }
    }

    /// Sets the local player's hero class (`zero_hash` of hero ODF class name) for
    /// `SoldierMusic` priority music selection.
    pub fn set_own_hero_swbf2(&mut self, hero_class_hash: Option<u32>) {
        if let Some(swbf2) = self.swbf2_mut() {
            swbf2.director.own_hero_class = hero_class_hash.and_then(|h| swbf2.class_by_hash(h));
        }
    }

    /// Updates the alive state of a placed building/subsystem by `zero_hash(instance_name)`.
    /// Setting `alive = false` stops any persistent `BuildingSound` and fires `DamageEffectSound`.
    pub fn set_building_alive_swbf2(&mut self, instance_name_hash: u32, alive: bool) {
        if let Some(swbf2) = self.swbf2_mut() {
            if let Some(idx) = swbf2.building_by_hash(instance_name_hash) {
                if let Some(b) = swbf2.vehicles.buildings.get_mut(idx) {
                    b.alive = alive;
                }
            }
        }
    }

    /// Complete 2005 per-frame step. Mirrors the shared engine pipeline and adds
    /// 2005 lightsabers, starfighters/chainguns/buildings/doors, spaces/triggers,
    /// `MaxPlaybackTime`/priority music, and logical-group VO queueing.
    pub(crate) fn step_swbf2(&mut self, collision: &mut impl Collision, events: &[[f64; ev::STRIDE]]) {
        if self.output_configuration != self.frame[f::CONFIGURATION] {
            self.output_configuration = self.frame[f::CONFIGURATION];
            for v in &mut self.engine.voices {
                v.sent = [f32::NAN; 5];
            }
            self.streams.invalidate_placement();
        }
        self.engine.wall_now = self.frame[f::WALL_NOW];
        self.engine.channels = self.frame[f::CHANNELS] as usize;
        self.engine.now = self.frame[f::NOW];
        self.engine.sample_rate = self.frame[f::RATE];
        let enhanced = self.frame[f::ENHANCED] != 0.0;
        if enhanced != self.engine.enhanced || !self.started {
            self.room_pending = if enhanced { 0 } else { self.original_room_mask };
        }
        self.engine.enhanced = enhanced;
        self.engine.light = self.frame[f::LIGHT] != 0.0;
        self.engine.hrtf = self.frame[f::HRTF] != 0.0;
        self.engine.binaural = self.frame[f::BINAURAL] != 0.0;
        self.engine.lfe = self.frame[f::LFE] != 0.0;
        self.engine.running = self.frame[f::RUNNING] != 0.0;
        self.engine.max_voices = (self.frame[f::MAX_VOICES] as usize).min(32);
        self.engine.virtual_voices = (self.frame[f::VIRTUAL_VOICES] as usize).min(64);
        for category in 0..8 {
            self.engine.volumes[category] = self.frame[match category {
                AMBIENCE => f::AMBIENCE_VOLUME,
                crate::engine::MUSIC => f::MUSIC_VOLUME,
                VOICE => f::VOICE_VOLUME,
                crate::engine::INTERFACE => f::INTERFACE_VOLUME,
                _ => f::EFFECTS_VOLUME,
            }];
        }
        self.engine.reclaim();
        self.diagnostic.update(&mut self.engine, &mut self.streams);
        for event in events {
            self.event(event);
        }
        if !self.started {
            return;
        }
        self.engine.listener.set(
            [self.frame[f::PX], self.frame[f::PY], self.frame[f::PZ]],
            [self.frame[f::FX], self.frame[f::FY], self.frame[f::FZ]],
            [self.frame[f::UX], self.frame[f::UY], self.frame[f::UZ]],
        );
        let dt = self.frame[f::DT];
        let game_dt = if self.paused { 0.0 } else { dt };
        let own = index(self.frame[f::OWN]);
        let at = [self.frame[f::PX], self.frame[f::PY], self.frame[f::PZ]];
        let (last, velocity) = &mut self.listener_motion;
        match *last {
            Some(p) if game_dt > 0.0 && crate::length(crate::sub(at, p)) < 20.0 => {
                let k = (game_dt / 0.1).min(1.0);
                for axis in 0..3 {
                    velocity[axis] += ((at[axis] - p[axis]) / game_dt - velocity[axis]) * k;
                }
            }
            Some(_) if game_dt == 0.0 => {}
            _ => *velocity = [0.0; 3],
        }
        *last = Some(at);

        self.player(own, game_dt, collision);
        self.soldiers(own);
        self.scene(own, game_dt);
        self.vehicles(game_dt);
        self.swbf2_vehicles(own, game_dt);
        self.swbf2_weapons(game_dt);
        self.swbf2_sabers(game_dt);
        self.swbf2_powers(game_dt);
        self.swbf2_acoustics(game_dt, collision);
        self.swbf2_ambience(game_dt);
        self.swbf2_music(game_dt);
        self.swbf2_voice_over(game_dt);

        let mut busy = [false; 512];
        assert!(self.engine.buses.len() <= busy.len());
        for (bus, value) in busy[..self.engine.buses.len()].iter_mut().enumerate() {
            *value = self.streams.busy(bus, &self.engine);
        }
        self.swbf2_render_controls(dt, &busy);
        self.streams.update(&mut self.engine, &self.acoustics, self.space_send);
        self.swbf2_drain_logical_queues();
        self.swbf2_streams_ended();
    }

    /// Render control tail for 2005 with space-to-space attenuation applied on top of acoustics.
    fn swbf2_render_controls(&mut self, dt: f64, stream_busy: &[bool]) {
        self.engine.time += dt;
        self.engine.ducks(dt, stream_busy);
        let mut cursor = 0;
        let mut live = [(0u64, 0u16); CAPACITY];
        let mut count = 0;
        for (id, v) in self.engine.voices.iter().enumerate() {
            if !v.done {
                live[count] = (v.serial, id as u16);
                count += 1;
            }
        }
        live[..count].sort_unstable();
        let mut next = 0;
        loop {
            while next < count {
                let (serial, id) = live[next];
                let v = &self.engine.voices[id as usize];
                if !v.done && v.serial == serial && serial > cursor {
                    break;
                }
                next += 1;
            }
            let voice = if next < count {
                Some((live[next].1 as usize, live[next].0))
            } else {
                self.engine
                    .voices
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| !v.done && v.serial > cursor)
                    .min_by_key(|(_, v)| v.serial)
                    .map(|(id, v)| (id, v.serial))
            };
            let group = self.layers.next_after(cursor);
            if let Some((id, serial)) = group.filter(|(_, serial)| voice.is_none_or(|(_, vs)| *serial < vs)) {
                cursor = serial;
                let handle = self.layers.handle(id);
                for layer in 0..self.layers.count(handle) {
                    if !self.layers.due(id, layer) {
                        continue;
                    }
                    let child = self.play(self.layers.child_ask(handle, layer));
                    let (gain, pitch) = self.layers.scales(handle);
                    if pitch != 0.0 {
                        self.pitch(child, pitch, false);
                    }
                    if gain != 0.0 {
                        self.gain(child, gain);
                    }
                    self.layers.set_child(handle, layer, child);
                }
                self.layers.advance(id, dt);
                if self.layers.all_launched(handle)
                    && (0..self.layers.count(handle)).all(|i| !self.playing(self.layers.child(handle, i)))
                {
                    self.layers.retire(handle);
                }
            } else if let Some((id, serial)) = voice {
                cursor = serial;
                let emitter = self.engine.voices[id].emitter;
                let followed = self.position(emitter);
                let motion = Some((self.velocity(emitter), self.listener_motion.1));
                self.engine.measure(id, dt, followed, self.engine.now, motion);
            } else {
                break;
            }
        }
        let mut ranked = [0; CAPACITY];
        let count = self.engine.rank(&mut ranked);
        let mut real = 0;
        for &id in &ranked[..count] {
            let want = self.engine.voices[id].score > 0.0 && real < self.engine.max_voices;
            if want {
                real += 1;
            }
            self.engine.set_real(id, want);
        }
        let listener_space = self.swbf2().and_then(|s| s.ambience.current_space);
        for i in 0..self.engine.order_len() {
            let id = self.engine.order_at(i);
            let v = &self.engine.voices[id];
            let mut shape = if self.engine.enhanced {
                self.acoustics.shape(v.category, v.mode3d, v.distance, v.occluded.unwrap_or(0.0))
            } else {
                Shape { send: self.space_send, ..Shape::default() }
            };
            if v.mode3d {
                if let Some(pos) = v.position {
                    let space_gain = self
                        .swbf2()
                        .map_or(1.0, |s| s.ambience.voice_space_gain(listener_space, pos));
                    shape.gain *= space_gain;
                }
            }
            self.engine.apply(id, shape);
        }
        self.engine.compact();
        for i in 0..crate::layers::GROUPS {
            let handle = self.layers.handle(i);
            if self.layers.is_active(handle) && !self.playing(handle) {
                self.layers.retire(handle);
            }
        }
    }

    pub(crate) fn swbf2_loop_has_room(&self, sound: usize, protected: bool) -> bool {
        let Some(s) = self.engine.sounds.get(sound) else {
            return false;
        };
        match s.kind {
            1 => self.engine.has_virtual_room(),
            2 => {
                let max_groups = if protected { crate::layers::GROUPS } else { 64 };
                self.layers.has_group_room(false, max_groups) && self.engine.has_virtual_room()
            }
            3 => self.layers.has_group_room(true, 64) && self.engine.has_virtual_room(),
            4 => self.streams.free(s.stream),
            _ => false,
        }
    }

    pub(crate) fn swbf2_loop_recovery_room(
        &self,
        sound: usize,
        emitter: usize,
        position: Option<Point>,
        protected: bool,
    ) -> bool {
        if self.swbf2_loop_has_room(sound, protected) {
            return true;
        }
        let Some(s) = self.engine.sounds.get(sound) else {
            return false;
        };
        if s.kind == 1 && !s.entries.is_empty() {
            if let Some(low) = self.engine.swbf2_lowest_unprotected_score() {
                if low <= 0.0 {
                    let mut ask = Ask::new(sound, WEAPONS);
                    ask.emitter = emitter;
                    ask.position = position;
                    ask.looping = Some(true);
                    return self.engine.swbf2_estimate_score(ask, self.position(emitter)) > 0.0;
                }
            }
        }
        false
    }

    pub(crate) fn swbf2_loop_can_win(
        &self,
        sound: usize,
        emitter: usize,
        position: Option<Point>,
        protected: bool,
    ) -> bool {
        let Some(s) = self.engine.sounds.get(sound) else {
            return false;
        };
        match s.kind {
            1 => {
                if s.entries.is_empty() {
                    return false;
                }
                if protected {
                    return true;
                }
                let Some(lowest) = self.engine.swbf2_lowest_unprotected_score() else {
                    return true;
                };
                let mut ask = Ask::new(sound, WEAPONS);
                ask.emitter = emitter;
                ask.position = position;
                ask.looping = Some(true);
                self.engine.swbf2_estimate_score(ask, self.position(emitter)) > lowest
            }
            2 | 3 => {
                let param = s.kind == 3;
                let max_groups = if !param && protected { crate::layers::GROUPS } else { 64 };
                if !self.layers.has_group_room(param, max_groups) {
                    return false;
                }
                if protected {
                    return true;
                }
                let Some(lowest) = self.engine.swbf2_lowest_unprotected_score() else {
                    return true;
                };
                let Some(def) = self.layers.definitions.get(sound) else {
                    return false;
                };
                let followed = self.position(emitter);
                def.layers.iter().any(|layer| {
                    let mut ask = Ask::new(layer.sound, WEAPONS);
                    ask.emitter = emitter;
                    ask.position = position;
                    ask.looping = Some(true);
                    self.engine.swbf2_estimate_score(ask, followed) > lowest
                })
            }
            4 => self.streams.free(s.stream) && !s.entries.is_empty(),
            _ => false,
        }
    }

    /// Returns `true` when a refused or stolen 2005 loop should re-request playback:
    /// immediately when free or silent pool room opens, or on the bounded retry cadence
    /// when the candidate outranks the lowest active voice. Outscored loops reset their
    /// timer without calling `Engine::play`, avoiding wasted `dropped` churn.
    pub(crate) fn swbf2_loop_should_retry(
        &self,
        retry: &mut LoopRetry,
        dt: f64,
        sound: usize,
        emitter: usize,
        position: Option<Point>,
        protected: bool,
    ) -> bool {
        let has_room = self.swbf2_loop_recovery_room(sound, emitter, position, protected);
        if !retry.due(dt, has_room) {
            return false;
        }
        if self.swbf2_loop_can_win(sound, emitter, position, protected) {
            true
        } else {
            retry.record(false, has_room);
            false
        }
    }

    pub(crate) fn play_sound_at(&mut self, sound: usize, category: usize, emitter: usize, position: Option<Point>, looping: Option<bool>) -> u64 {
        self.play_sound_protected(sound, category, emitter, position, looping, false)
    }

    pub(crate) fn play_sound_protected(
        &mut self,
        sound: usize,
        category: usize,
        emitter: usize,
        position: Option<Point>,
        looping: Option<bool>,
        protected: bool,
    ) -> u64 {
        if sound == NONE {
            return SILENT;
        }
        let mut ask = Ask::new(sound, category);
        ask.emitter = emitter;
        ask.position = position;
        ask.looping = looping;
        ask.protected = protected;
        self.play(ask)
    }
}

/// Helper to read a stream's active count across a logical group without modifying 2004 `Streams`.
pub(crate) fn stream_group_of(groups: &[usize], stream: usize) -> usize {
    groups.get(stream).copied().unwrap_or(stream)
}

/// Helper to query sound kind safely.
pub(crate) fn is_stream_sound(engine: &Engine, sound: usize) -> bool {
    engine.sounds.get(sound).is_some_and(|s| s.kind == 4)
}

/// Retry interval (seconds) for a refused or stolen 2005 loop while the pool is
/// saturated. A pool-blocked loop recovers immediately when free capacity opens.
pub(crate) const SWBF2_LOOP_RETRY_S: f64 = 0.10;

/// Bounded per-slot recovery state for 2005 looping voice controllers.
#[derive(Clone, Copy, Debug, Default)]
pub struct LoopRetry {
    cooldown: f64,
    pool_blocked: bool,
}

impl LoopRetry {
    pub(crate) fn reset(&mut self) {
        self.cooldown = 0.0;
        self.pool_blocked = false;
    }

    /// Marks the loop as currently playing so a later steal can recover.
    pub(crate) fn note_live(&mut self) {
        self.cooldown = SWBF2_LOOP_RETRY_S;
        self.pool_blocked = true;
    }

    /// Returns `true` when a missing wanted loop may retry: immediately when a
    /// pool-blocked loop sees free room, or once per `SWBF2_LOOP_RETRY_S` under
    /// saturation (never every tick).
    pub(crate) fn due(&mut self, dt: f64, has_room: bool) -> bool {
        if dt <= 0.0 {
            return false;
        }
        if self.pool_blocked && has_room {
            return true;
        }
        self.cooldown -= dt;
        self.cooldown <= 0.0
    }

    /// Records the outcome of an initial or recovery play attempt.
    pub(crate) fn record(&mut self, live: bool, had_room: bool) {
        self.cooldown = SWBF2_LOOP_RETRY_S;
        self.pool_blocked = live || !had_room;
    }
}

pub(crate) const SABER_CATEGORY: usize = WEAPONS;
pub(crate) const VEHICLE_FX_CATEGORY: usize = FOLEY;
