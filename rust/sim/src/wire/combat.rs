//! Combat publication bytes and the ordered, expiring producer queue.
//! The owner retains validated callback bytes, identities and queue decisions.
use crate::gen::{WbCall, WbEncode, WbResult};
use crate::store::{StrId, Strings};
type Result<T> = std::result::Result<T, &'static str>;

struct Writer {
    bytes: Vec<u8>,
    at: usize,
}
impl Writer {
    fn new(length: usize) -> Self {
        Self {
            bytes: vec![0; length],
            at: 0,
        }
    }
    fn bytes(&mut self, bytes: &[u8]) -> Result<()> {
        let end = self.at + bytes.len();
        if end > self.bytes.len() {
            return Err("Offset is outside the bounds of the DataView");
        }
        self.bytes[self.at..end].copy_from_slice(bytes);
        self.at = end;
        Ok(())
    }
    fn u32(&mut self, n: f64, error: &'static str) -> Result<()> {
        uint(n, error)?;
        self.bytes(&(n as u32).to_le_bytes())
    }
    fn u8(&mut self, n: f64) -> Result<()> {
        self.bytes(&[jsrt::to_uint32(n) as u8])
    }
    fn i32(&mut self, n: f64) -> Result<()> {
        self.bytes(&jsrt::to_int32(n).to_le_bytes())
    }
    fn f64(&mut self, n: f64, error: &'static str) -> Result<()> {
        if !n.is_finite() {
            return Err(error);
        }
        self.bytes(&n.to_le_bytes())
    }
    fn f32(&mut self, n: f64) -> Result<()> {
        if !n.is_finite() || n.abs() > 1e8 {
            return Err("invalid projectile sample");
        }
        self.bytes(&(n as f32).to_le_bytes())
    }
}
fn uint(n: f64, error: &'static str) -> Result<()> {
    if !n.is_finite() || n.fract() != 0. || !(0. ..=4294967295.).contains(&n) {
        return Err(error);
    }
    Ok(())
}
fn identity(c: &WbEncode, error: &'static str) -> Result<()> {
    for n in [c.id, c.generation, c.tick] {
        uint(n, error)?;
    }
    Ok(())
}

