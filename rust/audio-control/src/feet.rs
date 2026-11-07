//! A walker's feet (docs/vehicles.md "Effects and sounds"): each
//! TerrainCollision primitive of its class (tools/zero_vehicle.py vehicle_fx
//! `feet`, `stomp`), and when each comes down and lifts, by the rules of the
//! 2004 game's walker foot update:
//!
//! - A foot is where its primitive meets the ground (`bottom`: its centre
//!   less its up axis times half its height), and its height is that
//!   point's over the walker's root.
//! - It comes down (the stomp: the StompEffect, the foot's FootstepSound) by
//!   the class's StompDetectionType. 0, the default: as it falls through
//!   StompThreshold (0.15 m unless set) over the lowest it goes. 1 (the
//!   AT-AT, AT-TE and spider walker): once it falls less than 0.1 m in an
//!   update after falling more. The update runs only while the walker walks
//!   or turns, and standing clears type 1's state, so as it sets off every
//!   foot of a type 1 walker stomps at once.
//! - It lifts (HydraulicSound) as it rises past HydraulicSoundHeight (1 m
//!   unless set), once a rise.
//!
//! The rules read a foot's travel per update, so they hang on the update
//! rate, and the walk cycle plays at one pace in the 2004 game (its
//! AnimationSpeed). The port reads them once per class at 60 updates a
//! second over the walk clips (gait()), as the phases of the cycle at which
//! each foot comes down and lifts, the fall measured over any 1/60 s. A
//! frame's walk phase as drawn (Vehicle.poseCrew: his own hull's predicted
//! cycle, any other's replicated one) passing one of those phases is the
//! step, whatever the frame rate or the records' rate; the place is the
//! foot as drawn. The stomp effect (src/vehicle-effects.js) and the
//! footsteps and hydraulics (rust/audio-control/src/vehicles.rs) each keep their own
//! Footfalls. A walker without a walk cycle falls back on the drawn foot's
//! fall (fall()).
//!
//! Walker sound events from vehicle-feet.js. The cold height samples describe
//! animation assets; rule sampling, arming and frame crossings are native.
use crate::phase::{Phase, index};
use crate::foot_limits as lim;
use crate::engine::{NONE, FOLEY};
use crate::layout::{foot as f, emitter as e, class as c};

#[derive(Clone, Copy)]
pub struct Event { at: f64, kind: u8 }
pub struct Foot { pub point: [f64; 3], pub length: f64, pub detection: bool, pub forward: Vec<Event>, pub backward: Vec<Event> }
#[derive(Clone, Copy, Default)]
pub struct State { previous: Option<f64>, falling: bool, down: bool, armed: bool, generation: Option<f64> }
fn along(at: f64, from: f64, delta: f64, length: f64) -> f64 { let ahead = (((at - from) * delta.signum()) % length + length) % length; if ahead > 0.0 && ahead <= delta.abs() { ahead } else { -1.0 } }
fn sample(h: &[f64], length: f64, phase: f64) -> f64 {
    let fraction = ((phase % length + length) % length) / (length / h.len() as f64); let i = fraction.floor() as usize % h.len();
    h[i] + (h[(i + 1) % h.len()] - h[i]) * (fraction - fraction.floor())
}
impl Foot {
    pub fn new(point: [f64; 3], heights: &[f64], length: f64, detection: bool, threshold: f64, hydraulic: f64, centred: bool, rate: f64) -> Self {
        let schedule = |direction: f64| {
            let mut result = Vec::new(); if heights.is_empty() || length <= 0.0 { return result; }
            let low = heights.iter().copied().fold(f64::INFINITY, f64::min); let level = low + threshold; let lift0 = if centred { low } else { 0.0 };
            // footSchedule multiplied by its fixed update interval. Division can
            // shift a threshold crossing by one grid point for fractional rates.
            let window = rate * (1.0 / 60.0); let steps = heights.len().max((length / window * 16.0).ceil() as usize); let step = length / steps as f64;
            let mut landed = true; let mut armed = None; let mut crossing = false; let mut lifted = false; let mut stomps = Vec::new(); let mut lifts = Vec::new();
            // ports footSchedule: the first cycle settles the detector, and
            // only the second records events. Preserve down-before-arm ties.
            for pass in 0..2 { for k in 0..steps {
                let phase = if direction > 0.0 { k as f64 * step } else { (length - k as f64 * step) % length };
                let h = sample(heights, length, phase); let before = sample(heights, length, phase - direction * window); let mut stomp = false;
                if detection { if landed { if before - h > 0.1 { landed = false; armed = Some(phase); } } else if before - h < 0.1 { stomp = true; landed = true; } }
                else { let crossed = before > h && level > h && before > level; stomp = crossed && !crossing; crossing = crossed; }
                let mut lift = false; if h > before { if h - lift0 > hydraulic && !lifted { lifted = true; lift = true; } } else { lifted = false; }
                if pass == 1 && stomp { stomps.push((phase, if detection { armed } else { None })); } if pass == 1 && lift { lifts.push(phase); }
            } }
            for (phase, arm) in stomps { result.push(Event { at: phase, kind: 1 }); if let Some(at) = arm { result.push(Event { at, kind: 0 }); } }
            for at in lifts { result.push(Event { at, kind: 2 }); } result.truncate(lim::EVENTS); result
        };
        Self { point, length, detection, forward: schedule(1.0), backward: schedule(-1.0) }
    }
}
impl Phase {
    /// ports Footfalls.update/fall/place. Matrices are published presentation
    /// facts; multiplying the primitive's local bottom happens only here.
    pub fn hull_feet(&mut self, owner: usize, dt: f64, near: bool) {
        let row = self.emitters[owner]; let class = index(row[e::CLASS]); let previous = self.entity_state[owner].walk_phase; let was_walking = self.entity_state[owner].walking;
        let phase = row[e::WALK_PHASE]; let walking = row[e::WALK_BLEND] > 0.01;
        self.entity_state[owner].walk_phase = Some(phase); self.entity_state[owner].walking = walking;
        for id in 0..self.foot_count {
            let input = self.feet[id]; if input[f::PRESENT] == 0.0 || index(input[f::OWNER]) != owner { continue; } let number = input[f::INDEX] as usize;
            let Some(definition) = self.foot_definitions.get(class).and_then(|feet| feet.get(number)) else { continue; };
            let point = definition.point; let m = &input[f::M0..=f::M15]; let w = 1.0 / (m[3] * point[0] + m[7] * point[1] + m[11] * point[2] + m[15]);
            let at = [(m[0] * point[0] + m[4] * point[1] + m[8] * point[2] + m[12]) * w, (m[1] * point[0] + m[5] * point[1] + m[9] * point[2] + m[13]) * w, (m[2] * point[0] + m[6] * point[1] + m[10] * point[2] + m[14]) * w];
            let mut state=self.foot_state[id];state.reset_generation(row[e::GENERATION]);
            let mut events=[0u8;lim::EVENTS];
            let count=definition.events(&mut state,previous,was_walking,phase,walking,at[1],dt,&mut events);
            for &kind in &events[..count] {
                if near {self.play_class(class,if kind==1 {c::STEP0+number}else{c::HYDRAULIC},FOLEY,NONE,Some(at),None);}
            }
            self.foot_state[id] = state;
        }
    }
}

