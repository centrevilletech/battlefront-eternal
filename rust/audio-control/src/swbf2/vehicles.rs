//! 2005 vehicle, starfighter, chaingun, ambient building, and door sound controllers.
//! Covers `BoostSound` threshold crossings, `Proximity` parameterized layer modulation,
//! `TrickSound`/`FlipSound` maneuvers, cockpit loops, chaingun `WarmUpSound`/`BarrelSound`,
//! persistent `BuildingSound` + `DamageEffectSound`, and doors (`Open`/`Close`/`Locked`).

use super::{is_stream_sound, LoopRetry, VEHICLE_FX_CATEGORY};
use crate::engine::{EXPLOSIONS, NONE, SILENT, VOICE, WEAPONS};
use crate::layers::{graph, Layers};
use crate::layout::emitter as e;
use crate::phase::{index, Phase};
use crate::streams::Options;
use crate::{length, sub, Point};

pub const MAX_VEHICLE_EVENTS: usize = 64;
pub const MAX_DOOR_EVENTS: usize = 64;
pub const MAX_CHAINGUNS: usize = 64;

/// A speed-fraction threshold crossing one-shot (`BoostSound`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoostCue {
    pub sound: usize,
    pub threshold: f64,
    /// `true` fires when rising past `threshold`; `false` when falling past it.
    pub up: bool,
}

/// A `Proximity` parameter graph binding on one layer of a parameterized sound.
#[derive(Clone, Debug)]
pub struct ProximityBinding {
    pub sound: usize,
    pub layer: usize,
    pub pitch: bool,
    pub min_dist: f64,
    pub max_dist: f64,
    pub points: Vec<[f64; 2]>,
}

/// Explicit starfighter maneuver sound events pushed by flight code.
/// `class_hash` is `zero_hash` of the vehicle ODF class name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VehicleEvent {
    Trick { owner: usize, class_hash: u32 },
    Flip { owner: usize, class_hash: u32 },
}

/// Explicit door audio events pushed by world prop logic.
/// `class_hash` is `zero_hash` of the door ODF class name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DoorEvent {
    Open { class_hash: u32, position: Point },
    Close { class_hash: u32, position: Point },
    Locked { class_hash: u32, position: Point },
}

/// Per-frame chaingun spin-up and firing input.
/// `class_hash` is `zero_hash` of the weapon ODF class name.
/// Callers must push an entry every tick while a chaingun is active; omitting an
/// active weapon on a tick fades out and stops its warm-up and barrel loops.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChaingunInput {
    pub weapon: usize,
    pub owner: usize,
    pub class_hash: u32,
    pub spinning: bool,
    pub firing: bool,
}

/// A placed ship or structure prop with persistent `BuildingSound` and/or `DamageEffectSound`.
#[derive(Clone, Copy, Debug)]
pub struct BuildingEmitter {
    /// `zero_hash` of the placed object's instance name (0 when unnamed).
    pub name_hash: u32,
    pub class: usize,
    pub position: Point,
    pub alive: bool,
    pub damaged: bool,
    pub in_range: bool,
    pub voice: u64,
    pub retry: LoopRetry,
}

#[derive(Clone, Copy)]
struct VehicleTrack {
    initialized: bool,
    generation: f64,
    speed_frac: f64,
}

impl Default for VehicleTrack {
    fn default() -> Self {
        Self {
            initialized: false,
            generation: -1.0,
            speed_frac: 0.0,
        }
    }
}

#[derive(Clone, Copy)]
struct ChaingunState {
    weapon: usize,
    owner: usize,
    class: usize,
    warm_voice: u64,
    barrel_voice: u64,
    elapsed: f64,
    barrel_elapsed: f64,
    warm_retry: LoopRetry,
    barrel_retry: LoopRetry,
    spinning: bool,
    firing: bool,
    seen: bool,
}

impl Default for ChaingunState {
    fn default() -> Self {
        Self {
            weapon: NONE,
            owner: NONE,
            class: NONE,
            warm_voice: SILENT,
            barrel_voice: SILENT,
            elapsed: 0.0,
            barrel_elapsed: 0.0,
            warm_retry: LoopRetry::default(),
            barrel_retry: LoopRetry::default(),
            spinning: false,
            firing: false,
            seen: false,
        }
    }
}

