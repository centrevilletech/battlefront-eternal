//! The rounds on the module: the exports JavaScript drives them through once
//! a battle's world hands them over (src/ordnance-sim.js), and the world
//! their Rust reaches the battle through there: the imports of
//! rust/sim/schema/ordnance.mjs, which src/ordnance-sim.js serves with the
//! same OrdnanceWorld (src/ordnance-world.js) the JavaScript rounds call.
//!
//! A physical adapter borrows the battle root. Cx reborrows its canonical
//! rows, strings and definitions through that adapter; a row loan ends before
//! a callback can retire it or start another physical operation. Commands and
//! standalone host imports retain their existing generated boundary blocks.
//!
//! A round handed to JavaScript (a projectile made, a bolt launched, a
//! round's end, a grenade's first touch) crosses with its row as the store
//! exports it, after the import's own words, so its mirror is the round as
//! it is at that moment: the capture's observers read it then, and a sim
//! event keeps a grenade's position.

use super::defs::{Defs, ExplosionDef, Field, OrdnanceDef, SCALE_FIELDS};
use super::world::*;
use super::Cx;
use crate::abi::Sim;
use crate::gen::*;
use crate::store::calls::{import_out, Calls};
use crate::store::{same_f64, word_u32, DefId, KindCode, Nullable, Ref, RefSet, StrId, Strings, Vec3};

/// Ordnance and Items retain independent water bits beside their definition
/// bank. A Cx borrows definitions only for its invocation; no sibling field
/// address becomes an identity or a retained link between owners.
pub(crate) struct OrdnanceOwnerState { defs: Option<Defs>, water: [bool; 2] }
impl OrdnanceOwnerState {
    pub(crate) const fn new() -> Self { Self { defs: None, water: [false; 2] } }
}

/// Whether the rounds' and the items' battle has water (ordnance.water,
/// items.water): without, a water crossing is null and nobody is asked.

fn water(owner: &OrdnanceOwnerState, which: usize) -> bool {
    owner.water[which]
}

/// The rounds' (0) or the items' (1) water, present or not.
#[no_mangle]
pub extern "C" fn ord_water(which: u32, present: u32) { ord_water_in(crate::abi::cold(), which, present) }

pub(crate) fn ord_water_in(w: &mut crate::world::owner::World, which: u32, present: u32) {
    w.ordnance.water[(which & 1) as usize] = present != 0;
}

pub(crate) fn defs_in(owner: &mut OrdnanceOwnerState) -> &mut Defs {
    owner.defs.get_or_insert_with(Defs::default)
}

/// A definition's field from its two words (src/ordnance-sim.js `fieldWords`):
/// 0 undefined, 1 null, 2 false, 3 true, 4 a number, 5 a string (its
/// index), 6 an explosion (its index), 7 any other object.
pub(crate) fn field(tag: f64, value: f64, strings: &Strings) -> Field {
    match word_u32(tag) {
        0 => Field::Undefined,
        1 => Field::Null,
        2 => Field::Bool(false),
        3 => Field::Bool(true),
        4 => Field::Num(value),
        5 => Field::Str(String::from_utf16_lossy(strings.units(StrId(word_u32(value))))),
        6 => Field::Explosion(DefId(word_u32(value))),
        _ => Field::Object,
    }
}

/// The other way, for an explosion's definition handed back (BlastArgs).
pub(crate) fn field_words(f: &Field, strings: &mut Strings) -> (u32, f64) {
    match f {
        Field::Undefined => (0, 0.0),
        Field::Null => (1, 0.0),
        Field::Bool(false) => (2, 0.0),
        Field::Bool(true) => (3, 0.0),
        Field::Num(x) => (4, *x),
        Field::Str(s) => (5, strings.intern_str(s).0 as f64),
        Field::Explosion(id) => (6, id.0 as f64),
        Field::Object => (7, 0.0),
    }
}

/// The fields of the call block's record: (tag, value) word pairs.
pub(crate) fn call_fields<const N: usize>(calls: &Calls, strings: &Strings) -> [Field; N] {
    let (row, _) = calls.args();
    std::array::from_fn(|i| field(row[2 * i], row[2 * i + 1], strings))
}

/// Registers ordnance class `id` (DefTable order): its 16 fields
/// (OrdnanceDef::FIELDS), two words each, in the call block.
#[no_mangle]
// Ordnance construction resolves each immutable projectile class once.
// The definition table keeps JavaScript's absent/null/number field distinctions.
// Flight and immediate hits read numeric policies prepared by the unit owner.
// The class import never runs once per live projectile tick.
pub extern "C" fn ord_def(id: u32) { ord_def_in(crate::abi::world(), id) }

pub(crate) fn ord_def_in(w: &mut crate::world::owner::World, id: u32) {
    let s = &mut w.sim;
    let fields: [Field; 16] = call_fields(&s.calls, &s.strings);
    let def = OrdnanceDef::read(|name| fields[OrdnanceDef::FIELDS.iter().position(|f| *f == name).expect("an ordnance field")].clone());
    OrdnanceOwnerState::install_ordnance(defs_in(&mut w.ordnance), id, def);
}

/// Registers explosion `id`: Damage, DamageRadiusInner, DamageRadiusOuter,
/// then the five scales (SCALE_FIELDS), two words each, in the call block.
#[no_mangle]
// Ordnance construction prepares explosion radii and health-type policy.
// A referenced explosion may arrive before the unit owner binds its services.
// The cold binding pass supplies the corresponding native combat handle.
// Recursive blasts later use that handle without looking up field names.
pub extern "C" fn ord_explosion(id: u32) { ord_explosion_in(crate::abi::world(), id) }

