//! Every vehicle's own sounds, as the 2004 game plays them from a
//! hull's sound class (docs/vehicles.md, "Effects and sounds"): the engine,
//! turning on and off, taking off and landing, being hurt and destroyed,
//! its feet and its turrets. Read off each hull every frame, so the hulls
//! the page simulates, a worker's and a server's sound alike: all this needs
//! is whether it is alive, its health, flight state, velocity, where it is
//! drawn, who holds which seat and where each seat aims.
//!
//! - On: someone takes the pilot's seat of a hull that is off or turning
//!   off. TurnOnSound plays and the engine (EngineSound, a parameterized
//!   loop) starts.
//! - Turning off: the pilot's seat is empty, and a flyer is landed (one
//!   still in the air keeps its engine). TurningOffSound plays (R2's
//!   whistle) and, TurnOffTime (3 s unless set) later, TurnOffSound, and the
//!   engine stops. Meanwhile the engine's TurnOffTime input counts up, which
//!   fades its layers out and drops their pitch.
//! - A flyer that is on plays TakeoffSound (WindUpSound) as it starts to
//!   take off and LandSound (WindDownSound) as it starts to land.
//! - Each sound restarts the clock the engine's TurnOnTime, TurnOffTime,
//!   WindUpTime and WindDownTime inputs read; an input the hull's state
//!   doesn't drive is passed as a raw 1, which leaves its layers alone.
//! - Speed is the speed over MaxSpeed, Health the share of MaxHealth left.
//! - HurtSound when a hit takes more than 2% of the health left (the
//!   soldier's rule, provisional for hulls); DeathSound when it is destroyed,
//!   and its engine stops at once.
//! - A walker's FootstepSound<i> where its i-th foot (TerrainCollision
//!   primitive) comes down, and HydraulicSound as a foot lifts
//!   (src/vehicle-feet.js supplies the shared foot-placement rules).
//! - VehicleCollisionSound when the hull's speed drops by more than its
//!   CollisionThreshold in one simulation tick (1/60 s): a crash, read off the
//!   velocity every host replicates. The threshold applies to one tick; a
//!   page drawing slowly sees several ticks' braking at once,
//!   so the drop is measured against the time it took.
//! - Turret seats (every seat after the pilot's): TurretActivateSound as
//!   someone takes one and TurretDeactivateSound as the seat empties (the
//!   original game compares each turret's controller with last frame's). While
//!   manned, its aim's turning whirs: TurretYawSound and TurretPitchSound at
//!   the sound's gain times the turn rate over the seat's top rate
//!   (TurnRate, PitchRate), pitched from TurretYawSoundPitch
//!   (TurretPitchSoundPitch) up to the sound's own, fading by WHIR_FADE a
//!   frame once it stops.
//! - Placed turrets (src/props.js, the attached game's live objects) are the
//!   same turret, by their class (updateBuildings): whoever mans one, the
//!   player, a bot or another player online, its activation, whirs (the
//!   drum's yaw and pitch, at its MaxTurnSpeed) and deactivation. A
//!   destroyed one falls silent without a sound.
//!
//! An engine, the whirs and the footfalls sound only within the engine
//! layers' MaxDistance of the camera (AUDIBLE until one has played; a placed
//! turret's whirs, AUDIBLE); out there a hull's state still runs, and its
//! engine comes back without a TurnOnSound.
//!
//! Hull and turret sound policy from vehicles.js. Published poses are facts;
//! activation edges, turn-rate history and engine parameter state live here.
use crate::engine::{Ask, NONE, SILENT, FOLEY, EXPLOSIONS};
use crate::phase::{Phase, index};
use crate::layers::Layers;
use crate::layout::{class as c, emitter as e, turret as t};

