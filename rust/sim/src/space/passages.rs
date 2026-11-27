//! Links mission-controlled navigation barriers to the doors at their centres.
use bfe_wide::Wide;
use super::objects::SpaceDoor;
use super::digest::Fnv;
use crate::world::owner::World;
use bfe_content::{map::MapAdmission, value::Value};

#[derive(Clone, Debug)]
pub struct PassageBarrier {
    pub name: String,
    pub layer: Option<String>,
    pub source_index: u32,
    pub door: usize,
    mission_enabled: Option<bool>,
    delivered: Option<bool>,
}

impl PassageBarrier {
    pub(super) fn digest(&self, hash: Wide) -> Wide {
        let mut m = Fnv::new(hash);
        m.small(self.mission_enabled.map_or(2, u32::from));
        m.finish()
    }

    /// A mission-enabled barrier blocks independently of the physical shutter.
    /// A disabled barrier follows the shutter as it opens and closes.
    pub fn requested_enabled(&self, door: &SpaceDoor) -> Option<bool> {
        self.mission_enabled
            .map(|enabled| enabled || door.passage_blocked())
    }
}

fn arrays(map: &MapAdmission) -> impl Iterator<Item = (Option<&str>, &[Value])> {
    std::iter::once((None, &map.map_data))
        .chain(
            map.active_layers.iter().filter_map(|name| {
                Some((Some(name.as_str()), map.map_data.get("layers")?.get(name)?))
            }),
        )
        .map(|(name, map)| {
            (
                name,
                map.get("barriers")
                    .and_then(Value::as_array)
                    .unwrap_or_default(),
            )
        })
}

pub(super) fn validate(map: &MapAdmission) -> Result<(), String> {
    for (_, rows) in arrays(map) {
        if rows.len() > 65536 {
            return Err("too many navigation barriers".into());
        }
        for b in rows {
            for (key, size) in [("xfrm", 12), ("size", 3)] {
                if !b.get(key).and_then(Value::as_array).is_some_and(|v| {
                    v.len() == size
                        && v.iter().all(|n| {
                            n.as_f64().is_some_and(|n| {
                                n.is_finite()
                                    && (n as f32).is_finite()
                                    && (key != "size" || n >= 0.0)
                            })
                        })
                }) {
                    return Err(format!("invalid barrier {key}"));
                }
            }
            for key in ["sourceIndex", "flags"] {
                if !b
                    .get(key)
                    .and_then(Value::as_f64)
                    .is_some_and(|n| n.fract() == 0.0 && (0.0..=u32::MAX as f64).contains(&n))
                {
                    return Err(format!("invalid barrier {key}"));
                }
            }
        }
    }
    Ok(())
}

pub(super) fn candidates(map: &MapAdmission, doors: &[SpaceDoor]) -> Vec<PassageBarrier> {
    let mut links = Vec::new();
    for (layer, rows) in arrays(map) {
        for r in rows {
            let Some(name) = r
                .get("name")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            let flags = r.get("flags").and_then(Value::as_f64).unwrap_or_default() as u32;
            if flags & 1 == 0 {
                continue;
            }
            let Some(matrix) = r.get("xfrm").and_then(Value::as_array) else {
                continue;
            };
            let (x, z) = (matrix[9].as_f64().unwrap(), -matrix[11].as_f64().unwrap());
            // Ignore long room walls crossing the path. Only the centre of a
            // switchable gate lies in that door's authored proximity trigger.
            let mut matching = doors.iter().filter(|d| {
                let dx = x - d.trigger_position[0];
                let dz = z - d.trigger_position[2];
                dx * dx + dz * dz <= d.door_trigger_radius * d.door_trigger_radius
            });
            let Some(door) = matching.next() else {
                continue;
            };
            if matching.next().is_some() {
                continue;
            }
            links.push(PassageBarrier {
                name: name.to_owned(),
                layer: layer.map(str::to_owned),
                source_index: r.get("sourceIndex").and_then(Value::as_f64).unwrap() as u32,
                door: door.id,
                mission_enabled: None,
                delivered: None,
            });
        }
    }
    links
}

pub type NavigationToggle = fn(&mut World, &str, bool) -> bool;

#[derive(Clone, Debug)]
pub(crate) struct DoorConnection {
    name: String,
    doors: Vec<usize>,
    delivered: Option<bool>,
}

// Planning hubs live on a shared plane. Intersect the walk segment with the
// shutter's plane in XZ, within the authored proximity radius of the door.
fn crosses_door(a: [f64; 2], b: [f64; 2], m: &[f64; 16], radius: f64) -> bool {
    let side = |p: [f64; 2]| (p[0] - m[12]) * m[8] + (p[1] - m[14]) * m[10];
    let (start, end) = (side(a), side(b));
    if start * end >= 0.0 { return false; }
    let t = start / (start - end);
    let dx = a[0] + t * (b[0] - a[0]) - m[12];
    let dz = a[1] + t * (b[1] - a[1]) - m[14];
    dx * dx + dz * dz <= radius * radius
}

