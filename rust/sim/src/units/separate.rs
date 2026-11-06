//! Soldiers pushed apart (src/game.js separateSoldiers), as the original
//! game's collision pass does for soldiers, droids and droidekas: each
//! body's main_body is an upright cylinder for its posture, and an overlap
//! moves each along the contact by the other's share of their volumes.
//! Only side contacts are resolved; one standing on another's head is not.
//! Positions move; the next collision pass keeps them out of the walls.

use super::shapes::HitPosture;
use crate::gen::{Member, Unit, UnitBody, Units};
use three::Vector3;
use crate::store::Slab;

/// main_body by posture: radius, height and the middle's height.
const MAIN_STAND: [f64; 3] = [0.57, 1.65, 0.9];
const MAIN_CROUCH: [f64; 3] = [0.45, 1.45, 0.65];
const MAIN_PRONE: [f64; 3] = [0.8, 0.45, 0.25];
const WALKER_BODY: [f64; 3] = [1.0, 2.0, 1.0];

/// Who pushes whom this tick, beyond the units themselves: the battle's
/// own player (who is left out while he drives, mans a gun or is dead),
/// and whether every pair counts (a server's world, `separationScope.global`)
/// or only those within 60 m of the simulation camera.
#[derive(Clone, Copy, Debug)]
pub struct Scope {
    pub player: Option<u32>,
    pub player_out: bool,
    pub global: bool,
    pub camera: Vector3,
}

/// The pushers' cylinders and places, every pair visited each tick (the
/// JavaScript's flat arrays; their x and z move with each push).
#[derive(Default)]
pub struct Pushers {
    ids: Vec<crate::store::Ref<Unit>>,
    r: Vec<f64>,
    bottom: Vec<f64>,
    top: Vec<f64>,
    volume: Vec<f64>,
    x: Vec<f64>,
    z: Vec<f64>,
    // A body can be pushed by several later pairs. Publish it once while
    // retaining every individual tickCount increment used by lazy shapes.
    moved: Vec<bool>,
}

impl Pushers {
    /// UnitFactory.build reserves scratch before bodies enter the phase.
    /// Clearing and resizing these arrays then reuses construction storage.
    pub fn reserve(&mut self, capacity: usize) {
        self.ids.reserve(capacity.saturating_sub(self.ids.len()));
        for values in [&mut self.r, &mut self.bottom, &mut self.top,
            &mut self.volume, &mut self.x, &mut self.z] {
            values.reserve(capacity.saturating_sub(values.len()));
        }
        self.moved.reserve(capacity.saturating_sub(self.moved.len()));
    }

    /// separateSoldiers changes only contacting pairs, including corpses.
    /// Untouched passive bodies need no mirror publication at this boundary.
    pub fn changed(&self) -> impl Iterator<Item = crate::store::Ref<Unit>> + '_ {
        self.ids.iter().zip(&self.moved).filter_map(|(&id, &moved)| moved.then_some(id))
    }
}

/// separateSoldiers() over the registry's members in its order.
pub fn separate(registry: &Units, units: &mut Slab<Unit>, scope: &Scope, p: &mut Pushers) {
    p.ids.clear();
    for &m in &registry.list {
        let Member::Unit(r) = m else { continue };
        let Some(unit) = units.get(r) else { continue };
        if unit.seat.is_some() || matches!(unit.held, crate::store::Nullable::Value(_)) || unit.removed || !(unit.alive || unit.dead) {
            continue;
        }
        if Some(r.slot) == scope.player && scope.player_out {
            continue;
        }
        if !scope.global && unit.position().distance_to_squared(&scope.camera) > 60.0 * 60.0 {
            continue;
        }
        p.ids.push(r);
    }
    let n = p.ids.len();
    p.moved.clear();
    p.moved.resize(n, false);
    for v in [&mut p.r, &mut p.bottom, &mut p.top, &mut p.volume, &mut p.x, &mut p.z] {
        v.clear();
        v.resize(n, 0.0);
    }
    for i in 0..n {
        let unit = units.get(p.ids[i]).unwrap();
        let pos = unit.position();
        let body = if unit.walker {
            WALKER_BODY
        } else {
            match unit.hit_posture() {
                HitPosture::Stand => MAIN_STAND,
                HitPosture::Crouch => MAIN_CROUCH,
                HitPosture::Prone => MAIN_PRONE,
            }
        };
        let (r, h, y) = (body[0], body[1], body[2]);
        p.r[i] = r;
        p.bottom[i] = pos.y + y - h / 2.0;
        p.top[i] = pos.y + y + h / 2.0;
        p.volume[i] = std::f64::consts::PI * r * r * h;
        p.x[i] = pos.x;
        p.z[i] = pos.z;
    }
    for i in 0..n {
        for k in i + 1..n {
            if p.top[i] <= p.bottom[k] || p.top[k] <= p.bottom[i] {
                continue;
            }
            let mut dx = p.x[i] - p.x[k];
            let mut dz = p.z[i] - p.z[k];
            let reach = p.r[i] + p.r[k];
            let d2 = dx * dx + dz * dz;
            if d2 >= reach * reach {
                continue;
            }
            let (a, b) = (p.ids[i], p.ids[k]);
            let mut d = d2.sqrt();
            if d < 1e-4 {
                (dx,dz) = v8math::sin_cos(a.slot as f64);
                d = 1.0;
            }
            let depth = reach - d2.sqrt();
            let share = depth / (p.volume[i] + p.volume[k]);
            p.x[i] += (dx / d) * share * p.volume[k];
            p.z[i] += (dz / d) * share * p.volume[k];
            p.x[k] -= (dx / d) * share * p.volume[i];
            p.z[k] -= (dz / d) * share * p.volume[i];
            p.moved[i] = true;
            p.moved[k] = true;
            let (ax, az, bx, bz) = (p.x[i], p.z[i], p.x[k], p.z[k]);
            for (id, x, z) in [(a, ax, az), (b, bx, bz)] {
                let unit = units.get_mut(id).unwrap();
                match &mut unit.soldier {
                    UnitBody::Soldier(s) => {
                        s.position[0] = x;
                        s.position[2] = z;
                    }
                    UnitBody::Droideka(dk) => {
                        dk.position[0] = x;
                        dk.position[2] = z;
                    }
                }
                unit.tick_count += 1.0;
            }
        }
    }
}
