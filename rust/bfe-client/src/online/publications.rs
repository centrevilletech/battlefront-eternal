//! Snapshot field leases expire at the next receive. Retain one bounded copy
//! of changed semantic fields until their native owners consume this batch.
use bfe_presentation::network_layout::{net_limits as l, net_record as r};
const CAPACITY: usize = 2048;
#[derive(Clone)]
pub struct Field {
    pub row: [f64; r::STRIDE],
    pub bytes: [u8; l::FIELD_PAYLOAD],
    pub length: usize,
}
impl Default for Field {
    fn default() -> Self {
        Self { row: [0.; r::STRIDE], bytes: [0; l::FIELD_PAYLOAD], length: 0 }
    }
}
// A receive can publish more than one semantic field for a wire identity.
// Preserve arrival order until the displayed frame consumes the journal.
// Payload bytes outlive Session's temporary publication leases, but resource
// views never borrow these copies beyond that frame's synchronous call.
// This bank is sized before the first datagram, including a paused window.
pub struct Publications {
    recent: Recent,
    rows: Vec<Field>,
    at: usize,
    count: usize,
    pub refused: u64,
}
impl Default for Publications {
    fn default() -> Self {
        Self { recent: Default::default(), rows: vec![Field::default(); CAPACITY], at: 0, count: 0, refused: 0 }
    }
}
impl Publications {
    // Keep the newest field when a long pause exhausts the journal. Identity
    // and generation travel with every payload; array position is no lease.
    pub fn push(&mut self, row: &[f64], bytes: &[u8]) {
        if row[r::KIND] == bfe_presentation::network_layout::net_kind::COMBAT as f64 && !self.recent.accept(row) {
            return;
        }
        if bytes.len() > l::FIELD_PAYLOAD {
            self.refused += 1;
            return;
        }
        if self.count == CAPACITY {
            self.at = (self.at + 1) % CAPACITY;
            self.count -= 1;
            self.refused += 1;
        }
        let at = (self.at + self.count) % CAPACITY;
        let field = &mut self.rows[at];
        field.row.copy_from_slice(row);
        field.bytes[..bytes.len()].copy_from_slice(bytes);
        field.length = bytes.len();
        self.count += 1;
    }
    pub fn iter(&self) -> impl Iterator<Item = &Field> {
        (0..self.count).map(|n| &self.rows[(self.at + n) % CAPACITY])
    }
    // Clearing consumed rows must retain duplicate suppression across frames.
    // Resetting an epoch is different: the same ids and ticks can legitimately
    // appear in another battle, so its receipt clocks must be discarded too.
    // Both operations retain the backing bytes and identity table.
    pub fn reset(&mut self) {
        self.clear();
        self.recent.clear();
    }
    pub fn clear(&mut self) {
        self.at = 0;
        self.count = 0;
    }
}

// NetworkCombatViews.seen suppresses a reliable repeat and a datagram repeat
// together. Keep schema clocks separately: an end can share its birth's id.
struct Seen {
    key: Option<(u32, u32)>,
    ticks: [f64; 9],
}
struct Recent {
    rows: Vec<Seen>,
    index: super::index::Index,
    next: usize,
}
impl Default for Recent {
    fn default() -> Self {
        Self {
            rows: (0..4096).map(|_| Seen { key: None, ticks: [f64::NEG_INFINITY; 9] }).collect(),
            index: super::index::Index::new(4096),
            next: 0,
        }
    }
}
impl Recent {
    fn clear(&mut self) {
        self.index.clear();
        for row in &mut self.rows {
            row.key = None;
            row.ticks.fill(f64::NEG_INFINITY);
        }
        self.next = 0;
    }
    // NetworkCombatViews treats reliable and datagram delivery as one stream.
    // A terminal may share a birth's id and tick; keep separate schema clocks
    // so suppressing a repeated birth cannot suppress its first terminal.
    // The oldest identity is evicted with its index entry before slot reuse.
    fn accept(&mut self, row: &[f64]) -> bool {
        let schema = row[r::SCHEMA] as usize;
        if schema >= 9 {
            return false;
        }
        let key = (row[r::ID] as u32, row[r::GENERATION] as u32);
        let index = if let Some(&index) = self.index.get(&key) {
            index
        } else {
            let index = self.next;
            self.next = (self.next + 1) % self.rows.len();
            if let Some(old) = self.rows[index].key {
                self.index.remove(&old);
            }
            self.rows[index] = Seen { key: Some(key), ticks: [f64::NEG_INFINITY; 9] };
            self.index.insert(key, index);
            index
        };
        if row[r::TICK] <= self.rows[index].ticks[schema] {
            return false;
        }
        self.rows[index].ticks[schema] = row[r::TICK];
        true
    }
}
