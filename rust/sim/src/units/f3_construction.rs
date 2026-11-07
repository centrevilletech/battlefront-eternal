//! UnitFactory and player-class activation on the canonical F3 store.
//! Cold preparation transfers complete native buffers. The live operation
//! assigns identity and rebinds services without allocating or calling JS.
use super::{construction::{Construction, Prepared, Realized}, f3, module};
use crate::{abi::sim, gen::*, store::{Ref, StrId}};
use std::ptr::addr_of_mut;

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
fn factory() -> &'static mut Factory {
    // One battle thread. No table borrow crosses a native lifecycle service.
    unsafe { (*addr_of_mut!(super::owner::UNITS.factory)).as_mut().expect("prepared unit factory") }
}
pub(crate) fn reset() {
    unsafe { *addr_of_mut!(super::owner::UNITS.factory) = Some(Factory::default()); }
}

/// Each construction cookie admits at most one body. Resident cookies can
/// replace an existing member; counting them too keeps a conservative cold
/// bound without reserving history shapes for every unused numeric slot.
pub(crate) fn prepared_count()->usize { factory().resources.len() }

/// Future mixed ids include current hull/prop ids and all admitted bodies.
/// Reserve arrays now; preparing an inactive body does not consume an id.
#[no_mangle]
pub extern "C" fn f3_factory_reserve(limit: u32) {
    let limit=limit as usize;
    let f=factory();
    f3::with_rows(|store, owner| f.construction.reserve(store,owner,limit));
    f.limit=f.limit.max(limit);
    f.residents.resize_with(f.limit,||None);
    f3::reserve_native_slots(f.limit);
    module::reserve_bodies(f.limit);
    crate::vehicles::module::f2::reserve_registry(f.limit);
    f.players.reserve(f.limit.saturating_sub(f.players.len()));
    crate::recon::module::reserve_unit_slots(f.limit);
    crate::vehicles::module::f2::live_seats::reserve_unit_slots(f.limit);
}

/// UnitFactory.build completes ordinary adoption before staging. Its source
/// id is temporary and absent from the mixed registry; reuse it after taking
/// both the canonical row and every private body buffer out of that slot.
#[no_mangle]
pub extern "C" fn f3_factory_stage() {
    let (row,heap)=sim().calls.args();
    let input=F3ColdStage::take(row,heap);
    let source=input.adoption.key.reference();
    input.adoption.key.validate(&sim().store).expect("current cold construction row");
    let resources=Resources {
        body:module::take_body(source.slot),
        rider:crate::vehicles::module::f2::riders::take(source),
        publication:f3::take_publication(source),
        health:f3::health_scale(source.slot),
        bindings:f3::take_bindings(source.slot),
        adoption:input.adoption,
    };
    let unit=sim().store.unit.remove(source).expect("completed cold body");
    let f=factory();
    let cookie=input.cookie as usize;
    if f.resources.len()<=cookie {f.resources.resize_with(cookie+1,||None);}
    assert!(f.resources[cookie].is_none(),"one prepared body per cookie");
    f.resources[cookie]=Some(resources);
    f3::with_rows(|_,owner| {
        owner.withdraw(source);
        f.construction.prepare(owner,Prepared {cookie:input.cookie,source,unit,local:None});
    });
    f3::discard_publication(source);
    f3::reserve_history();
}

// A birth record contains numeric construction identity only. It can bind the
// retained Unit facade before any later Character or attacker ref is decoded.
fn activate(result: Realized, retired_cookie:Option<u32>) -> Ref<Unit> {
    let reference=result.key.reference();
    if let Some((cookie,_))=result.prepared {
        let resources=factory().resources[cookie as usize].take().expect("cold body services");
        module::install_body(reference,resources.body);
        crate::vehicles::module::f2::riders::install(reference,resources.rider);
        let water=resources.adoption.droideka_water.clone();
        f3::install_native_slot(reference,resources.adoption.category,resources.health,
            resources.bindings,water,resources.publication);
        crate::vehicles::module::f2::live_seats::adopt_unit(reference,
            resources.adoption.has_actor,resources.adoption.no_enter);
        factory().residents[reference.slot as usize]=Some(Resident {cookie,adoption:resources.adoption});
        F3UnitBirth {key:result.key,cookie,replacement:result.replacement,retired_cookie,
            order:sim().store.units.list.len() as u32}.emit(&mut sim().events);
    }
    f3::changed(reference);
    reference
}

