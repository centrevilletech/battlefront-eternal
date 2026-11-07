//! Items.spawn/dispense over prepared classes and lifecycle identity keys.
//! The generated Items map remains canonical; JS projects its private stable
//! keys into the original first-use owner numbering after the outer return.
//! Construction never inserts a gameplay ownership entry. Mine policy can
//! number an owner before its first throw, so those numbers belong to JS.
//! Native keys instead retain the pooled slot and the exact throwing life.
use crate::abi::sim;
use crate::gen::*;
use crate::ordnance::world::{ItemModel, PreparedItem};
use crate::store::{DefId, Ref, StrId};

const ITEM_ROOM: usize = 1024;
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
    drops: Vec<Vec<crate::items::Drop>>,
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
    pub fn unit_items(&mut self, row: F3UnitItems) {
        let at = row.class.0 as usize;
        if self.drops.len() <= at {
            self.drops.resize_with(at + 1, Vec::new);
            self.explosions.resize(at + 1, None);
            self.supplies.resize(at + 1, Supply::default());
        }
        self.supplies[at] = Supply {add_health:row.add_health,health_effect:row.health_effect,ammo_effect:row.ammo_effect};
        self.explosions[at] = row.explosion.map(DefId);
        let drops = &mut self.drops[at];
        drops.clear();
        for entry in row.drops {
            drops.push(crate::items::Drop {
                def: entry.def,
                probability: crate::ordnance::module::field(
                    entry.probability_tag as f64,
                    entry.probability,
                    &sim().strings,
                ),
            });
        }
    }
    pub fn drops(&self, class: DefId) -> &[crate::items::Drop] {
        &self.drops[class.0 as usize]
    }
    pub fn explosion(&self, class: DefId) -> Option<DefId> {
        self.explosions[class.0 as usize]
    }
    pub fn prepare(&mut self, row: F3ItemClass) {
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
        let definition = unsafe { &*crate::ordnance::module::defs() }.item(row.def);
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
    pub fn bind(&mut self, row: F3DispenserBinding) {
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
        if binding.key != row.owned_key {
            // Keep live old-life mines in their existing insertion position.
            // Only an already drained tombstone can disappear at rebind.
            self.current_keys[binding.key.0 as usize] = false;
            if !self.live_keys[binding.key.0 as usize] {
                sim().store.items.owned.remove(binding.key);
            }
        }
        binding.life = row.key.life;
        binding.name = row.name;
        binding.key = row.owned_key;
        binding
            .work
            .reserve(ITEM_ROOM.saturating_sub(binding.work.len()));
        binding
            .spare
            .reserve(ITEM_ROOM.saturating_sub(binding.spare.len()));
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
    pub fn changed(&mut self, key: StrId, removed: bool) {
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
        if let Some(list) = sim().store.items.owned.get(key) {
            self.notice.items.extend_from_slice(list);
        }
        self.notice.emit(&mut sim().events);
    }
}

#[no_mangle]
pub extern "C" fn f3_item_class() {
    // ItemsSim.defId publishes one immutable class with its measured model.
    let (row, heap) = sim().calls.args();
    super::f3::items().prepare(F3ItemClass::take(row, heap));
}
#[no_mangle]
pub extern "C" fn f3_unit_items() {
    // Game.onDeath's side-local drop names are resolved at construction.
    let (row, heap) = sim().calls.args();
    super::f3::items().unit_items(F3UnitItems::take(row, heap));
}
#[no_mangle]
pub extern "C" fn f3_dispenser_binding() -> u32 {
    // Reset replaces arsenal identity. Refuse a binding for a previous life
    // before it could change the current life's buffers or private key.
    let (row, heap) = sim().calls.args();
    let input = F3DispenserBinding::take(row, heap);
    if input.key.validate(&sim().store).is_err() {
        return 0;
    }
    super::f3::items().bind(input);
    1
}

/// Item snapshots carry values from this mutation, not views into a row that
/// may have moved, expired or reused its slot before the event is delivered.
pub(crate) fn changed(item: Ref<Item>, made: bool) {
    let row = sim().store.item.get(item).expect("changed item");
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
    F3ItemNotice { item, state, made }.emit(&mut sim().events);
}
pub(crate) fn retired(item: Ref<Item>) {
    // A raw qualified key does not ask JS to construct a second terminal
    // mirror. The last snapshot was emitted before this slot was removed.
    F3ItemRetired {
        slot: item.slot,
        generation: item.gen,
    }
    .emit(&mut sim().events);
}

// ItemsWorld.targets are stable objects selected at the original mineTargets
// slot. Snapshot identities before a mine can reuse the foreign input buffer;
// positions and supplies remain reads of the current canonical rows.
pub(crate) fn targets(count: u32) {
    let list = &mut super::f3::items().targets;
    list.clear();
    for index in 0..count as usize {
        let row = super::f3::item_target_input(index);
        if let Some(r) = row.unit {
            assert_eq!(sim().store.unit.get(r).expect("current pickup body").generation,row.life);
        } else {
            let r = row.hull.expect("pickup hull or unit");
            assert_eq!(sim().store.vehicle.get(r).expect("current pickup hull").target_generation,row.life);
        }
        list.push(row);
    }
}

/// Game.step's mineTargets selection runs after all projectile hits. Keep
/// local, factory and fleet order, including the local body if also built.
/// Capacity bounds exceed authored battle populations; excess membership
/// drops at the end without reallocating the retained target bank.
pub(crate) fn world_targets() -> u32 {
    let capacity = super::f3::items().targets.capacity();
    let mut count = 0;
    let local = super::f3::viewpoint();
    let unit = sim().store.unit.get(local).unwrap();
    if !unit.combat.dead && unit.seat.is_none() {
        let row = F3ItemTarget {unit:Some(local),hull:None,life:unit.generation};
        super::f3::world_item_target(count,&row);count += 1;
    }
    for index in 0..super::f3::built().len() {
        let reference = super::f3::built()[index];
        let Some(unit) = sim().store.unit.get(reference) else {continue};
        if !unit.alive || unit.seat.is_some() || matches!(unit.held,crate::store::Nullable::Value(_)) {continue;}
        if count == capacity {break;}
        let row = F3ItemTarget {unit:Some(reference),hull:None,life:unit.generation};
        super::f3::world_item_target(count,&row);count += 1;
    }
    for index in 0..sim().store.vehicles.vehicles.len() {
        let reference = sim().store.vehicles.vehicles[index];
        let Some(hull) = sim().store.vehicle.get(reference) else {continue};
        if !hull.alive {continue;}
        if count == capacity {break;}
        let row = F3ItemTarget {unit:None,hull:Some(reference),life:hull.target_generation};
        super::f3::world_item_target(count,&row);count += 1;
    }
    count as u32
}

// UnitFactory.itemTarget and Vehicle.itemTarget read current coordinates,
// including movement caused by an earlier mine in this same ordered list.
pub(crate) fn standing(index: usize) -> [f64;3] {
    let row = &super::f3::items().targets[index];
    if let Some(r) = row.unit {
        let p = sim().store.unit.get(r).unwrap().position();
        [p.x,p.y,p.z]
    } else { sim().store.vehicle.get(row.hull.unwrap()).unwrap().position }
}
pub(crate) fn watch(index: usize, team: f64) -> crate::ordnance::world::Watched {
    let row = &super::f3::items().targets[index];
    if let Some(r) = row.unit {
        let unit = sim().store.unit.get(r).unwrap();
        let p = unit.position(); let v = unit.velocity();
        crate::ordnance::world::Watched {relation:super::f3::relation(unit.team,team),
            center:[p.x,p.y+1.048,p.z],radius:1.278,velocity:[v.x,v.y,v.z]}
    } else {
        let v = sim().store.vehicle.get(row.hull.unwrap()).unwrap();
        crate::ordnance::world::Watched {relation:super::f3::relation(v.team.unwrap_or(0.0),team),
            center:v.item_center,radius:v8math::max(v.size[0],v.size[2])*0.5,velocity:v.velocity}
    }
}

// givePowerup (items.js) writes health and reserve ammunition without JS
// setters. Hull health borrows the existing native death services, preserving
// the original kill path even for a negative authored health amount.
pub(crate) fn give(index: usize, definition: DefId, services: Option<super::f3::WorldServices<'_>>) -> bool {
    let row = &super::f3::items().targets[index];
    let unit = row.unit; let hull = row.hull;
    let amounts = super::f3::items().powerups[definition.0 as usize];
    if let Some(r) = unit {
        let scale = super::f3::health_scale(r.slot) as usize;
        let unit = sim().store.unit.get_mut(r).unwrap();
        let profile = super::f3::items().supplies[unit.class_def.0 as usize];
        let amount = amounts.health[scale];
        let health = amount != 0.0 && !amount.is_nan() && unit.combat.health < unit.combat.max_health && !(profile.add_health < 0.0);
        if health { unit.combat.health = v8math::min(unit.combat.health + amount,unit.combat.max_health); }
        let mut ammo = false;
        let amount = amounts.ammo[scale];
        if amount != 0.0 && !amount.is_nan() {
            let defs = crate::combat::module::defs();
            for weapon in &mut unit.combat.weapons {
                let level = weapon.level(defs.weapons.class(weapon.def));
                ammo |= crate::combat::weapons::supply(weapon,level,amount,true);
            }
        }
        let p = unit.position(); let position = [p.x,p.y,p.z];
        if health { F3PowerupEffect {name:profile.health_effect,position}.emit(&mut sim().events); }
        if ammo { F3PowerupEffect {name:profile.ammo_effect,position}.emit(&mut sim().events); }
        if health || ammo { super::f3::changed(r); }
        health || ammo
    } else {
        let r = hull.unwrap();
        let scale = super::f3_hulls::health_scale(r) as usize;
        let v = sim().store.vehicle.get(r).unwrap();
        let amount = amounts.health[scale];
        let health = amount != 0.0 && !amount.is_nan() && v.health < v.max_health;
        let next = v8math::min(v.health + amount,v.max_health);
        if health { super::f3_hulls::mutate(r,next,None,services.expect("pickup hull world services")); }
        let amount = amounts.ammo[scale];
        let ammo = amount != 0.0 && !amount.is_nan() && crate::vehicles::module::f2::firing::supply(r,amount,true);
        // givePowerup can change only ammunition on an undamaged hull.
        // Its dirty mounted rows still need the shared outer publication.
        if ammo { crate::vehicles::module::f2::pending(); }
        health || ammo
    }
}
