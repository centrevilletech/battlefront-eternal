//! The placed buildings' collision bodies in the client's own collision world:
//! command posts, health and ammunition stations, doors, animated props and
//! turret buildings. The world gives each of them a body per look (built,
//! destroyed, unbuilt); without the same bodies here the player's predicted
//! soldier walks through what the world stops him at, and is pulled back.
use super::receiver::Receiver;
use bfe_content::{content::Content, install::CollisionKernel, Value};
use bfe_presentation::network_layout::{net_kind as k, net_record as r};
use bfe_sim::collision::CollisionOwner;

struct Building {
    /// Its place in the compact list of live buildings a turret record names.
    live: Option<usize>,
    /// The kernels of its built, destroyed and unbuilt looks.
    looks: [Option<u32>; 3],
}
pub struct Props {
    rows: Vec<Building>,
}
impl Props {
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
            let mut looks = [None; 3];
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
                let body = collision.body_new(tree, 0);
                if body < 0 {
                    return Err("the collision world has no room for a prop".into());
                }
                let body = body as u32;
                serial += 1.;
                collision.state.bodies[body as usize].offset = serial * 16777216.;
                let (lo, hi) = bfe_sim::vehicles::build::local_bounds(&shape.positions, &shape.indices);
                let io = collision.io_mut();
                io[20..23].copy_from_slice(&lo);
                io[23..26].copy_from_slice(&hi);
                collision.body_set(body);
                let w = &mut collision.state.worlds[world as usize];
                w.order.push(body);
                w.version += 1;
                collision.enable_body(body, look == first);
                looks[look] = Some(body);
            }
            rows.push(Building { live: live.iter().position(|source| *source == prop.instance), looks });
        }
        Ok(Self { rows })
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
    }
}
