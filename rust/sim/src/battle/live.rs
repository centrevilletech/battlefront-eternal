//! LiveObjects' blast, repair and target selection (src/props.js).
//! A target's lifecycle belongs to Building, Trap or Vehicle; this layer
//! owns traversal, predicates, geometry and callback order.
use super::props::{Identity, State};
use crate::ordnance::defs::{ExplosionDef, Field};
use three::{Matrix4, Vector3};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health { Building, Vehicle, Rebuild, Build }
impl Health { fn index(self) -> usize { match self { Self::Building => 0, Self::Vehicle => 1, Self::Rebuild => 2, Self::Build => 3 } } }
#[derive(Clone, Debug)]
pub struct Target {
    pub alive: bool, pub state: Option<State>, pub health_type: Option<Health>, pub health: f64, pub max_health: f64, pub team: f64,
    pub body: Option<usize>, pub gunner: bool, pub position: Vector3, pub matrix: Matrix4,
}
#[derive(Clone, Debug)]
pub struct Definition {
    pub repair: [Field; 4],
    pub target_vehicle: Field, pub target_neutral: Field, pub target_enemy: Field, pub target_friendly: Field,
    pub range: Field, pub angle: Field, pub shield_scale: Field, pub explosion: ExplosionDef,
}
#[derive(Clone, Copy, Debug)]
pub struct Hit { pub distance: f64, pub owner: Option<usize> }
#[derive(Clone, Debug)]
pub struct Damage { pub direction: Option<Vector3>, pub amount: f64, pub typed: f64, pub generic: Field }
#[derive(Clone, Copy, Debug)]
pub enum Out { Sight, Lock }
pub trait World {
    fn target(&mut self, index: usize) -> Target;
    fn sphere(&mut self, index: usize, blast: bool, point: Vector3) -> (Vector3, f64);
    fn cast(&mut self, origin: Vector3, direction: Vector3, far: f64, ignore: Option<usize>, static_only: bool, out: Out) -> Option<Hit>;
    fn team_scale(&mut self, queries: &mut Queries, index: usize) -> f64;
    fn damage(&mut self, queries: &mut Queries, index: usize, amount: f64, owner: Identity, info: Option<Damage>);
    fn set_health(&mut self, queries: &mut Queries, index: usize, value: Field);
    fn may_enter(&mut self, queries: &mut Queries, index: usize) -> bool;
}
fn zero(field: &Field) -> bool { matches!(field, Field::Num(n) if *n == 0.0) }
fn add_number(left: f64, right: &Field) -> Field {
    if let Field::Str(s) = right { Field::Str(jsrt::numfmt::number_to_string(left) + s) } else { Field::Num(left + right.num()) }
}
pub fn repairs(def: &Definition, target: &Target, team: f64) -> bool {
    let Some(kind) = target.health_type else { return false };
    let amount = &def.repair[kind.index()];
    if !amount.truthy() { return false; }
    if kind == Health::Vehicle && (zero(&def.target_vehicle) || !target.alive || target.team != team && zero(&def.target_enemy)) { return false; }
    if amount.num() > 0.0 && target.health >= target.max_health { return false; }
    if target.team == 0.0 { !zero(&def.target_neutral) } else if target.team != team { !zero(&def.target_enemy) } else { !zero(&def.target_friendly) }
}