pub(super) fn connections(map: &MapAdmission, bindings: &[super::door_animation::Binding]) -> Result<Vec<DoorConnection>, String> {
    let mut result = Vec::new();
    let Some(plan) = map.map_data.get("plan").filter(|v| !v.is_null() && !bindings.is_empty()) else { return Ok(result) };
    let hubs = plan.get("hubs").and_then(Value::as_array).ok_or("missing door planning hubs")?;
    let arcs = plan.get("arcs").and_then(Value::as_array).ok_or("missing door planning arcs")?;
    if hubs.len() > 8192 || arcs.len() > 4096 || arcs.len().saturating_mul(bindings.len()) > 1_000_000 {
        return Err("door planning geometry exceeds the admission limit".into());
    }
    let endpoint = |arc: &Value, key: &str| -> Result<[f64; 2], String> {
        let n = arc.get(key).and_then(Value::as_f64).filter(|n| n.fract() == 0.0 && (0.0..hubs.len() as f64).contains(n))
            .ok_or_else(|| format!("invalid door planning arc {key}"))?;
        let position = hubs[n as usize].get("position").and_then(Value::as_array).filter(|v| v.len() == 3)
            .ok_or("invalid door planning hub position")?;
        let coord = |i: usize| position[i].as_f64().filter(|v| (*v as f32).is_finite()).ok_or("invalid door planning coordinate");
        Ok([coord(0)?, coord(2)?])
    };
    let mut names = std::collections::HashSet::new();
    for arc in arcs {
        let Some(name) = arc.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()) else { continue };
        let flags = arc.get("filterFlags").and_then(Value::as_f64)
            .filter(|v| v.fract() == 0.0 && (0.0..=u32::MAX as f64).contains(v)).ok_or("invalid door planning filter")?;
        if flags as u32 & 1 == 0 { continue; }
        if !names.insert(name.to_ascii_lowercase()) { return Err(format!("ambiguous door planning connection {name}")); }
        let (a, b) = (endpoint(arc, "start")?, endpoint(arc, "end")?);
        let mut doors = Vec::new();
        for (id, binding) in bindings.iter().enumerate() {
            let obj = &map.placed[binding.instance];
            let radius = obj.door_animation.as_ref().map_or(super::objects::PROVISIONAL_DOOR_TRIGGER_RADIUS, |d| d.trigger_radius);
            if crosses_door(a, b, &obj.matrix, radius) { doors.push(id); }
        }
        if !doors.is_empty() { result.push(DoorConnection { name: name.to_owned(), doors, delivered: None }); }
    }
    Ok(result)
}

impl super::SpaceBattlefield {
    pub(super) fn publish_passages(&mut self, world: &mut World) {
        if let Some(set) = self.navigation_toggle { self.deliver_passages(world, set); }
        if let Some(set) = self.planning_toggle {
            for connection in &mut self.planning_connections {
                let blocked = connection.doors.iter().any(|&id| self.doors[id].passage_blocked()
                    || self.passage_barriers.iter().any(|b| b.door == id && b.mission_enabled == Some(true)));
                if connection.delivered != Some(blocked) && set(world, &connection.name, blocked) {
                    connection.delivered = Some(blocked);
                }
            }
        }
    }
    pub(super) fn deliver_passages(
        &mut self,
        world: &mut World,
        mut set: impl FnMut(&mut World, &str, bool) -> bool,
    ) {
        for link in &mut self.passage_barriers {
            let Some(enabled) = link.requested_enabled(&self.doors[link.door]) else {
                continue;
            };
            if link.delivered != Some(enabled) && set(world, &link.name, enabled) {
                link.delivered = Some(enabled);
            }
        }
    }
}

impl World {
    /// Register the navigation writer for individual named planning connections.
    /// True blocks the arc. A writer returning false is retried on the next tick.
    pub fn space_connect_planning_arcs(&mut self, set: NavigationToggle) -> bool {
        let Some(mut s) = self.space.take() else { return false };
        for c in &mut s.planning_connections { c.delivered = None; }
        s.planning_toggle = Some(set);
        s.publish_passages(self);
        self.space = Some(s);
        true
    }

    /// Authored walk connections crossing this placed shutter.
    pub fn space_door_connections(&self, door: usize) -> impl Iterator<Item = &str> {
        self.space().into_iter().flat_map(|s| &s.planning_connections)
            .filter(move |c| c.doors.contains(&door)).map(|c| c.name.as_str())
    }

    /// Connect an admitted world's named navigation barrier writer and publish its initial state.
    pub fn space_connect_navigation(&mut self, set: NavigationToggle) -> bool {
        let Some(s) = self.space.as_mut() else {
            return false;
        };
        for link in &mut s.passage_barriers {
            link.delivered = None;
        }
        s.navigation_toggle = Some(set);
        self.space_sync_passage_barriers(set);
        true
    }

    /// Mission-bound barrier identities for this placed door, in prepared order.
    pub fn space_door_barriers(&self, door: usize) -> impl Iterator<Item = &PassageBarrier> {
        self.space()
            .into_iter()
            .flat_map(|s| &s.passage_barriers)
            .filter(move |link| link.door == door && link.mission_enabled.is_some())
    }