pub(crate) fn ord_explosion_in(w: &mut crate::world::owner::World, id: u32) {
    let s = &mut w.sim;
    let fields: [Field; 8] = call_fields(&s.calls, &s.strings);
    let def = ExplosionDef::read(|name| match name {
        "Damage" => fields[0].clone(),
        "DamageRadiusInner" => fields[1].clone(),
        "DamageRadiusOuter" => fields[2].clone(),
        _ => fields[3 + SCALE_FIELDS.iter().position(|f| *f == name).expect("an explosion field")].clone(),
    });
    OrdnanceOwnerState::install_explosion(defs_in(&mut w.ordnance), id, def);
}

/// The words of a command's record in the call block: 0 FireArgs, 1
/// FlightMissileArgs (src/ordnance-sim.js writes them with the generated
/// put functions).
#[no_mangle]
pub extern "C" fn ord_args_words(which: u32) -> u32 {
    (match which {
        0 => FireArgs::WORDS,
        1 => FlightMissileArgs::WORDS,
        _ => 0,
    }) as u32
}

/// ordnance.js update (a world's): every round's step, then the lists kept.
/// Which kinds' mirrors have anything to sync: a bit a kind (beams, bolts,
/// grenades, rounds) that has entities or retired ones; none, and nothing
/// moved (the Ordnance's lists and `start` are as they were).
#[no_mangle]
// Ordnance.update advances the canonical lists in their original order.
// Native unit services share the same collision, history and damage owner.
// A projectile's immediate hit completes before the next flight entry advances.
// Mirrors and pure presentation notices are delivered after this entry returns.
pub extern "C" fn ord_update(dt: f64) -> u32 { ord_update_in(crate::abi::world(), dt) }

pub(crate) fn ord_update_in(w: &mut crate::world::owner::World, dt: f64) -> u32 {
    let boundary = crate::units::f3::external_in(w);
    let result = crate::units::f3::with_services_in(w, |w,recon| {
        let mut world = JsWorld::with_services(w, recon);
        let mut cx = Cx::new(&mut world);
        cx.update(dt);
        let s = cx.s();
        let o = &s.ordnance;
        let result = u32::from(!o.beams.is_empty() || !s.beam.retired.is_empty())
            | u32::from(!o.bolts.is_empty() || !s.bolt.retired.is_empty()) << 1
            | u32::from(!o.grenades.is_empty() || !s.grenade.retired.is_empty()) << 2
            | u32::from(!o.rounds.is_empty() || !s.round.retired.is_empty()) << 3;
        world.finish();
        result
    });
    boundary.finish(w,true);
    result
}

/// ordnance.js fireRound, its arguments (FireArgs) in the call block. The
/// answer, in the call block: the kind's code of the round made (0 for a
/// beam or a grenade, which fire gives none of), then its row.
#[no_mangle]
// Ordnance.fire can produce an instant hit before returning its shot.
// Use the same native world services as the later flight update.
// Launch identity must be published before any impact references that round.
// The answer is a retained mirror handle, not a copied JavaScript projectile.
pub extern "C" fn ord_fire() -> *const f64 { ord_fire_in(crate::abi::world()) }

pub(crate) fn ord_fire_in(w: &mut crate::world::owner::World) -> *const f64 {
    let boundary = crate::units::f3::external_in(w);
    let a = {
        let (row, heap) = w.sim.calls.args();
        FireArgs::take(row, heap)
    };
    let shot = crate::units::f3::with_services_in(w, |w,recon| {
        let mut world = JsWorld::with_services(w, recon);
        let mut cx = Cx::new(&mut world);
        let shot = cx.fire_round(a.def, a.origin, a.direction, a.owner, a.visual, a.ignore_body, a.emitter, a.channel, a.target_id, a.target_generation);
        world.finish();
        shot
    });
    let result = answer(&mut w.sim,shot);
    boundary.finish(w,true);
    result
}

/// ordnance.js flightMissile, its arguments (FlightMissileArgs) in the call
/// block; the answer as ord_fire's (0: the round is gone).
#[no_mangle]
pub extern "C" fn ord_flight_missile() -> *const f64 { ord_flight_missile_in(crate::abi::world()) }

pub(crate) fn ord_flight_missile_in(w: &mut crate::world::owner::World) -> *const f64 {
    let a = {
        let (row, heap) = w.sim.calls.args();
        FlightMissileArgs::take(row, heap)
    };
    let boundary = crate::units::f3::external_in(w);
    let flying = crate::units::f3::with_services_in(w, |w,recon| {
        let mut world = JsWorld::with_services(w, recon);
        let mut cx = Cx::new(&mut world);
        let flying = cx.flight_missile(a.round, a.flight_class, a.life, a.speed);
        world.finish();
        flying
    });
    let result = answer(&mut w.sim, if flying { Some(Shot::Round(a.round)) } else { None });
    boundary.finish(w,true);
    result
}

/// A command's answer: the round's kind and row, into the call block.
// Ordnance.fire returns only projectile kinds that have an ordinary mirror.
// Beam and grenade admission can succeed without returning that handle.
// Publish the row before its answer references are decoded by the caller.
// A missing shot is distinct from a just-retired projectile's terminal sample.
fn answer(s: &mut Sim, shot: Option<Shot>) -> *const f64 {
    let Sim { store, calls, .. } = s;
    match shot {
        None => calls.result(1, |row, _| row[0] = 0.0),
        Some(shot) => {
            let (kind, stride) = shot_layout(shot);
            calls.result(1 + stride, |row, heap| {
                row[0] = kind as f64;
                put_row(store, shot, &mut row[1..], heap);
            });
        }
    }
    calls.block.ptr()
}

fn shot_layout(shot: Shot) -> (KindCode, usize) {
    match shot {
        Shot::Bolt(_) => (KIND_BOLT, Bolt::EXPORT_STRIDE),
        Shot::Beam(_) => (KIND_BEAM, Beam::EXPORT_STRIDE),
        Shot::Grenade(_) => (KIND_GRENADE, Grenade::EXPORT_STRIDE),
        Shot::Round(_) => (KIND_ROUND, Round::EXPORT_STRIDE),
    }
}

