//! What navigation needs to know about a unit (nav/units.js), as a plain
//! snapshot the caller fills from the unit it drives: the fields the
//! JavaScript reads through `unit.*`, `unit.classDef?.*` and
//! `unit.soldier?.*`, each `Option` where the JavaScript's `?.` or a
//! typeof check can find it missing. `forward` and `origin` are the
//! results of the unit's own `forward(out)` / `origin(out)` methods when
//! it has them (executor.js:321, analogs.js:166): computing them belongs
//! to the unit's own package, and a replay serves them from its log.

use super::math::{f32, Vec3};

/// The classifier IDs, as docs/ai.md's unit.kind (units.js:5). Kinds stay
/// f64: the JavaScript compares them with `===` against these numbers.
pub mod kind {
    pub const SOLDIER: f64 = 0.0;
    pub const DROID: f64 = 1.0;
    pub const WALKER: f64 = 2.0;
    pub const DROIDEKA: f64 = 3.0;
    pub const FLYER: f64 = 5.0;
    pub const HOVER: f64 = 6.0;
    pub const TURRET: f64 = 7.0;
    pub const ARMED_BUILDING: f64 = 8.0;
    pub const BUILDING: f64 = 9.0;
    pub const ITEM: f64 = 10.0;
}

/// `unit.classDef?.AISizeType` (units.js:24): a string, an integer, or
/// anything else (which falls back to Soldier).
#[derive(Clone, Debug, PartialEq)]
pub enum SizeType {
    Missing,
    Text(std::rc::Rc<str>),
    /// A JavaScript number; only an integer one is used.
    Number(f64),
}

/// The class fields navigation reads. `bounds` is `[[min], [max]]` when
/// it is an array of length 2 (units.js:40); the entries it indexes can
/// themselves be missing in the JavaScript, which reads as NaN through
/// the arithmetic, so they are kept as written.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClassDef {
    pub ai_size_type: SizeType,
    pub bounds: Option<[[f64; 3]; 2]>,
    pub max_speed: Option<f64>,
    pub max_strafe_speed: Option<f64>,
    pub max_turn_speed: Option<f64>,
    pub max_pitch_speed: Option<f64>,
    pub forward_speed: Option<f64>,
    pub turn_rate: Option<f64>,
    pub jet_fuel_min_border: Option<f64>,
    pub jet_fuel_recharge_rate: Option<f64>,
}

impl Default for SizeType {
    fn default() -> Self {
        SizeType::Missing
    }
}

/// `unit.soldier?.jet` (units.js:70, goto.js:579).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct JetView {
    pub min_border: Option<f64>,
    pub recharge: Option<f64>,
}

/// A soldier's air state as navigation reads it (analogs.js:218,225,231):
/// the strings "jump" and "jet", another string, or nothing (null, which
/// is falsy where a string is not).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Air {
    None,
    Jump,
    Jet,
    Other,
}

/// A soldier's posture string (analogs.js:232-238).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Posture {
    Stand,
    Crouch,
    Prone,
    Other,
}

/// The fields of `unit.soldier` navigation reads. `avoidance` is the
/// collision contact vector (executor.js:257): the executor zeroes it
/// when it uses it, so the caller writes the snapshot's back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoldierView {
    pub aim_yaw: Option<f64>,
    pub aim_pitch: Option<f64>,
    pub max_speed: Option<f64>,
    pub max_strafe: Option<f64>,
    pub max_turn: Option<f64>,
    pub max_pitch: Option<f64>,
    pub avoidance: Option<Vec3>,
    pub fuel: f64,
    pub jet: Option<JetView>,
    pub air: Air,
    pub posture: Posture,
    pub lock: f64,
    pub dive: bool,
}

impl Default for SoldierView {
    fn default() -> Self {
        SoldierView {
            aim_yaw: None,
            aim_pitch: None,
            max_speed: None,
            max_strafe: None,
            max_turn: None,
            max_pitch: None,
            avoidance: None,
            fuel: 0.0,
            jet: None,
            air: Air::None,
            posture: Posture::Stand,
            lock: 0.0,
            dive: false,
        }
    }
}