pub struct VehicleBank {
    tracks: Vec<VehicleTrack>,
    pub proximity_bindings: Vec<ProximityBinding>,
    pub buildings: Vec<BuildingEmitter>,
    pub own_vehicle_class: Option<usize>,
    pub own_first_person: bool,
    cockpit_sound: usize,
    cockpit_voice: u64,
    cockpit_is_stream: bool,
    cockpit_retry: LoopRetry,
    vehicle_events: [Option<VehicleEvent>; MAX_VEHICLE_EVENTS],
    vehicle_events_len: usize,
    door_events: [Option<DoorEvent>; MAX_DOOR_EVENTS],
    door_events_len: usize,
    chaingun_inputs: [Option<ChaingunInput>; MAX_CHAINGUNS],
    chaingun_inputs_len: usize,
    chainguns: [ChaingunState; MAX_CHAINGUNS],
}

impl VehicleBank {
    pub fn empty() -> Self {
        Self {
            tracks: Vec::new(),
            proximity_bindings: Vec::new(),
            buildings: Vec::new(),
            own_vehicle_class: None,
            own_first_person: true,
            cockpit_sound: NONE,
            cockpit_voice: SILENT,
            cockpit_is_stream: false,
            cockpit_retry: LoopRetry::default(),
            vehicle_events: [None; MAX_VEHICLE_EVENTS],
            vehicle_events_len: 0,
            door_events: [None; MAX_DOOR_EVENTS],
            door_events_len: 0,
            chaingun_inputs: [None; MAX_CHAINGUNS],
            chaingun_inputs_len: 0,
            chainguns: [ChaingunState::default(); MAX_CHAINGUNS],
        }
    }

    pub fn preallocated(records: usize) -> Self {
        Self {
            tracks: vec![VehicleTrack::default(); records],
            ..Self::empty()
        }
    }

    pub fn push_vehicle_event(&mut self, event: VehicleEvent) {
        if self.vehicle_events_len < MAX_VEHICLE_EVENTS {
            self.vehicle_events[self.vehicle_events_len] = Some(event);
            self.vehicle_events_len += 1;
        }
    }

    pub fn push_door_event(&mut self, event: DoorEvent) {
        if self.door_events_len < MAX_DOOR_EVENTS {
            self.door_events[self.door_events_len] = Some(event);
            self.door_events_len += 1;
        }
    }

    pub fn push_chaingun(&mut self, input: ChaingunInput) {
        if self.chaingun_inputs_len < MAX_CHAINGUNS {
            self.chaingun_inputs[self.chaingun_inputs_len] = Some(input);
            self.chaingun_inputs_len += 1;
        }
    }
}

/// Normalized proximity input: `1.0` inside `min_dist`, falling linearly to `0.0` at `max_dist`.
pub fn proximity_value(distance: f64, min_dist: f64, max_dist: f64) -> f64 {
    let span = (max_dist - min_dist).max(0.001);
    1.0 - ((distance - min_dist) / span).clamp(0.0, 1.0)
}

