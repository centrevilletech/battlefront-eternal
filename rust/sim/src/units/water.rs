//! The map's water as the world's simulation sees it
//! (src/runtime/water-state.js WaterState): its surface height bobbing on
//! the tick clock, which cells of its grid are water, where a segment goes
//! down through it, and what it does to a soldier each tick (his wakes and
//! the splash of a fall into it, his gun under the surface, drowning).

use super::shapes::arr;
use crate::gen::WaterContact;
use three::Vector3;
use std::collections::HashSet;

const BOB: f64 = 0.06; // m
const BOB_RATE: f64 = 0.3; // Hz
const WAKE_LIFE: f64 = 2.0;
const WAKE_RADIUS: f64 = 0.25;
const SPLASH_SPEED: f64 = 5.0; // m/s down
const SPLASH_GAP: f64 = 1.0; // s
const DROWN_AFTER: f64 = 3.0; // s
const DROWN_DAMAGE: f64 = 30.0; // per second

/// What a soldier's tick in the water asks for besides damage: a wake's
/// ripple (the page's) and the splash of a fall into it (the game's
/// watersplash_md effect), where.
pub trait WaterEffects {
    fn ripple(&mut self, at: &Vector3, life: f64, radius: f64, velocity: &Vector3);
    fn splash(&mut self, at: &Vector3);
}

/// WaterState.
#[derive(Clone, Debug)]
pub struct WaterState {
    pub height: f64,
    pub cell: f64,
    pub offset: [f64; 2],
    pub grid: f64,
    /// `cells`: the water cells' numbers (j * grid + i).
    pub cells: HashSet<u64>,
    pub bobbing: bool,
    pub simulation_time: f64,
    pub physics_wave: f64,
}

impl WaterState {
    /// step(dt): the clock and the bob.
    pub fn step(&mut self, dt: f64) {
        self.simulation_time += dt;
        self.physics_wave = if self.bobbing { BOB * v8math::sin(2.0 * std::f64::consts::PI * BOB_RATE * self.simulation_time) } else { 0.0 };
    }

    /// `level`: the surface's height now, bob included.
    pub fn level(&self) -> f64 {
        self.height + self.physics_wave
    }

    /// over(x, z): whether (x, z) is over a water cell (points off the grid
    /// take the nearest edge cell).
    pub fn over(&self, x: f64, z: f64) -> bool {
        let n = self.grid;
        let i = v8math::min(v8math::max(((x + self.offset[0]) / self.cell).floor(), 0.0), n - 1.0);
        let j = v8math::min(v8math::max(((z + self.offset[1]) / self.cell).floor(), 0.0), n - 1.0);
        let key = j * n + i;
        // A Set of numbers: SameValueZero (NaN never a cell's number here).
        !key.is_nan() && self.cells.contains(&(key + 0.0).to_bits())
    }

    /// crossing(from, to): where a segment goes down through the surface
    /// over water, or None. Things only meet water coming down through it.
    pub fn crossing(&self, from: &Vector3, to: &Vector3) -> Option<Vector3> {
        let level = self.level();
        if !(from.y >= level && to.y < level) {
            return None;
        }
        let t = (from.y - level) / (from.y - to.y);
        let mut out = Vector3::default();
        out.lerp_vectors(from, to, t);
        out.y = level;
        if self.over(out.x, out.z) {
            Some(out)
        } else {
            None
        }
    }

    /// soldierStep(dt, contact, feet, head, gun, velocity, splash): one tick
    /// of a soldier and the water; returns the damage to take.
    #[allow(clippy::too_many_arguments)]
    pub fn soldier_step(&self, dt: f64, contact: &mut WaterContact, feet: &Vector3, head: &Vector3, gun: &Vector3, velocity: &Vector3, effects: &mut dyn WaterEffects) -> f64 {
        self.soldier_step_over(dt, contact, feet, head, gun, velocity, &mut |x, z| self.over(x, z), effects)
    }

