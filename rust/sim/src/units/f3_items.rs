//! Items.spawn/dispense over prepared classes and lifecycle identity keys.
//! The generated Items map remains canonical; JS projects its private stable
//! keys into the original first-use owner numbering after the outer return.
//! Construction never inserts a gameplay ownership entry. Mine policy can
//! number an owner before its first throw, so those numbers belong to JS.
//! Native keys name the character playing the body (the body's own life
//! when nobody plays it), so one list follows a character across bodies.
use crate::gen::*;
use crate::ordnance::world::{ItemModel, PreparedItem};
use crate::store::{DefId, Ref, StrId};

const ITEM_ROOM: usize = 1024;
/// A sphere's centre and radius.
type Sphere = ([f64; 3], f64);
// A weapon level has one owned list and one invocation's work buffer. The
// second buffer supplies the first Map insertion without allocating on fire.
struct Binding {
    weapon: DefId,
    level: u32,
    name: StrId,
    life: f64,
    key: StrId,
    work: Vec<Ref<Item>>,
    spare: Vec<Ref<Item>>,
}
pub(crate) struct Frame {
    unit: u32,
    index: usize,
    pub key: StrId,
    pub work: Vec<Ref<Item>>,
    pub spare: Vec<Ref<Item>>,
}
#[derive(Clone, Copy, Default)]
struct Powerup { health: [f64;5], ammo: [f64;5] }
#[derive(Clone, Copy, Default)]
struct Supply { add_health: f64, health_effect: StrId, ammo_effect: StrId }
pub(crate) struct Items {
    classes: Vec<Option<PreparedItem>>,
    powerups: Vec<Powerup>,
    supplies: Vec<Supply>,
    // Items.update keeps its caller's target order through recursive mine
    // blasts. Only identities cross; geometry and supplies stay canonical.
    targets: Vec<F3ItemTarget>,
    drops: Vec<std::rc::Rc<Vec<crate::items::Drop>>>,
    explosions: Vec<Option<crate::store::DefId>>,
    bindings: Vec<Vec<Binding>>,
    // Logical Map presence differs from retained buffer storage. A failed
    // throw inserts an empty entry; Items.update deletes it on the next tick.
    live_keys: Vec<bool>,
    // A deleted current-life entry may reactivate. A drained old-life entry
    // never can, even though its pooled Unit still has the same slab slot.
    current_keys: Vec<bool>,
    notice: F3DispenserNotice,
}
impl Items {
    pub fn new() -> Self {
        // Changed lists are copied into this notice before publication. Its
        // capacity belongs to the battle, rather than to each emitted event.
        let mut notice = F3DispenserNotice::default();
        notice.items.reserve(ITEM_ROOM);
        Self {
            classes: Vec::new(),
            powerups: Vec::new(), supplies: Vec::new(), targets: Vec::with_capacity(ITEM_ROOM),
            drops: Vec::new(),
            explosions: Vec::new(),
            bindings: Vec::new(),
            live_keys: Vec::new(),
            current_keys: Vec::new(),
            notice,
        }
    }
    pub fn reserve_unit(&mut self, count: usize) {
        // Adoption may extend the slot table; ticking only indexes it.
        self.bindings.resize_with(count, Vec::new);
    }
    pub fn class(&self, def: DefId) -> PreparedItem {
        // F1 resolves definitions before firing, so a pool scan needs only
        // this numeric handle and never the authored class or base text.
        self.classes[def.0 as usize].expect("prepared item class")
    }
    /// Items.dropFor resolves side-local class names once. Missing classes
    /// stay in the roll: choosing one ends it with no item, not another draw.
    pub fn unit_items(&mut self, strings: &crate::store::Strings, row: F3UnitItems) {
        let at = row.class.0 as usize;
        if self.drops.len() <= at {
            self.drops.resize_with(at + 1, || std::rc::Rc::new(Vec::new()));
            self.explosions.resize(at + 1, None);
            self.supplies.resize(at + 1, Supply::default());
        }
        self.supplies[at] = Supply {add_health:row.add_health,health_effect:row.health_effect,ammo_effect:row.ammo_effect};
        self.explosions[at] = row.explosion.map(DefId);
        let mut drops = Vec::with_capacity(row.drops.len());
        for entry in row.drops {
            drops.push(crate::items::Drop {
                def: entry.def,
                probability: crate::ordnance::module::field(
                    entry.probability_tag as f64,
                    entry.probability,
                    strings,
                ),
            });
        }
        self.drops[at] = std::rc::Rc::new(drops);
    }
    pub fn drops(&self, class: DefId) -> std::rc::Rc<Vec<crate::items::Drop>> {
        std::rc::Rc::clone(&self.drops[class.0 as usize])
    }
    /// Each unit class's death explosion (`classDef.explosion`), by class.
    pub fn class_explosions(&self) -> impl Iterator<Item = (DefId, DefId)> + '_ {
        self.explosions.iter().enumerate().filter_map(|(class, explosion)| explosion.map(|e| (DefId(class as u32), e)))
    }
    pub fn explosion(&self, class: DefId) -> Option<DefId> {
        self.explosions[class.0 as usize]
    }
    pub fn prepare(&mut self, definitions: &crate::ordnance::defs::Defs, row: F3ItemClass) {
        let at = row.def.0 as usize;
        if self.classes.len() <= at {
            self.classes.resize(at + 1, None);
            self.powerups.resize(at + 1, Powerup::default());
        }
        self.powerups[at] = Powerup {health:row.health_gain,ammo:row.ammo_gain};
        // healthScale lowercases the authored health type. Resolve its index
        // once here, rather than allocate a String for every mine in a blast.
        // No hook runs during preparation; this immutable definition borrow
        // ends before construction can register another class.
        let definition = definitions.item(row.def);
        let health = match &definition.health_type {
            crate::ordnance::defs::Field::Str(name) => name.to_lowercase(),
            _ => String::new(),
        };
        let scale = crate::ordnance::defs::HEALTH_TYPES
            .iter()
            .position(|&name| name == health)
            .unwrap_or(0);
        self.classes[at] = Some(PreparedItem {
            base: row.base,
            pool: row.pool,
            mine: row.mine,
            life: row.life,
            health: row.health,
            scale,
            model: ItemModel {
                mesh: row.model.mesh,
                radius: row.model.radius,
                center: row.model.center,
            },
        });
    }
    /// A new arsenal life prepares its names and two buffers outside ticking.
    /// Inserting an empty gameplay key here would change Map insertion order.
    pub fn bind(&mut self, sim: &mut crate::abi::Sim, row: F3DispenserBinding) {
        // Class and level identify the native service; name retains the
        // external Items.dispense command's original weapon-class key.
        let list = &mut self.bindings[row.key.slot as usize];
        let at = list
            .iter()
            .position(|b| b.weapon == row.weapon && b.level == row.level);
        let binding = match at {
            Some(at) => &mut list[at],
            None => {
                list.push(Binding {
                    weapon: row.weapon,
                    level: row.level,
                    name: row.name,
                    life: row.key.life,
                    key: row.owned_key,
                    work: Vec::with_capacity(ITEM_ROOM),
                    spare: Vec::with_capacity(ITEM_ROOM),
                });
                list.last_mut().unwrap()
            }
        };
        let replaced = (binding.key != row.owned_key).then_some(binding.key);
        binding.life = row.key.life;
        binding.name = row.name;
        binding.key = row.owned_key;
        binding
            .work
            .reserve(ITEM_ROOM.saturating_sub(binding.work.len()));
        binding
            .spare
            .reserve(ITEM_ROOM.saturating_sub(binding.spare.len()));
        // Two bodies can name one character's list (his old body and the
        // one he lives in now): the old key stops being current only when no
        // body names it any more. Keep live old-life mines in their existing
        // insertion position; only an already drained tombstone can go.
        if let Some(old) = replaced {
            if !self.bindings.iter().flatten().any(|b| b.key == old) {
                self.current_keys[old.0 as usize] = false;
                if !self.live_keys[old.0 as usize] {
                    sim.store.items.owned.remove(old);
                }
            }
        }
        let count = row.owned_key.0 as usize + 1;
        if self.live_keys.len() < count {
            self.live_keys.resize(count, false);
            self.current_keys.resize(count, false);
        }
        self.current_keys[row.owned_key.0 as usize] = true;
    }
    /// AI class jobs consume the same prepared owner keys as dispensing.
    pub fn owned_key(&self, unit: u32, life: f64, weapon: DefId, level: u32) -> Option<crate::store::StrId> {
        self.bindings.get(unit as usize)?.iter().find(|b| b.life == life && b.weapon == weapon && b.level == level).map(|b| b.key)
    }
    pub fn take(&mut self, unit: u32, life: f64, weapon: DefId, level: u32) -> Frame {
        // CombatWorld.dispense borrows its buffers out before committing the
        // shooter. Recursive mine damage can then read canonical state.
        let index = self.bindings[unit as usize]
            .iter()
            .position(|b| b.weapon == weapon && b.level == level)
            .expect("prepared dispenser");
        self.take_at(unit, life, index)
    }
    pub fn take_name(&mut self, unit: u32, life: f64, name: StrId) -> Frame {
        // The between-phase JS command uses an interned name, not a freshly
        // formatted owner key. Its gameplay life must still match.
        let index = self.bindings[unit as usize]
            .iter()
            .position(|b| b.name == name && b.life == life)
            .expect("prepared dispenser name");
        self.take_at(unit, life, index)
    }
    fn take_at(&mut self, unit: u32, life: f64, index: usize) -> Frame {
        let binding = &mut self.bindings[unit as usize][index];
        assert_eq!(binding.life, life, "prepared dispenser life");
        // Moving Vec ownership leaves no sidecar borrow across F1. Native
        // death chains cannot start another throw from this same weapon.
        Frame {
            unit,
            index,
            key: binding.key,
            work: std::mem::take(&mut binding.work),
            spare: std::mem::take(&mut binding.spare),
        }
    }
    pub fn restore(&mut self, frame: Frame) {
        // The displaced Map list returns as next throw's scratch; its old
        // elements are cleared only when that throw begins filtering.
        let binding = &mut self.bindings[frame.unit as usize][frame.index];
        binding.work = frame.work;
        binding.spare = frame.spare;
    }
    pub fn active(&self, key: StrId) -> bool {
        self.live_keys[key.0 as usize]
    }
    // A current life can throw again after its list empties. Keep that Vec;
    // an old life's drained key can never reactivate and releases its storage.
    pub fn current(&self, key: StrId) -> bool {
        self.current_keys[key.0 as usize]
    }
    pub fn changed_in(&mut self, sim: &mut crate::abi::Sim, key: StrId, removed: bool) {
        // Repeated empty updates must not repeatedly delete the JS key.
        // A real reactivation publishes an insertion even when it is empty.
        let active = &mut self.live_keys[key.0 as usize];
        if removed && !*active {
            return;
        }
        *active = !removed;
        self.notice.key = key;
        self.notice.removed = removed;
        self.notice.items.clear();
        if let Some(list) = sim.store.items.owned.get(key) {
            self.notice.items.extend_from_slice(list);
        }
        self.notice.emit(&mut sim.events);
    }
}

