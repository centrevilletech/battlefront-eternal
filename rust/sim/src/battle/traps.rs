//! Log trap simulation (src/traps.js): building, arming, triggering and
//! crushing. Loading places the trigger and effect points and supplies
//! the cooked class settings. The world keeps the hulls and collision
//! bodies; animation and page presentation stay outside this state machine.

use crate::store::StrId;
use super::stats::Text;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Down,
    Reset,
    Armed,
    Swing,
}

impl Phase {
    pub fn index(self) -> f64 {
        match self { Self::Down => 0.0, Self::Reset => 1.0, Self::Armed => 2.0, Self::Swing => 3.0 }
    }
}

/// Class data after the JavaScript constructor's defaults and placement.
#[derive(Clone, Debug)]
pub struct Settings {
    pub max_health: f64,
    pub reach: f64,
    pub min_radius: f64,
    pub death_time: f64,
    pub build_frames: Option<f64>,
    pub trigger_frames: Option<f64>,
    pub trigger_team: Option<StrId>,
    pub sound: Option<StrId>,
    pub death_effect: Option<StrId>,
    pub point: [f64; 3],
    pub effect_at: [f64; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trap {
    pub built: bool,
    pub health: f64,
    pub phase: Phase,
    pub started: Option<f64>,
    pub time: f64,
    pub sprung: bool,
    pub tick: f64,
    pub crushed: f64,
}

impl Default for Trap {
    fn default() -> Self {
        Self { built: false, health: 0.0, phase: Phase::Down, started: Some(0.0), time: 0.0, sprung: false, tick: 0.0, crushed: 0.0 }
    }
}

/// Hull reads and effects, in JavaScript's call order. `sphere` supplies
/// blastSphere's answer, or position and radius when there is no method.
/// `show` enables the matching collision bodies and shows the build pad.
pub trait World {
    type Vehicle: Copy;
    fn alive(&mut self, vehicle: Self::Vehicle) -> bool;
    fn sphere(&mut self, vehicle: Self::Vehicle) -> ([f64; 3], f64);
    fn side(&mut self, vehicle: Self::Vehicle) -> Option<StrId>;
    fn clear_attacker(&mut self, trap: &mut Trap, vehicle: Self::Vehicle);
    fn set_health(&mut self, trap: &mut Trap, vehicle: Self::Vehicle, health: f64);
    fn set_dying(&mut self, trap: &mut Trap, vehicle: Self::Vehicle, time: f64);
    fn hide_root(&mut self, trap: &mut Trap, vehicle: Self::Vehicle);
    fn show(&mut self, trap: &mut Trap, built: bool);
    fn sound(&mut self, trap: &mut Trap, name: StrId, at: [f64; 3]);
    fn effect(&mut self, trap: &mut Trap, name: StrId, at: [f64; 3]);
}

// Trap.setHealth only changes visibility; it never crushes a hull or emits
// a sound. Combat can share that arithmetic without entering the trap phase.
pub trait HealthWorld {
    fn health_show(&mut self, trap: &mut Trap, built: bool);
}
impl<W: World> HealthWorld for W {
    fn health_show(&mut self, trap: &mut Trap, built: bool) { self.show(trap, built); }
}

fn text(value: Option<StrId>, strings: &dyn Text) -> Option<StrId> {
    value.filter(|&name| strings.nonempty(name))
}

impl Trap {
    pub fn begin(&mut self, phase: Phase, tick: Option<f64>) {
        self.phase = phase;
        self.started = tick;
        self.time = 0.0;
    }

    pub fn set_health(&mut self, value: f64, settings: &Settings, world: &mut impl World) {
        self.set_health_with(value, settings.max_health, world);
    }

    // Trap.setHealth retains built at zero health, marking it sprung for the
    // later world step. Rebuilding starts Reset with no tick assigned yet.
    pub fn set_health_with(&mut self, value: f64, max_health: f64, world: &mut impl HealthWorld) {
        self.health = v8math::max(0.0, v8math::min(max_health, value));
        // V8's Math.min/max return this canonical NaN for clamp. Keep
        // its bits when health is read back through a typed-array view.
        if self.health.is_nan() { self.health = f64::from_bits(0xfff8_0000_0000_0000); }
        if self.built {
            if !(self.health > 0.0) { self.sprung = true; }
            return;
        }
        if self.health >= max_health {
            self.built = true;
            world.health_show(self, true);
            self.health = max_health;
            self.sprung = false;
            self.begin(Phase::Reset, None);
        }
    }

    pub fn catches<W: World>(&self, settings: &Settings, vehicle: W::Vehicle, world: &mut W) -> bool {
        if !world.alive(vehicle) { return false; }
        let (center, radius) = world.sphere(vehicle);
        if !(radius > settings.min_radius) { return false; }
        let r = settings.reach + radius;
        (center[0] - settings.point[0]).abs() <= r
            && (center[1] - settings.point[1]).abs() <= r
            && (center[2] - settings.point[2]).abs() <= r
    }

    pub fn crush<W: World>(&mut self, settings: &Settings, strings: &dyn Text, vehicles: &[W::Vehicle], world: &mut W) {
        // Array.filter finishes before the first death callback. One
        // victim's death can change another hull, but not this selection.
        let victims: Vec<_> = vehicles.iter().copied().filter(|&v| self.catches(settings, v, world)).collect();
        let mut killed = 0.0;
        for vehicle in victims {
            world.clear_attacker(self, vehicle);
            world.set_health(self, vehicle, 0.0);
            world.set_dying(self, vehicle, 0.0);
            world.hide_root(self, vehicle);
            killed += 1.0;
        }
        self.crushed += killed;
        if killed != 0.0 {
            if let Some(name) = text(settings.death_effect, strings) { world.effect(self, name, settings.effect_at); }
        }
    }

    pub fn step<W: World>(&mut self, dt: f64, tick: f64, settings: &Settings, strings: &dyn Text, vehicles: &[W::Vehicle], world: &mut W) {
        self.tick = tick;
        if !self.built {
            if self.phase != Phase::Down { self.begin(Phase::Down, Some(tick)); }
            return;
        }
        if self.started.is_none() { self.started = Some(tick); }
        if self.phase == Phase::Down { self.begin(Phase::Reset, Some(tick)); }
        if self.phase == Phase::Reset {
            self.time += dt;
            if self.time >= settings.build_frames.map_or(0.0, |frames| frames / 30.0) {
                self.begin(Phase::Armed, Some(tick));
            }
            return;
        }
        if self.phase == Phase::Armed {
            let triggered = self.sprung || text(settings.trigger_team, strings).is_some_and(|side| {
                vehicles.iter().copied().any(|v| self.catches(settings, v, world) && world.side(v) == Some(side))
            });
            if !triggered { return; }
            self.begin(Phase::Swing, Some(tick));
            if let Some(sound) = text(settings.sound, strings) { world.sound(self, sound, settings.point); }
        }
        if settings.death_time > 0.0 && self.time < settings.death_time && self.time + dt >= settings.death_time {
            self.crush(settings, strings, vehicles, world);
        }
        self.time += dt;
        if self.time >= settings.trigger_frames.map_or(0.0, |frames| frames / 30.0) {
            self.sprung = false;
            self.health = 0.0;
            self.built = false;
            world.show(self, false);
            self.begin(Phase::Down, Some(tick));
        }
    }

    pub fn state_record(&self) -> [f64; 3] {
        [self.phase.index(), self.started.unwrap_or(self.tick), if self.sprung { 1.0 } else { 0.0 }]
    }
}
