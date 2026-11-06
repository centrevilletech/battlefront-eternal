//! The world's things, as the 2004 game sounds them (docs/formats.md,
//! "Sound"): rounds in flight, command posts, recharge droids, turrets,
//! mines, and recon droids. Everything here is 3D at its thing.
//!
//! - Rounds carry their OrdnanceSound, a loop that follows them until they
//!   hit or run out.
//! - A command post plays its owner side's AmbientSound loop ("rep
//!   com_blg_commandpost_goodie"; nothing while neutral). While it is being
//!   taken it loops ChargeSound, rising in pitch from 0.6 to 1 with the
//!   capture; while two sides contest it, DisputeSound; while an owned one is
//!   being neutralized, DischargeSound, falling from 1 to 0.6. A destroyed
//!   objective (the shield generator's hoth_shield_amb hum) stops.
//! - A recharge droid keeps its AmbientSound going (the FX-7's chatter plays
//!   now and then: each time it ends it is asked for again, at its
//!   PlayProbability).
//! - A turret's gun fires its weapon's FireSound (turretFire). Whoever mans
//!   a placed turret, its activation, whirs and deactivation are a hull's
//!   turret seat's (rust/audio-control/src/vehicles.rs updateBuildings).
//! - A mine plays its landing (CollisionOtherSound) once it sticks. A time
//!   bomb ticks (TickSound, once) each time the whole seconds of its timer
//!   change, so twice at once and then every second, at the sound's pitch x
//!   ((1 - t) x TickSoundPitch + t), t being the share of its LifeSpan left:
//!   with 0.5 it falls from 1 to 0.5.
//! - A recon droid carries AmbientSound (its engine) and Ambient2Sound (its
//!   radio, asked for again each time it ends); its orbital strike's
//!   countdown is the destruct weapon's ChargeSound, which the original game
//!   pitches to 0.4 after the first frame.
//! - Another soldier charging his weapon (a bot's bowcaster) plays its
//!   ChargeSound at him from the charge's start to its release, pitched as
//!   the player's own (rust/audio-control/src/player.rs); a launcher's makes none.
//!
//! SceneSounds' retained loops and edge detectors. Identity rows survive an
//! absent frame so a returning object cannot accidentally restart its mine.
use crate::phase::{Phase, index};
use crate::engine::{Ask, NONE, SILENT, FOLEY, WEAPONS};
use crate::layout::{class as c, emitter as e, weapon as w, post as p, item as it};

