//! 2005 (`GameFamily::Swbf2`) weapon, chaingun, ordnance-flight, hit,
//! explosion, and destruction sound controllers. Zero heap allocations on
//! steady-state ticks.

use super::LoopRetry;
use crate::engine::{Ask, EFFECTS, EXPLOSIONS, FOLEY, NONE, SILENT, WEAPONS};
use crate::layout::{class as c, emitter as e};
use crate::phase::Phase;
use crate::{length, sub, Point};

pub const MAX_SPACE_WEAPONS: usize = 256;
pub const MAX_SPACE_ORDNANCE_LOOPS: usize = 16;
pub const MAX_SPACE_AUDIO_EVENTS: usize = 128;
pub const MAX_SPACE_ORDNANCE_INPUTS: usize = 256;

/// Queued one-shot 2005 space audio event processed during `Phase::step` after
/// listener and time are updated for the current frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpaceAudioEvent {
    RoundHit { weapon_hash: u32, position: Point },
    RoundExpired { weapon_hash: u32, position: Point },
    Destruction { class_hash: u32, position: Point },
}

/// Per-tick state of one mounted 2005 space weapon (fighter gun or ship/chair turret).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpaceWeaponInput {
    /// Stable slot identifier across ticks (e.g. `(emitter_id << 3) | weapon_slot`).
    pub key: usize,
    /// Emitter index in `Phase::emitters` or turret emitter (`turret_emitter_base + idx`).
    pub owner: usize,
    /// `zero_hash` of the weapon ODF class name.
    pub class_hash: u32,
    /// True if at least one round fired on this tick (salvos already folded).
    pub fired: bool,
    /// True while the trigger is actively sustaining fire (drives `FireLoopSound`, `BarrelSound`, `FireSoundStop`).
    pub firing: bool,
    /// True while a spin-up weapon's barrels are spinning (`WarmUpSound`).
    pub spinning: bool,
    /// True while the weapon is locked out by overheat (`OverheatSound` + pitch sweep).
    pub overheated: bool,
    /// Normalized heat (`0.0..=1.0`).
    pub heat: f64,
    /// Authored `HeatThreshold` (`0.0..=1.0`).
    pub heat_threshold: f64,
    /// True while reloading (`ReloadSound`).
    pub reloading: bool,
    /// True on the tick the player switched onto this weapon (`WeaponChangeSound`).
    pub changed: bool,
    /// True when this weapon belongs to the local player's mounted fighter or chair turret.
    pub own: bool,
}

/// Active flying ordnance entry for bounded in-flight `OrdnanceSound` / guided-rocket loops.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpaceOrdnanceInput {
    pub serial: u64,
    pub weapon_hash: u32,
    pub position: Point,
    pub velocity: Point,
}

#[derive(Clone, Copy)]
struct SpaceWeaponSlot {
    key: usize,
    owner: usize,
    class: usize,
    /// Slot 0 state/loop voice (`ReloadSound`, `OverheatSound`, `FireLoopSound`).
    v0: u64,
    /// Kind of `v0`: 0 = none, 1 = reload, 2 = overheat, 4 = fire loop.
    v0_kind: u8,
    /// Slot 1 fire voice (`FireSound`).
    v1: u64,
    /// Chaingun `WarmUpSound` voice.
    warm_voice: u64,
    /// Chaingun `BarrelSound` voice.
    barrel_voice: u64,
    warm_elapsed: f64,
    barrel_elapsed: f64,
    overheat_retry: LoopRetry,
    fire_loop_retry: LoopRetry,
    warm_retry: LoopRetry,
    barrel_retry: LoopRetry,
    fire_loop_started: bool,
    reloading: bool,
    overheated: bool,
    firing: bool,
    spinning: bool,
    seen: bool,
}

impl Default for SpaceWeaponSlot {
    fn default() -> Self {
        Self {
            key: NONE,
            owner: NONE,
            class: NONE,
            v0: SILENT,
            v0_kind: 0,
            v1: SILENT,
            warm_voice: SILENT,
            barrel_voice: SILENT,
            warm_elapsed: 0.0,
            barrel_elapsed: 0.0,
            overheat_retry: LoopRetry::default(),
            fire_loop_retry: LoopRetry::default(),
            warm_retry: LoopRetry::default(),
            barrel_retry: LoopRetry::default(),
            fire_loop_started: false,
            reloading: false,
            overheated: false,
            firing: false,
            spinning: false,
            seen: false,
        }
    }
}

