//! The things lying about, in Rust: mines and power-ups,
//! thrown by dispensers, dropped by the dead, placed by the world. Each
//! function transliterates the JavaScript method it names and reaches the
//! rest of the battle through the ordnance's [`World`] (its items' part,
//! ItemsWorld), as rust/sim/src/ordnance does.
//!
//! An item taken or set off is marked gone and stays in the list until the
//! next update's filter, as the JavaScript's does (a dispenser's count and
//! a blast's loop see it until then); the filter retires it.

// The items owner's world entry points.
pub mod module;

use crate::gen::*;
use crate::ordnance::defs::{Field, ItemDef};
use crate::ordnance::math::*;
use crate::ordnance::world::*;
use crate::ordnance::{blast_distance, Cx};
use crate::store::{DefId, Ref, Vec3};

const GRAVITY: f64 = 9.8;
const HARD: f64 = 1.0; // m/s into a surface that bounces rather than settles
const PICKUP_DELAY: f64 = 0.5; // s a flying item can't be taken
const FLOOR: f64 = -500.0;
pub const MINE_SINK: f64 = 0.2; // m/s
const SOLDIER_RADIUS: f64 = 0.35; // m, the soldier's body for pickups
const SOLDIER_HEIGHT: f64 = 1.55; // m

/// A death's drop list (Items.dropFor): each entry's class (None: one the
/// side does not have) and its probability (as the list gives it).
pub struct Drop {
    pub def: Option<DefId>,
    pub probability: Field,
}

impl<'a, W: World + ?Sized> Cx<'a, W> {
    #[inline]
    fn item(&mut self, r: Ref<Item>) -> &mut Item {
        self.s().item.get_mut(r).expect("an item the battle holds")
    }

    #[inline]
    fn item_def(&self, id: DefId) -> &ItemDef {
        self.defs().item(id)
    }

    /// spawn: a new item, unless the world has no room for its
    /// kind (counted over the list, the gone not yet filtered included).
    pub fn spawn(&mut self, def: DefId, position: Vec3, velocity: Vec3, yaw: f64, owner: ItemOwner, team: f64) -> Option<Ref<Item>> {
        let prepared = self.w.prepared_item(def);
        let (model,mine,life,health) = if let Some(info) = prepared {
            let mut live = 0;
            let mut index = 0;
            while index < self.s().items.items.len() {
                let other = {
                    let store = self.s();
                    store.item.get(store.items.items[index]).map(|item| item.def)
                };
                index += 1;
                if let Some(other) = other {
                    if self.w.prepared_item(other).expect("prepared item class").base == info.base { live += 1; }
                }
            }
            // Gone items count until Items.update filters the live list.
            if live as f64 >= info.pool { return None; }
            (info.model,info.mine,info.life,info.health)
        } else {
            let base = self.item_def(def).base.clone();
            let live = {
                // Counting borrows rows and definitions together, read-only;
                // no sibling pointer or mutable store loan crosses the scan.
                let defs = self.defs();
                let store = self.w.store();
                store.items.items.iter().filter(|&&r| store.item.get(r).is_some_and(|i| defs.item(i.def).base == base)).count()
            };
            if live as f64 >= self.defs().pool(&base) { return None; }
            let model = self.w.item_model(def);
            let d = self.item_def(def);
            (model,d.base.is("mine"),d.life_span.or_nullish(f64::INFINITY),d.max_health.or_nullish(f64::INFINITY))
        };
        let item = Item {
            def,
            mine,
            position,
            previous: position,
            velocity,
            yaw,
            up: UP,
            landed: false,
            sinking: false,
            radius: model.radius,
            center: model.center,
            turn: if model.mesh { Some(QUAT_IDENTITY) } else { None },
            owner,
            team,
            age: 0.0,
            life,
            health,
            gone: None,
            carrier: None,
            carrier_life: 0.0,
            carried: [0.0; 3],
            carried_up: UP,
            carried_forward: [0.0, 0.0, 1.0],
        };
        let r = self.s().item.insert(item);
        self.place(r);
        self.s().items.items.push(r);
        self.w.item_changed(r,true);
        Some(r)
    }

    /// dispense: a dispenser's throw; an owner already with
    /// `max_items` of this weapon's items about loses the oldest first.
    #[allow(clippy::too_many_arguments)]
    pub fn dispense(&mut self, weapon: &str, max_items: f64, def: DefId, position: Vec3, velocity: Vec3, yaw: f64, owner: ItemOwner, team: f64) -> Option<Ref<Item>> {
        let key = format!("{weapon}:{}", self.w.owner_key(owner));
        let key = self.strings().intern_str(&key);
        let mut list: Vec<Ref<Item>> = {
            let s = self.s();
            let held = s.items.owned.get(key).cloned().unwrap_or_default();
            held.into_iter().filter(|&r| s.item.get(r).is_some_and(|i| i.gone != Some(true))).collect()
        };
        if max_items > 0.0 && list.len() as f64 >= max_items {
            let oldest = list.remove(0);
            self.destroy(oldest);
        }
        let item = self.spawn(def, position, velocity, yaw, owner, team);
        if let Some(r) = item {
            list.push(r);
        }
        self.s().items.owned.set(key, list);
        item
    }