/// Native UnitFactory.acquire. A missing prepared class is a construction
/// error, never permission to reenter a JavaScript factory from the phase.
pub(crate) fn cold(class: StrId) -> Option<Ref<Unit>> {
    let result=f3::with_rows(|store,owner| factory().acquire(store,owner,class));
    result.ok().map(|result|activate(result,None))
}

/// PlayerBodies.adopt preserves a local Unit; a connection selects its own
/// canonical class pool. The caller performs removal before changing classes,
/// then resets/spawns this result at the original lifecycle observation point.
pub(crate) fn realize(request: &F3PlayerRealization, mut services: f3::WorldServices<'_>) -> Option<Ref<Unit>> {
    let result=f3::with_rows(|store,owner| factory().realize(store,owner,request)).ok()?;
    let retired_cookie=if result.replacement {
        let reference=result.key.reference();
        let old=factory().residents[reference.slot as usize].take().expect("resident viewpoint class");
        let cookie=old.cookie;
        factory().resources[cookie as usize]=Some(Resources {
            body:module::take_body(reference.slot),adoption:old.adoption,
            rider:crate::vehicles::module::f2::riders::take(reference),
            publication:f3::take_publication(reference),
            health:f3::health_scale(reference.slot),bindings:f3::take_bindings(reference.slot),
        });
        f3::with_rows(|_,owner|factory().construction.recycle_local(owner,cookie,reference));
        Some(cookie)
    } else {None};
    let reference=activate(result,retired_cookie);
    // A new pooled body can be observed by AI roster queries before it ever
    // aims. Bind its generation at realization, while the native loan exists.
    if let Some(ai)=services.ai.as_mut() {
        if !ai.unit_born(&sim().store,reference) { return None; }
    }
    let m=&request.metadata;
    let metadata=F3Metadata {character:m.character,controller:m.controller,team:m.team,
        life:sim().store.unit.get(reference).unwrap().generation,
        human:m.human,network_human:m.network_human,hero:m.hero};
    f3::bind_native_life(reference,&metadata,request.combat_random,request.weapon_random);
    if let Some(key)=request.recon_key {
        crate::recon::module::rebind_native(reference,key,request.recon_seed,&mut services);
    }
    Some(reference)
}

/// HumanSpawn and NetworkWorld.spawn keep one admission per Character.
/// Registering its streams is cold work; native class selection only reads
/// handles, including an absent lazy stream that has not drawn yet.
#[no_mangle]
pub extern "C" fn f3_factory_player_bind() {
    let (row,heap)=sim().calls.args();
    let input=F3PlayerBinding::take(row,heap);
    assert!(sim().store.character.get(input.character).is_some(),"admitted Character");
    factory().bind_player(&sim().store,input);
}

/// spawnUnit's human branch and NetworkWorld.spawn both return a spawned
/// body. A connection changes pooled bodies; the viewpoint retains its Unit.
/// Removal precedes acquisition so the old body's corpse enters its pool at
/// the original observation point, before the new body's spawn notices.
pub(crate) fn player(character: Ref<Character>, previous: Option<Ref<Unit>>, class: StrId,
    position:[f64;3], yaw:f64, local:bool, mut services:f3::WorldServices<'_>) -> Option<Ref<Unit>> {
    let binding=factory().players.iter().find(|row|row.character==character && row.local==local)?;
    let old=previous.or(binding.body);
    let (combat_random,weapon_random,recon_key,recon_seed)=
        (binding.combat_random,binding.weapon_random,binding.recon_key,binding.recon_seed);
    let previous=match old {Some(reference)=>Some(F3UnitKey::of(reference,sim().store.unit.get(reference)?)),None=>None};
    // Validate stock before removing a connection's existing body. A cold
    // capacity error must leave the old life and its registry entry intact.
    if let Some(key)=&previous {
        let old=key.validate(&sim().store).ok()?;
        if old.class_name!=class {
            let available=f3::with_rows(|store,owner| {
                let c=&factory().construction;
                if local {c.local_available(class,key.reference())} else {c.can_acquire(store,owner,class)}
            });
            if !available {return None;}
            if !local {f3::remove_native(key.reference(),services.reborrow());}
        }
    }
    let c=sim().store.character.get(character)?;
    let metadata=F3Metadata {character:Some(character),controller:None,team:c.team,
        life:previous.as_ref().map_or(0.,|key|key.life),human:true,
        network_human:if local {None}else{Some(true)},hero:c.hero==Some(true)};
    let request=F3PlayerRealization {class_name:class,previous,local,metadata,
        combat_random,weapon_random,recon_key,recon_seed};
    let reference=realize(&request,services.reborrow())?;
    sim().store.unit.get_mut(reference).unwrap().combat.player=true;
    f3::spawn_native(reference,character,position,yaw,services.reborrow());
    // PlayerCombat.reset creates fresh weapon state. Rebind after that reset,
    // preserving the connection's spread stream rather than the class stock's.
    let mut metadata=request.metadata;
    metadata.life=sim().store.unit.get(reference).unwrap().generation;
    f3::bind_native_life(reference,&metadata,combat_random,weapon_random);
    factory().players.iter_mut().find(|row|row.character==character).unwrap().body=Some(reference);
    Some(reference)
}