#[derive(Clone, Copy)]
struct OrdnanceLoop {
    serial: u64,
    voice: u64,
    retry: LoopRetry,
    seen: bool,
}

impl Default for OrdnanceLoop {
    fn default() -> Self {
        Self {
            serial: 0,
            voice: SILENT,
            retry: LoopRetry::default(),
            seen: false,
        }
    }
}

pub struct Swbf2WeaponBank {
    inputs: [Option<SpaceWeaponInput>; MAX_SPACE_WEAPONS],
    inputs_len: usize,
    slots: [SpaceWeaponSlot; MAX_SPACE_WEAPONS],
    ordnance_loops: [OrdnanceLoop; MAX_SPACE_ORDNANCE_LOOPS],
    audio_events: [Option<SpaceAudioEvent>; MAX_SPACE_AUDIO_EVENTS],
    audio_events_len: usize,
    ordnance_emitter_base: usize,
    ordnance_inputs: [SpaceOrdnanceInput; MAX_SPACE_ORDNANCE_INPUTS],
    ordnance_inputs_len: usize,
    ordnance_pending: bool,
}

impl Default for Swbf2WeaponBank {
    fn default() -> Self {
        Self {
            inputs: [None; MAX_SPACE_WEAPONS],
            inputs_len: 0,
            slots: [SpaceWeaponSlot::default(); MAX_SPACE_WEAPONS],
            ordnance_loops: [OrdnanceLoop::default(); MAX_SPACE_ORDNANCE_LOOPS],
            audio_events: [None; MAX_SPACE_AUDIO_EVENTS],
            audio_events_len: 0,
            ordnance_emitter_base: 0,
            ordnance_inputs: [SpaceOrdnanceInput {
                serial: 0,
                weapon_hash: 0,
                position: [0.0; 3],
                velocity: [0.0; 3],
            }; MAX_SPACE_ORDNANCE_INPUTS],
            ordnance_inputs_len: 0,
            ordnance_pending: false,
        }
    }
}

impl Swbf2WeaponBank {
    pub fn push(&mut self, input: SpaceWeaponInput) {
        if self.inputs_len < MAX_SPACE_WEAPONS {
            self.inputs[self.inputs_len] = Some(input);
            self.inputs_len += 1;
        }
    }

    pub fn push_event(&mut self, event: SpaceAudioEvent) {
        if self.audio_events_len < MAX_SPACE_AUDIO_EVENTS {
            self.audio_events[self.audio_events_len] = Some(event);
            self.audio_events_len += 1;
        }
    }

    pub fn set_ordnance(&mut self, emitter_base: usize, active: &[SpaceOrdnanceInput]) {
        let n = active.len().min(MAX_SPACE_ORDNANCE_INPUTS);
        self.ordnance_emitter_base = emitter_base;
        self.ordnance_inputs[..n].copy_from_slice(&active[..n]);
        self.ordnance_inputs_len = n;
        self.ordnance_pending = true;
    }
}

impl Phase {
    /// Queues one space weapon's per-tick audio state (zero allocation).
    pub fn push_space_weapon(&mut self, input: SpaceWeaponInput) {
        if let Some(swbf2) = self.swbf2_mut() {
            swbf2.weapons.push(input);
        }
    }

    /// Queues a space hit, expiry, or destruction one-shot to play during `Phase::step`.
    pub fn push_space_audio_event(&mut self, event: SpaceAudioEvent) {
        if let Some(swbf2) = self.swbf2_mut() {
            swbf2.weapons.push_event(event);
        }
    }

    /// Queues the current frame's active space ordnance for bounded loop sync during `Phase::step`.
    pub fn queue_space_ordnance(&mut self, emitter_base: usize, active: &[SpaceOrdnanceInput]) {
        if let Some(swbf2) = self.swbf2_mut() {
            swbf2.weapons.set_ordnance(emitter_base, active);
        }
    }