pub fn f3_item_class_in(world_ctx: &mut crate::world::owner::World) {
    // ItemsSim.defId publishes one immutable class with its measured model.
    let (row, heap) = world_ctx.sim.calls.args();
    super::f3::items_in(&mut world_ctx.units).prepare(crate::ordnance::module::defs_in(&mut world_ctx.ordnance), F3ItemClass::take(row, heap));
}

pub fn f3_unit_items_in(world_ctx: &mut crate::world::owner::World) {
    // Game.onDeath's side-local drop names are resolved at construction.
    let (row, heap) = world_ctx.sim.calls.args();
    super::f3::items_in(&mut world_ctx.units).unit_items(&world_ctx.sim.strings, F3UnitItems::take(row, heap));
}

pub fn f3_dispenser_binding_in(world_ctx: &mut crate::world::owner::World) -> u32 {
    // Reset replaces arsenal identity. Refuse a binding for a previous life
    // before it could change the current life's buffers or private key.
    let (row, heap) = world_ctx.sim.calls.args();
    let input = F3DispenserBinding::take(row, heap);
    if input.key.validate(&world_ctx.sim.store).is_err() {
        return 0;
    }
    super::f3::items_in(&mut world_ctx.units).bind(&mut world_ctx.sim, input);
    1
}

