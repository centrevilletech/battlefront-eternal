//! 2005 (`GameFamily::Swbf2`) hero power, active choke-hold, and thrown
//! lightsaber audio controllers. Zero heap allocations on steady-state ticks.

use super::{LoopRetry, SABER_CATEGORY};
use crate::engine::{Ask, EXPLOSIONS, NONE, SILENT, WEAPONS};
use crate::layout::{class as c, emitter as e};
use crate::phase::Phase;
use crate::Point;

pub const MAX_POWER_AUDIO_EVENTS: usize = 128;
pub const MAX_POWER_HOLDS: usize = 32;
pub const MAX_SABER_THROWS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerAudioKind {
    Cast,
    Hit,
    Thrown,
    Blocked,
    Held,
    Buffed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PowerAudioEvent {
    /// Stable caster key `(slot << 32) | generation`, matching `PowerHoldInput::key`.
    pub caster_key: u64,
    /// Audio emitter index when live in `Phase::emitters`, else `NONE` (plays at `caster_pos`).
    pub emitter: usize,
    /// `zero_hash` of the power weapon class (or melee blade class for combo blasts).
    pub weapon_hash: u32,
    /// `zero_hash` of the defender's blocking blade class on `Blocked`, else `0`.
    pub target_weapon_hash: u32,
    pub kind: PowerAudioKind,
    pub caster_pos: Point,
    pub target_pos: Option<Point>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PowerHoldInput {
    /// Stable hold key `(slot << 32) | generation`.
    pub key: u64,
    /// Audio emitter index when live in `Phase::emitters`, else `NONE`.
    pub emitter: usize,
    /// `zero_hash` of the held power weapon class (e.g. Force Choke).
    pub weapon_hash: u32,
    pub caster_pos: Point,
    pub target_pos: Point,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SaberThrowInput {
    /// Stable throw key (e.g. `(slot << 32) | (launched & 0xffff_ffff)`).
    pub key: u64,
    /// Thrower's audio emitter index (`NONE` to play launch placed at `position`).
    pub owner: usize,
    /// `zero_hash` of the throw weapon class.
    pub weapon_hash: u32,
    pub position: Point,
    pub velocity: Point,
}

#[derive(Clone, Copy)]
struct PowerHoldSlot {
    key: u64,
    weapon_hash: u32,
    voice: u64,
    retry: LoopRetry,
    seen: bool,
}

impl Default for PowerHoldSlot {
    fn default() -> Self {
        Self {
            key: 0,
            weapon_hash: 0,
            voice: SILENT,
            retry: LoopRetry::default(),
            seen: false,
        }
    }
}

#[derive(Clone, Copy)]
struct SaberThrowSlot {
    key: u64,
    twirl_voice: u64,
    retry: LoopRetry,
    seen: bool,
}

impl Default for SaberThrowSlot {
    fn default() -> Self {
        Self {
            key: 0,
            twirl_voice: SILENT,
            retry: LoopRetry::default(),
            seen: false,
        }
    }
}

pub struct Swbf2PowerBank {
    events: [Option<PowerAudioEvent>; MAX_POWER_AUDIO_EVENTS],
    events_len: usize,
    holds: [PowerHoldSlot; MAX_POWER_HOLDS],
    hold_inputs: [PowerHoldInput; MAX_POWER_HOLDS],
    hold_inputs_len: usize,
    hold_emitter_base: usize,
    holds_pending: bool,
    throws: [SaberThrowSlot; MAX_SABER_THROWS],
    throw_inputs: [SaberThrowInput; MAX_SABER_THROWS],
    throw_inputs_len: usize,
    throw_emitter_base: usize,
    throws_pending: bool,
}

impl Default for Swbf2PowerBank {
    fn default() -> Self {
        Self {
            events: [None; MAX_POWER_AUDIO_EVENTS],
            events_len: 0,
            holds: [PowerHoldSlot::default(); MAX_POWER_HOLDS],
            hold_inputs: [PowerHoldInput {
                key: 0,
                emitter: NONE,
                weapon_hash: 0,
                caster_pos: [0.0; 3],
                target_pos: [0.0; 3],
            }; MAX_POWER_HOLDS],
            hold_inputs_len: 0,
            hold_emitter_base: 0,
            holds_pending: false,
            throws: [SaberThrowSlot::default(); MAX_SABER_THROWS],
            throw_inputs: [SaberThrowInput {
                key: 0,
                owner: NONE,
                weapon_hash: 0,
                position: [0.0; 3],
                velocity: [0.0; 3],
            }; MAX_SABER_THROWS],
            throw_inputs_len: 0,
            throw_emitter_base: 0,
            throws_pending: false,
        }
    }
}

impl Swbf2PowerBank {
    pub fn push_event(&mut self, event: PowerAudioEvent) {
        if self.events_len < MAX_POWER_AUDIO_EVENTS {
            self.events[self.events_len] = Some(event);
            self.events_len += 1;
        }
    }

    pub fn set_holds(&mut self, emitter_base: usize, active: &[PowerHoldInput]) {
        let n = active.len().min(MAX_POWER_HOLDS);
        self.hold_emitter_base = emitter_base;
        self.hold_inputs[..n].copy_from_slice(&active[..n]);
        self.hold_inputs_len = n;
        self.holds_pending = true;
    }

    pub fn set_throws(&mut self, emitter_base: usize, active: &[SaberThrowInput]) {
        let n = active.len().min(MAX_SABER_THROWS);
        self.throw_emitter_base = emitter_base;
        self.throw_inputs[..n].copy_from_slice(&active[..n]);
        self.throw_inputs_len = n;
        self.throws_pending = true;
    }

    pub fn active_hold_count(&self) -> usize {
        self.holds.iter().filter(|h| h.key != 0).count()
    }

    pub fn active_throw_count(&self) -> usize {
        self.throws.iter().filter(|t| t.key != 0).count()
    }
}

impl Phase {
    /// Queues one hero power event (cast, hit, thrown, blocked) to play during `Phase::step`.
    pub fn push_power_event(&mut self, event: PowerAudioEvent) {
        if let Some(swbf2) = self.swbf2_mut() {
            swbf2.powers.push_event(event);
        }
    }

    /// Queues the current frame's active area-power holds (such as Force Choke).
    pub fn queue_power_holds(&mut self, emitter_base: usize, active: &[PowerHoldInput]) {
        if let Some(swbf2) = self.swbf2_mut() {
            swbf2.powers.set_holds(emitter_base, active);
        }
    }

    /// Queues the current frame's active in-flight thrown lightsabers.
    pub fn queue_saber_throws(&mut self, emitter_base: usize, active: &[SaberThrowInput]) {
        if let Some(swbf2) = self.swbf2_mut() {
            swbf2.powers.set_throws(emitter_base, active);
        }
    }

    pub(crate) fn swbf2_powers(&mut self, dt: f64) {
        // 1. Sync continuous choke/area holds first so Cast events can avoid double-playing.
        let mut hold_buf = [PowerHoldInput {
            key: 0,
            emitter: NONE,
            weapon_hash: 0,
            caster_pos: [0.0; 3],
            target_pos: [0.0; 3],
        }; MAX_POWER_HOLDS];
        let hold_work = {
            let Some(swbf2) = self.swbf2_mut() else { return };
            if swbf2.powers.holds_pending {
                let n = swbf2.powers.hold_inputs_len;
                let base = swbf2.powers.hold_emitter_base;
                hold_buf[..n].copy_from_slice(&swbf2.powers.hold_inputs[..n]);
                swbf2.powers.holds_pending = false;
                swbf2.powers.hold_inputs_len = 0;
                Some((base, n))
            } else {
                None
            }
        };
        if let Some((base, n)) = hold_work {
            self.sync_power_holds(base, &hold_buf[..n], dt);
        }

        // 2. Drain discrete power events (Push, Pull, Lightning cast+impact, Auras, Combo Blasts).
        let mut events = [None; MAX_POWER_AUDIO_EVENTS];
        let event_count = {
            let Some(swbf2) = self.swbf2_mut() else { return };
            let n = swbf2.powers.events_len;
            for (dst, src) in events[..n]
                .iter_mut()
                .zip(&mut swbf2.powers.events[..n])
            {
                *dst = src.take();
            }
            swbf2.powers.events_len = 0;
            n
        };
        for ev in events[..event_count].iter().flatten().copied() {
            self.handle_power_event(ev);
        }

        // 3. Sync active thrown sabers (launch FireSound + moving OrdnanceSound twirl loop).
        let mut throw_buf = [SaberThrowInput {
            key: 0,
            owner: NONE,
            weapon_hash: 0,
            position: [0.0; 3],
            velocity: [0.0; 3],
        }; MAX_SABER_THROWS];
        let throw_work = {
            let Some(swbf2) = self.swbf2_mut() else { return };
            if swbf2.powers.throws_pending {
                let n = swbf2.powers.throw_inputs_len;
                let base = swbf2.powers.throw_emitter_base;
                throw_buf[..n].copy_from_slice(&swbf2.powers.throw_inputs[..n]);
                swbf2.powers.throws_pending = false;
                swbf2.powers.throw_inputs_len = 0;
                Some((base, n))
            } else {
                None
            }
        };
        if let Some((base, n)) = throw_work {
            self.sync_saber_throws(base, &throw_buf[..n], dt);
        }
    }

    fn sync_power_holds(&mut self, emitter_base: usize, active: &[PowerHoldInput], dt: f64) {
        if let Some(swbf2) = self.swbf2_mut() {
            for slot in &mut swbf2.powers.holds {
                slot.seen = false;
            }
        }

        for &item in active {
            if item.key == 0 || item.weapon_hash == 0 {
                continue;
            }
            let Some(wcls) = self.swbf2_class_by_hash(item.weapon_hash) else {
                continue;
            };
            let fire_loop = self.sound(wcls, c::FIRE_LOOP);
            let (snd, is_fire_loop) = if fire_loop != NONE {
                (fire_loop, true)
            } else {
                (self.sound(wcls, c::FIRE), false)
            };
            if snd == NONE {
                continue;
            }
            let idx = self.swbf2().and_then(|s| {
                s.powers
                    .holds
                    .iter()
                    .position(|h| h.key == item.key && h.weapon_hash == item.weapon_hash)
                    .or_else(|| s.powers.holds.iter().position(|h| h.key == 0))
            });
            let Some(idx) = idx else { continue };

            let emitter = if item.emitter < self.entity_count && item.emitter != NONE {
                item.emitter
            } else if emitter_base + idx < self.emitters.len() {
                let eid = emitter_base + idx;
                self.update_power_emitter(eid, item.caster_pos, [0.0; 3]);
                eid
            } else {
                NONE
            };

            let existing = self.swbf2().unwrap().powers.holds[idx];
            let own = crate::phase::index(self.frame[crate::layout::frame::OWN]);
            let is_own = own != NONE && item.emitter == own;
            let mut retry = existing.retry;
            // Start once when the hold begins: respect the sound's authored Looping
            // property unless FireLoopSound was authored explicitly. Only true
            // FireLoopSound holds recover if refused or stolen; one-shot FireSound
            // holds (such as Force Choke) never replay after finishing.
            let voice = if existing.key == 0 {
                let had_room = self.swbf2_loop_has_room(snd, is_own);
                let mut ask = Ask::new(snd, WEAPONS);
                if emitter != NONE {
                    ask.emitter = emitter;
                } else {
                    ask.position = Some(item.caster_pos);
                }
                if is_fire_loop {
                    ask.looping = Some(true);
                }
                ask.protected = is_own;
                let v = self.play(ask);
                if is_fire_loop {
                    let live = self.playing(v);
                    retry.record(live, had_room);
                    if live { v } else { SILENT }
                } else {
                    retry.reset();
                    v
                }
            } else if is_fire_loop {
                if self.playing(existing.voice) {
                    retry.note_live();
                    existing.voice
                } else {
                    let pos_opt = if emitter == NONE { Some(item.caster_pos) } else { None };
                    if self.swbf2_loop_should_retry(&mut retry, dt, snd, emitter, pos_opt, is_own) {
                        let had_room = self.swbf2_loop_has_room(snd, is_own);
                        let mut ask = Ask::new(snd, WEAPONS);
                        if emitter != NONE {
                            ask.emitter = emitter;
                        } else {
                            ask.position = Some(item.caster_pos);
                        }
                        ask.looping = Some(true);
                        ask.protected = is_own;
                        let v = self.play(ask);
                        let live = self.playing(v);
                        retry.record(live, had_room);
                        if live { v } else { SILENT }
                    } else {
                        SILENT
                    }
                }
            } else {
                existing.voice
            };

            if let Some(swbf2) = self.swbf2_mut() {
                swbf2.powers.holds[idx] = PowerHoldSlot {
                    key: item.key,
                    weapon_hash: item.weapon_hash,
                    voice,
                    retry,
                    seen: true,
                };
            }
        }

        for i in 0..MAX_POWER_HOLDS {
            let slot = self.swbf2().unwrap().powers.holds[i];
            if slot.key != 0 && !slot.seen {
                if slot.voice != SILENT {
                    self.stop(slot.voice, 0.08);
                }
                if let Some(swbf2) = self.swbf2_mut() {
                    swbf2.powers.holds[i] = PowerHoldSlot::default();
                }
            }
        }
    }

    fn handle_power_event(&mut self, ev: PowerAudioEvent) {
        let wcls = self.swbf2_class_by_hash(ev.weapon_hash);
        let valid_emitter = ev.emitter != NONE && ev.emitter < self.entity_count;
        let own = crate::phase::index(self.frame[crate::layout::frame::OWN]);
        let is_own = own != NONE && ev.emitter == own;
        match ev.kind {
            PowerAudioKind::Cast => {
                let Some(wcls) = wcls else { return };
                // Skip if an active hold loop already owns this caster's choke sound.
                let held_now = ev.caster_key != 0
                    && self.swbf2().is_some_and(|s| {
                        s.powers
                            .holds
                            .iter()
                            .any(|h| h.key == ev.caster_key && h.weapon_hash == ev.weapon_hash)
                    });
                if held_now {
                    return;
                }
                let fire_snd = self.sound(wcls, c::FIRE);
                if fire_snd != NONE {
                    if valid_emitter {
                        self.play_sound_protected(fire_snd, WEAPONS, ev.emitter, None, Some(false), is_own);
                    } else {
                        self.play_sound_protected(fire_snd, WEAPONS, NONE, Some(ev.caster_pos), Some(false), is_own);
                    }
                    return;
                }
                // Combo blast (e.g. Mace ground slam, Sidious dark-side blast): the melee
                // blade class names no FireSound and links directly to its ExplosionName.
                let exp_cls = self
                    .swbf2()
                    .and_then(|s| s.classes.get(wcls))
                    .map_or(NONE, |c| c.explosion_class);
                if exp_cls != NONE {
                    let exp_snd = self.sound(exp_cls, c::EXPLOSION);
                    self.play_sound_protected(exp_snd, EXPLOSIONS, NONE, Some(ev.caster_pos), Some(false), is_own);
                }
            }
            PowerAudioKind::Hit | PowerAudioKind::Thrown => {
                let Some(wcls) = wcls else { return };
                // Play ordnance collision/explosion impacts; combo blasts (which have no
                // FireSound) already played once on Cast.
                let (ord_cls, wpn_exp) = self
                    .swbf2()
                    .and_then(|s| s.classes.get(wcls))
                    .map_or((NONE, NONE), |c| (c.ordnance_class, c.explosion_class));
                let pos = ev.target_pos.unwrap_or(ev.caster_pos);
                let col_snd = if ord_cls != NONE { self.sound(ord_cls, c::IMPACT) } else { NONE };
                if col_snd != NONE {
                    self.play_sound_at(col_snd, WEAPONS, NONE, Some(pos), Some(false));
                }
                let ord_exp = if ord_cls != NONE {
                    self.swbf2()
                        .and_then(|s| s.classes.get(ord_cls))
                        .map_or(NONE, |c| c.explosion_class)
                } else {
                    NONE
                };
                let exp_cls = if ord_exp != NONE {
                    ord_exp
                } else if self.sound(wcls, c::FIRE) != NONE {
                    wpn_exp
                } else {
                    NONE
                };
                if exp_cls != NONE {
                    let exp_snd = self.sound(exp_cls, c::EXPLOSION);
                    if exp_snd != NONE && exp_snd != col_snd {
                        self.play_sound_at(exp_snd, WEAPONS, NONE, Some(pos), Some(false));
                    }
                }
            }
            PowerAudioKind::Blocked => {
                let pos = ev.target_pos.unwrap_or(ev.caster_pos);
                let block_snd = self.swbf2().map_or(NONE, |s| {
                    let from_target = if ev.target_weapon_hash != 0 {
                        s.class_by_hash(ev.target_weapon_hash)
                            .and_then(|c| s.classes.get(c))
                            .map_or(NONE, |c| c.saber_deflect)
                    } else {
                        NONE
                    };
                    if from_target != NONE {
                        from_target
                    } else {
                        wcls.and_then(|c| s.classes.get(c))
                            .map_or(NONE, |c| c.saber_deflect)
                    }
                });
                self.play_sound_at(block_snd, SABER_CATEGORY, NONE, Some(pos), Some(false));
            }
            PowerAudioKind::Held | PowerAudioKind::Buffed => {}
        }
    }

    fn sync_saber_throws(&mut self, emitter_base: usize, active: &[SaberThrowInput], dt: f64) {
        if emitter_base + MAX_SABER_THROWS > self.emitters.len() {
            return;
        }
        if let Some(swbf2) = self.swbf2_mut() {
            for slot in &mut swbf2.powers.throws {
                slot.seen = false;
            }
        }

        for &item in active {
            if item.key == 0 || item.weapon_hash == 0 {
                continue;
            }
            let Some(wcls) = self.swbf2_class_by_hash(item.weapon_hash) else {
                continue;
            };
            let idx = self.swbf2().and_then(|s| {
                s.powers
                    .throws
                    .iter()
                    .position(|t| t.key == item.key)
                    .or_else(|| s.powers.throws.iter().position(|t| t.key == 0))
            });
            let Some(idx) = idx else { continue };

            let eid = emitter_base + idx;
            self.update_power_emitter(eid, item.position, item.velocity);

            let existing = self.swbf2().unwrap().powers.throws[idx];
            let mut retry = existing.retry;
            let twirl_snd = self
                .swbf2()
                .and_then(|s| s.classes.get(wcls))
                .map_or(NONE, |c| c.ordnance_class)
                .then_some_or_none(|ord| self.sound(ord, c::ORDNANCE));

            let twirl_voice = if existing.key == 0 {
                // Brand-new saber throw launch: play FireSound at thrower
                // and start looping OrdnanceSound following the blade.
                let throw_snd = self.sound(wcls, c::FIRE);
                if item.owner != NONE && item.owner < self.entity_count {
                    self.play_sound_at(throw_snd, SABER_CATEGORY, item.owner, None, Some(false));
                } else {
                    self.play_sound_at(throw_snd, SABER_CATEGORY, NONE, Some(item.position), Some(false));
                }
                if twirl_snd != NONE {
                    let had_room = self.swbf2_loop_has_room(twirl_snd, false);
                    let v = self.play_sound_at(twirl_snd, SABER_CATEGORY, eid, None, Some(true));
                    let live = self.playing(v);
                    retry.record(live, had_room);
                    if live { v } else { SILENT }
                } else {
                    retry.reset();
                    SILENT
                }
            } else if twirl_snd != NONE {
                if self.playing(existing.twirl_voice) {
                    retry.note_live();
                    existing.twirl_voice
                } else if self.swbf2_loop_should_retry(&mut retry, dt, twirl_snd, eid, None, false) {
                    let had_room = self.swbf2_loop_has_room(twirl_snd, false);
                    let v = self.play_sound_at(twirl_snd, SABER_CATEGORY, eid, None, Some(true));
                    let live = self.playing(v);
                    retry.record(live, had_room);
                    if live { v } else { SILENT }
                } else {
                    SILENT
                }
            } else {
                existing.twirl_voice
            };

            if let Some(swbf2) = self.swbf2_mut() {
                swbf2.powers.throws[idx] = SaberThrowSlot {
                    key: item.key,
                    twirl_voice,
                    retry,
                    seen: true,
                };
            }
        }

        for i in 0..MAX_SABER_THROWS {
            let slot = self.swbf2().unwrap().powers.throws[i];
            if slot.key != 0 && !slot.seen {
                if slot.twirl_voice != SILENT {
                    self.stop(slot.twirl_voice, 0.06);
                }
                if let Some(swbf2) = self.swbf2_mut() {
                    swbf2.powers.throws[i] = SaberThrowSlot::default();
                }
            }
        }
    }

    fn update_power_emitter(&mut self, emitter: usize, position: Point, velocity: Point) {
        let row = &mut self.emitters[emitter];
        row[e::ID] = emitter as f64;
        row[e::KIND] = 0.0;
        row[e::X..e::X + 3].copy_from_slice(&position);
        row[e::VX..e::VX + 3].copy_from_slice(&velocity);
        self.entity_count = self.entity_count.max(emitter + 1);
    }
}

trait MapValidIndex {
    fn then_some_or_none(self, f: impl FnOnce(usize) -> usize) -> usize;
}

impl MapValidIndex for usize {
    fn then_some_or_none(self, f: impl FnOnce(usize) -> usize) -> usize {
        if self == NONE { NONE } else { f(self) }
    }
}