    /// Associate a mission barrier command with its door. Other named groups
    /// continue to belong to the navigation owner's script API.
    pub fn space_set_passage_barrier(&mut self, name: &str, enabled: bool) -> bool {
        let Some(mut s) = self.space.take() else {
            return false;
        };
        let mut found = false;
        for link in &mut s.passage_barriers {
            if link.name.eq_ignore_ascii_case(name) {
                link.mission_enabled = Some(enabled);
                found = true;
            }
        }
        if found { s.publish_passages(self); }
        self.space = Some(s);
        found
    }

    /// Read a prepared barrier by its layer and source index, including after delivery.
    pub fn space_passage_state(&self, layer: Option<&str>, source_index: u32) -> Option<bool> {
        let space = self.space()?;
        let link = space
            .passage_barriers
            .iter()
            .find(|link| link.layer.as_deref() == layer && link.source_index == source_index)?;
        link.requested_enabled(space.doors.get(link.door)?)
    }

    /// Apply outstanding named changes to the navigation owner before planning.
    /// A refused change stays pending for the next call.
    pub fn space_sync_passage_barriers(&mut self, set: impl FnMut(&mut World, &str, bool) -> bool) {
        let Some(mut s) = self.space.take() else {
            return;
        };
        s.deliver_passages(self, set);
        self.space = Some(s);
    }
}

#[cfg(test)]
mod tests {
    use super::crosses_door;

    #[test]
    fn planning_link_crosses_the_placed_panel_in_either_direction() {
        let mut m = bfe_content::three::Matrix4::identity().elements;
        m[8] = 1.0;
        m[10] = 0.0;
        m[12] = 14.0;
        m[14] = -8.0;
        let (a, b) = ([11.0, -9.0], [23.0, -7.0]);
        assert!(crosses_door(a, b, &m, 2.0) && crosses_door(b, a, &m, 2.0));
        assert!(!crosses_door([13.8, -10.0], [13.8, -6.0], &m, 2.0));
        assert!(!crosses_door([11.0, -5.0], [23.0, -5.0], &m, 2.0));
    }

    #[test]
    #[ignore = "requires a prepared ship interior"]
    fn prepared_doors_publish_independent_connections_for_a_positioned_soldier() {
        use crate::{gen::{Unit, UnitBody}, space::SPACE_FIXED_DT, world::{admit, owner::World}};
        use std::{cell::RefCell, collections::BTreeMap};
        thread_local! { static ARCS: RefCell<BTreeMap<String, bool>> = RefCell::default(); }
        let Ok(root) = std::env::var("BFE_SWBF2_CONTENT") else { return };
        let files = bfe_content::files::DirFiles::from_resolved_root(root, bfe_family::GameFamily::Swbf2);
        let options = admit::Options {
            family: bfe_content::GameFamily::Swbf2,
            map: "spa6".into(),
            mode: "ass".into(),
            side: "rep".into(),
            seed: 41,
            ..Default::default()
        };
        let mut world = World::new();
        admit::admit_for_family(&mut world, &files, bfe_content::GameFamily::Swbf2, &options, Some("spa6c_ass"), false).unwrap();
        let count = world.space().unwrap().doors.len();
        assert!(count > 1);
        for id in 0..count { assert!(world.space_set_door_locked(id, true)); }
        ARCS.with_borrow_mut(|arcs| arcs.clear());
        assert!(world.space_connect_planning_arcs(|_, name, blocked| {
            ARCS.with_borrow_mut(|arcs| arcs.insert(name.to_owned(), blocked));
            true
        }));
        ARCS.with_borrow(|arcs| assert!(arcs.len() == count && arcs.values().all(|b| *b)));
        let soldier = world.sim.store.unit.insert(Unit { alive: true, ..Default::default() });
        for id in 0..count {
            let door = &world.space().unwrap().doors[id];
            let body = door.body.unwrap();
            let trigger = door.trigger_position;
            let arc = world.space_door_connections(id).next().unwrap().to_owned();
            println!("door={} instance={} body={body} arc={arc}", door.name, door.instance);
            let UnitBody::Soldier(s) = &mut world.sim.store.unit.get_mut(soldier).unwrap().soldier else { unreachable!() };
            s.position = trigger;
            assert!(world.space_set_door_locked(id, false));
            for _ in 0..120 {
                assert!(world.space_step_tick(SPACE_FIXED_DT));
                if !world.space_door_snapshot(id).unwrap().collidable { break; }
            }
            assert_eq!(world.space_body_enabled(body), Some(false));
            ARCS.with_borrow(|arcs| assert!(arcs.iter().all(|(name, blocked)| *blocked == (name != &arc))));
            assert!(world.space_set_door_locked(id, true));
            assert_eq!(world.space_body_enabled(body), Some(true));
            ARCS.with_borrow(|arcs| assert!(arcs.values().all(|blocked| *blocked)));
        }
    }
}
