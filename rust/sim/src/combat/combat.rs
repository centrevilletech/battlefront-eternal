//! A soldier's side of the fight (PlayerCombat, with,
//! and src/heroes.js regenerate): his aim,
//! his triggers and weapons, the rounds, items, droids and lightning they
//! fire, a repair tool's lock and a launcher's, the aim's kick, what
//! explosions and hurts do to him, his death and a droid's blowing apart.

use super::impacts::{BlastPacket, HealthScale, ImpactWorld};
use super::defs::{WeaponBase, Js, JsObj, WeaponClass, WeaponDefs, K};
use super::weapons::{arm_into, Aim, Arsenal, ArsenalInput, Shot, ShotKind, Trigger, DEG};
use super::world::{ArcTarget, CombatWorld, DamageInfo, LockPick, Presentation};
use crate::gen::{Air, Attacker, Combat, Kick, PendingShot, Posture, Seen, Unit, UnitBody};
use crate::store::{DefId, Nullable};
use jsrt::conv::{js_max, js_min};
use bfe_presentation::cameras::soldier::Section;
use std::rc::Rc;
use three::Vector3;

const AIM_REACH: f64 = 60.0;
const DROIDEKA_REACH: f64 = 512.0;
const RESPAWN_DELAY: f64 = 4.0;
const THROWN_SPEED: f64 = 2.0;
/// The speed a blast that kills a soldier on foot sends his body away from it
/// (the 2004 game adds it whether or not the explosion pushes).
const BLAST_DEATH_SPEED: f64 = 5.0;
const KICK_RATE: f64 = 10.0;
const KICK_FADE: f64 = 20.0;
// PlayerCombat.updateKick keeps the original exponential fade constants.
// Their separate accumulators serve aim pitch and drawn recoil.
// Combining the rates would change repeated-shot aim and lockout timing.
// Tiny values snap only after each tick's original decay operations.
const KICK_OFFSET_FADE: f64 = 6.0;
const KICK_HOLD_DRAIN: f64 = 2.0;
const UP: Vector3 = Vector3 { x: 0.0, y: 1.0, z: 0.0 };

/// The aim pivot and gun origin per posture (the original PlayerCombat implementation).
fn pivot_of(p: Posture) -> [f64; 3] {
    match p {
        Posture::Stand => [0.06, 1.7, 0.0],
        Posture::Crouch => [-0.1, 1.2, 0.0],
        Posture::Prone => [0.06, 0.6, 0.4],
    }
}

fn gun_of(p: Posture) -> [f64; 3] {
    match p {
        Posture::Stand => [-0.12, -0.1, 0.25],
        Posture::Crouch => [-0.12, -0.05, 0.25],
        Posture::Prone => [-0.12, -0.2, 0.15],
    }
}

/// BLAST_BODY: the sphere a blast measures to, by stance
/// (its centre above the feet, its radius) and the sight point's height.
pub fn blast_body(p: Posture) -> [f64; 3] {
    match p {
        Posture::Stand => [1.0, 1.4626, 1.4],
        Posture::Crouch => [0.75, 1.1023, 0.6],
        Posture::Prone => [0.35, 1.622, 0.2],
    }
}

/// The content combat reads: weapon classes, and unit classes (their
/// objects, by DefId in the battle's unitClass table).
#[derive(Clone, Debug, Default)]
pub struct CombatDefs {
    pub weapons: WeaponDefs,
    pub classes: Vec<Option<Rc<JsObj>>>,
    facts: Vec<ClassFacts>,
}

#[derive(Clone, Debug, Default)]
struct ClassFacts { add_health: f64, walkerdroid: bool, loadout:Vec<DefId>, cameras: Option<[Section; 6]>, sphere: Option<ModelSphere> }

/// What a blast and a mine's trigger measure a droideka to: the sphere
/// around its model box, carried with its body (the centre in the body's
/// frame: x to its left, z ahead), the same whether it stands or rolls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ModelSphere { pub center: [f64; 3], pub radius: f64 }

impl ModelSphere {
    /// Where its centre is for a body at `position` facing `yaw`.
    pub(crate) fn carried(&self, position: [f64; 3], yaw: f64) -> [f64; 3] {
        let [x, y, z] = self.center;
        let (sin, cos) = v8math::sin_cos(yaw);
        [position[0] + cos * x + sin * z, position[1] + y, position[2] - sin * x + cos * z]
    }
}

/// The sphere of a class's model box (`bounds`: its lowest and highest
/// corners): the box's centre and half its diagonal.
fn model_sphere(bounds: &Js) -> Option<ModelSphere> {
    let (low, high) = (bounds.at(0), bounds.at(1));
    let corner = |c: &Js| [c.at(0).num(), c.at(1).num(), c.at(2).num()];
    let (low, high) = (corner(low), corner(high));
    if !low.iter().chain(&high).all(|v| v.is_finite()) {
        return None;
    }
    let size = [high[0] - low[0], high[1] - low[1], high[2] - low[2]];
    let center = [(low[0] + high[0]) * 0.5, (low[1] + high[1]) * 0.5, (low[2] + high[2]) * 0.5];
    Some(ModelSphere { center, radius: 0.5 * (size[0] * size[0] + size[1] * size[1] + size[2] * size[2]).sqrt() })
}

#[derive(Default)]
pub struct Scratch {
    pub shots: Vec<Shot>,
    pub pending: Vec<PendingShot>,
    pub arc: ArcScratch,
}
#[derive(Clone)]
struct ArcCandidate { index: usize, origin: Vector3, a: f64 }
#[derive(Default)]
pub struct ArcScratch {
    targets: Vec<ArcTarget>,
    candidates: Vec<ArcCandidate>,
    sort: jsrt::sort::SortScratch<ArcCandidate>,
}
impl ArcScratch {
    pub fn reserve(&mut self, count: usize) {
        self.targets.reserve(count.saturating_sub(self.targets.len()));
        self.candidates.reserve(count.saturating_sub(self.candidates.len()));
        self.sort.reserve(count);
    }
}
impl Scratch {
    pub fn reserve(&mut self, count: usize) { self.shots.reserve(count); self.pending.reserve(count); self.arc.reserve(2048); }
}

impl CombatDefs {
    pub fn set_class(&mut self, id: DefId, obj: Rc<JsObj>) {
        let i = id.0 as usize;
        if self.classes.len() <= i {
            self.classes.resize(i + 1, None);
        }
        if self.facts.len() <= i { self.facts.resize_with(i + 1, ClassFacts::default); }
        // PlayerCombat.reset uses the same immutable loadout for every life.
        // Resolve its handles now instead of collecting a Vec on each spawn.
        let list = obj.get("weapons");
        let loadout = (0..list.len()).filter_map(|i| list.at(i).obj().and_then(|o| o.def).map(|(_,d)| d)).collect();
        // The class's cameras as admission parsed them (units::class::camera_def):
        // STAND, CROUCH, PRONE, then their ZOOM sections, ten numbers each.
        let numbers = obj.get("cameras");
        let cameras = (numbers.len() == 60).then(|| std::array::from_fn(|k| {
            let n = |j: usize| numbers.at(k * 10 + j).num();
            Section { eye: [n(0), n(1), n(2)], center: [n(3), n(4), n(5)], offset: [n(6), n(7), n(8)], tilt: n(9) }
        }));
        let walkerdroid = obj.get("base").is_str("walkerdroid");
        let sphere = if walkerdroid { model_sphere(obj.get("bounds")) } else { None };
        self.facts[i] = ClassFacts {
            add_health:obj.get("AddHealth").num(),walkerdroid,loadout,cameras,sphere,
        };
        self.classes[i] = Some(obj);
    }

    pub fn class(&self, id: DefId) -> &JsObj {
        self.classes[id.0 as usize].as_ref().expect("a unit class the battle registered")
    }

    /// A class's loadout (`def.weapons || []`), as weapon classes.
    pub fn loadout(&self, id: DefId) -> &[DefId] { &self.facts[id.0 as usize].loadout }

    /// A droideka class's sphere about its model box, read once as the class
    /// is prepared; None for any other class, or without a box.
    pub(crate) fn model_sphere(&self, id: DefId) -> Option<ModelSphere> {
        self.facts.get(id.0 as usize).and_then(|facts| facts.sphere)
    }
}

/// A trigger edge a network player's press carries (unit.networkShot or
/// unit.networkShots: src/runtime/network-shot-aim.js).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Edge {
    pub button: f64,
    pub id: Option<f64>,
    pub late: Option<f64>,
    pub aim_yaw: Option<f64>,
    pub aim_pitch: Option<f64>,
}

/// A unit's trigger edges: `networkShots` (one a button), else
/// `networkShot` (any button), else none.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Edges {
    pub shots: Option<Vec<Edge>>,
    pub shot: Option<Edge>,
    pub fixed: Option<[Option<Edge>; 2]>,
}

impl Edges {
    /// networkShotFor(owner, channel).
    pub fn for_channel(&self, channel: f64) -> Option<Edge> {
        if let Some(first) = self.fixed { return if channel == 0.0 { first[0] } else if channel == 1.0 { first[1] } else { None }; }
        match &self.shots {
            Some(list) => list.iter().find(|e| e.button == channel).copied(),
            None => self.shot,
        }
    }
}

/// The body combat fights with, as it reads it.
struct Body {
    position: Vector3,
    velocity: Vector3,
    aim_yaw: f64,
    aim_pitch: f64,
    /// `s.posture` (a droideka's: crouch folded or rolled, else stand).
    posture: Posture,
    air: Option<Air>,
    dive: f64,
    lock: f64,
    rolled: bool,
    droideka: bool,
}

// PlayerCombat reads motion after Unit.tick has moved the body.
// Keep a value snapshot because immediate fire can recurse through damage.
// Soldier and droideka use different eye, shield and posture facts.
// This snapshot owns no scratch or weapon storage.
// PlayerCombat's motion snapshot intentionally reads canonical posture.
// Drawn animation may interpolate between poses or still show an older tick.
// Physics, aiming and blast stance must all observe the completed unit state.
// No page smoothing value belongs in this native combat frame.
fn body(u: &Unit) -> Body {
    match &u.soldier {
        UnitBody::Soldier(s) => Body {
            position: v(s.position),
            velocity: v(s.velocity),
            aim_yaw: s.aim_yaw,
            aim_pitch: s.aim_pitch,
            posture: s.posture,
            air: s.air,
            dive: s.dive,
            lock: s.lock,
            rolled: false,
            droideka: false,
        },
        UnitBody::Droideka(d) => Body {
            position: v(d.position),
            velocity: v(d.velocity),
            aim_yaw: d.aim_yaw,
            aim_pitch: d.aim_pitch,
            posture: if d.mode >= 11.0 { Posture::Crouch } else { Posture::Stand },
            air: None,
            dive: f64::NAN,
            lock: f64::NAN,
            rolled: d.mode >= 11.0,
            droideka: true,
        },
    }
}

#[inline]
fn v(a: [f64; 3]) -> Vector3 {
    Vector3 { x: a[0], y: a[1], z: a[2] }
}

#[inline]
fn a(v: &Vector3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn posture_name(p: Posture) -> &'static str {
    match p {
        Posture::Stand => "stand",
        Posture::Crouch => "crouch",
        Posture::Prone => "prone",
    }
}

/// `x || 0`-style truthiness of a number.
#[inline]
fn truthy(x: f64) -> bool {
    x != 0.0 && !x.is_nan()
}

/// The world's random number for a combat roll: its generator, else the
/// legacy stream.
// PlayerCombat's random policy selects its supplied stream when present.
// The fallback stream is a separately prepared native binding.
// Do not draw at construction or when merely checking whether a roll is needed.
// The caller's branch fixes exactly where this draw occurs.
fn roll(c: &Combat, world: &mut dyn CombatWorld) -> f64 {
    world.draw_combat(c.random)
}

