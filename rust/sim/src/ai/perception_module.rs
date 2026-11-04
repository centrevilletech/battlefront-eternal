//! Perception's bounded queues on the battle's AI instance. Item tokens
//! preserve the calling object's identity while priorities and heap order
//! remain native. No export holds this state across a callback.

use super::{combat::perception::{Inserted, PriorityHeap, ThinQueue, thin_candidates}, module::state, random::ClassicRandom};
use crate::{abi::sim, store::word_u32};
use std::collections::HashMap;
use three::Vector3;
use super::combat::{perception::TargetTracker, world::Ref};

pub(super) struct TrackerState {
    pub data: TargetTracker,
    pub ref_tag: u32,
    pub generation_tag: u32,
    pub serial: f64,
}

#[no_mangle]
pub extern "C" fn ai_tracker_new(controller: u32) -> u32 {
    let s=state();s.trackers.push(TrackerState {data:TargetTracker::new(controller as u64),ref_tag:0,generation_tag:0,serial:0.0});
    (s.trackers.len()-1) as u32
}

#[no_mangle]
pub extern "C" fn ai_tracker_ref(id: u32, tag: u32, target: u32, generation: f64, generation_tag: u32) -> f64 {
    let t=&mut state().trackers[id as usize];t.ref_tag=tag;t.generation_tag=generation_tag;t.serial+=1.0;
    t.data.target=(tag==2).then_some(Ref {obj:target as u64,generation});t.serial
}

#[no_mangle]
pub extern "C" fn ai_tracker_read(id: u32) {
    let s=state();let t=&s.trackers[id as usize];
    let r=t.data.target.unwrap_or(Ref {obj:0,generation:0.0});s.out.clear();
    s.out.extend([t.ref_tag as f64,r.obj as f64,r.generation,t.generation_tag as f64,t.serial]);
}

#[no_mangle]
pub extern "C" fn ai_tracker_field(id: u32, field: u32) -> f64 {
    let t=&state().trackers[id as usize].data;
    match field {0=>t.point.x,1=>t.point.y,2=>t.point.z,3=>t.fresh as u32 as f64,4=>t.sample_time,
        5=>t.cost,6=>t.delay,7=>t.frozen as u32 as f64,8=>t.counting as u32 as f64,9=>t.deadline,_=>panic!("invalid tracker field")}
}

#[no_mangle]
pub extern "C" fn ai_tracker_write(id: u32, field: u32, value: f64) {
    let t=&mut state().trackers[id as usize].data;let flag=value!=0.0 && !value.is_nan();
    match field {0=>t.point.x=value,1=>t.point.y=value,2=>t.point.z=value,3=>t.fresh=flag,4=>t.sample_time=value,
        5=>t.cost=value,6=>t.delay=value,7=>t.frozen=flag,8=>t.counting=flag,9=>t.deadline=value,_=>panic!("invalid tracker field")}
}

#[no_mangle]
pub extern "C" fn ai_tracker_reset(id: u32, target: u32, generation: f64, generation_tag: u32) {
    ai_tracker_ref(id,if target==0 {0} else {2},target,generation,generation_tag);
    let t=&mut state().trackers[id as usize].data;
    t.fresh=false;t.sample_time=-1000.0;t.cost=10000.0;t.counting=false;
}

#[no_mangle]
pub extern "C" fn ai_tracker_command(id: u32, op: u32, number: f64, now: f64) {
    let t=&mut state().trackers[id as usize].data;
    match op {0=>t.start_countdown(now),1=>t.set_delay(number,now),2=>t.default_delay(now),
        3=>t.freeze(number!=0.0 && !number.is_nan(),now),_=>panic!("invalid tracker command")}
}

#[derive(Default)]
pub(super) struct CandidateState {
    pub tag: u32,
    pub unit: u32,
    pub generation: f64,
    pub generation_tag: u32,
    pub serial: f64,
    pub point: Vector3,
    pub scanned: f64,
}

pub(super) struct SightQueue {
    pub active: u32,
    pub scratch: u32,
    pub overflow: f64,
    pub tested: f64,
}

