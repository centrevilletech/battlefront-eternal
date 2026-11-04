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
#[derive(Default)]
pub struct Runtime {
    pub ring: super::ring::SnapshotRing<WireSnapshot>,
    pub slots: super::records::SnapshotSlotTable,
    pub previous: Option<WireSnapshot>,
}
impl Runtime {
    pub fn capture(
        &mut self,
        input: WireCapture,
        strings: &crate::store::Strings,
    ) -> codec::Result<()> {
        if !input.tick.is_finite()
            || input.tick.fract() != 0.
            || input.tick.abs() > 9007199254740991.
        {
            return Err("invalid snapshot ring tick");
        }
        let snapshot = super::records::snapshot_from_records(
            input.tick,
            input.epoch,
            &input.records,
            input.world,
            &mut self.slots,
            &input.bounds,
            self.previous.as_ref(),
            strings,
        )?;
        self.ring
            .set(input.tick as i64, input.epoch as u32, snapshot.clone())?;
        self.previous = Some(snapshot);
        Ok(())
    }
    pub fn encode(
        &self,
        tick: f64,
        epoch: f64,
        baseline: f64,
        interval: f64,
    ) -> codec::Result<Vec<Vec<u8>>> {
        let current = self
            .ring
            .get(tick as i64, Some(epoch as u32))
            .ok_or("snapshot not held")?;
        let base = if baseline == 0. {
            None
        } else {
            Some(
                self.ring
                    .get(baseline as i64, Some(epoch as u32))
                    .ok_or("chosen snapshot baseline not held")?,
            )
        };
        codec::encode_entities(current, base, interval)
    }
}
#[no_mangle]
pub extern "C" fn wire_create() -> *mut Runtime {
    Box::into_raw(Box::new(Runtime::default()))
}
/// # Safety
/// The pointer must be a live wire_create allocation, dropped exactly once.
#[no_mangle]
pub unsafe extern "C" fn wire_drop(runtime: *mut Runtime) {
    if !runtime.is_null() {
        drop(Box::from_raw(runtime));
    }
}
/// # Safety
/// The pointer must belong to this module and remain live through the call.
#[no_mangle]
pub unsafe extern "C" fn wire_capture(runtime: *mut Runtime) -> *const f64 {
    let input = {
        let (row, heap) = sim().calls.args();
        WireCapture::take(row, heap)
    };
    let result = (*runtime)
        .capture(input, &sim().strings)
        .map(|_| Vec::new());
    output(result)
}
/// # Safety
/// The pointer must belong to this module and remain live through the call.
#[no_mangle]
pub unsafe extern "C" fn wire_held(
    runtime: *const Runtime,
    tick: f64,
    epoch: f64,
    baseline: f64,
    interval: f64,
) -> *const f64 {
    output((*runtime).encode(tick, epoch, baseline, interval))
}

/// Transfer already captured semantic records and staggered world fields
/// without exposing them to JavaScript between the two native owners.
/// No callback runs while any of these distinct session boxes is borrowed.
#[no_mangle]
pub unsafe extern "C" fn wire_capture_produced(
    runtime: *mut Runtime,
    source: *const super::capture::Capture,
    world: *const super::stagger::Stagger,
) -> *const f64 {
    let mut input = {
        let (row, heap) = sim().calls.args();
        WireCapture::take(row, heap)
    };
    input.records = (&*source).held_records();
    input.world = (&*world).world();
    let result = (&mut *runtime)
        .capture(input, &sim().strings)
        .map(|_| Vec::new());
    output(result)
}
