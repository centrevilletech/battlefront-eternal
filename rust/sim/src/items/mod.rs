//! The things lying about (src/items.js), in Rust: mines and power-ups,
//! thrown by dispensers, dropped by the dead, placed by the world. Each
//! function transliterates the JavaScript method it names and reaches the
//! rest of the battle through the ordnance's [`World`] (its items' part,
//! ItemsWorld in src/ordnance-world.js), as rust/sim/src/ordnance does.
//!
//! An item taken or set off is marked gone and stays in the list until the
//! next update's filter, as the JavaScript's does (a dispenser's count and
//! a blast's loop see it until then); the filter retires it.

// The module's exports (WebAssembly only).
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

    /// items.js spawn: a new item, unless the world has no room for its
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
        };
        let r = self.s().item.insert(item);
        self.place(r);
        self.s().items.items.push(r);
        self.w.item_changed(r,true);
        Some(r)
    }

    /// items.js dispense: a dispenser's throw; an owner already with
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

    /// items.js dropFor: a soldier's drop at death, one roll down the
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

    /// items.js destroy: gone without a blast (a mine pops).
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

    /// items.js remove.
    fn remove_item(&mut self, r: Ref<Item>) {
        self.item(r).gone = Some(true);
        self.w.item_changed(r,false);
    }

    /// items.js expire: the end of an item's LifeSpan (a time bomb goes off).
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

    /// items.js hurt: damage to one item (a mine at 0 health pops).
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

    /// items.js blast: an explosion's damage to the mines lying about.
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

    /// items.js update: each item ages and flies, a mine watches for an
    /// enemy, a power-up for a taker (`targets` of them, the world's), then
    /// the list keeps the items not gone and each dispenser's list its own.
    pub fn items_update(&mut self, dt: f64, targets: usize) {
        let mut at = 0;
        while at < self.s().items.items.len() {
            let r = self.s().items.items[at];
            at += 1;
            let (gone, life, y) = {
                let i = self.item(r);
                if i.gone == Some(true) {
                    (true, 0.0, 0.0)
                } else {
                    i.age += dt;
                    i.life -= dt;
                    (false, i.life, i.position[1])
                }
            };
            if gone {
                continue;
            }
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

    /// items.js fly: gravity in two half steps around the move.
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

    /// items.js contact: what the item meets between where it was and where
    /// it is.
    fn contact(&mut self, r: Ref<Item>) {
        let (position, previous, def) = {
            let i = self.item(r);
            (i.position, i.previous, i.def)
        };
        let ray = sub(position, previous);
        let length = crate::ordnance::math::length(ray);
        if length < 1e-6 {
            return;
        }
        let ray = divide(ray, length);
        let start = add_scaled(previous, ray, -0.05);
        let Some(hit) = self.w.item_cast(start, ray, length + 0.1) else { return };
        let n = hit.normal;
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
            return;
        }
        if d > -HARD {
            i.position = hit.point;
            settle(i, n);
            return;
        }
        i.position = add_scaled(hit.point, n, 0.02);
        let mut v = add_scaled(v, n, -d);
        let along = crate::ordnance::math::length(v);
        if along > 1e-6 {
            v = scale(v, 1.0 - js_min((friction * -d * (1.0 + rebound)) / along, 1.0));
        }
        i.velocity = add_scaled(v, n, rebound * -d);
    }

    /// items.js place: the model upright on its surface, facing its way.
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

    /// items.js watch: a mine, set off by an enemy near enough and no longer
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

    /// items.js offer: a power-up, taken by the first soldier touching it
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

/// items.js settle: landed, upright on what it touches.
fn settle(i: &mut Item, normal: Vec3) {
    i.landed = true;
    i.sinking = false;
    i.velocity = [0.0, 0.0, 0.0];
    i.up = normal;
}