    /// Items.dispense with a lifecycle-prepared identity key and retained
    /// buffers. Keep the old canonical list visible until recursive effects
    /// finish; filter-then-set is observable when the oldest mine explodes.
    #[allow(clippy::too_many_arguments)]
    pub fn dispense_prepared(&mut self, key: crate::store::StrId, work: &mut Vec<Ref<Item>>, spare: &mut Vec<Ref<Item>>,
        most: f64, def: DefId, position: Vec3, velocity: Vec3, yaw: f64, owner: ItemOwner, team: f64) -> Option<Ref<Item>> {
        work.clear();
        {
            let store = self.s();
            if let Some(list) = store.items.owned.get(key) {
                for &item in list { if store.item.get(item).is_some_and(|i| i.gone != Some(true)) { work.push(item); } }
            }
        }
        if most > 0.0 && work.len() as f64 >= most { self.destroy(work.remove(0)); }
        let made = self.spawn(def,position,velocity,yaw,owner,team);
        if let Some(item) = made { work.push(item); }
        // Empty native entries retain their Vec as a tombstone. Re-adding
        // that logical key moves it to the end, as a deleted JS Map key does.
        let inactive = !self.w.dispenser_active(key);
        let old = if inactive { self.s().items.owned.remove(key) } else { None };
        let replaced = self.s().items.owned.replace(key,std::mem::take(work));
        *work = match old.or(replaced) { Some(buffer) => buffer, None => std::mem::take(spare) };
        self.w.dispenser_changed(key,false);
        made
    }

    /// dropFor: a soldier's drop at death, one roll down the
    /// class's drop list.
    pub fn drop_for(&mut self, drops: &[Drop], position: Vec3, velocity: Vec3, yaw: f64) -> Option<Ref<Item>> {
        let mut r = self.w.drop_roll();
        for drop in drops {
            r -= drop.probability.or_nullish(0.0);
            if r >= 0.0 {
                continue;
            }
            let def = drop.def?;
            // Thrown out to the soldier's side at 3 m/s, unless he was
            // moving faster than that.
            let speed = js_hypot(&[velocity[0], velocity[1], velocity[2]]);
            let out = if speed > 3.0 { velocity } else { scale([cos(yaw), 0.0, -sin(yaw)], 3.0) };
            return self.spawn(def, position, out, yaw, ItemOwner::Null, 0.0);
        }
        None
    }

    /// destroy: gone without a blast (a mine pops).
    pub fn destroy(&mut self, r: Ref<Item>) {
        if self.item(r).gone == Some(true) {
            return;
        }
        // Gone before its blast, which reaches the mines beside it, whose
        // blasts reach back here: a mine already gone ends the chain.
        self.remove_item(r);
        let (mine, def, position, owner) = {
            let i = self.item(r);
            (i.mine, i.def, i.position, i.owner)
        };
        let death = self.item_def(def).explosion_death.clone();
        if mine && death.truthy() && death.is_object() {
            self.w.explode(&death, position, owner);
        }
    }

    /// remove.
    fn remove_item(&mut self, r: Ref<Item>) {
        self.item(r).gone = Some(true);
        self.w.item_changed(r,false);
    }

    /// expire: the end of an item's LifeSpan (a time bomb goes off).
    fn expire(&mut self, r: Ref<Item>) {
        if self.item(r).gone == Some(true) {
            return;
        }
        self.remove_item(r);
        let (mine, def, position, owner) = {
            let i = self.item(r);
            (i.mine, i.def, i.position, i.owner)
        };
        let expire = self.item_def(def).explosion_expire.clone();
        if mine && expire.is_object() {
            self.w.explode(&expire, position, owner);
        }
    }

    /// hurt: damage to one item (a mine at 0 health pops).
    pub fn hurt(&mut self, r: Ref<Item>, amount: f64) {
        {
            let i = self.item(r);
            if i.gone == Some(true) || !i.mine || !i.health.is_finite() {
                return;
            }
            i.health -= amount;
        }
        self.w.item_changed(r,false);
        if self.item(r).health <= 0.0 {
            self.destroy(r);
        }
    }

    /// blast: an explosion's damage to the mines lying about.
    #[allow(clippy::neg_cmp_op_on_partial_ord)] // `!(damage > 0)`: NaN included, as the JavaScript tests it
    pub fn blast(&mut self, explosion: DefId, position: Vec3) {
        let mut at = 0;
        while at < self.s().items.items.len() {
            let r = self.s().items.items[at];
            at += 1;
            let (gone, mine, landed, health, def, center, radius) = {
                let i = self.item(r);
                (i.gone, i.mine, i.landed, i.health, i.def, i.position, i.radius)
            };
            if gone == Some(true) || !mine || !landed || !health.is_finite() {
                continue;
            }
            let distance = blast_distance(position,center,radius);
            let damage = if let Some(damage) = self.w.item_blast_damage(explosion,def,distance) { damage } else {
                let health_type = &self.item_def(def).health_type;
                self.defs().explosion(explosion).damage_at(distance,health_type)
            };
            if !(damage > 0.0) || self.w.blocked(position, center) {
                continue;
            }
            let dead = {
                let i = self.item(r);
                i.health -= damage;
                i.health <= 0.0
            };
            self.w.item_changed(r,false);
            if dead {
                self.destroy(r);
            }
        }
    }