#[derive(Clone, Copy)]
struct Axis { q: Option<f64>, rest: f64, rate: f64, gain: f64, voice: u64 }
impl Default for Axis { fn default() -> Self { Self { q: None, rest: 0.0, rate: 0.0, gain: 0.0, voice: SILENT } } }
#[derive(Clone, Copy, Default)]
pub struct Turret { initialized: bool, held: bool, axes: [Axis; 2], generation: f64 }
impl Phase {
    fn near_hull(&self, row: &[f64; e::STRIDE]) -> bool {
        let class = index(row[e::CLASS]); let reach = self.reaches.get(class).copied().filter(|n| n.is_finite()).unwrap_or(160.0) + 10.0;
        let p = self.engine.listener.position; let x = row[e::X] - p[0]; let y = row[e::Y] - p[1]; let z = row[e::Z] - p[2]; x * x + y * y + z * z < reach * reach
    }
    /// ports VehicleSounds.updateVehicle/updateEngine, keeping the off timer
    /// test before the clock increment and engine acceleration before speed
    /// history is overwritten. A dead hull ends its engine before its blast.
    pub fn vehicles(&mut self, dt: f64) {
        for id in 0..self.entity_count {
            if self.emitters[id][e::KIND] != 3.0 { continue; } let row = self.emitters[id]; let class = index(row[e::CLASS]); let mut s = self.entity_state[id];
            let speed = crate::length([row[e::VX], row[e::VY], row[e::VZ]]);
            if !s.initialized || s.generation != row[e::GENERATION] {
                self.stop(s.primary, 0.1); self.quiet_hull_turrets(id);
                s = crate::phase::EntityState { initialized: true, generation: row[e::GENERATION], alive: row[e::ALIVE] != 0.0,
                    flight: row[e::FLIGHT], health: row[e::HEALTH], speed, ..Default::default() };
            }
            if row[e::ALIVE] == 0.0 {
                if s.alive { self.stop(s.primary, 0.1); s.primary = SILENT; s.on = false; s.turning_off = false; self.quiet_hull_turrets(id); self.play_class(class, c::DEATH, EXPLOSIONS, NONE, self.position(id), None); }
                s.alive = false; self.entity_state[id] = s; continue;
            }
            s.alive = true; if dt <= 0.0 { self.entity_state[id] = s; continue; }
            let pilot = row[e::PILOTED] != 0.0; let landed = row[e::FLYING] == 0.0 || row[e::FLIGHT] == 1.0;
            if pilot && (!s.on || s.turning_off) { self.play_class(class, c::TURN_ON, FOLEY, id, None, None); s.on = true; s.turning_off = false; s.mode = 1; s.clock = 0.0; }
            else if !pilot && s.on && !s.turning_off && landed { self.play_class(class, c::TURNING_OFF, FOLEY, id, None, None); s.turning_off = true; s.mode = 2; s.clock = 0.0; }
            if row[e::FLYING] != 0.0 && row[e::FLIGHT] != s.flight && s.on {
                if row[e::FLIGHT] == 2.0 && s.flight == 1.0 { self.play_class(class, c::TAKEOFF, FOLEY, id, None, None); s.mode = 3; s.clock = 0.0; }
                else if row[e::FLIGHT] == 4.0 && s.flight != 4.0 { self.play_class(class, c::LAND, FOLEY, id, None, None); s.mode = 4; s.clock = 0.0; }
            }
            s.bump += dt; let drop = (s.speed - speed) * (1.0 / 60.0) / dt.max(1.0 / 60.0);
            let threshold = if row[e::COLLISION_DROP].is_finite() && row[e::COLLISION_DROP] != 0.0 { row[e::COLLISION_DROP] } else { 5.0 };
            if drop > threshold && (row[e::FLYING] == 0.0 || row[e::FLIGHT] == 3.0 && s.flight == 3.0) && s.bump > 0.3 { self.play_class(class, c::COLLISION, FOLEY, id, None, None); s.bump = 0.0; }
            s.flight = row[e::FLIGHT]; if s.health - row[e::HEALTH] > 0.02 * s.health { self.play_class(class, c::HURT, FOLEY, id, None, None); } s.health = row[e::HEALTH];
            let near = self.near_hull(&row);
            self.entity_state[id] = s;
            self.hull_feet(id, dt, near);
            s.walk_phase = self.entity_state[id].walk_phase; s.walking = self.entity_state[id].walking;
            if s.turning_off && s.clock >= self.number(class, c::TURN_OFF_TIME, 3.0) {
                self.play_class(class, c::TURN_OFF, FOLEY, id, None, None); self.stop(s.primary, 0.05); s.primary = SILENT; s.on = false; s.turning_off = false; s.mode = 0;
            }
            let acceleration = (speed - s.speed) / dt; s.speed = speed;
            if !s.on || !near { self.stop(s.primary, 0.15); s.primary = SILENT; }
            else {
                s.retry -= dt;
                if !self.playing(s.primary) && s.retry <= 0.0 {
                    let mut ask = Ask::new(self.sound(class, c::ENGINE), FOLEY); ask.emitter = id; ask.looping = Some(true); s.primary = self.parameterized(ask); s.retry = 1.0;
                    let mut reach: f64 = 0.0;
                    if Layers::is_group(s.primary) { for i in 0..self.layers.count(s.primary) { reach = reach.max(self.engine.reach(self.layers.child(s.primary, i))); } }
                    else { reach = self.engine.reach(s.primary); }
                    if reach > 0.0 { self.reaches[class] = reach; }
                }
                if Layers::is_group(s.primary) {
                    let mut inputs = [(1.0, true, true); 7];
                    inputs[0] = (if row[e::SPEED_LIMIT] > 0.0 { speed / row[e::SPEED_LIMIT] } else { 0.0 }, false, true);
                    inputs[1] = (acceleration, false, true); inputs[2] = (if row[e::MAX_HEALTH] > 0.0 { row[e::HEALTH] / row[e::MAX_HEALTH] } else { 1.0 }, false, true);
                    for i in 3..7 { if s.mode == i - 2 { inputs[i] = (s.clock, false, true); } }
                    self.inputs(s.primary, &inputs, false);
                } else {
                    // VehicleSounds.updateEngine calls apply on a plain Voice
                    // as well as on a parameterized engine. That early apply
                    // precedes measurement and advances Classic's glide again.
                    self.apply_handle(s.primary);
                }
            }
            s.clock += dt; self.entity_state[id] = s;
            self.hull_turrets(id, dt, near);
        }
        for id in 0..self.turret_count { if index(self.turrets[id][t::OWNER]) == NONE && self.turrets[id][t::PRESENT] != 0.0 { self.turret(id, dt, true); } }
    }
    fn hull_turrets(&mut self, owner: usize, dt: f64, near: bool) { for id in 0..self.turret_count { if index(self.turrets[id][t::OWNER]) == owner && self.turrets[id][t::PRESENT] != 0.0 { self.turret(id, dt, near); } } }
    fn quiet_hull_turrets(&mut self, owner: usize) {
        for id in 0..self.turret_count { if index(self.turrets[id][t::OWNER]) == owner { let mut s = self.turret_state[id]; self.quiet_turret(&mut s); s.initialized = false; self.turret_state[id] = s; } }
    }
    fn quiet_turret(&mut self, s: &mut Turret) { for a in &mut s.axes { self.stop(a.voice, 0.05); a.voice = SILENT; a.q = None; } }
    /// ports VehicleSounds.turret/whir. Quantised angles retain the rest
    /// interval across replicated frames, and stopped gain decays per frame,
    /// rather than being converted into a different time-based envelope.
    fn turret(&mut self, id: usize, dt: f64, mut near: bool) {
        let row = self.turrets[id]; let class = index(row[t::CLASS]); let owner = index(row[t::OWNER]); let mut s = self.turret_state[id]; let held = row[t::HELD] != 0.0 && row[t::ALIVE] != 0.0;
        let generation = if owner == NONE { 0.0 } else { self.emitters[owner][e::GENERATION] };
        if !s.initialized || s.generation != generation { s = Turret { initialized: true, held, generation, ..Default::default() }; self.turret_state[id] = s; return; }
        if row[t::ALIVE] == 0.0 { s.held = false; }
        let position = Some([row[t::X], row[t::Y], row[t::Z]]);
        if held != s.held { s.held = held; self.play_class(class, if held { c::ACTIVATE } else { c::DEACTIVATE }, FOLEY, NONE, position, None); }
        if owner == NONE { let p = self.engine.listener.position; let x = row[t::X] - p[0]; let y = row[t::Y] - p[1]; let z = row[t::Z] - p[2]; near = x * x + y * y + z * z < 170.0 * 170.0; }
        if !held || !near { self.quiet_turret(&mut s); self.turret_state[id] = s; return; }
        for axis in 0..2 {
            let mut a = s.axes[axis]; let angle = row[if axis == 0 { t::YAW } else { t::PITCH }]; let angle = if angle.is_nan() { 0.0 } else { angle }; let q = v8math::round(angle * 1000.0);
            let turning;
            if let Some(before) = a.q { if q != before { a.rate = (q - before).abs() / 1000.0 / (a.rest + dt); a.q = Some(q); a.rest = 0.0; turning = true; } else { a.rest += dt; turning = a.rest <= 0.1 && a.rate > 0.0; } }
            else { a.q = Some(q); turning = false; }
            let key = if axis == 0 { c::YAW } else { c::PITCH }; let sound_id = self.sound(class, key);
            if let Some(sound) = self.engine.sounds.get(sound_id) {
                let base = sound.gain.max(0.000001); let pitch = sound.pitch; let low = self.number(class, if axis == 0 { c::YAW_PITCH } else { c::PITCH_PITCH }, 0.8);
                if turning {
                    let top = row[if axis == 0 { t::YAW_TOP } else { t::PITCH_TOP }]; let fraction = (a.rate / top.max(0.001)).min(1.0);
                    if !self.playing(a.voice) { a.voice = self.play_class(class, key, FOLEY, self.turret_emitter_base + id, None, Some(true)); a.gain = base; } else { a.gain = (a.gain + 0.1 * base).min(base); }
                    self.gain(a.voice, (a.gain * fraction / base).clamp(0.0, 1.0 / base));
                    self.pitch(a.voice, ((low + (1.0 - low) * fraction) * pitch).max(low).min(pitch), true);
                } else if a.voice != SILENT { a.gain *= 0.6; self.gain(a.voice, a.gain / base); if a.gain < 0.0001 { self.stop(a.voice, 0.01); a.voice = SILENT; a.rate = 0.0; } }
            }
            s.axes[axis] = a;
        }
        self.turret_state[id] = s;
    }
    pub fn stop_vehicles(&mut self) {
        for id in 0..self.entity_count { if self.emitters[id][e::KIND] == 3.0 { self.stop(self.entity_state[id].primary, 0.05); self.entity_state[id].primary = SILENT; } }
        for id in 0..self.turret_count { let mut s = self.turret_state[id]; self.quiet_turret(&mut s); self.turret_state[id] = s; }
    }
}
