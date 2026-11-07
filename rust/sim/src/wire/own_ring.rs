//! Per-client own baselines keep the exact body and personal fields the
//! client received. Retired frames return storage, independently of tick gaps.
use crate::gen::WireField;
use super::{codec::Result, host::Own, ring::SnapshotRing};

struct Row { value: Own, payload: Vec<u8>, fields: Vec<WireField> }
impl Row {
    // prepareOwn reserves two personal fields and one complete correction.
    // Nested buffers belong to the row across optional-section changes.
    // They are never dropped when a dead player temporarily has no body.
    fn new() -> Self {
        Self { value: Own { payload: None, summaries: Vec::with_capacity(2) }, payload: Vec::with_capacity(768),
            fields: (0..2).map(|_| WireField { payload: Vec::with_capacity(96), ..Default::default() }).collect() }
    }
    // prepareOwn replaces personal fields rather than merging omissions.
    // Return the previous optional payload to its original byte bank first.
    // Failure leaves that bank available for a later valid publication.
    fn write(&mut self, payload: Option<&[u8]>, fields: &[WireField]) -> Result<()> {
        if let Some(old) = self.value.payload.take() { self.payload = old; }
        self.fields.extend(self.value.summaries.drain(..));
        if let Some(bytes) = payload {
            if bytes.len() > 1100 { return Err("own-player envelope exceeds budget"); }
            if bytes.len() > self.payload.capacity() { self.payload.try_reserve(bytes.len().saturating_sub(self.payload.len()))
                .map_err(|_| "own-player envelope exceeds budget")?; }
            self.payload.clear(); self.payload.extend_from_slice(bytes);
            self.value.payload = Some(std::mem::take(&mut self.payload));
        }
        for source in fields {
            let field = self.fields.pop().ok_or("own summary capacity exceeded")?;
            self.value.summaries.push(field); let field = self.value.summaries.last_mut().unwrap();
            if source.payload.len() > 900 { return Err("summary exceeds bounds"); }
            if source.payload.len() > field.payload.capacity() { field.payload.try_reserve(source.payload.len().saturating_sub(field.payload.len()))
                .map_err(|_| "summary exceeds bounds")?; }
            field.id = source.id; field.schema = source.schema; field.generation = source.generation;
            field.payload.clear(); field.payload.extend_from_slice(&source.payload);
        }
        Ok(())
    }
}
/// The baseline's row remains readable through this encoding operation even
/// if insertion evicted its tick. It can be reused by the next write only.
pub struct Prepared { current: usize, baseline: Option<usize> }
pub struct Ring { ring: SnapshotRing<usize>, rows: Vec<Row>, free: Vec<usize> }
impl Ring {
    // SnapshotRing holds one extra staging row beyond its visible depth.
    // The selected baseline may be the frame evicted by this insertion.
    // Keep its bytes alive until both own packet sections finish encoding.
    pub fn new(depth: usize) -> Result<Self> {
        if depth == 0 || depth > 128 { return Err("invalid own ring capacity"); }
        Ok(Self { ring: SnapshotRing::new(depth)?, rows: (0..=depth).map(|_| Row::new()).collect(), free: (0..=depth).rev().collect() })
    }
    /// prepareOwn can reserve a known exceptional body at a cold boundary.
    /// Ordinary battle bodies fit 768 bytes and personal fields fit 96.
    pub fn reserve(&mut self, bytes: usize) -> Result<()> {
        if bytes > 1100 { return Err("own-player envelope exceeds budget"); }
        for row in &mut self.rows {
            let bank = row.value.payload.as_mut().unwrap_or(&mut row.payload);
            bank.try_reserve(bytes.saturating_sub(bank.len())).map_err(|_| "own-player envelope exceeds budget")?;
        }
        Ok(())
    }
    pub fn latest(&self) -> Option<&Own> { self.ring.get(self.ring.newest_tick(), None).map(|&at| &self.rows[at].value) }
    pub fn has(&self, tick: f64, epoch: f64) -> bool { self.ring.has(tick as i64, Some(epoch as u32)) }
    pub fn get(&self, tick: f64, epoch: f64) -> Option<&Own> {
        self.ring.get(tick as i64, Some(epoch as u32)).map(|&at| &self.rows[at].value)
    }
    /// prepareOwn retains a missing correction's baseline body, and always
    /// replaces the personal field set. It does not retain omitted statistics.
    pub fn write(&mut self, tick: f64, epoch: f64, baseline: f64, correction: Option<&[u8]>, summaries: &[WireField]) -> Result<Prepared> {
        if !tick.is_finite() || tick.fract() != 0. || tick.abs() > 9007199254740991. { return Err("invalid snapshot ring tick"); }
        if self.ring.len() == self.ring.capacity() && tick < self.ring.oldest_tick() as f64 { return Err("snapshot not held"); }
        let base = self.ring.get(baseline as i64, Some(epoch as u32)).copied();
        let at = self.free.pop().ok_or("own ring capacity exceeded")?;
        let (left, right) = self.rows.split_at_mut(at); let (row, right) = right.split_first_mut().unwrap();
        let old = base.map(|p| if p < at { &left[p].value } else { &right[p - at - 1].value });
        let payload = correction.or_else(|| old.and_then(|b| b.payload.as_deref()));
        if let Err(error) = row.write(payload, summaries) { self.free.push(at); return Err(error); }
        // SnapshotRing permits replacement of an already published tick.
        // Replacement and age eviction return different rows to the free bank.
        // Neither can return the staging row currently being encoded.
        let replaced = self.ring.get(tick as i64, None).copied(); let free = &mut self.free;
        self.ring.set_with_eviction(tick as i64, epoch as u32, at, |old, _| free.push(old))?;
        if let Some(old) = replaced { self.free.push(old); }
        Ok(Prepared { current: at, baseline: base })
    }
    pub fn prepared(&self, prepared: &Prepared) -> (&Own, Option<&Own>) {
        (&self.rows[prepared.current].value, prepared.baseline.map(|at| &self.rows[at].value))
    }
}
