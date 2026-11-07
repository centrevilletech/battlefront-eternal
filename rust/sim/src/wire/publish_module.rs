//! Battle-local publication attachment. All owners are distinct native boxes;
//! an export borrows them once and returns owned bytes and generated metadata.
use super::{publisher::{Publisher, Personal}, host::Host, Runtime, shots::Counters, store_source::{Source, PublicationWorld}, stagger::Stagger, combat::Capture};

pub struct Session { publisher: Publisher, runtime: *mut Runtime, source: *mut Source, stagger: *mut Stagger, shots: *const Counters, host: *mut Host, combat: *mut Capture }
/// # Safety
/// Every pointer belongs to this simulation instance and outlives the session.
#[no_mangle]
pub unsafe extern "C" fn wire_publish_create(runtime: *mut Runtime, source: *mut Source, stagger: *mut Stagger, shots: *const Counters, host: *mut Host, combat: *mut Capture) -> *mut Session {
    if runtime.is_null() || source.is_null() || stagger.is_null() || shots.is_null() || host.is_null() || combat.is_null() { return std::ptr::null_mut(); }
    Box::into_raw(Box::new(Session { publisher: Publisher::default(), runtime, source, stagger, shots, host, combat }))
}
/// # Safety
/// The session and reliable edge owner remain live until the client leaves.
#[no_mangle]
pub unsafe extern "C" fn wire_publish_bind(session: *mut Session, client: u32, peer: u32, edges: *const super::edges::Host) -> u32 {
    let s = &mut *session; let host = &mut *s.host;
    if host.clients.len() > 256 { return 0; }
    let Some(client) = host.clients.iter_mut().find(|c| c.id == client) else { return 0 };
    let Ok(personal) = Personal::new(peer, edges) else { return 0 }; client.publication = Some(personal);
    s.publisher.admit(host.clients.len()); 1
}
/// # Safety
/// All cold bindings are live, distinct and belong to this instance. This
/// function never invokes a host callback while it borrows their world state.
#[no_mangle]
pub unsafe extern "C" fn wire_publish(session: *mut Session) -> *const f64 {
    use crate::gen::WireWorldFacts as F;
    let s = &mut *session; let source = &mut *s.source;
    let facts = source.publication_clock();
    let mut world = PublicationWorld { epoch: (&*s.host).epoch, source, stagger: &mut *s.stagger, shots: &*s.shots, combat: &mut *s.combat };
    s.publisher.run(&mut *s.host, &mut *s.runtime, &mut world, facts[F::TICK], facts[F::AT], facts[F::UNIX_MS], facts[F::DROPPED_TICKS]);
    s.publisher.block.ptr()
}
/// # Safety
/// The session is disposed exactly once before any of its bound owners.
#[no_mangle]
pub unsafe extern "C" fn wire_publish_drop(session: *mut Session) { if !session.is_null() { drop(Box::from_raw(session)); } }

/// Read occupancy without changing any capture, retention or ACK state.
/// # Safety
/// The session and its cold bindings belong to this live battle instance.
#[no_mangle]
pub unsafe extern "C" fn wire_publish_load(session: *const Session) -> *const f64 {
    let s = &*session; let load = s.publisher.load(&*s.host, &*s.runtime);
    crate::abi::sim().calls.result(crate::gen::WireLoad::WORDS, |row, heap| load.put(row, heap));
    crate::abi::sim().calls.block.ptr()
}
