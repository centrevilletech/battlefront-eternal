//! Shared identity and camera boundary for the owned AI phase.

use crate::gen::{AiLifeHandle, AiPhaseCamera, Store, Unit, Vehicle, KIND_UNIT, KIND_VEHICLE};
use crate::store::Ref;
use super::control::world::CameraView;
use super::random::{Random,SharedRandom};

impl AiLifeHandle {
    /// Resolve at the observation point; a pooled respawn cannot reuse an old life.
    pub fn unit<'a>(&self, store: &'a Store) -> Option<&'a Unit> {
        if self.kind != u32::from(KIND_UNIT) { return None; }
        let unit = store.unit.get(Ref::new(self.slot, self.generation))?;
        (unit.generation == self.life).then_some(unit)
    }

    /// Hull policy reads the F2 slab directly, including its current target life.
    pub fn vehicle<'a>(&self, store: &'a Store) -> Option<&'a Vehicle> {
        if self.kind != u32::from(KIND_VEHICLE) { return None; }
        let hull = store.vehicle.get(Ref::new(self.slot, self.generation))?;
        (hull.target_generation == self.life).then_some(hull)
    }

    /// Storage reuse and respawn invalidate different parts of an AI reference.
    pub fn matches<T>(&self, kind: u32, entity: Ref<T>, life: f64) -> bool {
        self.kind == kind && self.slot == entity.slot && self.generation == entity.gen
            && self.life == life
    }
}

impl From<&AiPhaseCamera> for CameraView {
    fn from(camera: &AiPhaseCamera) -> Self {
        Self {
            elements: camera.elements, fov: camera.fov, aspect: camera.aspect,
            near: camera.near, far: camera.far, zoom: camera.zoom,
        }
    }
}

/// AI streams share A with bot weapons and B with lifecycle commands. These
/// states are held through the unit and vehicle phases, not reseeded per tick.
pub struct RandomState {
    a: SharedRandom,
    b: SharedRandom,
    c: SharedRandom,
    nav_fallback: SharedRandom,
    random_b: bool,
}

impl RandomState {
    pub fn new(seed: u32) -> Self {
        Self::with_random_b(seed, true)
    }

    /// A degraded service bag without B shares A, as ClassicAI construction does.
    pub fn with_random_b(seed: u32, random_b: bool) -> Self {
        Self {
            a: SharedRandom::from_state(seed),
            b: SharedRandom::from_state(seed ^ 0x9e37_79b9),
            c: SharedRandom::from_state(seed ^ 0x51ed_270b),
            nav_fallback: SharedRandom::from_state(0x9e37_79b9),
            random_b,
        }
    }

    /// Loading can follow cold roster/spawn work that already consumed A.
    /// Keep its current state separate from the match seed that initialized
    /// the independent B and C streams; adoption itself draws nothing.
    pub fn from_stream_states(seed:u32,states:[u32;3],random_b:bool)->Self {
        let mut random=Self::with_random_b(seed,random_b);
        random.a=SharedRandom::from_state(states[0]);random.b=SharedRandom::from_state(states[1]);random.c=SharedRandom::from_state(states[2]);random
    }
    pub fn aim_stream(&self)->SharedRandom {self.c.clone()}

    /// Native service frames consume the same streams at their own source
    /// points. This is shared authority, never a copied generator snapshot.
    pub fn share(&self)->Self {
        Self {a:self.a.clone(),b:self.b.clone(),c:self.c.clone(),nav_fallback:self.nav_fallback.clone(),random_b:self.random_b}
    }

    /// Weapon owners bind these same authorities once at world construction.
    pub fn weapon_stream(&self)->SharedRandom {self.a.clone()}
    pub fn command_stream(&self)->SharedRandom {if self.random_b {self.b.clone()}else{self.a.clone()}}

    pub fn draw_counts(&self) -> [u64; 3] { [self.a.draw_count(), self.b.draw_count(), self.c.draw_count()] }
    pub(crate) fn a_mut(&mut self) -> &mut dyn Random { &mut self.a }
    pub(crate) fn b_mut(&mut self) -> &mut dyn Random {
        if self.random_b { &mut self.b } else { &mut self.a }
    }
    pub(crate) fn nav_game_mut(&mut self) -> &mut dyn Random {
        if self.random_b { &mut self.b } else { &mut self.nav_fallback }
    }

    pub fn seeds(&self) -> [u32; 3] { [self.a.seed(), self.b.seed(), self.c.seed()] }

    /// Enhanced aim alone consumes C (enhanced/aim.js).
    pub fn aim_uniform(&mut self) -> f64 {
        self.c.uniform()
    }
}

impl super::control::world::Draws for RandomState {
    fn a_uniform(&mut self) -> f64 {
        self.a.uniform()
    }
    fn b_uniform(&mut self) -> f64 {
        if !self.random_b { return self.a_uniform(); }
        self.b.uniform()
    }
}

impl super::combat::world::Draws for RandomState {
    fn uniform_a(&mut self)->f64 {self.a.uniform()}
    fn uniform_b(&mut self)->f64 {self.b_mut().uniform()}
    fn uniform_c(&mut self)->f64 {self.aim_uniform()}
}
impl super::vehicles::random::Draws for RandomState {
    fn next_a(&mut self)->u32 {Random::next(&mut self.a)}
}

/// F4's roster/spawn pass consumes the AI's raw A draws in place, between
/// the preceding lifecycle callbacks and any newly bound weapon draws.
impl crate::battle::spawning::Draws for RandomState {
    fn next(&mut self)->f64 {Random::next(&mut self.a) as f64}
}
impl crate::battle::spawning::Draws for SharedRandom {
    fn next(&mut self)->f64 {Random::next(self) as f64}
}
