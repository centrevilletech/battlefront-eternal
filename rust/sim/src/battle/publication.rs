//! Sparse publications of the battle Store's canonical rows. Prepared caches
//! retain the generated row and heap encoding; commands mark only their target.
use crate::gen::{Battle, Character, F4Delta, F4Handle, F4ObjectKind, HumanSpawn, MobilePost, Post, Spawning, Store};
use crate::store::{Ref, Slab};
use crate::store::block::{Block, HEADER};
use crate::vehicles::f2_delta::Cache;

// The first two words of every generated entity/singleton store row.
const ROW_SLOT: usize = 0;
const ROW_GENERATION: usize = 1;
#[derive(Clone, Copy)]
pub enum Source {
    Character(Ref<Character>), Post(Ref<Post>), Mobile(Ref<MobilePost>), Battle, Spawning, Human,
}
impl Source {
    fn kind(self) -> F4ObjectKind {
        match self { Self::Character(_) => F4ObjectKind::Character, Self::Post(_) => F4ObjectKind::Post,
            Self::Mobile(_) => F4ObjectKind::MobilePost, Self::Battle => F4ObjectKind::Battle,
            Self::Spawning => F4ObjectKind::Spawning, Self::Human => F4ObjectKind::Human }
    }
    fn identity(self) -> (u32, u32) {
        match self { Self::Character(r) => (r.slot, r.gen), Self::Post(r) => (r.slot, r.gen),
            Self::Mobile(r) => (r.slot, r.gen), _ => (0, 1) }
    }
    fn words(self) -> usize {
        match self { Self::Character(_) => Character::EXPORT_STRIDE, Self::Post(_) => Post::EXPORT_STRIDE,
            Self::Mobile(_) => MobilePost::EXPORT_STRIDE, Self::Battle => Battle::EXPORT_STRIDE,
            Self::Spawning => Spawning::EXPORT_STRIDE, Self::Human => HumanSpawn::EXPORT_STRIDE }
    }
    fn put(self, s: &Store, row: &mut [f64], heap: &mut Vec<f64>) {
        let (slot, generation) = self.identity();
        row[ROW_SLOT] = slot as f64; row[ROW_GENERATION] = generation as f64;
        match self {
            Self::Character(r) => s.character.get(r).expect("publish a live Character").put_export(row, heap),
            Self::Post(r) => s.post.get(r).expect("publish a live post").put_export(row, heap),
            Self::Mobile(r) => s.mobile_post.get(r).expect("publish a live mobile post").put_export(row, heap),
            Self::Battle => s.battle.put_export(row, heap), Self::Spawning => s.spawning.put_export(row, heap),
            Self::Human => s.human_spawn.put_export(row, heap),
        }
    }
}
struct Entry { source: Source, handle: F4Handle, cache: Cache, delta: F4Delta, dirty: bool, budget: usize, heap: usize }
impl Entry {
    fn new(source: Source, life: f64, heap: usize) -> Self {
        let (slot, generation) = source.identity();
        let words = HEADER + source.words() + heap;
        let mut delta = F4Delta::default(); delta.mask.reserve(words.div_ceil(32));
        Self { source, handle: F4Handle { kind: source.kind(), slot, generation, life },
            cache: Cache::borrowed(words), delta, dirty: true, budget: F4Delta::WORDS + words + words.div_ceil(32), heap }
    }
    fn encode(&mut self, s: &Store, retired: bool) -> bool {
        let source = self.source;
        let changed = self.cache.encode(self.handle.slot, self.handle.generation, source.words(), |row, heap| source.put(s, row, heap));
        // Heap sizes are preparation hints, not a frame-time failure limit.
        // Exceptional content can grow the retained encoding, just as the
        // old stats arrays did; ordinary prepared maps reuse these buffers.
        let changed = changed || retired || self.delta.target != self.handle;
        self.dirty = false;
        if changed {
            self.delta.target = self.handle.clone(); self.delta.retired = retired;
            self.delta.length = self.cache.last.words.len() as u32;
            self.delta.mask.clear(); self.delta.mask.extend_from_slice(&self.cache.delta.value.mask);
            self.delta.source = self.cache.last.ptr() as u32;
            self.delta.borrowed_words = self.cache.borrowed_words() as u32;
        }
        changed
    }
}

