//! EnvironmentWakes.update/add/clear from environment-volume.js. Identity is
//! numeric, so reordering the input never pairs one actor with another's trail.
use crate::layout::{wake_actor as actor, wake_frame as frame, wake_result as out, wakes as lim};

#[derive(Clone, Copy, Default)]
struct State { id: f64, position: [f64; 3], gap: f64, seen: u64 }
pub struct Wakes {
    pub frame: Vec<f64>,
    pub actors: Vec<f64>,
    pub result: Vec<f64>,
    pub points: Vec<f64>,
    pub motion: Vec<f64>,
    states: Vec<State>,
    count: usize,
    time: f64,
    cursor: u64,
    serial: u64,
    overflow: u64,
}
impl Default for Wakes { fn default() -> Self { Self::new() } }
impl Wakes {
    pub fn new() -> Self {
        let mut points = vec![0.0; lim::COUNT * 4];
        for p in points.chunks_exact_mut(4) { p[3] = -100.0; }
        Self { frame: vec![0.0; frame::STRIDE], actors: vec![0.0; lim::ACTORS * actor::STRIDE],
            result: vec![0.0; out::STRIDE], points, motion: vec![0.0; lim::COUNT*4],
            states: vec![State::default(); lim::STATES], count: 0, time: 0.0, cursor: 0, serial: 0, overflow: 0 }
    }
    pub fn clear(&mut self) {
        // EnvironmentWakes.clear kept time and cursor, but forgot every actor's
        // preceding position. A mode switch must not manufacture a long wake.
        for p in self.points.chunks_exact_mut(4) { p[3] = -100.0; }
        self.count = 0;
    }
    fn add(&mut self, position: [f64;3], velocity: [f64;3], strength: f64) {
        let at = self.cursor as usize % lim::COUNT * 4; self.cursor += 1;
        self.points[at..at+3].copy_from_slice(&position); self.points[at+3] = self.time;
        self.motion[at..at+3].copy_from_slice(&velocity); self.motion[at+3] = strength;
    }
    // Resource tools can submit clear/add commands without advancing a frame.
    // Live placed effects clear in their mode transition before update instead.
    // Both forms publish the same points and preserve time and cursor.
    pub fn apply(&mut self) {
        if self.frame[frame::SET_TIME] != 0.0 { self.time = self.frame[frame::TIME]; self.frame[frame::SET_TIME] = 0.0; }
        if self.frame[frame::CLEAR] != 0.0 { self.clear(); self.frame[frame::CLEAR] = 0.0; }
        if self.frame[frame::ADD] != 0.0 {
            self.add([self.frame[frame::POSITION],self.frame[frame::POSITION+1],self.frame[frame::POSITION+2]],
                [self.frame[frame::VELOCITY],self.frame[frame::VELOCITY+1],self.frame[frame::VELOCITY+2]],self.frame[frame::STRENGTH]);
            self.frame[frame::ADD] = 0.0;
        }
        self.result[out::TIME] = self.time; self.result[out::CURSOR] = self.cursor as f64; self.result[out::OVERFLOW] = self.overflow as f64;
    }
    // EnvironmentWakes.update observes every supplied actor, even if irrelevant.
    // Updating its last position prevents an offscreen-to-onscreen jump from
    // manufacturing a long trail when it returns beside a steam source.
    pub fn update(&mut self, relevant: impl Fn([f64;3]) -> bool) {
        self.apply();
        let dt = self.frame[frame::DT]; self.time += dt; self.serial += 1;
        let count = (self.frame[frame::ACTORS] as usize).min(lim::ACTORS);
        if self.frame[frame::ACTORS] > lim::ACTORS as f64 { self.overflow += 1; }
        for i in 0..count {
            let row = &self.actors[i*actor::STRIDE..(i+1)*actor::STRIDE];
            let allowed = row[actor::RELEVANT] != 0.0;
            let id = row[actor::ID]; if id <= 0.0 || !id.is_finite() { continue; }
            let p = [row[actor::POSITION],row[actor::POSITION+1],row[actor::POSITION+2]];
            let at = if let Some(at) = self.states[..self.count].iter().position(|s| s.id == id) { at }
                else if self.count < lim::STATES {
                    let at = self.count; self.count += 1; self.states[at] = State { id, position: p, gap: 0.0, seen: self.serial }; at
                } else {
                    // More actor identities than a battle can contain: recycle
                    // the least recently observed one instead of growing or
                    // failing. A fresh identity starts at its own position.
                    self.overflow += 1;
                    let at = self.states.iter().enumerate().min_by_key(|(_,s)| s.seen).map(|(i,_)|i).unwrap_or(0);
                    self.states[at] = State { id, position: p, gap: 0.0, seen: self.serial }; at
                };
            // A newly admitted actor begins at its current position. Its first
            // observation contributes no speed, while its gap still advances.
            // The minimum dt protects paused observations without emitting.
            let mut state = self.states[at];
            let inverse = 1.0 / dt.max(1e-5);
            let velocity = [(p[0]-state.position[0])*inverse,(p[1]-state.position[1])*inverse,(p[2]-state.position[2])*inverse];
            let speed = (velocity[0]*velocity[0]+velocity[1]*velocity[1]+velocity[2]*velocity[2]).sqrt();
            state.gap += dt;
            if dt > 0.0 && state.gap >= 0.10 && speed > 0.3 && speed < 60.0 && allowed && relevant(p) {
                self.add(p,velocity,(0.4+speed*0.1).min(1.6)); state.gap = 0.0;
            }
            // Teleports and stationary actors retain their growing gap, as in
            // the JavaScript owner. Only an actual wake resets that timer.
            // The old WeakMap identity is now a bounded, id-qualified record.
            state.position = p; state.seen = self.serial; self.states[at] = state;
        }
        self.result[out::TIME] = self.time; self.result[out::CURSOR] = self.cursor as f64; self.result[out::OVERFLOW] = self.overflow as f64;
    }
}