/// A round's row as the store exports it (slot, generation, its mirrored
/// fields), live or retired since the last export (a bolt whose catch-up
/// ended it).
fn put_row(store: &Store, shot: Shot, row: &mut [f64], heap: &mut Vec<f64>) {
    macro_rules! row {
        ($slab:ident, $r:expr) => {{
            let r = $r;
            let v = store.$slab.get(r).or_else(|| store.$slab.retired.iter().rev().find(|(x, _)| *x == r).map(|(_, v)| v)).expect("a round the ordnance holds");
            row[0] = r.slot as f64;
            row[1] = r.gen as f64;
            v.put_export(row, heap);
        }};
    }
    match shot {
        Shot::Bolt(r) => row!(bolt, r),
        Shot::Beam(r) => row!(beam, r),
        Shot::Grenade(r) => row!(grenade, r),
        Shot::Round(r) => row!(round, r),
    }
}

#[link(wasm_import_module = "bfe")]
extern "C" {
    fn bfe_import_projectile();
    fn bfe_import_launch();
    fn bfe_import_impact();
    fn bfe_import_grenade_contact();
}

/// The battle as the module's imports give it.
pub struct JsWorld<'w, 'a> {
        w: &'w mut crate::world::owner::World,
        /// The items' targets as last read (None: read them again before the
        /// next use), and their sides toward each team asked about.
        targets: Option<Vec<TargetState>>,
        relations: Vec<(f64, Vec<f64>)>,
        near: Vec<Ref<Unit>>,
        near_frame: Option<usize>,
        recon: Option<crate::units::f3::WorldServices<'a>>,
    }

impl<'w, 'a> JsWorld<'w, 'a> {
    /// One root borrow supplies every canonical row used by this adapter.
    pub(crate) fn with_services(
        w: &'w mut crate::world::owner::World,
        recon: Option<crate::units::f3::WorldServices<'a>>,
    ) -> Self {
        w.ordnance.defs.get_or_insert_with(Defs::default);
        Self {
            w,
            targets: None,
            relations: Vec::new(),
            near: Vec::new(),
            near_frame: None,
            recon,
        }
    }

    // OrdnanceWorld's lifecycle calls are observations. Keep their sampled
    // values in the shared ring without syncing or changing a live mirror.
    // Captures use the projectile's stable key plus this value snapshot.
    // Ordnance observers retain the shot's state at its original callback slot.
    // A later death or retirement must not change that already-issued sample.
    // Store the identity beside the sampled pose instead of reconstructing it later.
    // The outer coordinator binds births before delivering these pure observers.
    fn notice(&mut self, event: F3OrdnanceEvent, shot: Shot) -> F3OrdnanceNotice {
        let mut notice = F3OrdnanceNotice { event, ..Default::default() };
        match shot {
            Shot::Bolt(r) => {
                let b = self.store().bolt.get(r).expect("observed bolt");
                notice.key = F3OrdnanceKey { kind:F3ProjectileKind::Bolt, slot:r.slot, generation:r.gen };
                notice.def = b.def; notice.owner = b.owner; notice.position = b.position;
                notice.direction = b.direction; notice.speed = b.speed; notice.life = b.life; notice.merge = b.merge;
                notice.offset = b.offset.unwrap_or([0.0;3]); notice.has_offset = b.offset.is_some();
                notice.owner_id = b.network_fire_unit; notice.owner_generation = b.network_fire_generation;
                notice.fire_id = b.network_fire_id; notice.channel = b.network_fire_channel;
            }
            Shot::Beam(r) => {
                let b = self.store().beam.get(r).expect("observed beam");
                notice.key = F3OrdnanceKey { kind:F3ProjectileKind::Beam, slot:r.slot, generation:r.gen };
                notice.def = b.def; notice.owner = b.owner; notice.position = b.position;
                notice.direction = b.direction; notice.life = b.life; notice.channel = b.network_fire_channel;
            }
            Shot::Grenade(r) => {
                let g = self.store().grenade.get(r).expect("observed grenade");
                notice.key = F3OrdnanceKey { kind:F3ProjectileKind::Grenade, slot:r.slot, generation:r.gen };
                notice.def = g.def; notice.owner = g.owner; notice.position = g.position;
                notice.life = g.fuse.unwrap_or(0.0); notice.channel = g.network_fire_channel;
            }
            Shot::Round(r) => {
                let round = self.store().round.get(r).expect("observed round");
                notice.key = F3OrdnanceKey { kind:F3ProjectileKind::Round, slot:r.slot, generation:r.gen };
                notice.def = round.def; notice.owner = round.owner; notice.position = round.position;
                notice.life = round.life; notice.channel = round.network_fire_channel;
            }
        }
        // Bolts carry their captured fire metadata on the row. Other kinds'
        // launch observers read the owner's edge at this exact call point.
        if !matches!(shot,Shot::Bolt(_)) {
            let shooter = crate::units::f3_sensors::shooter_in(self.w, notice.owner,notice.channel);
            notice.owner_id = shooter.unit; notice.owner_generation = shooter.generation; notice.fire_id = shooter.fire_id;
        }
        notice.team = crate::combat::live::owner_facts_in(&self.w.sim, &mut self.w.units, notice.owner).0;
        notice.has_source = matches!(notice.owner,OrdnanceOwner::Unit(_) | OrdnanceOwner::OwnerSnapshot(_));
        notice.source = F3LifeRef { id:notice.owner_id, generation:notice.owner_generation };
        notice
    }

    /// The items' target `index` as it is now (the batch read once, and
    /// again after an explosion).
    fn target(&mut self, index: usize) -> &TargetState {
        let targets = self.targets.get_or_insert_with(|| import_item_targets(&NoArgs {}).targets);
        &targets[index]
    }