impl State {
    /// A reused resource starts a fresh detector, without adopting a phase
    /// or height left by its preceding life.
    pub fn reset_generation(&mut self,generation:f64) {
        if self.generation!=Some(generation) {*self=Self{generation:Some(generation),..Default::default()};}
    }
}
impl Foot {
    /// Footfalls.update/fall, shared by sounds and visual stomp events.
    /// The caller supplies the previously displayed cycle once per hull;
    /// every foot keeps only its own arming and fallback height state.
    pub fn events(&self,state:&mut State,previous:Option<f64>,was_walking:bool,
        phase:f64,walking:bool,height:f64,dt:f64,out:&mut [u8;lim::EVENTS])->usize {
            let mut events = [(0.0, 0u8); lim::EVENTS]; let mut count = 0;
            if self.length > 0.0 {
                if let Some(from) = previous {
                    if walking && !was_walking { state.armed = false; if self.detection { events[0] = (0.0, 1); count = 1; } }
                    else if walking {
                        let length = self.length; let delta = (((phase - from) % length) + length * 1.5) % length - length / 2.0;
                        if delta != 0.0 { for event in if delta > 0.0 { &self.forward } else { &self.backward } {
                            let reach = along(event.at, from, delta, length); if reach < 0.0 || count == events.len() { continue; } let mut slot = count;
                            while slot > 0 && events[slot - 1].0 > reach { events[slot] = events[slot - 1]; slot -= 1; } events[slot] = (reach, event.kind); count += 1;
                        } }
                    }
                }
            } else if dt > 0.0 {
                if let Some(before) = state.previous {
                    let fall = (before - height) / dt;
                    if fall > 0.5 { state.falling = true; state.down = false; }
                    else if state.falling && fall < 0.15 { state.falling = false; state.down = true; events[0] = (0.0, 1); count = 1; }
                    else if state.down && -fall > 0.5 { state.down = false; events[0] = (0.0, 2); count = 1; }
                } state.previous = Some(height);
            }

        let mut kept=0;
        for &(_,kind) in &events[..count] {
            if kind==0 {state.armed=true;continue;}
            if kind==1 && self.detection && !(walking&&!was_walking) {
                if !state.armed {continue;} state.armed=false;
            }
            out[kept]=kind;kept+=1;
        }
        kept
    }
}
