//! weapon-targeting.js lockPick's view search. Hulls and stationary turrets
//! differ in their foreign query and sight contracts, but retain/acquire order,
//! volume scoring and first-equal selection are shared.
use super::*;
use crate::abi::sim;
use crate::combat::weapons::LockTarget;
use crate::gen::Vehicle;
use crate::store::Ref;
use crate::vehicles::module::{vec, MEMBERS};
use std::ptr::addr_of_mut;

pub struct LockFacts {
    pub id: f64, pub generation: f64, pub alive: bool, pub eligible: bool,
    pub relation: f64, pub category: u32, pub hull: Option<Ref<Vehicle>>,
    pub body: Option<u32>, pub center: Vector3, pub radius: f64, pub aim: Vector3,
}
pub trait LockQuery {
    fn facts(&mut self, index: usize, shapes: bool, aim: bool) -> LockFacts;
    fn clear(&mut self, origin: &Vector3, sight: &mut Vector3, length: f64, body: Option<u32>) -> bool;
}
pub struct LockView {
    pub range: f64, pub on: f64, pub off: f64, pub relations: u32, pub categories: u32,
    pub complete: bool, pub id: f64, pub generation: f64,
}
pub fn allowed(relations: u32, categories: u32, relation: f64, category: u32) -> bool {
    relations & (1 << (relation as i32 + 1)) != 0 && categories & (1 << category) != 0
}
/// Human lock selection only; bot hints bypass the cone in each caller, as
/// seatLockTarget does. Direction has been normalized at the caller's slot.
pub fn pick_view(query: &mut impl LockQuery, view: LockView, origin: &Vector3, direction: &Vector3) -> Option<LockTarget> {
    if view.range <= 0.0 || direction.length_sq() == 0.0 { return None; }
    let owned = crate::units::f3::active();
    let n = if owned { crate::units::f3_fleet::count() } else { unsafe { (&*addr_of_mut!(MEMBERS)).len() } };
    for pass in 0..2 {
        if pass == 0 && !view.complete { continue; }
        let cone = v8math::max(0.0, if pass == 0 { view.off } else { view.on }) * std::f64::consts::PI / 180.0;
        let mut best = None; let mut best_angle = f64::INFINITY; let mut best_distance = f64::INFINITY;
        for i in 0..n {
            if pass == 0 {
                let (id, generation) = if owned {
                    let key = crate::units::f3_fleet::key(i);
                    (crate::units::registry::target_id(key) as f64, crate::units::f3_fleet::life(key))
                } else {
                    let m = unsafe { (&*addr_of_mut!(MEMBERS))[i] };
                    (m.id, m.generation.unwrap_or(0.0))
                };
                if id != view.id || generation != view.generation { continue; }
            }
            let mut f = query.facts(i,false,false);
            if !f.eligible || !allowed(view.relations,view.categories,f.relation,f.category) { continue; }
            let hull = f.hull.and_then(|r| sim().store.vehicle.get(r));
            let (angle,distance) = if let Some(v) = hull {
                box_span(origin,direction,&Matrix4 { elements: v.matrix },&vec(&v.bounds_min),&vec(&v.bounds_max))
            } else {
                f = query.facts(i,true,false); sphere_span(origin,direction,&f.center,f.radius)
            };
            if angle > cone || distance > view.range { continue; }
            if best.is_some() && (best_angle < angle || best_angle == angle && best_distance <= distance) { continue; }
            f = query.facts(i,false,true);
            let mut sight = f.aim; sight.sub(origin); let length = sight.length();
            if !query.clear(origin,&mut sight,length,f.body) { continue; }
            best = Some(LockTarget { id: f.id, generation: f.generation, alive: f.alive });
            best_angle = angle; best_distance = distance;
        }
        if best.is_some() { return best; }
    }
    None
}