/// PlayerCombat.zooms(def): how many zoom steps.
fn zoom_count(d: &super::defs::Level) -> usize {
    let mut count = 1;
    let mut last = 1.0;
    let min = d.get(K::ZoomMin).or(1.0);
    let max = if d.get(K::ZoomRate).truthy() { 0.0 } else { d.get(K::ZoomMax).or(1.0) };
    for zoom in [min, max] {
        if zoom > last { count += 1; last = zoom; }
    }
    count
}

/// PlayerCombat.zoom reads the selected primary's increasing zoom steps.
/// Keep missing weapons and out-of-range steps at one without creating the
/// temporary PlayerCombat.zooms array for every seated reader.
// PlayerCombat.zoom chooses from the current primary weapon's zoom list.
// A switch may make the old zoom step invalid, which falls back to one.
// Content has already resolved level handles before this observation.
// Zoom reads never advance weapon timers or consume input.
pub fn zoom_factor(c: &mut Combat, defs: &CombatDefs) -> f64 {
    if !truthy(c.zoom_step) { return 1.0; }
    let Some(index) = current(c, defs, 0) else { return 1.0 };
    let d = c.weapons[index].level(class_of(c, defs, index));
    let mut count = 0.0;
    let mut last = 1.0;
    let minimum = d.get(K::ZoomMin).or(1.0);
    let maximum = if d.get(K::ZoomRate).truthy() { 0.0 } else { d.get(K::ZoomMax).or(1.0) };
    for zoom in [minimum, maximum] {
        if zoom > last {
            count += 1.0;
            if c.zoom_step == count { return zoom; }
            last = zoom;
        }
    }
    1.0
}

fn arsenal<'a>(c: &'a mut Combat) -> Arsenal<'a> {
    Arsenal { weapons: &mut c.weapons, selected: &mut c.selected }
}

/// The current weapon of a channel.
// Arsenal.current uses channel-relative selected indices.
// The underlying weapon vector can interleave both channels.
// Ask the shared arsenal implementation to preserve that indexing rule.
// An absent selection must remain distinct from index zero.
fn current(c: &mut Combat, defs: &CombatDefs, channel: usize) -> Option<usize> {
    arsenal(c).current(&defs.weapons, channel)
}

/// The class of the weapon at `i`.
// PlayerCombat reads the selected weapon's prepared class handle.
// Charge levels can change independently of the base class.
// The level handle retains that class's cached missile and firing policy.
// No string-keyed class traversal occurs in the live fire branch.
fn class_of<'d>(c: &Combat, defs: &'d CombatDefs, i: usize) -> &'d WeaponClass {
    defs.weapons.class(c.weapons[i].def)
}

/// The aim pivot's and gun origin's frame for the soldier's own aim
/// (aimBasis, the original PlayerCombat implementation).
// PlayerCombat.aimBasis uses posture offsets in the body's yaw frame.
// Keep the negative-facing basis and cross-product order used by three.js.
// The pivot is the origin for pitch while the gun has a separate offset.
// Those origins also feed instant shots and the network shot's convergence.
fn aim_basis(c: &mut Combat, b: &Body) {
    let (yaw, pitch) = (b.aim_yaw, b.aim_pitch);
    let posture = if b.air.is_some() { Posture::Stand } else { b.posture };
    let (sy, cy) = v8math::sin_cos(yaw);
    let (sp, cp) = v8math::sin_cos(pitch);
    let forward = Vector3 { x: sy * cp, y: sp, z: cy * cp };
    let left = Vector3 { x: cy, y: 0.0, z: -sy };
    let mut up = Vector3::default();
    up.cross_vectors(&forward, &left);
    let [px, py, pz] = pivot_of(posture);
    let pivot = Vector3 { x: b.position.x + left.x * px + sy * pz, y: b.position.y + py, z: b.position.z + left.z * px + cy * pz };
    let [gx, gy, gz] = gun_of(posture);
    let mut origin = pivot;
    origin.add_scaled_vector(&forward, gz).add_scaled_vector(&left, gx).add_scaled_vector(&up, gy);
    c.aim_dir = a(&forward);
    c.left = a(&left);
    c.up = a(&up);
    c.pivot = a(&pivot);
    c.origin = a(&origin);
}

/// aim() for a combat without a camera: from the gun
/// origin along the aimer's direction, or straight along the aim; a
/// droideka from its eye (aim).
// PlayerCombat aim retains a target point independently of its muzzle.
// A bot's aimer may override direction at the post-movement import point.
// A missing override uses the body's current yaw and pitch.
// The gameplay eye comes from the class, rather than a drawn camera rig.
fn aim(c: &mut Combat, b: &Body, eye_height: f64) {
    let reach;
    if b.droideka {
        let (yaw, p) = (b.aim_yaw, b.aim_pitch);
        let (sy, cy) = v8math::sin_cos(yaw);
        let (sp, cp) = v8math::sin_cos(p);
        let forward = Vector3 { x: sy * cp, y: sp, z: cy * cp };
        let left = Vector3 { x: cy, y: 0.0, z: -sy };
        let mut up = Vector3::default();
        up.cross_vectors(&forward, &left);
        c.aim_dir = a(&forward);
        c.left = a(&left);
        c.up = a(&up);
        // eye(position, origin): EyePointOffset over its origin (1 m rolled).
        c.origin = [b.position.x, b.position.y + if b.rolled { 1.0 } else { eye_height }, b.position.z];
        c.pivot = c.origin;
        reach = DROIDEKA_REACH;
    } else {
        aim_basis(c, b);
        reach = AIM_REACH;
    }
    let mut shot = match c.aim_override {
        Nullable::Value(o) => v(o),
        _ => v(c.aim_dir),
    };
    shot.normalize();
    c.shot_dir = a(&shot);
    let mut target = v(c.origin);
    target.add_scaled_vector(&shot, reach);
    c.target = a(&target);
}

/// infantryFireBlocked(soldier, gunUnderwater, stick).
// PlayerCombat gates fire before updating its arsenal.
// A dive, get-up or underwater gun can suppress an otherwise valid press.
// Preserve the existing comparisons so NaN takes the same branches as JS.
// Blocked input does not silently become a new press on the next tick.
// PlayerCombat keeps locomotion and weapon refusal as distinct state.
// A blocked tick may still update timers and kick in the surrounding solver.
// The retained control edges are consumed at their original observation point.
// Reordering this check can accidentally make a held trigger fire again.
fn fire_blocked(b: &Body, gun_underwater: bool, stick: f64, allow_jump_fall: bool) -> bool {
    let air_blocked = b.air.is_some_and(|air| {
        if allow_jump_fall {
            !matches!(air, Air::Jet | Air::Jump | Air::Fall)
        } else {
            air != Air::Jet
        }
    });
    (air_blocked || truthy(b.dive) || b.lock > 0.0 || b.rolled || (b.posture == Posture::Prone && stick > 0.1)) || gun_underwater
}