    /// Target `index`'s side toward `team` (relation(team)).
    fn relation(&mut self, index: usize, team: f64) -> f64 {
        let at = match self.relations.iter().position(|(t, _)| same_f64(*t, team)) {
            Some(at) => at,
            None => {
                self.relations.push((team, import_item_relations(&TeamArgs { team }).relations));
                self.relations.len() - 1
            }
        };
        self.relations[at].1[index]
    }

    fn string(&self, id: StrId) -> String {
        // SAFETY: the module's note.
        String::from_utf16_lossy(&self.w.sim.strings.units(id))
    }

    fn store(&self) -> &Store {
        &self.w.sim.store
    }

    /// A damage call's definition: the class, or the beam's copy of it
    /// (its class and this step's Damage).
    fn def_arg(&self, def: DefArg) -> (DefId, Option<Ref<Beam>>, Option<f64>) {
        match def {
            DefArg::Ordnance(id) => (id, None, None),
            DefArg::Beam(r) => {
                let b = self.store().beam.get(r).expect("a beam the ordnance holds");
                (b.def, Some(r), b.damage)
            }
        }
    }

    /// An import that hands a round over: its own words (`words`, written
    /// by `put`), then the round's row; then the call.
    // Ordnance's foreign presentation observer needs committed projectile rows.
    // Write only the affected shot before handing its generated argument block over.
    // The import may issue another command and reuse the shared exchange storage.
    // The resumed native solver therefore keeps its arguments in local value frames.
    fn hand(&self, shot: Shot, words: usize, put: impl FnOnce(&mut [f64], &mut Vec<f64>), call: unsafe extern "C" fn()) {
        let (_, stride) = shot_layout(shot);
        let store = self.store();
        import_out(words + stride, |row, heap| {
            put(&mut row[..words], heap);
            put_row(store, shot, &mut row[words..], heap);
        });
        // SAFETY: an import the module's loader gives every instance.
        unsafe { call() };
    }
}

impl JsWorld<'_, '_> {
    pub(crate) fn finish(mut self) {
        if let Some(frame) = self.near_frame.take() {
            crate::units::f3::put_ordnance_query_in(&mut self.w.units,frame,std::mem::take(&mut self.near));
        }
    }
}

fn hit_of(a: CastAnswer) -> Option<Hit> {
    a.found.then_some(Hit { distance: a.distance, point: a.point, normal: a.normal, terrain: a.terrain, body: a.body })
}

fn point_of(a: PointAnswer) -> Option<Vec3> {
    a.found.then_some(a.point)
}

impl World for JsWorld<'_, '_> {
    fn store(&self) -> &Store { &self.w.sim.store }
    fn store_mut(&mut self) -> &mut Store { &mut self.w.sim.store }
    fn strings_mut(&mut self) -> &mut Strings { &mut self.w.sim.strings }
    fn definitions(&self) -> &Defs { self.w.ordnance.defs.as_ref().expect("prepared ordnance definitions") }

    fn cast(&mut self, origin: Vec3, direction: Vec3, far: f64, out: Out, ignore_body: Nullable<Ref<Body>>, static_only: Option<bool>) -> Option<Hit> {
        if crate::units::f3::active_in(&self.w.units) {
            let ignore = match ignore_body { Nullable::Value(r) => Some(r), _ => None };
            return crate::units::native::ordnance_cast_in(self.w, crate::units::f3::collision_world_in(&self.w.units), origin, direction, far, ignore, static_only.unwrap_or(false), false);
        }
        hit_of(import_cast(&CastArgs { origin, direction, far, behind: out == Out::Behind, ignore_body, static_only }))
    }

    fn cast_moving(&mut self, origin: Vec3, direction: Vec3, far: f64, out: Out, ignore_body: Nullable<Ref<Body>>) -> Option<Hit> {
        if crate::units::f3::active_in(&self.w.units) {
            let ignore = match ignore_body { Nullable::Value(r) => Some(r), _ => None };
            return crate::units::native::ordnance_cast_in(self.w, crate::units::f3::collision_world_in(&self.w.units), origin, direction, far, ignore, false, true);
        }
        hit_of(import_cast_moving(&CastArgs { origin, direction, far, behind: out == Out::Behind, ignore_body, static_only: None }))
    }

    // OrdnanceWorld.castUnits chooses canonical live or historical geometry.
    // A historical target never rewinds the shooter's current muzzle position.
    // Exposed riders are admitted only for projectile queries that request them.
    // Strict equal-distance ties retain the mixed registry's first member.
    fn raycast_units(&mut self, origin: Vec3, direction: Vec3, far: f64, owner: OrdnanceOwner, history_tick: Option<f64>, ignore_body: Option<Ref<Body>>, mount: Option<Ref<Vehicle>>) -> Option<UnitHit> {
        if crate::units::f3::active_in(&self.w.units) {
            return crate::units::f3_sensors::ordnance_raycast_in(self.w, origin,direction,far,owner,history_tick,ignore_body,mount);
        }
        let a = import_raycast_units(&UnitCastArgs { origin, direction, far, owner, history_tick, ignore_body, mount });
        if !a.found {
            return None;
        }
        Some(UnitHit { unit: a.unit.expect("the unit a raycast met"), distance: a.distance, point: a.point, normal: a.normal, part: a.part })
    }

    fn rider_mount(&mut self, body: Nullable<Ref<Body>>) -> Option<RiderMount> {
        // No body met, no hull to strike through (riderMount's `body?.owner`).
        if !matches!(body, Nullable::Value(_)) {
            return None;
        }
        if crate::units::f3::active_in(&self.w.units) {
            let Nullable::Value(body)=body else {return None};
            return crate::units::f3_hulls::rider_mount_in(self.w, body);
        }
        let a = import_rider_mount(&BodyArgs { body });
        if !a.found {
            return None;
        }
        Some(RiderMount { vehicle: a.vehicle.expect("the mount's hull"), bound_radius: a.bound_radius, body: a.body })
    }

