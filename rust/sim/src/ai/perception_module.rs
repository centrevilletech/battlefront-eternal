//! Perception's bounded queues on the battle's AI instance. Item tokens
//! preserve the calling object's identity while priorities and heap order
//! remain native. No export holds this state across a callback.

use super::{combat::perception::{Inserted, PriorityHeap}, module::state};

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
