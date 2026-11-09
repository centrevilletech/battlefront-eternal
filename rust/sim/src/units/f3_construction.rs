//! UnitFactory and player-class activation on the canonical F3 store.
//! Cold preparation transfers complete native buffers. The live operation
//! assigns identity and rebinds services without allocating or calling JS.
use super::{construction::{Construction, Prepared, Realized}, f3, module};
use crate::{gen::*, store::{Ref, StrId}};

struct Resources {
    body: module::PreparedBody,
    rider: Option<crate::vehicles::module::f2::riders::Rider>,
    publication:crate::vehicles::f2_delta::Cache,
    adoption: F3Adoption,
    health: crate::combat::impacts::HealthScale,
    bindings: Vec<u32>,
}
struct Resident {cookie:u32, adoption:F3Adoption}
#[derive(Default)]
pub(super) struct Factory {
    construction: Construction,
    resources: Vec<Option<Resources>>,
    players: Vec<F3PlayerBinding>,
    residents: Vec<Option<Resident>>,
    limit: usize,
}
pub(crate) fn reset_in(world_ctx: &mut crate::world::owner::World) {
    { world_ctx.units.factory = Some(Factory::default()); }
}

/// Each construction cookie admits at most one body. Resident cookies can
/// replace an existing member; counting them too keeps a conservative cold
/// bound without reserving history shapes for every unused numeric slot.
pub(crate) fn prepared_count_in(units: &super::owner::Units)->usize { units.factory.as_ref().expect("prepared unit service").resources.len() }
/// The same count before attachF3 made the factory: no cookie handed out yet.
pub(crate) fn cookies_so_far_in(units: &super::owner::Units)->usize { units.factory.as_ref().map_or(0, |f| f.resources.len()) }

/// Future mixed ids include current hull/prop ids and all admitted bodies.
/// Reserve arrays now; preparing an inactive body does not consume an id.

pub fn f3_factory_reserve_in(world_ctx: &mut crate::world::owner::World, limit: u32) {
    let limit=limit as usize;
    let f=world_ctx.units.factory.as_mut().expect("prepared unit service");
    f3::with_rows_in(&mut world_ctx.sim.store, world_ctx.units.state.as_mut().expect("prepared unit service"), |store, owner| f.construction.reserve(store,owner,limit));
    f.limit=f.limit.max(limit);
    f.residents.resize_with(f.limit,||None);
    let limit = f.limit;
    f.players.reserve(limit.saturating_sub(f.players.len()));
    f3::reserve_native_slots_in(world_ctx, limit);
    module::reserve_bodies_in(&mut world_ctx.units, limit);
    crate::vehicles::module::f2::reserve_registry_in(world_ctx, limit);
    crate::recon::module::reserve_unit_slots_in(world_ctx, limit);
    crate::vehicles::module::f2::live_seats::reserve_unit_slots_in(world_ctx, limit);
}

/// UnitFactory.build completes ordinary adoption before staging. Its source
/// id is temporary and absent from the mixed registry; reuse it after taking
/// both the canonical row and every private body buffer out of that slot.

pub fn f3_factory_stage_in(world_ctx: &mut crate::world::owner::World) {
    let (row,heap)=world_ctx.sim.calls.args();
    let input=F3ColdStage::take(row,heap);
    let source=input.adoption.key.reference();
    input.adoption.key.validate(&world_ctx.sim.store).expect("current cold construction row");
    let resources=Resources {
        body:module::take_body_in(&mut world_ctx.units, source.slot),
        rider:crate::vehicles::module::f2::riders::take_in(world_ctx, source),
        publication:f3::take_publication_in(&mut world_ctx.units, source),
        health:f3::health_scale_in(&world_ctx.units, source.slot),
        bindings:f3::take_bindings_in(&mut world_ctx.units, source.slot),
        adoption:input.adoption,
    };
    let unit=world_ctx.sim.store.unit.remove(source).expect("completed cold body");
    let f=world_ctx.units.factory.as_mut().expect("prepared unit service");
    let cookie=input.cookie as usize;
    if f.resources.len()<=cookie {f.resources.resize_with(cookie+1,||None);}
    assert!(f.resources[cookie].is_none(),"one prepared body per cookie");
    f.resources[cookie]=Some(resources);
    f3::with_rows_in(&mut world_ctx.sim.store, world_ctx.units.state.as_mut().expect("prepared unit service"), |_,owner| {
        owner.withdraw(source);
        f.construction.prepare(owner,Prepared {cookie:input.cookie,source,unit,local:None});
    });
    f3::discard_publication_in(world_ctx, source);
    f3::reserve_history_in(world_ctx);
}

