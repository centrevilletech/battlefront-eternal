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

use super::defs::{Js, JsObj, WeaponClass, WeaponDefs, K};
use super::weapons::{arm, Aim, Arsenal, ArsenalInput, Shot, ShotKind, Trigger, DEG};
use super::world::{CombatWorld, DamageInfo, LockPick, Presentation};
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
const KICK_OFFSET_FADE: f64 = 6.0;
const KICK_HOLD_DRAIN: f64 = 2.0;
const UP: Vector3 = Vector3 { x: 0.0, y: 1.0, z: 0.0 };

/// The aim pivot and gun origin per posture (combat.js:106-107).
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

/// BLAST_BODY (combat.js:116): the sphere a blast measures to, by stance
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
}

impl CombatDefs {
    pub fn set_class(&mut self, id: DefId, obj: Rc<JsObj>) {
        let i = id.0 as usize;
        if self.classes.len() <= i {
            self.classes.resize(i + 1, None);
        }
        self.classes[i] = Some(obj);
    }

    pub fn class(&self, id: DefId) -> &JsObj {
        self.classes[id.0 as usize].as_ref().expect("a unit class the battle registered")
    }

    /// A class's loadout (`def.weapons || []`), as weapon classes.
    pub fn loadout(&self, id: DefId) -> Vec<DefId> {
        let list = self.class(id).get("weapons");
        (0..list.len()).filter_map(|i| list.at(i).obj().and_then(|o| o.def).map(|(_, d)| d)).collect()
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
}

impl Edges {
    /// networkShotFor(owner, channel).
    pub fn for_channel(&self, channel: f64) -> Option<Edge> {
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
fn roll(c: &Combat, world: &mut dyn CombatWorld) -> f64 {
    world.draw_combat(c.random)
}

/// PlayerCombat.zooms(def) (combat.js:272): how many zoom steps.
fn zoom_count(d: &super::defs::Level) -> usize {
    let mut out = vec![1.0];
    let min = d.get(K::ZoomMin).or(1.0);
    let max = if d.get(K::ZoomRate).truthy() { 0.0 } else { d.get(K::ZoomMax).or(1.0) };
    for zoom in [min, max] {
        if zoom > *out.last().unwrap() {
            out.push(zoom);
        }
    }
    out.len()
}

fn arsenal<'a>(c: &'a mut Combat) -> Arsenal<'a> {
    Arsenal { weapons: &mut c.weapons, selected: &mut c.selected }
}

/// The current weapon of a channel.
fn current(c: &mut Combat, defs: &CombatDefs, channel: usize) -> Option<usize> {
    arsenal(c).current(&defs.weapons, channel)
}

/// The class of the weapon at `i`.
fn class_of<'d>(c: &Combat, defs: &'d CombatDefs, i: usize) -> &'d WeaponClass {
    defs.weapons.class(c.weapons[i].def)
}