    /// Plays a 2005 space round's authored impact (`CollisionSound` / `CollisionOtherSound`)
    /// and explosion (`SoundProperty` / guided-rocket `DeathSound`) at `position`.
    pub fn play_space_round_hit(&mut self, weapon_hash: u32, position: Point) {
        let Some(weapon_cls) = self.swbf2_class_by_hash(weapon_hash) else {
            return;
        };
        let (ord_cls, weapon_exp) = self
            .swbf2()
            .and_then(|s| s.classes.get(weapon_cls))
            .map_or((NONE, NONE), |cls| (cls.ordnance_class, cls.explosion_class));

        if ord_cls != NONE {
            let impact = self.sound(ord_cls, c::IMPACT);
            let snd = if impact != NONE { impact } else { self.sound(ord_cls, c::BOUNCE) };
            self.play_sound_at(snd, WEAPONS, NONE, Some(position), Some(false));
        }

        let ord_exp = self
            .swbf2()
            .and_then(|s| s.classes.get(ord_cls))
            .map_or(NONE, |cls| cls.explosion_class);
        let exp_cls = if ord_exp != NONE { ord_exp } else { weapon_exp };
        let exp_snd = if exp_cls != NONE { self.sound(exp_cls, c::EXPLOSION) } else { NONE };
        if exp_snd != NONE {
            self.play_sound_at(exp_snd, EXPLOSIONS, NONE, Some(position), Some(false));
        } else if ord_cls != NONE {
            let death = self.sound(ord_cls, c::DEATH);
            self.play_sound_at(death, EXPLOSIONS, NONE, Some(position), Some(false));
        }
    }

    /// Plays a 2005 space round's authored expiry explosion (`ExplosionExpire` -> `SoundProperty`),
    /// such as flak rounds that detonate at maximum range.
    pub fn play_space_round_expired(&mut self, weapon_hash: u32, position: Point) {
        let Some(weapon_cls) = self.swbf2_class_by_hash(weapon_hash) else {
            return;
        };
        let (ord_cls, weapon_expire) = self
            .swbf2()
            .and_then(|s| s.classes.get(weapon_cls))
            .map_or((NONE, NONE), |cls| (cls.ordnance_class, cls.expire_explosion_class));
        let ord_expire = self
            .swbf2()
            .and_then(|s| s.classes.get(ord_cls))
            .map_or(NONE, |cls| cls.expire_explosion_class);
        let exp_cls = if ord_expire != NONE { ord_expire } else { weapon_expire };
        if exp_cls != NONE {
            let snd = self.sound(exp_cls, c::EXPLOSION);
            self.play_sound_at(snd, EXPLOSIONS, NONE, Some(position), Some(false));
        }
    }

    /// Plays a 2005 craft, turret, or ship subsystem's authored destruction audio:
    /// `DeathSound` and/or linked `ExplosionDestruct`/`ExplosionName` `SoundProperty`.
    pub fn play_space_destruction(&mut self, class_hash: u32, position: Point) {
        let Some(cls) = self.swbf2_class_by_hash(class_hash) else {
            return;
        };
        let death_snd = self.sound(cls, c::DEATH);
        if death_snd != NONE {
            self.play_sound_at(death_snd, EXPLOSIONS, NONE, Some(position), Some(false));
        }
        let exp_cls = self
            .swbf2()
            .and_then(|s| s.classes.get(cls))
            .map_or(NONE, |c| c.destruct_explosion_class);
        if exp_cls != NONE {
            let exp_snd = self.sound(exp_cls, c::EXPLOSION);
            if exp_snd != NONE && exp_snd != death_snd {
                self.play_sound_at(exp_snd, EXPLOSIONS, NONE, Some(position), Some(false));
            }
        }
    }

    /// Synchronizes up to `MAX_SPACE_ORDNANCE_LOOPS` nearest in-range in-flight
    /// ordnance/rocket sounds (`OrdnanceSound` or guided-rocket `EngineSound`),
    /// using reserved emitter slots `emitter_base..emitter_base + MAX_SPACE_ORDNANCE_LOOPS`.
    pub fn sync_space_ordnance(&mut self, emitter_base: usize, active: &[SpaceOrdnanceInput]) {
        let dt = if self.paused {
            0.0
        } else {
            self.frame[crate::layout::frame::DT]
        };
        self.sync_space_ordnance_dt(emitter_base, active, dt);
    }