pub fn bolt(c: &WbEncode) -> Result<Vec<u8>> {
    if c.schema != 1. && c.schema != 2. {
        return Err("unsupported combat schema");
    }
    for (array, v) in [
        (c.position_array, &c.position),
        (c.direction_array, &c.direction),
        (c.offset_array, &c.offset),
    ] {
        if !array || v.len() != 3 {
            return Err("invalid combat vector");
        }
    }
    let mut w = Writer::new(if c.schema == 1. { 130 } else { 82 });
    for n in [
        c.expiry_tick,
        c.definition_id,
        c.owner_id,
        c.owner_generation,
        c.fire_id,
    ] {
        w.u32(n, "invalid combat integer")?;
    }
    w.f64(c.launch_tick, "nonfinite combat value")?;
    for n in c.position.iter().chain(&c.direction) {
        w.f64(*n, "nonfinite combat value")?;
    }
    w.u8(c.surface)?;
    if c.schema == 1. {
        for n in &c.offset {
            w.f64(*n, "nonfinite combat value")?;
        }
        for n in [c.speed, c.life, c.merge] {
            w.f64(n, "nonfinite combat value")?;
        }
    }
    w.u32(c.execution_tick, "invalid combat integer")?;
    if c.channel != 0. && c.channel != 1. {
        return Err("invalid combat channel");
    }
    w.u8(c.channel)?;
    identity(c, "invalid combat integer")?;
    w.bytes.truncate(w.at);
    Ok(w.bytes)
}
pub fn feedback(c: &WbEncode) -> Result<Vec<u8>> {
    if c.schema != 3. && c.schema != 4. {
        return Err("unsupported combat feedback");
    }
    let mut w = Writer::new(78);
    for n in [
        c.expiry_tick,
        c.target_id,
        c.target_generation,
        c.attacker_id,
        c.attacker_generation,
    ] {
        w.u32(n, "invalid feedback identity")?;
    }
    for n in &c.position {
        w.f64(*n, "invalid feedback position")?;
    }
    w.i32(c.team)?;
    w.i32(c.attacker_team)?;
    w.bytes(&(c.has_source as u16).to_le_bytes())?;
    if c.has_source {
        for n in &c.source {
            // Array.every skips holes. Iteration subsequently writes those
            // holes as NaN; decodeFeedback is the later validation point.
            w.bytes(&n.to_le_bytes())?;
        }
    } else {
        for _ in 0..3 {
            w.f64(0., "invalid feedback source")?;
        }
    }
    Ok(w.bytes)
}
pub fn explosion(c: &WbEncode) -> Result<Vec<u8>> {
    identity(c, "invalid explosion identity")?;
    uint(c.definition_id, "invalid explosion identity")?;
    uint(c.expiry_tick, "invalid explosion identity")?;
    if !c.position_array || c.position.len() != 3 {
        return Err("invalid explosion position");
    }
    let mut w = Writer::new(33);
    w.u32(c.expiry_tick, "invalid explosion identity")?;
    w.u32(c.definition_id, "invalid explosion identity")?;
    for n in &c.position {
        w.f64(*n, "invalid explosion position")?;
    }
    w.u8(c.wet as u8 as f64)?;
    Ok(w.bytes)
}
pub fn projectile(c: &WbEncode) -> Result<Vec<u8>> {
    let mut w = Writer::new(if c.terminal { 4 } else { 82 });
    w.u32(c.expiry_tick, "invalid projectile integer")?;
    if !c.terminal {
        w.u32(c.definition_id, "invalid projectile integer")?;
        w.u8(c.r#type)?;
        for n in [c.owner_id, c.owner_generation, c.fire_id, c.launch_tick] {
            w.u32(n, "invalid projectile integer")?;
        }
        for list in [&c.position, &c.velocity, &c.quaternion, &c.end] {
            for n in list {
                w.f32(*n)?;
            }
        }
        w.f32(c.life)?;
        if c.channel != 0. && c.channel != 1. {
            return Err("invalid projectile channel");
        }
        w.u8(c.channel)?;
    }
    identity(c, "invalid projectile integer")?;
    Ok(w.bytes)
}
pub fn encode(c: &WbEncode) -> Result<Vec<u8>> {
    match jsrt::to_uint32(c.op) {
        0 => bolt(c),
        1 => feedback(c),
        2 => explosion(c),
        3 => projectile(c),
        _ => Err("unknown combat encoder"),
    }
}

#[derive(Clone, Debug)]
struct Pending {
    key: f64,
    schema: f64,
    expiry_tick: f64,
    saved: Option<Saved>,
}
// CombatCapture.emit keeps the validated payload at its original callback
// point. Publication borrows these bytes without a host round trip.
#[derive(Clone, Debug)]
struct Saved { fact: crate::gen::WireJournalFact, payload: [u8;900], length: usize }
fn same_key(a: f64, b: f64) -> bool {
    a == b || a.is_nan() && b.is_nan()
}
pub struct Capture {
    pending: Vec<Pending>,
    pruned_tick: Option<f64>,
    next_id: f64,
    faults: Vec<StrId>,
    pub dynamics: Vec<crate::gen::WireDynamicFact>,
    pub saturated: u64,
}
impl Default for Capture {
    fn default() -> Self {
        Self {
            pending: Vec::with_capacity(4096),
            pruned_tick: None,
            next_id: 1.,
            faults: Vec::with_capacity(9),
            dynamics: Vec::with_capacity(4096),saturated:0,
        }
    }
}
impl Capture {
    pub fn next_id(&self) -> f64 { self.next_id }
    pub fn set_next_id(&mut self, id: f64) { self.next_id = id; }
    // ProjectileCapture.launch shares this namespace with callbacks emitted
    // earlier in the tick. A late host admission consumes the next id here.
    pub(crate) fn allocate_id(&mut self) -> f64 { let id = self.next_id; self.next_id += 1.; id }
    /// The batch publisher consumed the same journal; no ID-list materialization.
    pub(crate) fn clear(&mut self) { self.pending.clear(); self.dynamics.clear(); }
    pub(crate) fn sampled(&mut self, tick: f64, error: Option<StrId>) {
        self.pruned_tick = Some(tick);
        if let Some(error) = error { self.faults.push(error); if self.faults.len() > 8 { self.faults.remove(0); } }
    }
    pub fn commit(&mut self, key: f64, fact: crate::gen::WireJournalFact, payload: &[f64]) -> bool {
        if payload.len() > 900 { return false; }
        let Some(entry) = self.pending.iter_mut().find(|p| same_key(p.key,key)) else { return false };
        let mut saved=Saved {fact,payload:[0;900],length:payload.len()};
        for (target,&byte) in saved.payload.iter_mut().zip(payload) {*target=jsrt::to_uint32(byte) as u8;}
        entry.saved=Some(saved);true
    }
    pub(crate) fn len(&self)->usize {self.pending.len()}
    pub(crate) fn publish(&self, events: &mut super::journal::Events) {
        for entry in &self.pending { if let Some(saved) = &entry.saved {
            let fact = &saved.fact;
            events.emit_raw(fact.id,fact.generation,fact.tick,fact.schema,&saved.payload[..saved.length],super::cosmetics::View {
                schema:fact.schema,position:fact.has_position.then_some(fact.position),
                owner:super::cosmetics::Owner {tag:fact.owner_tag,value:fact.owner},speed:fact.speed,life:fact.life,..Default::default() });
        }}
    }
    pub fn dynamic(&mut self, fact:crate::gen::WireDynamicFact) -> bool {
        if let Some(old)=self.dynamics.iter_mut().find(|p|same_key(p.id,fact.id)) {*old=fact;true}
        else if self.dynamics.len()<4096 {self.dynamics.push(fact);true} else {self.saturated+=1;false}
    }
    fn saved(&self)->crate::gen::WbSavedResult {
        crate::gen::WbSavedResult { records:self.pending.iter().filter_map(|p|p.saved.as_ref().map(|s|crate::gen::WbSaved {
            key:p.key,fact:s.fact.clone(),payload:s.payload[..s.length].to_vec(),expires_tick:p.expiry_tick})).collect() }
    }
    pub fn run(&mut self, c: WbCall, strings: &mut Strings) -> WbResult {
        let mut out = WbResult::default();
        match jsrt::to_uint32(c.op) {
            0 => {
                // Capacity is checked before payload decoding. Failed records
                // still prune once for this tick, just as successful records do.
                if self.pruned_tick != Some(c.tick) {
                    self.pending.retain(|p| {
                        if p.expiry_tick < c.tick {
                            out.ids.push(p.key);
                            false
                        } else {
                            true
                        }
                    });
                    self.pruned_tick = Some(c.tick);
                }
                out.key = if c.schema == 2.
                    && self
                        .pending
                        .iter()
                        .any(|p| same_key(p.key, c.id) && p.schema == 1.)
                {
                    -c.id
                } else {
                    c.id
                };
                if !self.pending.iter().any(|p| same_key(p.key, out.key))
                    && self.pending.len() as f64 >= c.capacity.min(4096.)
                {
                    self.saturated+=1;out.error = Some(strings.intern_str("combat capture capacity exceeded"));
                }
            }
            1 => {
                let entry = Pending {
                    key: if c.key == 0. { 0. } else { c.key },
                    schema: c.schema,
                    expiry_tick: c.expiry_tick,
                    saved: None,
                };
                if let Some(old) = self.pending.iter_mut().find(|p| same_key(p.key, c.key)) {
                    *old = entry;
                } else if self.pending.len()<4096 {
                    self.pending.push(entry);
                } else {self.saturated+=1;out.error=Some(strings.intern_str("combat capture capacity exceeded"));}
            }
            2 => {
                out.ids = self.pending.drain(..).map(|p| p.key).collect();
            }
            3 => out.ids = self.pending.iter().map(|p| p.key).collect(),
            4 => {}
            5 => self.next_id = c.id,
            6 => {
                let text = strings
                    .units(c.text)
                    .iter()
                    .take(160)
                    .copied()
                    .collect::<Vec<_>>();
                self.faults.push(strings.intern(&text));
                if self.faults.len() > 8 {
                    self.faults.remove(0);
                }
            }
            7 => out.faults = self.faults.drain(..).collect(),
            8 => out.faults = self.faults.clone(),
            _ => out.error = Some(strings.intern_str("unknown combat capture operation")),
        }
        out.next_id = self.next_id;
        out
    }
}

#[no_mangle]
pub extern "C" fn wire_combat_words(encode: u32) -> u32 {
    if encode != 0 {
        WbEncode::WORDS as u32
    } else {
        WbCall::WORDS as u32
    }
}
pub(crate) fn wire_combat_encode_in(w: &mut crate::world::owner::World) -> *const f64 {
    let c = {
        let (row, heap) = w.sim.calls.args();
        WbEncode::take(row, heap)
    };
    super::module::output(&mut w.sim, encode(&c).map(|bytes| vec![bytes]))
}
pub(crate) fn wire_combat_create_in(w: &mut crate::world::owner::World) -> *mut Capture {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");

    w.sim.strings.intern_str("combat capture capacity exceeded");
    owner.combat.insert(Capture::default()).unwrap_or(0) as *mut Capture

}
/// CombatCapture.nextId is a scalar journal cursor. Its getter and setter
/// preserve separate observations around synchronous callback work without
/// serializing a complete queue result for each half of a postfix increment.
/// # Safety
/// The handle belongs to this module instance and remains live for the call.
pub(crate) unsafe fn wire_combat_next_id_in(w: &mut crate::world::owner::World, handle: *const Capture) -> f64 {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.combat.get(handle as usize).is_none() {return f64::NAN;}

    owner.combat.get_mut(handle as usize).unwrap().next_id

}
/// # Safety
/// The live handle is exclusive for this operation. Keep the supplied IEEE
/// value: validation still belongs to the subsequent record encoder.
pub(crate) unsafe fn wire_combat_set_next_id_in(w: &mut crate::world::owner::World, handle: *mut Capture, id: f64) {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.combat.get(handle as usize).is_none() {return;}

    owner.combat.get_mut(handle as usize).unwrap().next_id = id;

}
/// # Safety
/// The handle belongs to this module instance and is released once.
pub(crate) unsafe fn wire_combat_drop_in(w: &mut crate::world::owner::World, handle: *mut Capture) {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.combat.get(handle as usize).is_none() {return;}

    if !handle.is_null() {
        owner.combat.remove(handle as usize);
    }

}
/// # Safety
/// The live handle is exclusive for this operation. Inputs are owned and
/// no simulation callback runs until the operation has returned.
pub(crate) unsafe fn wire_combat_run_in(w: &mut crate::world::owner::World, handle: *mut Capture) -> *const f64 {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.combat.get(handle as usize).is_none() {return std::ptr::null();}

    let c = {
        let (row, heap) = w.sim.calls.args();
        WbCall::take(row, heap)
    };
    let s = &mut w.sim;
    let out = owner.combat.get_mut(handle as usize).unwrap().run(c, &mut s.strings);
    s.calls
        .result(WbResult::WORDS, |row, heap| out.put(row, heap));
    s.calls.block.ptr()

}

/// Ordered callback ingress commits a record before the next observer runs.
pub(crate) fn wire_combat_commit_in(w: &mut crate::world::owner::World, handle:usize) ->u32 {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
    let Some(c)=owner.combat.get_mut(handle) else{return 0};
    let (row,heap)=w.sim.calls.args();
    use crate::gen::WbCommit as C;
    if row.len()<C::WORDS {return 0;}
    let at=row[C::PAYLOAD];let length=row[C::PAYLOAD+1];
    let valid=|n:f64,max:usize|n.is_finite()&&n>=0.&&n.fract()==0.&&n<=max as f64;
    if !valid(at,heap.len())||!valid(length,900) {return 0;}
    let Some(payload)=heap.get(at as usize..(at as usize).saturating_add(length as usize)) else{return 0};
    c.commit(row[C::KEY],crate::gen::WireJournalFact::take(&row[C::FACT..],&[]),payload) as u32

}
pub(crate) fn wire_combat_dynamic_in(w: &mut crate::world::owner::World, handle:usize) ->u32 {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
    let Some(c)=owner.combat.get_mut(handle) else{return 0};
    let (row,heap)=w.sim.calls.args();
    if row.len()<crate::gen::WireDynamicFact::WORDS {return 0;}
    c.dynamic(crate::gen::WireDynamicFact::take(row,heap)) as u32

}
/// Diagnostic/drain materialization is separate from publication's borrow.
pub(crate) fn wire_combat_saved_in(w: &mut crate::world::owner::World, handle:usize,drain:u32) ->*const f64 {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
    let Some(c)=owner.combat.get_mut(handle) else{return std::ptr::null()};
    let result=c.saved();if drain!=0 {c.pending.clear();}
    let sim=&mut w.sim;sim.calls.result(crate::gen::WbSavedResult::WORDS,|row,heap|result.put(row,heap));sim.calls.block.ptr()

}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scalar_cursor_keeps_the_queue_operations_ieee_values() {
        let mut c = Capture::default();
        let mut strings = Strings::default();
        for value in [-0., 1., 4294967295., 4294967296., f64::INFINITY, f64::NAN] {
            c.set_next_id(value);
            let old = c.run(WbCall { op: 4., ..Default::default() }, &mut strings);
            assert_eq!(c.next_id().to_bits(), old.next_id.to_bits());
            c.run(WbCall { op: 5., id: value, ..Default::default() }, &mut strings);
            assert_eq!(c.next_id().to_bits(), value.to_bits());
        }
    }
    #[test]
    fn terminal_queues_beside_launch_and_capacity_precedes_decode() {
        let mut c = Capture::default();
        let mut strings = Strings::default();
        let launch = WbCall {
            op: 1.,
            key: 4.,
            schema: 1.,
            expiry_tick: 9.,
            ..WbCall::default()
        };
        c.run(launch, &mut strings);
        let terminal = WbCall {
            op: 0.,
            id: 4.,
            schema: 2.,
            tick: 3.,
            capacity: 1.,
            ..WbCall::default()
        };
        let out = c.run(terminal.clone(), &mut strings);
        assert_eq!(out.key, -4.);
        assert!(out.error.is_some());
        let out = c.run(
            WbCall {
                tick: 10.,
                ..terminal
            },
            &mut strings,
        );
        assert_eq!(out.ids, [4.]);
        assert_eq!(out.key, 4.);
        assert!(out.error.is_none());
    }
    #[test]
    fn fixed_payloads_keep_zero_padding_and_scalar_precedence() {
        let mut c = WbEncode {
            id: 1.,
            generation: 2.,
            tick: 3.,
            expiry_tick: 21.,
            definition_id: 4.,
            position_array: true,
            position: vec![1., -0., 3.],
            ..WbEncode::default()
        };
        let b = explosion(&c).unwrap();
        assert_eq!(b.len(), 33);
        assert_eq!(&b[16..24], &(-0.0f64).to_le_bytes());
        c.position[0] = f64::NAN;
        c.id = -1.;
        assert_eq!(explosion(&c), Err("invalid explosion identity"));
        c.id = 1.;
        c.schema = 3.;
        c.position = vec![1., 2.];
        c.team = -1.;
        c.attacker_team = -1.;
        let b = feedback(&c).unwrap();
        assert_eq!(b.len(), 78);
        assert_eq!(&b[70..], &[0; 8]);
    }
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn wire_combat_encode() -> *const f64 { wire_combat_encode_in(crate::abi::world()) }
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn wire_combat_create() -> *mut Capture { wire_combat_create_in(crate::abi::world()) }
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub unsafe extern "C" fn wire_combat_next_id(handle: *const Capture) -> f64 { wire_combat_next_id_in(crate::abi::world(), handle) }
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub unsafe extern "C" fn wire_combat_set_next_id(handle: *mut Capture, id: f64) { wire_combat_set_next_id_in(crate::abi::world(), handle, id) }
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub unsafe extern "C" fn wire_combat_drop(handle: *mut Capture) { wire_combat_drop_in(crate::abi::world(), handle) }
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub unsafe extern "C" fn wire_combat_run(handle: *mut Capture) -> *const f64 { wire_combat_run_in(crate::abi::world(), handle) }
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn wire_combat_commit(handle:usize) ->u32 { wire_combat_commit_in(crate::abi::world(), handle) }
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn wire_combat_dynamic(handle:usize) ->u32 { wire_combat_dynamic_in(crate::abi::world(), handle) }
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn wire_combat_saved(handle:usize,drain:u32) ->*const f64 { wire_combat_saved_in(crate::abi::world(), handle, drain) }