/// The aim pivot's and gun origin's frame for the soldier's own aim
/// (aimBasis, combat.js:317).
fn aim_basis(c: &mut Combat, b: &Body) {
    let (yaw, pitch) = (b.aim_yaw, b.aim_pitch);
    let posture = if b.air.is_some() { Posture::Stand } else { b.posture };
    let (sy, cy, sp, cp) = (v8math::sin(yaw), v8math::cos(yaw), v8math::sin(pitch), v8math::cos(pitch));
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

/// aim() (combat.js:366) for a combat without a camera: from the gun
/// origin along the aimer's direction, or straight along the aim; a
/// droideka from its eye (droideka.js aim).
fn aim(c: &mut Combat, b: &Body, eye_height: f64) {
    let reach;
    if b.droideka {
        let (yaw, p) = (b.aim_yaw, b.aim_pitch);
        let (sy, cy, sp, cp) = (v8math::sin(yaw), v8math::cos(yaw), v8math::sin(p), v8math::cos(p));
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
fn fire_blocked(b: &Body, gun_underwater: bool, stick: f64) -> bool {
    (b.air.is_some_and(|air| air != Air::Jet) || truthy(b.dive) || b.lock > 0.0 || b.rolled || (b.posture == Posture::Prone && stick > 0.1)) || gun_underwater
}

/// What update reads of the unit's controls (IntentControls take/held).
fn take(u: &mut Unit, name: &str) -> bool {
    match name {
        "firePrimary" => std::mem::replace(&mut u.controls.pressed[0], false),
        "fireSecondary" => std::mem::replace(&mut u.controls.pressed[1], false),
        "use" => std::mem::replace(&mut u.intent.r#use, false),
        "reload" => std::mem::replace(&mut u.intent.reload, false),
        "nextPrimary" => std::mem::replace(&mut u.intent.next_primary, false),
        "previousPrimary" => std::mem::replace(&mut u.intent.previous_primary, false),
        "nextSecondary" => std::mem::replace(&mut u.intent.next_secondary, false),
        "zoom" => std::mem::replace(&mut u.intent.zoom, false),
        _ => false,
    }
}

fn held(u: &Unit, channel: usize) -> bool {
    u.intent.buttons[channel] > 0.0
}

/// readWeaponInput, seedPresses and latePresses (combat-weapon-input.js).
fn weapon_input(id: u32, u: &mut Unit, edges: &Edges) -> ArsenalInput {
    let primary = take(u, "firePrimary");
    let secondary = take(u, "fireSecondary");
    let mut input = ArsenalInput {
        primary: Trigger { held: held(u, 0) || primary, pressed: primary, seed: None, late: 0.0 },
        secondary: Trigger { held: held(u, 1) || secondary, pressed: secondary, seed: None, late: 0.0 },
        reload: take(u, "reload"),
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
fn network_shot_direction(u: &Unit, edges: &Edges, dir: &mut Vector3, channel: f64) {
    if let Some((yaw, pitch)) = edge_aim(u, edges, channel) {
        let cos = v8math::cos(pitch);
        dir.set(v8math::sin(yaw) * cos, v8math::sin(pitch), v8math::cos(yaw) * cos);
    }
}

/// networkShotTarget(owner, eye, target, channel).
fn network_shot_target(u: &Unit, edges: &Edges, eye: &Vector3, target: &mut Vector3, channel: f64) {
    if let Some((yaw, pitch)) = edge_aim(u, edges, channel) {
        let reach = target.distance_to(eye);
        if !reach.is_finite() || reach <= 0.0 {
            return;
        }
        let cos = v8math::cos(pitch);
        target.set(eye.x + v8math::sin(yaw) * cos * reach, eye.y + v8math::sin(pitch) * reach, eye.z + v8math::cos(yaw) * cos * reach);
    }
}

/// regenerate(combat, dt) (heroes.js:6).
fn regenerate(c: &mut Combat, class: &JsObj, dt: f64) {
    if c.dead || !(c.health > 0.0) || !(dt > 0.0) {
        return;
    }
    let rate = class.get("AddHealth").num();
    let rate = if truthy(rate) { rate } else { 0.0 };
    if rate > 0.0 {
        c.health = js_min(c.max_health, c.health + rate * dt);
    }
}

/// The weapon's animation bank (`def.AnimationBank || "rifle"`).
fn bank(class: &WeaponClass) -> String {
    let b = class.base().get(K::AnimationBank);
    if b.truthy() {
        match b {
            Js::Str(s) => s.to_string(),
            other => format!("{}", other.num()),
        }
    } else {
        "rifle".to_string()
    }
}

fn weapon_action(c: &Combat, defs: &CombatDefs, action: &str, bank_name: String, posture: Option<&'static str>, world: &mut dyn CombatWorld) {
    let prepare = action.starts_with("shoot")
        && c.weapons.iter().any(|w| {
            let class = defs.weapons.class(w.def);
            bank(class) == bank_name && w.thrown(&defs.weapons) && w.busy()
        });
    world.presentation(&Presentation::WeaponAction { action: action.to_string(), bank: bank_name, posture, prepare });
}

/// update(dt, controls, stick) (combat.js:474) for a world's combat.
pub fn update(c: &mut Combat, id: u32, u: &mut Unit, defs: &CombatDefs, dt: f64, stick: f64, edges: &Edges, world: &mut dyn CombatWorld) {
    let class = defs.class(u.class_def);
    regenerate(c, class, dt);
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
        for name in ["firePrimary", "fireSecondary", "reload", "nextPrimary", "previousPrimary", "nextSecondary", "zoom"] {
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
        if c.weapons[g].level(class_of(c, defs, g)).get(K::Base).is_str("shield") {
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
        Some(i) => world.lock_on(&v(c.origin), &v(c.aim_dir), i as u32, c.repair_target, fix_held, c.team),
        None => None,
    };
    c.repair_lock = world.lock_key(c.repair_target);
    for channel in 0..2 {
        let Some(i) = current(c, defs, channel) else { continue };
        let class = class_of(c, defs, i);
        let level = c.weapons[i].level(class);
        if !(level.get(K::Base).is_str("launcher") && level.get(K::Ordnance).get("base").is_str("missile")) {
            continue;
        }
        // The cone runs from the gun through the point under the reticle.
        match world.lock_pick(i as u32, &v(c.origin), &v(c.shot_dir)) {
            LockPick::Nothing => c.weapons[i].clear_lock(),
            LockPick::None => c.weapons[i].update_lock(class, dt, None),
            LockPick::Unit { id, generation, alive } => {
                c.weapons[i].update_lock(class, dt, Some(super::weapons::LockTarget { id, generation, alive }))
            }
        }
    }
    let aim_in = Aim { stick, posture: Some(b.posture), zoomed: c.zoom_step > 0.0, repair_target: c.repair_target.is_some() };
    let mut shots = Vec::new();
    arsenal(c).update(&defs.weapons, dt, &input, &aim_in, blocked, world, &mut shots);
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
                weapon_action(c, defs, "reload", bank(class), posture, world);
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
            if w.level(class).get(K::Base).is_str("dispenser") {
                action = "shoot";
            }
            weapon_action(c, defs, action, bank(class), posture, world);
        }
        c.seen[i].reloading = c.weapons[i].reloading();
        c.seen[i].throwing = c.weapons[i].busy();
    }
    for shot in &shots {
        fire(c, u, defs, shot, edges, world);
    }
    update_kick(c, u, dt);
}

/// selectCombatWeapons(combat, controls, zooms) (combat-weapon-input.js:29).
fn select_weapons(c: &mut Combat, u: &mut Unit, defs: &CombatDefs) -> Option<usize> {
    if take(u, "nextPrimary") && arsenal(c).next(&defs.weapons, 0, 1.0) {
        c.zoom_step = 0.0;
    }
    if take(u, "previousPrimary") && arsenal(c).next(&defs.weapons, 0, -1.0) {
        c.zoom_step = 0.0;
    }
    if take(u, "nextSecondary") {
        arsenal(c).next(&defs.weapons, 1, 1.0);
    }
    let primary = current(c, defs, 0);
    if take(u, "zoom") {
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

/// updateKick(dt) (combat.js:629): the aim climbs while the kick holds,
/// and the rate fades.
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

/// fire(shot) (combat.js:664).
fn fire(c: &mut Combat, u: &mut Unit, defs: &CombatDefs, shot: &Shot, edges: &Edges, world: &mut dyn CombatWorld) {
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
        let action = if swing == 1.0 { "shoot".to_string() } else { format!("shoot{}", swing) };
        weapon_action(c, defs, &action, bank(class), posture, world);
        return;
    }
    if shot.kind == ShotKind::Repair {
        // repairTick(weapon) (combat.js:872).
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
    if ordnance.get("base").is_str("emitterordnance") {
        arc(c, u, defs, shot, &ordnance, edges, world);
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

/// flushMuzzles() (combat.js:764): the rounds a droideka's guns owe, after
/// the tick has sampled its gun rig.
pub fn flush_muzzles(c: &mut Combat, u: &mut Unit, defs: &CombatDefs, edges: &Edges, world: &mut dyn CombatWorld) {
    let pending = c.pending_muzzles.clone();
    for payload in &pending {
        emit_gun_round(c, u, defs, payload, edges, world);
    }
    c.pending_muzzles.clear();
}

/// emitGunRound(payload) (combat.js:770).
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

/// dispense(shot) (combat.js:919): a dispenser's item thrown along the aim.
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

/// arc(shot, def) (combat.js:809): the arc caster's lightning.
fn arc(c: &mut Combat, u: &mut Unit, defs: &CombatDefs, shot: &Shot, def: &Js, edges: &Edges, world: &mut dyn CombatWorld) {
    let ShotKind::Round { charge, .. } = shot.kind else { return };
    let i = shot.weapon as usize;
    let channel = c.weapons[i].channel(&defs.weapons);
    let p = v(c.origin);
    let mut d = v(c.shot_dir);
    network_shot_direction(u, edges, &mut d, channel);
    d.normalize();
    let l = def.get("ConeLength").nn(10.0);
    let sine = v8math::sin(((def.get("ConeAngle").nn(60.0) / 2.0) * std::f64::consts::PI) / 180.0);
    let mut center = p;
    center.add_scaled_vector(&d, 0.9 * l);
    let mut toward = Vector3::default();
    #[derive(Clone)]
    struct Candidate {
        index: usize,
        origin: Vector3,
        a: f64,
    }
    let targets = world.arc_targets();
    let mut candidates: Vec<Candidate> = Vec::new();
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
        candidates.push(Candidate { index, origin: target.origin, a: along });
    }
    jsrt::sort::js_sort(&mut candidates, &mut |x: &Candidate, y: &Candidate| x.a - y.a);
    let name = def.get("LightningEffect");
    let bolt = match name {
        Js::Str(s) => world.has_lightning(s),
        _ => false,
    };
    let lightning_name = name.str().unwrap_or("").to_string();
    let mut damage = def.get("Damage").nn(0.0) + (def.get("MaxDamage").nn(0.0) - def.get("Damage").nn(0.0)) * charge;
    let mut from = p;
    let mut dir = Vector3::default();
    let mut prev = 0.0;
    let mut links = 0.0;
    let first_radius = def.get("FirstRadius").clone();
    for cand in &candidates {
        if links >= def.get("MaxTargets").nn(5.0) || damage < def.get("DamageThreshold").nn(0.0) {
            break;
        }
        let jump = if links == 0.0 { l } else { def.get("MaxJumpDistance").nn(10.0) };
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
        if links == 0.0 && first_radius.num() > 0.0 {
            toward.sub_vectors(&t, &p);
            let along = toward.dot(&d);
            toward.add_scaled_vector(&d, -along);
            if toward.length_sq() > v8math::pow(first_radius.num(), 2.0) {
                continue;
            }
        }
        prev = cand.a;
        if damage > 0.0 {
            world.hurt_arc_target(cand.index, damage, &from, u, c);
        }
        if bolt {
            world.presentation(&Presentation::Lightning { name: lightning_name.clone(), from, to: t, charge, channel: if links == 0.0 { Some(channel) } else { None } });
            if links == 0.0 || roll(c, world) > 0.3 {
                from = t;
            }
        }
        links += 1.0;
        damage -= def.get("DamageDropPerTarget").nn(0.0);
    }
    if links == 0.0 && bolt {
        let hit = world.cast(&p, &d, l, false);
        let mut to = p;
        to.add_scaled_vector(&d, hit.map(|h| h.distance).unwrap_or(l));
        world.presentation(&Presentation::Lightning { name: lightning_name, from: p, to, charge, channel: Some(channel) });
    }
}

/// explode(explosion, position, owner, scale) (combat.js:967): a blast's
/// damage and push on the soldier.
#[allow(clippy::too_many_arguments)]
pub fn explode(c: &mut Combat, u: &mut Unit, defs: &CombatDefs, explosion: &Js, position: &Vector3, owner: Attacker, scale: f64, world: &mut dyn CombatWorld) {
    let class = defs.class(u.class_def);
    let walkerdroid = class.get("base").is_str("walkerdroid");
    let was_dead = c.dead;
    // Nothing is left to throw of a droid that went up in pieces, and a
    // droideka's hull never takes the infantry launch impulse.
    if was_dead && (c.blow_apart || walkerdroid) {
        return;
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
    let mut push = explosion.get("Push").or(0.0) * falloff(distance, explosion.get("PushRadiusInner").or(0.0), explosion.get("PushRadiusOuter").or(0.0));
    let damage = scale * explosion.get("Damage").or(0.0) * falloff(distance, explosion.get("DamageRadiusInner").or(0.0), explosion.get("DamageRadiusOuter").or(0.0));
    let scale_name = world.text(c.scale);
    let typed = explosion.get(&scale_name).nn(1.0);
    let hurts = !was_dead && (damage > 0.0 || !(damage * typed <= 0.0));
    if !(push > 0.0) && !hurts {
        return;
    }
    // Terrain or a building between the blast and his sight point shields him.
    let sight_point = Vector3 { x: b.position.x, y: b.position.y + sight, z: b.position.z };
    if blast_blocked(world, position, &sight_point) {
        return;
    }
    if span > 1e-3 {
        away.divide_scalar(span);
    }
    // The push is rolled before the damage: a droid the blast would push
    // may be marked to blow apart at its death instead, unpushed.
    if push > 0.0 && truthy(c.chunks) && !was_dead && c.mounted != Some(true) && b.air != Some(Air::Thrown) && roll(c, world) <= c.chunks {
        c.blow_apart = true;
        push = 0.0;
    }
    if !was_dead {
        let generic = explosion.get("ShieldScale").nn(1.0);
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
}

/// falloff(distance, inner, outer) (ordnance.js:1023).
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
fn blast_blocked(world: &mut dyn CombatWorld, position: &Vector3, target: &Vector3) -> bool {
    let mut sight = Vector3::default();
    let span = sight.sub_vectors(target, position).length();
    if span < 1e-3 {
        return false;
    }
    sight.divide_scalar(span);
    match world.cast(position, &sight, span, true) {
        Some(hit) => hit.distance < span - 0.05,
        None => false,
    }
}

/// damage(amount, onShield, source, attacker, info) (combat.js:1031).
#[allow(clippy::too_many_arguments)]
pub fn damage_with(c: &mut Combat, u: &mut Unit, defs: &CombatDefs, amount: f64, on_shield: f64, source: Option<Vector3>, attacker: Attacker, info: &DamageInfo, world: &mut dyn CombatWorld) {
    let _ = defs;
    let mut amount = amount;
    if c.dead || amount <= 0.0 {
        return;
    }
    if let UnitBody::Droideka(d) = &mut u.soldier {
        let mut hits = Vec::new();
        amount = d.absorb(amount, on_shield, |_, taken| hits.push(taken));
        for taken in hits {
            world.shield_hit(taken, source, info.explosion);
        }
    }
    if amount <= 0.0 {
        return;
    }
    c.health -= amount;
    c.last_attacker = attacker;
    world.damaged(amount, attacker, info, source, u, c);
    if c.health <= 0.0 {
        let left = c.health;
        c.health = 0.0;
        c.dead = true;
        c.death_time = 0.0;
        c.zoom_step = 0.0;
        let air_thrown = matches!(&u.soldier, UnitBody::Soldier(s) if s.air == Some(Air::Thrown));
        let apart = truthy(c.chunks) && !air_thrown && (c.blow_apart || roll(c, world) <= c.chunks);
        c.blow_apart = apart;
        if apart {
            world.blown_apart(left, u, c);
        }
        world.died(u, c);
    }
}

/// reset() (combat.js:300): full health and a fresh loadout.
pub fn reset(c: &mut Combat, u: &Unit, defs: &CombatDefs) {
    c.health = c.max_health;
    c.dead = false;
    c.blow_apart = false;
    c.zoom_step = 0.0;
    c.kick = Kick { rate: 0.0, hold: 0.0, offset: 0.0 };
    c.weapons = arm(&defs.loadout(u.class_def), &defs.weapons, c.player);
    c.selected = [0.0, 0.0];
    c.seen = vec![Seen { known: false, reloading: false, throwing: false }; c.weapons.len()];
    c.pending_muzzles.clear();
}