// A birth record contains numeric construction identity only. It can bind the
// retained Unit facade before any later Character or attacker ref is decoded.
fn activate_in(world_ctx: &mut crate::world::owner::World, result: Realized, retired_cookie:Option<u32>) -> Ref<Unit> {
    let reference=result.key.reference();
    if let Some((cookie,_))=result.prepared {
        let resources=world_ctx.units.factory.as_mut().expect("prepared unit service").resources[cookie as usize].take().expect("cold body services");
        module::install_body_in(&mut world_ctx.units, reference,resources.body);
        crate::vehicles::module::f2::riders::install_in(world_ctx, reference,resources.rider);
        let water=resources.adoption.droideka_water.clone();
        f3::install_native_slot_in(&mut world_ctx.units, reference,resources.adoption.category,resources.health,
            resources.bindings,water,resources.publication);
        crate::vehicles::module::f2::live_seats::adopt_unit_in(world_ctx, reference,
            resources.adoption.has_actor,resources.adoption.no_enter);
        world_ctx.units.factory.as_mut().expect("prepared unit service").residents[reference.slot as usize]=Some(Resident {cookie,adoption:resources.adoption});
        F3UnitBirth {key:result.key,cookie,replacement:result.replacement,retired_cookie,
            order:world_ctx.sim.store.units.list.len() as u32}.emit(&mut world_ctx.sim.events);
    }
    f3::changed_in(world_ctx, reference);
    reference
}

/// Native UnitFactory.acquire. A missing prepared class is a construction
/// error, never permission to reenter a JavaScript factory from the phase.
pub(crate) fn cold_in(world_ctx: &mut crate::world::owner::World, class: StrId, mut services:f3::WorldServices<'_>) -> Option<Ref<Unit>> {
    let result=f3::with_rows_in(&mut world_ctx.sim.store, world_ctx.units.state.as_mut().expect("prepared unit service"), |store,owner| world_ctx.units.factory.as_mut().expect("prepared unit service").construction.acquire(store,owner,class,
        |reference|services.ai.as_mut().is_none_or(|ai|ai.unit_born(reference))));
    result.ok().map(|result|activate_in(world_ctx, result,None))
}

/// PlayerBodies.adopt preserves a local Unit; a connection selects its own
/// canonical class pool. The caller performs removal before changing classes,
/// then resets/spawns this result at the original lifecycle observation point.
pub(crate) fn realize_in(world_ctx: &mut crate::world::owner::World, request: &F3PlayerRealization, mut services: f3::WorldServices<'_>) -> Option<Ref<Unit>> {
    let result=f3::with_rows_in(&mut world_ctx.sim.store, world_ctx.units.state.as_mut().expect("prepared unit service"), |store,owner| world_ctx.units.factory.as_mut().expect("prepared unit service").construction.realize(store,owner,request.class_name,request.previous.clone(),request.local,
        |reference|services.ai.as_mut().is_none_or(|ai|ai.unit_born(reference)))).ok()?;
    let retired_cookie=if result.replacement {
        let reference=result.key.reference();
        let old=world_ctx.units.factory.as_mut().expect("prepared unit service").residents[reference.slot as usize].take().expect("resident viewpoint class");
        let cookie=old.cookie;
        world_ctx.units.factory.as_mut().expect("prepared unit service").resources[cookie as usize]=Some(Resources {
            body:module::take_body_in(&mut world_ctx.units, reference.slot),adoption:old.adoption,
            rider:crate::vehicles::module::f2::riders::take_in(world_ctx, reference),
            publication:f3::take_publication_in(&mut world_ctx.units, reference),
            health:f3::health_scale_in(&world_ctx.units, reference.slot),bindings:f3::take_bindings_in(&mut world_ctx.units, reference.slot),
        });
        f3::with_rows_in(&mut world_ctx.sim.store, world_ctx.units.state.as_mut().expect("prepared unit service"), |_,owner|world_ctx.units.factory.as_mut().expect("prepared unit service").construction.recycle_local(owner,cookie,reference));
        Some(cookie)
    } else {None};
    let reference=activate_in(world_ctx, result,retired_cookie);
    let m=&request.metadata;
    let metadata=F3Metadata {character:m.character,controller:m.controller,team:m.team,
        life:world_ctx.sim.store.unit.get(reference).unwrap().generation,
        human:m.human,network_human:m.network_human,hero:m.hero};
    f3::bind_native_life_in(world_ctx, reference,&metadata,request.combat_random,request.weapon_random);
    if let Some(key)=request.recon_key {
        crate::recon::module::rebind_native_in(world_ctx, reference,key,request.recon_seed,&mut services);
    }
    Some(reference)
}