    // OrdnanceWorld.fatRay gathers ordered candidates into retained storage.
    // Already-struck units and the launching owner are excluded before testing.
    // The result buffer is returned through recycle_fat_query after consumption.
    // No per-projectile temporary vector is allocated in the owning unit world.
    fn fat_query(&mut self, center: Vec3, radius: f64, owner: OrdnanceOwner, struck: &RefSet<Unit>, ignore_body: Option<Ref<Body>>) -> Option<Vec<Ref<Unit>>> {
        if crate::units::f3::active_in(&self.w.units) {
            // A pure explosion needs no query frame. Borrow a buffer only
            // when a fat ray actually starts a sweep, and retain it thereafter.
            if self.near_frame.is_none() {
                let (frame,buffer) = crate::units::f3::take_ordnance_query_in(&mut self.w.units);
                self.near_frame = Some(frame); self.near = buffer;
            }
            let mut near = std::mem::take(&mut self.near);
            crate::units::f3_sensors::fat_query_in(self.w, center,radius,owner,struck,ignore_body,&mut near);
            return Some(near);
        }
        let a = import_fat_query(&NearArgs { center, radius, owner, struck: struck.clone(), ignore_body });
        a.found.then_some(a.units)
    }

    fn recycle_fat_query(&mut self, mut query: Vec<Ref<Unit>>) {
        if self.near_frame.is_some() { query.clear(); self.near = query; }
    }

    fn unit_sphere(&mut self, unit: Ref<Unit>) -> Option<(Vec3, f64)> {
        if crate::units::f3::active_in(&self.w.units) { return Some(crate::units::f3_sensors::unit_sphere_in(self.w, unit)); }
        let a = import_unit_sphere(&UnitArgs { unit });
        a.found.then_some((a.center, a.radius))
    }

    fn is_droid(&mut self, unit: Ref<Unit>) -> bool {
        if crate::units::f3::active_in(&self.w.units) {
            return crate::units::f3::health_scale_in(&mut self.w.units, unit.slot) == crate::combat::impacts::HealthScale::Droid;
        }
        import_is_droid(&UnitArgs { unit }).value
    }

    fn gone(&mut self, x: BeamSource) -> bool {
        if let Some(gone) = crate::vehicles::module::f2::firing::emitter_gone(x) { return gone; }
        if let Some(r) = crate::vehicles::module::f2::live_seats::emitter_lease(x) { return crate::vehicles::module::f2::live_seats::beam_gone(r); }
        // `x?.alive === false`: never for nobody or the player's own.
        if matches!(x, BeamSource::Null | BeamSource::Player) {
            return false;
        }
        if crate::units::f3::active_in(&self.w.units) {
            return match x {
                BeamSource::Unit(r) => self.store().unit.get(r).is_none_or(|unit| !unit.alive),
                BeamSource::Vehicle(r) => self.store().vehicle.get(r).is_none_or(|hull| !hull.alive),
                BeamSource::OwnerSnapshot(r) => !crate::combat::owners::alive(self.store(),r),
                BeamSource::Emitter(_) => panic!("native mounted emitter service required"),
                _ => false,
            };
        }
        import_gone(&GoneArgs { subject: x }).value
    }

    // OrdnanceWorld.beamLocalOrigin freezes the emitter-local start point.
    // A released seat emitter keeps the generation of its original occupancy.
    // The next holder in that seat must not retarget an already launched beam.
    // World-space reconstruction follows the current canonical source frame.
    fn beam_local_origin(&mut self, source: BeamSource, origin: Vec3) -> Option<Vec3> {
        if let Some(local) = crate::vehicles::module::f2::firing::emitter_local(source, origin) { return Some(local); }
        if let Some(r) = crate::vehicles::module::f2::live_seats::emitter_lease(source) { return crate::vehicles::module::f2::live_seats::beam_local(r, origin); }
        if crate::units::f3::active_in(&self.w.units) {
            // Ordinary Unit and captured-life owners expose no matrix. Beams
            // keep their supplied start, just as beamLocalOrigin returns null.
            let matrix = match source {
                BeamSource::Vehicle(r) => self.store().vehicle.get(r).expect("beam hull").matrix,
                BeamSource::Emitter(_) => panic!("native mounted emitter service required"),
                _ => return None,
            };
            let mut inverse = three::Matrix4::default(); inverse.elements = matrix; inverse.invert();
            let mut point = three::Vector3::new(origin[0],origin[1],origin[2]); point.apply_matrix4(&inverse);
            return Some([point.x,point.y,point.z]);
        }
        point_of(import_beam_local_origin(&SourceArgs { source, origin }))
    }

    // OrdnanceWorld.beamPose observes its source at the beam's flight point.
    // A unit uses native aim; a mounted source uses its generation-qualified lease.
    // Direction and position are updated together before testing this beam segment.
    // Presentation interpolation is never an input to authoritative collision.
    fn beam_pose(&mut self, source: BeamSource, local_origin: Vec3, position: &mut Vec3, direction: &mut Vec3) {
        if crate::vehicles::module::f2::firing::emitter_pose(source, local_origin, position, direction) { return; }
        if let Some(r) = crate::vehicles::module::f2::live_seats::emitter_lease(source) { crate::vehicles::module::f2::live_seats::beam_pose(r, local_origin, position, direction); return; }
        if crate::units::f3::active_in(&self.w.units) {
            let BeamSource::Vehicle(r) = source else { panic!("native mounted beam pose required") };
            let hull = self.store().vehicle.get(r).expect("beam hull");
            let mut matrix = three::Matrix4::default(); matrix.elements = hull.matrix;
            let mut point = three::Vector3::new(local_origin[0],local_origin[1],local_origin[2]); point.apply_matrix4(&matrix);
            *position = [point.x,point.y,point.z];
            let seat = &hull.seats[hull.seat_index as usize];
            *direction = [v8math::sin(seat.aim_yaw)*v8math::cos(seat.pitch),v8math::sin(seat.pitch),v8math::cos(seat.aim_yaw)*v8math::cos(seat.pitch)];
            return;
        }
        let a = import_beam_pose(&BeamPoseArgs { source, local_origin, position: *position, direction: *direction });
        *position = a.position;
        *direction = a.direction;
    }