    /// soldier_step with `over` answered by `over` (the replays serve it).
    #[allow(clippy::too_many_arguments)]
    pub fn soldier_step_over(
        &self,
        dt: f64,
        contact: &mut WaterContact,
        feet: &Vector3,
        head: &Vector3,
        gun: &Vector3,
        velocity: &Vector3,
        over: &mut dyn FnMut(f64, f64) -> bool,
        effects: &mut dyn WaterEffects,
    ) -> f64 {
        soldier_step_at(self.level(), dt, contact, feet, head, gun, velocity, over, effects)
    }
}

/// WaterState.soldierStep(dt, contact, feet, head, gun, velocity, splash)
/// for water whose level now is `level` and whose cells `over` answers
/// (the module's map rules, rust/sim/src/vehicles/native.rs MapRules, in a
/// battle; a capture in a replay): one tick of a soldier and the water;
/// returns the damage to take.
#[allow(clippy::too_many_arguments)]
pub fn soldier_step_at(
    level: f64,
    dt: f64,
    contact: &mut WaterContact,
    feet: &Vector3,
    head: &Vector3,
    gun: &Vector3,
    velocity: &Vector3,
    over: &mut dyn FnMut(f64, f64) -> bool,
    effects: &mut dyn WaterEffects,
) -> f64 {
    let wet = level > feet.y && over(feet.x, feet.z);
    contact.gun_under = Some(gun.y < level && over(gun.x, gun.z));
    let mut damage = 0.0;
    let or_zero = |x: Option<f64>| x.filter(|v| *v != 0.0 && !v.is_nan()).unwrap_or(0.0);
    if wet {
        let wake = or_zero(contact.wake_clock) + dt;
        contact.wake_clock = Some(wake);
        let interval = 1.0 / (0.3 * velocity.length_sq() + 4.0);
        if wake > interval && velocity.length_sq() > 0.0 {
            let mut at = *feet;
            at.add_scaled_vector(velocity, 0.03);
            effects.ripple(&at, WAKE_LIFE, WAKE_RADIUS, velocity);
            contact.wake_clock = Some(v8math::min(wake - interval, interval));
        }
        let splash = or_zero(contact.splash_clock) + dt;
        contact.splash_clock = Some(splash);
        if velocity.y < -SPLASH_SPEED && splash > SPLASH_GAP {
            contact.splash_clock = Some(0.0);
            effects.splash(&Vector3 { x: feet.x, y: level, z: feet.z });
        }
    }
    if head.y < level && over(head.x, head.z) {
        let mut under = or_zero(contact.under) + dt;
        if under >= DROWN_AFTER {
            under -= 1.0;
            damage = DROWN_DAMAGE;
        }
        contact.under = Some(under);
    } else {
        contact.under = Some(0.0);
    }
    damage
}

#[allow(unused_imports)]
use arr as _arr;