impl Phase {
    pub(crate) fn swbf2_vehicles(&mut self, _own: usize, dt: f64) {
        // 1. Starfighter BoostSound threshold crossings & Proximity layer modulation.
        let limit = self
            .entity_count
            .min(self.swbf2().map_or(0, |s| s.vehicles.tracks.len()));
        let ear = self.engine.listener.ear;
        for id in 0..limit {
            let row = self.emitters[id];
            if row[e::KIND] != 3.0 {
                continue;
            }
            let class = index(row[e::CLASS]);
            let mut tr = self.swbf2().unwrap().vehicles.tracks[id];
            let speed = length([row[e::VX], row[e::VY], row[e::VZ]]);
            let speed_limit = if row[e::SPEED_LIMIT] > 0.0 { row[e::SPEED_LIMIT] } else { 1.0 };
            let frac = speed / speed_limit;

            if !tr.initialized || tr.generation != row[e::GENERATION] || row[e::ALIVE] == 0.0 {
                tr = VehicleTrack {
                    initialized: true,
                    generation: row[e::GENERATION],
                    speed_frac: frac,
                };
                if let Some(swbf2) = self.swbf2_mut() {
                    swbf2.vehicles.tracks[id] = tr;
                }
                continue;
            }

            let state = self.entity_state[id];
            if dt > 0.0 && state.on && !state.turning_off {
                let mut cues = [NONE; 12];
                let mut cue_count = 0;
                if let Some(cls) = self.swbf2().and_then(|s| s.classes.get(class)) {
                    for cue in &cls.boost_cues {
                        let crossed = if cue.up {
                            tr.speed_frac < cue.threshold && frac >= cue.threshold
                        } else {
                            tr.speed_frac > cue.threshold && frac <= cue.threshold
                        };
                        if crossed && cue_count < cues.len() {
                            cues[cue_count] = cue.sound;
                            cue_count += 1;
                        }
                    }
                }
                for &sound in &cues[..cue_count] {
                    self.play_sound_at(sound, VEHICLE_FX_CATEGORY, id, None, Some(false));
                }

                // Apply any Proximity parameter graphs on this vehicle's engine layers.
                let engine_sound = self.sound(class, crate::layout::class::ENGINE);
                if engine_sound != NONE && Layers::is_group(state.primary) {
                    let dist = length(sub([row[e::X], row[e::Y], row[e::Z]], ear));
                    let layer_count = self.layers.count(state.primary);
                    let mut mods = [(NONE, false, 1.0f64); 16];
                    let mut mod_count = 0;
                    if let Some(swbf2) = self.swbf2() {
                        for b in &swbf2.vehicles.proximity_bindings {
                            if b.sound != engine_sound || b.layer >= layer_count || mod_count >= mods.len() {
                                continue;
                            }
                            let x = proximity_value(dist, b.min_dist, b.max_dist);
                            let y = graph(&b.points, x);
                            mods[mod_count] = (b.layer, b.pitch, y);
                            mod_count += 1;
                        }
                    }
                    for &(layer_idx, is_pitch, val) in &mods[..mod_count] {
                        let child = self.layers.child(state.primary, layer_idx);
                        if child != SILENT {
                            if is_pitch {
                                self.pitch(child, val, false);
                            } else {
                                self.gain(child, val);
                            }
                        }
                    }
                }
            }

            tr.speed_frac = frac;
            if let Some(swbf2) = self.swbf2_mut() {
                swbf2.vehicles.tracks[id] = tr;
            }
        }

        // 2. Explicit starfighter maneuver events (TrickSound / FlipSound).
        let mut v_events = [None; MAX_VEHICLE_EVENTS];
        let v_count = {
            let Some(swbf2) = self.swbf2_mut() else { return };
            let n = swbf2.vehicles.vehicle_events_len;
            for (dst, src) in v_events[..n]
                .iter_mut()
                .zip(&mut swbf2.vehicles.vehicle_events[..n])
            {
                *dst = src.take();
            }
            swbf2.vehicles.vehicle_events_len = 0;
            n
        };
        for ev in v_events[..v_count].iter().flatten().copied() {
            match ev {
                VehicleEvent::Trick { owner, class_hash } => {
                    let sound = self
                        .swbf2()
                        .and_then(|s| s.class_by_hash(class_hash).and_then(|c| s.classes.get(c)))
                        .map_or(NONE, |c| c.trick_sound);
                    self.play_sound_at(sound, VEHICLE_FX_CATEGORY, owner, None, Some(false));
                }
                VehicleEvent::Flip { owner, class_hash } => {
                    let sound = self
                        .swbf2()
                        .and_then(|s| s.class_by_hash(class_hash).and_then(|c| s.classes.get(c)))
                        .map_or(NONE, |c| c.flip_sound);
                    self.play_sound_at(sound, VEHICLE_FX_CATEGORY, owner, None, Some(false));
                }
            }
        }

        // 3. Cockpit 1st / 3rd person ambient chatter loop for mounted local player (VOICE category).
        let (want_cockpit, old_sound, old_voice, old_is_stream, mut cockpit_retry) = {
            let Some(swbf2) = self.swbf2() else { return };
            let want = swbf2.vehicles.own_vehicle_class.map_or(NONE, |class| {
                swbf2.classes.get(class).map_or(NONE, |c| {
                    if swbf2.vehicles.own_first_person {
                        if c.cockpit_1p != NONE { c.cockpit_1p } else { c.cockpit_3p }
                    } else {
                        c.cockpit_3p
                    }
                })
            });
            (
                want,
                swbf2.vehicles.cockpit_sound,
                swbf2.vehicles.cockpit_voice,
                swbf2.vehicles.cockpit_is_stream,
                swbf2.vehicles.cockpit_retry,
            )
        };
        if want_cockpit != old_sound {
            if old_voice != SILENT {
                if old_is_stream {
                    self.streams.stop(&mut self.engine, old_voice, 0.2);
                } else {
                    self.stop(old_voice, 0.2);
                }
            }
            cockpit_retry.reset();
            let (new_voice, new_is_stream) = if want_cockpit != NONE {
                let had_room = self.swbf2_loop_has_room(want_cockpit, false);
                let (v, is_stream) = if is_stream_sound(&self.engine, want_cockpit) {
                    let mut opts = Options::new(want_cockpit, VOICE);
                    opts.ask.looping = Some(true);
                    opts.fade = 0.25;
                    (self.streams.play(&mut self.engine, opts), true)
                } else {
                    (self.play_sound_at(want_cockpit, VOICE, NONE, None, Some(true)), false)
                };
                let live = if is_stream { self.streams.playing(v) } else { self.playing(v) };
                cockpit_retry.record(live, had_room);
                (if live { v } else { SILENT }, is_stream)
            } else {
                (SILENT, false)
            };
            if let Some(swbf2) = self.swbf2_mut() {
                swbf2.vehicles.cockpit_voice = new_voice;
                swbf2.vehicles.cockpit_sound = want_cockpit;
                swbf2.vehicles.cockpit_is_stream = new_is_stream;
                swbf2.vehicles.cockpit_retry = cockpit_retry;
            }
        } else if want_cockpit != NONE {
            let live = if old_is_stream {
                self.streams.playing(old_voice)
            } else {
                self.playing(old_voice)
            };
            if live {
                cockpit_retry.note_live();
                if let Some(swbf2) = self.swbf2_mut() {
                    swbf2.vehicles.cockpit_retry = cockpit_retry;
                }
            } else {
                let (new_voice, new_is_stream) = if self.swbf2_loop_should_retry(&mut cockpit_retry, dt, want_cockpit, NONE, None, false) {
                    let had_room = self.swbf2_loop_has_room(want_cockpit, false);
                    let (v, is_stream) = if is_stream_sound(&self.engine, want_cockpit) {
                        let mut opts = Options::new(want_cockpit, VOICE);
                        opts.ask.looping = Some(true);
                        opts.fade = 0.25;
                        (self.streams.play(&mut self.engine, opts), true)
                    } else {
                        (self.play_sound_at(want_cockpit, VOICE, NONE, None, Some(true)), false)
                    };
                    let now_live = if is_stream { self.streams.playing(v) } else { self.playing(v) };
                    cockpit_retry.record(now_live, had_room);
                    (if now_live { v } else { SILENT }, is_stream)
                } else {
                    (SILENT, old_is_stream)
                };
                if let Some(swbf2) = self.swbf2_mut() {
                    swbf2.vehicles.cockpit_voice = new_voice;
                    swbf2.vehicles.cockpit_is_stream = new_is_stream;
                    swbf2.vehicles.cockpit_retry = cockpit_retry;
                }
            }
        }

        // 4. Chaingun WarmUpSound & BarrelSound controllers.
        self.swbf2_chainguns(dt);

        // 5. Persistent ambient buildings (`BuildingSound` / `DamageEffectSound`).
        // `DamageEffectSound` fires on transition to destroyed even when no ambient
        // `BuildingSound` loop was playing (for classes without a loop or out of earshot).
        let b_count = self.swbf2().map_or(0, |s| s.vehicles.buildings.len());
        for i in 0..b_count {
            let (mut b, amb_sound, dmg_sound) = {
                let swbf2 = self.swbf2().unwrap();
                let b = swbf2.vehicles.buildings[i];
                let (a, d) = swbf2
                    .classes
                    .get(b.class)
                    .map_or((NONE, NONE), |c| (c.building_sound, c.damage_effect_sound));
                (b, a, d)
            };
            if !b.alive {
                if b.voice != SILENT {
                    self.stop(b.voice, 0.15);
                }
                if b.voice != SILENT || b.in_range {
                    if let Some(swbf2) = self.swbf2_mut() {
                        swbf2.vehicles.buildings[i].voice = SILENT;
                        swbf2.vehicles.buildings[i].in_range = false;
                        swbf2.vehicles.buildings[i].retry.reset();
                    }
                }
                if !b.damaged {
                    if let Some(swbf2) = self.swbf2_mut() {
                        swbf2.vehicles.buildings[i].damaged = true;
                    }
                    self.play_sound_at(dmg_sound, EXPLOSIONS, NONE, Some(b.position), Some(false));
                }
            } else {
                if b.damaged {
                    if let Some(swbf2) = self.swbf2_mut() {
                        swbf2.vehicles.buildings[i].damaged = false;
                    }
                }
                let now_in_range = amb_sound != NONE && {
                    let mute = self.swbf2_sound_mute_distance(amb_sound);
                    mute <= 0.0 || length(sub(b.position, ear)) < mute
                };
                if now_in_range {
                    if self.playing(b.voice) {
                        b.in_range = true;
                        b.retry.note_live();
                    } else {
                        b.voice = SILENT;
                        let want_start = if !b.in_range {
                            true
                        } else {
                            self.swbf2_loop_should_retry(&mut b.retry, dt, amb_sound, NONE, Some(b.position), false)
                        };
                        if want_start {
                            let had_room = self.swbf2_loop_has_room(amb_sound, false);
                            let voice = self.play_sound_at(amb_sound, VEHICLE_FX_CATEGORY, NONE, Some(b.position), Some(true));
                            let live = self.playing(voice);
                            b.voice = if live { voice } else { SILENT };
                            b.in_range = true;
                            b.retry.record(live, had_room);
                        }
                    }
                    if let Some(swbf2) = self.swbf2_mut() {
                        swbf2.vehicles.buildings[i].voice = b.voice;
                        swbf2.vehicles.buildings[i].in_range = b.in_range;
                        swbf2.vehicles.buildings[i].retry = b.retry;
                    }
                } else if b.in_range || b.voice != SILENT {
                    if b.voice != SILENT {
                        self.stop(b.voice, 0.15);
                    }
                    if let Some(swbf2) = self.swbf2_mut() {
                        swbf2.vehicles.buildings[i].voice = SILENT;
                        swbf2.vehicles.buildings[i].in_range = false;
                        swbf2.vehicles.buildings[i].retry.reset();
                    }
                }
            }
        }

        // 6. Door events (`OpenSound` / `CloseSound` / `LockedSound`).
        let mut d_events = [None; MAX_DOOR_EVENTS];
        let d_count = {
            let Some(swbf2) = self.swbf2_mut() else { return };
            let n = swbf2.vehicles.door_events_len;
            for (dst, src) in d_events[..n]
                .iter_mut()
                .zip(&mut swbf2.vehicles.door_events[..n])
            {
                *dst = src.take();
            }
            swbf2.vehicles.door_events_len = 0;
            n
        };
        for ev in d_events[..d_count].iter().flatten().copied() {
            let (position, sound) = match ev {
                DoorEvent::Open { class_hash, position } => (
                    position,
                    self.swbf2()
                        .and_then(|s| s.class_by_hash(class_hash).and_then(|c| s.classes.get(c)))
                        .map_or(NONE, |c| c.door_open),
                ),
                DoorEvent::Close { class_hash, position } => (
                    position,
                    self.swbf2()
                        .and_then(|s| s.class_by_hash(class_hash).and_then(|c| s.classes.get(c)))
                        .map_or(NONE, |c| c.door_close),
                ),
                DoorEvent::Locked { class_hash, position } => (
                    position,
                    self.swbf2()
                        .and_then(|s| s.class_by_hash(class_hash).and_then(|c| s.classes.get(c)))
                        .map_or(NONE, |c| c.door_locked),
                ),
            };
            self.play_sound_at(sound, VEHICLE_FX_CATEGORY, NONE, Some(position), Some(false));
        }
    }