    fn sync_space_ordnance_dt(&mut self, emitter_base: usize, active: &[SpaceOrdnanceInput], dt: f64) {
        if self.swbf2().is_none() || emitter_base + MAX_SPACE_ORDNANCE_LOOPS > self.emitters.len() {
            return;
        }
        let ear = self.engine.listener.ear;
        // Pick up to MAX_SPACE_ORDNANCE_LOOPS closest in-range rounds with an authored loop sound.
        // Guided rockets and missiles (higher MuteDistance) naturally stay within range further out.
        let mut chosen: [(f64, usize, usize, bool, SpaceOrdnanceInput); MAX_SPACE_ORDNANCE_LOOPS] =
            [(f64::INFINITY, NONE, NONE, false, SpaceOrdnanceInput { serial: 0, weapon_hash: 0, position: [0.0; 3], velocity: [0.0; 3] });
                MAX_SPACE_ORDNANCE_LOOPS];
        let mut chosen_len = 0usize;

        for &item in active {
            if item.serial == 0 {
                continue;
            }
            let Some(weapon_cls) = self.swbf2_class_by_hash(item.weapon_hash) else {
                continue;
            };
            let ord_cls = self
                .swbf2()
                .and_then(|s| s.classes.get(weapon_cls))
                .map_or(NONE, |c| c.ordnance_class);
            if ord_cls == NONE {
                continue;
            }
            let (snd_id, is_param) = {
                let ord_snd = self.sound(ord_cls, c::ORDNANCE);
                if ord_snd != NONE {
                    (ord_snd, self.engine.sounds.get(ord_snd).is_some_and(|s| s.kind == 3))
                } else {
                    let eng_snd = self.sound(ord_cls, c::ENGINE);
                    if eng_snd != NONE {
                        (eng_snd, self.engine.sounds.get(eng_snd).is_some_and(|s| s.kind == 3))
                    } else {
                        continue;
                    }
                }
            };
            let dist = length(sub(item.position, ear));
            let mute = self.swbf2_sound_mute_distance(snd_id);
            if mute > 0.0 && dist >= mute {
                continue;
            }
            if chosen_len < MAX_SPACE_ORDNANCE_LOOPS {
                chosen[chosen_len] = (dist, snd_id, ord_cls, is_param, item);
                chosen_len += 1;
            } else {
                let mut worst = 0;
                for i in 1..MAX_SPACE_ORDNANCE_LOOPS {
                    if chosen[i].0 > chosen[worst].0 {
                        worst = i;
                    }
                }
                if dist < chosen[worst].0 {
                    chosen[worst] = (dist, snd_id, ord_cls, is_param, item);
                }
            }
        }

        if let Some(swbf2) = self.swbf2_mut() {
            for slot in &mut swbf2.weapons.ordnance_loops {
                slot.seen = false;
            }
        }

        // Keep already-tracked rounds in their existing slots first, recovering refused/stolen loops.
        let mut matched = [false; MAX_SPACE_ORDNANCE_LOOPS];
        for (ci, &(_, snd_id, _, is_param, item)) in chosen[..chosen_len].iter().enumerate() {
            let existing = self.swbf2().and_then(|s| {
                s.weapons
                    .ordnance_loops
                    .iter()
                    .position(|l| l.serial == item.serial)
            });
            if let Some(li) = existing {
                matched[ci] = true;
                let emitter = emitter_base + li;
                self.update_ordnance_emitter(emitter, item.position, item.velocity);
                let mut slot = self.swbf2().unwrap().weapons.ordnance_loops[li];
                slot.seen = true;
                if self.playing(slot.voice) {
                    slot.retry.note_live();
                } else {
                    slot.voice = SILENT;
                    if self.swbf2_loop_should_retry(&mut slot.retry, dt, snd_id, emitter, None, false) {
                        let had_room = self.swbf2_loop_has_room(snd_id, false);
                        let mut ask = Ask::new(snd_id, if is_param { FOLEY } else { WEAPONS });
                        ask.emitter = emitter;
                        ask.looping = Some(true);
                        let voice = if is_param {
                            self.parameterized(ask)
                        } else {
                            self.play(ask)
                        };
                        let live = self.playing(voice);
                        slot.voice = if live { voice } else { SILENT };
                        slot.retry.record(live, had_room);
                    }
                }
                if let Some(swbf2) = self.swbf2_mut() {
                    swbf2.weapons.ordnance_loops[li] = slot;
                }
            }
        }

        // Stop any previously tracked round that expired or fell out of range.
        for li in 0..MAX_SPACE_ORDNANCE_LOOPS {
            let slot = self.swbf2().unwrap().weapons.ordnance_loops[li];
            if slot.serial != 0 && !slot.seen {
                if slot.voice != SILENT {
                    self.stop(slot.voice, 0.04);
                }
                if let Some(swbf2) = self.swbf2_mut() {
                    swbf2.weapons.ordnance_loops[li] = OrdnanceLoop::default();
                }
            }
        }

        // Start newly entered rounds in free slots.
        for (ci, &(_, snd_id, _, is_param, item)) in chosen[..chosen_len].iter().enumerate() {
            if matched[ci] {
                continue;
            }
            let free = self
                .swbf2()
                .and_then(|s| s.weapons.ordnance_loops.iter().position(|l| l.serial == 0));
            let Some(li) = free else { break };
            let emitter = emitter_base + li;
            self.update_ordnance_emitter(emitter, item.position, item.velocity);
            let had_room = self.swbf2_loop_has_room(snd_id, false);
            let mut ask = Ask::new(snd_id, if is_param { FOLEY } else { WEAPONS });
            ask.emitter = emitter;
            ask.looping = Some(true);
            let voice = if is_param {
                self.parameterized(ask)
            } else {
                self.play(ask)
            };
            let live = self.playing(voice);
            let mut retry = LoopRetry::default();
            retry.record(live, had_room);
            if let Some(swbf2) = self.swbf2_mut() {
                swbf2.weapons.ordnance_loops[li] = OrdnanceLoop {
                    serial: item.serial,
                    voice: if live { voice } else { SILENT },
                    retry,
                    seen: true,
                };
            }
        }
    }

