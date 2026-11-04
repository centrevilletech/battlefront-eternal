//! The rounds on the module: the exports JavaScript drives them through once
//! a battle's world hands them over (src/ordnance-sim.js), and the world
//! their Rust reaches the battle through there: the imports of
//! rust/sim/schema/ordnance.mjs, which src/ordnance-sim.js serves with the
//! same OrdnanceWorld (src/ordnance-world.js) the JavaScript rounds call.
//!
//! The store is reached through pointers taken from the instance's one
//! `Sim` (as `Cx` does), never through a reference held across an import:
//! JavaScript serving one may call back in (a death a round caused firing
//! another round, a string read), and finds the store as the phase left it.
//! A command's arguments come in the call block (`sim_call_room`), and its
//! answer goes out there; an import's go out and come back in the import
//! block (store::calls), as every import's do.
//!
//! A round handed to JavaScript (a projectile made, a bolt launched, a
//! round's end, a grenade's first touch) crosses with its row as the store
//! exports it, after the import's own words, so its mirror is the round as
//! it is at that moment: the capture's observers read it then, and a sim
//! event keeps a grenade's position.

use std::ptr::addr_of_mut;

use super::defs::{Defs, ExplosionDef, Field, OrdnanceDef, SCALE_FIELDS};
use super::world::*;
use super::Cx;
use crate::abi::{sim, Sim};
use crate::gen::*;
use crate::store::calls::{import_out, Calls};
use crate::store::{same_f64, word_u32, DefId, KindCode, Nullable, Ref, RefSet, StrId, Strings, Vec3};

static mut DEFS: Option<Defs> = None;

/// Whether the rounds' and the items' battle has water (ordnance.water,
/// items.water): without, a water crossing is null and nobody is asked.
static mut WATER: [bool; 2] = [false, false];

fn water(which: usize) -> bool {
    // SAFETY: one thread.
    unsafe { (*addr_of_mut!(WATER))[which] }
}

/// The rounds' (0) or the items' (1) water, present or not.
#[no_mangle]
pub extern "C" fn ord_water(which: u32, present: u32) {
    // SAFETY: one thread.
    unsafe { (*addr_of_mut!(WATER))[(which & 1) as usize] = present != 0 };
}

/// The battle's definitions as JavaScript registered them (`ord_def`,
/// `ord_explosion`).
pub(crate) fn defs() -> *mut Defs {
    // SAFETY: one thread; nothing holds a reference to the definitions
    // across a call out of the module (Cx reads them through this pointer).
    unsafe { (*addr_of_mut!(DEFS)).get_or_insert_with(Defs::default) as *mut Defs }
}