    /// update: each item ages and flies, a mine watches for an
    /// enemy, a power-up for a taker (`targets` of them, the world's), then
    /// the list keeps the items not gone and each dispenser's list its own.
    pub fn items_update(&mut self, dt: f64, targets: usize) {
        let mut at = 0;
        while at < self.s().items.items.len() {
            let r = self.s().items.items[at];
            at += 1;
            let gone = {
                let i = self.item(r);
                if i.gone != Some(true) {
                    i.age += dt;
                    i.life -= dt;
                }
                i.gone == Some(true)
            };
            if gone {
                continue;
            }
            // A carried mine is first put where its hull has taken it, as
            // the 2004 game does, so a time bomb on a moving hull goes off
            // there.
            if self.item(r).carrier.is_some() {
                self.carry(r, dt);
            }
            let (life, y) = {
                let i = self.item(r);
                (i.life, i.position[1])
            };
            if life <= 0.0 || y < FLOOR {
                // A time bomb's timer ran out: it goes off where it lies.
                if life <= 0.0 {
                    self.expire(r);
                } else {
                    self.remove_item(r);
                }
                continue;
            }
            if !self.item(r).landed {
                self.fly(r, dt);
            }
            if self.item(r).mine {
                self.watch(r, targets);
            } else {
                self.offer(r, targets);
            }
        }
        let owned = self.w.owned_item_frames();
        let mut read = 0;
        let mut write = 0;
        while read < self.s().items.items.len() {
            let item = self.s().items.items[read];read += 1;
            if self.s().item.get(item).is_some_and(|i| i.gone != Some(true)) {
                self.s().items.items[write] = item;write += 1;
                self.w.item_changed(item,false);
            } else if owned {
                // The terminal snapshot already went to the event stream;
                // no retired Vec needs to retain a second copy of this row.
                self.w.item_retired(item);
                self.s().item.remove(item);
            } else { self.s().item.retire(item); }
        }
        self.s().items.items.truncate(write);
        let mut index = 0;
        while index < self.s().items.owned.entries.len() {
            let (key,changed,empty) = {
                let store = self.s();
                let (key,list) = &mut store.items.owned.entries[index];
                let before = list.len();
                list.retain(|&r| store.item.get(r).is_some_and(|i| i.gone != Some(true)));
                (*key,before != list.len(),list.is_empty())
            };
            if changed || owned && empty { self.w.dispenser_changed(key,empty); }
            if empty && (!owned || !self.w.retain_empty_dispenser(key)) { self.s().items.owned.entries.remove(index); }
            else { index += 1; }
        }
    }

    /// fly: gravity in two half steps around the move.
    fn fly(&mut self, r: Ref<Item>, dt: f64) {
        let def = self.item(r).def;
        let g = -GRAVITY * self.item_def(def).gravity.or_nullish(1.0);
        let (mine, sinking, previous, position) = {
            let i = self.item(r);
            if i.sinking {
                i.velocity = [0.0, -MINE_SINK, 0.0];
            } else {
                i.velocity[1] += g * 0.5 * dt;
            }
            i.previous = i.position;
            i.position = add_scaled(i.position, i.velocity, dt);
            if !i.sinking {
                i.velocity[1] += g * 0.5 * dt;
            }
            (i.mine, i.sinking, i.previous, i.position)
        };
        if mine && !sinking && self.w.item_water_crossing(previous, position).is_some() {
            let i = self.item(r);
            i.sinking = true;
            i.velocity = [0.0, -MINE_SINK, 0.0];
        }
        self.contact(r);
        self.place(r);
    }

