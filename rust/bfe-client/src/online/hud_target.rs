//! What the reticle is on. updateTargetHud (src/game.js) runs
//! findReticleTarget (src/hud-units.js) for each channel's weapon whenever a
//! correction has been applied: the target's relation colours the reticle,
//! and an enemy under channel 0 shows his health under the player's own.
//! The search itself is bfe_sim::combat::page_targeting::Query; this module
//! gives it each unit as the browser's registry would (a soldier's hit-shape
//! bounds, a hull's box, a turret's sphere) and answers its sight lines.
use super::{collision::Bodies, radar::Teams, receiver::Receiver};
use bfe_content::Value;
use bfe_presentation::network_layout::{net_kind as k, net_record as r};
use bfe_sim::collision::{CollisionOwner, Ray};
use bfe_sim::combat::page_targeting::{Query, TARGETS};
use bfe_sim::gen::{PageTargetFrame as F, PageTargetRow as R};
use bfe_sim::units::shapes::{
    bound_shapes, soldier_shapes, Bounds, Half, HitPosture, HitShape, ShapePart, ShapeType, DROIDEKA_BALL, DROIDEKA_ORD_AT,
    DROIDEKA_ORD_HALF, DROIDEKA_ORD_RADIUS,
};
use three::{Matrix4, Quaternion, Vector3};

// ENTITY_SOLDIER_STATES (src/runtime/network-entity-codec.js): jump, fall,
// thrown and jet are in the air, the two dives follow, then the two deaths.
const AIR: std::ops::RangeInclusive<u32> = 1..=4;
const DIVE: std::ops::RangeInclusive<u32> = 5..=6;
const DEAD: std::ops::RangeInclusive<u32> = 7..=8;
// A hull's record while it burns out (ENTITY_VEHICLE_STATES' "dying").
const DYING: f64 = 6.;
// categoryOf (src/ai/classic/weapons-ai.js): a class's HealthType.
const CATEGORIES: [&str; 5] = ["person", "animal", "droid", "vehicle", "building"];
// The relation code of a reticle on nothing (relationCode(null)).
const NOTHING: f64 = 4.;

/// findReticleTarget's reads of a weapon definition: how far its reticle
/// reaches, its cone, and what it may lock.
#[derive(Clone, Copy, Default)]
pub struct Reticle {
    far: f64,
    angle: f64,
    lock_range: f64,
    relation_mask: u32,
    category_mask: u32,
}

impl Reticle {
    pub fn new(def: &Value) -> Self {
        let number = bfe_content::js::to_number;
        // `a || b` over Numbers: zero and NaN give way.
        let or = |a: f64, b: f64| if a == 0. || a.is_nan() { b } else { a };
        // reticleRange: the extracted ReticleRange, else the round's flight
        // (speed times life), else the lock's range.
        let authored = number(def.get("ReticleRange"));
        let far = if def.get("ReticleRange").is_some() && authored.is_finite() {
            authored.max(0.)
        } else {
            let ordnance = def.get("ordnance");
            let flight = number(ordnance.and_then(|o| o.get("Velocity"))) * number(ordnance.and_then(|o| o.get("LifeSpan")));
            or(or(flight, number(def.get("LockOnRange"))), 0.)
        };
        // weaponAI's masks as admission reads them: the definition's own ai
        // record, else its Target flags (the AI's registered record for the
        // weapon's class is not consulted here).
        let lock = bfe_sim::world::admit::rows::units::lock_rule(def);
        Self { far, angle: lock.on_angle, lock_range: lock.range, relation_mask: lock.relation_mask, category_mask: lock.category_mask }
    }
}

/// A class as a target: its soldiers' CollisionRootScale and what its
/// HealthType makes it to a weapon's masks.
#[derive(Clone, Copy)]
pub struct Class {
    pub scale: f64,
    pub category: u32,
}

impl Class {
    /// A soldier class: `def.CollisionRootScale ?? 1`, and its HealthType
    /// (a soldier without one is a person).
    pub fn soldier(def: &Value) -> Self {
        let scale = def.get("CollisionRootScale").and_then(Value::as_f64).unwrap_or(1.);
        let kind = def.get("HealthType").and_then(Value::as_str).unwrap_or("").to_ascii_lowercase();
        Self { scale, category: CATEGORIES.iter().position(|name| *name == kind).unwrap_or(0) as u32 }
    }
    /// VehicleTarget.category: a creature is an animal, any other hull a
    /// vehicle.
    pub fn vehicle(settings: &Value) -> Self {
        let animal = settings.get("HealthType").and_then(Value::as_str).is_some_and(|kind| kind.eq_ignore_ascii_case("animal"));
        Self { scale: 1., category: if animal { 1 } else { 3 } }
    }
}

