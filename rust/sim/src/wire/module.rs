//! The generated exchange owns all inputs before encoding. No callback is
//! made while a runtime, the store or the exchange block is borrowed.
use super::codec;
use crate::abi::sim;
use crate::gen::*;

pub fn output(result: codec::Result<Vec<Vec<u8>>>) -> *const f64 {
    let s = sim();
    let out = match result {
        Ok(parts) => WireOutput {
            error: None,
            parts: parts.into_iter().map(|bytes| WireBytes { bytes }).collect(),
        },
        Err(error) => WireOutput {
            error: Some(s.strings.intern_str(error)),
            parts: Vec::new(),
        },
    };
    s.calls
        .result(WireOutput::WORDS, |row, heap| out.put(row, heap));
    s.calls.block.ptr()
}
/// wire_held consumes Encoder's retained datagrams directly into the generated
/// result layout. It must not clone each packet merely to serialize the list.
fn output_retained(result: codec::Result<&[Vec<u8>]>) -> *const f64 {
    let s = sim();
    let (parts, error) = match result {
        Ok(parts) => (parts, None),
        Err(error) => (&[][..], Some(s.strings.intern_str(error))),
    };
    s.calls.result(WireOutput::WORDS, |row, heap| {
        row[WireOutput::ERROR] = if error.is_some() { 0. } else { 1. };
        row[WireOutput::ERROR + 1] = error.map_or(0., |id| id.0 as f64);
        row[WireOutput::PARTS] = 0.; row[WireOutput::PARTS + 1] = parts.len() as f64;
        heap.resize(parts.len() * WireBytes::WORDS, 0.);
        for (index, bytes) in parts.iter().enumerate() {
            let row = index * WireBytes::WORDS;
            heap[row + WireBytes::BYTES] = heap.len() as f64;
            heap[row + WireBytes::BYTES + 1] = bytes.len() as f64;
            heap.extend(bytes.iter().map(|byte| *byte as f64));
        }
    });
    s.calls.block.ptr()
}

#[no_mangle]
pub extern "C" fn wire_words(op: u32) -> u32 {
    (match op {
        0 => WireEncode::WORDS,
        1 => WireDelta::WORDS,
        2 => WireWorld::WORDS,
        3 => WireOwn::WORDS,
        4 => WireCapture::WORDS,
        _ => 0,
    }) as u32
}
#[no_mangle]
pub extern "C" fn wire_encode(op: u32) -> *const f64 {
    let (row, heap) = sim().calls.args();
    let result = match op {
        0 => {
            let a = WireEncode::take(row, heap);
            codec::encode_entities(&a.current, a.baseline.as_ref(), a.interval)
        }
        1 => {
            let a = WireDelta::take(row, heap);
            codec::word_delta(&a.current, a.baseline.as_deref()).map(|b| vec![b])
        }
        2 => {
            let a = WireWorld::take(row, heap);
            codec::world_section(&a.current, a.baseline.as_deref().unwrap_or(&[])).map(|b| vec![b])
        }
        3 => {
            let a = WireOwn::take(row, heap);
            codec::own_datagram(&a).map(|b| vec![b])
        }
        _ => Err("unknown wire operation"),
    };
    output(result)
}