    /// contact: what the item meets between where it was and where
    /// it is.
    fn contact(&mut self, r: Ref<Item>) {
        let (position, previous, def, mine, owner, young) = {
            let i = self.item(r);
            (i.position, i.previous, i.def, i.mine, i.owner, i.life.is_finite() && i.age < 0.5)
        };
        let ray = sub(position, previous);
        let length = crate::ordnance::math::length(ray);
        if length < 1e-6 {
            return;
        }
        let ray = divide(ray, length);
        let start = add_scaled(previous, ray, -0.05);
        let hit = self.w.item_cast(start, ray, length + 0.1);
        // A mine meets soldiers and droids too, team mates included, and
        // bounces off them rather than sticking, as the 2004 game's do. Its
        // thrower is left out only for the first 0.5 s of an item with a
        // LifeSpan (a time bomb); a land mine meets him too. A body the mine
        // is still inside of as the step starts (its thrower's, as it leaves
        // his hand) or is moving away from does not turn it.
        if mine {
            let thrower = match owner {
                ItemOwner::Unit(unit) if young => OrdnanceOwner::Unit(unit),
                ItemOwner::Player if young => OrdnanceOwner::Player,
                _ => OrdnanceOwner::Null,
            };
            if let Some(body) = self.w.raycast_units(start, ray, length + 0.1, thrower, None, None, None) {
                if body.distance > 0.0 && hit.as_ref().is_none_or(|h| body.distance < h.distance)
                    && self.bounce_off_body(r, body.point, body.normal) {
                    return;
                }
            }
        }
        let Some(hit) = hit else { return };
        let n = hit.normal;
        // The hull it met, if it met one. A ridden animal is met as a living
        // body: a mine bounces off it.
        let hull = hit.body.and_then(|body| self.w.hull_of(body));
        if mine && hull.is_some_and(|hull| self.w.creature(hull)) {
            self.bounce_off_body(r, hit.point, n);
            return;
        }
        // And that hull's frame, if it can be undone.
        let hull = hull.and_then(|hull| self.s().vehicle.get(hull).map(|v| (hull, v.target_generation, v.matrix)))
            .filter(|&(_, _, matrix)| three::Matrix4 { elements: matrix }.determinant() != 0.0);
        let (rebound, friction) = {
            let d = self.item_def(def);
            (d.rebound.or_nullish(0.3), d.friction.or_nullish(0.5))
        };
        let i = self.item(r);
        let v = i.velocity;
        let d = dot(v, n);
        if d >= 0.0 {
            return;
        }
        if i.mine {
            // Stuck, upright on the surface (resting on it, the port's choice).
            i.position = add_scaled(hit.point, n, i.radius * 0.3);
            settle(i, n);
            // On a hull it rides along, held where it stuck in the hull's
            // frame, as the 2004 game's mines do.
            if let Some((carrier, life, matrix)) = hull {
                let mut inverse = three::Matrix4 { elements: matrix };
                inverse.invert();
                let forward = [sin(i.yaw), 0.0, cos(i.yaw)];
                i.carrier = Some(carrier);
                i.carrier_life = life;
                i.carried = transform_point(&inverse, i.position);
                i.carried_up = transform_direction(&inverse, i.up);
                i.carried_forward = transform_direction(&inverse, forward);
            }
            return;
        }
        if d > -HARD {
            i.position = hit.point;
            settle(i, n);
            return;
        }
        rebound_off(i, hit.point, n, d, rebound, friction);
    }

    /// bounce_off_body: off a body, by the item's Rebound and Friction; one moving
    /// away from the body's surface goes on as it was (false).
    fn bounce_off_body(&mut self, r: Ref<Item>, point: Vec3, n: Vec3) -> bool {
        let def = self.item(r).def;
        let (rebound, friction) = {
            let d = self.item_def(def);
            (d.rebound.or_nullish(0.3), d.friction.or_nullish(0.5))
        };
        let i = self.item(r);
        let d = dot(i.velocity, n);
        if d < 0.0 {
            rebound_off(i, point, n, d, rebound, friction);
        }
        d < 0.0
    }

    /// carry: a mine stuck to a hull goes where the hull has taken the
    /// place it stuck to, turned with it, still at rest on it (no velocity of
    /// its own); the tick the hull dies or is gone (cleared away, or another
    /// life of its slot) it falls from there, moving as the hull carried it
    /// over the last step.
    fn carry(&mut self, r: Ref<Item>, dt: f64) {
        let (carrier, life) = {
            let i = self.item(r);
            (i.carrier, i.carrier_life)
        };
        let frame = carrier.and_then(|hull| self.s().vehicle.get(hull))
            .filter(|v| v.alive && v.target_generation == life)
            .map(|v| v.matrix);
        let i = self.item(r);
        let Some(matrix) = frame else {
            i.carrier = None;
            i.landed = false;
            if dt > 0.0 {
                i.velocity = divide(sub(i.position, i.previous), dt);
            }
            return;
        };
        let m = three::Matrix4 { elements: matrix };
        let forward = transform_direction(&m, i.carried_forward);
        i.previous = i.position;
        i.position = transform_point(&m, i.carried);
        i.up = transform_direction(&m, i.carried_up);
        i.yaw = v8math::atan2(forward[0], forward[2]);
        self.place(r);
    }

    /// place: the model upright on its surface, facing its way.
    fn place(&mut self, r: Ref<Item>) {
        let i = self.item(r);
        if i.turn.is_none() {
            return;
        }
        let up = i.up;
        let mut forward = [sin(i.yaw), 0.0, cos(i.yaw)];
        forward = add_scaled(forward, up, -dot(forward, up));
        if length_sq(forward) < 1e-8 {
            forward = add_scaled([0.0, 0.0, 1.0], up, -up[2]);
        }
        let forward = normalize(forward);
        let right = normalize(cross(up, forward));
        i.turn = Some(from_basis(right, up, forward));
    }

    /// watch: a mine, set off by an enemy near enough and no longer
    /// closing in, unless a friend is near.
    fn watch(&mut self, r: Ref<Item>, targets: usize) {
        let (landed, def, team, position, owner) = {
            let i = self.item(r);
            (i.landed, i.def, i.team, i.position, i.owner)
        };
        let (trigger, trigger_radius, suppress_radius) = {
            let d = self.item_def(def);
            (d.explosion_trigger.clone(), d.trigger_radius.clone(), d.suppress_radius.clone())
        };
        if !landed || !trigger.truthy() || !trigger_radius.positive() {
            return;
        }
        let mut fire = false;
        for index in 0..targets {
            let target = self.w.watch(index, team);
            if target.relation == 0.0 {
                continue;
            }
            let toward = sub(target.center, position);
            let gap = length_sq(toward);
            if target.relation > 0.0 {
                // (x) ** 2: V8's pow is x * x for an exponent of 2.
                let reach = suppress_radius.num() + target.radius;
                if suppress_radius.positive() && gap < reach * reach {
                    return;
                }
                continue;
            }
            let reach = trigger_radius.num() + target.radius;
            if gap < reach * reach && dot(target.velocity, toward) >= 0.0 {
                fire = true;
            }
        }
        if !fire {
            return;
        }
        if trigger.is_object() {
            self.w.explode(&trigger, position, owner);
        }
        self.remove_item(r);
    }