/// Item snapshots carry values from this mutation, not views into a row that
/// may have moved, expired or reused its slot before the event is delivered.
pub(crate) fn changed_in(sim: &mut crate::abi::Sim, item: Ref<Item>, made: bool) {
    let row = sim.store.item.get(item).expect("changed item");
    let state = F3ItemState {
        def: row.def,
        mine: row.mine,
        position: row.position,
        previous: row.previous,
        velocity: row.velocity,
        yaw: row.yaw,
        up: row.up,
        landed: row.landed,
        sinking: row.sinking,
        radius: row.radius,
        center: row.center,
        turn: row.turn,
        owner: row.owner,
        team: row.team,
        age: row.age,
        life: row.life,
        health: row.health,
        gone: row.gone,
    };
    F3ItemNotice { item, state, made }.emit(&mut sim.events);
}
pub(crate) fn retired_in(sim: &mut crate::abi::Sim, item: Ref<Item>) {
    // A raw qualified key does not ask JS to construct a second terminal
    // mirror. The last snapshot was emitted before this slot was removed.
    F3ItemRetired {
        slot: item.slot,
        generation: item.gen,
    }
    .emit(&mut sim.events);
}

// ItemsWorld.targets are stable objects selected at the original mineTargets
// slot. Snapshot identities before a mine can reuse the foreign input buffer;
// positions and supplies remain reads of the current canonical rows.
pub(crate) fn targets_in(world_ctx: &mut crate::world::owner::World, count: u32) {
    super::f3::items_in(&mut world_ctx.units).targets.clear();
    for index in 0..count as usize {
        let row = super::f3::item_target_input_in(&world_ctx.units, index);
        if let Some(r) = row.unit {
            assert_eq!(world_ctx.sim.store.unit.get(r).expect("current pickup body").generation,row.life);
        } else {
            let r = row.hull.expect("pickup hull or unit");
            assert_eq!(world_ctx.sim.store.vehicle.get(r).expect("current pickup hull").target_generation,row.life);
        }
        super::f3::items_in(&mut world_ctx.units).targets.push(row);
    }
}