/// A unit as navigation sees it in one call. `alive_false` is exactly
/// `unit.alive === false` (goto.js:726 tests `!== false`, so a missing
/// alive counts as alive). `id` is the caller's identity token for the
/// unit object: follow targets, leaders and control attachment compare
/// it where the JavaScript compares objects.
#[derive(Clone, Debug, PartialEq)]
pub struct NavUnit {
    pub id: u64,
    /// Native controller actors follow changing bodies. Attachment compares
    /// the qualified body identity while follow queries retain the actor id.
    pub control_identity: Option<u64>,
    pub kind: f64,
    pub position: Vec3,
    pub velocity: Vec3,
    pub generation: Option<f64>,
    pub alive_false: bool,
    pub nav_mask: Option<f64>,
    pub nav_radius: Option<f64>,
    pub class_def: Option<ClassDef>,
    pub soldier: Option<SoldierView>,
    pub forward: Option<Vec3>,
    pub origin: Option<Vec3>,
}

impl NavUnit {
    pub fn new(id: u64, kind: f64, position: Vec3) -> Self {
        NavUnit {
            id,
            control_identity: None,
            kind,
            position,
            velocity: Vec3::default(),
            generation: None,
            alive_false: false,
            nav_mask: None,
            nav_radius: None,
            class_def: None,
            soldier: None,
            forward: None,
            origin: None,
        }
    }
}

/// A reference to a unit across calls: `{ unit, generation }` (goto.js
/// follow targets, control.js leaders). An absent generation matches any
/// (goto.js:726).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitRef {
    pub unit: u64,
    pub generation: Option<f64>,
}

/// AISizeType bits (units.js:20), compared case-insensitively.
fn size_bit(text: &str) -> Option<u32> {
    let text=text.trim_matches(bfe_content::js::is_js_space);
    for (name,bit) in [("soldier",1),("hover",2),("small",4),("medium",8),("huge",16),("flyer",32)] {
        if text.eq_ignore_ascii_case(name) {return Some(bit);}
    }
    None
}

/// Number.isInteger, for the values the JavaScript tests that way.
fn is_integer(x: f64) -> bool {
    x.is_finite() && v8math::trunc(x) == x
}

/// units.js:22.
pub fn traversal_mask(unit: &NavUnit) -> u32 {
    if let Some(mask) = unit.nav_mask {
        if is_integer(mask) {
            return v8math::to_uint32(mask);
        }
    }
    match unit.class_def.as_ref().map(|d| &d.ai_size_type) {
        Some(SizeType::Text(raw)) => size_bit(raw).unwrap_or(1),
        Some(SizeType::Number(raw)) if is_integer(*raw) => v8math::to_uint32(*raw),
        _ => 1,
    }
}

/// units.js:35.
pub const DEFAULT_RADIUS: f64 = 0.476;

/// units.js:37.
pub fn traversal_radius(unit: &NavUnit) -> f64 {
    if let Some(r) = unit.nav_radius {
        return r;
    }
    if let Some(b) = unit.class_def.as_ref().and_then(|d| d.bounds) {
        let dx = b[1][0] - b[0][0];
        let dz = b[1][2] - b[0][2];
        let diagonal = f32(v8math::sqrt(f32(f32(dx * dx) + f32(dz * dz))));
        return f32(diagonal * if unit.kind == kind::HOVER { 0.5 } else { 0.25 });
    }
    DEFAULT_RADIUS
}

/// units.js:51. `def` is `unit.classDef ?? {}`; a present-but-null field
/// in the JavaScript (`??`) falls through, which the Options model.
pub fn nominal_speed(unit: &NavUnit) -> f64 {
    if unit.kind == kind::HOVER {
        return unit.class_def.as_ref().and_then(|d| d.forward_speed).unwrap_or(0.0);
    }
    if let Some(v) = unit.class_def.as_ref().and_then(|d| d.max_speed) {
        return v;
    }
    if let Some(v) = unit.soldier.as_ref().and_then(|s| s.max_speed) {
        return v;
    }
    9.0
}

/// units.js:60.
pub fn is_soldier(unit: &NavUnit) -> bool {
    unit.kind == kind::SOLDIER
}

/// units.js:64.
pub fn is_soldier_or_droid(unit: &NavUnit) -> bool {
    unit.kind == kind::SOLDIER || unit.kind == kind::DROID
}

/// units.js:69: JetFuelRechargeRate > 0 (or the soldier's jet recharge).
pub fn has_jetpack(unit: &NavUnit) -> bool {
    let rate = unit
        .class_def
        .as_ref()
        .and_then(|d| d.jet_fuel_recharge_rate)
        .or_else(|| unit.soldier.as_ref().and_then(|s| s.jet).and_then(|j| j.recharge))
        .unwrap_or(0.0);
    rate > 0.0
}