    /// offer: a power-up, taken by the first soldier touching it
    /// that gets something.
    fn offer(&mut self, r: Ref<Item>, targets: usize) {
        let (landed, age, center, turn, position, radius, def) = {
            let i = self.item(r);
            (i.landed, i.age, i.center, i.turn, i.position, i.radius, i.def)
        };
        if !landed && age <= PICKUP_DELAY {
            return;
        }
        let mut c = center.unwrap_or([0.0, 0.0, 0.0]);
        if let Some(q) = turn {
            c = apply_quaternion(c, q);
        }
        let c = add(c, position);
        for index in 0..targets {
            let p = self.w.standing(index);
            let across = js_hypot(&[c[0] - p[0], c[2] - p[2]]);
            if across > SOLDIER_RADIUS + radius {
                continue;
            }
            if c[1] < p[1] - radius || c[1] > p[1] + SOLDIER_HEIGHT + radius {
                continue;
            }
            if self.w.give(index, def) {
                self.w.pickup(def, position);
                self.remove_item(r);
                return;
            }
        }
    }
}

/// The bounce off a surface with normal `n` met at `d` (the velocity along
/// `n`, below 0): the part into it gone, then Friction times it times
/// (1 + Rebound) of the rest, and back out at Rebound times it.
fn rebound_off(i: &mut Item, point: Vec3, n: Vec3, d: f64, rebound: f64, friction: f64) {
    i.position = add_scaled(point, n, 0.02);
    let mut v = add_scaled(i.velocity, n, -d);
    let along = crate::ordnance::math::length(v);
    if along > 1e-6 {
        v = scale(v, 1.0 - js_min((friction * -d * (1.0 + rebound)) / along, 1.0));
    }
    i.velocity = add_scaled(v, n, rebound * -d);
}

/// A point and a direction by a matrix (three.js applyMatrix4 and
/// transformDirection).
fn transform_point(m: &three::Matrix4, p: Vec3) -> Vec3 {
    let mut v = three::Vector3::new(p[0], p[1], p[2]);
    v.apply_matrix4(m);
    [v.x, v.y, v.z]
}
fn transform_direction(m: &three::Matrix4, d: Vec3) -> Vec3 {
    let mut v = three::Vector3::new(d[0], d[1], d[2]);
    v.transform_direction(m);
    [v.x, v.y, v.z]
}

/// settle: landed, upright on what it touches.
fn settle(i: &mut Item, normal: Vec3) {
    i.landed = true;
    i.sinking = false;
    i.velocity = [0.0, 0.0, 0.0];
    i.up = normal;
}

#[cfg(test)]
mod tests {
    //! Items against a stand-in world: a floor at y = 0, a hull's deck at
    //! y = 1 over 3 <= x <= 6 (while the hull has not moved; the hull a ridden
    //! animal or not), and one soldier standing as a sphere of radius 0.9 about
    //! (2, 0.9, 0), whom the units' cast leaves out when he is the owner.
    use super::*;
    use crate::ordnance::defs::Defs;
    use crate::store::{Nullable, RefSet, StrId, Strings};

    const MINE: DefId = DefId(0);
    const TIME_BOMB: DefId = DefId(1);
    const POWERUP: DefId = DefId(2);

    struct Stand {
        store: Store,
        strings: Strings,
        defs: Defs,
        soldier: Option<Ref<Unit>>,
        deck: Option<(Ref<Body>, Ref<Vehicle>)>,
        creature: bool,
    }