/// Game.step's mineTargets selection runs after all projectile hits. Keep
/// local, factory and fleet order, including the local body if also built.
/// Capacity bounds exceed authored battle populations; excess membership
/// drops at the end without reallocating the retained target bank.
pub(crate) fn world_targets_in(world_ctx: &mut crate::world::owner::World) -> Option<u32> {
    let capacity = super::f3::items_in(&mut world_ctx.units).targets.capacity();
    let mut count = 0;
    let local = super::f3::registered_viewpoint_in(&world_ctx.units)?;
    let unit = world_ctx.sim.store.unit.get(local)?;
    if !unit.combat.dead && unit.seat.is_none() {
        let row = F3ItemTarget {unit:Some(local),hull:None,life:unit.generation};
        super::f3::world_item_target_in(&mut world_ctx.units, count,&row);count += 1;
    }
    for index in 0..super::f3::built_in(&world_ctx.units).len() {
        let reference = super::f3::built_in(&world_ctx.units)[index];
        let Some(unit) = world_ctx.sim.store.unit.get(reference) else {continue};
        if !unit.alive || unit.seat.is_some() || matches!(unit.held,crate::store::Nullable::Value(_)) {continue;}
        if count == capacity {break;}
        let row = F3ItemTarget {unit:Some(reference),hull:None,life:unit.generation};
        super::f3::world_item_target_in(&mut world_ctx.units, count,&row);count += 1;
    }
    for index in 0..world_ctx.sim.store.vehicles.vehicles.len() {
        let reference = world_ctx.sim.store.vehicles.vehicles[index];
        let Some(hull) = world_ctx.sim.store.vehicle.get(reference) else {continue};
        if !hull.alive {continue;}
        if count == capacity {break;}
        let row = F3ItemTarget {unit:None,hull:Some(reference),life:hull.target_generation};
        super::f3::world_item_target_in(&mut world_ctx.units, count,&row);count += 1;
    }
    Some(count as u32)
}

