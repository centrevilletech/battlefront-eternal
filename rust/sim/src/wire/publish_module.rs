//! A publication names sibling banks by qualified instance-local identities.
//! Each call resolves them once and passes disjoint field borrows to the
//! single publisher. No session retains a Rust address or borrows JavaScript.
use super::{publisher::{Publisher,Personal},host::Host,Runtime,shots::Counters,store_source::{Source,PublicationWorld},stagger::Stagger,combat::Capture};
pub struct Session {publisher:Publisher,runtime:usize,source:usize,stagger:usize,shots:usize,host:usize,combat:usize}
#[no_mangle]
pub unsafe extern "C" fn wire_publish_create(runtime:*mut Runtime,source:*mut Source,stagger:*mut Stagger,shots:*const Counters,host:*mut Host,combat:*mut Capture)->*mut Session {
    super::owner::with(|owner| {
        let (runtime,source,stagger,shots,host,combat)=(runtime as usize,source as usize,stagger as usize,shots as usize,host as usize,combat as usize);
        if owner.runtimes.get(runtime).is_none()||owner.sources.get(source).is_none()||owner.staggers.get(stagger).is_none()
            ||owner.shots.get(shots).is_none()||owner.hosts.get(host).is_none()||owner.combat.get(combat).is_none() {return std::ptr::null_mut();}
        owner.publications.insert(Session {publisher:Publisher::default(),runtime,source,stagger,shots,host,combat}).unwrap_or(0) as *mut Session
    })
}
#[no_mangle]
pub unsafe extern "C" fn wire_publish_bind(session:*mut Session,client:u32,peer:u32,edges:*const super::edges::Host)->u32 {
    super::owner::with(|owner| {
        let Some(s)=owner.publications.get_mut(session as usize) else{return 0};
        let Some(host)=owner.hosts.get_mut(s.host) else{return 0};
        if host.clients.len()>256||owner.edges.get(edges as usize).is_none() {return 0;}
        let Some(client)=host.clients.iter_mut().find(|c|c.id==client) else{return 0};
        let Ok(personal)=Personal::new(peer,edges as usize) else{return 0};client.publication=Some(personal);
        s.publisher.admit(host.clients.len());1
    })
}
#[no_mangle]
pub unsafe extern "C" fn wire_publish(session:*mut Session)->*const f64 {
    super::owner::with(|owner| {
        use crate::gen::WireWorldFacts as F;
        let Some(s)=owner.publications.get_mut(session as usize) else{return std::ptr::null()};
        let (Some(source),Some(stagger),Some(shots),Some(host),Some(combat),Some(runtime))=(
            owner.sources.get_mut(s.source),owner.staggers.get_mut(s.stagger),owner.shots.get(s.shots),
            owner.hosts.get_mut(s.host),owner.combat.get_mut(s.combat),owner.runtimes.get_mut(s.runtime)) else{return std::ptr::null()};
        let facts=source.publication_clock();
        let mut world=PublicationWorld {epoch:host.epoch,source,stagger,shots,combat,edges:&owner.edges};
        s.publisher.run(host,runtime,&mut world,facts[F::TICK],facts[F::AT],facts[F::UNIX_MS],facts[F::DROPPED_TICKS]);
        s.publisher.block.ptr()
    })
}
#[no_mangle]
pub unsafe extern "C" fn wire_publish_drop(session:*mut Session) {super::owner::with(|owner|{owner.publications.remove(session as usize);});}
/// Occupancy observes retained banks without changing capture or ACK state.
#[no_mangle]
pub unsafe extern "C" fn wire_publish_load(session:*const Session)->*const f64 {
    super::owner::with(|owner| {
        let Some(s)=owner.publications.get(session as usize) else{return std::ptr::null()};
        let (Some(host),Some(runtime))=(owner.hosts.get(s.host),owner.runtimes.get(s.runtime)) else{return std::ptr::null()};
        let load=s.publisher.load(host,runtime);
        crate::abi::sim().calls.result(crate::gen::WireLoad::WORDS,|row,heap|load.put(row,heap));crate::abi::sim().calls.block.ptr()
    })
}