/// An index by generated kind and slab slot avoids a search per Character or
/// command. Retired caches retain final values through same-slot replacement.
pub struct Publication {
    entries: Slab<Entry>,
    order: Vec<Ref<Entry>>,
    index: [Vec<Option<Ref<Entry>>>; F4ObjectKind::NAMES.len()],
    retired: Vec<Entry>, published_retired: Vec<Entry>, selected: Vec<Ref<Entry>>,
    budget: usize, pending: bool, borrowed_words: usize,
    pub block: Block,
}
impl Publication {
    pub fn new(capacity: usize) -> Self {
        Self { entries: Slab::with_capacity(capacity), order: Vec::with_capacity(capacity),
            index: std::array::from_fn(|_| Vec::new()), retired: Vec::with_capacity(capacity),
            published_retired: Vec::with_capacity(capacity), selected: Vec::with_capacity(capacity), budget: HEADER,
            pending: false, borrowed_words: 0, block: Block::default() }
    }
    /// Heap room is derived from the prepared roster/post/stat capacities at
    /// construction. Encoding ordinary phase rows then reuses that storage.
    pub fn register(&mut self, source: Source, life: f64, heap: usize) -> F4Handle {
        assert!(life.is_finite(), "finite publication life");
        self.pending = true;
        let kind = source.kind() as usize; let (slot, generation) = source.identity();
        if let Some(r) = self.index[kind].get(slot as usize).copied().flatten() {
            let e = self.entries.get_mut(r).unwrap();
            assert_eq!(e.handle.generation, generation, "retire the old publication before replacing its slot");
            // Re-admission can add content to the same life. The next encode
            // retains the expanded buffer instead of rejecting the frame.
            e.heap = e.heap.max(heap);
            e.handle.life = life; e.dirty = true; return e.handle.clone();
        }
        let e = Entry::new(source, life, heap); let handle = e.handle.clone();
        let words = HEADER + source.words() + heap;
        self.budget += F4Delta::WORDS + words + words.div_ceil(32);
        self.block.words.reserve(self.budget.saturating_sub(self.block.words.len()));
        self.block.heap.reserve(self.budget.saturating_sub(self.block.heap.len()));
        let r = self.entries.insert(e); self.order.push(r);
        let count = self.order.len() + self.retired.len();
        self.selected.reserve(count.saturating_sub(self.selected.len()));
        self.retired.reserve(count.saturating_sub(self.retired.len()));
        self.published_retired.reserve(count.saturating_sub(self.published_retired.len()));
        let length = self.index[kind].len().max(slot as usize + 1);
        self.index[kind].resize(length, None);
        self.index[kind][slot as usize] = Some(r); handle
    }
    pub fn handle(&self, source: Source) -> Option<&F4Handle> {
        let (slot,generation)=source.identity();
        let r=self.index[source.kind() as usize].get(slot as usize).copied().flatten()?;
        let h=&self.entries.get(r)?.handle;(h.generation==generation).then_some(h)
    }
    fn find(&self, h: &F4Handle) -> Option<Ref<Entry>> {
        let r = self.index[h.kind as usize].get(h.slot as usize).copied().flatten()?;
        (self.entries.get(r)?.handle == *h).then_some(r)
    }
    pub fn mark(&mut self, h: &F4Handle) -> bool {
        let Some(r) = self.find(h) else { return false };
        self.entries.get_mut(r).unwrap().dirty = true; self.pending = true; true
    }
    /// Capture the final row before removing it from Store. A later birth can
    /// reuse its slot while this retired cache waits for the next publication.
    pub fn retire(&mut self, s: &Store, h: &F4Handle) -> bool {
        let Some(r) = self.find(h) else { return false };
        self.pending = true;
        let mut e = self.entries.remove(r).unwrap(); e.encode(s, true); self.retired.push(e);
        self.index[h.kind as usize][h.slot as usize] = None; self.order.retain(|old| *old != r); true
    }
    /// Borrowed caches live until the next foreign barrier. In particular,
    /// retirement must not free a row while its final publication names it.
    pub fn flush(&mut self, s: &Store) -> usize {
        self.published_retired.clear(); self.borrowed_words = 0;
        // A soldier observer usually has no score/post changes. Keep its empty
        // handoff independent of the size of the admitted battle roster.
        if !self.pending {
            if self.block.words.len() != HEADER { self.block.begin(0, F4Delta::WORDS); self.block.finish(); }
            return 0;
        }
        self.pending = false;
        self.selected.clear();
        for &r in &self.order {
            let e = self.entries.get_mut(r).unwrap();
            if e.dirty && e.encode(s, false) { self.selected.push(r); }
        }
        let retired = self.retired.len();
        self.block.begin(retired + self.selected.len(), F4Delta::WORDS);
        for (i, e) in self.retired.iter().enumerate() {
            self.borrowed_words += e.delta.borrowed_words as usize;
            let (row, heap) = self.block.row(i); e.delta.put(row, heap);
        }
        for (i, &r) in self.selected.iter().enumerate() {
            let e = self.entries.get(r).unwrap(); self.borrowed_words += e.delta.borrowed_words as usize;
            let (row, heap) = self.block.row(retired + i); e.delta.put(row, heap);
        }
        self.block.set_retired(retired); self.block.finish();
        for e in &self.retired { self.budget -= e.budget; }
        std::mem::swap(&mut self.retired, &mut self.published_retired);
        retired + self.selected.len()
    }
    pub fn borrowed_words(&self) -> usize { self.borrowed_words }
    /// Inspect a qualified encoding without truncating a native pointer to the
    /// WASM boundary's u32 address. Replayers use this on a 64-bit host too.
    pub fn encoded(&self, h: &F4Handle) -> Option<&[f64]> {
        if let Some(r) = self.find(h) { return Some(&self.entries.get(r)?.cache.last.words); }
        self.published_retired.iter().find(|e| e.handle == *h).map(|e| e.cache.last.words.as_slice())
    }
}
