//! The registry (src/registry.js Units): every combatant in the battle in
//! the order it was added, the teams' relations, and the questions rounds,
//! capture and the AI ask of it: who is near a point (`query`) and what a
//! segment passes through (`raycast`).
//!
//! Its members are the units and the targets hulls, turrets and buildings
//! add (`Member`). The registry's own state is the store's (`gen::Units`);
//! the units are the store's units, by id.

use super::shapes::{segment_shape, segment_sphere, shape_normal, v3, Bounds, ShapePart};
use super::world::UnitsWorld;
use crate::gen::{Member, Unit, Units};
use three::Vector3;
use crate::store::{Slab, Strings};

/// The faction SetTeamName gives a name it doesn't know.
pub const LOCALS: f64 = 5.0;

/// The mission's table (src/ai/classic/spawning.js teamTable): each team's
/// view of another, its faction and whom it captures for, by team number.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TeamTable {
    pub relations: Vec<Vec<f64>>,
    pub factions: Vec<Option<f64>>,
    pub capture: Vec<Option<f64>>,
}

/// The index of an array a team number names as JavaScript indexes one
/// (`list[team]`, -0 as 0), or none for a number that is no index of it.
fn index(team: f64, len: usize) -> Option<usize> {
    if team >= 0.0 && team.fract() == 0.0 && team < len as f64 {
        Some(team as usize)
    } else {
        None
    }
}

/// What a raycast found (registry.js makeUnitHit): the unit, the distance,
/// the point, the shape's outward normal there and which part.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitHit {
    pub unit: u32,
    pub distance: f64,
    pub point: Vector3,
    pub normal: Vector3,
    pub part: ShapePart,
}

/// Whom a raycast leaves out (its `exclude`): nobody, a unit, or a list.
#[derive(Clone, Copy, Debug)]
pub enum Exclude<'a> {
    Nobody,
    One(Member),
    Many(&'a [Member]),
}

impl Exclude<'_> {
    fn has(&self, m: Member) -> bool {
        match self {
            Exclude::Nobody => false,
            Exclude::One(x) => *x == m,
            Exclude::Many(list) => list.contains(&m),
        }
    }
}

/// The key the registry files a pair of teams' relation under (`"a:b"`,
/// the lower first).
fn pair_key(a: f64, b: f64, strings: &mut Strings) -> crate::store::StrId {
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    strings.intern_str(&format!("{}:{}", number_text(lo), number_text(hi)))
}

/// A team number as JavaScript writes it in a template string.
fn number_text(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e21 {
        format!("{}", x as i64)
    } else {
        // Teams are small whole numbers; anything else is written as V8
        // writes it only where the two forms agree.
        format!("{x}")
    }
}

/// A target member's registry id.
pub fn target_id(m: Member) -> u32 {
    match m {
        Member::Unit(r) => r.slot,
        Member::VehicleTarget(r) => r.slot,
        Member::TurretTarget(r) => r.slot,
        Member::ObjectiveTarget(r) => r.slot,
    }
}

/// A reference held across a death (registry.js `ref`): the unit and the
/// life it was taken in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitRef {
    pub unit: u32,
    pub generation: f64,
}

impl Units {
    /// add(unit) (registry.js): the next id, the member at the list's end.
    /// Returns the id (a unit's slot).
    pub fn add(&mut self, kind: impl FnOnce(u32) -> Member) -> u32 {
        let id = self.next_id as u32;
        self.next_id += 1.0;
        self.list.push(kind(id));
        id
    }

    /// remove(unit) (registry.js): out of the list (the next keep their order).
    pub fn remove(&mut self, m: Member) {
        if let Some(i) = self.list.iter().position(|x| *x == m) {
            self.list.remove(i);
        }
    }

    /// byId(id): the member with that id, or none.
    pub fn by_id(&self, id: u32) -> Option<Member> {
        self.list.iter().copied().find(|m| target_id(*m) == id)
    }

    /// resolve(ref): the unit a reference names, while it is the same life
    /// of that unit (and alive).
    pub fn resolve(&self, units: &Slab<Unit>, r: Option<UnitRef>) -> Option<u32> {
        let r = r?;
        let u = units.get(crate::store::Ref::new(r.unit, 0))?;
        if u.generation == r.generation && u.alive {
            Some(r.unit)
        } else {
            None
        }
    }

    /// relation(a, b) (registry.js): team a's relation to team b, 1 friend,
    /// 0 neutral, -1 enemy.
    pub fn relation(&self, a: f64, b: f64, teams: Option<&TeamTable>, strings: &mut Strings) -> f64 {
        if a == b {
            return if a != 0.0 && !a.is_nan() { 1.0 } else { 0.0 };
        }
        let falsy = |x: f64| x == 0.0 || x.is_nan();
        if falsy(a) || falsy(b) {
            return 0.0;
        }
        let key = pair_key(a, b, strings);
        if let Some(set) = self.alliances.get(key) {
            return *set;
        }
        teams
            .and_then(|t| index(a, t.relations.len()).map(|i| &t.relations[i]))
            .and_then(|row| index(b, row.len()).map(|j| row[j]))
            .unwrap_or(-1.0)
    }

