//! The generated exchange owns all inputs before encoding. No callback is
//! made while a runtime, the store or the exchange block is borrowed.
use super::{codec, PositionBounds};
use crate::gen::*;

pub fn output(s: &mut crate::abi::Sim, result: codec::Result<Vec<Vec<u8>>>) -> Option<&[f64]> {
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
    Some(&s.calls.block.words[..])
}
/// wire_held consumes Encoder's retained datagrams directly into the generated
/// result layout. It must not clone each packet merely to serialize the list.
fn output_retained<'a>(s: &'a mut crate::abi::Sim, result: codec::Result<&[Vec<u8>]>) -> Option<&'a [f64]> {
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
    Some(&s.calls.block.words[..])
}
pub fn wire_words(op: u32) -> u32 {
    (match op {
        0 => WireEncode::WORDS,
        1 => WireDelta::WORDS,
        2 => WireWorld::WORDS,
        3 => WireOwn::WORDS,
        4 => WireCapture::WORDS,
        _ => 0,
    }) as u32
}

pub(crate) fn wire_encode_in(w: &mut crate::world::owner::World, op: u32) -> Option<&[f64]> {
    let (row, heap) = w.sim.calls.args();
    let result = match op {
        0 => {
            let a = WireEncode::take(row, heap);
            let bounds = w.wire.as_deref().expect("admitted wire owner").bounds();
            codec::encode_entities(&a.current, a.baseline.as_ref(), a.interval, bounds.width())
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
    output(&mut w.sim, result)
}

/// A host owns a separate runtime inside its battle's module. Its snapshots
/// never alias the generated call block or another host's baseline ring.
pub struct Runtime {
    pub ring: super::ring::SnapshotRing<WireSnapshot>,
    pub slots: super::records::SnapshotSlotTable,
    pub previous: Option<WireSnapshot>,
    pub(super) bounds: PositionBounds,
    pub(super) capacity: super::bounds::Capacity,
    encoder: super::buffered::Encoder,
    retained: Option<super::retained::Ring>,
}
impl Default for Runtime {
    fn default() -> Self {
        Self::with_bounds(PositionBounds::LEGACY)
    }
}
impl Runtime {
    pub(super) fn with_bounds(bounds: PositionBounds) -> Self { Self::for_battle(bounds, super::bounds::Capacity::LEGACY) }
    /// A runtime for a battle's position box and publication capacity.
    pub fn for_battle(bounds: PositionBounds, capacity: super::bounds::Capacity) -> Self {
        Self { ring: Default::default(), slots: Default::default(), previous: None,
            bounds, capacity, encoder: super::buffered::Encoder::with_capacity(1024, capacity), retained: None }
    }
    pub fn capture(
        &mut self,
        input: WireCapture,
        strings: &crate::store::Strings,
    ) -> codec::Result<()> {
        let b = input.bounds;
        self.bounds.check_box([b.x_min, b.y_min, b.z_min], [b.x_extent, b.y_extent, b.z_extent])?;
        let WireCapture { tick, epoch, records, world, .. } = input;
        if !tick.is_finite()
            || tick.fract() != 0.
            || tick.abs() > 9007199254740991.
        {
            return Err("invalid snapshot ring tick");
        }
        let snapshot = super::records::snapshot_from_records(
            tick,
            epoch,
            &records,
            world,
            &mut self.slots,
            &self.bounds,
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
        if self.retained.is_none() { self.retained = Some(super::retained::Ring::with_capacity(seats, fields, 128, self.capacity)?); } Ok(())
    }
    pub fn capture_canonical<'a>(&mut self, tick: f64, epoch: f64, records: &[WireRecord], fields: impl Iterator<Item = &'a WireField>, strings: &crate::store::Strings) -> codec::Result<()> {
        self.retained.as_mut().ok_or("canonical snapshot ring not attached")?.capture(tick, epoch, records, fields,
            &mut self.slots, &self.bounds, strings)
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
        self.encoder.encode(current, base, interval, self.bounds.width())
    }
}
pub(crate) fn wire_create_in(w: &mut crate::world::owner::World, args: &[f64]) -> usize {
    let bounds = match args {
        [] => w.wire.as_deref().expect("admitted wire owner").bounds(),
        &[x, y, z, dx, dy, dz] => {
            let Some(bounds) = PositionBounds::from_padded([x, y, z], [dx, dy, dz]) else { return 0 };
            bounds
        }
        // A 2005 battle's creation also names its slot domain.
        &[x, y, z, dx, dy, dz, slots] => {
            let Some(bounds) = PositionBounds::from_padded([x, y, z], [dx, dy, dz]) else { return 0 };
            if slots != super::bounds::Capacity::FULL.slots as f64 { return 0; }
            return wire_create_for_battle_in(w, bounds, super::bounds::Capacity::FULL);
        }
        _ => return 0,
    };
    wire_create_with_bounds_in(w, bounds)
}

pub(crate) fn wire_create_with_bounds_in(w: &mut crate::world::owner::World, bounds: PositionBounds) -> usize {
    wire_create_for_battle_in(w, bounds, super::bounds::Capacity::LEGACY)
}
/// A runtime with the battle's box and publication capacity.
pub(crate) fn wire_create_for_battle_in(w: &mut crate::world::owner::World, bounds: PositionBounds, capacity: super::bounds::Capacity) -> usize {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
    if !owner.bind_bounds(bounds) { return 0; }
    owner.runtimes.insert(Runtime::for_battle(bounds, capacity)).unwrap_or(0)
}
/// Bind the canonical producer before its first publication. Ring banks
/// reserve the loaded content's maximum crew size at this cold boundary.
/// # Safety
/// Both allocations belong to this simulation instance and remain live.
pub(crate) fn wire_bind_store_in(w: &mut crate::world::owner::World, runtime: usize, capacity: Option<usize>) -> u32 {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
    let Some(runtime) = owner.runtimes.get_mut(runtime) else { return 0 };
    let Some(capacity) = capacity else { return 0 };
    if runtime.retained.is_some() { return 1; }
    let Ok(ring) = super::retained::Ring::with_capacity(capacity, 1024, 128, runtime.capacity) else { return 0 };
    runtime.retained = Some(ring); 1
}
/// # Safety
/// The handle names a wire runtime in this module instance.
pub(crate) fn wire_drop_in(w: &mut crate::world::owner::World, runtime: usize) {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.runtimes.get(runtime as usize).is_none() {return;}

    if runtime != 0 {
        owner.runtimes.remove(runtime as usize);
    }

}
/// # Safety
/// The handle belongs to this module instance for the call.
pub(crate) fn wire_capture_in(w: &mut crate::world::owner::World, runtime: usize) -> Option<&[f64]> {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.runtimes.get(runtime as usize).is_none() {return None;}

    let input = {
        let (row, heap) = w.sim.calls.args();
        WireCapture::take(row, heap)
    };
    let result = owner.runtimes.get_mut(runtime as usize).unwrap()
        .capture(input, &w.sim.strings)
        .map(|_| Vec::new());
    output(&mut w.sim, result)

}
/// # Safety
/// The handle belongs to this module instance for the call.
pub(crate) fn wire_held_in(w: &mut crate::world::owner::World, runtime: usize,
    tick: f64,
    epoch: f64,
    baseline: f64,
    interval: f64,) -> Option<&[f64]> {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.runtimes.get(runtime as usize).is_none() {return None;}

    output_retained(&mut w.sim, owner.runtimes.get_mut(runtime as usize).unwrap().encode(tick, epoch, baseline, interval))

}