/// HumanSpawn and NetworkWorld.spawn keep one admission per Character.
/// Registering its streams is cold work; native class selection only reads
/// handles, including an absent lazy stream that has not drawn yet.

pub fn f3_factory_player_bind_in(world_ctx: &mut crate::world::owner::World) {
    let (row,heap)=world_ctx.sim.calls.args();
    let input=F3PlayerBinding::take(row,heap);
    assert!(world_ctx.sim.store.character.get(input.character).is_some(),"admitted Character");
    world_ctx.units.factory.as_mut().expect("prepared unit service").bind_player(&world_ctx.sim.store,input);
}

/// spawnUnit's human branch and NetworkWorld.spawn both return a spawned
/// body. A connection changes pooled bodies; the viewpoint retains its Unit.
/// Removal precedes acquisition so the old body's corpse enters its pool at
/// the original observation point, before the new body's spawn notices.
pub(crate) fn player_in(world_ctx: &mut crate::world::owner::World, character: Ref<Character>, previous: Option<Ref<Unit>>, class: StrId,
    position:[f64;3], yaw:f64, local:bool, mut services:f3::WorldServices<'_>) -> Option<Ref<Unit>> {
    let binding=world_ctx.units.factory.as_mut().expect("prepared unit service").players.iter().find(|row|row.character==character && row.local==local)?;
    let old=previous.or(binding.body);
    let (combat_random,weapon_random,recon_key,recon_seed)=
        (binding.combat_random,binding.weapon_random,binding.recon_key,binding.recon_seed);
    let previous=match old {Some(reference)=>Some(F3UnitKey::of(reference,world_ctx.sim.store.unit.get(reference)?)),None=>None};
    // Validate stock before removing a connection's existing body. A cold
    // capacity error must leave the old life and its registry entry intact.
    if let Some(key)=&previous {
        let old=key.validate(&world_ctx.sim.store).ok()?;
        if old.class_name!=class {
            let available=f3::with_rows_in(&mut world_ctx.sim.store, world_ctx.units.state.as_mut().expect("prepared unit service"), |store,owner| {
                let c=&world_ctx.units.factory.as_mut().expect("prepared unit service").construction;
                if local {c.local_available(class,key.reference())} else {c.can_acquire(store,owner,class)}
            });
            if !available {return None;}
            if !local {f3::remove_native_in(world_ctx, key.reference(),services.reborrow());}
        }
    }
    let c=world_ctx.sim.store.character.get(character)?;
    let metadata=F3Metadata {character:Some(character),controller:None,team:c.team,
        life:previous.as_ref().map_or(0.,|key|key.life),human:true,
        network_human:if local {None}else{Some(true)},hero:c.hero==Some(true)};
    let request=F3PlayerRealization {class_name:class,previous,local,metadata,
        combat_random,weapon_random,recon_key,recon_seed};
    let reference=realize_in(world_ctx, &request,services.reborrow())?;
    world_ctx.sim.store.unit.get_mut(reference).unwrap().combat.player=true;
    f3::spawn_native_in(world_ctx, reference,character,position,yaw,services.reborrow());
    // PlayerCombat.reset creates fresh weapon state. Rebind after that reset,
    // preserving the connection's spread stream rather than the class stock's.
    let mut metadata=request.metadata;
    metadata.life=world_ctx.sim.store.unit.get(reference).unwrap().generation;
    f3::bind_native_life_in(world_ctx, reference,&metadata,combat_random,weapon_random);
    world_ctx.units.factory.as_mut().expect("prepared unit service").players.iter_mut().find(|row|row.character==character).unwrap().body=Some(reference);
    Some(reference)
}

/// Deferred old rows may be dropped only after all ordered readers return.

pub fn f3_factory_finish_in(units: &mut super::owner::Units) { units.factory.as_mut().expect("prepared unit service").construction.finish(); }

/// The ordinary external caller joins the same event/publication barrier as
/// native F4 callers; neither path imports another body to answer acquisition.

