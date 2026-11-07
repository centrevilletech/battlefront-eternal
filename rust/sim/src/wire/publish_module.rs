//! A publication names sibling banks by qualified instance-local identities.
//! Each call resolves them once and passes disjoint field borrows to the
//! single publisher. No session retains a Rust address or borrows JavaScript.
use super::{publisher::{Publisher,Personal},host::Host,Runtime,shots::Counters,store_source::{Source,PublicationWorld},stagger::Stagger,combat::Capture};
pub struct Session {publisher:Publisher,runtime:usize,source:usize,stagger:usize,shots:usize,host:usize,combat:usize}
pub(crate) unsafe fn wire_publish_create_in(w: &mut crate::world::owner::World, runtime:usize,source:usize,stagger:usize,shots:usize,host:usize,combat:usize) ->usize {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
    let (runtime,source,stagger,shots,host,combat)=(runtime as usize,source as usize,stagger as usize,shots as usize,host as usize,combat as usize);
    if owner.runtimes.get(runtime).is_none()||owner.sources.get(source).is_none()||owner.staggers.get(stagger).is_none()
        ||owner.shots.get(shots).is_none()||owner.hosts.get(host).is_none()||owner.combat.get(combat).is_none() {return 0;}
    owner.publications.insert(Session {publisher:Publisher::default(),runtime,source,stagger,shots,host,combat}).unwrap_or(0)

}
pub(crate) unsafe fn wire_publish_bind_in(w: &mut crate::world::owner::World, session:usize,client:u32,peer:u32,edges:usize) ->u32 {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
    let Some(s)=owner.publications.get_mut(session as usize) else{return 0};
    let Some(host)=owner.hosts.get_mut(s.host) else{return 0};
    if host.clients.len()>256||owner.edges.get(edges as usize).is_none() {return 0;}
    let Some(client)=host.clients.iter_mut().find(|c|c.id==client) else{return 0};
    let Ok(personal)=Personal::new(peer,edges as usize) else{return 0};client.publication=Some(personal);
    s.publisher.admit(host.clients.len());1

}
pub(crate) unsafe fn wire_publish_in(w: &mut crate::world::owner::World, session:usize) -> Option<&[f64]> {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
    use crate::gen::WireWorldFacts as F;
    let Some(s)=owner.publications.get_mut(session as usize) else{return None};
    let (Some(source),Some(stagger),Some(shots),Some(host),Some(combat),Some(runtime))=(
        owner.sources.get_mut(s.source),owner.staggers.get_mut(s.stagger),owner.shots.get(s.shots),
        owner.hosts.get_mut(s.host),owner.combat.get_mut(s.combat),owner.runtimes.get_mut(s.runtime)) else{return None};
    let facts=source.publication_clock();
    let mut world=PublicationWorld {w: super::store_source::Read {sim:&w.sim,input:&w.input,units:&w.units,fleet:&w.fleet,battle:&w.battle,combat:&w.combat,collision:&mut w.collision,movement:&mut w.movement},epoch:host.epoch,source,stagger,shots,combat,edges:&owner.edges};
    s.publisher.run(host,runtime,&mut world,facts[F::TICK],facts[F::AT],facts[F::UNIX_MS],facts[F::DROPPED_TICKS]);
    Some(&s.publisher.block.words[..])

}
pub(crate) unsafe fn wire_publish_drop_in(w: &mut crate::world::owner::World, session:usize) {super::owner::with_in(w, |owner|{owner.publications.remove(session as usize);});}
/// Occupancy observes retained banks without changing capture or ACK state.
pub(crate) unsafe fn wire_publish_load_in(w: &mut crate::world::owner::World, session:usize) -> Option<&[f64]> {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
    let Some(s)=owner.publications.get(session as usize) else{return None};
    let (Some(host),Some(runtime))=(owner.hosts.get(s.host),owner.runtimes.get(s.runtime)) else{return None};
    let load=s.publisher.load(host,runtime);
    w.sim.calls.result(crate::gen::WireLoad::WORDS,|row,heap|load.put(row,heap));Some(&w.sim.calls.block.words[..])

}

// The WASM boundary owns addresses; shared services borrow their owners.
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    #[no_mangle]
    pub unsafe extern "C" fn wire_publish_create(runtime:*mut Runtime,source:*mut Source,stagger:*mut Stagger,shots:*const Counters,host:*mut Host,combat:*mut Capture) ->*mut Session { wire_publish_create_in(crate::abi::facade::world(), runtime as usize, source as usize, stagger as usize, shots as usize, host as usize, combat as usize) as *mut Session }
    #[no_mangle]
    pub unsafe extern "C" fn wire_publish_bind(session:*mut Session,client:u32,peer:u32,edges:*const super::super::edges::Host) ->u32 { wire_publish_bind_in(crate::abi::facade::world(), session as usize, client, peer, edges as usize) }
    #[no_mangle]
    pub unsafe extern "C" fn wire_publish(session:*mut Session) ->*const f64 { let w = crate::abi::facade::world(); if wire_publish_in(w, session as usize).is_none() { return std::ptr::null() } w.wire.as_deref_mut().unwrap().publications.get_mut(session as usize).unwrap().publisher.descriptor() }
    #[no_mangle]
    pub unsafe extern "C" fn wire_publish_drop(session:*mut Session) { wire_publish_drop_in(crate::abi::facade::world(), session as usize) }
    #[no_mangle]
    pub unsafe extern "C" fn wire_publish_load(session:*const Session) ->*const f64 { wire_publish_load_in(crate::abi::facade::world(), session as usize).map_or(std::ptr::null(), |b| b.as_ptr()) }
}
