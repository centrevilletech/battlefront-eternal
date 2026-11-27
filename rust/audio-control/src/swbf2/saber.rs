//! Lightsaber and melee combo sound state for 2005 battles.
//! Maintains continuous blade hum, ignite/extinguish transitions, delayed
//! combo swing triggers (in seconds, never frame counts), hit, block, and
//! blaster-bolt deflect sounds with zero per-tick allocations.
//!
//! All identifiers in `SaberEvent` are 32-bit case-folded FNV-1a hashes
//! (`audio_control::swbf2::zero_hash`) matching simulation weapon class names,
//! combo machine `name_hash` values, state names, and sound property names.

use super::{LoopRetry, SABER_CATEGORY};
use crate::engine::{NONE, SILENT};
use crate::layout::emitter as e;
use crate::phase::Phase;
use crate::Point;

pub const MAX_PENDING_SABER_EVENTS: usize = 128;
pub const MAX_DELAYED_SWINGS: usize = 64;

/// Public melee/lightsaber event surface pushed by combat code before `step`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SaberEvent {
    /// Ignites the blade for `weapon_class_hash`: plays `TurnOnSound` and starts the `OnSound` hum.
    Ignite { owner: usize, weapon_class_hash: u32 },
    /// Extinguishes the blade (`weapon_class_hash == 0` uses the active weapon): stops hum,
    /// clears queued swings, plays `TurnOffSound`.
    Extinguish { owner: usize, weapon_class_hash: u32 },
    /// Declarative blade toggle; triggers ignite/extinguish only on state change.
    SetBladeActive { owner: usize, weapon_class_hash: u32, active: bool },
    /// Suspends or resumes the in-hand blade hum while the saber is in flight
    /// without playing TurnOffSound or TurnOnSound.
    SetThrown { owner: usize, thrown: bool },
    /// Schedules an explicit swing sound (`zero_hash` of sound name) following `owner` after `delay_s` seconds.
    Swing { owner: usize, sound_hash: u32, delay_s: f64 },
    /// Schedules all `Sound(...)` triggers belonging to `(combo_hash, state_hash)`, where
    /// `combo_hash` is the combo machine's `name_hash` and `state_hash` is `zero_hash(state_name)`.
    ComboState { owner: usize, combo_hash: u32, state_hash: u32 },
    /// Plays `HitSound` of `weapon_class_hash` at the impact point.
    Hit { owner: usize, weapon_class_hash: u32, position: Point },
    /// Plays a block sound (`sound_hash`, or `DeflectSound` of active blade when `0`) at the contact point.
    Block { owner: usize, sound_hash: u32, position: Point },
    /// Plays `DeflectSound` of `weapon_class_hash` at the deflected bolt point.
    Deflect { owner: usize, weapon_class_hash: u32, position: Point },
}

#[derive(Clone, Copy)]
struct SaberOwner {
    active: bool,
    thrown: bool,
    weapon_class: usize,
    hum_voice: u64,
    hum_retry: LoopRetry,
    generation: f64,
}

impl Default for SaberOwner {
    fn default() -> Self {
        Self {
            active: false,
            thrown: false,
            weapon_class: NONE,
            hum_voice: SILENT,
            hum_retry: LoopRetry::default(),
            generation: -1.0,
        }
    }
}

#[derive(Clone, Copy)]
struct DelayedSwing {
    active: bool,
    owner: usize,
    sound: usize,
    remaining_s: f64,
}

impl Default for DelayedSwing {
    fn default() -> Self {
        Self {
            active: false,
            owner: NONE,
            sound: NONE,
            remaining_s: 0.0,
        }
    }
}

pub struct SaberBank {
    owners: Vec<SaberOwner>,
    delayed: [DelayedSwing; MAX_DELAYED_SWINGS],
    pending: [Option<SaberEvent>; MAX_PENDING_SABER_EVENTS],
    pending_len: usize,
}

impl SaberBank {
    pub fn empty() -> Self {
        Self {
            owners: Vec::new(),
            delayed: [DelayedSwing::default(); MAX_DELAYED_SWINGS],
            pending: [None; MAX_PENDING_SABER_EVENTS],
            pending_len: 0,
        }
    }

    pub fn preallocated(records: usize) -> Self {
        Self {
            owners: vec![SaberOwner::default(); records],
            delayed: [DelayedSwing::default(); MAX_DELAYED_SWINGS],
            pending: [None; MAX_PENDING_SABER_EVENTS],
            pending_len: 0,
        }
    }

    pub fn push(&mut self, event: SaberEvent) {
        if self.pending_len < MAX_PENDING_SABER_EVENTS {
            self.pending[self.pending_len] = Some(event);
            self.pending_len += 1;
        }
    }