    // HitHistory.shotTick applies the shooter's input-pipeline rewind budget.
    // A current-life check rejects captures belonging to an earlier pooled spawn.
    // The current world tick bounds future requests independently of ring capacity.
    // A missing eligible press keeps an ordinary live-world shot.
    fn shot_tick(&mut self, owner: OrdnanceOwner, channel: f64) -> Nullable<f64> {
        if crate::units::f3::active_in(&self.w.units) { return crate::units::f3_sensors::shot_tick_in(self.w, owner,channel); }
        import_shot_tick(&ShotArgs { owner, channel }).value
    }

    fn history_tick(&mut self) -> f64 {
        if crate::units::f3::active_in(&self.w.units) { return crate::units::f3::history_in(&mut self.w.units).tick; }
        import_history_tick(&NoArgs {}).value
    }

    fn shooter(&mut self, owner: OrdnanceOwner, channel: f64) -> Shooter {
        if crate::units::f3::active_in(&self.w.units) { return crate::units::f3_sensors::shooter_in(self.w, owner,channel); }
        let a = import_shooter(&ShotArgs { owner, channel });
        Shooter { fire_id: a.fire_id, unit: a.unit, generation: a.generation }
    }

    fn missile_target(&mut self, target_id: f64, target_generation: f64) -> Option<Vec3> {
        if crate::units::f3::active_in(&self.w.units) { return crate::units::f3_sensors::missile_target_in(self.w, target_id,target_generation); }
        point_of(import_missile_target(&TargetArgs { target_id, target_generation }))
    }

    fn navigate_missile(&mut self, position: Vec3, velocity: &mut Vec3, flight_class: Option<StrId>, target_id: f64, target_generation: f64, dt: f64) -> f64 {
        if crate::units::f3::active_in(&self.w.units) {
            return crate::units::f3_sensors::navigate_missile_in(self.w, position,velocity,flight_class,target_id,target_generation,dt);
        }
        let a = import_navigate_missile(&SeekArgs { position, velocity: *velocity, flight_class, target_id, target_generation, dt });
        *velocity = a.velocity;
        a.target_id
    }

    fn water_crossing(&mut self, from: Vec3, to: Vec3) -> Option<Vec3> {
        if !water(&self.w.ordnance, 0) {
            return None;
        }
        if crate::units::f3::active_in(&self.w.units) { return crate::units::f3_sensors::water_crossing_in(self.w, from,to); }
        point_of(import_water_crossing(&SegmentArgs { from, to }))
    }

    fn waver(&mut self) -> f64 {
        if crate::units::f3::active_in(&self.w.units) { return crate::units::f3::random_gameplay_in(&mut self.w.units); }
        import_waver(&NoArgs {}).value
    }

    fn spin(&mut self) -> f64 {
        if crate::units::f3::active_in(&self.w.units) { return crate::units::f3::random_spin_in(&mut self.w.units); }
        import_spin(&NoArgs {}).value
    }

    // OrdnanceWorld.explosion completes native fan-out synchronously.
    // Unit deaths can recurse into this service before their parent's next victim.
    // The generated explosion notice follows units and precedes the other owners.
    // No callback enters a JavaScript damage phase from this native span.
    fn explosion(&mut self, def: &Field, position: Vec3, owner: OrdnanceOwner) {
        if crate::units::f3::active_in(&self.w.units) {
            let Field::Explosion(def) = def else { panic!("prepared native explosion") };
            crate::combat::live::blast_in(self.w, *def,position,owner,self.recon.as_mut().map(crate::units::f3::WorldServices::reborrow));
            return;
        }
        // SAFETY: the module's note.
        let (def_tag, def) = field_words(def, &mut self.w.sim.strings);
        import_explosion(&BlastArgs { def_tag, def, position, owner });
    }

    fn hit(&mut self, body: Ref<Body>, def: DefArg, owner: OrdnanceOwner, direction: Vec3) {
        if crate::units::f3::active_in(&self.w.units) {
            let packet = crate::units::f3_props::hit_policy_in(self.w, def);
            crate::combat::live::hit_body_packet_in(self.w, body, packet, owner,
                Some(three::Vector3::new(direction[0], direction[1], direction[2])),
                self.recon.as_mut().expect("native world services").reborrow());
            return;
        }
        let (def, beam, damage) = self.def_arg(def);
        import_hit(&StrikeArgs { body, def, beam, damage, owner, direction });
    }

    // Game.onHitUnit receives the exact terminal point and incoming direction.
    // Native health-scale policy distinguishes body damage from shield damage.
    // Head effects precede friendly-fire rejection, retaining the original ordering.
    // Death and any resulting explosion finish before this projectile resumes.
    fn hit_unit(&mut self, unit: Ref<Unit>, def: DefArg, point: Point, owner: OrdnanceOwner, part: Nullable<HitPart>, direction: Vec3) {
        if crate::units::f3::active_in(&self.w.units) {
            crate::combat::live::hit_unit_in(self.w, unit,def,point.value(),owner,part,direction,self.recon.as_mut().map(crate::units::f3::WorldServices::reborrow));
            return;
        }
        let beam_def = matches!(def, DefArg::Beam(_));
        let (def, mut beam, damage) = self.def_arg(def);
        let at = match point {
            Point::Fresh(_) => 0,
            Point::BeamEnd(r, _) => {
                beam = Some(r);
                1
            }
            Point::UnitHit(_) => 2,
            Point::Start(_) => 3,
        };
        import_hit_unit(&UnitStrikeArgs { unit, def, beam, beam_def, damage, at, point: point.value(), owner, part, direction });
    }

