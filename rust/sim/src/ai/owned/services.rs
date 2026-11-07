//! createServiceCandidates (src/ai/services.js). Repair scans include wrecked
//! buildings that the living registry omits. Candidate order is body registry,
//! fleet, then authored buildings; lazy spheres are read only in the filter.
use super::{
    entities::{Entities, Entity},
    scratch::{List, Pool},
};
use crate::{
    ai::{control::world::UKey, nav::math::Vec3},
    gen::{Member, RepairTarget, Store},
};

pub trait Geometry {
    fn position(&mut self, unit: UKey) -> Vec3;
    fn radius(&mut self, unit: UKey) -> Option<f64>;
}
pub struct Candidates {
    buildings: Vec<(RepairTarget, UKey)>,
    rows: Pool<UKey>,
}
impl Candidates {
    pub fn new(capacity: usize, buildings: usize, scopes: usize) -> Self {
        Self {
            buildings: Vec::with_capacity(buildings),
            rows: Pool::new(scopes, capacity),
        }
    }
    pub fn building(&mut self, target: RepairTarget, wrapper: UKey, command_post: bool) {
        // The source deliberately excludes destructible command posts from
        // bot repair jobs, while human tools can still repair them.
        if !command_post {
            self.buildings.push((target, wrapper));
        }
    }
    /// Finish the snapshot before any sphere query can reenter the battle.
    /// No mutable world reference is retained in the returned result scope.
    pub fn snapshot(&self, store: &Store, entities: &Entities) -> List<UKey> {
        let mut out = self.rows.take();
        for &member in &store.units.list {
            let Member::Unit(reference) = member else {
                continue;
            };
            let Some(unit) = store.unit.get(reference) else {
                continue;
            };
            if unit.alive && unit.seat.is_none() {
                out.push_reserved(
                    entities
                        .find(Entity::Unit(reference))
                        .expect("prepared service body"),
                );
            }
        }
        for &hull in &store.vehicles.vehicles {
            // Source fleet entries without aiTarget do not join the scan.
            if let Some(entry) = entities.find(Entity::HullEntry(hull)) {
                out.push_reserved(entry);
            }
        }
        for &(_, wrapper) in &self.buildings {
            if entities.get(wrapper).is_some() {
                out.push_reserved(wrapper);
            }
        }
        out
    }
    pub fn filter(
        mut candidates: List<UKey>,
        center: Vec3,
        radius: f64,
        geometry: &mut impl Geometry,
    ) -> List<UKey> {
        candidates.retain(|&unit| {
            let point = geometry.position(unit);
            let x = point.x - center.x;
            let y = point.y - center.y;
            let z = point.z - center.z;
            let distance = x * x + y * y + z * z;
            // Sphere access follows the position/distance expression. NaN
            // and missing radii both take JavaScript's `radius || 0` path.
            let r = geometry
                .radius(unit)
                .filter(|r| !r.is_nan() && *r != 0.)
                .unwrap_or(0.);
            let span = radius + r;
            distance <= span * span
        });
        candidates
    }
    pub fn matches(&self, wrapper: UKey, current: RepairTarget) -> bool {
        self.buildings
            .iter()
            .any(|&(target, key)| key == wrapper && target == current)
    }
}