    impl World for Stand {
        fn store(&self) -> &Store { &self.store }
        fn store_mut(&mut self) -> &mut Store { &mut self.store }
        fn strings_mut(&mut self) -> &mut Strings { &mut self.strings }
        fn definitions(&self) -> &Defs { &self.defs }
        fn item_cast(&mut self, origin: Vec3, direction: Vec3, far: f64) -> Option<Hit> {
            let mut best: Option<Hit> = None;
            let mut plane = |height: f64, body: Option<Ref<Body>>, within: &dyn Fn(Vec3) -> bool| {
                if direction[1] >= 0.0 { return; }
                let distance = (height - origin[1]) / direction[1];
                let point = add_scaled(origin, direction, distance);
                if distance >= 0.0 && distance <= far && within(point) && best.as_ref().is_none_or(|b| distance < b.distance) {
                    best = Some(Hit { distance, point, normal: UP, terrain: body.is_none(), body });
                }
            };
            plane(0.0, None, &|_| true);
            if let Some((body, hull)) = self.deck {
                if self.store.vehicle.get(hull).is_some_and(|v| v.alive && v.matrix[12] == 0.0) {
                    plane(1.0, Some(body), &|p| (3.0..=6.0).contains(&p[0]));
                }
            }
            best
        }
        fn hull_of(&mut self, body: Ref<Body>) -> Option<Ref<Vehicle>> {
            self.deck.filter(|&(b, _)| b == body).map(|(_, hull)| hull)
        }
        fn creature(&mut self, _hull: Ref<Vehicle>) -> bool { self.creature }
        fn raycast_units(&mut self, origin: Vec3, direction: Vec3, far: f64, owner: OrdnanceOwner, _history_tick: Option<f64>, _ignore_body: Option<Ref<Body>>, _mount: Option<Ref<Vehicle>>) -> Option<UnitHit> {
            let unit = self.soldier?;
            if owner == OrdnanceOwner::Unit(unit) { return None; }
            // The ray against the sphere about (2, 0.9, 0), met at once from
            // inside, as the units' cast does.
            let to = sub([2.0, 0.9, 0.0], origin);
            let c = dot(to, to) - 0.81;
            let b = dot(to, direction);
            let distance = if c <= 0.0 { 0.0 } else if b < 0.0 || b * b < c { return None } else { b - (b * b - c).sqrt() };
            if distance > far { return None; }
            let point = add_scaled(origin, direction, distance);
            Some(UnitHit { unit, distance, point, normal: normalize(sub(point, [2.0, 0.9, 0.0])), part: Nullable::Null })
        }
        fn item_model(&mut self, _def: DefId) -> ItemModel { ItemModel { mesh: true, radius: 0.2, center: None } }
        fn owner_key(&mut self, _owner: ItemOwner) -> String { "thrower".into() }
        fn item_water_crossing(&mut self, _from: Vec3, _to: Vec3) -> Option<Vec3> { None }
        fn cast(&mut self, _origin: Vec3, _direction: Vec3, _far: f64, _out: Out, _ignore_body: Nullable<Ref<Body>>, _static_only: Option<bool>) -> Option<Hit> { unimplemented!() }
        fn cast_moving(&mut self, _origin: Vec3, _direction: Vec3, _far: f64, _out: Out, _ignore_body: Nullable<Ref<Body>>) -> Option<Hit> { unimplemented!() }
        fn rider_mount(&mut self, _body: Nullable<Ref<Body>>) -> Option<RiderMount> { unimplemented!() }
        fn fat_query(&mut self, _center: Vec3, _radius: f64, _owner: OrdnanceOwner, _struck: &RefSet<Unit>, _ignore_body: Option<Ref<Body>>) -> Option<Vec<Ref<Unit>>> { unimplemented!() }
        fn unit_sphere(&mut self, _unit: Ref<Unit>) -> Option<(Vec3, f64)> { unimplemented!() }
        fn is_droid(&mut self, _unit: Ref<Unit>) -> bool { unimplemented!() }
        fn gone(&mut self, _x: BeamSource) -> bool { unimplemented!() }
        fn beam_local_origin(&mut self, _source: BeamSource, _origin: Vec3) -> Option<Vec3> { unimplemented!() }
        fn beam_pose(&mut self, _source: BeamSource, _local_origin: Vec3, _position: &mut Vec3, _direction: &mut Vec3) { unimplemented!() }
        fn shot_tick(&mut self, _owner: OrdnanceOwner, _channel: f64) -> Nullable<f64> { unimplemented!() }
        fn history_tick(&mut self) -> f64 { unimplemented!() }
        fn shooter(&mut self, _owner: OrdnanceOwner, _channel: f64) -> Shooter { unimplemented!() }
        fn missile_target(&mut self, _target_id: f64, _target_generation: f64) -> Option<Vec3> { unimplemented!() }
        fn navigate_missile(&mut self, _position: Vec3, _velocity: &mut Vec3, _flight_class: Option<StrId>, _target_id: f64, _target_generation: f64, _dt: f64) -> f64 { unimplemented!() }
        fn water_crossing(&mut self, _from: Vec3, _to: Vec3) -> Option<Vec3> { unimplemented!() }
        fn waver(&mut self) -> f64 { unimplemented!() }
        fn spin(&mut self) -> f64 { unimplemented!() }
        fn explosion(&mut self, _def: &Field, _position: Vec3, _owner: OrdnanceOwner) { unimplemented!() }
        fn hit(&mut self, _body: Ref<Body>, _def: DefArg, _owner: OrdnanceOwner, _direction: Vec3) { unimplemented!() }
        fn hit_unit(&mut self, _unit: Ref<Unit>, _def: DefArg, _point: Point, _owner: OrdnanceOwner, _part: Nullable<HitPart>, _direction: Vec3) { unimplemented!() }
        fn projectile(&mut self, _shot: Shot, _kind: ShotKind) { unimplemented!() }
        fn launch(&mut self, _bolt: Ref<Bolt>, _tick: Nullable<f64>) { unimplemented!() }
        fn impact(&mut self, _shot: Shot, _info: &ImpactInfo) { unimplemented!() }
        fn grenade_contact(&mut self, _grenade: Ref<Grenade>) { unimplemented!() }
        fn bounce(&mut self, _def: DefId, _position: Vec3, _speed: f64) { unimplemented!() }
        fn blocked(&mut self, _position: Vec3, _target: Vec3) -> bool { unimplemented!() }
        fn drop_roll(&mut self) -> f64 { unimplemented!() }
        fn explode(&mut self, _def: &Field, _position: Vec3, _owner: ItemOwner) { unimplemented!() }
        fn watch(&mut self, _index: usize, _team: f64) -> Watched { unimplemented!() }
        fn standing(&mut self, _index: usize) -> Vec3 { unimplemented!() }
        fn give(&mut self, _index: usize, _def: DefId) -> bool { unimplemented!() }
        fn pickup(&mut self, _def: DefId, _position: Vec3) { unimplemented!() }
    }

