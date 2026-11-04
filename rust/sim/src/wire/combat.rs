//! Combat publication bytes and the ordered, expiring producer queue.
//! The adapter retains decoded views; native state owns queue decisions.
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
            w.f64(*n, "invalid feedback source")?;
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
}
fn same_key(a: f64, b: f64) -> bool {
    a == b || a.is_nan() && b.is_nan()
}
pub struct Capture {
    pending: Vec<Pending>,
    pruned_tick: Option<f64>,
    next_id: f64,
    faults: Vec<StrId>,
}
impl Default for Capture {
    fn default() -> Self {
        Self {
            pending: vec![],
            pruned_tick: None,
            next_id: 1.,
            faults: vec![],
        }
    }
}
impl Capture {
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
                    && self.pending.len() as f64 >= c.capacity
                {
                    out.error = Some(strings.intern_str("combat capture capacity exceeded"));
                }
            }
            1 => {
                let entry = Pending {
                    key: if c.key == 0. { 0. } else { c.key },
                    schema: c.schema,
                    expiry_tick: c.expiry_tick,
                };
                if let Some(old) = self.pending.iter_mut().find(|p| same_key(p.key, c.key)) {
                    *old = entry;
                } else {
                    self.pending.push(entry);
                }
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
#[no_mangle]
pub extern "C" fn wire_combat_encode() -> *const f64 {
    let c = {
        let (row, heap) = crate::abi::sim().calls.args();
        WbEncode::take(row, heap)
    };
    super::module::output(encode(&c).map(|bytes| vec![bytes]))
}
#[no_mangle]
pub extern "C" fn wire_combat_create() -> *mut Capture {
    Box::into_raw(Box::new(Capture::default()))
}
/// # Safety
/// The handle belongs to this module instance and is released once.
#[no_mangle]
pub unsafe extern "C" fn wire_combat_drop(handle: *mut Capture) {
    if !handle.is_null() {
        drop(Box::from_raw(handle));
    }
}
/// # Safety
/// The live handle is exclusive for this operation. Inputs are owned and
/// no simulation callback runs until the operation has returned.
#[no_mangle]
pub unsafe extern "C" fn wire_combat_run(handle: *mut Capture) -> *const f64 {
    let c = {
        let (row, heap) = crate::abi::sim().calls.args();
        WbCall::take(row, heap)
    };
    let s = crate::abi::sim();
    let out = (*handle).run(c, &mut s.strings);
    s.calls
        .result(WbResult::WORDS, |row, heap| out.put(row, heap));
    s.calls.block.ptr()
}

#[cfg(test)]
mod tests {
    use super::*;
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