pub fn f3_factory_acquire_in(world_ctx: &mut crate::world::owner::World, class:u32)->i32 {
    let boundary=f3::external_in(world_ctx);
    let result = (|| {
    let class=StrId(class);
    let reference=f3::acquire_native_in(world_ctx, class).or_else(|| f3::with_services_in(world_ctx, |world_ctx, services|
        f3::construct_native_in(world_ctx, class,f3::required_services!(&mut world_ctx.units; services,None))));
    reference.map_or(-1,|unit|unit.slot as i32)

    })();
    boundary.finish(world_ctx, true);
    result
}

pub fn f3_factory_realize_in(world_ctx: &mut crate::world::owner::World)->i32 {
    let (row,heap)=world_ctx.sim.calls.args();
    let request=F3PlayerRealization::take(row,heap);
    let boundary=f3::external_in(world_ctx);
    let result = (|| {
    f3::with_services_in(world_ctx, |world_ctx, services| f3::realize_native_in(world_ctx, &request,f3::required_services!(&mut world_ctx.units; services,None)))
        .map_or(-1,|unit|unit.slot as i32)

    })();
    boundary.finish(world_ctx, true);
    result
}

/// The external lifecycle command is also a fixture entry for F4's native
/// player factory. It joins the same sparse publication and notice barrier.

pub fn f3_factory_player_spawn_in(world_ctx: &mut crate::world::owner::World)->i32 {
    let (row,heap)=world_ctx.sim.calls.args();
    let request=F3PlayerSpawn::take(row,heap);
    let boundary=f3::external_in(world_ctx);
    let result = (|| {
    f3::with_services_in(world_ctx, |world_ctx, services| f3::realize_player_native_in(world_ctx, request.character,
        request.previous,request.class_name,request.position,request.yaw,request.local,
        f3::required_services!(&mut world_ctx.units; services,None)))
        .map_or(-1,|unit|unit.slot as i32)

    })();
    boundary.finish(world_ctx, true);
    result
}

/// The viewpoint's initial body is already live but outside UnitFactory.
/// Register its retained class resources so later local class changes cache
/// it just like PlayerBodies.bundles, without exposing it to connection pools.

pub fn f3_factory_resident_in(world_ctx: &mut crate::world::owner::World) {
    let (row,heap)=world_ctx.sim.calls.args();
    let input=F3ColdStage::take(row,heap);
    let slot=input.adoption.key.slot as usize;
    input.adoption.key.validate(&world_ctx.sim.store).expect("resident viewpoint body");
    let f=world_ctx.units.factory.as_mut().expect("prepared unit service");
    if f.residents.len()<=slot {f.residents.resize_with(slot+1,||None);}
    let cookie=input.cookie as usize;
    if f.resources.len()<=cookie {f.resources.resize_with(cookie+1,||None);}
    let class=world_ctx.sim.store.unit.get(input.adoption.key.reference()).unwrap().class_name;
    f3::with_rows_in(&mut world_ctx.sim.store, world_ctx.units.state.as_mut().expect("prepared unit service"), |_,owner|owner.reserve_class(class,f.limit));
    f.residents[slot]=Some(Resident {cookie:input.cookie,adoption:input.adoption});
    f3::reserve_history_in(world_ctx);
}

impl Factory {
    fn bind_player(&mut self, store: &Store, input: F3PlayerBinding) {

        assert!(store.character.get(input.character).is_some(),"admitted Character");
        let rows=&mut self.players;
        if let Some(row)=rows.iter_mut().find(|row|row.character==input.character) {
            *row=input;
        } else {
            assert!(rows.len()<rows.capacity(),"reserved player admissions");
            rows.push(input);
        }
    }
}

// The WASM boundary owns addresses; shared services borrow their owners.
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    #[no_mangle]
    pub extern "C" fn f3_factory_reserve(limit: u32) { f3_factory_reserve_in(crate::abi::facade::world(), limit) }
    #[no_mangle]
    pub extern "C" fn f3_factory_stage() { f3_factory_stage_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f3_factory_player_bind() { f3_factory_player_bind_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f3_factory_finish() { f3_factory_finish_in(&mut crate::abi::facade::world().units) }
    #[no_mangle]
    pub extern "C" fn f3_factory_acquire(class:u32)->i32 { f3_factory_acquire_in(crate::abi::facade::world(), class) }
    #[no_mangle]
    pub extern "C" fn f3_factory_realize()->i32 { f3_factory_realize_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f3_factory_player_spawn()->i32 { f3_factory_player_spawn_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f3_factory_resident() { f3_factory_resident_in(crate::abi::facade::world()) }
}