/// What update reads of the unit's controls (IntentControls take/held).
#[derive(Clone,Copy)]
pub enum Control { FirePrimary,FireSecondary,Use,Reload,NextPrimary,PreviousPrimary,NextSecondary,Zoom }
pub(crate) fn take(u: &mut Unit, name: Control) -> bool {
    match name {
        Control::FirePrimary => std::mem::replace(&mut u.controls.pressed[0], false),
        Control::FireSecondary => std::mem::replace(&mut u.controls.pressed[1], false),
        Control::Use => std::mem::replace(&mut u.intent.r#use, false),
        Control::Reload => std::mem::replace(&mut u.intent.reload, false),
        Control::NextPrimary => std::mem::replace(&mut u.intent.next_primary, false),
        Control::PreviousPrimary => std::mem::replace(&mut u.intent.previous_primary, false),
        Control::NextSecondary => std::mem::replace(&mut u.intent.next_secondary, false),
        Control::Zoom => std::mem::replace(&mut u.intent.zoom, false),
    }
}

fn held(u: &Unit, channel: usize) -> bool {
    u.intent.buttons[channel] > 0.0
}

/// readWeaponInput, seedPresses and latePresses.
// readWeaponInput consumes control edges at the combat observation point.
// Network press IDs seed spread using the current body id and gameplay life.
// Late presses carry their original lockout age into the weapon solver.
// Held-only input does not acquire a press seed or consume another edge.
fn weapon_input(id: u32, u: &mut Unit, edges: &Edges) -> ArsenalInput {
    let primary = take(u, Control::FirePrimary);
    let secondary = take(u, Control::FireSecondary);
    let mut input = ArsenalInput {
        primary: Trigger { held: held(u, 0) || primary, pressed: primary, seed: None, late: 0.0 },
        secondary: Trigger { held: held(u, 1) || secondary, pressed: secondary, seed: None, late: 0.0 },
        reload: take(u, Control::Reload),
    };
    for channel in 0..2 {
        let press = edges.for_channel(channel as f64).and_then(|e| e.id);
        let trigger = if channel == 1 { &mut input.secondary } else { &mut input.primary };
        if let Some(press_id) = press {
            if truthy(press_id) && trigger.pressed {
                trigger.seed = Some(jsrt::rng::spread_seed_f64(id as f64, u.generation, channel as f64, press_id));
            }
        }
    }
    for channel in 0..2 {
        let late = edges.for_channel(channel as f64).and_then(|e| e.late);
        let trigger = if channel == 1 { &mut input.secondary } else { &mut input.primary };
        if let Some(late) = late {
            if trigger.pressed && late > 0.0 {
                trigger.late = js_min(late, 15.0) / 60.0;
            }
        }
    }
    input
}

/// The press-time aim of a network player's edge (networkShotDirection's
/// and networkShotTarget's guard): its yaw and pitch, if it applies.
// networkShotFor keeps primary and secondary press samples separate.
// A sampled angle applies only when that channel has a current press.
// An ordinary held shot continues using the body's current aim.
// This distinction also controls the historical target tick.
fn edge_aim(u: &Unit, edges: &Edges, channel: f64) -> Option<(f64, f64)> {
    let e = edges.for_channel(channel)?;
    if u.network_human != Some(true) || u.seat.is_some() {
        return None;
    }
    match (e.aim_yaw, e.aim_pitch) {
        (Some(y), Some(p)) if y.is_finite() && p.is_finite() => Some((y, p)),
        _ => None,
    }
}

/// networkShotDirection(owner, direction, channel).
// PlayerCombat's network shot keeps the press's sampled aim angles.
// The shooter still fires from its present canonical body position.
// Rewinding the target must not rewind or move this origin.
// Apply the captured aim only for the matching weapon channel.
fn network_shot_direction(u: &Unit, edges: &Edges, dir: &mut Vector3, channel: f64) {
    if let Some((yaw, pitch)) = edge_aim(u, edges, channel) {
        let (sin, cos) = v8math::sin_cos(pitch);
        let (sy, cy) = v8math::sin_cos(yaw);
        dir.set(sy * cos, sin, cy * cos);
    }
}

/// From a gun to the point it converges on, for a world without the rig's
/// turning guns. A surface nearer than the barrels (the droideka backed
/// against a wall) never turns a shot back through the droideka: it fires
/// straight along the aim then.
fn gun_line(gun: &Vector3, target: &Vector3, forward: &Vector3) -> Vector3 {
    let mut dir = Vector3::default();
    dir.sub_vectors(target, gun).normalize();
    if dir.dot(forward) <= 0.0 { *forward } else { dir }
}

/// The point a droideka's guns converge on, and the aim it lies along, as
/// the 2004 game finds it after the droideka moved. A player's: the first
/// thing within 512 m along the line his reticle sits on (see
/// `bfe_presentation::aim::camera_line`: the chase camera's line from its
/// rest place, level with the eye; the eye's own line in first person or
/// zoomed), soldiers met where his screen showed them; the point 512 m out
/// when nothing is there. A bot's: along its aim as far as its quarry's aim
/// point (512 m without a quarry).
fn droideka_target(c: &Combat, u: &Unit, edges: &Edges, channel: f64, world: &mut dyn CombatWorld) -> (Vector3, Vector3) {
    let UnitBody::Droideka(d) = &u.soldier else { unreachable!("a droideka's target for a soldier") };
    let eye = v(c.origin);
    if !u.human || matches!(c.aim_override, Nullable::Value(_)) {
        let direction = v(c.shot_dir);
        let reach = world.bot_target_point().map_or(DROIDEKA_REACH, |point| point.distance_to(&eye));
        let mut target = eye;
        target.add_scaled_vector(&direction, reach);
        return (target, direction);
    }
    let (yaw, pitch) = edge_aim(u, edges, channel).unwrap_or((d.aim_yaw, d.aim_pitch));
    let stand = bfe_presentation::cameras::soldier::Section { eye: [0.0; 3], center: d.track_center, offset: d.track_offset, tilt: d.track_tilt };
    let eyes = c.first_person || c.zoom_step > 0.0;
    let (origin, direction) = bfe_presentation::aim::camera_line::<crate::cameras::HostMath>(v(d.position), yaw, pitch, eye, (!eyes).then_some(&stand));
    let reach = world.aim_reach(u, &origin, &direction, DROIDEKA_REACH, channel);
    let mut target = origin;
    target.add_scaled_vector(&direction, reach);
    (target, direction)
}

/// A soldier player's shot converged on his crosshair's point, as the 2004
/// game converges every round he fires in third person: the first thing
/// within 60 m along the ray from `crosshair_start` (the posture's camera,
/// its ZOOM section when zoomed), soldiers met where his screen showed them;
/// the ray's end when nothing is there. None in first person, zoomed with a
/// primary weapon whose zoom is first person (`primary`, the current one, as
/// the client's view and the 2004 game decide it, whatever weapon fires),
/// for bots and for a class without cameras: those fire along the aim.
fn crosshair_aim(c: &Combat, u: &Unit, defs: &CombatDefs, primary: Option<usize>, channel: f64, world: &mut dyn CombatWorld) -> Option<Vector3> {
    let UnitBody::Soldier(s) = &u.soldier else { return None };
    let zoomed = c.zoom_step > 0.0;
    let scoped = |p: usize| c.weapons[p].level(class_of(c, defs, p)).get(K::ZoomFirstPerson).truthy();
    if !u.human || matches!(c.aim_override, Nullable::Value(_)) || c.first_person || zoomed && primary.is_some_and(scoped) {
        return None;
    }
    let cameras = defs.facts.get(u.class_def.0 as usize)?.cameras.as_ref()?;
    let posture = if s.air.is_some() { Posture::Stand } else { s.posture };
    let section = &cameras[posture as usize + if zoomed { 3 } else { 0 }];
    let (start, direction) = bfe_presentation::aim::crosshair_start::<crate::cameras::HostMath>(v(s.position), s.aim_yaw, s.aim_pitch, section);
    let mut target = start;
    target.add_scaled_vector(&direction, world.aim_reach(u, &start, &direction, AIM_REACH, channel));
    let mut dir = Vector3::default();
    dir.sub_vectors(&target, &v(c.origin)).normalize();
    (dir.dot(&direction) > 0.0).then_some(dir)
}

/// 2005 guided-rocket reticle world target and camera ray direction so a shifted
/// launch origin can re-converge onto the point under the crosshair.
fn swbf2_crosshair_target(c: &Combat, u: &Unit, defs: &CombatDefs, primary: Option<usize>, channel: f64, world: &mut dyn CombatWorld) -> Option<(Vector3, Vector3)> {
    let UnitBody::Soldier(s) = &u.soldier else { return None };
    let zoomed = c.zoom_step > 0.0;
    let scoped = |p: usize| c.weapons[p].level(class_of(c, defs, p)).get(K::ZoomFirstPerson).truthy();
    if !u.human || matches!(c.aim_override, Nullable::Value(_)) || c.first_person || zoomed && primary.is_some_and(scoped) {
        return None;
    }
    let cameras = defs.facts.get(u.class_def.0 as usize)?.cameras.as_ref()?;
    let posture = if s.air.is_some() { Posture::Stand } else { s.posture };
    let section = &cameras[posture as usize + if zoomed { 3 } else { 0 }];
    let (start, direction) = bfe_presentation::aim::crosshair_start::<crate::cameras::HostMath>(v(s.position), s.aim_yaw, s.aim_pitch, section);
    let mut target = start;
    target.add_scaled_vector(&direction, world.aim_reach(u, &start, &direction, AIM_REACH, channel));
    Some((target, direction))
}

/// regenerate(combat, dt).
// PlayerCombat.regenerate applies health before this tick's fire.
// The cap remains full precision, including million-health hero classes.
// Do not route it through a network field with a sixteen-bit health limit.
// The original multiply then add order determines the stored low bits.
fn regenerate(c: &mut Combat, rate: f64, dt: f64) {
    if c.dead || !(c.health > 0.0) || !(dt > 0.0) {
        return;
    }
    let rate = if truthy(rate) { rate } else { 0.0 };
    if rate > 0.0 {
        c.health = js_min(c.max_health, c.health + rate * dt);
    }
}

/// The weapon's animation bank (`def.AnimationBank || "rifle"`).
// PlayerCombat selects the prepared weapon's animation bank.
// The bank affects presentation actions and alert policy.
// It is immutable class content shared across pooled lives.
// Reading it must not resolve another string-keyed property.
fn bank(class: &WeaponClass) -> &str { &class.base().bank }

// PlayerCombat weapon animation is a notification of a completed action.
// The native solver chooses its bank and posture before publishing.
// The client later applies it to drawn bones without advancing authority.
// A missing action still preserves the original notice ordering.
fn weapon_action(c: &Combat, defs: &CombatDefs, action: &str, definition: DefId, posture: Option<&'static str>, world: &mut dyn CombatWorld) {
    let bank_name = bank(defs.weapons.class(definition));
    let prepare = action.starts_with("shoot")
        && c.weapons.iter().any(|w| {
            let class = defs.weapons.class(w.def);
            bank(class) == bank_name && w.thrown(&defs.weapons) && w.busy()
        });
    world.presentation(&Presentation::WeaponAction { definition, action, bank: bank_name, posture, prepare });
}

/// PlayerCombat.update with the phase's retained shot buffer.
#[allow(clippy::too_many_arguments)]
// PlayerCombat.update runs after movement and the foreign bot aimer.
// Remote control can short-circuit without consuming unrelated soldier edges.
// Weapon switches and zoom precede aim, so fire sees the selected level.
// Arsenal updates collect shots before any shot executes a nested world action.
// The scratch belongs to the phase and is reused by the next unit.
// Deferred droideka muzzles remain pending until the hidden gun rig is posed.
pub fn update_buffered(c: &mut Combat, id: u32, u: &mut Unit, defs: &CombatDefs, dt: f64, stick: f64, edges: &Edges, world: &mut dyn CombatWorld, scratch: &mut Scratch) {
    let shots = &mut scratch.shots;
    regenerate(c, defs.facts[u.class_def.0 as usize].add_health, dt);
    let eye_height = match &u.soldier {
        UnitBody::Droideka(d) => d.eye_height,
        _ => 0.0,
    };
    // Manning a turret, the soldier's own weapons wait.
    if c.mounted == Some(true) {
        for w in c.weapons.iter_mut() {
            w.clear_lock();
        }
        aim(c, &body(u), eye_height);
        return;
    }
    if c.dead {
        for w in c.weapons.iter_mut() {
            w.clear_lock();
        }
        c.death_time += dt;
        for name in [Control::FirePrimary,Control::FireSecondary,Control::Reload,Control::NextPrimary,Control::PreviousPrimary,Control::NextSecondary,Control::Zoom] {
            take(u, name);
        }
        if c.death_time >= RESPAWN_DELAY {
            world.respawn(u, c);
        }
        return;
    }
    // Flying a remote droid, the soldier does nothing.
    if u.remotes_active == Some(true) {
        for w in c.weapons.iter_mut() {
            w.clear_lock();
        }
        aim(c, &body(u), eye_height);
        return;
    }
    let mut input = weapon_input(id, u, edges);
    // A remote weapon whose droid is out: fire flies it again, reload blows
    // it up (AllowDestruct).
    for i in 0..c.weapons.len() {
        let level = c.weapons[i].level(class_of(c, defs, i));
        if !level.get(K::Droid).truthy() {
            continue;
        }
        let allow_destruct = level.get(K::AllowDestruct).clone();
        c.weapons[i].admit_shot = true;
        let droid = world.remote_of(i as u32);
        c.weapons[i].deployed = droid.is_some();
        let channel = c.weapons[i].channel(&defs.weapons);
        let Some(droid) = droid else { continue };
        if current(c, defs, channel as usize) != Some(i) {
            continue;
        }
        let pressed = if channel == 0.0 { input.primary.pressed } else { input.secondary.pressed };
        if pressed {
            world.remote_enter(droid, u, c);
            let trigger = if channel == 0.0 { &mut input.primary } else { &mut input.secondary };
            trigger.pressed = false;
            trigger.held = false;
        } else if input.reload && !matches!(allow_destruct, Js::Num(x) if x == 0.0) {
            world.remote_kill(droid, u, c);
            input.reload = false;
        }
    }
    let primary = select_weapons(c, u, defs);
    let b = body(u);
    let family = world.battle_family();
    let blocked = fire_blocked(&b, c.gun_underwater, stick, family == bfe_family::GameFamily::Swbf2) || !world.fire_permitted();
    // Battlefront II detonator (`base = "detonator"`, `TriggerAll = 1`): pressing
    // its trigger while a pack is deployed detonates all live packs immediately,
    // even while reloading or out of ammo, without throwing a new pack.
    if family == bfe_family::GameFamily::Swbf2 && !blocked {
        for ch in 0..2usize {
            let Some(i) = current(c, defs, ch) else { continue };
            let level = c.weapons[i].level(class_of(c, defs, i));
            if level.kind == WeaponBase::Detonator && level.get(K::TriggerAll).truthy() {
                let trig = if ch == 0 { &mut input.primary } else { &mut input.secondary };
                if trig.pressed && world.detonator_deployed(i as u32, c) {
                    world.detonator_trigger(i as u32, u, c);
                    *trig = Trigger::default();
                }
            }
        }
    }
    // A droideka's shield goes up or down with each press of its trigger.
    if let Some(g) = current(c, defs, 1) {
        if c.weapons[g].level(class_of(c, defs, g)).kind==WeaponBase::Shield {
            if input.secondary.pressed && !blocked {
                if let UnitBody::Droideka(d) = &mut u.soldier {
                    d.toggle_shield();
                }
            }
            input.secondary = Trigger::default();
        }
    }
    aim(c, &b, eye_height);
    // A droideka's guns turn every tick toward the point its aim is on, as
    // the 2004 game's aimers do (back to rest while it folds, rolls or
    // unfolds); its rounds leave along them.
    if let UnitBody::Droideka(d) = &u.soldier {
        let ready = crate::units::droideka::guns_ready(d);
        let (target, _) = droideka_target(c, u, edges, 0.0, world);
        c.target = a(&target);
        world.turn_guns(u, ready.then_some(&target), dt);
    }
    // A repair weapon's lock, the trigger held widening it.
    let fixer = [current(c, defs, 0), current(c, defs, 1)].into_iter().flatten().find(|&i| c.weapons[i].repair(&defs.weapons));
    let fix_held = match fixer {
        Some(i) => {
            if c.weapons[i].channel(&defs.weapons) == 0.0 {
                input.primary.held
            } else {
                input.secondary.held
            }
        }
        None => false,
    };
    c.repair_target = match fixer {
        Some(i) => world.lock_on(&v(c.origin), &v(c.aim_dir), i as u32, &c.weapons[i], c.repair_target, fix_held, c.team),
        None => None,
    };
    c.repair_lock = world.lock_key(c.repair_target);
    for channel in 0..2 {
        let Some(i) = current(c, defs, channel) else { continue };
        let class = class_of(c, defs, i);
        let level = c.weapons[i].level(class);
        if !(level.kind==WeaponBase::Launcher && level.missile) {
            continue;
        }
        // The cone runs from the gun through the point under the reticle.
        match world.lock_pick(i as u32, &c.weapons[i], &v(c.origin), &v(c.shot_dir)) {
            LockPick::Nothing => c.weapons[i].clear_lock(),
            LockPick::None => c.weapons[i].update_lock(class, dt, None),
            LockPick::Unit { id, generation, alive } => {
                c.weapons[i].update_lock(class, dt, Some(super::weapons::LockTarget { id, generation, alive }))
            }
        }
    }
    let aim_in = Aim { stick, posture: Some(b.posture), zoomed: c.zoom_step > 0.0, repair_target: c.repair_target.is_some() };
    shots.clear();
    arsenal(c).update_for_family(&defs.weapons, family, dt, &input, &aim_in, blocked, world, shots);
    // Reloads of the gun in hand and throws play their clips over the upper
    // body for as long as the weapon stays in that state.
    let posture = Some(posture_name(b.posture));
    for i in 0..c.weapons.len() {
        if !c.seen[i].known {
            c.seen[i] = Seen { known: true, reloading: false, throwing: false };
        }
        let was = c.seen[i].clone();
        let class = class_of(c, defs, i);
        let reloading = c.weapons[i].reloading();
        if Some(i) == primary {
            if reloading && !was.reloading {
                weapon_action(c, defs, "reload", c.weapons[i].def, posture, world);
            }
            if !reloading && was.reloading {
                world.presentation(&Presentation::EndWeaponAction { action: Some("reload") });
            }
        }
        let w = &c.weapons[i];
        if w.thrown(&defs.weapons) && w.busy() && !was.throwing {
            let mut action = if w.charge > 0.7 {
                "shoot3"
            } else if w.charge > 0.4 {
                "shoot2"
            } else {
                "shoot"
            };
            if w.level(class).kind==WeaponBase::Dispenser {
                action = "shoot";
            }
            weapon_action(c, defs, action, c.weapons[i].def, posture, world);
        }
        c.seen[i].reloading = c.weapons[i].reloading();
        c.seen[i].throwing = c.weapons[i].busy();
    }
    for shot in shots.iter() {
        fire(c, u, defs, shot, edges, world, &mut scratch.arc);
    }
    update_kick(c, u, dt);
}

/// selectCombatWeapons(combat, controls, zooms) (the original weapon input helpers).
// selectCombatWeapons consumes each channel's next/previous edges in order.
// A busy weapon can refuse selection without moving the selected index.
// Primary selection affects zoom before this tick computes the aim basis.
// The cached control enum avoids a property-name search for each soldier.
fn select_weapons(c: &mut Combat, u: &mut Unit, defs: &CombatDefs) -> Option<usize> {
    if take(u, Control::NextPrimary) && arsenal(c).next(&defs.weapons, 0, 1.0) {
        c.zoom_step = 0.0;
    }
    if take(u, Control::PreviousPrimary) && arsenal(c).next(&defs.weapons, 0, -1.0) {
        c.zoom_step = 0.0;
    }
    if take(u, Control::NextSecondary) {
        arsenal(c).next(&defs.weapons, 1, 1.0);
    }
    let primary = current(c, defs, 0);
    if take(u, Control::Zoom) {
        if let Some(p) = primary {
            let n = zoom_count(c.weapons[p].level(class_of(c, defs, p)));
            c.zoom_step = jsrt::conv::js_mod(c.zoom_step + 1.0, n as f64);
        }
    }
    let b = body(u);
    if b.air.is_some_and(|air| air != Air::Jet) || truthy(b.dive) || primary.is_some_and(|p| c.weapons[p].reloading()) {
        c.zoom_step = 0.0;
    }
    primary
}

/// updateKick(dt): the aim climbs while the kick holds,
/// and the rate fades.
// PlayerCombat.updateKick applies aim kick before fading its accumulator.
// Droideka pitch clamps to its prepared class limits after that addition.
// Keep tiny-value snapping after the decay, including the hold check.
// Changing this order alters later aim even when the visible kick is small.
pub fn update_kick(c: &mut Combat, u: &mut Unit, dt: f64) {
    let k = &mut c.kick;
    if !truthy(k.rate) && !truthy(k.offset) && !truthy(k.hold) {
        return;
    }
    match &mut u.soldier {
        UnitBody::Soldier(s) => {
            s.aim_pitch += js_min(k.hold, 1.0) * k.rate * dt;
            let [low, high] = s.pitch_range();
            s.aim_pitch = three::clamp(s.aim_pitch, low, high);
        }
        UnitBody::Droideka(d) => {
            d.aim_pitch += js_min(k.hold, 1.0) * k.rate * dt;
            let [low, high] = d.pitch_range();
            d.aim_pitch = three::clamp(d.aim_pitch, low, high);
        }
    }
    k.offset = (k.offset + k.rate * dt) * (1.0 - js_min(KICK_OFFSET_FADE * dt, 1.0));
    k.rate *= 1.0 - js_min(KICK_FADE * dt, 1.0);
    k.hold = js_max(k.hold - KICK_HOLD_DRAIN * dt, 0.0);
    if k.rate.abs() < 1e-6 && k.offset.abs() < 1e-6 && !truthy(k.hold) {
        k.rate = 0.0;
        k.offset = 0.0;
    }
}

/// fire(shot).
// PlayerCombat.fire delivers the fired observer before selecting the effect.
// Melee strike/end notices take their own early paths before ordinary fire.
// Repair, dispenser, launcher, arc and deferred muzzles retain source order.
// Every hit or explosion completes immediately before the next collected shot.
fn fire(c: &mut Combat, u: &mut Unit, defs: &CombatDefs, shot: &Shot, edges: &Edges, world: &mut dyn CombatWorld, arc_scratch: &mut ArcScratch) {
    match shot.kind {
        ShotKind::MeleeEnd => {
            world.presentation(&Presentation::EndWeaponAction { action: None });
            return;
        }
        ShotKind::MeleeHit => {
            world.melee_strike(u, c);
            return;
        }
        _ => {}
    }
    world.fired(shot, u, c);
    let i = shot.weapon as usize;
    let class = class_of(c, defs, i);
    let posture = Some(posture_name(body(u).posture));
    if let ShotKind::MeleeStart { swing } = shot.kind {
        let action = if swing == 1.0 { "shoot" } else if swing == 2.0 { "shoot2" } else { "shoot3" };
        weapon_action(c, defs, action, c.weapons[i].def, posture, world);
        return;
    }
    if shot.kind == ShotKind::Repair {
        // repairTick(weapon).
        let level = c.weapons[i].level(class);
        let heat = level.get(K::HeatPerShot).or(0.0);
        let fx = (world.battle_family() == bfe_family::GameFamily::Swbf2)
            .then(|| level.field("MuzzleFlashEffect").str().map(str::to_string))
            .flatten();
        let target = c.repair_target;
        if world.repair(i as u32, target, c.team, u, c) {
            c.weapons[i].heat += heat;
            if let Some(fx_name) = fx.as_deref() {
                world.emit_swbf2_effect(fx_name, &v(c.origin), Some(&v(c.aim_dir)), None);
            }
        }
        return;
    }
    let level = c.weapons[i].level(class);
    if level.get(K::Item).truthy() {
        dispense(c, u, defs, shot, world);
        return;
    }
    if level.get(K::Droid).truthy() {
        let guided = world.battle_family() == bfe_family::GameFamily::Swbf2
            && level.get(K::Droid).get("GuidedMissile").truthy();
        let mut dir = v(c.shot_dir);
        let mut origin = v(c.origin);
        if guided {
            let channel = c.weapons[i].channel(&defs.weapons);
            let mut aim_target: Option<(Vector3, Vector3)> = None;
            if edge_aim(u, edges, channel).is_some() {
                network_shot_direction(u, edges, &mut dir, channel);
                let dir_len = dir.length();
                if dir_len > 1e-6 {
                    dir.divide_scalar(dir_len);
                }
                let reach = world.aim_reach(u, &origin, &dir, AIM_REACH, channel);
                let mut target = origin;
                target.add_scaled_vector(&dir, reach);
                aim_target = Some((target, dir));
            } else if let Some((target, cross_dir)) = {
                let primary = current(c, defs, 0);
                swbf2_crosshair_target(c, u, defs, primary, channel, world)
            } {
                let mut converged = Vector3::default();
                converged.sub_vectors(&target, &origin).normalize();
                if converged.dot(&cross_dir) > 0.0 {
                    dir = converged;
                    aim_target = Some((target, cross_dir));
                }
            }
            let dir_len = dir.length();
            if dir_len > 1e-6 {
                dir.divide_scalar(dir_len);
            }
            // Apply authored FirePointExtraOffset (right, up, forward) in launch frame
            // only when clear of solid geometry and close targets; fall back to the
            // unobstructed fire origin otherwise so launch never embeds in a wall or
            // skips past a close target, then re-converge onto the reticle target.
            let extra = level.field("FirePointExtraOffset");
            if extra.truthy() {
                let ox = extra.at(0).nn(0.0);
                let oy = extra.at(1).nn(0.0);
                let oz = extra.at(2).nn(0.0);
                let mut right = Vector3::default();
                right.cross_vectors(&dir, &UP);
                let rlen = right.length();
                if rlen > 1e-6 {
                    right.divide_scalar(rlen);
                } else {
                    right = Vector3::new(1.0, 0.0, 0.0);
                }
                let mut up = Vector3::default();
                up.cross_vectors(&right, &dir);
                let mut offset = Vector3::default();
                offset.add_scaled_vector(&right, ox);
                offset.add_scaled_vector(&up, oy);
                offset.add_scaled_vector(&dir, oz);
                let dist = offset.length();
                if dist > 1e-6 {
                    let mut step_dir = offset;
                    step_dir.divide_scalar(dist);
                    let target_past_offset = aim_target
                        .is_none_or(|(target, _)| target.distance_to(&origin) > dist + 0.25);
                    if target_past_offset
                        && world.cast(&origin, &step_dir, dist, false).is_none()
                        && world.cast(&origin, &dir, dist, false).is_none()
                        && world.aim_reach(u, &origin, &step_dir, dist, channel) >= dist - 1e-4
                        && world.aim_reach(u, &origin, &dir, dist, channel) >= dist - 1e-4
                    {
                        origin.add_scaled_vector(&step_dir, dist);
                        if let Some((target, ref_dir)) = aim_target {
                            let mut to_target = Vector3::default();
                            to_target.sub_vectors(&target, &origin);
                            let span = to_target.length();
                            if span > 0.5 {
                                to_target.divide_scalar(span);
                                if to_target.dot(&ref_dir) > 0.0 {
                                    dir = to_target;
                                }
                            }
                        }
                    }
                }
            }
        }
        // A remote droid leaves the gun origin along the aim at Velocity,
        // plus the soldier's own velocity. Guided missiles launch cleanly
        // along the converged aim without sideways walk drift.
        let speed = level.get(K::Velocity).nn(10.0) * level.get(K::MaxStrength).nn(1.0);
        let mut velocity = dir;
        velocity.multiply_scalar(speed);
        if !guided {
            velocity.add(&body(u).velocity);
        }
        let admission = match shot.kind {
            ShotKind::Round { admission, .. } => admission,
            _ => None,
        };
        let (yaw, team) = (body(u).aim_yaw, c.team);
        world.remote_launch(i as u32, &origin, &velocity, yaw, team, admission, u, c);
        return;
    }
    let ordnance = level.get(K::Ordnance).clone();
    if !ordnance.truthy() {
        return;
    }
    let ShotKind::Round { yaw, pitch, .. } = shot.kind else { return };
    if level.emitter {
        arc(c, u, defs, shot, &level.arc, edges, world, arc_scratch);
        return;
    }
    let channel = c.weapons[i].channel(&defs.weapons);
    // A droideka's guns take turns, each round leaving its barrel along its
    // gun, which this tick's aimer turned toward the point under the reticle
    // (`c.target`; its press's aim, for a network player's press).
    if matches!(u.soldier, UnitBody::Droideka(_)) && world.next_gun(u, c).is_some() {
        let forward = v(c.shot_dir);
        let selected = match &u.soldier {
            UnitBody::Droideka(d) => d.last_gun.as_ref().map(|g| (g.index, g.barrel)),
            _ => None,
        };
        let (index, barrel) = selected.unwrap_or((f64::NAN, f64::NAN));
        let ShotKind::Round { charge, target_id, target_generation, .. } = shot.kind else { unreachable!() };
        let payload = PendingShot {
            weapon: i as f64,
            yaw,
            pitch,
            charge,
            target_id,
            target_generation,
            selected_index: index,
            selected_barrel: barrel,
            target: c.target,
            forward: a(&forward),
            kick: c.kick.offset,
        };
        if c.defer_muzzles {
            c.pending_muzzles.push(payload);
        } else {
            emit_gun_round(c, u, defs, &payload, world);
        }
    } else {
        // Collision starts at the gameplay gun origin. A press flies along
        // the aim its player's screen converged; a soldier's held round
        // converges here on his crosshair's point.
        let mut dir = v(c.shot_dir);
        if edge_aim(u, edges, channel).is_some() {
            network_shot_direction(u, edges, &mut dir, channel);
        } else if let Some(converged) = {
            let primary = current(c, defs, 0);
            crosshair_aim(c, u, defs, primary, channel, world)
        } {
            dir = converged;
        }
        // The spread (and a pattern's offset, and the kick): turn about the
        // vertical, then tip about the horizontal.
        dir.apply_axis_angle(&UP, yaw);
        let mut side = Vector3 { x: dir.z, y: 0.0, z: -dir.x };
        side.normalize();
        dir.apply_axis_angle(&side, -(pitch + c.kick.offset));
        let level = c.weapons[i].level(class_of(c, defs, i));
        let elevate = level.get(K::ShotElevate).clone();
        if elevate.truthy() {
            dir.y += elevate.num();
            dir.normalize();
        }
        let thrown = c.weapons[i].thrown(&defs.weapons);
        world.fire_round(&v(c.origin), &dir, shot, !thrown, None, u, c);
    }
    // Each round kicks the aim.
    let level = c.weapons[i].level(class_of(c, defs, i));
    let strength = level.get(K::KickStrength).clone();
    if strength.truthy() {
        c.kick.rate += KICK_RATE * strength.num() * DEG;
        c.kick.hold += (1.0 / level.get(K::KickBuildup).or(2.0) + 2.0) * level.get(K::ShotDelay).num();
    }
}

/// PlayerCombat.flushMuzzles with a retained snapshot for synchronous effects.
// PlayerCombat.flushMuzzles runs after Droideka's hidden aimer update.
// Pending shots retain their weapon, spread and the point their press aimed at.
// Use the newly posed barrel while keeping that shot's earlier gameplay facts.
// The shared buffer is cleared without releasing its construction capacity.
// PlayerCombat does not recollect or reseed shots during this flush.
// Its pending records describe work already admitted by Arsenal.update.
// A later barrel pose changes only the muzzle service's sampled frame.
// The shot buffer's lifetime ends after every retained emission completes.
pub fn flush_muzzles_buffered(c: &mut Combat, u: &mut Unit, defs: &CombatDefs, world: &mut dyn CombatWorld, pending: &mut Vec<PendingShot>) {
    pending.clear(); pending.extend_from_slice(&c.pending_muzzles);
    for payload in pending.iter() { emit_gun_round(c, u, defs, payload, world); }
    c.pending_muzzles.clear();
}

/// emitGunRound(payload).
// PlayerCombat.fireRound transforms the prepared muzzle at fire time.
// The muzzle points at the retained aim target before spread is applied.
// Alternate barrels advance once per emitted round in the original order.
// F1 receives native service handles rather than a callback into its export.
fn emit_gun_round(c: &mut Combat, u: &mut Unit, defs: &CombatDefs, p: &PendingShot, world: &mut dyn CombatWorld) {
    let Some(from) = world.gun_at((p.selected_index, p.selected_barrel)) else { return };
    let i = p.weapon as usize;
    let mut dir = world.gun_along((p.selected_index, p.selected_barrel))
        .unwrap_or_else(|| gun_line(&from, &v(p.target), &v(p.forward)));
    dir.apply_axis_angle(&UP, p.yaw);
    let mut side = Vector3 { x: dir.z, y: 0.0, z: -dir.x };
    side.normalize();
    dir.apply_axis_angle(&side, -(p.pitch + p.kick));
    let level = c.weapons[i].level(class_of(c, defs, i));
    let elevate = level.get(K::ShotElevate).clone();
    if elevate.truthy() {
        dir.y += elevate.num();
        dir.normalize();
    }
    let shot = Shot {
        weapon: p.weapon as u32,
        kind: ShotKind::Round { yaw: p.yaw, pitch: p.pitch, charge: p.charge, target_id: p.target_id, target_generation: p.target_generation, admission: None },
    };
    world.fire_round(&from, &dir, &shot, true, Some((p.selected_index, p.selected_barrel)), u, c);
}

/// dispense(shot): a dispenser's item thrown along the aim.
// PlayerCombat.dispense gives items the launch position and velocity.
// A class's dispenser list enforces its own retained-item count.
// Failure to dispense keeps the original weapon admission behavior.
// The owner handle freezes attribution separately from later body reuse.
fn dispense(c: &mut Combat, u: &mut Unit, defs: &CombatDefs, shot: &Shot, world: &mut dyn CombatWorld) {
    let i = shot.weapon as usize;
    let level = c.weapons[i].level(class_of(c, defs, i));
    let min = level.get(K::MinStrength).nn(0.3);
    let charge = match shot.kind {
        ShotKind::Round { charge, .. } => charge,
        _ => 1.0,
    };
    let strength = min + (level.get(K::MaxStrength).nn(1.0) - min) * charge;
    let mut velocity = v(c.shot_dir);
    velocity.multiply_scalar(level.get(K::Velocity).nn(10.0) * strength).add(&body(u).velocity);
    let most = level.get(K::MaxItems).or(0.0);
    let heat = level.get(K::HeatPerShot).or(0.0);
    let (origin, yaw, team) = (v(c.origin), body(u).aim_yaw, c.team);
    let made = world.dispense(i as u32, most, &origin, &velocity, yaw, team, u, c);
    if !made {
        let w = &mut c.weapons[i];
        if !w.bottomless {
            w.clip = js_min(w.clip + w.per_shot, 1.0);
        }
        w.heat = js_max(w.heat - heat, 0.0);
    }
}

/// arc(shot, def): the arc caster's lightning.
// PlayerCombat.arc acquires links before applying their damage.
// Stable along-distance ties keep the registry's original encounter order.
// A line can link soldiers, mines and recon with different damage policies.
// Each immediate death finishes before the following retained link is visited.
// The original draw sites remain even when an effect has zero visual length.
// One phase scratch keeps the candidate and victim arrays off the allocator.
fn arc(c: &mut Combat, u: &mut Unit, defs: &CombatDefs, shot: &Shot, def: &super::defs::ArcDef, edges: &Edges, world: &mut dyn CombatWorld, scratch: &mut ArcScratch) {
    if let Some(weapon) = c.weapons.get(shot.weapon as usize) {
        world.observe_arc_weapon(weapon.def);
    }
    if world.battle_family() == bfe_family::GameFamily::Swbf2 {
        swbf2_arc(c, u, defs, shot, def, edges, world, scratch);
    } else {
        released_arc(c, u, defs, shot, def, edges, world, scratch);
    }
}

fn released_arc(c: &mut Combat, u: &mut Unit, defs: &CombatDefs, shot: &Shot, def: &super::defs::ArcDef, edges: &Edges, world: &mut dyn CombatWorld, scratch: &mut ArcScratch) {
    let ShotKind::Round { charge, .. } = shot.kind else { return };
    let i = shot.weapon as usize;
    let channel = c.weapons[i].channel(&defs.weapons);
    let p = v(c.origin);
    let mut d = v(c.shot_dir);
    network_shot_direction(u, edges, &mut d, channel);
    d.normalize();
    let l = def.length;
    let sine = def.sine;
    let mut center = p;
    center.add_scaled_vector(&d, 0.9 * l);
    let mut toward = Vector3::default();
    // arcTargets used to allocate a view list for each shot. The phase owns
    // its snapshot and TimSort scratch, so nested hurts cannot overwrite it.
    world.arc_targets_into(&mut scratch.targets);
    let targets = &scratch.targets;
    let candidates = &mut scratch.candidates;
    candidates.clear();
    for (index, target) in targets.iter().enumerate() {
        let cc = target.center.unwrap_or(target.origin);
        let r = l + target.radius.unwrap_or(0.0);
        if (cc.x - center.x).abs() > r || (cc.y - center.y).abs() > r || (cc.z - center.z).abs() > r {
            continue;
        }
        toward.sub_vectors(&target.origin, &p);
        let along = toward.dot(&d);
        if !(along < l) {
            continue;
        }
        // Inside the cone about the shot's line, either way along it.
        toward.add_scaled_vector(&d, -along);
        if !(toward.length_sq() < v8math::pow(sine * along, 2.0)) {
            continue;
        }
        candidates.push(ArcCandidate { index, origin: target.origin, a: along });
    }
    jsrt::sort::js_sort_buffered(candidates, &mut |x: &ArcCandidate, y: &ArcCandidate| x.a - y.a, &mut scratch.sort);
    let weapon_def = c.weapons[i].def;
    let weapon_level = c.weapons[i].level;
    let bolt = def.name.as_deref().is_some_and(|name| world.has_lightning_for(weapon_def, weapon_level, name));
    let lightning_name = def.name.as_deref().unwrap_or("");
    let mut damage = def.damage + (def.max_damage - def.damage) * charge;
    let mut from = p;
    let mut dir = Vector3::default();
    let mut prev = 0.0;
    let mut links = 0.0;
    let first_radius = def.first_radius;
    for cand in candidates.iter() {
        if links >= def.max_targets || damage < def.threshold {
            break;
        }
        let jump = if links == 0.0 { l } else { def.jump };
        if cand.a - prev > jump {
            break;
        }
        let mut t = cand.origin;
        t.y += 1.0;
        // Blocked from where its arc starts, or (the first) its point too far
        // off the shot's line.
        let span = dir.sub_vectors(&t, &from).length();
        if span > 1e-3 {
            dir.divide_scalar(span);
            if world.cast(&from, &dir, span, false).is_some() {
                continue;
            }
        }
        if links == 0.0 && first_radius > 0.0 {
            toward.sub_vectors(&t, &p);
            let along = toward.dot(&d);
            toward.add_scaled_vector(&d, -along);
            if toward.length_sq() > v8math::pow(first_radius, 2.0) {
                continue;
            }
        }
        prev = cand.a;
        if damage > 0.0 {
            world.hurt_arc_target(cand.index, damage, &from, u, c);
        }
        if bolt {
            world.presentation(&Presentation::Lightning { definition: weapon_def, level: weapon_level, name: lightning_name, from, to: t, charge, channel: if links == 0.0 { Some(channel) } else { None } });
            if links == 0.0 || roll(c, world) > 0.3 {
                from = t;
            }
        }
        links += 1.0;
        damage -= def.drop;
    }
    if links == 0.0 && bolt {
        let hit = world.cast(&p, &d, l, false);
        let mut to = p;
        to.add_scaled_vector(&d, hit.map(|h| h.distance).unwrap_or(l));
        world.presentation(&Presentation::Lightning { definition: weapon_def, level: weapon_level, name: lightning_name, from: p, to, charge, channel: Some(channel) });
    }
}

fn swbf2_arc(c: &mut Combat, u: &mut Unit, defs: &CombatDefs, shot: &Shot, def: &super::defs::ArcDef, edges: &Edges, world: &mut dyn CombatWorld, scratch: &mut ArcScratch) {
    let ShotKind::Round { charge, .. } = shot.kind else { return };
    let i = shot.weapon as usize;
    let channel = c.weapons[i].channel(&defs.weapons);
    let p = v(c.origin);
    let mut d = v(c.shot_dir);
    network_shot_direction(u, edges, &mut d, channel);
    d.normalize();
    let l = def.length;
    let sine = def.sine;
    let mut center = p;
    center.add_scaled_vector(&d, 0.9 * l);
    let mut toward = Vector3::default();
    world.arc_targets_into(&mut scratch.targets);
    let targets = &scratch.targets;
    let candidates = &mut scratch.candidates;
    candidates.clear();
    for (index, target) in targets.iter().enumerate() {
        if target.relation > 0.0 && !def.affect_friends {
            continue;
        }
        if target.relation < 0.0 && !def.affect_enemies {
            continue;
        }
        let cc = target.center.unwrap_or(target.origin);
        let r = l + target.radius.unwrap_or(0.0);
        if (cc.x - center.x).abs() > r || (cc.y - center.y).abs() > r || (cc.z - center.z).abs() > r {
            continue;
        }
        toward.sub_vectors(&target.origin, &p);
        let along = toward.dot(&d);
        if def.no_chaining && !(along > 0.0) {
            continue;
        }
        if !(along < l) {
            continue;
        }
        toward.add_scaled_vector(&d, -along);
        if !(toward.length_sq() < v8math::pow(sine * along, 2.0)) {
            continue;
        }
        candidates.push(ArcCandidate { index, origin: target.origin, a: along });
    }
    jsrt::sort::js_sort_buffered(candidates, &mut |x: &ArcCandidate, y: &ArcCandidate| x.a - y.a, &mut scratch.sort);
    let weapon_def = c.weapons[i].def;
    let weapon_level = c.weapons[i].level;
    let bolt = def.name.as_deref().is_some_and(|name| world.has_lightning_for(weapon_def, weapon_level, name));
    let lightning_name = def.name.as_deref().unwrap_or("");
    let mut damage = def.damage + (def.max_damage - def.damage) * charge;
    let mut from = p;
    let mut dir = Vector3::default();
    let mut prev = 0.0;
    let mut links = 0.0;
    let first_radius = def.first_radius;
    for cand in candidates.iter() {
        if links >= def.max_targets || damage < def.threshold {
            break;
        }
        if !def.no_chaining {
            let jump = if links == 0.0 { l } else { def.jump };
            if cand.a - prev > jump {
                break;
            }
        }
        let mut t = cand.origin;
        t.y += 1.0;
        let ray_from = if def.no_chaining { p } else { from };
        let span = dir.sub_vectors(&t, &ray_from).length();
        if span > 1e-3 {
            dir.divide_scalar(span);
            if world.cast(&ray_from, &dir, span, false).is_some() {
                continue;
            }
        }
        if !def.no_chaining && links == 0.0 && first_radius > 0.0 {
            toward.sub_vectors(&t, &p);
            let along = toward.dot(&d);
            toward.add_scaled_vector(&d, -along);
            if toward.length_sq() > v8math::pow(first_radius, 2.0) {
                continue;
            }
        }
        prev = cand.a;
        if damage > 0.0 {
            world.hurt_arc_target(cand.index, damage, &ray_from, u, c);
        }
        if def.smolder_damage_rate > 0.0 && def.smolder_timer > 0.0 {
            world.apply_burn_arc_target(cand.index, def.smolder_damage_rate, def.smolder_timer, def.smolder_effect.as_deref());
        }
        if bolt {
            world.presentation(&Presentation::Lightning { definition: weapon_def, level: weapon_level, name: lightning_name, from: ray_from, to: t, charge, channel: if links == 0.0 { Some(channel) } else { None } });
            if !def.no_chaining && (links == 0.0 || roll(c, world) > 0.3) {
                from = t;
            }
        }
        links += 1.0;
        damage -= def.drop;
    }
    if let Some(name) = def.aimer_effect.as_deref() {
        world.emit_swbf2_effect(name, &p, Some(&d), None);
    }
    if links == 0.0 && bolt {
        let hit = world.cast(&p, &d, l, false);
        let mut to = p;
        to.add_scaled_vector(&d, hit.map(|h| h.distance).unwrap_or(l));
        world.presentation(&Presentation::Lightning { definition: weapon_def, level: weapon_level, name: lightning_name, from: p, to, charge, channel: Some(channel) });
    }
}

/// explode(explosion, position, owner, scale): a blast's
/// damage and push on the soldier.
#[allow(clippy::too_many_arguments)]
pub fn explode(c: &mut Combat, u: &mut Unit, defs: &CombatDefs, explosion: &Js, position: &Vector3, owner: Attacker, scale: f64, world: &mut dyn CombatWorld) {
    let packet = BlastPacket::read(explosion);
    let health = HealthScale::from_scale(&world.text(c.scale));
    explode_prepared(c, u, defs, &packet, health, position, owner, scale, world);
}

/// PlayerCombat.explode using immutable numeric content prepared at load.
/// The live registry calls this form so each victim neither allocates a
/// health-scale String nor looks up keyed fields on its explosion class.
#[allow(clippy::too_many_arguments)]
// PlayerCombat.explode measures damage and push independently.
// Static cover is tested only after the radius admits a useful effect.
// Shield absorption and body damage keep their separate health-type scales.
// Damage completes before push; a blown-apart corpse therefore receives none.
// The unit's lazy hit-shape cache is not invalidated by a blast.
// The caller owns live-registry traversal and depth-first fan-out ordering.
pub fn explode_prepared<W: ImpactWorld + ?Sized>(c: &mut Combat, u: &mut Unit, defs: &CombatDefs,
    explosion: &BlastPacket, health: HealthScale, position: &Vector3, owner: Attacker, scale: f64, world: &mut W) -> bool {
    let facts = &defs.facts[u.class_def.0 as usize];
    let walkerdroid = facts.walkerdroid;
    let was_dead = c.dead;
    // Nothing is left to throw of a droid that went up in pieces, and a
    // droideka's hull never takes the infantry launch impulse.
    if was_dead && (c.blow_apart || walkerdroid) {
        return false;
    }
    let b = body(u);
    let (center, radius, sight_point) = match facts.sphere.filter(|_| walkerdroid) {
        // A droideka is measured to the sphere around its model, standing or
        // rolled, and seen at that sphere's centre.
        Some(sphere) => {
            let [x, y, z] = sphere.carried([b.position.x, b.position.y, b.position.z], b.aim_yaw);
            let center = Vector3 { x, y, z };
            (center, sphere.radius, center)
        }
        None => {
            let stance = if was_dead {
                Posture::Prone
            } else if b.air.is_some() {
                Posture::Stand
            } else {
                b.posture
            };
            let [middle, radius, sight] = blast_body(stance);
            let mut center = b.position;
            center.y += middle;
            (center, radius, Vector3 { x: b.position.x, y: b.position.y + sight, z: b.position.z })
        }
    };
    let mut away = Vector3::default();
    away.sub_vectors(&center, position);
    let span = away.length();
    let distance = center.distance_to(position) - 0.5 * radius;
    let mut push = explosion.push * falloff(distance, explosion.push_inner, explosion.push_outer);
    let damage = scale * explosion.hit.damage * falloff(distance, explosion.inner, explosion.outer);
    let typed = explosion.hit.scales[health as usize];
    let hurts = !was_dead && (damage > 0.0 || !(damage * typed <= 0.0));
    if !(push > 0.0) && !hurts {
        return false;
    }
    // Terrain or a building between the blast and his sight point shields him.
    if world.impact_blocked(position, &sight_point) {
        return false;
    }
    // The owned publisher needs only rows whose values changed. A blast can
    // reach a mounted body or a zero-typed health class without changing it.
    let before = (c.health.to_bits(),c.last_attacker,c.dead,c.blow_apart,u.velocity(),blast_shield(u));
    if span > 1e-3 {
        away.divide_scalar(span);
    }
    // The push is rolled before the damage: a droid the blast would push
    // may be marked to blow apart at its death instead, unpushed.
    if push > 0.0 && truthy(c.chunks) && !was_dead && c.mounted != Some(true) && b.air != Some(Air::Thrown) && world.impact_draw(c.random) <= c.chunks {
        c.blow_apart = true;
        push = 0.0;
    }
    // A rider (held by an animated prop or in a seat) is neither shoved nor
    // pushed, alive or killed: one the blast kills leaves by his own path.
    // Dying gives up the seat inside the damage step, so whether he rode is
    // read first.
    let rider = c.mounted == Some(true) || u.seat.is_some();
    if !was_dead {
        let generic = explosion.hit.shield;
        let info = DamageInfo { explosion: true, amount: Some(damage), typed: Some(typed), generic: Some(generic), known: Some(false), point: Some(None) };
        damage_with(c, u, defs, damage * typed, damage * generic, Some(*position), owner, &info, world);
        // A soldier on foot this blast killed is sent off from it along the
        // line from the blast to his feet, whether or not the explosion pushes.
        if c.dead && !rider && !c.blow_apart {
            if let UnitBody::Soldier(s) = &mut u.soldier {
                let mut off = Vector3::default();
                if off.sub_vectors(&b.position, position).length() > 0.0 {
                    off.normalize();
                    let mut velocity = v(s.velocity);
                    velocity.add_scaled_vector(&off, BLAST_DEATH_SPEED);
                    s.velocity = a(&velocity);
                }
            }
        }
    }
    // Thrown whole, alive or killed by this blast; a droid that blew apart
    // at its death has no body left; droidekas never take the push, nor a
    // rider, nor a living soldier already flying from an earlier one.
    let flying = b.air == Some(Air::Thrown) && !c.dead;
    if push > 0.0 && !rider && !(c.dead && c.blow_apart) && !walkerdroid && !flying {
        if let UnitBody::Soldier(s) = &mut u.soldier {
            let mut velocity = v(s.velocity);
            velocity.add_scaled_vector(&away, push);
            velocity.y += 0.6 * push;
            s.velocity = a(&velocity);
            if s.velocity[1] > THROWN_SPEED && s.air != Some(Air::Thrown) {
                s.air = Some(Air::Thrown);
                s.state_time = 0.0;
                s.grounded = false;
            }
        }
    }
    let velocity = u.velocity();
    before.0 != c.health.to_bits() || before.1 != c.last_attacker || before.2 != c.dead || before.3 != c.blow_apart
        || before.4.x.to_bits() != velocity.x.to_bits() || before.4.y.to_bits() != velocity.y.to_bits()
        || before.4.z.to_bits() != velocity.z.to_bits() || before.5 != blast_shield(u)
}

// PlayerCombat.explode can spend a shield without health damage. Compare its
// exact stored values, preserving NaN and signed-zero state in publications.
// PlayerCombat.explode distinguishes a deployed droideka's live shield.
// The shield identifier does not turn a rolled body into an exposed soldier.
// These facts are read from the current canonical body before damage.
// An absent shield keeps ordinary body damage behavior.
fn blast_shield(unit: &Unit) -> Option<(u64,bool)> {
    match &unit.soldier {
        UnitBody::Droideka(body) => body.shield.as_ref().map(|shield| (shield.value.to_bits(),shield.on)),
        _ => None,
    }
}

/// falloff(distance, inner, outer).
// PlayerCombat.explode keeps the original inner/outer radius arithmetic.
// Inside the inner radius the full effect applies.
// At the outer boundary the existing comparisons determine the zero result.
// Do not combine damage and push falloff because their radii can differ.
fn falloff(distance: f64, inner: f64, outer: f64) -> f64 {
    if distance < inner {
        return 1.0;
    }
    if distance < outer && outer > inner {
        return (outer - distance) / (outer - inner);
    }
    0.0
}

/// damage(amount, onShield, source, attacker, info), dispatched on the game
/// family: a 2005 battle puts its timed modifiers on the amounts first; every
/// other battle runs the released body below and nothing else.
#[allow(clippy::too_many_arguments)]
pub fn damage_with<W: ImpactWorld + ?Sized>(c: &mut Combat, u: &mut Unit, defs: &CombatDefs, amount: f64, on_shield: f64, source: Option<Vector3>, attacker: Attacker, info: &DamageInfo, world: &mut W) {
    match world.impact_family() {
        bfe_family::GameFamily::Swbf2 => swbf2_damage_with(c, u, defs, amount, on_shield, source, attacker, info, world),
        _ => released_damage_with(c, u, defs, amount, on_shield, source, attacker, info, world),
    }
}

/// A 2005 battle's hit: the attacker's offence and the victim's defence
/// modifiers scale both amounts, then the released body applies them.
#[allow(clippy::too_many_arguments)]
fn swbf2_damage_with<W: ImpactWorld + ?Sized>(c: &mut Combat, u: &mut Unit, defs: &CombatDefs, amount: f64, on_shield: f64, source: Option<Vector3>, attacker: Attacker, info: &DamageInfo, world: &mut W) {
    let (amount, on_shield) = match world.impact_scale(attacker) {
        Some(scale) => (amount * scale, on_shield * scale),
        None => (amount, on_shield),
    };
    released_damage_with(c, u, defs, amount, on_shield, source, attacker, info, world);
}

/// damage(amount, onShield, source, attacker, info).
#[allow(clippy::too_many_arguments)]
// PlayerCombat.damage commits health and attribution before death hooks.
// Shield damage is independent of typed body damage and may absorb it first.
// The original source and damage-info distinctions survive into the notice.
// Lethal damage detaches seats and can trigger an immediate nested explosion.
// No queued task defers death to the following registry victim or world tick.
// A corpse rejects later damage while its removal timer keeps advancing.
fn released_damage_with<W: ImpactWorld + ?Sized>(c: &mut Combat, u: &mut Unit, defs: &CombatDefs, amount: f64, on_shield: f64, source: Option<Vector3>, attacker: Attacker, info: &DamageInfo, world: &mut W) {
    let _ = defs;
    let mut amount = amount;
    if c.dead || amount <= 0.0 {
        return;
    }
    if let UnitBody::Droideka(d) = &mut u.soldier {
        let mut hit = None;
        amount = d.absorb(amount, on_shield, |_, taken| hit = Some(taken));
        let left = d.shield.as_ref().map_or(0.0, |s| s.value);
        if let Some(taken) = hit {
            world.impact_shield(taken, left, source, info.explosion);
        }
    }
    if amount <= 0.0 {
        return;
    }
    c.health -= amount;
    c.last_attacker = attacker;
    world.impact_damaged(amount, attacker, info, source, u, c);
    if c.health <= 0.0 {
        let left = c.health;
        c.health = 0.0;
        c.dead = true;
        c.death_time = 0.0;
        c.zoom_step = 0.0;
        let air_thrown = matches!(&u.soldier, UnitBody::Soldier(s) if s.air == Some(Air::Thrown));
        let apart = truthy(c.chunks) && !air_thrown && (c.blow_apart || world.impact_draw(c.random) <= c.chunks);
        c.blow_apart = apart;
        if apart {
            world.impact_blown_apart(left, u, c);
        }
        world.impact_died(u, c);
    }
}

/// reset(): full health and a fresh loadout.
// PlayerCombat.reset starts a new arsenal and restores class health.
// The body's stable Unit and vector identities survive a pooled respawn.
// Retained seen and weapon buffers are cleared or reset in place.
// Fields the JS reset left alone keep their original lifetime behavior.
pub fn reset(c: &mut Combat, u: &Unit, defs: &CombatDefs) {
    c.health = c.max_health;
    c.dead = false;
    c.blow_apart = false;
    c.zoom_step = 0.0;
    c.kick = Kick { rate: 0.0, hold: 0.0, offset: 0.0 };
    // The JS arsenal facade is a new identity each life. Its canonical rows
    // can retain their Vec while Weapon.create resets every dynamic field.
    arm_into(defs.loadout(u.class_def), &defs.weapons, c.player, &mut c.weapons);
    c.selected = [0.0, 0.0];
    c.seen.resize_with(c.weapons.len(),Seen::default);
    c.seen.fill(Seen::default());
    c.pending_muzzles.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body_with_air(air: Option<Air>) -> Body {
        Body {
            droideka: false,
            position: Vector3::default(),
            velocity: Vector3::default(),
            aim_yaw: 0.0,
            aim_pitch: 0.0,
            posture: Posture::Stand,
            air,
            dive: 0.0,
            lock: 0.0,
            rolled: false,
        }
    }

    #[test]
    fn swbf2_allows_jump_and_fall_fire_but_still_blocks_thrown_and_dive() {
        for air in [Air::Jump, Air::Fall] {
            let b = body_with_air(Some(air));
            assert!(fire_blocked(&b, false, 0.0, false));
            assert!(!fire_blocked(&b, false, 0.0, true));
        }
        let jet = body_with_air(Some(Air::Jet));
        assert!(!fire_blocked(&jet, false, 0.0, false));
        assert!(!fire_blocked(&jet, false, 0.0, true));

        let thrown = body_with_air(Some(Air::Thrown));
        assert!(fire_blocked(&thrown, false, 0.0, false));
        assert!(fire_blocked(&thrown, false, 0.0, true));

        let mut diving = body_with_air(None);
        diving.dive = 1.0;
        assert!(fire_blocked(&diving, false, 0.0, true));
        assert!(fire_blocked(&body_with_air(None), true, 0.0, true));
    }

    #[test]
    fn swbf2_no_chaining_emitter_hits_multiple_cone_targets_and_emits_flame_and_burn() {
        use super::super::defs::{ArcDef, Js, JsObj, WeaponClass};
        use super::super::weapons::{Admission, WeaponDraws};
        use super::super::world::{CastHit, Droid};
        use crate::gen::{RepairTarget, Weapon};
        use crate::store::StrId;
        use std::rc::Rc;

        struct FakeArcWorld {
            targets: Vec<ArcTarget>,
            hurts: Vec<(usize, f64)>,
            burns: Vec<(usize, f64, f64, Option<String>)>,
            effects: Vec<(String, Vector3, Option<Vector3>)>,
        }
        impl WeaponDraws for FakeArcWorld {
            fn draw(&mut self, _: u32) -> f64 { 0.5 }
            fn legacy(&mut self, _: u32) -> f64 { 0.5 }
            fn seeded(&mut self, _: f64, _: f64) {}
            fn admit(&mut self, _: u32) -> Admission { Admission::Yes }
        }
        impl CombatWorld for FakeArcWorld {
            fn cast(&mut self, _: &Vector3, _: &Vector3, _: f64, _: bool) -> Option<CastHit> { None }
            fn remote_of(&mut self, _: u32) -> Option<Droid> { None }
            fn remote_enter(&mut self, _: Droid, _: &mut Unit, _: &mut Combat) {}
            fn remote_kill(&mut self, _: Droid, _: &mut Unit, _: &mut Combat) {}
            fn remote_launch(&mut self, _: u32, _: &Vector3, _: &Vector3, _: f64, _: f64, _: Option<f64>, _: &mut Unit, _: &mut Combat) {}
            fn lock_on(&mut self, _: &Vector3, _: &Vector3, _: u32, _: &Weapon, _: Option<RepairTarget>, _: bool, _: f64) -> Option<RepairTarget> { None }
            fn lock_key(&mut self, _: Option<RepairTarget>) -> StrId { StrId(0) }
            fn repair(&mut self, _: u32, _: Option<RepairTarget>, _: f64, _: &mut Unit, _: &mut Combat) -> bool { false }
            fn lock_pick(&mut self, _: u32, _: &Weapon, _: &Vector3, _: &Vector3) -> LockPick { LockPick::None }
            fn fire_round(&mut self, _: &Vector3, _: &Vector3, _: &Shot, _: bool, _: Option<(f64, f64)>, _: &mut Unit, _: &mut Combat) {}
            fn dispense(&mut self, _: u32, _: f64, _: &Vector3, _: &Vector3, _: f64, _: f64, _: &mut Unit, _: &mut Combat) -> bool { false }
            fn melee_strike(&mut self, _: &mut Unit, _: &mut Combat) {}
            fn arc_targets(&mut self) -> Vec<ArcTarget> { self.targets.clone() }
            fn hurt_arc_target(&mut self, index: usize, amount: f64, _: &Vector3, _: &mut Unit, _: &mut Combat) {
                self.hurts.push((index, amount));
            }
            fn apply_burn_arc_target(&mut self, index: usize, dps: f64, seconds: f64, effect: Option<&str>) {
                self.burns.push((index, dps, seconds, effect.map(str::to_string)));
            }
            fn emit_swbf2_effect(&mut self, name: &str, position: &Vector3, forward: Option<&Vector3>, _: Option<f64>) {
                self.effects.push((name.to_string(), *position, forward.copied()));
            }
            fn has_lightning(&mut self, _: &str) -> bool { false }
            fn fired(&mut self, _: &Shot, _: &mut Unit, _: &mut Combat) {}
            fn damaged(&mut self, _: f64, _: Attacker, _: &DamageInfo, _: Option<Vector3>, _: &mut Unit, _: &mut Combat) {}
            fn died(&mut self, _: &mut Unit, _: &mut Combat) {}
            fn blown_apart(&mut self, _: f64, _: &mut Unit, _: &mut Combat) {}
            fn respawn(&mut self, _: &mut Unit, _: &mut Combat) {}
            fn presentation(&mut self, _: &Presentation) {}
            fn next_gun(&mut self, _: &mut Unit, _: &mut Combat) -> Option<Vector3> { None }
            fn gun_at(&mut self, _: (f64, f64)) -> Option<Vector3> { None }
            fn shield_hit(&mut self, _: f64, _: f64, _: Option<Vector3>, _: bool) {}
            fn draw_combat(&mut self, _: bool) -> f64 { 0.5 }
            fn text(&self, _: StrId) -> String { String::new() }
            fn battle_family(&self) -> bfe_family::GameFamily { bfe_family::GameFamily::Swbf2 }
        }

        let mut defs = CombatDefs::default();
        let w_fields = vec![
            (Rc::from("base"), Js::Str("cannon".into())),
            (Rc::from("channel"), Js::Num(0.0)),
        ];
        defs.weapons.set(DefId(0), WeaponClass::new(Rc::new(JsObj { fields: w_fields, def: None })));
        let mut combat = Combat::default();
        combat.weapons.push(Weapon::create(DefId(0), &defs.weapons, true));
        combat.origin = [0.0, 1.0, 0.0];
        combat.shot_dir = [0.0, 0.0, 1.0];
        let mut unit = Unit::default();

        let arc_def = ArcDef {
            length: 14.0,
            sine: v8math::sin(16.0_f64.to_radians()),
            name: None,
            damage: 11.0,
            max_damage: 19.0,
            first_radius: 3.0,
            max_targets: 4.0,
            threshold: 0.0,
            jump: 0.0,
            drop: 0.0,
            no_chaining: true,
            affect_friends: true,
            affect_enemies: true,
            smolder_damage_rate: 22.0,
            smolder_timer: 2.5,
            smolder_effect: Some(Rc::from("inv_burn_fx")),
            aimer_effect: Some(Rc::from("inv_cone_stream")),
        };

        let mut world = FakeArcWorld {
            targets: vec![
                // In cone at 4 m.
                ArcTarget { origin: Vector3::new(0.2, 0.0, 4.0), center: None, radius: None, relation: -1.0 },
                // In cone at 11 m (jump=0 must not stop NoChaining cone!).
                ArcTarget { origin: Vector3::new(-0.4, 0.0, 11.0), center: None, radius: None, relation: -1.0 },
                // Directly behind shooter on the aim axis at -0.5 m (inside broadphase AABB around 0.9*14=12.6
                // with radius 1.0, and zero perpendicular offset so the double-cone check would pass):
                // only NoChaining's `along > 0` check excludes it!
                ArcTarget { origin: Vector3::new(0.0, 1.0, -0.5), center: Some(Vector3::new(0.0, 1.0, -0.5)), radius: Some(1.0), relation: -1.0 },
            ],
            hurts: Vec::new(),
            burns: Vec::new(),
            effects: Vec::new(),
        };
        let shot = Shot {
            weapon: 0,
            kind: ShotKind::Round { yaw: 0.0, pitch: 0.0, charge: 1.0, target_id: -1.0, target_generation: -1.0, admission: None },
        };
        let mut scratch = ArcScratch::default();
        arc(&mut combat, &mut unit, &defs, &shot, &arc_def, &Edges::default(), &mut world, &mut scratch);

        assert_eq!(world.hurts, vec![(0, 19.0), (1, 19.0)]);
        assert_eq!(world.burns.len(), 2);
        assert_eq!(world.burns[0].1, 22.0);
        assert_eq!(world.burns[0].2, 2.5);
        assert_eq!(world.burns[0].3.as_deref(), Some("inv_burn_fx"));
        assert_eq!(world.effects.len(), 1);
        assert_eq!(world.effects[0].0, "inv_cone_stream");
        assert_eq!(world.effects[0].2, Some(Vector3::new(0.0, 0.0, 1.0)));

        // AffectFriends = false skips friendly targets; AffectEnemies = false skips enemy targets.
        let mut friend_only = arc_def.clone();
        friend_only.affect_friends = true;
        friend_only.affect_enemies = false;
        let mut enemy_only = arc_def;
        enemy_only.affect_friends = false;
        enemy_only.affect_enemies = true;

        world.targets = vec![
            ArcTarget { origin: Vector3::new(0.1, 0.0, 4.0), center: None, radius: None, relation: 1.0 },
            ArcTarget { origin: Vector3::new(-0.1, 0.0, 5.0), center: None, radius: None, relation: -1.0 },
        ];
        world.hurts.clear();
        world.burns.clear();
        arc(&mut combat, &mut unit, &defs, &shot, &enemy_only, &Edges::default(), &mut world, &mut scratch);
        assert_eq!(world.hurts, vec![(1, 19.0)]);
        assert_eq!(world.burns.len(), 1);
        assert_eq!(world.burns[0].0, 1);

        world.hurts.clear();
        world.burns.clear();
        arc(&mut combat, &mut unit, &defs, &shot, &friend_only, &Edges::default(), &mut world, &mut scratch);
        assert_eq!(world.hurts, vec![(0, 19.0)]);
        assert_eq!(world.burns.len(), 1);
        assert_eq!(world.burns[0].0, 0);
    }

    #[test]
    fn swbf2_guided_launch_falls_back_when_offset_obstructed_and_reconverges_when_clear() {
        use super::super::defs::{Js, JsObj, WeaponClass};
        use super::super::weapons::{Admission, WeaponDraws};
        use super::super::world::{CastHit, Droid};
        use crate::gen::{RepairTarget, Weapon};
        use crate::store::StrId;
        use std::rc::Rc;

        struct FakeGuidedLaunchWorld {
            wall_dist: Option<f64>,
            aim_dist: f64,
            launches: Vec<(Vector3, Vector3)>,
        }
        impl WeaponDraws for FakeGuidedLaunchWorld {
            fn draw(&mut self, _: u32) -> f64 { 0.5 }
            fn legacy(&mut self, _: u32) -> f64 { 0.5 }
            fn seeded(&mut self, _: f64, _: f64) {}
            fn admit(&mut self, _: u32) -> Admission { Admission::Yes }
        }
        impl CombatWorld for FakeGuidedLaunchWorld {
            fn cast(&mut self, origin: &Vector3, dir: &Vector3, far: f64, _: bool) -> Option<CastHit> {
                let d = self.wall_dist?;
                if d <= far {
                    let mut point = *origin;
                    point.add_scaled_vector(dir, d);
                    return Some(CastHit { distance: d, point, normal: Vector3::new(0.0, 0.0, -1.0) });
                }
                None
            }
            fn aim_reach(&mut self, _: &Unit, _: &Vector3, _: &Vector3, far: f64, _: f64) -> f64 {
                if self.aim_dist < far { self.aim_dist } else { far }
            }
            fn remote_of(&mut self, _: u32) -> Option<Droid> { None }
            fn remote_enter(&mut self, _: Droid, _: &mut Unit, _: &mut Combat) {}
            fn remote_kill(&mut self, _: Droid, _: &mut Unit, _: &mut Combat) {}
            fn remote_launch(&mut self, _: u32, origin: &Vector3, velocity: &Vector3, _: f64, _: f64, _: Option<f64>, _: &mut Unit, _: &mut Combat) {
                self.launches.push((*origin, *velocity));
            }
            fn lock_on(&mut self, _: &Vector3, _: &Vector3, _: u32, _: &Weapon, _: Option<RepairTarget>, _: bool, _: f64) -> Option<RepairTarget> { None }
            fn lock_key(&mut self, _: Option<RepairTarget>) -> StrId { StrId(0) }
            fn repair(&mut self, _: u32, _: Option<RepairTarget>, _: f64, _: &mut Unit, _: &mut Combat) -> bool { false }
            fn lock_pick(&mut self, _: u32, _: &Weapon, _: &Vector3, _: &Vector3) -> LockPick { LockPick::None }
            fn fire_round(&mut self, _: &Vector3, _: &Vector3, _: &Shot, _: bool, _: Option<(f64, f64)>, _: &mut Unit, _: &mut Combat) {}
            fn dispense(&mut self, _: u32, _: f64, _: &Vector3, _: &Vector3, _: f64, _: f64, _: &mut Unit, _: &mut Combat) -> bool { false }
            fn melee_strike(&mut self, _: &mut Unit, _: &mut Combat) {}
            fn arc_targets(&mut self) -> Vec<ArcTarget> { Vec::new() }
            fn hurt_arc_target(&mut self, _: usize, _: f64, _: &Vector3, _: &mut Unit, _: &mut Combat) {}
            fn has_lightning(&mut self, _: &str) -> bool { false }
            fn fired(&mut self, _: &Shot, _: &mut Unit, _: &mut Combat) {}
            fn damaged(&mut self, _: f64, _: Attacker, _: &DamageInfo, _: Option<Vector3>, _: &mut Unit, _: &mut Combat) {}
            fn died(&mut self, _: &mut Unit, _: &mut Combat) {}
            fn blown_apart(&mut self, _: f64, _: &mut Unit, _: &mut Combat) {}
            fn respawn(&mut self, _: &mut Unit, _: &mut Combat) {}
            fn presentation(&mut self, _: &Presentation) {}
            fn next_gun(&mut self, _: &mut Unit, _: &mut Combat) -> Option<Vector3> { None }
            fn gun_at(&mut self, _: (f64, f64)) -> Option<Vector3> { None }
            fn shield_hit(&mut self, _: f64, _: f64, _: Option<Vector3>, _: bool) {}
            fn draw_combat(&mut self, _: bool) -> f64 { 0.5 }
            fn text(&self, _: StrId) -> String { String::new() }
            fn battle_family(&self) -> bfe_family::GameFamily { bfe_family::GameFamily::Swbf2 }
        }

        let mut defs = CombatDefs::default();
        let droid_obj = Rc::new(JsObj {
            fields: vec![(Rc::from("GuidedMissile"), Js::Num(1.0))],
            def: None,
        });
        let extra_arr = Js::Arr(Rc::new(vec![Js::Num(0.0), Js::Num(0.4), Js::Num(2.0)]));
        let w_fields = vec![
            (Rc::from("base"), Js::Str("remote".into())),
            (Rc::from("channel"), Js::Num(1.0)),
            (Rc::from("droid"), Js::Obj(droid_obj)),
            (Rc::from("FirePointExtraOffset"), extra_arr),
            (Rc::from("Velocity"), Js::Num(20.0)),
        ];
        defs.weapons.set(DefId(0), WeaponClass::new(Rc::new(JsObj { fields: w_fields, def: None })));
        let mut combat = Combat::default();
        combat.weapons.push(Weapon::create(DefId(0), &defs.weapons, true));
        combat.origin = [0.0, 1.5, 0.0];
        combat.shot_dir = [0.0, 0.0, 1.0];
        let mut unit = Unit { human: true, network_human: Some(true), ..Default::default() };
        let edges = Edges {
            fixed: Some([None, Some(Edge { button: 1.0, id: Some(1.0), late: None, aim_yaw: Some(0.0), aim_pitch: Some(0.0) })]),
            ..Default::default()
        };
        let shot = Shot {
            weapon: 0,
            kind: ShotKind::Round { yaw: 0.0, pitch: 0.0, charge: 1.0, target_id: -1.0, target_generation: -1.0, admission: None },
        };
        let mut scratch = ArcScratch::default();

        // Clear path, reticle target 12 m ahead: offset applies and velocity re-converges onto (0, 1.5, 12).
        let mut world = FakeGuidedLaunchWorld { wall_dist: None, aim_dist: 12.0, launches: Vec::new() };
        fire(&mut combat, &mut unit, &defs, &shot, &edges, &mut world, &mut scratch);
        let (orig, vel) = world.launches[0];
        assert!((orig.y - 1.9).abs() < 1e-6 && (orig.z - 2.0).abs() < 1e-6, "clear offset must apply: {orig:?}");
        let t_to_target = (12.0 - orig.z) / vel.z;
        let y_at_target = orig.y + vel.y * t_to_target;
        assert!((y_at_target - 1.5).abs() < 1e-5, "re-converged ray must hit target height 1.5: got {y_at_target}");

        // Wall at 1.2 m along offset: falls back to unobstructed fire origin rather than embedding on wall.
        let mut world = FakeGuidedLaunchWorld { wall_dist: Some(1.2), aim_dist: 12.0, launches: Vec::new() };
        fire(&mut combat, &mut unit, &defs, &shot, &edges, &mut world, &mut scratch);
        let (orig, _) = world.launches[0];
        assert!((orig.y - 1.5).abs() < 1e-6 && orig.z.abs() < 1e-6, "wall obstruction must fall back to origin: {orig:?}");

        // Close target at 1.4 m (closer than offset): falls back to origin rather than spawning behind target.
        let mut world = FakeGuidedLaunchWorld { wall_dist: None, aim_dist: 1.4, launches: Vec::new() };
        fire(&mut combat, &mut unit, &defs, &shot, &edges, &mut world, &mut scratch);
        let (orig, _) = world.launches[0];
        assert!((orig.y - 1.5).abs() < 1e-6 && orig.z.abs() < 1e-6, "close target must fall back to origin: {orig:?}");
    }
}