// WaterState's surviving host queries use this immutable grid bank. A released
// facade returns its slot, so changing battles does not retain old map cells.
// These exports never advance water: Game.step's native clock remains sole.
#[cfg(target_arch = "wasm32")]
mod queries {
    use super::*;
    use std::cell::RefCell;
    struct Grid { state: WaterState, numbers: Vec<f64> }
    #[derive(Default)]
    struct Queries { input: Vec<u8>, grids: Vec<Option<Grid>>, point: [f64; 3] }
    thread_local! { static QUERIES: RefCell<Queries> = RefCell::new(Queries::default()); }
    #[no_mangle]
    pub extern "C" fn water_query_input(length: u32) -> *mut u8 {
        QUERIES.with_borrow_mut(|q| { q.input.resize((length as usize).min(16*1024*1024), 0); q.input.as_mut_ptr() })
    }
    #[no_mangle]
    pub extern "C" fn water_query_new() -> u32 {
        QUERIES.with_borrow_mut(|q| {
            let data: serde_json::Value = serde_json::from_slice(&q.input).unwrap_or_default();
            let number = |key: &str| data[key].as_f64().unwrap_or(0.0);
            let grid = number("grid");
            // Set preserves first occurrence order, including duplicate cells
            // in a cold input. Sound/map import reads that original number list.
            let mut numbers=Vec::new();let mut cells=HashSet::new();
            for c in data["cells"].as_array().into_iter().flatten(){
                let number=c[1].as_f64().unwrap_or(0.0)*grid+c[0].as_f64().unwrap_or(0.0);
                if cells.insert((number+0.0).to_bits()){numbers.push(number);}
            }
            let state = WaterState { height: number("height"), cell: number("cell"), grid,
                offset: [data["offset"][0].as_f64().unwrap_or(0.0),data["offset"][1].as_f64().unwrap_or(0.0)],
                cells,
                bobbing: data["look"]["OscillationEnable"][0].as_f64().unwrap_or(1.0)>0.0,
                simulation_time:0.0, physics_wave:0.0 };
            let at = q.grids.iter().position(Option::is_none).unwrap_or(q.grids.len());
            if at == q.grids.len() { q.grids.push(None); }
            q.grids[at] = Some(Grid {state,numbers}); at as u32
        })
    }
    #[no_mangle]
    pub extern "C" fn water_query_release(id:u32) { QUERIES.with_borrow_mut(|q| { if let Some(row)=q.grids.get_mut(id as usize) {*row=None;} }); }
    #[no_mangle]
    pub extern "C" fn water_query_bobbing(id:u32)->u32 { QUERIES.with_borrow(|q| q.grids.get(id as usize).and_then(Option::as_ref).map_or(0,|g|u32::from(g.state.bobbing))) }
    #[no_mangle]
    pub extern "C" fn water_query_cells(id:u32)->*const f64 { QUERIES.with_borrow(|q| q.grids.get(id as usize).and_then(Option::as_ref).map_or(std::ptr::null(),|g|g.numbers.as_ptr())) }
    #[no_mangle]
    pub extern "C" fn water_query_count(id:u32)->u32 { QUERIES.with_borrow(|q| q.grids.get(id as usize).and_then(Option::as_ref).map_or(0,|g|g.numbers.len() as u32)) }
    #[no_mangle]
    pub extern "C" fn water_query_level(height:f64,wave:f64)->f64 { height+wave }
    #[no_mangle]
    pub extern "C" fn water_query_over(id:u32,x:f64,z:f64)->u32 { QUERIES.with_borrow(|q| q.grids.get(id as usize).and_then(Option::as_ref).map_or(0,|g|u32::from(g.state.over(x,z)))) }
    // WaterState.crossing reads the current published level, independently of
    // display dt. Reusing WaterState::crossing keeps exact lerp operation order.
    #[no_mangle]
    pub extern "C" fn water_query_crossing(id:u32,level:f64,ax:f64,ay:f64,az:f64,bx:f64,by:f64,bz:f64)->u32 {
        QUERIES.with_borrow_mut(|q| {
            let Some(grid)=q.grids.get_mut(id as usize).and_then(Option::as_mut) else {return 0;};
            grid.state.height=level;grid.state.physics_wave=0.0;
            let Some(p)=grid.state.crossing(&Vector3::new(ax,ay,az),&Vector3::new(bx,by,bz)) else {return 0;};
            q.point=[p.x,p.y,p.z];1
        })
    }
    #[no_mangle]
    pub extern "C" fn water_query_point()->*const f64 { QUERIES.with_borrow(|q|q.point.as_ptr()) }
}

#[cfg(test)]
mod grid_tests {
    use super::*;
    #[test]
    fn grid_queries_clamp_edges_and_cross_only_downward() {
        let water=WaterState{height:2.,cell:32.,offset:[16.,16.],grid:2.,cells:[0f64.to_bits(),3f64.to_bits()].into_iter().collect(),bobbing:false,simulation_time:0.,physics_wave:0.};
        assert!(water.over(-1000.,-1000.));assert!(water.over(1000.,1000.));assert!(!water.over(-1000.,1000.));
        let from=Vector3::new(20.,3.,20.);let to=Vector3::new(22.,1.,22.);
        assert_eq!(water.crossing(&from,&to),Some(Vector3::new(21.,2.,21.)));
        assert!(water.crossing(&to,&from).is_none());assert!(water.crossing(&Vector3::new(-20.,3.,20.),&Vector3::new(-20.,1.,20.)).is_none());
    }
}