    fn projectile(&mut self, shot: Shot, kind: ShotKind) {
        if crate::units::f3::active_in(&self.w.units) {
            self.notice(F3OrdnanceEvent::Projectile,shot).emit(&mut self.w.sim.events);
            return;
        }
        let (code, _) = shot_layout(shot);
        let args = ProjectileArgs { kind: code as u32, r#type: kind as u32 };
        self.hand(shot, ProjectileArgs::WORDS, |row, heap| args.put(row, heap), bfe_import_projectile);
    }

    fn launch(&mut self, bolt: Ref<Bolt>, tick: Nullable<f64>) {
        if crate::units::f3::active_in(&self.w.units) {
            let mut notice = self.notice(F3OrdnanceEvent::Launch,Shot::Bolt(bolt));
            notice.tick = tick; notice.emit(&mut self.w.sim.events);
            return;
        }
        let args = LaunchArgs { kind: KIND_BOLT as u32, tick };
        self.hand(Shot::Bolt(bolt), LaunchArgs::WORDS, |row, heap| args.put(row, heap), bfe_import_launch);
    }

    // OrdnanceWorld.impact observes the projectile before its identity retires.
    // Retain terminal point, normal and sampled life for network capture observers.
    // A later mirror refresh must not substitute a new slot occupant's state.
    // Pure impact effects drain after the owning native entry has completed.
    fn impact(&mut self, shot: Shot, info: &ImpactInfo) {
        if crate::units::f3::active_in(&self.w.units) {
            let mut notice = self.notice(F3OrdnanceEvent::Impact,shot);
            match info {
                ImpactInfo::Plain { point, normal, surface } => {
                    notice.point = *point; notice.normal = *normal; notice.surface = *surface;
                }
                ImpactInfo::Struck { impact, surface } => {
                    // Impact holds only fixed numbers and qualified handles.
                    notice.struck = impact.clone(); notice.has_struck = true;
                    notice.point = impact.point; notice.normal = impact.normal; notice.surface = *surface;
                }
            }
            notice.emit(&mut self.w.sim.events);
            return;
        }
        let (code, _) = shot_layout(shot);
        let args = match info {
            ImpactInfo::Plain { point, normal, surface } => ImpactArgs { kind: code as u32, point: *point, normal: *normal, surface: *surface, struck: None },
            ImpactInfo::Struck { impact, surface } => ImpactArgs { kind: code as u32, point: impact.point, normal: impact.normal, surface: *surface, struck: Some(impact.clone()) },
        };
        self.hand(shot, ImpactArgs::WORDS, |row, heap| args.put(row, heap), bfe_import_impact);
    }

    // OrdnanceWorld.grenadeContact samples each accepted contact in source order.
    // Sound and spark observers must see the contact before a later bounce moves it.
    // The retained journal separates those observations from canonical flight state.
    // Gameplay collision and immediate explosion remain native operations.
    fn grenade_contact(&mut self, grenade: Ref<Grenade>) {
        let current=self.store().grenade.contains(grenade);
        if current {if let Some(ai)=self.recon.as_mut().and_then(|services|services.ai.as_mut()) {ai.grenade_contact(grenade);}}
        if crate::units::f3::active_in(&self.w.units) {
            self.notice(F3OrdnanceEvent::GrenadeContact,Shot::Grenade(grenade)).emit(&mut self.w.sim.events);
            return;
        }
        let args = ContactArgs { kind: KIND_GRENADE as u32 };
        self.hand(Shot::Grenade(grenade), ContactArgs::WORDS, |row, heap| args.put(row, heap), bfe_import_grenade_contact);
    }

    fn bounce(&mut self, def: DefId, position: Vec3, speed: f64) {
        if crate::units::f3::active_in(&self.w.units) {
            F3OrdnanceNotice { event:F3OrdnanceEvent::Bounce, def, position, speed, ..Default::default() }.emit(&mut self.w.sim.events);
            return;
        }
        import_bounce(&BounceArgs { def, position, speed });
    }

    // The things lying about (ItemsWorld).

    fn item_cast(&mut self, origin: Vec3, direction: Vec3, far: f64) -> Option<Hit> {
        if crate::units::f3::active_in(&self.w.units) {
            return crate::units::native::ordnance_cast_in(self.w, crate::units::f3::collision_world_in(&self.w.units),origin,direction,far,None,false,false);
        }
        hit_of(import_item_cast(&ItemCastArgs { origin, direction, far }))
    }

    fn blocked(&mut self, position: Vec3, target: Vec3) -> bool {
        if crate::units::f3::active_in(&self.w.units) {
            // ItemsWorld.blocked uses blastBlocked: moving hulls do not
            // shield a mine, and the final five centimetres are tolerated.
            let mut direction = three::Vector3::new(target[0]-position[0],target[1]-position[1],target[2]-position[2]);
            let span = direction.length();if span < 1e-3 { return false; }
            direction.divide_scalar(span);
            return crate::units::native::ordnance_cast_in(self.w, crate::units::f3::collision_world_in(&self.w.units),position,
                [direction.x,direction.y,direction.z],span,None,true,false).is_some_and(|hit| hit.distance < span-0.05);
        }
        import_blocked(&BlockedArgs { position, target }).value
    }

    fn item_water_crossing(&mut self, from: Vec3, to: Vec3) -> Option<Vec3> {
        if !water(&self.w.ordnance, 1) {
            return None;
        }
        if crate::units::f3::active_in(&self.w.units) { return crate::units::f3_sensors::water_crossing_in(self.w, from,to); }
        point_of(import_item_water_crossing(&SegmentArgs { from, to }))
    }

    fn item_model(&mut self, def: DefId) -> ItemModel {
        if crate::units::f3::active_in(&self.w.units) { return crate::units::f3::items_in(&mut self.w.units).class(def).model; }
        let a = import_item_model(&ItemDefArgs { def });
        ItemModel { mesh: a.mesh, radius: a.radius, center: a.center }
    }
    // ItemsWorld.model geometry is prepared once for each immutable class.
    // The native item owner reads its radius and center without a model callback.
    // A missing visual mesh keeps the class's original physical fallback.
    // Ordinary item ticks never load or measure presentation geometry.
    fn prepared_item(&mut self, def: DefId) -> Option<PreparedItem> {
        crate::units::f3::active_in(&self.w.units).then(|| crate::units::f3::items_in(&mut self.w.units).class(def))
    }
    fn item_blast_damage(&mut self, explosion: DefId, item: DefId, distance: f64) -> Option<f64> {
        if !crate::units::f3::active_in(&self.w.units) { return None; }
        let packet = crate::units::f3::blast_packet_in(self.w, explosion);
        let scale = crate::units::f3::items_in(&mut self.w.units).class(item).scale;
        Some(packet.hit.damage * crate::ordnance::defs::falloff(distance,packet.inner,packet.outer) * packet.hit.scales[scale])
    }
    fn owned_item_frames(&self) -> bool { crate::units::f3::active_in(&self.w.units) }
    // ItemsWorld publishes mutation samples beside stable item handles.
    // A newly created item's identity is bound before later notices name it.
    // Pickup and retirement can both occur before the outer phase returns.
    // Their saved observations must not be reconstructed from final live state.
    fn item_changed(&mut self, item: Ref<Item>, made: bool) {
        if crate::units::f3::active_in(&self.w.units) { crate::units::f3_items::changed_in(&mut self.w.sim, item,made); }
    }
    fn item_retired(&mut self, item: Ref<Item>) {
        crate::units::f3_items::retired_in(&mut self.w.sim, item);
    }
    fn dispenser_active(&self, key: StrId) -> bool { crate::units::f3::items_read(&self.w.units).active(key) }
    fn retain_empty_dispenser(&self, key: StrId) -> bool { crate::units::f3::items_read(&self.w.units).current(key) }
    fn dispenser_changed(&mut self, key: StrId, removed: bool) {
        if crate::units::f3::active_in(&self.w.units) { crate::units::f3::items_in(&mut self.w.units).changed(key,removed); }
    }

    fn owner_key(&mut self, owner: ItemOwner) -> String {
        let key = import_owner_key(&OwnerArgs { owner }).key;
        self.string(key)
    }

    fn drop_roll(&mut self) -> f64 {
        if crate::units::f3::active_in(&self.w.units) { return crate::units::f3::random_drop_in(&mut self.w.units); }
        import_drop_roll(&NoArgs {}).value
    }

    fn explode(&mut self, def: &Field, position: Vec3, owner: ItemOwner) {
        if crate::units::f3::active_in(&self.w.units) {
            let owner = match owner {
                ItemOwner::Null => OrdnanceOwner::Null,
                ItemOwner::Unit(r) => OrdnanceOwner::Unit(r),
                ItemOwner::Player => OrdnanceOwner::Player,
            };
            <Self as World>::explosion(self,def,position,owner);
            return;
        }
        // SAFETY: the module's note.
        let (def_tag, def) = field_words(def, &mut self.w.sim.strings);
        import_explode(&ItemBlastArgs { def_tag, def, position, owner });
        // An explosion moves what it reaches: the targets are read again.
        self.targets = None;
    }

    fn watch(&mut self, index: usize, team: f64) -> Watched {
        if crate::units::f3::active_in(&self.w.units) { return crate::units::f3_items::watch_in(self.w, index,team); }
        let relation = self.relation(index, team);
        let t = self.target(index);
        Watched { relation, center: t.center, radius: t.radius, velocity: t.velocity }
    }

    fn standing(&mut self, index: usize) -> Vec3 {
        if crate::units::f3::active_in(&self.w.units) { return crate::units::f3_items::standing_in(self.w, index); }
        self.target(index).position
    }

    fn give(&mut self, index: usize, def: DefId) -> bool {
        if crate::units::f3::active_in(&self.w.units) {
            return crate::units::f3_items::give_in(self.w, index,def,self.recon.as_mut().map(crate::units::f3::WorldServices::reborrow));
        }
        import_give(&GiveArgs { index: index as u32, def }).value
    }

    // ItemsWorld.pickup applies supply to canonical unit or hull state.
    // The target list contains qualified handles, not copied body snapshots.
    // Supply observers follow the health/ammunition mutation in source order.
    // A later unit publication cannot restore the target's earlier resources.
    fn pickup(&mut self, def: DefId, position: Vec3) {
        if crate::units::f3::active_in(&self.w.units) { PickupArgs {def,position}.emit(&mut self.w.sim.events); return; }
        import_pickup(&PickupArgs { def, position });
    }
}

impl OrdnanceOwnerState {
    fn install_ordnance(d: &mut Defs, id: u32, def: OrdnanceDef) {        let i = id as usize;
        if d.ordnance.len() <= i {
            d.ordnance.resize(i + 1, def.clone());
        }
        d.ordnance[i] = def;
    }
}

impl OrdnanceOwnerState {
    fn install_explosion(d: &mut Defs, id: u32, def: ExplosionDef) {        let i = id as usize;
        if d.explosion.len() <= i {
            d.explosion.resize(i + 1, def.clone());
        }
        d.explosion[i] = def;
    }
}