// UnitFactory.itemTarget and Vehicle.itemTarget read current coordinates,
// including movement caused by an earlier mine in this same ordered list.
pub(crate) fn standing_in(world_ctx: &mut crate::world::owner::World, index: usize) -> [f64;3] {
    let row = &super::f3::items_in(&mut world_ctx.units).targets[index];
    if let Some(r) = row.unit {
        let p = world_ctx.sim.store.unit.get(r).unwrap().position();
        [p.x,p.y,p.z]
    } else { world_ctx.sim.store.vehicle.get(row.hull.unwrap()).unwrap().position }
}
// A mine's trigger measures everyone to the sphere a blast measures him to,
// as the 2004 game's do: a hull's about its model, turned with it.
pub(crate) fn watch_in(world_ctx: &mut crate::world::owner::World, index: usize, team: f64) -> crate::ordnance::world::Watched {
    let (unit, hull) = {
        let row = &super::f3::items_in(&mut world_ctx.units).targets[index];
        (row.unit, row.hull)
    };
    if let Some(r) = unit {
        let class = world_ctx.sim.store.unit.get(r).unwrap().class_def;
        // A droideka's sphere is the one its blasts measure it to.
        let model = crate::combat::module::defs_in(&mut world_ctx.combat).model_sphere(class);
        let unit = world_ctx.sim.store.unit.get(r).unwrap();
        let v = unit.velocity();
        let (center, radius) = unit_sphere(unit, model);
        crate::ordnance::world::Watched {relation:super::f3::relation_in(&world_ctx.units, unit.team,team),
            center,radius,velocity:[v.x,v.y,v.z]}
    } else {
        let hull = hull.unwrap();
        let (center, radius) = super::f3_hulls::sphere_in(world_ctx, hull, true);
        let v = world_ctx.sim.store.vehicle.get(hull).unwrap();
        crate::ordnance::world::Watched {relation:super::f3::relation_in(&world_ctx.units, v.team.unwrap_or(0.0),team),
            center:[center.x,center.y,center.z],radius,velocity:v.velocity}
    }
}

/// A unit's sphere for a mine's trigger, its centre and radius: a soldier's
/// by stance (standing while in the air); a droideka's about its model box
/// (`model`: its class's sphere, the one a blast measures it to), carried
/// with its body, standing or rolled, or the standing soldier's without one.
pub(crate) fn unit_sphere(unit: &Unit, model: Option<crate::combat::combat::ModelSphere>) -> Sphere {
    match &unit.soldier {
        UnitBody::Soldier(s) => {
            let stance = if s.air.is_some() { Posture::Stand } else { s.posture };
            let [middle, radius, _] = crate::combat::combat::blast_body(stance);
            ([s.position[0], s.position[1] + middle, s.position[2]], radius)
        }
        UnitBody::Droideka(d) => {
            let Some(model) = model else {
                let p = d.position;
                let [middle, radius, _] = crate::combat::combat::blast_body(Posture::Stand);
                return ([p[0], p[1] + middle, p[2]], radius);
            };
            (model.carried(d.position, d.aim_yaw), model.radius)
        }
    }
}