#[no_mangle]
pub extern "C" fn ai_los_new(active: u32, scratch: u32, overflow: f64, tested: f64) -> u32 {
    let s=state();s.sight_queues.push(SightQueue {active,scratch,overflow,tested});(s.sight_queues.len()-1) as u32
}

#[no_mangle]
pub extern "C" fn ai_los_field(id: u32, field: u32) -> f64 {
    let q=&state().sight_queues[id as usize];
    match field {0=>q.active as f64,1=>q.scratch as f64,2=>q.overflow,3=>q.tested,_=>panic!("invalid sight queue field")}
}

#[no_mangle]
pub extern "C" fn ai_los_write(id: u32, field: u32, value: f64) {
    let q=&mut state().sight_queues[id as usize];
    match field {0=>q.active=word_u32(value),1=>q.scratch=word_u32(value),2=>q.overflow=value,3=>q.tested=value,_=>panic!("invalid sight queue field")}
}

#[no_mangle]
pub extern "C" fn ai_los_submit(id: u32, priority: f64, item: u32) {
    let s=state();let active=s.sight_queues[id as usize].active;
    let h=&mut s.heaps[active as usize];
    let answer=match h.insert(priority,Some(item)) {
        Inserted::Placed=>0,
        Inserted::Displaced(old)=>old.unwrap_or(0),
        Inserted::Dropped=>{s.sight_queues[id as usize].overflow+=1.0;item},
    };
    s.out.clear();s.out.extend([answer as f64,h.count as f64,h.dropped]);
}

#[no_mangle]
pub extern "C" fn ai_perception_new() -> u32 {
    let s=state();s.perceptions.push(CandidateState::default());(s.perceptions.len()-1) as u32
}

#[no_mangle]
pub extern "C" fn ai_perception_candidate(id: u32, tag: u32, unit: u32, generation: f64, has_generation: u32) -> f64 {
    let p=&mut state().perceptions[id as usize];
    p.tag=tag;p.unit=unit;p.generation=generation;p.generation_tag=has_generation;p.serial+=1.0;p.serial
}

#[no_mangle]
pub extern "C" fn ai_perception_read(id: u32) {
    let s=state();let p=&s.perceptions[id as usize];s.out.clear();
    s.out.extend([p.tag as f64,p.unit as f64,p.generation,p.generation_tag as f64,p.serial]);
}

#[no_mangle]
pub extern "C" fn ai_perception_field(id: u32, field: u32) -> f64 {
    let p=&state().perceptions[id as usize];
    match field {0=>p.point.x,1=>p.point.y,2=>p.point.z,3=>p.scanned,_=>panic!("invalid perception field")}
}

#[no_mangle]
pub extern "C" fn ai_perception_write(id: u32, field: u32, value: f64) {
    let p=&mut state().perceptions[id as usize];
    match field {0=>p.point.x=value,1=>p.point.y=value,2=>p.point.z=value,3=>p.scanned=value,_=>panic!("invalid perception field")}
}

#[no_mangle]
pub extern "C" fn ai_cluster_lookup(target: u32, controller: u32, generation: f64, create: u32) -> u32 {
    state().combat.shared.clusters.state(target as u64,(controller!=0).then_some(controller as u64),generation,create!=0).unwrap_or(0) as u32
}

#[no_mangle]
pub extern "C" fn ai_cluster_has(key: u32) -> u32 {state().combat.shared.clusters.states.contains_key(&(key as u64)) as u32}

/// Adopt an existing source cache once when its owner moves to the module.
#[no_mangle]
pub extern "C" fn ai_cluster_seed(key: u32, scan: f64, flagged: f64, generation: f64) {
    use super::combat::weapons_ai::ClusterState;
    let states=&mut state().combat.shared.clusters.states;
    assert!(!states.contains_key(&(key as u64)),"cluster state already belongs to the module");
    states.insert(key as u64,ClusterState {scan_at:scan,flagged_at:flagged,generation});
}

#[no_mangle]
pub extern "C" fn ai_cluster_field(key: u32, field: u32) -> f64 {
    let c=&state().combat.shared.clusters.states[&(key as u64)];
    match field {0=>c.scan_at,1=>c.flagged_at,2=>c.generation,_=>panic!("invalid cluster field")}
}