    fn swbf2_chainguns(&mut self, dt: f64) {
        let mut inputs = [None; MAX_CHAINGUNS];
        let n = {
            let Some(swbf2) = self.swbf2_mut() else { return };
            for slot in &mut swbf2.vehicles.chainguns {
                slot.seen = false;
            }
            let n = swbf2.vehicles.chaingun_inputs_len;
            for (dst, src) in inputs[..n]
                .iter_mut()
                .zip(&mut swbf2.vehicles.chaingun_inputs[..n])
            {
                *dst = src.take();
            }
            swbf2.vehicles.chaingun_inputs_len = 0;
            n
        };

        for input in inputs[..n].iter().flatten().copied() {
            let Some(class) = self.swbf2_class_by_hash(input.class_hash) else {
                continue;
            };
            let idx = self.swbf2().and_then(|s| {
                s.vehicles
                    .chainguns
                    .iter()
                    .position(|c| c.weapon == input.weapon)
                    .or_else(|| s.vehicles.chainguns.iter().position(|c| c.weapon == NONE))
            });
            let Some(idx) = idx else { continue };
            let mut st = self.swbf2().unwrap().vehicles.chainguns[idx];
            st.weapon = input.weapon;
            st.owner = input.owner;
            st.class = class;
            st.seen = true;

            let (warm_sound, warm_time, start_pitch, fade_in, fade_out, barrel_sound, b_fade_in, b_fade_out) =
                self.swbf2()
                    .and_then(|s| s.classes.get(class))
                    .map_or((NONE, 0.5, 0.5, 0.1, 0.1, NONE, 0.05, 0.1), |c| {
                        (
                            c.warm_up,
                            c.warm_up_time.max(0.01),
                            c.warm_up_start_pitch.clamp(0.1, 4.0),
                            c.warm_up_fade_in.max(0.0),
                            c.warm_up_fade_out.max(0.01),
                            c.barrel,
                            c.barrel_fade_in.max(0.0),
                            c.barrel_fade_out.max(0.01),
                        )
                    });

            let own = index(self.frame[crate::layout::frame::OWN]);
            let is_own = own != NONE && input.owner == own;

            if input.spinning && warm_sound != NONE {
                if !st.spinning {
                    st.elapsed = 0.0;
                } else {
                    st.elapsed += dt;
                }
                if self.playing(st.warm_voice) {
                    st.warm_retry.note_live();
                } else {
                    st.warm_voice = SILENT;
                    let want_start = if !st.spinning {
                        true
                    } else {
                        self.swbf2_loop_should_retry(&mut st.warm_retry, dt, warm_sound, input.owner, None, is_own)
                    };
                    if want_start {
                        let had_room = self.swbf2_loop_has_room(warm_sound, is_own);
                        let voice = self.play_sound_protected(
                            warm_sound,
                            WEAPONS,
                            input.owner,
                            None,
                            Some(true),
                            is_own,
                        );
                        let now_live = self.playing(voice);
                        st.warm_voice = if now_live { voice } else { SILENT };
                        st.warm_retry.record(now_live, had_room);
                    }
                }
                if st.warm_voice != SILENT {
                    let frac = (st.elapsed / warm_time).clamp(0.0, 1.0);
                    let pitch = start_pitch + (1.0 - start_pitch) * frac;
                    let gain = if fade_in > 0.0 {
                        (st.elapsed / fade_in).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };
                    self.pitch(st.warm_voice, pitch, false);
                    self.gain(st.warm_voice, gain);
                }
            } else {
                st.warm_retry.reset();
                if st.warm_voice != SILENT {
                    self.stop(st.warm_voice, fade_out);
                    st.warm_voice = SILENT;
                }
                st.elapsed = 0.0;
            }

            if input.firing && barrel_sound != NONE {
                if !st.firing {
                    st.barrel_elapsed = 0.0;
                } else {
                    st.barrel_elapsed += dt;
                }
                if self.playing(st.barrel_voice) {
                    st.barrel_retry.note_live();
                } else {
                    st.barrel_voice = SILENT;
                    let want_start = if !st.firing {
                        true
                    } else {
                        self.swbf2_loop_should_retry(&mut st.barrel_retry, dt, barrel_sound, input.owner, None, is_own)
                    };
                    if want_start {
                        let had_room = self.swbf2_loop_has_room(barrel_sound, is_own);
                        let voice = self.play_sound_protected(
                            barrel_sound,
                            WEAPONS,
                            input.owner,
                            None,
                            Some(true),
                            is_own,
                        );
                        let now_live = self.playing(voice);
                        st.barrel_voice = if now_live { voice } else { SILENT };
                        st.barrel_retry.record(now_live, had_room);
                    }
                }
                if st.barrel_voice != SILENT {
                    let gain = if b_fade_in > 0.0 {
                        (st.barrel_elapsed / b_fade_in).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };
                    self.gain(st.barrel_voice, gain);
                }
            } else {
                st.barrel_retry.reset();
                if st.barrel_voice != SILENT {
                    self.stop(st.barrel_voice, b_fade_out);
                    st.barrel_voice = SILENT;
                }
                st.barrel_elapsed = 0.0;
            }

            st.spinning = input.spinning;
            st.firing = input.firing;
            if let Some(swbf2) = self.swbf2_mut() {
                swbf2.vehicles.chainguns[idx] = st;
            }
        }

        // Stop any chaingun slots that were not updated this tick.
        for idx in 0..MAX_CHAINGUNS {
            let st = self.swbf2().unwrap().vehicles.chainguns[idx];
            if st.weapon != NONE && !st.seen {
                if st.warm_voice != SILENT {
                    self.stop(st.warm_voice, 0.1);
                }
                if st.barrel_voice != SILENT {
                    self.stop(st.barrel_voice, 0.1);
                }
                if let Some(swbf2) = self.swbf2_mut() {
                    swbf2.vehicles.chainguns[idx] = ChaingunState::default();
                }
            }
        }
    }