    fn update_ordnance_emitter(&mut self, emitter: usize, position: Point, velocity: Point) {
        let row = &mut self.emitters[emitter];
        row[e::ID] = emitter as f64;
        row[e::KIND] = 0.0;
        row[e::X..e::X + 3].copy_from_slice(&position);
        row[e::VX..e::VX + 3].copy_from_slice(&velocity);
        self.entity_count = self.entity_count.max(emitter + 1);
    }

    pub(crate) fn swbf2_sound_mute_distance(&self, sound_id: usize) -> f64 {
        let Some(sound) = self.engine.sounds.get(sound_id) else {
            return 0.0;
        };
        if sound.kind == 1 {
            return if sound.mode3d { sound.curve.mute } else { 0.0 };
        }
        if sound.kind == 2 || sound.kind == 3 {
            let Some(def) = self.layers.definitions.get(sound_id) else {
                return 0.0;
            };
            let mut max_mute = 0.0_f64;
            for layer in &def.layers {
                if let Some(child) = self.engine.sounds.get(layer.sound) {
                    if child.mode3d {
                        max_mute = max_mute.max(child.curve.mute);
                    }
                }
            }
            return max_mute;
        }
        0.0
    }

    pub(crate) fn swbf2_weapons(&mut self, dt: f64) {
        // 1. Drain queued space hit, expiry, and destruction one-shots using current listener/time.
        let mut events = [None; MAX_SPACE_AUDIO_EVENTS];
        let event_count = {
            let Some(swbf2) = self.swbf2_mut() else { return };
            let n = swbf2.weapons.audio_events_len;
            for (dst, src) in events[..n]
                .iter_mut()
                .zip(&mut swbf2.weapons.audio_events[..n])
            {
                *dst = src.take();
            }
            swbf2.weapons.audio_events_len = 0;
            n
        };
        for ev in events[..event_count].iter().flatten().copied() {
            match ev {
                SpaceAudioEvent::RoundHit { weapon_hash, position } => {
                    self.play_space_round_hit(weapon_hash, position);
                }
                SpaceAudioEvent::RoundExpired { weapon_hash, position } => {
                    self.play_space_round_expired(weapon_hash, position);
                }
                SpaceAudioEvent::Destruction { class_hash, position } => {
                    self.play_space_destruction(class_hash, position);
                }
            }
        }

        // 2. Sync bounded in-flight ordnance/rocket loops if submitted this frame.
        let mut ord_buf = [SpaceOrdnanceInput {
            serial: 0,
            weapon_hash: 0,
            position: [0.0; 3],
            velocity: [0.0; 3],
        }; MAX_SPACE_ORDNANCE_INPUTS];
        let ord_work = {
            let swbf2 = self.swbf2_mut().unwrap();
            if swbf2.weapons.ordnance_pending {
                let n = swbf2.weapons.ordnance_inputs_len;
                let base = swbf2.weapons.ordnance_emitter_base;
                ord_buf[..n].copy_from_slice(&swbf2.weapons.ordnance_inputs[..n]);
                swbf2.weapons.ordnance_pending = false;
                swbf2.weapons.ordnance_inputs_len = 0;
                Some((base, n))
            } else {
                None
            }
        };
        if let Some((base, n)) = ord_work {
            self.sync_space_ordnance_dt(base, &ord_buf[..n], dt);
        }

        // 3. Process active fighter and ship/chair turret weapons.
        let mut inputs = [None; MAX_SPACE_WEAPONS];
        let count = {
            let Some(swbf2) = self.swbf2_mut() else { return };
            for slot in &mut swbf2.weapons.slots {
                slot.seen = false;
            }
            let n = swbf2.weapons.inputs_len;
            for (dst, src) in inputs[..n]
                .iter_mut()
                .zip(&mut swbf2.weapons.inputs[..n])
            {
                *dst = src.take();
            }
            swbf2.weapons.inputs_len = 0;
            n
        };

        for input in inputs[..count].iter().flatten().copied() {
            let Some(class) = self.swbf2_class_by_hash(input.class_hash) else {
                continue;
            };
            let idx = self.swbf2().and_then(|s| {
                s.weapons
                    .slots
                    .iter()
                    .position(|w| w.key == input.key)
                    .or_else(|| s.weapons.slots.iter().position(|w| w.key == NONE))
            });
            let Some(idx) = idx else { continue };
            let mut st = self.swbf2().unwrap().weapons.slots[idx];

            // Reset state voices cleanly if this slot changed owner or weapon class.
            if st.key != NONE && (st.owner != input.owner || st.class != class) {
                self.stop_space_weapon_slot(&mut st);
            }
            st.key = input.key;
            st.owner = input.owner;
            st.class = class;
            st.seen = true;

            if input.changed {
                let change_snd = self.sound(class, c::CHANGE);
                self.play_sound_protected(change_snd, EFFECTS, input.owner, None, Some(false), input.own);
            }

            // ReloadSound on slot 0.
            if input.reloading && !st.reloading {
                if st.v0 != SILENT && st.v0_kind != 0 {
                    self.stop(st.v0, 0.02);
                }
                let reload_snd = self.sound(class, c::RELOAD);
                st.v0 = self.play_sound_protected(reload_snd, WEAPONS, input.owner, None, None, input.own);
                st.v0_kind = if st.v0 != SILENT { 1 } else { 0 };
            } else if !input.reloading && st.reloading && st.v0_kind == 1 {
                self.stop(st.v0, 0.02);
                st.v0 = SILENT;
                st.v0_kind = 0;
            }

            // OverheatSound on slot 0 with linear pitch sweep from base * OverheatSoundPitch -> base.
            let overheat_snd = self.sound(class, c::OVERHEAT);
            if input.overheated {
                if overheat_snd != NONE {
                    let live = st.v0_kind == 2 && self.playing(st.v0);
                    if live {
                        st.overheat_retry.note_live();
                    } else {
                        if st.v0_kind == 2 {
                            st.v0 = SILENT;
                            st.v0_kind = 0;
                        }
                        let want_start = if !st.overheated {
                            true
                        } else if st.v0_kind != 1 {
                            self.swbf2_loop_should_retry(&mut st.overheat_retry, dt, overheat_snd, input.owner, None, input.own)
                        } else {
                            false
                        };
                        if want_start {
                            if st.v0 != SILENT {
                                self.stop(st.v0, 0.02);
                            }
                            let had_room = self.swbf2_loop_has_room(overheat_snd, input.own);
                            let voice = self.play_sound_protected(
                                overheat_snd,
                                WEAPONS,
                                input.owner,
                                None,
                                Some(true),
                                input.own,
                            );
                            let now_live = self.playing(voice);
                            st.v0 = if now_live { voice } else { SILENT };
                            st.v0_kind = if now_live { 2 } else { 0 };
                            st.overheat_retry.record(now_live, had_room);
                        }
                    }
                }
                if st.v0_kind == 2 && st.v0 != SILENT {
                    let base = self.base_pitch(class, c::OVERHEAT);
                    let low = self.number(class, c::OVERHEAT_PITCH, 1.0);
                    let threshold = input.heat_threshold.max(0.0);
                    let t = (threshold / input.heat.max(0.000001)).min(1.0);
                    self.pitch(st.v0, base * (low + (1.0 - low) * t), true);
                }
            } else {
                st.overheat_retry.reset();
                if st.v0_kind == 2 && st.v0 != SILENT {
                    self.stop(st.v0, 0.05);
                    st.v0 = SILENT;
                    st.v0_kind = 0;
                }
            }

            let fire_stop = self.number(class, c::FIRE_STOP, 0.0) != 0.0;
            let fire_loop_snd = self.sound(class, c::FIRE_LOOP);

            // FireSound on slot 1 + FireLoopSound on slot 0.
            if input.fired {
                if fire_stop && st.v1 != SILENT {
                    self.stop(st.v1, 0.01);
                }
                let fire_snd = self.sound(class, c::FIRE);
                if fire_snd != NONE {
                    st.v1 = self.play_sound_protected(fire_snd, WEAPONS, input.owner, None, None, input.own);
                }
            }

            if input.overheated {
                st.fire_loop_started = false;
                st.fire_loop_retry.reset();
            }
            if input.firing {
                if fire_loop_snd != NONE && !input.overheated && st.v0_kind != 2 {
                    let live = st.v0_kind == 4 && self.playing(st.v0);
                    if live {
                        st.fire_loop_started = true;
                        st.fire_loop_retry.note_live();
                    } else {
                        if st.v0_kind == 4 {
                            st.v0 = SILENT;
                            st.v0_kind = 0;
                        }
                        let want_start = if !st.fire_loop_started {
                            true
                        } else {
                            self.swbf2_loop_should_retry(&mut st.fire_loop_retry, dt, fire_loop_snd, input.owner, None, input.own)
                        };
                        if want_start {
                            if st.v0 != SILENT {
                                self.stop(st.v0, 0.02);
                            }
                            let had_room = self.swbf2_loop_has_room(fire_loop_snd, input.own);
                            st.fire_loop_started = true;
                            let voice = self.play_sound_protected(
                                fire_loop_snd,
                                WEAPONS,
                                input.owner,
                                None,
                                Some(true),
                                input.own,
                            );
                            let now_live = self.playing(voice);
                            st.v0 = if now_live { voice } else { SILENT };
                            st.v0_kind = if now_live { 4 } else { 0 };
                            st.fire_loop_retry.record(now_live, had_room);
                        }
                    }
                }
            } else {
                st.fire_loop_started = false;
                st.fire_loop_retry.reset();
                if st.v0_kind == 4 && st.v0 != SILENT {
                    self.stop(st.v0, 0.05);
                    st.v0 = SILENT;
                    st.v0_kind = 0;
                }
                if st.firing && fire_stop && st.v1 != SILENT {
                    self.stop(st.v1, 0.02);
                    st.v1 = SILENT;
                }
            }

            // Chaingun WarmUpSound & BarrelSound.
            let (warm_sound, warm_time, start_pitch, fade_in, fade_out, barrel_sound, b_fade_in, b_fade_out) =
                self.swbf2()
                    .and_then(|s| s.classes.get(class))
                    .map_or((NONE, 0.5, 0.5, 0.1, 0.1, NONE, 0.05, 0.1), |cls| {
                        (
                            cls.warm_up,
                            cls.warm_up_time.max(0.01),
                            cls.warm_up_start_pitch.clamp(0.1, 4.0),
                            cls.warm_up_fade_in.max(0.0),
                            cls.warm_up_fade_out.max(0.01),
                            cls.barrel,
                            cls.barrel_fade_in.max(0.0),
                            cls.barrel_fade_out.max(0.01),
                        )
                    });

            if input.spinning && warm_sound != NONE {
                if !st.spinning {
                    st.warm_elapsed = 0.0;
                } else {
                    st.warm_elapsed += dt;
                }
                if self.playing(st.warm_voice) {
                    st.warm_retry.note_live();
                } else {
                    st.warm_voice = SILENT;
                    let want_start = if !st.spinning {
                        true
                    } else {
                        self.swbf2_loop_should_retry(&mut st.warm_retry, dt, warm_sound, input.owner, None, input.own)
                    };
                    if want_start {
                        let had_room = self.swbf2_loop_has_room(warm_sound, input.own);
                        let voice = self.play_sound_protected(
                            warm_sound,
                            WEAPONS,
                            input.owner,
                            None,
                            Some(true),
                            input.own,
                        );
                        let now_live = self.playing(voice);
                        st.warm_voice = if now_live { voice } else { SILENT };
                        st.warm_retry.record(now_live, had_room);
                    }
                }
                if st.warm_voice != SILENT {
                    let frac = (st.warm_elapsed / warm_time).clamp(0.0, 1.0);
                    let pitch = start_pitch + (1.0 - start_pitch) * frac;
                    let gain = if fade_in > 0.0 {
                        (st.warm_elapsed / fade_in).clamp(0.0, 1.0)
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
                st.warm_elapsed = 0.0;
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
                        self.swbf2_loop_should_retry(&mut st.barrel_retry, dt, barrel_sound, input.owner, None, input.own)
                    };
                    if want_start {
                        let had_room = self.swbf2_loop_has_room(barrel_sound, input.own);
                        let voice = self.play_sound_protected(
                            barrel_sound,
                            WEAPONS,
                            input.owner,
                            None,
                            Some(true),
                            input.own,
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

            st.reloading = input.reloading;
            st.overheated = input.overheated;
            st.firing = input.firing;
            st.spinning = input.spinning;
            if let Some(swbf2) = self.swbf2_mut() {
                swbf2.weapons.slots[idx] = st;
            }
        }

        // Stop looping state on any slot not refreshed this tick (craft/turret destroyed).
        for idx in 0..MAX_SPACE_WEAPONS {
            let mut st = self.swbf2().unwrap().weapons.slots[idx];
            if st.key != NONE && !st.seen {
                self.stop_space_weapon_slot(&mut st);
                if let Some(swbf2) = self.swbf2_mut() {
                    swbf2.weapons.slots[idx] = SpaceWeaponSlot::default();
                }
            }
        }
    }

    fn stop_space_weapon_slot(&mut self, st: &mut SpaceWeaponSlot) {
        st.fire_loop_started = false;
        st.overheat_retry.reset();
        st.fire_loop_retry.reset();
        st.warm_retry.reset();
        st.barrel_retry.reset();
        if st.v0 != SILENT {
            self.stop(st.v0, 0.05);
            st.v0 = SILENT;
            st.v0_kind = 0;
        }
        if st.v1 != SILENT && st.class != NONE && self.number(st.class, c::FIRE_STOP, 0.0) != 0.0 {
            self.stop(st.v1, 0.03);
            st.v1 = SILENT;
        }
        if st.warm_voice != SILENT {
            self.stop(st.warm_voice, 0.1);
            st.warm_voice = SILENT;
            st.warm_elapsed = 0.0;
        }
        if st.barrel_voice != SILENT {
            self.stop(st.barrel_voice, 0.1);
            st.barrel_voice = SILENT;
        }
        st.barrel_elapsed = 0.0;
    }

    pub(crate) fn stop_swbf2_weapons(&mut self) {
        if self.swbf2().is_none() {
            return;
        }
        for idx in 0..MAX_SPACE_WEAPONS {
            let mut st = self.swbf2().unwrap().weapons.slots[idx];
            if st.key != NONE {
                self.stop_space_weapon_slot(&mut st);
                if let Some(swbf2) = self.swbf2_mut() {
                    swbf2.weapons.slots[idx] = SpaceWeaponSlot::default();
                }
            }
        }
        for li in 0..MAX_SPACE_ORDNANCE_LOOPS {
            let slot = self.swbf2().unwrap().weapons.ordnance_loops[li];
            if slot.voice != SILENT {
                self.stop(slot.voice, 0.05);
            }
            if let Some(swbf2) = self.swbf2_mut() {
                swbf2.weapons.ordnance_loops[li] = OrdnanceLoop::default();
            }
        }
        if let Some(swbf2) = self.swbf2_mut() {
            swbf2.weapons.inputs_len = 0;
            swbf2.weapons.audio_events_len = 0;
            swbf2.weapons.ordnance_pending = false;
            swbf2.weapons.ordnance_inputs_len = 0;
        }
    }
}