// A power-up writes health and reserve ammunition directly. Hull health borrows the existing native death services, preserving
// the original kill path even for a negative authored health amount.
pub(crate) fn give_in(world_ctx: &mut crate::world::owner::World, index: usize, definition: DefId, services: Option<super::f3::WorldServices<'_>>) -> bool {
    let row = &super::f3::items_in(&mut world_ctx.units).targets[index];
    let unit = row.unit; let hull = row.hull;
    let amounts = super::f3::items_in(&mut world_ctx.units).powerups[definition.0 as usize];
    if let Some(r) = unit {
        let scale = super::f3::health_scale_in(&world_ctx.units, r.slot) as usize;
        let unit = world_ctx.sim.store.unit.get_mut(r).unwrap();
        let profile = super::f3::items_in(&mut world_ctx.units).supplies[unit.class_def.0 as usize];
        let amount = amounts.health[scale];
        let health = amount != 0.0 && !amount.is_nan() && unit.combat.health < unit.combat.max_health && !(profile.add_health < 0.0);
        if health { unit.combat.health = v8math::min(unit.combat.health + amount,unit.combat.max_health); }
        let mut ammo = false;
        let amount = amounts.ammo[scale];
        if amount != 0.0 && !amount.is_nan() {
            let defs = crate::combat::module::defs_in(&mut world_ctx.combat);
            for weapon in &mut unit.combat.weapons {
                let level = weapon.level(defs.weapons.class(weapon.def));
                ammo |= crate::combat::weapons::supply(weapon,level,amount,true);
            }
        }
        let p = unit.position(); let position = [p.x,p.y,p.z];
        if health { F3PowerupEffect {name:profile.health_effect,position}.emit(&mut world_ctx.sim.events); }
        if ammo { F3PowerupEffect {name:profile.ammo_effect,position}.emit(&mut world_ctx.sim.events); }
        if health || ammo { super::f3::changed_in(world_ctx, r); }
        health || ammo
    } else {
        let r = hull.unwrap();
        let scale = super::f3_hulls::health_scale_in(&world_ctx.units, r) as usize;
        let v = world_ctx.sim.store.vehicle.get(r).unwrap();
        let amount = amounts.health[scale];
        let health = amount != 0.0 && !amount.is_nan() && v.health < v.max_health;
        let next = v8math::min(v.health + amount,v.max_health);
        if health { super::f3_hulls::mutate_in(world_ctx, r,next,None,services.expect("pickup hull world services")); }
        let amount = amounts.ammo[scale];
        let ammo = amount != 0.0 && !amount.is_nan() && crate::vehicles::module::f2::firing::supply_in(world_ctx, r,amount,true);
        // givePowerup can change only ammunition on an undamaged hull.
        // Its dirty mounted rows still need the shared outer publication.
        if ammo { crate::vehicles::module::f2::pending_in(&mut world_ctx.fleet); }
        health || ammo
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn soldier(posture: Posture, air: Option<Air>) -> Unit {
        Unit { soldier: UnitBody::Soldier(Soldier { position: [2.0, 5.0, -1.0], posture, air, ..Default::default() }), ..Default::default() }
    }

    /// The horizontal distance at which a mine lying 0.06 m up sets off on
    /// a unit whose sphere is `sphere` (TriggerRadius 1, the land mine's).
    fn reach(feet: [f64; 3], (center, radius): Sphere) -> f64 {
        let rise = center[1] - (feet[1] + 0.06);
        ((1.0 + radius) * (1.0 + radius) - rise * rise).sqrt()
    }

    #[test]
    fn a_characters_list_stays_current_while_he_lives_in_another_body() {
        let mut sim = crate::abi::Sim::new();
        let mut items = Items::new();
        items.reserve_unit(4);
        let his = sim.strings.intern_str("mines:character:5:1");
        let other = sim.strings.intern_str("mines:character:6:1");
        let row = |slot: u32, life: f64, key: StrId| F3DispenserBinding {
            key: F3UnitKey { slot, generation: 1, life }, weapon: DefId(2), level: 0, name: StrId(0), owned_key: key };
        // He dies in body 1 and lives again in body 2: both name his list.
        items.bind(&mut sim, row(1, 1.0, his));
        items.bind(&mut sim, row(2, 1.0, his));
        // Another character takes body 1: his list is still current.
        items.bind(&mut sim, row(1, 2.0, other));
        assert!(items.current(his) && items.current(other));
        // Body 2 goes to that other character too: now nobody names his list.
        items.bind(&mut sim, row(2, 2.0, other));
        assert!(!items.current(his));
        // A bot takes a stock body with no new life: its body list gives way
        // to his list, the one the host binds then.
        let stock = sim.strings.intern_str("mines:body:3:1:1");
        items.bind(&mut sim, row(3, 1.0, stock));
        items.bind(&mut sim, row(3, 1.0, his));
        assert!(items.current(his) && !items.current(stock));
        assert_eq!(items.take(3, 1.0, DefId(2), 0).key, his);
    }

    #[test]
    fn a_mine_measures_a_soldier_by_his_stance() {
        let feet = [2.0, 5.0, -1.0];
        let at = |posture, air| unit_sphere(&soldier(posture, air), None);
        assert_eq!(at(Posture::Stand, None), ([2.0, 6.0, -1.0], 1.4626));
        assert_eq!(at(Posture::Crouch, None), ([2.0, 5.75, -1.0], 1.1023));
        assert_eq!(at(Posture::Prone, None), ([2.0, 5.35, -1.0], 1.622));
        assert_eq!(at(Posture::Crouch, Some(Air::Jump)), at(Posture::Stand, None));
        // Before: one sphere 1.048 m up, radius 1.278, 2.05 m whatever the stance.
        for (posture, distance) in [(Posture::Stand, 2.276), (Posture::Crouch, 1.986), (Posture::Prone, 2.606)] {
            assert!((reach(feet, at(posture, None)) - distance).abs() < 5e-4, "{posture:?}");
        }
    }

    #[test]
    fn a_mine_measures_a_droideka_to_its_model_box_turned_with_it() {
        use crate::combat::defs::{Js, JsObj};
        use std::rc::Rc;
        // cis_inf_droideka's model box, as admission sends it with the class:
        // one sphere about it, read as the class is prepared, for its blasts
        // and for a mine's trigger alike.
        let list = |v: [f64; 3]| Js::Arr(Rc::new(v.iter().map(|x| Js::Num(*x)).collect()));
        let bounds = Js::Arr(Rc::new(vec![list([-0.809129, -0.000631, -0.841644]), list([0.801283, 2.091974, 1.436694])]));
        let class = |base: &str| Rc::new(JsObj { fields: vec![("base".into(), Js::Str(base.into())), ("weapons".into(), Js::Arr(Default::default())),
            ("bounds".into(), bounds.clone())], def: None });
        let mut defs = crate::combat::combat::CombatDefs::default();
        defs.set_class(DefId(0), class("walkerdroid"));
        defs.set_class(DefId(1), class("soldier"));
        let model = defs.model_sphere(DefId(0)).expect("a droideka's sphere");
        assert!(defs.model_sphere(DefId(1)).is_none());
        let half = |i: usize, low: f64, high: f64| (model.center[i] - (low + high) * 0.5).abs() < 1e-12;
        assert!(half(0, -0.809129, 0.801283) && half(1, -0.000631, 2.091974) && half(2, -0.841644, 1.436694), "{model:?}");
        assert!((model.radius - 1.7438).abs() < 1e-4, "{}", model.radius);
        // Turned a quarter round, its centre 0.3 m ahead lies along +x.
        let turn = std::f64::consts::FRAC_PI_2;
        let unit = Unit { soldier: UnitBody::Droideka(Droideka { position: [10.0, 0.0, 10.0], aim_yaw: turn, mode: 11.0, ..Default::default() }), ..Default::default() };
        let (center, radius) = unit_sphere(&unit, Some(model));
        let [x, y, z] = model.center;
        assert!((center[0] - (10.0 + z)).abs() < 1e-12 && center[1] == y && (center[2] - (10.0 - x)).abs() < 1e-12, "{center:?}");
        assert_eq!(radius, model.radius);
    }
}
