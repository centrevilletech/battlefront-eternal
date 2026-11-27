//! A publication names sibling banks by qualified instance-local identities.
//! Each call resolves them once and passes disjoint field borrows to the
//! single publisher. No session retains a Rust address or borrows JavaScript.
use super::{publisher::{Publisher,Personal},store_source::PublicationWorld};
pub struct Session {publisher:Publisher,runtime:usize,source:usize,stagger:usize,shots:usize,host:usize,combat:usize}
pub(crate) fn wire_publish_create_in(w: &mut crate::world::owner::World, runtime:usize,source:usize,stagger:usize,shots:usize,host:usize,combat:usize) ->usize {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
    let (runtime,source,stagger,shots,host,combat)=(runtime as usize,source as usize,stagger as usize,shots as usize,host as usize,combat as usize);
    let Some((bounds, capacity)) = owner.runtimes.get(runtime).map(|runtime| (runtime.bounds, runtime.capacity)) else { return 0; };
    if owner.sources.get(source).is_none()||owner.staggers.get(stagger).is_none()
        ||owner.shots.get(shots).is_none()||owner.hosts.get(host).is_none()||owner.combat.get(combat).is_none() {return 0;}
    owner.publications.insert(Session {publisher:Publisher::for_battle(bounds, capacity),runtime,source,stagger,shots,host,combat}).unwrap_or(0)

}
pub(crate) fn wire_publish_bind_in(w: &mut crate::world::owner::World, session:usize,client:u32,peer:u32,edges:usize) ->u32 {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
    let Some(s)=owner.publications.get_mut(session as usize) else{return 0};
    let Some(host)=owner.hosts.get_mut(s.host) else{return 0};
    if host.clients.len()>256||owner.edges.get(edges as usize).is_none() {return 0;}
    let Some(client)=host.clients.iter_mut().find(|c|c.id==client) else{return 0};
    let Ok(personal)=Personal::new(peer,edges as usize) else{return 0};client.publication=Some(personal);
    s.publisher.admit(host.clients.len());1

}
/// Prepares what a scripted network battle's mission says, from the whole
/// world, before the publication borrows its owners apart.
fn sample_mission(w: &mut crate::world::owner::World, session: usize) {
    fn source(w: &mut crate::world::owner::World, session: usize) -> Option<&mut super::store_source::Source> {
        let owner = w.wire.as_deref_mut()?;
        let at = owner.publications.get(session)?.source;
        owner.sources.get_mut(at)
    }
    let Some(source_bank) = source(w, session) else { return };
    let tick = source_bank.publication_clock()[crate::gen::WireWorldFacts::TICK];
    let Some(mut bank) = source_bank.mission.take() else { return };
    bank.sample(w, tick as u32);
    if let Some(source) = source(w, session) { source.mission = Some(bank); }
}
fn sample_space(w: &mut crate::world::owner::World, session: usize) {
    fn source(w: &mut crate::world::owner::World, session: usize) -> Option<&mut super::store_source::Source> {
        let owner = w.wire.as_deref_mut()?;
        let at = owner.publications.get(session)?.source;
        owner.sources.get_mut(at)
    }
    let Some(source_bank) = source(w, session) else { return };
    let tick = source_bank.publication_clock()[crate::gen::WireWorldFacts::TICK];
    let Some(mut bank) = source_bank.space.take() else { return };
    bank.sample(w, tick as u32);
    if let Some(source) = source(w, session) { source.space = Some(bank); }
}
pub(crate) fn wire_publish_in(w: &mut crate::world::owner::World, session:usize) -> Option<&[f64]> {
    match w.movement.family() {
        bfe_family::GameFamily::Swbf1 => released_wire_publish_in(w, session),
        bfe_family::GameFamily::Swbf2 => swbf2_wire_publish_in(w, session),
    }
}
fn swbf2_wire_publish_in(w: &mut crate::world::owner::World, session: usize) -> Option<&[f64]> {
    sample_mission(w, session);
    let due = {
        let owner = w.wire.as_deref()?;
        let s = owner.publications.get(session)?;
        let source = owner.sources.get(s.source)?;
        let host = owner.hosts.get(s.host)?;
        let tick = source.publication_clock()[crate::gen::WireWorldFacts::TICK];
        host.due(tick)
    };
    if due {
        sample_space(w, session);
    }
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
    use crate::gen::WireWorldFacts as F;
    let Some(s)=owner.publications.get_mut(session as usize) else{return None};
    let (Some(source),Some(stagger),Some(shots),Some(host),Some(combat),Some(runtime))=(
        owner.sources.get_mut(s.source),owner.staggers.get_mut(s.stagger),owner.shots.get(s.shots),
        owner.hosts.get_mut(s.host),owner.combat.get_mut(s.combat),owner.runtimes.get_mut(s.runtime)) else{return None};
    let facts=source.publication_clock();
    let mut world=PublicationWorld {w: super::store_source::Read {sim:&w.sim,input:&w.input,units:&w.units,fleet:&w.fleet,battle:&w.battle,combat:&w.combat,collision:&mut w.collision,movement:&mut w.movement,space:w.space.as_deref()},epoch:host.epoch,source,stagger,shots,combat,edges:&owner.edges};
    s.publisher.run(host,runtime,&mut world,facts[F::TICK],facts[F::AT],facts[F::UNIX_MS],facts[F::DROPPED_TICKS]);
    Some(&s.publisher.block.words[..])
}
fn released_wire_publish_in(w: &mut crate::world::owner::World, session:usize) -> Option<&[f64]> {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
    use crate::gen::WireWorldFacts as F;
    let Some(s)=owner.publications.get_mut(session as usize) else{return None};
    let (Some(source),Some(stagger),Some(shots),Some(host),Some(combat),Some(runtime))=(
        owner.sources.get_mut(s.source),owner.staggers.get_mut(s.stagger),owner.shots.get(s.shots),
        owner.hosts.get_mut(s.host),owner.combat.get_mut(s.combat),owner.runtimes.get_mut(s.runtime)) else{return None};
    let facts=source.publication_clock();
    let mut world=PublicationWorld {w: super::store_source::Read {sim:&w.sim,input:&w.input,units:&w.units,fleet:&w.fleet,battle:&w.battle,combat:&w.combat,collision:&mut w.collision,movement:&mut w.movement,space:None},epoch:host.epoch,source,stagger,shots,combat,edges:&owner.edges};
    s.publisher.run(host,runtime,&mut world,facts[F::TICK],facts[F::AT],facts[F::UNIX_MS],facts[F::DROPPED_TICKS]);
    Some(&s.publisher.block.words[..])

}
/// The native publication (Context::world_publish): `facts`, a WireWorldFacts
/// row with its links, stamps the source's publication clock as the host's
/// writeWorldFacts does, then the shared body runs. Answers with the result
/// block and the encoder bytes its spans point into; None for an unknown
/// session or a facts row of the wrong size.
pub(crate) fn wire_publish_native_in<'a>(w: &'a mut crate::world::owner::World, session:usize, facts:&[f64]) -> Option<(&'a [f64], &'a [u8])> {
    {
        let owner = w.wire.as_deref_mut()?;
        let s = owner.publications.get(session)?;
        owner.sources.get_mut(s.source)?.stamp_publication_clock(facts)?;
    }
    wire_publish_in(w, session)?;
    let s = w.wire.as_deref()?.publications.get(session)?;
    Some((&s.publisher.block.words[..], &s.publisher.bytes[..]))
}
pub(crate) fn published_own_in(w: &crate::world::owner::World, session: usize, client: u32) -> Option<super::publisher::OwnFacts> {
    w.wire.as_deref()?.publications.get(session)?.publisher.own_facts.iter().find(|f| f.client == client).copied()
}
pub(crate) fn wire_publish_drop_in(w: &mut crate::world::owner::World, session:usize) {super::owner::with_in(w, |owner|{owner.publications.remove(session as usize);});}
/// Occupancy observes retained banks without changing capture or ACK state.
pub(crate) fn wire_publish_load_in(w: &mut crate::world::owner::World, session:usize) -> Option<&[f64]> {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
    let Some(s)=owner.publications.get(session as usize) else{return None};
    let (Some(host),Some(runtime))=(owner.hosts.get(s.host),owner.runtimes.get(s.runtime)) else{return None};
    let load=s.publisher.load(host,runtime);
    w.sim.calls.result(crate::gen::WireLoad::WORDS,|row,heap|load.put(row,heap));Some(&w.sim.calls.block.words[..])

}

