//! The placed buildings' collision bodies in the client's own collision world:
//! command posts, health and ammunition stations, doors, animated props and
//! turret buildings. The world gives each of them a body per look (built,
//! destroyed, unbuilt); without the same bodies here the player's predicted
//! soldier walks through what the world stops him at, and is pulled back.
use super::receiver::Receiver;
use bfe_content::{content::Content, install::CollisionKernel, Value};
use bfe_presentation::network_layout::{net_kind as k, net_record as r};
use bfe_sim::collision::CollisionOwner;
use three::Matrix4;

fn read_prop_matrix(desc: &Value) -> [f64; 16] {
    if let Some(arr) = desc.get("matrix").and_then(Value::as_array).filter(|a| a.len() >= 16) {
        std::array::from_fn(|k| arr[k].as_f64().unwrap_or(if k % 5 == 0 { 1.0 } else { 0.0 }))
    } else {
        Matrix4::default().elements
    }
}

pub(crate) fn transform_aabb(lo: [f64; 3], hi: [f64; 3], m: &[f64; 16]) -> [f64; 6] {
    let mut bmin = [f64::INFINITY; 3];
    let mut bmax = [f64::NEG_INFINITY; 3];
    for ix in 0..2 {
        let x = if ix == 0 { lo[0] } else { hi[0] };
        for iy in 0..2 {
            let y = if iy == 0 { lo[1] } else { hi[1] };
            for iz in 0..2 {
                let z = if iz == 0 { lo[2] } else { hi[2] };
                let wx = m[0] * x + m[4] * y + m[8] * z + m[12];
                let wy = m[1] * x + m[5] * y + m[9] * z + m[13];
                let wz = m[2] * x + m[6] * y + m[10] * z + m[14];
                bmin[0] = bmin[0].min(wx);
                bmin[1] = bmin[1].min(wy);
                bmin[2] = bmin[2].min(wz);
                bmax[0] = bmax[0].max(wx);
                bmax[1] = bmax[1].max(wy);
                bmax[2] = bmax[2].max(wz);
            }
        }
    }
    [bmin[0], bmin[1], bmin[2], bmax[0], bmax[1], bmax[2]]
}