#[derive(Clone, Debug, Default)]
pub struct Queries { pub center: Vector3, pub toward: Vector3 }
impl Queries {
    pub fn blast(&mut self, candidates: &[usize], def: &ExplosionDef, position: Vector3, owner: Identity, team_scale: bool, collision: bool, world: &mut impl World) {
        for &index in candidates {
            let target = world.target(index); if !target.alive { continue; }
            let (center, radius) = world.sphere(index, true, self.center); self.center = center;
            let distance = crate::ordnance::blast_distance([position.x, position.y, position.z], [center.x, center.y, center.z], radius);
            let damage = def.damage_at(distance, &Field::Str("building".into()));
            if !(damage > 0.0) { continue; }
            let mut sight = center; sight.sub(&position); let span = sight.length();
            if collision && !(span < 1e-3) {
                sight.divide_scalar(span);
                if world.cast(position, sight, span, target.body, true, Out::Sight).is_some_and(|h| h.distance < span - 0.05) { continue; }
            }
            let scale = if team_scale { world.team_scale(self, index) } else { 1.0 };
            world.damage(self, index, damage * scale, owner, None);
        }
    }
    pub fn struck(&mut self, building: Option<usize>, def: &Definition, owner: Identity, team_scale: bool, direction: Option<Vector3>, world: &mut impl World) {
        let Some(index) = building else { return };
        let typed = def.explosion.health_scale(&Field::Str("building".into()));
        let team = if team_scale { world.team_scale(self, index) } else { 1.0 };
        let amount = def.explosion.damage.or(0.0);
        world.damage(self, index, amount * typed * team, owner, Some(Damage { direction, amount: amount * team, typed,
            generic: if def.shield_scale.is_nullish() { Field::Num(1.0) } else { def.shield_scale.clone() } }));
    }
    pub fn repair(&mut self, building: Option<usize>, def: &Definition, team: f64, world: &mut impl World) -> bool {
        let Some(index) = building else { return false };
        let target = world.target(index); if !repairs(def, &target, team) { return false; }
        let amount = &def.repair[target.health_type.unwrap().index()];
        let value = add_number(target.health, amount);
        world.set_health(self, index, value); true
    }
    fn score(&mut self, index: usize, widen: f64, eye: Vector3, direction: Vector3, def: &Definition, team: f64, collision: bool, world: &mut impl World) -> Option<f64> {
        let target = world.target(index);
        if !(target.alive || matches!(target.state, Some(State::Destroyed | State::Unbuilt))) || !repairs(def, &target, team) { return None; }
        let (center, radius) = world.sphere(index, false, self.center); self.center = center;
        self.toward.sub_vectors(&center, &eye); let t = self.toward.dot(&direction);
        let range = if def.range.truthy() { add_number(radius, &def.range).num() } else { radius + 0.0 }; let slope = (def.angle.or(0.0) * std::f64::consts::PI) / 180.0;
        if t < 0.0 || t > range { return None; }
        if self.toward.length() > range { return None; }
        self.toward.add_scaled_vector(&direction, -t); let miss2 = self.toward.length_sq();
        let cone = radius + t * (slope + widen); if miss2 > cone * cone { return None; }
        self.toward.sub_vectors(&center, &eye); let span = self.toward.length();
        if span > 1e-3 && collision {
            self.toward.divide_scalar(span);
            if world.cast(eye, self.toward, span, None, false, Out::Lock).is_some_and(|h| h.owner != Some(index) && h.distance < span - radius) { return None; }
        }
        Some(16.0 * miss2 + t)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn lock_on(&mut self, candidates: &[usize], eye: Vector3, direction: Vector3, def: &Definition, current: Option<usize>, held: bool, team: f64, collision: bool, world: &mut impl World) -> Option<usize> {
        let mut best = None; let mut score = f64::INFINITY;
        if let Some(index) = current {
            if let Some(s) = self.score(index, if held { 0.7854 } else { 0.0 }, eye, direction, def, team, collision, world) { best = Some(index); score = s; }
        }
        for &index in candidates {
            if Some(index) == current { continue; }
            if let Some(s) = self.score(index, 0.0, eye, direction, def, team, collision, world) { if s < score { best = Some(index); score = s; } }
        }
        best
    }
    pub fn enterable(&mut self, candidates: &[usize], eye: Vector3, direction: Vector3, may: bool, world: &mut impl World) -> Option<usize> {
        let mut best = None; let mut score = f64::INFINITY;
        for &index in candidates {
            let target = world.target(index);
            if !target.alive || target.gunner && (!may || !world.may_enter(self, index)) { continue; }
            let mut center = Vector3::new(0.0, 1.65, 0.32); center.apply_matrix4(&target.matrix);
            let reach = 2.25; let mut delta = center; delta.sub(&eye); let t = delta.dot(&direction);
            if eye.distance_to(&target.position) > reach + 3.5 || t < 0.0 || t > reach + 5.0 { continue; }
            delta.add_scaled_vector(&direction, -t); let miss = delta.length(); if miss > reach + t { continue; }
            let candidate = 16.0 * miss * miss + t; if candidate < score { score = candidate; best = Some(index); }
        }
        best
    }
}