    pub(crate) fn stop_swbf2_vehicles(&mut self) {
        let (cockpit_voice, cockpit_is_stream, b_count) = match self.swbf2() {
            Some(s) => (s.vehicles.cockpit_voice, s.vehicles.cockpit_is_stream, s.vehicles.buildings.len()),
            None => return,
        };
        if cockpit_voice != SILENT {
            if cockpit_is_stream {
                self.streams.stop(&mut self.engine, cockpit_voice, 0.05);
            } else {
                self.stop(cockpit_voice, 0.05);
            }
        }
        if let Some(swbf2) = self.swbf2_mut() {
            swbf2.vehicles.cockpit_voice = SILENT;
            swbf2.vehicles.cockpit_sound = NONE;
            swbf2.vehicles.cockpit_retry.reset();
            swbf2.vehicles.vehicle_events_len = 0;
            swbf2.vehicles.door_events_len = 0;
            swbf2.vehicles.chaingun_inputs_len = 0;
        }
        for i in 0..b_count {
            let v = self.swbf2().unwrap().vehicles.buildings[i].voice;
            if v != SILENT {
                self.stop(v, 0.05);
            }
            if let Some(swbf2) = self.swbf2_mut() {
                swbf2.vehicles.buildings[i].voice = SILENT;
                swbf2.vehicles.buildings[i].in_range = false;
                swbf2.vehicles.buildings[i].retry.reset();
            }
        }
        for idx in 0..MAX_CHAINGUNS {
            let st = self.swbf2().unwrap().vehicles.chainguns[idx];
            if st.warm_voice != SILENT {
                self.stop(st.warm_voice, 0.05);
            }
            if st.barrel_voice != SILENT {
                self.stop(st.barrel_voice, 0.05);
            }
            if let Some(swbf2) = self.swbf2_mut() {
                swbf2.vehicles.chainguns[idx] = ChaingunState::default();
            }
        }
    }
}