/// Deferred old rows may be dropped only after all ordered readers return.
#[no_mangle]
pub extern "C" fn f3_factory_finish() { factory().construction.finish(); }

/// The ordinary external caller joins the same event/publication barrier as
/// native F4 callers; neither path imports another body to answer acquisition.
#[no_mangle]
pub extern "C" fn f3_factory_acquire(class:u32)->i32 {
    let _outer=f3::external();
    let class=StrId(class);
    let reference=f3::acquire_native(class).or_else(|| f3::with_services(|services|
        f3::construct_native(class,f3::required_services!(services,None))));
    reference.map_or(-1,|unit|unit.slot as i32)
}
#[no_mangle]
pub extern "C" fn f3_factory_realize()->i32 {
    let (row,heap)=sim().calls.args();
    let request=F3PlayerRealization::take(row,heap);
    let _outer=f3::external();
    f3::with_services(|services| f3::realize_native(&request,f3::required_services!(services,None)))
        .map_or(-1,|unit|unit.slot as i32)
}

/// The external lifecycle command is also a fixture entry for F4's native
/// player factory. It joins the same sparse publication and notice barrier.
#[no_mangle]
pub extern "C" fn f3_factory_player_spawn()->i32 {
    let (row,heap)=sim().calls.args();
    let request=F3PlayerSpawn::take(row,heap);
    let _outer=f3::external();
    f3::with_services(|services| f3::realize_player_native(request.character,
        request.previous,request.class_name,request.position,request.yaw,request.local,
        f3::required_services!(services,None)))
        .map_or(-1,|unit|unit.slot as i32)
}

/// The viewpoint's initial body is already live but outside UnitFactory.
/// Register its retained class resources so later local class changes cache
/// it just like PlayerBodies.bundles, without exposing it to connection pools.
#[no_mangle]
pub extern "C" fn f3_factory_resident() {
    let (row,heap)=sim().calls.args();
    let input=F3ColdStage::take(row,heap);
    let slot=input.adoption.key.slot as usize;
    input.adoption.key.validate(&sim().store).expect("resident viewpoint body");
    let f=factory();
    if f.residents.len()<=slot {f.residents.resize_with(slot+1,||None);}
    let cookie=input.cookie as usize;
    if f.resources.len()<=cookie {f.resources.resize_with(cookie+1,||None);}
    let class=sim().store.unit.get(input.adoption.key.reference()).unwrap().class_name;
    f3::with_rows(|_,owner|owner.reserve_class(class,f.limit));
    f.residents[slot]=Some(Resident {cookie:input.cookie,adoption:input.adoption});
    f3::reserve_history();
}

impl Factory {
    // UnitFactory.acquire/PlayerBodies.adopt return a realization before any
    // rig installation, lifecycle hook or journal notice. Construction and
    // Ownership are borrowed together only while assigning the canonical id.
    fn acquire(&mut self, store: &mut Store, owner: &mut super::ownership::Ownership, class: StrId)
        -> Result<Realized, super::construction::Error> {
        self.construction.acquire(store, owner, class)
    }
    fn realize(&mut self, store: &mut Store, owner: &mut super::ownership::Ownership, request: &F3PlayerRealization)
        -> Result<Realized, super::construction::Error> {
        self.construction.realize(store, owner, request.class_name, request.previous.clone(), request.local)
    }
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