    fn stand(soldier: bool, deck: bool) -> Stand {
        let mut store = Store::default();
        let soldier = soldier.then(|| store.unit.insert(Unit::default()));
        let deck = deck.then(|| {
            let body = Ref::<Body>::new(7, 1);
            let mut identity = [0.0; 16];
            for i in [0, 5, 10, 15] { identity[i] = 1.0; }
            let hull = store.vehicle.insert(Vehicle { body, matrix: identity, target_generation: 1.0, alive: true, ..Default::default() });
            (body, hull)
        });
        let num = |x: f64| Field::Num(x);
        let item = |base: &str, life_span: Field| ItemDef {
            base: Field::Str(base.into()), gravity: num(1.0), rebound: num(0.0), friction: num(1.0),
            life_span, max_health: num(200.0), health_type: Field::Undefined,
            explosion_death: Field::Undefined, explosion_trigger: Field::Undefined, explosion_expire: Field::Undefined,
            trigger_radius: Field::Undefined, suppress_radius: Field::Undefined,
        };
        let defs = Defs { item: vec![item("mine", Field::Undefined), item("mine", num(5.0)), item("powerupitem", num(30.0))], ..Default::default() };
        Stand { store, strings: Strings::default(), defs, soldier, deck, creature: false }
    }

    /// An item of `def` thrown by `owner` from `from` at `velocity`, flown
    /// for two seconds.
    fn throw_as(w: &mut Stand, def: DefId, owner: ItemOwner, from: Vec3, velocity: Vec3) -> Ref<Item> {
        let mut cx = Cx::new(w);
        let item = cx.spawn(def, from, velocity, 0.0, owner, 1.0).expect("room for an item");
        for _ in 0..120 { cx.items_update(1.0 / 60.0, 0); }
        item
    }
    fn throw(w: &mut Stand, from: Vec3, velocity: Vec3) -> Ref<Item> { throw_as(w, MINE, ItemOwner::Null, from, velocity) }
    fn at(w: &Stand, item: Ref<Item>) -> &Item { w.store.item.get(item).unwrap() }

    #[test]
    fn a_mine_bounces_off_a_soldier() {
        // Thrown level at 4 m/s from 1.5 m up, it would land 2.2 m away,
        // beyond the soldier; it meets him at 1.1 m and drops at his feet.
        let mut w = stand(false, false);
        let mine = throw(&mut w, [0.0, 1.5, 0.0], [4.0, 0.0, 0.0]);
        assert!(at(&w, mine).position[0] > 2.1 && at(&w, mine).position[0] < 2.3, "{:?}", at(&w, mine).position);
        let mut w = stand(true, false);
        let mine = throw(&mut w, [0.0, 1.5, 0.0], [4.0, 0.0, 0.0]);
        assert!(at(&w, mine).landed && at(&w, mine).position[0] > 1.0 && at(&w, mine).position[0] < 1.2, "{:?}", at(&w, mine).position);
        // Thrown from inside a body (its thrower's), it leaves it, even
        // heading down and on through the body's middle.
        let mut w = stand(true, false);
        let mine = throw(&mut w, [2.1, 1.5, 0.0], [4.0, 0.0, 0.0]);
        assert!(at(&w, mine).position[0] > 4.2 && at(&w, mine).position[0] < 4.4, "{:?}", at(&w, mine).position);
        let mut w = stand(true, false);
        let mine = throw(&mut w, [1.6, 1.5, 0.0], [4.0, -1.0, 0.0]);
        assert!(at(&w, mine).landed && at(&w, mine).position[0] > 3.0, "{:?}", at(&w, mine).position);
        // A power-up passes him.
        let mut w = stand(true, false);
        let powerup = throw_as(&mut w, POWERUP, ItemOwner::Null, [0.0, 1.5, 0.0], [4.0, 0.0, 0.0]);
        assert!(at(&w, powerup).position[0] > 2.1 && at(&w, powerup).position[0] < 2.3, "{:?}", at(&w, powerup).position);
    }

    #[test]
    fn its_thrower_is_left_out_only_while_a_time_bomb_is_young() {
        // A land mine meets even its thrower; a time bomb not in its first 0.5 s.
        let mut w = stand(true, false);
        let thrower = ItemOwner::Unit(w.soldier.unwrap());
        let mine = throw_as(&mut w, MINE, thrower, [0.0, 1.5, 0.0], [4.0, 0.0, 0.0]);
        assert!(at(&w, mine).position[0] > 1.0 && at(&w, mine).position[0] < 1.2, "{:?}", at(&w, mine).position);
        let bomb = throw_as(&mut w, TIME_BOMB, thrower, [0.0, 1.5, 3.0], [4.0, 0.0, 0.0]);
        let bomb_at = at(&w, bomb).position;
        let mut w = stand(true, false);
        let thrower = ItemOwner::Unit(w.soldier.unwrap());
        let young = throw_as(&mut w, TIME_BOMB, thrower, [0.0, 1.5, 0.0], [4.0, 0.0, 0.0]);
        assert!(at(&w, young).position[0] > 2.1 && at(&w, young).position[0] < 2.3, "{:?}", at(&w, young).position);
        assert!(bomb_at[0] > 2.1, "{bomb_at:?}");
    }

