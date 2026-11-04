//! Perception's bounded queues on the battle's AI instance. Item tokens
//! preserve the calling object's identity while priorities and heap order
//! remain native. No export holds this state across a callback.

use super::{combat::perception::{Inserted, PriorityHeap, ThinQueue, thin_candidates}, module::state, random::ClassicRandom};
use crate::{abi::sim, store::word_u32};
use std::collections::HashMap;

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
