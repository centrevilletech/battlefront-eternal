//! A soldier's side of the fight (src/combat.js PlayerCombat, with
//! src/combat-weapon-input.js, src/combat-input.js,
//! src/runtime/network-shot-aim.js and src/heroes.js regenerate): his aim,
//! his triggers and weapons, the rounds, items, droids and lightning they
//! fire, a repair tool's lock and a launcher's, the aim's kick, what
//! explosions and hurts do to him, his death and a droid's blowing apart.
//!
//! The world's combats only: a bot's or a network player's (no camera, no
//! HUD, no first-person view: a world draws nothing, and its combats aim
//! along their aimer's direction or their own aim). What only draws (the
//! weapon in the hand, flashes, lights, the animator's clips, sounds) is
//! the page's and is not here. Line numbers are src/combat.js's on
//! rust/sim-p11-combat.

use super::impacts::{BlastPacket, HealthScale, ImpactWorld};
use super::defs::{WeaponBase, Js, JsObj, WeaponClass, WeaponDefs, K};
use super::weapons::{arm_into, Aim, Arsenal, ArsenalInput, Shot, ShotKind, Trigger, DEG};
use super::world::{ArcTarget, CombatWorld, DamageInfo, LockPick, Presentation};
use crate::gen::{Air, Attacker, Combat, Kick, PendingShot, Posture, Seen, Unit, UnitBody};
use crate::store::{DefId, Nullable};
use jsrt::conv::{js_max, js_min};
use std::rc::Rc;
use three::Vector3;

const AIM_REACH: f64 = 60.0;
const DROIDEKA_REACH: f64 = 512.0;
const RESPAWN_DELAY: f64 = 4.0;
const THROWN_SPEED: f64 = 2.0;
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
fn blast_body(p: Posture) -> [f64; 3] {
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
struct ClassFacts { add_health: f64, walkerdroid: bool, loadout:Vec<DefId> }

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
        self.facts[i] = ClassFacts {
            add_health:obj.get("AddHealth").num(),walkerdroid:obj.get("base").is_str("walkerdroid"),loadout,
        };
        self.classes[i] = Some(obj);
    }

    pub fn class(&self, id: DefId) -> &JsObj {
        self.classes[id.0 as usize].as_ref().expect("a unit class the battle registered")
    }

    /// A class's loadout (`def.weapons || []`), as weapon classes.
    pub fn loadout(&self, id: DefId) -> &[DefId] { &self.facts[id.0 as usize].loadout }
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
/// droideka from its eye (droideka.js aim).
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