/// Turret.sphere for a built turret (src/props.js): the middle of its
/// model's box where it stands, and half the box's diagonal.
pub fn turret_sphere(model_box: Option<&Value>, matrix: &[f64; 16]) -> ([f64; 3], f64) {
    let corner = |at: usize| -> Option<[f64; 3]> {
        let c = model_box?.at(at)?;
        Some([c.at(0)?.as_f64()?, c.at(1)?.as_f64()?, c.at(2)?.as_f64()?])
    };
    let (mut center, radius) = match (corner(0), corner(1)) {
        (Some(lo), Some(hi)) => (
            Vector3::new((lo[0] + hi[0]) / 2., (lo[1] + hi[1]) / 2., (lo[2] + hi[2]) / 2.),
            ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt() / 2.,
        ),
        _ => (Vector3::new(0., 1., 0.), 1.),
    };
    let mut m = Matrix4::default();
    m.elements.copy_from_slice(matrix);
    center.apply_matrix4(&m);
    ([center.x, center.y, center.z], radius)
}

/// A weapon's reticle and its kept lock, for one channel.
#[derive(Clone, Copy)]
pub struct Aimed {
    pub reticle: Reticle,
    pub lock: (f64, f64),
}

/// What the search found: each channel's relation code for the reticle (1
/// friendly, 2 enemy, 0 neutral, 4 nothing), and channel 0's enemy as his
/// record's kind, class or building, and health.
pub struct Found {
    pub relations: [f64; 2],
    pub enemy: Option<(u32, usize, f64)>,
}

// How a candidate answers a sight line: the hull body a ray may end on,
// and how near its centre a ray into a placed building may end.
#[derive(Clone, Copy, Default)]
struct Sight {
    body: i32,
    reach: f64,
    kind: u32,
    source: usize,
    health: f64,
    alive: bool,
    relation: f64,
    key: (u32, u32),
}

pub struct Targets {
    query: Box<Query>,
    classes: Vec<Class>,
    turrets: Vec<(usize, [f64; 3], f64)>,
    sights: Vec<Sight>,
    shapes: [HitShape; 2],
}

impl Targets {
    pub fn new(classes: Vec<Class>, turrets: Vec<(usize, [f64; 3], f64)>) -> Self {
        Self { query: Box::new(Query::new()), classes, turrets, sights: Vec::with_capacity(TARGETS), shapes: Default::default() }
    }

    /// updateTargetHud for a living player. `ray`: where the search starts
    /// and which way it looks; `viewer`: the unit he controls (his own, or
    /// the vehicle or turret he sits in), which is never its own target;
    /// `weapons`: channel 0's weapon and, in a vehicle seat, channel 1's.
    #[allow(clippy::too_many_arguments)]
    pub fn find(
        &mut self,
        receiver: &Receiver,
        teams: &Teams,
        bodies: &Bodies,
        collision: &mut CollisionOwner,
        world: u32,
        ray: ([f64; 3], [f64; 3]),
        viewer: (u32, u32),
        weapons: [Option<Aimed>; 2],
    ) -> Found {
        self.rows(receiver, teams, bodies, viewer);
        let mut found = Found { relations: [NOTHING; 2], enemy: None };
        for (channel, aimed) in weapons.iter().enumerate() {
            let Some(aimed) = aimed else { continue };
            // lockedUnit: a kept lock is the target while that life lives.
            let kept = self.sights.iter().position(|sight| {
                sight.key.0 as f64 == aimed.lock.0 && sight.key.1 as f64 == aimed.lock.1 && sight.alive
            });
            let Some(target) = kept.or_else(|| self.search(collision, world, ray, &aimed.reticle)) else { continue };
            let sight = self.sights[target];
            found.relations[channel] = super::hud_feed::relation_code(sight.relation);
            if channel == 0 && sight.relation < 0. {
                found.enemy = Some((sight.kind, sight.source, sight.health));
            }
        }
        found
    }