    #[test]
    fn a_mine_bounces_off_a_ridden_animal() {
        let mut w = stand(false, true);
        w.creature = true;
        let mine = throw(&mut w, [3.0, 2.0, 0.0], [1.0, 0.0, 0.0]);
        let item = at(&w, mine);
        assert!(item.carrier.is_none() && !item.landed && item.position[1] > 1.0 && item.position[1] < 1.1, "{:?}", item.position);
    }

    #[test]
    fn a_characters_fifth_mine_pops_his_first_after_a_death() {
        // One character's list, named the same for each of his lives: four
        // mines out (MaxItems 4), a death, and the new life's first throw.
        let mut w = stand(false, false);
        let key = w.strings.intern_str("mines:character:3:1");
        let mut cx = Cx::new(&mut w);
        let (mut work, mut spare) = (Vec::new(), Vec::new());
        let mines: Vec<_> = (0..4)
            .map(|n| cx.dispense_prepared(key, &mut work, &mut spare, 4.0, MINE, [n as f64, 1.5, 0.0], [0.0; 3], 0.0, ItemOwner::Null, 1.0).unwrap())
            .collect();
        let (mut work, mut spare) = (Vec::new(), Vec::new());
        let fifth = cx.dispense_prepared(key, &mut work, &mut spare, 4.0, MINE, [5.0, 1.5, 0.0], [0.0; 3], 0.0, ItemOwner::Null, 1.0).unwrap();
        assert_eq!(cx.s().item.get(mines[0]).unwrap().gone, Some(true));
        assert!(mines[1..].iter().chain([&fifth]).all(|&m| cx.s().item.get(m).unwrap().gone != Some(true)));
        assert_eq!(cx.s().items.owned.get(key).unwrap().as_slice(), &[mines[1], mines[2], mines[3], fifth]);
    }

    #[test]
    fn a_mine_rides_the_hull_it_sticks_to() {
        let mut w = stand(false, true);
        let mine = throw(&mut w, [3.0, 2.0, 0.0], [1.0, 0.0, 0.0]);
        let (_, hull) = w.deck.unwrap();
        let stuck = at(&w, mine).position;
        assert_eq!(at(&w, mine).carrier, Some(hull));
        assert!(stuck[1] > 1.0 && stuck[1] < 1.2, "{stuck:?}");
        // The hull drives 5 m on and turns a quarter turn about its origin:
        // the mine keeps its place on the deck.
        let v = w.store.vehicle.get_mut(hull).unwrap();
        v.matrix = [0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 5.0, 0.0, 0.0, 1.0];
        Cx::new(&mut w).items_update(1.0 / 60.0, 0);
        let rode = at(&w, mine);
        let want = [5.0 + stuck[2], stuck[1], -stuck[0]];
        assert!(rode.landed && distance_to(rode.position, want) < 1e-9, "{:?} {want:?}", rode.position);
        // Its slot lives again elsewhere: the mine falls to the floor.
        w.store.vehicle.get_mut(hull).unwrap().target_generation = 2.0;
        for _ in 0..60 { Cx::new(&mut w).items_update(1.0 / 60.0, 0); }
        let fell = at(&w, mine);
        assert!(fell.landed && fell.carrier.is_none() && fell.position[1] < 0.1, "{:?}", fell.position);
    }

    #[test]
    fn a_mine_falls_when_its_hull_is_gone_or_dies() {
        // The hull's row removed from the store.
        let mut w = stand(false, true);
        let mine = throw(&mut w, [3.0, 2.0, 0.0], [1.0, 0.0, 0.0]);
        let (_, hull) = w.deck.unwrap();
        w.store.vehicle.remove(hull);
        for _ in 0..60 { Cx::new(&mut w).items_update(1.0 / 60.0, 0); }
        assert!(at(&w, mine).carrier.is_none() && at(&w, mine).position[1] < 0.1, "{:?}", at(&w, mine).position);
        // The hull dies while driving at 12 m/s: the mine goes on at that
        // speed as it falls.
        let mut w = stand(false, true);
        let mine = throw(&mut w, [3.0, 2.0, 0.0], [1.0, 0.0, 0.0]);
        let (_, hull) = w.deck.unwrap();
        let stuck = at(&w, mine).position;
        w.store.vehicle.get_mut(hull).unwrap().matrix[12] = 0.2;
        Cx::new(&mut w).items_update(1.0 / 60.0, 0);
        w.store.vehicle.get_mut(hull).unwrap().alive = false;
        Cx::new(&mut w).items_update(1.0 / 60.0, 0);
        let freed = at(&w, mine);
        assert!(freed.carrier.is_none() && !freed.landed, "{:?}", freed.position);
        assert!((freed.velocity[0] - 12.0).abs() < 1e-6, "{:?}", freed.velocity);
        for _ in 0..60 { Cx::new(&mut w).items_update(1.0 / 60.0, 0); }
        assert!(at(&w, mine).position[0] > stuck[0] + 3.0, "{:?}", at(&w, mine).position);
    }
}