/// infantryFireBlocked(soldier, gunUnderwater, stick) (combat-input.js:2).
// PlayerCombat gates fire before updating its arsenal.
// A dive, get-up or underwater gun can suppress an otherwise valid press.
// Preserve the existing comparisons so NaN takes the same branches as JS.
// Blocked input does not silently become a new press on the next tick.
// PlayerCombat keeps locomotion and weapon refusal as distinct state.
// A blocked tick may still update timers and kick in the surrounding solver.
// The retained control edges are consumed at their original observation point.
// Reordering this check can accidentally make a held trigger fire again.
fn fire_blocked(b: &Body, gun_underwater: bool, stick: f64) -> bool {
    (b.air.is_some_and(|air| air != Air::Jet) || truthy(b.dive) || b.lock > 0.0 || b.rolled || (b.posture == Posture::Prone && stick > 0.1)) || gun_underwater
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

/// readWeaponInput, seedPresses and latePresses (combat-weapon-input.js).
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

/// networkShotTarget(owner, eye, target, channel).
// PlayerCombat converges the sampled press direction from its current eye.
// This target is separate from the later muzzle's actual origin.
// Keeping both points preserves close-range parallax for instant shots.
// No historical body pose is copied into the live Unit.
fn network_shot_target(u: &Unit, edges: &Edges, eye: &Vector3, target: &mut Vector3, channel: f64) {
    if let Some((yaw, pitch)) = edge_aim(u, edges, channel) {
        let reach = target.distance_to(eye);
        if !reach.is_finite() || reach <= 0.0 {
            return;
        }
        let (sin, cos) = v8math::sin_cos(pitch);
        let (sy, cy) = v8math::sin_cos(yaw);
        target.set(eye.x + sy * cos * reach, eye.y + sin * reach, eye.z + cy * cos * reach);
    }
}

/// regenerate(combat, dt) (heroes.js:6).
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
// The page later applies it to drawn bones without advancing authority.
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
    let blocked = fire_blocked(&b, c.gun_underwater, stick);
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
    arsenal(c).update(&defs.weapons, dt, &input, &aim_in, blocked, world, shots);
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
        let heat = c.weapons[i].level(class).get(K::HeatPerShot).or(0.0);
        let target = c.repair_target;
        if world.repair(i as u32, target, c.team, u, c) {
            c.weapons[i].heat += heat;
        }
        return;
    }
    let level = c.weapons[i].level(class);
    if level.get(K::Item).truthy() {
        dispense(c, u, defs, shot, world);
        return;
    }
    if level.get(K::Droid).truthy() {
        // A remote droid leaves the gun origin along the aim at Velocity,
        // plus the soldier's own velocity.
        let speed = level.get(K::Velocity).nn(10.0) * level.get(K::MaxStrength).nn(1.0);
        let mut velocity = v(c.shot_dir);
        velocity.multiply_scalar(speed).add(&body(u).velocity);
        let admission = match shot.kind {
            ShotKind::Round { admission, .. } => admission,
            _ => None,
        };
        let (origin, yaw, team) = (v(c.origin), body(u).aim_yaw, c.team);
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
    // Collision starts at the gameplay gun origin.
    let mut dir = v(c.shot_dir);
    let mut from = v(c.origin);
    // A droideka's guns take turns, each firing at the aim point.
    let gun = if matches!(u.soldier, UnitBody::Droideka(_)) { world.next_gun(u, c) } else { None };
    if let Some(g) = gun {
        from = g;
        dir.sub_vectors(&v(c.target), &g).normalize();
        // A close surface between the eye and barrels cannot make a shot turn
        // back through the droideka: keep firing into the forward hemisphere.
        if dir.dot(&v(c.aim_dir)) <= 0.0 {
            dir = v(c.aim_dir);
        }
    }
    let channel = c.weapons[i].channel(&defs.weapons);
    network_shot_direction(u, edges, &mut dir, channel);
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
    if gun.is_some() {
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
            forward: c.aim_dir,
            kick: c.kick.offset,
        };
        if c.defer_muzzles {
            c.pending_muzzles.push(payload);
        } else {
            emit_gun_round(c, u, defs, &payload, edges, world);
        }
    } else {
        let thrown = c.weapons[i].thrown(&defs.weapons);
        world.fire_round(&from, &dir, shot, !thrown, None, u, c);
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
// Pending shots retain their original weapon, spread and captured trigger edge.
// Use the newly posed barrel while keeping that shot's earlier gameplay facts.
// The shared buffer is cleared without releasing its construction capacity.
// PlayerCombat does not recollect or reseed shots during this flush.
// Its pending records describe work already admitted by Arsenal.update.
// A later barrel pose changes only the muzzle service's sampled frame.
// The shot buffer's lifetime ends after every retained emission completes.
pub fn flush_muzzles_buffered(c: &mut Combat, u: &mut Unit, defs: &CombatDefs, edges: &Edges, world: &mut dyn CombatWorld, pending: &mut Vec<PendingShot>) {
    pending.clear(); pending.extend_from_slice(&c.pending_muzzles);
    for payload in pending.iter() { emit_gun_round(c, u, defs, payload, edges, world); }
    c.pending_muzzles.clear();
}

/// emitGunRound(payload).
// PlayerCombat.fireRound transforms the prepared muzzle at fire time.
// The muzzle points at the retained aim target before spread is applied.
// Alternate barrels advance once per emitted round in the original order.
// F1 receives native service handles rather than a callback into its export.
fn emit_gun_round(c: &mut Combat, u: &mut Unit, defs: &CombatDefs, p: &PendingShot, edges: &Edges, world: &mut dyn CombatWorld) {
    let Some(from) = world.gun_at((p.selected_index, p.selected_barrel)) else { return };
    let i = p.weapon as usize;
    let channel = c.weapons[i].channel(&defs.weapons);
    let mut target = v(p.target);
    network_shot_target(u, edges, &v(c.origin), &mut target, channel);
    let forward = v(p.forward);
    let mut dir = Vector3::default();
    dir.sub_vectors(&target, &from).normalize();
    if dir.dot(&forward) <= 0.0 {
        dir = forward;
    }
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
    let walkerdroid = defs.facts[u.class_def.0 as usize].walkerdroid;
    let was_dead = c.dead;
    // Nothing is left to throw of a droid that went up in pieces, and a
    // droideka's hull never takes the infantry launch impulse.
    if was_dead && (c.blow_apart || walkerdroid) {
        return false;
    }
    let b = body(u);
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
    let sight_point = Vector3 { x: b.position.x, y: b.position.y + sight, z: b.position.z };
    if blast_blocked(world, position, &sight_point) {
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
    if !was_dead {
        let generic = explosion.hit.shield;
        let info = DamageInfo { explosion: true, amount: Some(damage), typed: Some(typed), generic: Some(generic), known: Some(false), point: Some(None) };
        damage_with(c, u, defs, damage * typed, damage * generic, Some(*position), owner, &info, world);
    }
    // Thrown whole, alive or killed by this blast; a droid that blew apart
    // at its death has no body left; droidekas never take the push.
    if push > 0.0 && c.mounted != Some(true) && !(c.dead && c.blow_apart) && !walkerdroid {
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

/// falloff(distance, inner, outer) (ordnance.js:1023).
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

/// blastBlocked(collision, position, target) (ordnance.js:1064): terrain or
/// a building (never a moving body) between them.
// PlayerCombat.explode uses static-only cover for its line of sight.
// A near-zero ray cannot normalize safely and therefore stays unobstructed.
// The final tolerance avoids treating the victim's own surface as cover.
// Moving hulls follow their separate blast path and do not shelter this ray.
fn blast_blocked<W: ImpactWorld + ?Sized>(world: &mut W, position: &Vector3, target: &Vector3) -> bool {
    let mut sight = Vector3::default();
    let span = sight.sub_vectors(target, position).length();
    if span < 1e-3 {
        return false;
    }
    sight.divide_scalar(span);
    match world.impact_cast(position, &sight, span, true) {
        Some(hit) => hit.distance < span - 0.05,
        None => false,
    }
}

/// damage(amount, onShield, source, attacker, info).
#[allow(clippy::too_many_arguments)]
// PlayerCombat.damage commits health and attribution before death hooks.
// Shield damage is independent of typed body damage and may absorb it first.
// The original source and damage-info distinctions survive into the notice.
// Lethal damage detaches seats and can trigger an immediate nested explosion.
// No queued task defers death to the following registry victim or world tick.
// A corpse rejects later damage while its removal timer keeps advancing.
pub fn damage_with<W: ImpactWorld + ?Sized>(c: &mut Combat, u: &mut Unit, defs: &CombatDefs, amount: f64, on_shield: f64, source: Option<Vector3>, attacker: Attacker, info: &DamageInfo, world: &mut W) {
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
