//! Vehicles.update publication and synchronous hull callbacks. Each dirty hull
//! is encoded against its last publication; unchanged rows never cross to JS.
use crate::gen::{F2Delta, F2HullLife, F2Pad, Store, Vehicle};
use crate::vehicles::f2_delta::Delta;
use crate::store::block::Block;
use crate::store::Ref;

#[derive(Default)]
struct Last {
    row: Block,
    next: Block,
    life: Vec<f64>,
    delta: Delta,
}

#[derive(Default)]
pub struct Publication {
    pub hulls: Block,
    pub lives: Block,
    pub pads: Block,
    last: Vec<Last>,
    dirty: Vec<Ref<Vehicle>>,
    selected: Vec<Ref<Vehicle>>, life_selected: Vec<Ref<Vehicle>>,
    pad_dirty: Vec<usize>,
    life_scratch: Vec<f64>,
}

fn same(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a,b)| a.to_bits() == b.to_bits())
}

fn life(r: Ref<Vehicle>, v: &Vehicle) -> F2HullLife {
    F2HullLife { vehicle: r, generation: v.target_generation, visible: v.visible,
        flight_class: v.flight_class, min_speed: v.min_speed, mid_speed: v.mid_speed,
        boost_speed: v.boost_speed, speed: v.speed, turn_rate: v.turn_rate }
}

impl Publication {
    /// Vehicle construction reserves its publication buffers before the tick.
    pub fn register(&mut self, st: &Store, r: Ref<Vehicle>) {
        if self.last.len() <= r.slot as usize { self.last.resize_with(r.slot as usize + 1, Last::default); }
        let count = self.last.len();
        let cache = &mut self.last[r.slot as usize];
        let v = st.vehicle.get(r).expect("registered publication hull");
        cache.row.words.clear();
        cache.next.begin(1, Vehicle::EXPORT_STRIDE);
        let (row, heap) = cache.next.row(0);
        row[0] = r.slot as f64; row[1] = r.gen as f64;
        v.put_export(row, heap); cache.next.finish();
        cache.row.words.reserve(cache.next.words.len());
        cache.delta.reserve(cache.next.words.len());
        cache.row.heap.reserve(cache.next.heap.len());
        cache.life.resize(F2HullLife::WORDS, f64::NAN);
        self.life_scratch.resize(F2HullLife::WORDS, 0.0);
        self.dirty.reserve(count); self.selected.reserve(count); self.life_selected.reserve(count);
        let words: usize = self.last.iter().map(|r| r.next.words.len().max(r.row.words.len())).sum();
        let heap: usize = self.last.iter().map(|r| r.next.heap.len().max(r.row.heap.len())).sum();
        self.hulls.words.reserve(words); self.hulls.heap.reserve(heap);
        self.lives.words.reserve(5 + count * F2HullLife::WORDS);
        self.pad_dirty.reserve(st.vehicles.slots.len()); self.pads.words.reserve(5 + st.vehicles.slots.len() * F2Pad::WORDS);
        self.mark(r);
    }

    /// Hull::to_store changed canonical state; coalesce repeated substeps.
    pub fn mark(&mut self, r: Ref<Vehicle>) {
        if !self.dirty.contains(&r) { self.dirty.push(r); }
    }

    /// Spawn clocks and respawn clocks change only their own pad record.
    pub fn pad(&mut self, index: usize) {
        if !self.pad_dirty.contains(&index) { self.pad_dirty.push(index); }
    }

    /// Publish dirty hulls before a foreign hook or at the phase boundary.
    /// A callback may publish recursively; the next flush starts a new block.
    pub fn flush(&mut self, st: &mut Store) {
        self.selected.clear(); self.life_selected.clear();
        let mut retired = 0;
        for &(r, ref v) in &st.vehicle.retired {
            let cache = &mut self.last[r.slot as usize];
            cache.next.begin(1, Vehicle::EXPORT_STRIDE); let (row, heap) = cache.next.row(0);
            row[0] = r.slot as f64; row[1] = r.gen as f64; v.put_export(row,heap); cache.next.finish();
            cache.delta.encode(r.slot,r.gen,&cache.next,&cache.row);
            std::mem::swap(&mut cache.row,&mut cache.next);
            self.selected.push(r); retired += 1;
        }
        for &r in &self.dirty {
            let Some(v) = st.vehicle.get(r) else { continue };
            let cache = &mut self.last[r.slot as usize];
            cache.next.begin(1, Vehicle::EXPORT_STRIDE);
            let (row, heap) = cache.next.row(0);
            row[0] = r.slot as f64; row[1] = r.gen as f64;
            v.put_export(row, heap); cache.next.finish();
            life(r, v).put(&mut self.life_scratch, &mut self.lives.heap);
            if !same(&cache.life, &self.life_scratch) {
                self.life_selected.push(r); cache.life.copy_from_slice(&self.life_scratch);
            }
            if !same(&cache.row.words, &cache.next.words) {
                cache.delta.encode(r.slot, r.gen, &cache.next, &cache.row);
                self.selected.push(r);
                std::mem::swap(&mut cache.row, &mut cache.next);
            }
        }
        self.hulls.begin(self.selected.len(), F2Delta::WORDS);
        self.hulls.set_retired(retired);
        self.lives.begin(self.life_selected.len(), F2HullLife::WORDS);
        for (i, &r) in self.selected.iter().enumerate() {
            let (row, heap) = self.hulls.row(i);
            self.last[r.slot as usize].delta.value.put(row, heap);
        }
        for (i,&r) in self.life_selected.iter().enumerate() {
            let (row,heap) = self.lives.row(i); life(r,st.vehicle.get(r).unwrap()).put(row,heap);
        }
        self.hulls.finish(); self.lives.finish();
        st.vehicle.retired.clear(); self.dirty.clear();
        self.pads.begin(self.pad_dirty.len(), F2Pad::WORDS);
        for (i, &index) in self.pad_dirty.iter().enumerate() {
            let pad = &st.vehicles.slots[index];
            let (row, heap) = self.pads.row(i);
            F2Pad { index: index as f64, remaining: pad.remaining, vehicle: pad.vehicle }.put(row, heap);
        }
        self.pads.finish(); self.pad_dirty.clear();
    }
}
