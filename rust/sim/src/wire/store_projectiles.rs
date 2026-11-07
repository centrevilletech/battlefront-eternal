//! ProjectileCapture.sample reads canonical ordnance lists in grenade, round,
//! beam order. Only journal identity/attribution facts remain foreign until
//! the enclosing tick owns the ordered event journal; no pose crosses from JS.
use crate::{gen::*};
use super::{codec::Result, journal::Events, cosmetics::{View, Owner}};
const CAPACITY: usize = 4096;
#[derive(Clone, Copy)]
pub struct Sample { pub definition: crate::store::DefId, pub owner: OrdnanceOwner, pub channel: f64, kind: u8, slot: u32, generation: u32, position: [f64; 3], velocity: [f64; 3], quaternion: [f64; 4], end: [f64; 3], life: f64 }
pub struct Capture { pub facts: Vec<WireDynamicFact>, lookup: Vec<usize>, seen: [bool; CAPACITY], pub retired: Vec<u32>, pub created: Vec<WireDynamicFact>, pub unknown: u64, pub malformed: u64, pub error: Option<&'static str>, pruned: bool }
impl Default for Capture {
    fn default() -> Self { Self { facts: Vec::with_capacity(CAPACITY), lookup: Vec::with_capacity(CAPACITY), seen: [false; CAPACITY],
        retired: Vec::with_capacity(CAPACITY), created: Vec::with_capacity(CAPACITY), unknown: 0, malformed: 0, error: None, pruned: false } }
}
impl Capture {
    // ProjectileCapture keeps attribution until its terminal sample is drained.
    // Reclaim those exact ids before accepting callbacks for newly born lives.
    // Updates never rescan or import the JS active-projectile list.
    pub fn begin_updates(&mut self) {
        self.facts.retain(|f| !self.retired.iter().any(|&id| f.id == id as f64));
        self.retired.clear();
    }
    pub fn update(&mut self, fact: WireDynamicFact) -> bool {
        if let Some(old) = self.facts.iter_mut().find(|f| (f.r#type, f.slot, f.ref_generation) == (fact.r#type, fact.slot, fact.ref_generation)) {
            *old = fact; true
        } else if self.facts.len() < CAPACITY { self.facts.push(fact); true }
        else { self.malformed += 1; false }
    }
    fn sample(&mut self, tick: f64, sample: Sample, events: &mut Events, identify: &mut impl FnMut(Sample) -> Result<WireDynamicFact>) -> bool {
        let key = (sample.kind, sample.slot, sample.generation);
        let at = match self.lookup.binary_search_by_key(&key, |&i| { let f = &self.facts[i]; (f.r#type, f.slot, f.ref_generation) }) {
            Ok(at) => self.lookup[at],
            Err(at) => {
                // ProjectileCapture.sample used launch() for bodies that were
                // alive before the host attached. Preserve that first-observed
                // order without reading the JS ordnance lists at publication.
                if self.facts.len() == CAPACITY { self.error = Some("dynamic projectile capture capacity exceeded"); return false; }
                let mut fact = match identify(sample) { Ok(fact) => fact, Err(error) => { self.unknown += 1; self.error = Some(error); return false; } };
                fact.slot = sample.slot; fact.ref_generation = sample.generation; fact.r#type = sample.kind;
                let index = self.facts.len(); self.created.push(fact.clone()); self.facts.push(fact); self.lookup.insert(at, index); index
            }
        }; self.seen[at] = true; let fact = &self.facts[at]; if fact.terminal { return true; }
        // projectileRecord validates raw samples before CombatCapture.emit prunes.
        // A malformed first grenade therefore leaves later journal rows untouched.
        // Keep this failure order even when another projectile would be valid.
        let result = payload(tick, fact, Some(sample));
        let bytes = match result { Ok(bytes) => bytes, Err(error) => { self.malformed += 1; self.error = Some(error); return false; } };
        if !self.pruned { events.prune(tick); self.pruned = true; }
        if events.len() >= CAPACITY { self.error = Some("combat capture capacity exceeded"); return false; }
        // decodeProjectile sees float32 orientation, not the canonical doubles.
        // Normalize neither value here: invalid authored motion must report a fault.
        // The same rounded position determines delivery range on the receiving side.
        let quaternion = sample.quaternion.map(|n| n as f32 as f64);
        if fact.launch_tick > tick || (v8math::hypot_n(&quaternion) - 1.).abs() > 0.01 {
            self.malformed += 1; self.error = Some("invalid projectile type/orientation"); return false;
        }
        events.emit_raw(fact.id, fact.generation, tick, 6., &bytes, View { schema: 6.,
            position: Some(sample.position.map(|n| n as f32 as f64)), owner: Owner::number(fact.owner_id), life: sample.life.max(0.) as f32 as f64,
            speed: f64::NAN, ..Default::default() }); true
    }
    pub fn take(&mut self, tick: f64, store: &Store, events: &mut Events) {
        self.take_with(tick, store, events, |_| Err("combat definition absent from matched registry"));
    }
    // ProjectileCapture.sample snapshots ordnance at publication cadence.
    // Identity facts are separate from the canonical body and its pool life.
    // A callback-created terminal suppresses samples even if the row still lives.
    pub fn take_with(&mut self, tick: f64, store: &Store, events: &mut Events, mut identify: impl FnMut(Sample) -> Result<WireDynamicFact>) {
        if self.facts.len() > CAPACITY { self.malformed += (self.facts.len() - CAPACITY) as u64; self.facts.truncate(CAPACITY); }
        self.lookup.clear(); self.lookup.extend(0..self.facts.len()); self.seen.fill(false); self.retired.clear(); self.created.clear(); self.error = None; self.pruned = false;
        self.lookup.sort_unstable_by_key(|&i| { let f = &self.facts[i]; (f.r#type, f.slot, f.ref_generation) });
        // A list may still hold a terminal row until its owning phase retires
        // it. Match the old gone/life filter before marking its identity live.
        for &r in &store.ordnance.grenades {
            let Some(p) = store.grenade.get(r).filter(|p| p.gone != Some(true)) else { continue };
            if !self.sample(tick, Sample { definition: p.def, owner: p.owner, channel: p.network_fire_channel, kind: 0, slot: r.slot, generation: r.gen, position: p.position, velocity: p.velocity,
                quaternion: p.quaternion, end: p.position, life: p.fuse.unwrap_or(0.).max(0.) }, events, &mut identify) { return; }
        }
        for &r in &store.ordnance.rounds {
            let Some(p) = store.round.get(r).filter(|p| p.gone != Some(true)) else { continue };
            if !self.sample(tick, Sample { definition: p.def, owner: p.owner, channel: p.network_fire_channel, kind: 1, slot: r.slot, generation: r.gen, position: p.position, velocity: p.velocity,
                quaternion: p.quaternion, end: p.position, life: p.life.max(0.) }, events, &mut identify) { return; }
        }
        for &r in &store.ordnance.beams {
            let Some(p) = store.beam.get(r).filter(|p| p.life > 0.) else { continue };
            if !self.sample(tick, Sample { definition: p.def, owner: p.owner, channel: p.network_fire_channel, kind: 2, slot: r.slot, generation: r.gen, position: p.position, velocity: [0.; 3],
                quaternion: [0., 0., 0., 1.], end: p.end, life: p.life.max(0.) }, events, &mut identify) { return; }
        }
        // Removed identities follow active-journal insertion order, after all
        // live samples. A queued impact suppresses its separate removal event.
        for (at, fact) in self.facts.iter().enumerate() {
            if self.seen[at] { continue; }
            if !fact.terminal {
                let bytes = match payload(tick, fact, None) { Ok(bytes) => bytes, Err(error) => { self.malformed += 1; self.error = Some(error); return; } };
                if !self.pruned { events.prune(tick); self.pruned = true; }
                if events.len() >= CAPACITY { self.error = Some("combat capture capacity exceeded"); return; }
                events.emit_raw(fact.id, fact.generation, tick, 7., &bytes[..4], View { schema: 7., ..Default::default() });
            }
            self.retired.push(fact.id as u32);
        }
    }
}
// projectileRecord writes f32 pose samples, then decodeProjectile sees those
// rounded values for range and lifetime. The own body is never substituted.
// projectileRecord emits the same 82-byte sample for all live kinds.
// Removal emits only expiry; its unused stack tail never enters the wire.
fn payload(tick: f64, fact: &WireDynamicFact, sample: Option<Sample>) -> Result<[u8; 82]> {
    struct Bytes { value: [u8; 82], at: usize }
    impl Bytes {
        fn u32(&mut self, n: f64) -> Result<()> {
            if !n.is_finite() || n.fract() != 0. || n < 0. || n > u32::MAX as f64 { return Err("invalid projectile integer"); }
            self.value[self.at..self.at + 4].copy_from_slice(&(n as u32).to_le_bytes()); self.at += 4; Ok(())
        }
        fn f32(&mut self, n: f64) -> Result<()> {
            if !n.is_finite() || n.abs() > 1e8 { return Err("invalid projectile sample"); }
            self.value[self.at..self.at + 4].copy_from_slice(&(n as f32).to_le_bytes()); self.at += 4; Ok(())
        }
    }
    let mut out = Bytes { value: [0; 82], at: 0 }; out.u32(tick + 18.)?;
    if let Some(sample) = sample {
        out.u32(fact.definition_id)?; out.value[out.at] = sample.kind; out.at += 1;
        for n in [fact.owner_id, fact.owner_generation, fact.fire_id, fact.launch_tick] { out.u32(n)?; }
        for n in sample.position.into_iter().chain(sample.velocity).chain(sample.quaternion).chain(sample.end) { out.f32(n)?; }
        out.f32(sample.life)?; if fact.channel != 0. && fact.channel != 1. { return Err("invalid projectile channel"); }
        out.value[out.at] = fact.channel as u8;
    }
    for n in [fact.id, fact.generation, tick] { if !n.is_finite() || n.fract() != 0. || n < 0. || n > u32::MAX as f64 { return Err("invalid projectile integer"); } }
    Ok(out.value)
}