/// A host owns a separate runtime inside its battle's module. Its snapshots
/// never alias the generated call block or another host's baseline ring.
pub struct Runtime {
    pub ring: super::ring::SnapshotRing<WireSnapshot>,
    pub slots: super::records::SnapshotSlotTable,
    pub previous: Option<WireSnapshot>,
    encoder: super::buffered::Encoder,
    retained: Option<super::retained::Ring>,
}
impl Default for Runtime {
    fn default() -> Self {
        Self { ring: Default::default(), slots: Default::default(), previous: None,
            encoder: super::buffered::Encoder::new(1024), retained: None }
    }
}
impl Runtime {
    pub fn capture(
        &mut self,
        input: WireCapture,
        strings: &crate::store::Strings,
    ) -> codec::Result<()> {
        self.capture_records(input.tick, input.epoch, &input.records, input.world, &input.bounds, strings)
    }
    fn capture_records(&mut self, tick: f64, epoch: f64, records: &[WireRecord], world: Vec<WireField>,
        bounds: &WireBounds, strings: &crate::store::Strings) -> codec::Result<()> {
        if !tick.is_finite()
            || tick.fract() != 0.
            || tick.abs() > 9007199254740991.
        {
            return Err("invalid snapshot ring tick");
        }
        let snapshot = super::records::snapshot_from_records(
            tick,
            epoch,
            records,
            world,
            &mut self.slots,
            bounds,
            self.previous.as_ref(),
            strings,
        )?;
        self.ring
            .set(tick as i64, epoch as u32, snapshot.clone())?;
        self.previous = Some(snapshot);
        Ok(())
    }
    /// Reserve a canonical host's complete retention window at attachment.
    pub fn reserve_world(&mut self, seats: usize, fields: usize) -> codec::Result<()> {
        if self.retained.is_none() { self.retained = Some(super::retained::Ring::new(seats, fields, 128)?); } Ok(())
    }
    pub fn capture_canonical<'a>(&mut self, tick: f64, epoch: f64, records: &[WireRecord], fields: impl Iterator<Item = &'a WireField>, strings: &crate::store::Strings) -> codec::Result<()> {
        self.retained.as_mut().ok_or("canonical snapshot ring not attached")?.capture(tick, epoch, records, fields,
            &mut self.slots, &super::records::default_bounds(), strings)
    }
    pub fn latest(&self) -> Option<&WireSnapshot> { self.retained.as_ref().and_then(|r| r.latest()).or(self.previous.as_ref()) }
    pub fn encode(
        &mut self,
        tick: f64,
        epoch: f64,
        baseline: f64,
        interval: f64,
    ) -> codec::Result<&[Vec<u8>]> {
        let (current, base) = if let Some(ring) = &mut self.retained {
            ring.pair(tick, epoch, (baseline != 0.).then_some(baseline))?
        } else {
            let current = self.ring.get(tick as i64, Some(epoch as u32)).ok_or("snapshot not held")?;
            let base = if baseline == 0. { None } else { Some(self.ring.get(baseline as i64, Some(epoch as u32)).ok_or("chosen snapshot baseline not held")?) };
            (current, base)
        };
        self.encoder.encode(current, base, interval)
    }
}
#[no_mangle]
pub extern "C" fn wire_create() -> *mut Runtime {
    super::owner::with(|owner| {

    owner.runtimes.insert(Runtime::default()).unwrap_or(0) as *mut Runtime

    })
}
/// Bind the canonical producer before its first publication. Ring banks
/// reserve the loaded content's maximum crew size at this cold boundary.
/// # Safety
/// Both allocations belong to this simulation instance and remain live.
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub unsafe extern "C" fn wire_bind_store(runtime: *mut Runtime, source: *const super::store_source::Source) -> u32 {
    super::owner::with(|owner| {
if owner.runtimes.get(runtime as usize).is_none() {return 0;}
if owner.sources.get(source as usize).is_none() {return 0;}

    if runtime.is_null() || source.is_null() { return 0; }
    let runtime = owner.runtimes.get_mut(runtime as usize).unwrap();
    if runtime.retained.is_some() { return 1; }
    let Ok(ring) = super::retained::Ring::new((owner.sources.get(source as usize).unwrap()).capture.seat_capacity(), 1024, 128) else { return 0 };
    runtime.retained = Some(ring); 1

    })
}
/// # Safety
/// The handle names a wire runtime in this module instance.
#[no_mangle]
pub unsafe extern "C" fn wire_drop(runtime: *mut Runtime) {
    super::owner::with(|owner| {
if owner.runtimes.get(runtime as usize).is_none() {return;}

    if !runtime.is_null() {
        owner.runtimes.remove(runtime as usize);
    }

    })
}
/// # Safety
/// The handle belongs to this module instance for the call.
#[no_mangle]
pub unsafe extern "C" fn wire_capture(runtime: *mut Runtime) -> *const f64 {
    super::owner::with(|owner| {
if owner.runtimes.get(runtime as usize).is_none() {return std::ptr::null();}

    let input = {
        let (row, heap) = sim().calls.args();
        WireCapture::take(row, heap)
    };
    let result = owner.runtimes.get_mut(runtime as usize).unwrap()
        .capture(input, &sim().strings)
        .map(|_| Vec::new());
    output(result)

    })
}
/// # Safety
/// The handle belongs to this module instance for the call.
#[no_mangle]
pub unsafe extern "C" fn wire_held(
    runtime: *mut Runtime,
    tick: f64,
    epoch: f64,
    baseline: f64,
    interval: f64,
) -> *const f64 {
    super::owner::with(|owner| {
if owner.runtimes.get(runtime as usize).is_none() {return std::ptr::null();}

    output_retained(owner.runtimes.get_mut(runtime as usize).unwrap().encode(tick, epoch, baseline, interval))

    })
}
