//! Snapshot hull colliders lend the prepared map query world their current
//! transforms. They have no vehicle solver or authoritative state: replay
//! and cameras read the same immutable class geometry and presence flags.
use super::{index::Index, receiver::Receiver};
use bfe_presentation::network_layout::{net_kind as k, net_record as r};
use bfe_sim::collision::CollisionOwner;
use bfe_sim::vehicles::native::Collider;
use three::{Matrix4, Quaternion, Vector3};
struct Body {
    class: usize,
    kernel: u32,
    body: u32,
    bounds: [f64; 6],
    identity: Option<(u32, u32)>,
    active: bool,
}
pub struct Bodies {
    rows: Vec<Body>,
    bound: Index,
    pub flags: Vec<f64>,
    pub refused: u64,
}
impl Bodies {
    pub fn new(collision: &mut CollisionOwner, world: u32, classes: &[(usize, u32, [f64; 6])], first: u32) -> Self {
        let mut rows = Vec::new();
        for (class, tree, bounds) in classes {
            for _ in 0..(64 / classes.len().max(1)).max(1) {
                let kernel = collision.body_new(*tree, 1) as u32;
                let body = first + rows.len() as u32;
                collision.state.worlds[world as usize].order.push(kernel);
                collision.state.worlds[world as usize].version += 1;
                collision.enable_body(kernel, false);
                rows.push(Body { class: *class, kernel, body, bounds: *bounds, identity: None, active: false });
            }
        }
        let flags = vec![0.; collision.state.worlds[world as usize].order.len()];
        Self { rows, bound: Index::new(128), flags, refused: 0 }
    }
    pub fn colliders(&self) -> impl Iterator<Item = (u32, Collider)> + '_ {
        self.rows.iter().map(|row| (row.body, Collider { kernel: row.kernel, local_bounds: row.bounds, walls: vec![], springs: vec![] }))
    }
    pub fn frame(&mut self, receiver: &Receiver, collision: &mut CollisionOwner, world: u32) {
        for row in &mut self.rows {
            row.active = false;
        }
        for fact in receiver.active().filter(|r| r[r::KIND] == k::VEHICLE as f64 && r[r::STATE] != 4.) {
            let key = (fact[r::ID] as u32, fact[r::GENERATION] as u32);
            if let Some(&slot) = self.bound.get(&key) {
                if self.rows[slot].class == fact[r::CLASS_ID] as usize {
                    self.rows[slot].active = true;
                }
            }
        }
        for (index, row) in self.rows.iter_mut().enumerate() {
            if !row.active {
                if let Some(key) = row.identity.take() {
                    if self.bound.get(&key) == Some(&index) {
                        self.bound.remove(&key);
                    }
                }
                collision.enable_body(row.kernel, false);
            }
        }
        for fact in receiver.active().filter(|r| r[r::KIND] == k::VEHICLE as f64 && r[r::STATE] != 4.) {
            let key = (fact[r::ID] as u32, fact[r::GENERATION] as u32);
            let slot = if let Some(&slot) = self.bound.get(&key) {
                slot
            } else {
                let Some(slot) = self.rows.iter().position(|row| row.identity.is_none() && row.class == fact[r::CLASS_ID] as usize) else {
                    self.refused += 1;
                    continue;
                };
                self.bound.insert(key, slot);
                self.rows[slot].identity = Some(key);
                slot
            };
            let row = &mut self.rows[slot];
            row.active = true;
            // Every occupied vehicle seat excludes its hull, including passengers
            // whose body checkpoint remains an infantry body.
            let own = receiver.own.mount_kind == k::VEHICLE
                && receiver.own.mount.is_some_and(|lease| lease.0 == key.0 && lease.1 == key.1);
            collision.enable_body(row.kernel, !own);
            let p = r::POSITION;
            let q = r::QUATERNION;
            let mut matrix = Matrix4::default();
            matrix.compose(
                &Vector3::new(fact[p], fact[p + 1], fact[p + 2]),
                &Quaternion::new(fact[q], fact[q + 1], fact[q + 2], fact[q + 3]),
                &Vector3::new(1., 1., 1.),
            );
            let mut inverse = matrix;
            inverse.invert();
            let mut lo = [f64::INFINITY; 3];
            let mut hi = [f64::NEG_INFINITY; 3];
            for mask in 0..8 {
                let b = row.bounds;
                let mut point = Vector3::new(
                    b[if mask & 1 == 0 { 0 } else { 3 }],
                    b[if mask & 2 == 0 { 1 } else { 4 }],
                    b[if mask & 4 == 0 { 2 } else { 5 }],
                );
                point.apply_matrix4(&matrix);
                for (i, v) in [point.x, point.y, point.z].iter().enumerate() {
                    lo[i] = lo[i].min(*v);
                    hi[i] = hi[i].max(*v);
                }
            }
            let io = collision.io_mut();
            io[20..23].copy_from_slice(&lo);
            io[23..26].copy_from_slice(&hi);
            io[26..42].copy_from_slice(&matrix.elements);
            io[42..58].copy_from_slice(&inverse.elements);
            collision.body_set(row.kernel);
        }
        collision.body_flags(world, &mut self.flags);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bfe_content::install::CollisionKernel;

    #[test]
    fn passenger_queries_exclude_only_the_occupied_vehicle_generation() {
        let mut collision = Box::new(CollisionOwner::new());
        let vertices = [-1., -1., 0., 1., -1., 0., 0., 1., 0.];
        let indices = [0, 2, 1];
        let bvh = bfe_content::bvh::build(bfe_content::bvh::Positions::F32(&vertices), &indices);
        let positions = collision.positions(&vertices);
        let tree = collision.tree(positions, &indices, &bvh);
        let world = collision.world(tree, -1, -1);
        let mut bodies = Bodies::new(&mut collision, world, &[(7, tree, [-1., -1., 0., 1., 1., 0.])], 0);
        let mut receiver = Box::new(Receiver::new(serde_json::json!({})));
        receiver.corrections = 1;
        let row = &mut receiver.own_fact;
        row[r::KIND] = k::VEHICLE as f64;
        row[r::ID] = 5.;
        row[r::GENERATION] = 9.;
        row[r::CLASS_ID] = 7.;
        row[r::QUATERNION + 3] = 1.;
        bodies.frame(&receiver, &mut collision, world);
        assert_eq!(bodies.flags[0], 1.);

        // A passenger has an infantry body and no piloted-hull header.
        receiver.own.vehicle = 0.;
        receiver.own.mount = Some((5, 9, 1));
        receiver.own.mount_kind = k::VEHICLE;
        bodies.frame(&receiver, &mut collision, world);
        assert_eq!(bodies.flags[0], 0.);

        // Independent entity ids and reused generations cannot hide a hull.
        receiver.own.mount_kind = k::TURRET;
        bodies.frame(&receiver, &mut collision, world);
        assert_eq!(bodies.flags[0], 1.);
        receiver.own.mount_kind = k::VEHICLE;
        receiver.own.mount = Some((5, 10, 1));
        bodies.frame(&receiver, &mut collision, world);
        assert_eq!(bodies.flags[0], 1.);
    }
}
