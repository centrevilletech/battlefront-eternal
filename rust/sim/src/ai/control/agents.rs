//! The Classic agents' data (agents.js): the table sine/cosine, the
//! degraded direct helper, and each agent's state. Behavior is the
//! core's dispatch (mod.rs), where the whole roster is in reach.

use super::controller::SourceGoal;
use super::fsm::Fsm;
use super::patrol::Patrol;
use super::world::{EntRef, ForeignKind, HelperKind, HelperObs, JobRef, UKey};
use super::super::nav::math::Vec3;
use std::sync::OnceLock;

/// The class jobs' embedded machines also tick through the core.
#[derive(Clone, Debug, PartialEq)]
pub struct Snipe {
    pub fsm: Fsm,
    pub scout: bool,
    pub hint: Option<usize>,
    pub stance: f64,
    pub look: bool,
    pub look_point: Vec3,
    /// classes.js's behavior helper rather than the agents.js base.
    pub full: bool,
    pub retry_at: Option<f64>,
    pub revalidate_at: f64,
    pub block_aim: bool,
}

impl Snipe {
    pub fn new(scout: bool, full: bool) -> Self {
        Snipe {
            fsm: Fsm { state: 1.0, pending: 1.0 },
            scout,
            hint: None,
            stance: 0.0,
            look: false,
            look_point: Vec3::default(),
            full,
            retry_at: None,
            revalidate_at: 0.0,
            block_aim: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mine {
    pub fsm: Fsm,
    pub full: bool,
    pub hint: Option<usize>,
    pub placement: Option<Vec3>,
    pub aim_point: Option<Vec3>,
    pub fired_at: Option<f64>,
    pub retry_at: Option<f64>,
}

impl Mine {
    pub fn new(full: bool) -> Self {
        Mine { fsm: Fsm { state: 1.0, pending: 1.0 }, full, hint: None, placement: None, aim_point: None, fired_at: None, retry_at: None }
    }
}

/// agent.service (classes.js startRepairService).
#[derive(Clone, Debug, PartialEq)]
pub struct ServiceState {
    pub approach_at: f64,
    pub progress_at: f64,
    pub saved_health: f64,
    pub toward: Option<Vec3>,
    pub closing: bool,
    pub workable: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectiveFlavor {
    Trooper,
    Scout,
    Repair,
}

/// The objective agents' shared shape (UnitTrooperAgent and its scout
/// and repair subclasses; `enhanced` swaps in the Enhanced policies).
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectiveAgent {
    pub flavor: ObjectiveFlavor,
    pub enhanced: bool,
    pub post: Option<usize>,
    pub patrol: Patrol,
    pub snipe: Snipe,
    pub mine: Mine,
    pub start_position: Vec3,
    pub center: Vec3,
    pub offset: Vec3,
    pub entry_time: f64,
    pub support: bool,
    /// The mounted patrol's repeated support-point waits.
    pub support_holds: Option<f64>,
    pub mine_tries: f64,
    // scout
    pub deadline: f64,
    // repair
    pub job_at: f64,
    pub job: Option<JobRef>,
    pub service: Option<ServiceState>,
}

impl ObjectiveAgent {
    pub fn new(flavor: ObjectiveFlavor, enhanced: bool, post: Option<usize>, classes: bool) -> Self {
        ObjectiveAgent {
            flavor,
            enhanced,
            post,
            patrol: Patrol::new(),
            snipe: Snipe::new(flavor == ObjectiveFlavor::Scout, classes),
            mine: Mine::new(classes),
            start_position: Vec3::default(),
            center: Vec3::default(),
            offset: Vec3::default(),
            entry_time: 0.0,
            support: false,
            support_holds: None,
            mine_tries: 0.0,
            deadline: 0.0,
            job_at: -1.0,
            job: None,
            service: None,
        }
    }
}

/// InterruptHelper (interrupts.js:62).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Interrupts {
    pub checked_at: f64,
    pub charge: bool,
    pub last: f64,
}

/// DirectHelper (agents.js:98), the degraded straight-line helper.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Direct {
    pub target: Vec3,
    pub active: bool,
    pub arrived: bool,
    pub failed: bool,
    pub status: f64,
    pub has_path: bool,
    pub radius: f64,
}

impl Default for Direct {
    fn default() -> Self {
        Direct { target: Vec3::default(), active: false, arrived: false, failed: false, status: 0.0, has_path: false, radius: 1.0 }
    }
}

/// An agent's movement helper: its kind, what calls left observable,
/// and the direct helper's state when it is one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Helper {
    pub kind: Option<HelperKind>,
    pub obs: HelperObs,
    pub direct: Direct,
}

impl Default for Helper {
    fn default() -> Self {
        Helper { kind: None, obs: HelperObs::default(), direct: Direct::default() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AgentKind {
    Wait { since: f64, look_point: Vec3 },
    WaitSecondary,
    Random { start: Vec3, last_retry: f64, retried_at: Vec3 },
    Follow { leader: Option<EntRef>, leader_point: Vec3, countdown: f64 },
    Hold { countdown: f64, look_point: Vec3 },
    Board { vehicle: Option<EntRef> },
    Unboard,
    Objective(Box<ObjectiveAgent>),
    Foreign(ForeignKind),
    MissingCombat,
}

/// The agent base (agents.js:295 Agent). `tracker` is the combat
/// package's (written by initAgent); the core never reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct Agent {
    pub fsm: Fsm,
    pub unit: Option<UKey>,
    pub goal: Option<SourceGoal>,
    pub interrupts: Interrupts,
    pub helper: Helper,
    pub kind: AgentKind,
}

impl Agent {
    pub fn done(&self) -> bool {
        self.fsm.done()
    }
    pub fn helper_status(&self) -> f64 {
        match self.helper.kind {
            None => 0.0,
            Some(HelperKind::Direct) => self.helper.direct.status,
            Some(_) => self.helper.obs.status,
        }
    }
    pub fn helper_has_path(&self) -> bool {
        match self.helper.kind {
            None => false,
            Some(HelperKind::Direct) => self.helper.direct.has_path,
            Some(_) => self.helper.obs.has_path,
        }
    }
    pub fn helper_arrived(&self) -> bool {
        match self.helper.kind {
            None => false,
            Some(HelperKind::Direct) => self.helper.direct.arrived,
            Some(_) => self.helper.obs.arrived,
        }
    }
    pub fn helper_failed(&self) -> bool {
        match self.helper.kind {
            None => false,
            Some(HelperKind::Direct) => self.helper.direct.failed,
            Some(_) => self.helper.obs.failed,
        }
    }
}

// ---------------------------------------------------------------- table trig

struct Tables {
    sin: [f64; 2049],
    cos: [f64; 2049],
}

static TABLES: OnceLock<Tables> = OnceLock::new();

fn tables() -> &'static Tables {
    TABLES.get_or_init(|| {
        let mut sin = [0.0f64; 2049];
        let mut cos = [0.0f64; 2049];
        let step = v8math::fround((2.0 * std::f64::consts::PI) / 2048.0);
        for i in 0..2048usize {
            let a = v8math::fround(i as f64 * step);
            sin[i] = v8math::fround(v8math::sin(a));
            cos[i] = v8math::fround(v8math::cos(a));
        }
        Tables { sin, cos }
    })
}

/// tableSinCos (agents.js:41).
pub fn table_sin_cos(angle: f64) -> (f64, f64) {
    let t = tables();
    let scale = v8math::fround(2048.0 / (2.0 * std::f64::consts::PI));
    let x = v8math::fround(angle * scale);
    let whole = v8math::trunc(x);
    let frac = v8math::fround(x - whole);
    let wi = v8math::to_int32(whole);
    let i = (wi & 2047) as usize;
    let j = ((wi + 1) & 2047) as usize;
    let keep = v8math::fround(1.0 - frac);
    let sin = v8math::fround(v8math::fround(t.sin[i] * keep) + v8math::fround(t.sin[j] * frac));
    let cos = v8math::fround(v8math::fround(t.cos[i] * keep) + v8math::fround(t.cos[j] * frac));
    (sin, cos)
}

/// rotateY (agents.js:56).
pub fn rotate_y(v: Vec3, angle: f64) -> Vec3 {
    let (sin, cos) = table_sin_cos(angle);
    Vec3::new(v.x * cos - v.z * sin, v.y, v.z * cos + v.x * sin)
}

/// randomCube (agents.js:65): z, y, x draws.
pub fn random_cube(draw: &mut dyn FnMut() -> f64) -> Vec3 {
    let z = v8math::fround(draw() - 0.5);
    let y = v8math::fround(draw() - 0.5);
    let x = v8math::fround(draw() - 0.5);
    Vec3::new(x * 2.0, y * 2.0, z * 2.0)
}

pub fn horizontal_distance_sq(a: Vec3, b: Vec3) -> f64 {
    let dx = a.x - b.x;
    let dz = a.z - b.z;
    dx * dx + dz * dz
}

/// nominalSpeed (agents.js:87).
pub fn nominal_speed(unit: Option<&super::world::AiUnit>) -> f64 {
    let Some(unit) = unit else { return 0.0 };
    if let Some(s) = &unit.soldier {
        if let Some(v) = s.max_speed {
            return v;
        }
    }
    if let Some(c) = &unit.class_def {
        if let Some(v) = c.max_speed {
            return v;
        }
        if let Some(v) = c.forward_speed {
            return v;
        }
    }
    0.0
}