    // The registry as findReticleTarget walks it: one row a unit, with the
    // facts its eligibility, cone test and ranking read.
    fn rows(&mut self, receiver: &Receiver, teams: &Teams, bodies: &Bodies, viewer: (u32, u32)) {
        self.sights.clear();
        for fact in receiver.active() {
            if self.sights.len() == TARGETS {
                break;
            }
            let kind = fact[r::KIND] as u32;
            if !matches!(kind, k::SOLDIER | k::DROIDEKA | k::VEHICLE | k::TURRET) {
                continue;
            }
            let key = (fact[r::ID] as u32, fact[r::GENERATION] as u32);
            let state = fact[r::STATE] as u32;
            let position = [fact[r::POSITION], fact[r::POSITION + 1], fact[r::POSITION + 2]];
            let mut row = R {
                viewer: key == viewer,
                alive: fact[r::DEAD] == 0. && !(kind == k::SOLDIER && DEAD.contains(&state)) && !(kind == k::VEHICLE && fact[r::STATE] == DYING),
                seated: fact[r::SEAT_PRESENT] != 0.,
                health: fact[r::HEALTH],
                // KIND (src/seats.js): no record here is an item (10), the
                // one kind the search refuses by its number.
                kind: 0.,
                relation: teams.relation(receiver.own.team, fact[r::TEAM] as usize),
                ..Default::default()
            };
            let mut sight = Sight {
                body: -1,
                kind,
                source: fact[r::CLASS_ID] as usize,
                health: fact[r::HEALTH],
                alive: row.alive,
                relation: row.relation,
                key,
                ..Default::default()
            };
            let class = self.classes.get(sight.source).copied().unwrap_or(Class { scale: 1., category: 0 });
            row.category = class.category;
            let mut bounds = Bounds::default();
            match kind {
                k::SOLDIER => {
                    // Unit.hitPosture and updateSpheres (src/units.js): the
                    // posture's two shapes about his feet, turned with him.
                    let posture = if DEAD.contains(&state) {
                        HitPosture::Prone
                    } else if DIVE.contains(&state) {
                        HitPosture::Crouch
                    } else if AIR.contains(&state) {
                        HitPosture::Stand
                    } else {
                        [HitPosture::Stand, HitPosture::Crouch, HitPosture::Prone][(fact[r::POSTURE] as usize).min(2)]
                    };
                    let (sin, cos) = fact[r::YAW].sin_cos();
                    for (shape, part) in self.shapes.iter_mut().zip(soldier_shapes(posture, class.scale)) {
                        let [x, y, z] = part.at;
                        shape.center = [position[0] + cos * x + sin * z, position[1] + y, position[2] - sin * x + cos * z];
                        shape.r#type = part.kind;
                        shape.part = part.part;
                        shape.radius = part.radius;
                        shape.half_size = part.half_box;
                        shape.half = if part.kind == ShapeType::Box { Half::Vec3(part.half_box) } else { Half::F64(part.half) };
                        shape.yaw = fact[r::YAW];
                    }
                    bound_shapes(&self.shapes, 2, &mut bounds);
                }
                k::DROIDEKA => {
                    // Deployed: the ball and the upright cylinder ahead of
                    // its root. A rolled droideka is its ball alone in the
                    // browser; the record here does not say which it is.
                    let (sin, cos) = fact[r::YAW].sin_cos();
                    let [ball, ord] = &mut self.shapes;
                    ball.r#type = ShapeType::Sphere;
                    ball.part = ShapePart::Body;
                    ball.center = [position[0], position[1] + DROIDEKA_BALL, position[2]];
                    ball.radius = DROIDEKA_BALL;
                    ball.half = Half::F64(0.);
                    let [_, y, z] = DROIDEKA_ORD_AT;
                    ord.r#type = ShapeType::Cylinder;
                    ord.part = ShapePart::Body;
                    ord.center = [position[0] + sin * z, position[1] + y, position[2] + cos * z];
                    ord.radius = DROIDEKA_ORD_RADIUS;
                    ord.half = Half::F64(DROIDEKA_ORD_HALF);
                    bound_shapes(&self.shapes, 2, &mut bounds);
                    row.category = 2;
                }
                k::VEHICLE => {
                    // VehicleTarget: the hull's collision box where it is,
                    // aimed at by its middle. A hull this page has no body
                    // for is no target.
                    let Some((body, b)) = bodies.hull(key) else { continue };
                    let q = r::QUATERNION;
                    let mut matrix = Matrix4::default();
                    matrix.compose(
                        &Vector3::new(position[0], position[1], position[2]),
                        &Quaternion::new(fact[q], fact[q + 1], fact[q + 2], fact[q + 3]),
                        &Vector3::new(1., 1., 1.),
                    );
                    let mut center = Vector3::new((b[0] + b[3]) / 2., (b[1] + b[4]) / 2., (b[2] + b[5]) / 2.);
                    center.apply_matrix4(&matrix);
                    bounds.center = [center.x, center.y, center.z];
                    bounds.radius = Vector3::new(b[3] - b[0], b[4] - b[1], b[5] - b[2]).length() * 0.5;
                    row.hull = true;
                    row.min = [b[0], b[1], b[2]];
                    row.max = [b[3], b[4], b[5]];
                    row.matrix = matrix.elements;
                    sight.body = body as i32;
                }
                _ => {
                    // TurretTarget: a building (category 4), by its sphere.
                    sight.source = fact[r::BUILDING] as usize;
                    let Some(&(_, center, radius)) = self.turrets.iter().find(|turret| turret.0 == sight.source) else { continue };
                    bounds = Bounds { center, radius };
                    row.category = 4;
                    sight.reach = radius;
                }
            }
            row.point = bounds.center;
            row.center = bounds.center;
            row.radius = bounds.radius;
            row.box_radius = bounds.radius;
            let at = self.sights.len() * R::WORDS;
            row.put(&mut self.query.rows[at..at + R::WORDS], &mut Vec::new());
            self.sights.push(sight);
        }
    }

