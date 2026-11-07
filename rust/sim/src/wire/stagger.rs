//! World summary refresh cadence, preserving first-offer Map order and
//! identity skips, including fields seen before their first due tick.
use crate::gen::{WireField, WireStaggerTake};
use jsrt::{same_value_zero, to_uint32};
#[derive(Clone)]
pub struct Entry {
    pub schema: f64,
    pub id: f64,
    pub token: f64,
    pub field: Option<WireField>,
    pending: Option<WireField>,
    native_state: Option<(u8, u64)>,
    pub state_team: Option<Vec<u16>>,
    pub round: f64,
    pub due: f64,
}
#[derive(Default)]
pub struct Stagger {
    pub entries: Vec<Entry>,
    pub cadence: Vec<(f64, f64)>,
    pub round: f64,
    native_payloads: Vec<Vec<u8>>,
}
impl Stagger {
    /// Own summaries refresh together every six ticks. Their two fields
    /// publish immediately, unlike independently staggered common fields.
    pub fn own() -> Self {
        Self { entries: Vec::with_capacity(2), native_payloads: (0..2).map(|_| Vec::with_capacity(900)).collect(),
            cadence: vec![(3., 1.), (5., 1.)], ..Default::default() }
    }
    /// Personal summary scratch serves several clients at the same tick.
    /// Return its payloads before the next client so a previous client's
    /// refresh deadline cannot hold the following client's bytes.
    pub fn clear_native(&mut self) -> super::codec::Result<()> {
        if self.entries.iter().any(|e| e.token > 0. || e.state_team.is_some()) {
            return Err("foreign world fields cannot enter canonical capture");
        }
        for entry in self.entries.drain(..) {
            if let Some(field) = entry.field.or(entry.pending) { let mut bytes = field.payload; bytes.clear(); self.native_payloads.push(bytes); }
        }
        Ok(())
    }
    pub fn new() -> Self {
        let mut out = Self {
            entries: Vec::with_capacity(384),
            native_payloads: (0..192).map(|_| Vec::with_capacity(900)).collect(),
            cadence: vec![
                (1., 6.),
                (2., 6.),
                (4., 60.),
                (5., 300.),
                (6., 30.),
                (7., 48.),
                (8., 12.),
                (9., 60.),
                (10., 60.),
            ],
            ..Self::default()
        };
        out.cadence.reserve(256 - out.cadence.len()); out
    }
    fn find(&self, schema: f64, id: f64) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|e| same_value_zero(e.schema, schema) && same_value_zero(e.id, id))
    }
    /// A failed producer must not advance any world-field clocks. Source
    /// capture prepares this retained bank, then swaps it into the committed
    /// stagger only after every field builder succeeds.
    pub fn copy_native_from(&mut self, other: &Self) -> super::codec::Result<()> {
        if other.entries.iter().any(|entry| entry.token > 0. || entry.state_team.is_some()) {
            return Err("foreign world fields cannot enter canonical capture");
        }
        if other.cadence.len() > self.cadence.capacity() { return Err("world cadence capacity exceeded"); }
        let bank = &mut self.native_payloads;
        for entry in self.entries.drain(..) {
            if let Some(field) = entry.field.or(entry.pending) { let mut bytes = field.payload; bytes.clear(); bank.push(bytes); }
        }
        self.round = other.round; self.cadence.clear(); self.cadence.extend_from_slice(&other.cadence);
        for old in &other.entries {
            if self.entries.len() == self.entries.capacity() { return Err("world field capacity exceeded"); }
            let source = old.field.as_ref().or(old.pending.as_ref());
            let next = if let Some(field) = source {
                let mut payload = bank.pop().ok_or("world presentation capacity exceeded")?;
                if field.payload.len() > payload.capacity() { bank.push(payload); return Err("World presentation record too large"); }
                payload.clear(); payload.extend_from_slice(&field.payload);
                Some(WireField { id: field.id, schema: field.schema, generation: field.generation, payload })
            } else { None };
            let (field, pending) = if old.field.is_some() { (next, None) } else { (None, next) };
            self.entries.push(Entry { schema: old.schema, id: old.id, token: old.token, field, pending,
                native_state: old.native_state, state_team: None, round: old.round, due: old.due });
        }
        Ok(())
    }
    pub fn begin(&mut self) { self.round += 1.; }
    pub fn offer(&mut self, tick: f64, record: crate::gen::WireStaggerRecord) {
        let round = self.round;
        let f = record.field;
        let schema = f.schema;
        let cadence = self
            .cadence
            .iter()
            .find(|(s, _)| *s == schema)
            .map_or(6., |x| x.1);
        let at = match self
            .entries
            .iter()
            .position(|e| same_value_zero(e.schema, schema) && same_value_zero(e.id, f.id))
        {
            Some(i) => i,
            None => {
                self.entries.push(Entry {
                    schema,
                    id: f.id,
                    token: 0.,
                    field: None, pending: None, native_state: None,
                    state_team: None,
                    round,
                    due: if schema == 9. || schema == 10. {
                        tick
                    } else {
                        tick + (to_uint32(f.id * 2654435761.) as f64) % cadence
                    },
                });
                self.entries.len() - 1
            }
        };
        let e = &mut self.entries[at];
        e.round = round;
        if record.token == e.token && tick < e.due {
            return;
        }
        let state_team = if schema == 7. && e.field.is_some() {
            super::json::building_state_team(&f.payload)
        } else {
            None
        };
        let immediate = schema == 7. && e.field.is_some() && state_team != e.state_team
            || (schema == 9. || schema == 10.)
                && e.field.as_ref().is_some_and(|b| b.payload != f.payload);
        if immediate || tick >= e.due {
            e.token = record.token;
            e.due = tick + cadence;
            if schema == 7. {
                e.state_team =
                    state_team.or_else(|| super::json::building_state_team(&f.payload));
            }
            e.field = Some(f);
        }
    }
    pub fn finish(&mut self, complete: &[f64]) {
        let bank = &mut self.native_payloads; let round = self.round;
        self.entries.retain_mut(|entry| {
            let remove = entry.round != round && entry.field.as_ref().is_some_and(|field| complete.contains(&field.schema));
            if remove && entry.token < 0. {
                if let Some(field) = entry.field.take() { let mut bytes = field.payload; bytes.clear(); bank.push(bytes); }
            }
            // An offered-but-not-yet-due field remains even when absent, just
            // as JS sweeps entry.record?.schema. Native payloads recycle only
            // after a published field actually leaves a complete schema.
            !remove
        });
    }
    pub fn take(&mut self, a: WireStaggerTake) {
        self.begin();
        for record in a.records { self.offer(a.tick, record); }
        self.finish(&a.complete);
    }
    /// captureWorldPresentation checks held values before building their JSON.
    /// Mark kept entries for the complete-schema sweep without importing them
    /// into a host Map. Creatures and traps still compare every capture.
    pub fn needs_native(&mut self, tick: f64, schema: f64, id: f64, state: Option<(u8, u64)>) -> bool {
        let round = self.round;
        let Some(entry) = self.entries.iter_mut().find(|e| e.schema == schema && e.id == id) else { return true };
        entry.round = round;
        tick >= entry.due || schema == 9. || schema == 10.
            || schema == 7. && entry.field.is_some() && entry.native_state != state
    }
    /// Fields produced from the store retain their payloads and use native
    /// identities. Both pending births and published fields have reserved bytes;
    /// a first offer waits for its hashed cadence unless it is schema 9 or 10.
    pub fn offer_native(&mut self, tick: f64, schema: f64, id: f64, generation: f64,
        payload: &[u8], state: Option<(u8, u64)>) -> super::codec::Result<()> {
        if payload.len() > 900 { return Err("World presentation record too large"); }
        let cadence = self.cadence.iter().find(|(s, _)| *s == schema).map_or(6., |s| s.1);
        let at = if let Some(at) = self.entries.iter().position(|e| e.schema == schema && e.id == id) { at }
        else {
            if self.entries.len() == self.entries.capacity() { return Err("world field capacity exceeded"); }
            let bytes = self.native_payloads.pop().ok_or("world presentation capacity exceeded")?;
            self.entries.push(Entry { schema, id, token: 0., field: None, pending: Some(WireField {
                id, schema, generation, payload: bytes }), native_state: None, state_team: None, round: self.round,
                due: if schema == 9. || schema == 10. { tick } else { tick + (to_uint32(id * 2654435761.) as f64) % cadence } });
            self.entries.len() - 1
        };
        let entry = &mut self.entries[at]; entry.round = self.round;
        let immediate = entry.field.as_ref().is_some_and(|field|
            schema == 7. && entry.native_state != state || (schema == 9. || schema == 10.) && field.payload != payload);
        if immediate || tick >= entry.due {
            if entry.field.is_none() { entry.field = entry.pending.take(); }
            let field = entry.field.as_mut().ok_or("missing world presentation storage")?;
            field.generation = generation; field.payload.clear(); field.payload.extend_from_slice(payload);
            entry.token = -(schema * 4294967296. + id); entry.native_state = state;
            entry.due = tick + cadence;
        }
        Ok(())
    }
    pub fn offer_unit(&mut self, tick: f64, id: f64, generation: f64, payload: &[u8]) -> super::codec::Result<()> {
        self.offer_native(tick, 6., id, generation, payload, None)
    }
    pub fn tokens(&self) -> Vec<f64> {
        self.entries
            .iter()
            .filter(|e| e.field.is_some())
            .map(|e| e.token)
            .collect()
    }
    pub fn world(&self) -> Vec<WireField> {
        self.entries
            .iter()
            .filter_map(|e| e.field.clone())
            .collect()
    }
    /// -1 asks for fresh capture; zero is a held field not taken yet.
    pub fn held(&self, tick: f64, schema: f64, id: f64, state_team: Option<&[u16]>) -> f64 {
        let Some(e) = self.find(schema, id) else {
            return -1.;
        };
        if tick >= e.due
            || e.field.is_some()
                && (if schema == 7. {
                    state_team != e.state_team.as_deref()
                } else {
                    schema == 9. || schema == 10.
                })
        {
            -1.
        } else {
            e.token
        }
    }
    pub fn due(&self, tick: f64, schema: f64, id: f64) -> bool {
        self.find(schema, id).is_none_or(|e| tick >= e.due)
    }
    pub fn any_due(&self, tick: f64, schemas: &[f64]) -> bool {
        self.entries.is_empty()
            || self
                .entries
                .iter()
                .any(|e| schemas.contains(&e.schema) && e.field.is_some() && tick >= e.due)
    }
    pub fn schema_due(&self, tick: f64, schemas: &[f64]) -> bool {
        let mut seen = false;
        for e in &self.entries {
            if schemas.contains(&e.schema) {
                seen = true;
                if tick >= e.due {
                    return true;
                }
            }
        }
        !seen
    }
}
use crate::{
    gen::{WireCadences, WireHostList},
    store::StrId,
};
fn list(s: &mut crate::abi::Sim, values: Vec<f64>) -> *const f64 {
    let out = WireHostList { values };
    s.calls
        .result(WireHostList::WORDS, |row, heap| out.put(row, heap));
    s.calls.block.ptr()
}
pub(crate) fn wire_stagger_create_in(w: &mut crate::world::owner::World) -> *mut Stagger {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");

    owner.staggers.insert(Stagger::new()).unwrap_or(0) as *mut Stagger

}
pub(crate) unsafe fn wire_stagger_drop_in(w: &mut crate::world::owner::World, p: *mut Stagger) {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.staggers.get(p as usize).is_none() {return;}

    if !p.is_null() {
        owner.staggers.remove(p as usize);
    }

}
#[no_mangle]
pub extern "C" fn wire_stagger_words(op: u32) -> u32 {
    match op {
        0 => WireStaggerTake::WORDS as u32,
        1 => WireHostList::WORDS as u32,
        2 => WireCadences::WORDS as u32,
        _ => 0,
    }
}
pub(crate) unsafe fn wire_stagger_take_in(w: &mut crate::world::owner::World, p: *mut Stagger) -> *const f64 {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.staggers.get(p as usize).is_none() {return std::ptr::null();}

    let a = {
        let (row, heap) = w.sim.calls.args();
        WireStaggerTake::take(row, heap)
    };
    let s = owner.staggers.get_mut(p as usize).unwrap();
    s.take(a);
    list(&mut w.sim, s.tokens())

}
pub(crate) unsafe fn wire_stagger_cadence_in(w: &mut crate::world::owner::World, p: *mut Stagger) {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.staggers.get(p as usize).is_none() {return;}

    let a = {
        let (row, heap) = w.sim.calls.args();
        WireCadences::take(row, heap)
    };
    (owner.staggers.get_mut(p as usize).unwrap()).cadence = a.values.into_iter().map(|v| (v.schema, v.ticks)).collect();

}
pub(crate) unsafe fn wire_stagger_held_in(w: &mut crate::world::owner::World, p: *const Stagger,
    tick: f64,
    schema: f64,
    id: f64,
    state: i32,) -> f64 {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.staggers.get(p as usize).is_none() {return f64::NAN;}

    let state = if state < 0 {
        None
    } else {
        Some(w.sim.strings.units(StrId(state as u32)))
    };
    (owner.staggers.get(p as usize).unwrap()).held(tick, schema, id, state)

}
pub(crate) unsafe fn wire_stagger_due_in(w: &mut crate::world::owner::World, p: *const Stagger,
    tick: f64,
    schema: f64,
    id: f64,) -> u32 {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.staggers.get(p as usize).is_none() {return 0;}

    (owner.staggers.get(p as usize).unwrap()).due(tick, schema, id) as u32

}
pub(crate) unsafe fn wire_stagger_schemas_in(w: &mut crate::world::owner::World, p: *const Stagger, tick: f64, any: u32) -> u32 {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.staggers.get(p as usize).is_none() {return 0;}

    let a = {
        let (row, heap) = w.sim.calls.args();
        WireHostList::take(row, heap)
    };
    if any != 0 {
        (owner.staggers.get(p as usize).unwrap()).any_due(tick, &a.values) as u32
    } else {
        (owner.staggers.get(p as usize).unwrap()).schema_due(tick, &a.values) as u32
    }

}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn wire_stagger_create() -> *mut Stagger { wire_stagger_create_in(crate::abi::world()) }
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub unsafe extern "C" fn wire_stagger_drop(p: *mut Stagger) { wire_stagger_drop_in(crate::abi::world(), p) }
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub unsafe extern "C" fn wire_stagger_take(p: *mut Stagger) -> *const f64 { wire_stagger_take_in(crate::abi::world(), p) }
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub unsafe extern "C" fn wire_stagger_cadence(p: *mut Stagger) { wire_stagger_cadence_in(crate::abi::world(), p) }
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub unsafe extern "C" fn wire_stagger_held(p: *const Stagger,
    tick: f64,
    schema: f64,
    id: f64,
    state: i32,) -> f64 { wire_stagger_held_in(crate::abi::world(), p, tick, schema, id, state) }
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub unsafe extern "C" fn wire_stagger_due(p: *const Stagger,
    tick: f64,
    schema: f64,
    id: f64,) -> u32 { wire_stagger_due_in(crate::abi::world(), p, tick, schema, id) }
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub unsafe extern "C" fn wire_stagger_schemas(p: *const Stagger, tick: f64, any: u32) -> u32 { wire_stagger_schemas_in(crate::abi::world(), p, tick, any) }