#[derive(Clone, Copy)]
pub struct PostState { team: Option<f64>, ambient: u64, state: usize, voice: u64 }
impl Default for PostState { fn default() -> Self { Self { team: None, ambient: SILENT, state: NONE, voice: SILENT } } }
#[derive(Clone, Copy)]
pub struct ItemState { mine: bool, timer: bool, age: f64, seconds: f64 }
impl Default for ItemState { fn default() -> Self { Self { mine: false, timer: false, age: 0.0, seconds: 0.0 } } }
impl Phase {
    /// ports SceneSounds.updateRounds/updateStations/updateCharges/updateDroids
    /// in their original order. Followed voices read the newly published pose
    /// only in the later measurement pass; the acoustics fan sees the last one.
    pub fn scene(&mut self, own: usize, dt: f64) {
        // Each pass tests the kind before touching the rest of a row. Rounds
        // reuse identities, so the table spans far more rows than are live.
        for id in 0..self.entity_count {
            let round = self.emitters[id][e::KIND] == 4.0;
            if !round && self.entity_state[id].mode != 4 { continue; }
            let mut s = self.entity_state[id];
            if round {
                let (class, removed, life) = { let row = &self.emitters[id]; (index(row[e::CLASS]), row[e::REMOVED], row[e::LIFE]) };
                if !s.initialized { s.primary = self.play_class(class, c::ORDNANCE, WEAPONS, id, None, Some(true)); s.initialized = true; }
                if removed != 0.0 || life <= 0.0 { self.stop(s.primary, 0.03); s.initialized = false; }
                s.mode = 4;
            } else { self.stop(s.primary, 0.03); s.initialized = false; }
            self.entity_state[id] = s;
        }
        self.scene_posts();
        for id in 0..self.entity_count {
            if self.emitters[id][e::KIND] != 5.0 { continue; } let row = self.emitters[id]; let s = self.entity_state[id];
            if row[e::ALIVE] == 0.0 { self.stop(s.primary, 0.1); self.entity_state[id].primary = SILENT; }
            else if !self.playing(s.primary) { self.entity_state[id].primary = self.play_class(index(row[e::CLASS]), c::AMBIENT, FOLEY, NONE, self.position(id), None); }
        }
        self.scene_items(dt);
        self.primary_weapons();
        for id in 0..self.entity_count {
            let row = &self.emitters[id]; if row[e::KIND] != 1.0 && row[e::KIND] != 2.0 { if self.entity_state[id].charge_tracked { self.stop(self.entity_state[id].secondary, 0.02); self.entity_state[id].secondary = SILENT; self.entity_state[id].mode = NONE; self.entity_state[id].charge_tracked = false; } continue; }
            let weapon = self.primaries[id]; let mut s = self.entity_state[id];
            let active = id != own && row[e::REMOVED] == 0.0 && row[e::ALIVE] != 0.0 && weapon != NONE && self.weapons[weapon][w::CHARGING] != 0.0 && self.weapons[weapon][w::LAUNCHER] == 0.0;
            if !active { self.stop(s.secondary, 0.02); s.secondary = SILENT; s.mode = NONE; s.charge_tracked = false; }
            else {
                s.charge_tracked = true; let class = index(self.weapons[weapon][w::CLASS]); let base = self.base_pitch(class, c::CHARGE); let low = self.number(class, c::CHARGE_PITCH, 1.0);
                if s.mode != weapon {
                    self.stop(s.secondary, 0.02); s.secondary = self.play_class(class, c::CHARGE, WEAPONS, id, None, None); s.mode = weapon;
                    self.pitch(s.secondary, (base * low).clamp(0.2, 4.0), true);
                } else { let max = self.weapons[weapon][w::MAX_PRESSED]; let t = self.weapons[weapon][w::HELD] / if max == 0.0 { 1.0 } else { max }; self.pitch(s.secondary, (base * (low + (1.0 - low) * t)).clamp(0.2, 0.4), true); }
            }
            self.entity_state[id] = s;
        }
        for id in 0..self.entity_count {
            let live = { let row = &self.emitters[id]; row[e::KIND] == 6.0 && row[e::ALIVE] != 0.0 };
            if !live && self.entity_state[id].mode != 6 { continue; }
            let row = self.emitters[id]; let mut s = self.entity_state[id];
            if !live {
                if s.mode == 6 { self.stop(s.primary, 0.05); self.stop(s.secondary, 0.05); self.stop(s.third, 0.02); s = Default::default(); }
                self.entity_state[id] = s; continue;
            }
            s.mode = 6; let class = index(row[e::CLASS]);
            if !self.playing(s.primary) { s.primary = self.play_class(class, c::AMBIENT, FOLEY, id, None, None); }
            if !self.playing(s.secondary) { s.secondary = self.play_class(class, c::AMBIENT2, FOLEY, id, None, None); }
            let destruct = (0..self.weapon_count).filter(|&weapon| index(self.weapons[weapon][w::OWNER]) == id && self.weapons[weapon][w::STRIKE] != 0.0).min_by(|&a, &b| self.weapons[a][w::ORDINAL].total_cmp(&self.weapons[b][w::ORDINAL])).map_or(NONE, |weapon| index(self.weapons[weapon][w::CLASS]));
            if row[e::COUNTDOWN] >= 0.0 && row[e::SALVO] == 0.0 && destruct != NONE {
                if s.third == SILENT { s.third = self.play_class(destruct, c::CHARGE, WEAPONS, id, None, Some(true)); s.clock = 0.0; }
                let value = self.base_pitch(destruct, c::CHARGE) * self.number(destruct, c::CHARGE_PITCH, 1.0);
                self.pitch(s.third, value.clamp(0.2, if s.clock == 0.0 { 4.0 } else { 0.4 }), true); s.clock += 1.0;
            } else if s.third != SILENT { self.stop(s.third, 0.02); s.third = SILENT; }
            self.entity_state[id] = s;
        }
    }
    fn scene_posts(&mut self) {
        for id in 0..self.post_count {
            let row = self.posts[id]; if row[p::PRESENT] == 0.0 { continue; } let mut s = self.post_state[id];
            if row[p::ACTIVE] == 0.0 { self.stop(s.ambient, 0.2); self.stop(s.voice, 0.1); self.post_state[id] = PostState::default(); continue; }
            let position = Some([row[p::X], row[p::Y], row[p::Z]]); let class = index(row[p::CLASS]);
            if s.team != Some(row[p::TEAM]) {
                s.team = Some(row[p::TEAM]); self.stop(s.ambient, 0.2); s.ambient = SILENT;
                let sound = if row[p::TEAM] == 1.0 { index(row[p::AMBIENT1]) } else if row[p::TEAM] == 2.0 { index(row[p::AMBIENT2]) } else { NONE };
                if sound != NONE { let mut ask = Ask::new(sound, FOLEY); ask.position = position; ask.looping = Some(true); s.ambient = self.play(ask); }
            }
            let counts = self.post_counts[id]; let occupied = counts.iter().any(|n| *n > 0.0);
            let enemies = counts.iter().enumerate().any(|(team, n)| *n > 0.0 && team as f64 != row[p::TEAM]);
            let (state, pitch) = if row[p::TEAM] != 0.0 && enemies { (c::DISCHARGE, 0.6 + 0.4 * (1.0 - row[p::NEUTRALIZE])) }
                else if row[p::TEAM] == 0.0 && row[p::CONTESTED] != 0.0 { (c::DISPUTE, 1.0) }
                else if row[p::TEAM] == 0.0 && row[p::PROGRESS] > 0.0 && occupied { (c::CHARGE, 0.6 + 0.4 * row[p::PROGRESS]) } else { (NONE, 1.0) };
            if state != s.state { self.stop(s.voice, 0.1); s.state = state; s.voice = if state == NONE { SILENT } else { self.play_class(class, state, FOLEY, NONE, position, Some(true)) }; }
            if s.voice != SILENT { self.pitch(s.voice, self.base_pitch(class, state) * pitch, true); } self.post_state[id] = s;
        }
    }
    fn scene_items(&mut self, dt: f64) {
        for id in 0..self.item_count {
            let row = self.items[id]; let mut s = self.item_state[id]; let class = index(row[it::CLASS]); let position = Some([row[it::X], row[it::Y], row[it::Z]]);
            if row[it::PRESENT] != 0.0 && row[it::LANDING_SOURCE] != 0.0 && row[it::MINE] != 0.0 && row[it::LANDED] != 0.0 && !s.mine { s.mine = true; self.play_class(class, c::BOUNCE, FOLEY, NONE, position, None); }
            let span = row[it::SPAN];
            if row[it::PRESENT] == 0.0 || row[it::MINE_BASE] == 0.0 || row[it::GONE] != 0.0 || !(span > 0.0 && span.is_finite()) || self.sound(class, c::TICK) == NONE { s.timer = false; self.item_state[id] = s; continue; }
            if !s.timer { s.timer = true; s.seconds = span.floor(); s.age = 0.0; self.item_tick(class, position, 1.0); } else { s.age += dt; }
            let left = if row[it::LIFE].is_finite() { row[it::LIFE] } else { span - s.age }; let seconds = left.max(0.0).floor();
            if seconds != s.seconds { s.seconds = seconds; self.item_tick(class, position, (left / span).clamp(0.0, 1.0)); }
            self.item_state[id] = s;
        }
    }
    fn item_tick(&mut self, class: usize, position: Option<[f64; 3]>, t: f64) {
        let voice = self.play_class(class, c::TICK, WEAPONS, NONE, position, None); let low = self.number(class, c::TICK_PITCH, 1.0);
        self.pitch(voice, self.base_pitch(class, c::TICK) * ((1.0 - t) * low + t), true);
    }
    pub fn stop_scene(&mut self) {
        for id in 0..self.entity_count { let kind = self.emitters[id][e::KIND]; if kind == 4.0 || kind == 5.0 { self.stop(self.entity_state[id].primary, 0.05); } }
        for id in 0..self.post_count { self.stop(self.post_state[id].ambient, 0.1); self.stop(self.post_state[id].voice, 0.1); }
    }
}