    // findReticleTarget for one weapon: the query proposes each candidate
    // that would beat the best so far, and reticleLos answers whether the
    // way to it is clear.
    fn search(&mut self, collision: &mut CollisionOwner, world: u32, ray: ([f64; 3], [f64; 3]), reticle: &Reticle) -> Option<usize> {
        F {
            origin: ray.0,
            direction: ray.1,
            far: reticle.far,
            // The query reads its cone, in degrees, from this field.
            yaw: reticle.angle,
            lock_range: reticle.lock_range,
            relation_mask: reticle.relation_mask,
            category_mask: reticle.category_mask,
            count: self.sights.len() as u32,
            ..Default::default()
        }
        .put(&mut self.query.frame, &mut Vec::new());
        self.query.begin();
        let mut visible = false;
        loop {
            let candidate = self.query.resume(visible);
            if candidate < 0 {
                break;
            }
            let point = F::take(&self.query.frame, &[]).point;
            visible = clear(collision, world, ray.0, point, &self.sights[candidate as usize]);
        }
        usize::try_from(F::take(&self.query.frame, &[]).best).ok()
    }
}

// reticleLos (src/game.js): nothing between the eye and the point, or the
// first thing met is the target itself. A hull is known by its body. A
// turret's building is part of the placed world here, so a ray that ends
// within the turret's own sphere counts as ending on it.
fn clear(collision: &mut CollisionOwner, world: u32, from: [f64; 3], to: [f64; 3], sight: &Sight) -> bool {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if length < 1e-6 {
        return true;
    }
    let ray = Ray { ox: from[0], oy: from[1], oz: from[2], dx: d[0] / length, dy: d[1] / length, dz: d[2] / length };
    match collision.ray_hit(world, ray, length, -1) {
        None => true,
        Some((body, hit)) => body == sight.body || (sight.reach > 0. && hit.distance >= length - sight.reach),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reticle_reads_its_range_cone_and_lock_from_the_definition() {
        let def = |text: &str| bfe_content::json::parse(text).unwrap();
        // The round's flight when no range was extracted; a lock's range
        // when the round has none; the extracted range first, never below 0.
        let rifle = Reticle::new(&def(r#"{"ordnance":{"Velocity":200,"LifeSpan":1.5},"TargetEnemy":1,"TargetFriendly":1}"#));
        assert_eq!((rifle.far, rifle.angle, rifle.lock_range), (300., 0., 0.));
        assert_eq!(rifle.relation_mask, 1 | 4);
        let launcher = Reticle::new(&def(r#"{"LockOnRange":150,"LockOnAngle":6,"TargetVehicle":1,"TargetPerson":0}"#));
        assert_eq!((launcher.far, launcher.angle, launcher.lock_range), (150., 6., 150.));
        assert_eq!(launcher.category_mask & 8, 8);
        assert_eq!(launcher.category_mask & 1, 0);
        assert_eq!(Reticle::new(&def(r#"{"ReticleRange":-5,"LockOnRange":150}"#)).far, 0.);
        assert_eq!(Reticle::new(&def("{}")).far, 0.);
    }

    #[test]
    fn classes_and_turrets_are_targets_as_the_browser_makes_them() {
        let def = |text: &str| bfe_content::json::parse(text).unwrap();
        let droid = Class::soldier(&def(r#"{"HealthType":"Droid","CollisionRootScale":1.25}"#));
        assert_eq!((droid.scale, droid.category), (1.25, 2));
        assert_eq!(Class::soldier(&def("{}")).category, 0);
        assert_eq!(Class::vehicle(&def(r#"{"HealthType":"animal"}"#)).category, 1);
        assert_eq!(Class::vehicle(&def("{}")).category, 3);
        // A box from (0,0,0) to (2,4,4), moved 10 along x.
        let mut matrix = [0.; 16];
        for i in [0, 5, 10, 15] {
            matrix[i] = 1.;
        }
        matrix[12] = 10.;
        let (center, radius) = turret_sphere(Some(&def("[[0,0,0],[2,4,4]]")), &matrix);
        assert_eq!((center, radius), ([11., 2., 2.], 3.));
        assert_eq!(turret_sphere(None, &matrix), ([10., 1., 0.], 1.));
    }
}