struct Building {
    /// Source prop instance index in `props.json["instances"]`.
    instance: usize,
    /// Its place in the compact list of live buildings a turret record names.
    live: Option<usize>,
    /// True for 2005 props driven by world animations (model-space collision).
    world_animated: bool,
    /// The kernels of its built, destroyed and unbuilt looks.
    looks: [Option<u32>; 3],
    /// Local AABB per look for updating world-animated moving bodies.
    local_bounds: [Option<([f64; 3], [f64; 3])>; 3],
}
pub struct Props {
    rows: Vec<Building>,
    /// For each Space building (the battle's building order), its row here.
    space_rows: Vec<Option<usize>>,
}
/// A world-animated building's bodies: its prop instance and, per look, the
/// body and its local box.
pub type AnimatedBodies = (usize, [Option<(u32, [f64; 3], [f64; 3])>; 3]);
impl Props {
    /// The bodies of the buildings world animations move (the frigates).
    pub fn animated_bodies(&self) -> Vec<AnimatedBodies> {
        self.rows.iter().filter(|b| b.world_animated).map(|b| {
            (b.instance, std::array::from_fn(|look| Some((b.looks[look]?, b.local_bounds[look]?.0, b.local_bounds[look]?.1))))
        }).collect()
    }
    pub fn built_body(&self, building: usize) -> Option<u32> {
        self.rows.iter().find(|row| row.live == Some(building))?.looks[0]
    }
    /// Every building's bodies, in instance order and look order, as the
    /// world makes them. Only a building's first look is enabled: unbuilt
    /// where it starts that way, else built.
    pub fn new(collision: &mut CollisionOwner, world: u32, content: &Content) -> Result<Self, String> {
        let instances = content.battle.props.as_ref().and_then(|p| p.get("instances")).and_then(Value::as_array).unwrap_or(&[]);
        let live = bfe_sim::world::admit::rows::props::live_sources(&content.battle);
        let mut shared: Vec<(*const f32, usize)> = Vec::new();
        let mut serial = collision.state.bodies.len() as f64;
        let mut rows = Vec::new();
        for prop in &content.prop_bodies {
            let desc = instances.get(prop.instance).ok_or("a prop body names no instance")?;
            let first = if desc.get("unbuilt").is_some_and(Value::truthy) { 2 } else { 0 };
            let inst_matrix = if prop.world_animated { read_prop_matrix(desc) } else { Matrix4::default().elements };
            let mut looks = [None; 3];
            let mut local_bounds = [None; 3];
            for (look, shape) in [&prop.built, &prop.destroyed, &prop.unbuilt].into_iter().enumerate() {
                let Some(shape) = shape else { continue };
                let key = std::rc::Rc::as_ptr(&shape.positions) as *const f32;
                let positions = match shared.iter().find(|(k, _)| *k == key) {
                    Some(&(_, id)) => id,
                    None => {
                        let id = collision.positions(&shape.positions);
                        shared.push((key, id));
                        id
                    }
                };
                let geometry = bfe_content::bvh::build(bfe_content::bvh::Positions::F32(&shape.positions), &shape.indices);
                let tree = collision.tree(positions, &shape.indices, &geometry);
                collision.state.trees[tree as usize].polygons.extend_from_slice(&shape.polygons);
                let body = collision.body_new(tree, u32::from(prop.world_animated));
                if body < 0 {
                    return Err("the collision world has no room for a prop".into());
                }
                let body = body as u32;
                serial += 1.;
                collision.state.bodies[body as usize].offset = serial * 16777216.;
                let (lo, hi) = bfe_sim::vehicles::build::local_bounds(&shape.positions, &shape.indices);
                if prop.world_animated {
                    let world_box = transform_aabb(lo, hi, &inst_matrix);
                    let mut inv = Matrix4 { elements: inst_matrix };
                    inv.invert();
                    let io = collision.io_mut();
                    io[20..26].copy_from_slice(&world_box);
                    io[26..42].copy_from_slice(&inst_matrix);
                    io[42..58].copy_from_slice(&inv.elements);
                } else {
                    let io = collision.io_mut();
                    io[20..23].copy_from_slice(&lo);
                    io[23..26].copy_from_slice(&hi);
                }
                collision.body_set(body);
                let w = &mut collision.state.worlds[world as usize];
                w.order.push(body);
                w.version += 1;
                collision.enable_body(body, look == first);
                looks[look] = Some(body);
                local_bounds[look] = Some((lo, hi));
            }
            rows.push(Building {
                instance: prop.instance,
                live: live.iter().position(|source| *source == prop.instance),
                world_animated: prop.world_animated,
                looks,
                local_bounds,
            });
        }
        Ok(Self { rows, space_rows: Vec::new() })
    }
    /// Binds a Space battle's buildings (their prop instances, in the battle's
    /// building order) to the rows here, so their looks follow the server's.
    pub fn bind_space_buildings(&mut self, instances: impl Iterator<Item = usize>) {
        self.space_rows = instances.map(|instance| self.rows.iter().position(|b| b.instance == instance)).collect();
    }
    /// A turret building's record says which look stands (0 unbuilt, 1
    /// built, 2 destroyed); its body follows.
    pub fn frame(&self, receiver: &Receiver, collision: &mut CollisionOwner) {
        for row in receiver.active().filter(|row| row[r::KIND] == k::TURRET as f64) {
            let Some(building) = self.rows.iter().find(|b| b.live == Some(row[r::BUILDING] as usize)) else {
                continue;
            };
            let shown = match row[r::STATE] as u32 {
                1 => 0,
                2 => 1,
                _ => 2,
            };
            for (look, kernel) in building.looks.iter().enumerate() {
                if let Some(kernel) = kernel {
                    collision.enable_body(*kernel, look == shown);
                }
            }
        }
        if let Some(space) = &receiver.space_online {
            // A Space building's collision is the body of the look its summary says stands
            // (a destroyed frigate with no destroyed shape has none), as on the server.
            for (slot, row) in self.space_rows.iter().enumerate() {
                let (Some(row), Some(look)) = (row.and_then(|r| self.rows.get(r)), space.building_look(slot)) else { continue };
                for (index, kernel) in row.looks.iter().enumerate() {
                    if let Some(kernel) = kernel {
                        collision.enable_body(*kernel, index == usize::from(look));
                    }
                }
            }
            for door in &space.door_states {
                if let Some(kernel) = self
                    .rows
                    .iter()
                    .find(|b| b.instance == door.instance)
                    .and_then(|b| b.looks[0])
                {
                    collision.enable_body(kernel, door.collidable);
                }
            }
            for building in &self.rows {
                if !building.world_animated {
                    continue;
                }
                let Some((live_matrix, live_inverse)) = space.world_anim_matrix(building.instance) else {
                    continue;
                };
                for (look, kernel) in building.looks.iter().enumerate() {
                    let (Some(kernel), Some((lo, hi))) = (*kernel, building.local_bounds[look]) else {
                        continue;
                    };
                    if let Some(body) = collision.state.bodies.get_mut(kernel as usize) {
                        body.bounds = transform_aabb(lo, hi, &live_matrix);
                        body.matrix = live_matrix;
                        body.inverse = live_inverse;
                    }
                }
            }
        }
    }
}
