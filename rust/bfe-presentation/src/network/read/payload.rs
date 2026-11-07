//! SnapshotClient's field versions share a bounded byte arena. Small flags
//! must not reserve a whole datagram, while opaque roster payloads remain
//! contiguous for word deltas. Every reservation happens before receive.
use crate::network_layout::net_limits as l;

pub(super) struct Payloads { pub bytes: Vec<u8>, tree: Vec<u8> }
impl Payloads {
    // Encode-only page owners never need a receive arena. Setup reserves
    // this storage once, before transport can deliver its first packet.
    pub fn empty() -> Self { Self { bytes: Vec::new(), tree: Vec::new() } }
    // Both budgets come from the shared network layout. A node per binary
    // partition keeps allocator metadata bounded alongside the payload bytes.
    pub fn new() -> Self {
        Self { bytes: vec![0; l::FIELD_BYTES], tree: vec![0; 2 * l::FIELD_BYTES / l::FIELD_BLOCK - 1] }
    }
    // A tree node is free, split, or full. Searches skip full subtrees;
    // their work is bounded by the byte budget rather than field count.
    pub fn allocate(&mut self, length: usize) -> Option<usize> {
        let size = length.max(l::FIELD_BLOCK).checked_next_power_of_two()?;
        if size > self.bytes.len() { return None; }
        self.take(0, 0, self.bytes.len(), size)
    }
    // Children cover disjoint halves. A full child can be skipped even when
    // its earlier allocations consisted of many differently sized fields.
    fn take(&mut self, node: usize, offset: usize, size: usize, wanted: usize) -> Option<usize> {
        if self.tree[node] == 2 { return None; }
        if size == wanted {
            if self.tree[node] != 0 { return None; }
            self.tree[node] = 2;
            return Some(offset);
        }
        let half = size / 2;
        let left = node * 2 + 1;
        let result = self.take(left, offset, half, wanted)
            .or_else(|| self.take(left + 1, offset + half, half, wanted));
        self.tree[node] = if self.tree[left] == 2 && self.tree[left + 1] == 2 { 2 } else { 1 };
        result
    }
    // Coalescing the last reference restores larger contiguous spans. A
    // long battle can alternate flags and large rosters without fragmentation
    // accumulating across ACK-ring rotations or multipart evictions.
    pub fn release(&mut self, offset: usize, length: usize) {
        let size = length.max(l::FIELD_BLOCK).next_power_of_two();
        // The allocation's size and address identify its node directly;
        // releasing a version does not search a list of other live fields.
        let mut node = self.bytes.len() / size - 1 + offset / size;
        self.tree[node] = 0;
        while node != 0 {
            node = (node - 1) / 2;
            let left = node * 2 + 1;
            self.tree[node] = if self.tree[left] == 0 && self.tree[left + 1] == 0 { 0 }
                else if self.tree[left] == 2 && self.tree[left + 1] == 2 { 2 } else { 1 };
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn released_small_versions_coalesce_into_full_payloads() {
        let mut arena = Payloads::new();
        let count = l::FIELD_BYTES / l::FIELD_BLOCK;
        let offsets: Vec<_> = (0..count).map(|_| arena.allocate(1).unwrap()).collect();
        assert!(arena.allocate(1).is_none());
        for &offset in offsets.iter().step_by(2) { arena.release(offset, 1); }
        assert!(arena.allocate(l::FIELD_PAYLOAD).is_none());
        for &offset in offsets.iter().skip(1).step_by(2) { arena.release(offset, 1); }
        assert_eq!(arena.allocate(l::FIELD_BYTES), Some(0));
        arena.release(0, l::FIELD_BYTES);
        assert_eq!(arena.allocate(l::FIELD_PAYLOAD), Some(0));
    }
}