#[no_mangle]
pub extern "C" fn ai_cluster_write(key: u32, field: u32, value: f64) {
    let c=state().combat.shared.clusters.states.get_mut(&(key as u64)).unwrap();
    match field {0=>c.scan_at=value,1=>c.flagged_at=value,2=>c.generation=value,_=>panic!("invalid cluster field")}
}

#[no_mangle]
pub extern "C" fn ai_heap_new(capacity: u32, sentinel: f64) -> u32 {
    let s = state();
    s.heaps.push(PriorityHeap::new(capacity as usize, sentinel));
    (s.heaps.len() - 1) as u32
}

/// The return token, current count and lifetime drop count. A zero token
/// is the heap's null slot; callers allocate nonzero tokens for values.
#[no_mangle]
pub extern "C" fn ai_heap_command(id: u32, op: u32, priority: f64, item: u32, slot: u32) {
    let s = state();
    let h = &mut s.heaps[id as usize];
    let answer = match op {
        0 => { h.clear(); 0.0 }
        1 => match h.insert(priority, Some(item)) {
            Inserted::Placed => 0.0,
            Inserted::Displaced(old) => old.unwrap_or(0) as f64,
            Inserted::Dropped => item as f64,
        },
        2 => if h.count > 0 { h.items[1].unwrap_or(0) as f64 } else { 0.0 },
        3 => h.peek_priority(),
        4 => h.pop().flatten().unwrap_or(0) as f64,
        5 => h.min_index() as f64,
        6 => { h.place(slot as usize, priority, Some(item)); 0.0 }
        7 => 0.0,
        _ => panic!("invalid perception heap command"),
    };
    s.out.clear();
    s.out.extend([answer, h.count as f64, h.dropped]);
}

/// Full slot order for observers. Cleared slots and stale priority words
/// are retained exactly as in the source arrays.
#[no_mangle]
pub extern "C" fn ai_heap_read(id: u32) {
    let s = state();
    let h = &s.heaps[id as usize];
    s.out.clear();
    s.out.extend([h.count as f64, h.dropped]);
    for (&priority, item) in h.priority.iter().zip(&h.items) {
        s.out.extend([priority as f64, item.unwrap_or(0) as f64]);
    }
}

struct HeapThin<'a> {
    heap: &'a mut PriorityHeap<u32>,
    live: HashMap<u32,Option<u64>>,
    random: ClassicRandom,
    requests: Vec<(Option<u64>,f64)>,
}
impl ThinQueue for HeapThin<'_> {
    fn size(&self) -> usize { self.heap.size() }
    fn pop(&mut self) -> Option<u64> {
        self.heap.pop().flatten().and_then(|token| self.live[&token])
    }
    fn skip(&mut self) -> bool { self.random.int(1.0,2.0) != 1.0 }
    fn submit(&mut self, target: Option<u64>, priority: f64) { self.requests.push((target,priority)); }
}

/// Unit lives are immutable during the heap-only ranking pass. The caller
/// supplies each held reference's current life; no frame-end state is fed
/// back. The ordinary LOS queue receives the ordered submissions afterward.
#[no_mangle]
pub extern "C" fn ai_heap_thin(id: u32) {
    let words = sim().calls.args().0.to_vec();
    let resolve = |v: &[f64]| {
        (v[0] != 0.0 && v[1] != 0.0 && v[2] == v[3]).then_some(v[0] as u64)
    };
    let retained = resolve(&words[1..5]);
    let live = words[5..].chunks_exact(5).map(|v|(word_u32(v[0]),resolve(&v[1..]))).collect();
    let s = state();
    let mut q = HeapThin { heap: &mut s.heaps[id as usize], live, random: ClassicRandom::from_state(word_u32(words[0])), requests: Vec::new() };
    let submitted = thin_candidates(&mut q,retained);
    s.out.clear();s.out.extend([q.random.state as f64,submitted]);
    for (target,priority) in q.requests { s.out.extend([target.unwrap_or(0) as f64,priority]); }
}