    pub fn blade_active(&self, owner: usize) -> bool {
        self.owners.get(owner).is_some_and(|s| s.active)
    }

    pub fn hum_voice(&self, owner: usize) -> u64 {
        self.owners.get(owner).map_or(SILENT, |s| s.hum_voice)
    }

    pub fn pending_delayed_count(&self) -> usize {
        self.delayed.iter().filter(|s| s.active).count()
    }

    fn cancel_owner_swings(&mut self, owner: usize) {
        for slot in &mut self.delayed {
            if slot.active && slot.owner == owner {
                slot.active = false;
            }
        }
    }

    fn queue_swing(&mut self, owner: usize, sound: usize, delay_s: f64) {
        if sound == NONE {
            return;
        }
        if let Some(slot) = self.delayed.iter_mut().find(|s| !s.active) {
            *slot = DelayedSwing {
                active: true,
                owner,
                sound,
                remaining_s: delay_s.max(0.0),
            };
        }
    }
}

impl Phase {
    pub(crate) fn swbf2_sabers(&mut self, dt: f64) {
        let mut events = [None; MAX_PENDING_SABER_EVENTS];
        let count = {
            let Some(swbf2) = self.swbf2_mut() else { return };
            let count = swbf2.sabers.pending_len;
            for (dst, src) in events[..count]
                .iter_mut()
                .zip(&mut swbf2.sabers.pending[..count])
            {
                *dst = src.take();
            }
            swbf2.sabers.pending_len = 0;
            count
        };

        let mut hum_attempted = [NONE; MAX_PENDING_SABER_EVENTS];
        let mut hum_attempted_len = 0usize;
        for ev in events[..count].iter().flatten().copied() {
            match ev {
                SaberEvent::Ignite { owner, weapon_class_hash } => {
                    if let Some(cls) = self.swbf2_class_by_hash(weapon_class_hash) {
                        self.saber_ignite(owner, cls);
                        hum_attempted[hum_attempted_len] = owner;
                        hum_attempted_len += 1;
                    }
                }
                SaberEvent::Extinguish { owner, weapon_class_hash } => {
                    let cls = self.swbf2_class_by_hash(weapon_class_hash).unwrap_or(NONE);
                    self.saber_extinguish(owner, cls);
                }
                SaberEvent::SetBladeActive { owner, weapon_class_hash, active } => {
                    let was = self.swbf2().is_some_and(|s| s.sabers.blade_active(owner));
                    if active && !was {
                        if let Some(cls) = self.swbf2_class_by_hash(weapon_class_hash) {
                            self.saber_ignite(owner, cls);
                            hum_attempted[hum_attempted_len] = owner;
                            hum_attempted_len += 1;
                        }
                    } else if !active && was {
                        let cls = self.swbf2_class_by_hash(weapon_class_hash).unwrap_or(NONE);
                        self.saber_extinguish(owner, cls);
                    }
                }
                SaberEvent::SetThrown { owner, thrown } => {
                    let prev = self.swbf2().and_then(|s| s.sabers.owners.get(owner).copied());
                    if let Some(prev) = prev {
                        if thrown && !prev.thrown {
                            if prev.hum_voice != SILENT {
                                self.stop(prev.hum_voice, 0.04);
                            }
                            if let Some(swbf2) = self.swbf2_mut() {
                                swbf2.sabers.owners[owner].thrown = true;
                                swbf2.sabers.owners[owner].hum_voice = SILENT;
                                swbf2.sabers.owners[owner].hum_retry.reset();
                            }
                        } else if !thrown && prev.thrown {
                            let mut hum_retry = LoopRetry::default();
                            let hum_voice = if prev.active {
                                let hum_sound = self
                                    .swbf2()
                                    .and_then(|s| s.classes.get(prev.weapon_class))
                                    .map_or(NONE, |c| c.saber_on_hum);
                                if hum_sound != NONE {
                                    let had_room = self.swbf2_loop_has_room(hum_sound, false);
                                    let v = self.play_sound_at(hum_sound, SABER_CATEGORY, owner, None, Some(true));
                                    let live = self.playing(v);
                                    hum_retry.record(live, had_room);
                                    hum_attempted[hum_attempted_len] = owner;
                                    hum_attempted_len += 1;
                                    if live { v } else { SILENT }
                                } else {
                                    SILENT
                                }
                            } else {
                                SILENT
                            };
                            if let Some(swbf2) = self.swbf2_mut() {
                                swbf2.sabers.owners[owner].thrown = false;
                                swbf2.sabers.owners[owner].hum_voice = hum_voice;
                                swbf2.sabers.owners[owner].hum_retry = hum_retry;
                            }
                        }
                    }
                }
                SaberEvent::Swing { owner, sound_hash, delay_s } => {
                    let sound = self.named(sound_hash);
                    if sound != NONE {
                        if delay_s <= 0.0 {
                            self.play_sound_at(sound, SABER_CATEGORY, owner, None, Some(false));
                        } else if let Some(swbf2) = self.swbf2_mut() {
                            swbf2.sabers.queue_swing(owner, sound, delay_s);
                        }
                    }
                }
                SaberEvent::ComboState { owner, combo_hash, state_hash } => {
                    let mut triggers = [(NONE, 0.0); 16];
                    let mut n = 0;
                    if let Some(swbf2) = self.swbf2() {
                        if let Some(combo_id) = swbf2.combo_by_hash(combo_hash) {
                            if let Some(set) = swbf2.combos.get(combo_id) {
                                if let Some(state) = set.states.iter().find(|s| s.state_hash == state_hash) {
                                    for trig in &state.triggers {
                                        if n < triggers.len() {
                                            triggers[n] = (trig.sound, trig.delay_s);
                                            n += 1;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    for &(sound, delay_s) in &triggers[..n] {
                        if delay_s <= 0.0 {
                            self.play_sound_at(sound, SABER_CATEGORY, owner, None, Some(false));
                        } else if let Some(swbf2) = self.swbf2_mut() {
                            swbf2.sabers.queue_swing(owner, sound, delay_s);
                        }
                    }
                }
                SaberEvent::Hit { owner, weapon_class_hash, position } => {
                    let chosen = self.swbf2().map_or(NONE, |s| {
                        if weapon_class_hash != 0 {
                            s.class_by_hash(weapon_class_hash)
                                .and_then(|c| s.classes.get(c))
                                .map_or(NONE, |c| c.saber_hit)
                        } else {
                            s.sabers
                                .owners
                                .get(owner)
                                .and_then(|o| s.classes.get(o.weapon_class))
                                .map_or(NONE, |c| c.saber_hit)
                        }
                    });
                    self.play_sound_at(chosen, SABER_CATEGORY, NONE, Some(position), Some(false));
                }
                SaberEvent::Block { owner, sound_hash, position } => {
                    let direct = if sound_hash != 0 { self.named(sound_hash) } else { NONE };
                    let chosen = if direct != NONE {
                        direct
                    } else {
                        self.swbf2().map_or(NONE, |s| {
                            let wcls = s.sabers.owners.get(owner).map_or(NONE, |o| o.weapon_class);
                            s.classes.get(wcls).map_or(NONE, |c| c.saber_deflect)
                        })
                    };
                    self.play_sound_at(chosen, SABER_CATEGORY, NONE, Some(position), Some(false));
                }
                SaberEvent::Deflect { owner, weapon_class_hash, position } => {
                    let sound = self.swbf2().map_or(NONE, |s| {
                        if weapon_class_hash != 0 {
                            s.class_by_hash(weapon_class_hash)
                                .and_then(|c| s.classes.get(c))
                                .map_or(NONE, |c| c.saber_deflect)
                        } else {
                            let wcls = s.sabers.owners.get(owner).map_or(NONE, |o| o.weapon_class);
                            s.classes.get(wcls).map_or(NONE, |c| c.saber_deflect)
                        }
                    });
                    self.play_sound_at(sound, SABER_CATEGORY, NONE, Some(position), Some(false));
                }
            }
        }

        // Keep active blade hums bound to live owners; silence immediately on death or respawn.
        let owners_len = self.swbf2().map_or(0, |s| s.sabers.owners.len()).min(self.entity_count);
        for owner in 0..owners_len {
            let state = self.swbf2().unwrap().sabers.owners[owner];
            if !state.active {
                continue;
            }
            let row = self.emitters[owner];
            let dead = row[e::KIND] == 0.0
                || row[e::ALIVE] == 0.0
                || row[e::REMOVED] != 0.0
                || (state.generation >= 0.0 && row[e::GENERATION] != state.generation);
            if dead {
                let hum = state.hum_voice;
                if let Some(swbf2) = self.swbf2_mut() {
                    swbf2.sabers.owners[owner] = SaberOwner::default();
                    swbf2.sabers.cancel_owner_swings(owner);
                }
                self.stop(hum, 0.05);
            } else if !state.thrown && !hum_attempted[..hum_attempted_len].contains(&owner) {
                let hum_sound = self
                    .swbf2()
                    .and_then(|s| s.classes.get(state.weapon_class))
                    .map_or(NONE, |c| c.saber_on_hum);
                if hum_sound != NONE {
                    let mut retry = state.hum_retry;
                    if self.playing(state.hum_voice) {
                        retry.note_live();
                        if let Some(swbf2) = self.swbf2_mut() {
                            swbf2.sabers.owners[owner].hum_retry = retry;
                        }
                    } else {
                        let new_voice = if self.swbf2_loop_should_retry(&mut retry, dt, hum_sound, owner, None, false) {
                            let had_room = self.swbf2_loop_has_room(hum_sound, false);
                            let v = self.play_sound_at(hum_sound, SABER_CATEGORY, owner, None, Some(true));
                            let live = self.playing(v);
                            retry.record(live, had_room);
                            if live { v } else { SILENT }
                        } else {
                            SILENT
                        };
                        if let Some(swbf2) = self.swbf2_mut() {
                            swbf2.sabers.owners[owner].hum_voice = new_voice;
                            swbf2.sabers.owners[owner].hum_retry = retry;
                        }
                    }
                }
            }
        }

        // Advance delayed swing triggers in seconds.
        if dt > 0.0 {
            let mut due = [(NONE, NONE); MAX_DELAYED_SWINGS];
            let mut due_len = 0;
            if let Some(swbf2) = self.swbf2_mut() {
                for slot in &mut swbf2.sabers.delayed {
                    if !slot.active {
                        continue;
                    }
                    slot.remaining_s -= dt;
                    if slot.remaining_s <= 0.0 {
                        due[due_len] = (slot.owner, slot.sound);
                        due_len += 1;
                        slot.active = false;
                    }
                }
            }
            for &(owner, sound) in &due[..due_len] {
                let alive = self
                    .emitters
                    .get(owner)
                    .is_some_and(|r| r[e::KIND] != 0.0 && r[e::ALIVE] != 0.0);
                if alive {
                    self.play_sound_at(sound, SABER_CATEGORY, owner, None, Some(false));
                }
            }
        }
    }

    fn saber_ignite(&mut self, owner: usize, weapon_class: usize) {
        let Some(prev) = self.swbf2().and_then(|s| s.sabers.owners.get(owner).copied()) else {
            return;
        };
        if prev.hum_voice != SILENT {
            self.stop(prev.hum_voice, 0.04);
        }
        let (turn_on, on_hum) = self
            .swbf2()
            .and_then(|s| s.classes.get(weapon_class))
            .map_or((NONE, NONE), |c| (c.saber_turn_on, c.saber_on_hum));
        self.play_sound_at(turn_on, SABER_CATEGORY, owner, None, Some(false));
        let mut hum_retry = LoopRetry::default();
        let hum_voice = if on_hum != NONE {
            let had_room = self.swbf2_loop_has_room(on_hum, false);
            let v = self.play_sound_at(on_hum, SABER_CATEGORY, owner, None, Some(true));
            let live = self.playing(v);
            hum_retry.record(live, had_room);
            if live { v } else { SILENT }
        } else {
            SILENT
        };
        let generation = self
            .emitters
            .get(owner)
            .map_or(-1.0, |r| r[e::GENERATION]);
        if let Some(swbf2) = self.swbf2_mut() {
            swbf2.sabers.owners[owner] = SaberOwner {
                active: true,
                thrown: false,
                weapon_class,
                hum_voice,
                hum_retry,
                generation,
            };
        }
    }

    fn saber_extinguish(&mut self, owner: usize, weapon_class: usize) {
        let Some(prev) = self.swbf2().and_then(|s| s.sabers.owners.get(owner).copied()) else {
            return;
        };
        if let Some(swbf2) = self.swbf2_mut() {
            swbf2.sabers.owners[owner] = SaberOwner::default();
            swbf2.sabers.cancel_owner_swings(owner);
        }
        if prev.hum_voice != SILENT {
            self.stop(prev.hum_voice, 0.08);
        }
        let cls = if weapon_class != NONE { weapon_class } else { prev.weapon_class };
        let turn_off = self
            .swbf2()
            .and_then(|s| s.classes.get(cls))
            .map_or(NONE, |c| c.saber_turn_off);
        self.play_sound_at(turn_off, SABER_CATEGORY, owner, None, Some(false));
    }

    pub(crate) fn stop_swbf2_sabers(&mut self) {
        let len = self.swbf2().map_or(0, |s| s.sabers.owners.len());
        for owner in 0..len {
            let hum = self.swbf2().unwrap().sabers.owners[owner].hum_voice;
            if hum != SILENT {
                self.stop(hum, 0.05);
            }
            if let Some(swbf2) = self.swbf2_mut() {
                swbf2.sabers.owners[owner] = SaberOwner::default();
            }
        }
        if let Some(swbf2) = self.swbf2_mut() {
            for slot in &mut swbf2.sabers.delayed {
                slot.active = false;
            }
            swbf2.sabers.pending_len = 0;
        }
    }
}