    /// setRelation(a, b, value).
    pub fn set_relation(&mut self, a: f64, b: f64, value: f64, strings: &mut Strings) {
        let key = pair_key(a, b, strings);
        self.alliances.set(key, value);
    }

    /// locals(team): a locals team (faction Locals); without the mission's
    /// table, any team above 2.
    pub fn locals(&self, team: f64, teams: Option<&TeamTable>) -> bool {
        match teams {
            Some(t) => index(team, t.factions.len()).and_then(|i| t.factions[i]) == Some(LOCALS),
            None => team > 2.0,
        }
    }

    /// captureTeam(team): whom a soldier of `team` counts for at a command
    /// post (0 nobody).
    pub fn capture_team(&self, team: f64, teams: Option<&TeamTable>) -> f64 {
        teams.and_then(|t| index(team, t.capture.len()).and_then(|i| t.capture[i])).unwrap_or(team)
    }

    /// query(center, radius, out, filter) (registry.js): the living members
    /// whose broadphase sphere meets the ball, in the registry's order.
    pub fn query(
        &self,
        units: &mut Slab<Unit>,
        center: &Vector3,
        radius: f64,
        out: &mut Vec<Member>,
        mut filter: Option<&mut dyn FnMut(Member) -> bool>,
        world: &mut dyn UnitsWorld,
    ) {
        out.clear();
        for &m in &self.list {
            let s = match m {
                Member::Unit(r) => {
                    let Some(u) = units.get_mut(r) else { continue };
                    if !u.alive {
                        continue;
                    }
                    u.sphere(r.slot, world)
                }
                Member::VehicleTarget(_) | Member::TurretTarget(_) | Member::ObjectiveTarget(_) => {
                    let id = target_id(m);
                    if !world.target_alive(id) {
                        continue;
                    }
                    world.target_sphere(id)
                }
            };
            let reach = radius + s.radius;
            if v3(s.center).distance_to_squared(center) > reach * reach {
                continue;
            }
            if let Some(f) = filter.as_deref_mut() {
                if !f(m) {
                    continue;
                }
            }
            out.push(m);
        }
    }

    /// raycast(origin, direction, far, exclude, out, filter, riders)
    /// (registry.js): the nearest soldier on foot a segment from `origin`
    /// along unit `direction` for `far` meets, by his hit shapes (a body
    /// still lying where it fell stops rounds too). `riders`: rounds also
    /// meet a soldier shown on an exposed seat.
    #[allow(clippy::too_many_arguments)]
    pub fn raycast(
        &self,
        units: &mut Slab<Unit>,
        origin: &Vector3,
        direction: &Vector3,
        far: f64,
        exclude: Exclude,
        mut filter: Option<&mut dyn FnMut(Member) -> bool>,
        riders: bool,
        world: &mut dyn UnitsWorld,
    ) -> Option<UnitHit> {
        let mut best = far;
        let mut found: Option<(u32, usize)> = None;
        for &m in &self.list {
            // Soldiers on foot, and for rounds the riders they can see; the
            // registry's other members have no hit shapes (they are struck
            // through their collision bodies).
            let r = match m {
                Member::Unit(r) => r,
                // (Its `alive` and `dead` are read before the test that
                // passes it over; reading them changes nothing.)
                Member::VehicleTarget(_) | Member::TurretTarget(_) | Member::ObjectiveTarget(_) => continue,
            };
            let Some(u) = units.get(r) else { continue };
            if !(u.alive || (u.dead && !u.removed)) {
                continue;
            }
            if u.seat.is_some() && !(riders && world.seat_exposed(r.slot, u)) {
                continue;
            }
            if exclude.has(m) {
                continue;
            }
            if let Some(f) = filter.as_deref_mut() {
                if !f(m) {
                    continue;
                }
            }
            // Broadphase first: the unit's whole sphere.
            let u = units.get_mut(r).unwrap();
            let s: Bounds = u.sphere(r.slot, world);
            if segment_sphere(origin, direction, best, &v3(s.center), s.radius).is_none() {
                continue;
            }
            let shapes = u.hit_shapes(r.slot, world);
            for (i, candidate) in shapes.iter().enumerate() {
                if let Some(t) = segment_shape(origin, direction, best, candidate) {
                    if t < best {
                        best = t;
                        found = Some((r.slot, i));
                    }
                }
            }
        }
        let (id, i) = found?;
        let u = units.get(crate::store::Ref::new(id, 0)).unwrap();
        let shape = u.hit_set()[i].clone();
        let mut point = Vector3::default();
        point.copy(origin).add_scaled_vector(direction, best);
        let normal = shape_normal(&point, &shape, direction);
        Some(UnitHit { unit: id, distance: best, point, normal, part: shape.part })
    }
}