/// The store and the strings, as pointers (the module's note).
pub(crate) fn parts() -> (*mut Store, *mut Strings) {
    let s: *mut Sim = sim();
    // SAFETY: `s` is the instance's Sim, which lives as long as the instance.
    unsafe { (addr_of_mut!((*s).store), addr_of_mut!((*s).strings)) }
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
pub extern "C" fn ord_def(id: u32) {
    let s = sim();
    let fields: [Field; 16] = call_fields(&s.calls, &s.strings);
    let def = OrdnanceDef::read(|name| fields[OrdnanceDef::FIELDS.iter().position(|f| *f == name).expect("an ordnance field")].clone());
    // SAFETY: defs()'s note.
    let d = unsafe { &mut *defs() };
    let i = id as usize;
    if d.ordnance.len() <= i {
        d.ordnance.resize(i + 1, def.clone());
    }
    d.ordnance[i] = def;
}

/// Registers explosion `id`: Damage, DamageRadiusInner, DamageRadiusOuter,
/// then the five scales (SCALE_FIELDS), two words each, in the call block.
#[no_mangle]
pub extern "C" fn ord_explosion(id: u32) {
    let s = sim();
    let fields: [Field; 8] = call_fields(&s.calls, &s.strings);
    let def = ExplosionDef::read(|name| match name {
        "Damage" => fields[0].clone(),
        "DamageRadiusInner" => fields[1].clone(),
        "DamageRadiusOuter" => fields[2].clone(),
        _ => fields[3 + SCALE_FIELDS.iter().position(|f| *f == name).expect("an explosion field")].clone(),
    });
    // SAFETY: defs()'s note.
    let d = unsafe { &mut *defs() };
    let i = id as usize;
    if d.explosion.len() <= i {
        d.explosion.resize(i + 1, def.clone());
    }
    d.explosion[i] = def;
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
pub extern "C" fn ord_update(dt: f64) -> u32 {
    let (store, strings) = parts();
    let mut world = JsWorld::new(store, strings);
    // SAFETY: the module's note.
    let mut cx = unsafe { Cx::new(store, strings, defs(), &mut world) };
    cx.update(dt);
    let s = cx.s();
    let o = &s.ordnance;
    u32::from(!o.beams.is_empty() || !s.beam.retired.is_empty())
        | u32::from(!o.bolts.is_empty() || !s.bolt.retired.is_empty()) << 1
        | u32::from(!o.grenades.is_empty() || !s.grenade.retired.is_empty()) << 2
        | u32::from(!o.rounds.is_empty() || !s.round.retired.is_empty()) << 3
}

/// ordnance.js fireRound, its arguments (FireArgs) in the call block. The
/// answer, in the call block: the kind's code of the round made (0 for a
/// beam or a grenade, which fire gives none of), then its row.
#[no_mangle]
pub extern "C" fn ord_fire() -> *const f64 {
    let a = {
        let (row, heap) = sim().calls.args();
        FireArgs::take(row, heap)
    };
    let (store, strings) = parts();
    let mut world = JsWorld::new(store, strings);
    // SAFETY: the module's note.
    let mut cx = unsafe { Cx::new(store, strings, defs(), &mut world) };
    let shot = cx.fire_round(a.def, a.origin, a.direction, a.owner, a.visual, a.ignore_body, a.emitter, a.channel, a.target_id, a.target_generation);
    answer(shot)
}

/// ordnance.js flightMissile, its arguments (FlightMissileArgs) in the call
/// block; the answer as ord_fire's (0: the round is gone).
#[no_mangle]
pub extern "C" fn ord_flight_missile() -> *const f64 {
    let a = {
        let (row, heap) = sim().calls.args();
        FlightMissileArgs::take(row, heap)
    };
    let (store, strings) = parts();
    let mut world = JsWorld::new(store, strings);
    // SAFETY: the module's note.
    let mut cx = unsafe { Cx::new(store, strings, defs(), &mut world) };
    let flying = cx.flight_missile(a.round, a.flight_class, a.life, a.speed);
    answer(if flying { Some(Shot::Round(a.round)) } else { None })
}

/// A command's answer: the round's kind and row, into the call block.
fn answer(shot: Option<Shot>) -> *const f64 {
    let s = sim();
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
pub struct JsWorld {
    store: *mut Store,
    strings: *mut Strings,
    /// The items' targets as last read (None: read them again before the
    /// next use), and their sides toward each team asked about.
    targets: Option<Vec<TargetState>>,
    relations: Vec<(f64, Vec<f64>)>,
}

impl JsWorld {
    /// The world over the store and strings `parts()` gives.
    pub(crate) fn new(store: *mut Store, strings: *mut Strings) -> Self {
        JsWorld { store, strings, targets: None, relations: Vec::new() }
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
        String::from_utf16_lossy(unsafe { &*self.strings }.units(id))
    }

    fn store(&self) -> &Store {
        // SAFETY: the module's note; the borrow ends before the import.
        unsafe { &*self.store }
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

fn hit_of(a: CastAnswer) -> Option<Hit> {
    a.found.then_some(Hit { distance: a.distance, point: a.point, normal: a.normal, terrain: a.terrain, body: a.body })
}

fn point_of(a: PointAnswer) -> Option<Vec3> {
    a.found.then_some(a.point)
}

impl World for JsWorld {
    fn cast(&mut self, origin: Vec3, direction: Vec3, far: f64, out: Out, ignore_body: Nullable<Ref<Body>>, static_only: Option<bool>) -> Option<Hit> {
        hit_of(import_cast(&CastArgs { origin, direction, far, behind: out == Out::Behind, ignore_body, static_only }))
    }

    fn cast_moving(&mut self, origin: Vec3, direction: Vec3, far: f64, out: Out, ignore_body: Nullable<Ref<Body>>) -> Option<Hit> {
        hit_of(import_cast_moving(&CastArgs { origin, direction, far, behind: out == Out::Behind, ignore_body, static_only: None }))
    }

    fn raycast_units(&mut self, origin: Vec3, direction: Vec3, far: f64, owner: OrdnanceOwner, history_tick: Option<f64>, ignore_body: Option<Ref<Body>>, mount: Option<Ref<Vehicle>>) -> Option<UnitHit> {
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
        let a = import_rider_mount(&BodyArgs { body });
        if !a.found {
            return None;
        }
        Some(RiderMount { vehicle: a.vehicle.expect("the mount's hull"), bound_radius: a.bound_radius, body: a.body })
    }

    fn fat_query(&mut self, center: Vec3, radius: f64, owner: OrdnanceOwner, struck: &RefSet<Unit>, ignore_body: Option<Ref<Body>>) -> Option<Vec<Ref<Unit>>> {
        let a = import_fat_query(&NearArgs { center, radius, owner, struck: struck.clone(), ignore_body });
        a.found.then_some(a.units)
    }

    fn unit_sphere(&mut self, unit: Ref<Unit>) -> Option<(Vec3, f64)> {
        let a = import_unit_sphere(&UnitArgs { unit });
        a.found.then_some((a.center, a.radius))
    }

    fn is_droid(&mut self, unit: Ref<Unit>) -> bool {
        import_is_droid(&UnitArgs { unit }).value
    }

    fn gone(&mut self, x: BeamSource) -> bool {
        if let Some(r) = crate::vehicles::module::f2::live_seats::emitter_lease(x) { return crate::vehicles::module::f2::live_seats::beam_gone(r); }
        // `x?.alive === false`: never for nobody or the player's own.
        if matches!(x, BeamSource::Null | BeamSource::Player) {
            return false;
        }
        import_gone(&GoneArgs { subject: x }).value
    }

    fn beam_local_origin(&mut self, source: BeamSource, origin: Vec3) -> Option<Vec3> {
        if let Some(r) = crate::vehicles::module::f2::live_seats::emitter_lease(source) { return crate::vehicles::module::f2::live_seats::beam_local(r, origin); }
        point_of(import_beam_local_origin(&SourceArgs { source, origin }))
    }

    fn beam_pose(&mut self, source: BeamSource, local_origin: Vec3, position: &mut Vec3, direction: &mut Vec3) {
        if let Some(r) = crate::vehicles::module::f2::live_seats::emitter_lease(source) { crate::vehicles::module::f2::live_seats::beam_pose(r, local_origin, position, direction); return; }
        let a = import_beam_pose(&BeamPoseArgs { source, local_origin, position: *position, direction: *direction });
        *position = a.position;
        *direction = a.direction;
    }

    fn shot_tick(&mut self, owner: OrdnanceOwner, channel: f64) -> Nullable<f64> {
        import_shot_tick(&ShotArgs { owner, channel }).value
    }

    fn history_tick(&mut self) -> f64 {
        import_history_tick(&NoArgs {}).value
    }

    fn shooter(&mut self, owner: OrdnanceOwner, channel: f64) -> Shooter {
        let a = import_shooter(&ShotArgs { owner, channel });
        Shooter { fire_id: a.fire_id, unit: a.unit, generation: a.generation }
    }

    fn missile_target(&mut self, target_id: f64, target_generation: f64) -> Option<Vec3> {
        point_of(import_missile_target(&TargetArgs { target_id, target_generation }))
    }

    fn navigate_missile(&mut self, position: Vec3, velocity: &mut Vec3, flight_class: Option<StrId>, target_id: f64, target_generation: f64, dt: f64) -> f64 {
        let a = import_navigate_missile(&SeekArgs { position, velocity: *velocity, flight_class, target_id, target_generation, dt });
        *velocity = a.velocity;
        a.target_id
    }

    fn water_crossing(&mut self, from: Vec3, to: Vec3) -> Option<Vec3> {
        if !water(0) {
            return None;
        }
        point_of(import_water_crossing(&SegmentArgs { from, to }))
    }

    fn waver(&mut self) -> f64 {
        import_waver(&NoArgs {}).value
    }

    fn spin(&mut self) -> f64 {
        import_spin(&NoArgs {}).value
    }

    fn explosion(&mut self, def: &Field, position: Vec3, owner: OrdnanceOwner) {
        // SAFETY: the module's note.
        let (def_tag, def) = field_words(def, unsafe { &mut *self.strings });
        import_explosion(&BlastArgs { def_tag, def, position, owner });
    }

    fn hit(&mut self, body: Ref<Body>, def: DefArg, owner: OrdnanceOwner, direction: Vec3) {
        let (def, beam, damage) = self.def_arg(def);
        import_hit(&StrikeArgs { body, def, beam, damage, owner, direction });
    }

    fn hit_unit(&mut self, unit: Ref<Unit>, def: DefArg, point: Point, owner: OrdnanceOwner, part: Nullable<HitPart>, direction: Vec3) {
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
        let (code, _) = shot_layout(shot);
        let args = ProjectileArgs { kind: code as u32, r#type: kind as u32 };
        self.hand(shot, ProjectileArgs::WORDS, |row, heap| args.put(row, heap), bfe_import_projectile);
    }

    fn launch(&mut self, bolt: Ref<Bolt>, tick: Nullable<f64>) {
        let args = LaunchArgs { kind: KIND_BOLT as u32, tick };
        self.hand(Shot::Bolt(bolt), LaunchArgs::WORDS, |row, heap| args.put(row, heap), bfe_import_launch);
    }

    fn impact(&mut self, shot: Shot, info: &ImpactInfo) {
        let (code, _) = shot_layout(shot);
        let args = match info {
            ImpactInfo::Plain { point, normal, surface } => ImpactArgs { kind: code as u32, point: *point, normal: *normal, surface: *surface, struck: None },
            ImpactInfo::Struck { impact, surface } => ImpactArgs { kind: code as u32, point: impact.point, normal: impact.normal, surface: *surface, struck: Some(impact.clone()) },
        };
        self.hand(shot, ImpactArgs::WORDS, |row, heap| args.put(row, heap), bfe_import_impact);
    }

    fn grenade_contact(&mut self, grenade: Ref<Grenade>) {
        let args = ContactArgs { kind: KIND_GRENADE as u32 };
        self.hand(Shot::Grenade(grenade), ContactArgs::WORDS, |row, heap| args.put(row, heap), bfe_import_grenade_contact);
    }

    fn bounce(&mut self, def: DefId, position: Vec3, speed: f64) {
        import_bounce(&BounceArgs { def, position, speed });
    }

    // The things lying about (ItemsWorld).

    fn item_cast(&mut self, origin: Vec3, direction: Vec3, far: f64) -> Option<Hit> {
        hit_of(import_item_cast(&ItemCastArgs { origin, direction, far }))
    }

    fn blocked(&mut self, position: Vec3, target: Vec3) -> bool {
        import_blocked(&BlockedArgs { position, target }).value
    }

    fn item_water_crossing(&mut self, from: Vec3, to: Vec3) -> Option<Vec3> {
        if !water(1) {
            return None;
        }
        point_of(import_item_water_crossing(&SegmentArgs { from, to }))
    }

    fn item_model(&mut self, def: DefId) -> ItemModel {
        let a = import_item_model(&ItemDefArgs { def });
        ItemModel { mesh: a.mesh, radius: a.radius, center: a.center }
    }

    fn owner_key(&mut self, owner: ItemOwner) -> String {
        let key = import_owner_key(&OwnerArgs { owner }).key;
        self.string(key)
    }

    fn drop_roll(&mut self) -> f64 {
        import_drop_roll(&NoArgs {}).value
    }

    fn explode(&mut self, def: &Field, position: Vec3, owner: ItemOwner) {
        // SAFETY: the module's note.
        let (def_tag, def) = field_words(def, unsafe { &mut *self.strings });
        import_explode(&ItemBlastArgs { def_tag, def, position, owner });
        // An explosion moves what it reaches: the targets are read again.
        self.targets = None;
    }

    fn watch(&mut self, index: usize, team: f64) -> Watched {
        let relation = self.relation(index, team);
        let t = self.target(index);
        Watched { relation, center: t.center, radius: t.radius, velocity: t.velocity }
    }

    fn standing(&mut self, index: usize) -> Vec3 {
        self.target(index).position
    }

    fn give(&mut self, index: usize, def: DefId) -> bool {
        import_give(&GiveArgs { index: index as u32, def }).value
    }

    fn pickup(&mut self, def: DefId, position: Vec3) {
        import_pickup(&PickupArgs { def, position });
    }
}
