//! Cosmetic wire bytes and per-client delivery state, matching
//! snapshot-cosmetics.js and snapshot-cosmetic-outbox.js. Event identities
//! are opaque host tokens; key replacement never changes insertion order.
use super::codec::{Result, Writer};
use super::records::{quantize_pitch, quantize_position, quantize_yaw};
use crate::gen::WireBounds;
use jsrt::{js_max, js_min, js_round, JsMap};
use std::collections::VecDeque;
use std::rc::Rc;

pub const RESEND_TICKS: f64 = 30.0;
pub const MAX_OWED_EVENTS: usize = 1024;
const COSMETIC_TICKS: f64 = 18.0;
const LAUNCH_TICKS: f64 = 24.0;
const RANK_OWN: u8 = 0;
const RANK_LAUNCH: u8 = 1;
const RANK_OWED: u8 = 2;
const RANK_PRESENTATION: u8 = 3;
const RANK_SAMPLE: u8 = 4;

/// Missing and null ids differ under the source's strict equality test.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Owner {
    pub tag: u8,
    pub value: f64,
}
impl Owner {
    pub fn number(value: f64) -> Self {
        Self { tag: 2, value }
    }
}
fn same_owner(a: Owner, b: Owner) -> bool {
    a.tag == b.tag && (a.tag != 2 || a.value == b.value)
}

#[derive(Clone, Debug, Default)]
pub struct View {
    pub schema: f64,
    pub position: Option<[f64; 3]>,
    pub direction: Option<[f64; 3]>,
    pub owner: Owner,
    pub speed: f64,
    pub life: f64,
}

pub fn cosmetic_priority(
    view: &View,
    position: Option<[f64; 3]>,
    owner: Owner,
    range: f64,
) -> Option<f64> {
    let (Some(mut at), Some(position)) = (view.position, position) else {
        return Some(0.0);
    };
    if view.schema == 1.0 && view.speed > 0.0 {
        if let Some(direction) = view.direction {
            let t = js_max(
                0.0,
                js_min(
                    view.speed * view.life,
                    (position[0] - at[0]) * direction[0]
                        + (position[1] - at[1]) * direction[1]
                        + (position[2] - at[2]) * direction[2],
                ),
            );
            for i in 0..3 {
                at[i] += direction[i] * t;
            }
        }
    }
    let dx = at[0] - position[0];
    let dy = at[1] - position[1];
    let dz = at[2] - position[2];
    let d2 = dx * dx + dy * dy + dz * dz;
    if same_owner(view.owner, owner) {
        Some(-1.0)
    } else if d2 <= range * range {
        Some(d2)
    } else {
        None
    }
}

#[derive(Clone, Debug)]
pub struct Record {
    pub id: f64,
    pub generation: f64,
    pub tick: f64,
    pub schema: f64,
    pub combat: bool,
    pub payload: Vec<u8>,
}
#[derive(Clone, Debug, Default)]
pub struct Compact {
    pub view: View,
    pub expiry_tick: f64,
    pub definition: f64,
    pub owner_generation: f64,
    pub fire: f64,
    pub launch_tick: f64,
    pub execution_tick: f64,
    pub channel: f64,
    pub surface: f64,
    pub offset: Vec<f64>,
    pub merge: f64,
}
fn u32(w: &mut Writer, v: f64) -> Result<()> {
    w.u32(v).map_err(|_| "snapshot u32 out of range")
}
fn u16(w: &mut Writer, v: f64) -> Result<()> {
    w.u16(v).map_err(|_| "snapshot u16 out of range")
}
fn i16(w: &mut Writer, v: f64) -> Result<()> {
    w.i16(v).map_err(|_| "snapshot i16 out of range")
}
fn u8(w: &mut Writer, v: f64) -> Result<()> {
    w.u8(v).map_err(|_| "snapshot u8 out of range")
}

/// Only the launch/impact source is decoded by the compact encoder. Other
/// payloads pass through unchanged, including their original validation.
pub fn decode_compact(record: &Record) -> Result<Compact> {
    let mut out = Compact::default(); decode_compact_into(record, &mut out)?; Ok(out)
}
/// The publication owner reserves the three launch-offset values once.
/// Decoding malformed content never replaces that retained vector.
pub fn decode_compact_into(record: &Record, e: &mut Compact) -> Result<()> {
    if !record.combat
        || ![1.0, 2.0].contains(&record.schema)
        || record.payload.len() != if record.schema == 1.0 { 130 } else { 82 }
    {
        return Err("invalid combat payload");
    }
    struct Reader<'a> {
        bytes: &'a [u8],
        at: usize,
    }
    impl Reader<'_> {
        fn u32(&mut self) -> f64 {
            let n = u32::from_le_bytes(self.bytes[self.at..self.at + 4].try_into().unwrap());
            self.at += 4;
            n as f64
        }
        fn f64(&mut self) -> Result<f64> {
            let n = f64::from_le_bytes(self.bytes[self.at..self.at + 8].try_into().unwrap());
            self.at += 8;
            if n.is_finite() {
                Ok(n)
            } else {
                Err("nonfinite combat payload")
            }
        }
        fn vec(&mut self) -> Result<[f64; 3]> {
            Ok([self.f64()?, self.f64()?, self.f64()?])
        }
    }
    let mut r = Reader {
        bytes: &record.payload,
        at: 0,
    };
    e.offset.clear(); e.view = View::default(); e.view.schema = record.schema;
    e.expiry_tick = r.u32(); e.definition = r.u32();
    e.view.schema = record.schema;
    e.view.owner = Owner::number(r.u32());
    e.owner_generation = r.u32();
    e.fire = r.u32();
    e.launch_tick = r.f64()?;
    e.view.position = Some(r.vec()?);
    e.view.direction = Some(r.vec()?);
    e.surface = r.bytes[r.at] as f64;
    r.at += 1;
    if record.schema == 1.0 {
        e.offset.extend_from_slice(&r.vec()?);
        e.view.speed = r.f64()?;
        e.view.life = r.f64()?;
        e.merge = r.f64()?;
    }
    e.execution_tick = r.u32();
    e.channel = r.bytes[r.at] as f64;
    if e.channel > 1.0
        || e.launch_tick < 0.0
        || e.execution_tick < e.launch_tick
        || e.execution_tick > record.tick
    {
        return Err("invalid combat execution tick");
    }
    if e.launch_tick > record.tick
        || e.expiry_tick < record.tick
        || e.expiry_tick - record.tick > 7320.0
        || e.surface > 5.0
        || record.schema == 1.0
            && (e.view.life < 0.0 || e.view.life > 120.0 || e.view.speed < 0.0 || e.merge <= 0.0)
    {
        return Err("invalid combat lifetime");
    }
    Ok(())
}
fn compact(e: &Compact, b: &WireBounds) -> bool {
    let Some(p) = e.view.position else {
        return false;
    };
    let within = |n: f64, min: f64, extent: f64| n >= min && n <= min + extent;
    within(p[0], b.x_min, b.x_extent)
        && within(p[1], b.y_min, b.y_extent)
        && within(p[2], b.z_min, b.z_extent)
        && (e.view.schema != 1.0 || e.offset.iter().all(|v| v.abs() * 256.0 <= 32767.0))
}

pub fn encode_cosmetic_event(
    record: &Record,
    bounds: &WireBounds,
    view: Option<&Compact>,
) -> Result<Vec<u8>> {
    let mut w = Writer::default(); encode_cosmetic_event_into(record, bounds, view, &mut w)?; Ok(w.0)
}
/// Encoded bytes stay in the publisher's event bank through all client offers.
pub fn encode_cosmetic_event_into(record: &Record, bounds: &WireBounds, view: Option<&Compact>, target: &mut Writer) -> Result<()> {
    target.0.clear(); let mut w = target;
    u32(&mut w, record.id)?;
    u32(&mut w, record.generation)?;
    u32(&mut w, record.tick)?;
    u16(&mut w, record.schema)?;
    let decoded;
    let e = if record.schema == 1.0 || record.schema == 2.0 {
        Some(if let Some(e) = view {
            e
        } else {
            decoded = decode_compact(record)?;
            &decoded
        })
    } else {
        None
    };
    if let Some(e) = e.filter(|e| compact(e, bounds)) {
        u8(&mut w, if record.schema == 1.0 { 1.0 } else { 2.0 })?;
        for n in [
            e.expiry_tick,
            e.definition,
            if e.view.owner.tag == 2 {
                e.view.owner.value
            } else {
                f64::NAN
            },
            e.owner_generation,
            e.fire,
        ] {
            u32(&mut w, n)?;
        }
        u32(&mut w, js_round(e.launch_tick * 64.0))?;
        u32(&mut w, e.execution_tick)?;
        u8(&mut w, e.channel)?;
        u8(&mut w, e.surface)?;
        for n in quantize_position(e.view.position.unwrap(), bounds) {
            u16(&mut w, n)?;
        }
        let dir = e.view.direction.ok_or("invalid cosmetic direction")?;
        let yaw = v8math::atan2(dir[0], dir[2]);
        let pitch = v8math::asin(js_max(-1.0, js_min(1.0, dir[1])));
        u16(&mut w, quantize_yaw(yaw))?;
        i16(&mut w, quantize_pitch(pitch))?;
        if record.schema == 1.0 {
            for n in &e.offset {
                i16(
                    &mut w,
                    js_max(-32767.0, js_min(32767.0, js_round(n * 256.0))),
                )?;
            }
            w.f32(e.view.speed)?;
            w.f32(e.view.life)?;
            w.f32(e.merge)?;
        }
    } else {
        u8(&mut w, 0.0)?;
        u16(&mut w, record.payload.len() as f64)?;
        w.raw(&record.payload);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    Launch,
    Terminal,
    Sample,
    Explosion,
    Presentation,
}
impl Kind {
    fn from(n: usize) -> Result<Self> {
        match n {
            0 => Ok(Self::Launch),
            1 => Ok(Self::Terminal),
            2 => Ok(Self::Sample),
            3 => Ok(Self::Explosion),
            4 => Ok(Self::Presentation),
            _ => Err("invalid cosmetic kind"),
        }
    }
}
fn key(prefix: char, id: f64) -> Vec<u16> {
    format!("{prefix}{}", jsrt::numfmt::number_to_string(id))
        .encode_utf16()
        .collect()
}
#[derive(Clone, Debug)]
pub struct Metadata {
    pub key: Vec<u16>,
    pub kind: Kind,
    pub projectile: Option<f64>,
    pub must: bool,
    pub expires: f64,
}
pub fn cosmetic_metadata(schema: f64, id: f64, tick: f64, execution: f64, life: f64) -> Metadata {
    if schema == 1.0 {
        Metadata {
            key: key('l', id),
            kind: Kind::Launch,
            projectile: Some(id),
            must: true,
            expires: (execution + life * 60.0).ceil() + LAUNCH_TICKS,
        }
    } else if schema == 2.0 || schema == 7.0 {
        Metadata {
            key: key('t', id),
            kind: Kind::Terminal,
            projectile: Some(id),
            must: true,
            expires: tick + COSMETIC_TICKS + LAUNCH_TICKS,
        }
    } else if schema == 6.0 {
        Metadata {
            key: key('s', id),
            kind: Kind::Sample,
            projectile: Some(id),
            must: false,
            expires: tick + COSMETIC_TICKS,
        }
    } else if schema == 5.0 {
        Metadata {
            key: key('e', id),
            kind: Kind::Explosion,
            projectile: None,
            must: true,
            expires: tick + COSMETIC_TICKS,
        }
    } else {
        Metadata {
            key: key('e', id),
            kind: Kind::Presentation,
            projectile: None,
            must: false,
            expires: tick + LAUNCH_TICKS,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Event {
    pub identity: u64,
    pub schema: f64,
    pub meta: Metadata,
    pub view: View,
    pub bytes: Vec<u8>,
}
#[derive(Clone, Debug)]
pub struct Entry {
    pub event: Rc<Event>,
    pub d2: f64,
    pub rank: u8,
    pub sent_tick: f64,
}
#[derive(Clone, Debug)]
pub struct Sent {
    pub tick: f64,
    pub events: Vec<Rc<Event>>,
}
#[derive(Clone, Debug)]
pub struct Projectile {
    pub expires: f64,
    pub confirmed: bool,
}
#[derive(Clone, Debug)]
pub struct Packed {
    pub first: Vec<u8>,
    pub second: Vec<u8>,
}

pub struct Outbox {
    pub range: f64,
    pub entries: JsMap<Vec<u16>, Entry>,
    pub inflight: VecDeque<Sent>,
    pub projectiles: JsMap<f64, Projectile>,
    pub dropped: f64,
    pub owed_dropped: f64,
    pub resent: f64,
    pub pruned_tick: f64,
    garbage: usize,
}
impl Outbox {
    pub fn new(range: f64) -> Self {
        Self {
            range,
            entries: JsMap::new(),
            inflight: VecDeque::new(),
            projectiles: JsMap::new(),
            dropped: 0.0,
            owed_dropped: 0.0,
            resent: 0.0,
            pruned_tick: f64::NEG_INFINITY,
            garbage: 0,
        }
    }
    fn delete(&mut self, key: &Vec<u16>) {
        if self.entries.delete(key) {
            self.garbage += 1;
        }
    }
    fn compact(&mut self) {
        // No iterator crosses an operation boundary. Release tombstones
        // periodically without changing any live key's insertion order.
        if self.garbage >= 64 {
            self.entries = self.entries.clone();
            self.projectiles = self.projectiles.clone();
            self.garbage = 0;
        }
    }
    pub fn offer(&mut self, events: Vec<Rc<Event>>, position: Option<[f64; 3]>, owner: Owner) {
        for event in events {
            let known = event
                .meta
                .projectile
                .and_then(|id| self.projectiles.get(&id));
            let confirmed = known.is_some_and(|k| k.confirmed);
            let mut d2 = cosmetic_priority(&event.view, position, owner, self.range);
            if event.meta.kind == Kind::Terminal {
                if known.is_some() {
                    d2 = d2.or(Some(self.range * self.range));
                } else if event.schema == 7.0 {
                    continue;
                }
            }
            let Some(d2) = d2 else { continue };
            if event.meta.kind == Kind::Terminal {
                self.delete(&key('s', event.meta.projectile.unwrap_or(f64::NAN)));
            }
            let rank = if d2 < 0.0 {
                RANK_OWN
            } else if event.meta.kind == Kind::Launch
                || event.meta.kind == Kind::Sample && !confirmed
            {
                RANK_LAUNCH
            } else if event.meta.must {
                RANK_OWED
            } else if event.meta.kind == Kind::Presentation {
                RANK_PRESENTATION
            } else {
                RANK_SAMPLE
            };
            self.entries.set(
                event.meta.key.clone(),
                Entry {
                    event,
                    d2,
                    rank,
                    sent_tick: -1.0,
                },
            );
        }
        if self.entries.size() > MAX_OWED_EVENTS {
            for key in self.entries.keys() {
                self.delete(&key);
                self.dropped += 1.0;
                if self.entries.size() <= MAX_OWED_EVENTS {
                    break;
                }
            }
        }
        self.compact();
    }
    pub fn pack(
        &mut self,
        tick: f64,
        first_budget: f64,
        second_budget: f64,
        confirmable: bool,
        spare: f64,
    ) -> Result<Packed> {
        for key in self.entries.keys() {
            let entry = self.entries.get(&key).unwrap();
            if entry.event.meta.expires < tick {
                if entry.sent_tick < 0.0 {
                    self.dropped += 1.0;
                    if entry.rank <= RANK_OWED {
                        self.owed_dropped += 1.0;
                    }
                }
                self.delete(&key);
            }
        }
        if tick >= self.pruned_tick + RESEND_TICKS || tick < self.pruned_tick {
            self.pruned_tick = tick;
            for id in self.projectiles.keys() {
                if self.projectiles.get(&id).unwrap().expires < tick {
                    self.projectiles.delete(&id);
                    self.garbage += 1;
                }
            }
        }
        while self
            .inflight
            .front()
            .is_some_and(|s| s.tick <= tick - RESEND_TICKS)
        {
            let sent = self.inflight.pop_front().unwrap();
            self.settle(sent, false);
        }
        let mut candidates = Vec::new();
        let mut bytes = 0usize;
        for entry in self.entries.values() {
            if entry.sent_tick >= 0.0 || !confirmable && entry.rank <= RANK_OWED {
                continue;
            }
            bytes += 2 + entry.event.bytes.len();
            candidates.push(entry.clone());
        }
        if bytes as f64 > first_budget {
            jsrt::sort::js_sort(&mut candidates, &mut |a, b| {
                let rank = a.rank as f64 - b.rank as f64;
                if rank != 0.0 {
                    rank
                } else {
                    a.d2 - b.d2
                }
            });
        }
        let mut first = Writer::default();
        let mut second = Writer::default();
        let mut owed = Vec::new();
        let mut refinements = 0usize;
        for entry in candidates {
            let event = entry.event;
            let size = 2 + event.bytes.len();
            let w = if (first.0.len() + size) as f64 <= first_budget {
                &mut first
            } else if (second.0.len() + size) as f64 <= second_budget {
                &mut second
            } else {
                continue;
            };
            if entry.rank > RANK_OWED && (refinements + size) as f64 > spare {
                continue;
            }
            if entry.rank > RANK_OWED {
                refinements += size;
            }
            u16(w, event.bytes.len() as f64)?;
            w.raw(&event.bytes);
            if matches!(event.meta.kind, Kind::Launch | Kind::Sample) {
                let until = if event.meta.kind == Kind::Launch {
                    event.meta.expires
                } else {
                    tick + (event.view.life * 60.0).ceil() + COSMETIC_TICKS + LAUNCH_TICKS
                };
                let id = event.meta.projectile.unwrap_or(f64::NAN);
                if let Some(known) = self.projectiles.get_mut(&id) {
                    known.expires = js_max(known.expires, until);
                } else {
                    self.projectiles.set(
                        id,
                        Projectile {
                            expires: until,
                            confirmed: false,
                        },
                    );
                }
            }
            if confirmable && entry.rank <= RANK_OWED {
                self.entries.get_mut(&event.meta.key).unwrap().sent_tick = tick;
                owed.push(event);
            } else {
                self.delete(&event.meta.key);
            }
        }
        if !owed.is_empty() {
            self.inflight.push_back(Sent { tick, events: owed });
        }
        self.compact();
        Ok(Packed {
            first: first.0,
            second: second.0,
        })
    }
    pub fn acknowledge(&mut self, tick: f64) {
        while self.inflight.front().is_some_and(|s| s.tick <= tick) {
            let sent = self.inflight.pop_front().unwrap();
            let delivered = sent.tick == tick;
            self.settle(sent, delivered);
        }
        self.compact();
    }
    pub fn resend(&mut self) {
        while let Some(sent) = self.inflight.pop_front() {
            self.settle(sent, false);
        }
        self.compact();
    }
    pub fn settle(&mut self, sent: Sent, delivered: bool) {
        for event in sent.events {
            if delivered {
                if let Some(id) = event.meta.projectile {
                    if let Some(known) = self.projectiles.get_mut(&id) {
                        known.confirmed = true;
                    }
                }
            }
            let Some(entry) = self.entries.get_mut(&event.meta.key) else {
                continue;
            };
            if entry.event.identity != event.identity || entry.sent_tick != sent.tick {
                continue;
            }
            if delivered {
                self.delete(&event.meta.key);
            } else {
                entry.sent_tick = -1.0;
                self.resent += 1.0;
            }
        }
    }
    pub fn inspect(&self) -> Vec<f64> {
        let mut out = vec![
            self.dropped,
            self.owed_dropped,
            self.resent,
            self.pruned_tick,
            self.entries.size() as f64,
        ];
        for (key, entry) in self.entries.entries_ref() {
            put_key(&mut out, &key);
            out.extend([
                entry.event.identity as f64,
                entry.d2,
                entry.rank as f64,
                entry.sent_tick,
            ]);
        }
        out.push(self.inflight.len() as f64);
        for sent in &self.inflight {
            out.extend([sent.tick, sent.events.len() as f64]);
            for event in &sent.events {
                out.push(event.identity as f64);
            }
        }
        out.push(self.projectiles.size() as f64);
        for (id, p) in self.projectiles.entries_ref() {
            out.extend([id, p.expires, p.confirmed as u8 as f64]);
        }
        out
    }
}

// The synchronous exchange has separate f64 words and payload bytes.
// Every operation is available to the native twin through Session::run.
// 0 encode: record (id,generation,tick,schema,combat), bounds (min,extent
// per x,y,z), optional compact view. 1 priority: view, position, owner,
// range. 2 metadata: schema,id,tick,execution,life. 3 initialize: range.
// 4 offer: position,owner,count,events. 5 pack: tick,budgets,confirmable,
// spare. 6 acknowledge: tick. 7 resend. 8 inspect. 9 pack encoded events.
// Views are schema, optional position/direction (present,x,y,z), owner
// (tag,value: missing=0,null=1,number=2), speed,life. Event: identity,key
// (UTF-16 length,units),schema,kind,optional projectile,must,expires,
// payload range (offset,length),view. No native reference crosses JS.
#[derive(Default)]
pub struct Session {
    pub input: Vec<f64>,
    pub payload: Vec<u8>,
    pub words: Vec<f64>,
    pub bytes: Vec<u8>,
    pub outbox: Option<Outbox>,
    error: Option<&'static str>,
}
struct Input<'a> {
    words: &'a [f64],
    at: usize,
}
impl Input<'_> {
    fn number(&mut self) -> Result<f64> {
        let n = *self.words.get(self.at).ok_or("invalid cosmetic exchange")?;
        self.at += 1;
        Ok(n)
    }
    fn size(&mut self) -> Result<usize> {
        let n = self.number()?;
        if !n.is_finite() || n < 0.0 || n.fract() != 0.0 || n > u32::MAX as f64 {
            Err("invalid cosmetic exchange")
        } else {
            Ok(n as usize)
        }
    }
    fn flag(&mut self) -> Result<bool> {
        Ok(self.number()? != 0.0)
    }
    fn vec(&mut self) -> Result<[f64; 3]> {
        Ok([self.number()?, self.number()?, self.number()?])
    }
    fn optional_vec(&mut self) -> Result<Option<[f64; 3]>> {
        let present = self.flag()?;
        let v = self.vec()?;
        Ok(present.then_some(v))
    }
    fn owner(&mut self) -> Result<Owner> {
        Ok(Owner {
            tag: self.size()? as u8,
            value: self.number()?,
        })
    }
    fn key(&mut self) -> Result<Vec<u16>> {
        let n = self.size()?;
        if n > self.words.len().saturating_sub(self.at) {
            return Err("invalid cosmetic exchange");
        }
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(self.number()? as u16);
        }
        Ok(out)
    }
    fn view(&mut self) -> Result<View> {
        Ok(View {
            schema: self.number()?,
            position: self.optional_vec()?,
            direction: self.optional_vec()?,
            owner: self.owner()?,
            speed: self.number()?,
            life: self.number()?,
        })
    }
    fn compact(&mut self) -> Result<Compact> {
        let view = self.view()?;
        let mut e = Compact {
            view,
            expiry_tick: self.number()?,
            definition: self.number()?,
            owner_generation: self.number()?,
            fire: self.number()?,
            launch_tick: self.number()?,
            execution_tick: self.number()?,
            channel: self.number()?,
            surface: self.number()?,
            merge: self.number()?,
            offset: Vec::new(),
        };
        let n = self.size()?;
        if n > self.words.len().saturating_sub(self.at) {
            return Err("invalid cosmetic exchange");
        }
        for _ in 0..n {
            e.offset.push(self.number()?);
        }
        Ok(e)
    }
    fn bytes<'a>(&mut self, payload: &'a [u8]) -> Result<&'a [u8]> {
        let at = self.size()?;
        let n = self.size()?;
        payload
            .get(at..at.checked_add(n).ok_or("invalid cosmetic exchange")?)
            .ok_or("invalid cosmetic exchange")
    }
    fn event(&mut self, payload: &[u8]) -> Result<Rc<Event>> {
        let identity = self.number()? as u64;
        let key = self.key()?;
        let schema = self.number()?;
        let kind = Kind::from(self.size()?)?;
        let present = self.flag()?;
        let projectile = self.number()?;
        let must = self.flag()?;
        let expires = self.number()?;
        let bytes = self.bytes(payload)?.to_vec();
        let view = self.view()?;
        Ok(Rc::new(Event {
            identity,
            schema,
            meta: Metadata {
                key,
                kind,
                projectile: present.then_some(projectile),
                must,
                expires,
            },
            view,
            bytes,
        }))
    }
    fn done(&self) -> Result<()> {
        if self.at == self.words.len() {
            Ok(())
        } else {
            Err("invalid cosmetic exchange")
        }
    }
}
fn put_key(out: &mut Vec<f64>, key: &[u16]) {
    out.push(key.len() as f64);
    out.extend(key.iter().map(|n| *n as f64));
}
impl Session {
    pub fn run(&mut self, op: u32) -> Result<()> {
        self.words.clear();
        self.bytes.clear();
        let mut r = Input {
            words: &self.input,
            at: 0,
        };
        match op {
            0 => {
                let record = Record {
                    id: r.number()?,
                    generation: r.number()?,
                    tick: r.number()?,
                    schema: r.number()?,
                    combat: r.flag()?,
                    payload: self.payload.clone(),
                };
                let bounds = WireBounds {
                    x_min: r.number()?,
                    x_extent: r.number()?,
                    y_min: r.number()?,
                    y_extent: r.number()?,
                    z_min: r.number()?,
                    z_extent: r.number()?,
                };
                let view = if r.flag()? { Some(r.compact()?) } else { None };
                r.done()?;
                self.bytes = encode_cosmetic_event(&record, &bounds, view.as_ref())?;
            }
            1 => {
                let view = r.view()?;
                let p = r.optional_vec()?;
                let owner = r.owner()?;
                let range = r.number()?;
                r.done()?;
                let priority = cosmetic_priority(&view, p, owner, range);
                self.words
                    .extend([priority.is_some() as u8 as f64, priority.unwrap_or(0.0)]);
            }
            2 => {
                let meta = cosmetic_metadata(
                    r.number()?,
                    r.number()?,
                    r.number()?,
                    r.number()?,
                    r.number()?,
                );
                r.done()?;
                self.words.extend([
                    meta.kind as u8 as f64,
                    meta.projectile.is_some() as u8 as f64,
                    meta.projectile.unwrap_or(0.0),
                    meta.must as u8 as f64,
                    meta.expires,
                ]);
                put_key(&mut self.words, &meta.key);
            }
            3 => {
                let range = r.number()?;
                r.done()?;
                self.outbox = Some(Outbox::new(range));
            }
            4 => {
                let p = r.optional_vec()?;
                let owner = r.owner()?;
                let count = r.size()?;
                let mut events = Vec::new();
                for _ in 0..count {
                    events.push(r.event(&self.payload)?);
                }
                r.done()?;
                self.outbox
                    .as_mut()
                    .ok_or("missing cosmetic outbox")?
                    .offer(events, p, owner);
            }
            5 => {
                let tick = r.number()?;
                let first = r.number()?;
                let second = r.number()?;
                let confirmable = r.flag()?;
                let spare = r.number()?;
                r.done()?;
                let packed = self
                    .outbox
                    .as_mut()
                    .ok_or("missing cosmetic outbox")?
                    .pack(tick, first, second, confirmable, spare)?;
                self.words
                    .extend([packed.first.len() as f64, packed.second.len() as f64]);
                self.bytes = packed.first;
                self.bytes.extend(packed.second);
            }
            6 => {
                let tick = r.number()?;
                r.done()?;
                self.outbox
                    .as_mut()
                    .ok_or("missing cosmetic outbox")?
                    .acknowledge(tick);
            }
            7 => {
                r.done()?;
                self.outbox
                    .as_mut()
                    .ok_or("missing cosmetic outbox")?
                    .resend();
            }
            8 => {
                r.done()?;
                self.words = self
                    .outbox
                    .as_ref()
                    .ok_or("missing cosmetic outbox")?
                    .inspect();
            }
            9 => {
                let budget = r.number()?;
                let count = r.size()?;
                let mut w = Writer::default();
                let mut packed = Vec::new();
                let mut dropped = Vec::new();
                for i in 0..count {
                    let bytes = r.bytes(&self.payload)?;
                    if (w.0.len() + 2 + bytes.len()) as f64 <= budget {
                        u16(&mut w, bytes.len() as f64)?;
                        w.raw(bytes);
                        packed.push(i as f64);
                    } else {
                        dropped.push(i as f64);
                    }
                }
                r.done()?;
                self.words.push(packed.len() as f64);
                self.words.extend(packed);
                self.words.push(dropped.len() as f64);
                self.words.extend(dropped);
                self.bytes = w.0;
            }
            _ => return Err("invalid cosmetic operation"),
        }
        Ok(())
    }
}

#[no_mangle]
pub extern "C" fn wire_cosmetics_create() -> *mut Session {
    super::owner::with(|owner| {

    owner.cosmetics.insert(Session::default()).unwrap_or(0) as *mut Session

    })
}
#[no_mangle]
pub unsafe extern "C" fn wire_cosmetics_drop(ptr: *mut Session) {
    super::owner::with(|owner| {
if owner.cosmetics.get(ptr as usize).is_none() {return;}

    if !ptr.is_null() {
        owner.cosmetics.remove(ptr as usize);
    }

    })
}
#[no_mangle]
pub unsafe extern "C" fn wire_cosmetics_room(ptr: *mut Session, n: u32) -> *mut f64 {
    super::owner::with(|owner| {
if owner.cosmetics.get(ptr as usize).is_none() {return std::ptr::null_mut();}

    let s = owner.cosmetics.get_mut(ptr as usize).unwrap();
    s.input.resize(n as usize, 0.0);
    s.input.as_mut_ptr()

    })
}
#[no_mangle]
pub unsafe extern "C" fn wire_cosmetics_payload(ptr: *mut Session, n: u32) -> *mut u8 {
    super::owner::with(|owner| {
if owner.cosmetics.get(ptr as usize).is_none() {return std::ptr::null_mut();}

    let s = owner.cosmetics.get_mut(ptr as usize).unwrap();
    s.payload.resize(n as usize, 0);
    s.payload.as_mut_ptr()

    })
}
#[no_mangle]
pub unsafe extern "C" fn wire_cosmetics_run(ptr: *mut Session, op: u32) -> u32 {
    super::owner::with(|owner| {
if owner.cosmetics.get(ptr as usize).is_none() {return 0;}

    let s = owner.cosmetics.get_mut(ptr as usize).unwrap();
    s.error = s.run(op).err();
    s.error.is_some() as u32

    })
}
#[no_mangle]
pub unsafe extern "C" fn wire_cosmetics_words(ptr: *mut Session) -> *const f64 {
    super::owner::with(|owner| {
if owner.cosmetics.get(ptr as usize).is_none() {return std::ptr::null();}

    let s = owner.cosmetics.get(ptr as usize).unwrap();
    s.words.as_ptr()

    })
}
#[no_mangle]
pub unsafe extern "C" fn wire_cosmetics_word_count(ptr: *mut Session) -> u32 {
    super::owner::with(|owner| {
if owner.cosmetics.get(ptr as usize).is_none() {return 0;}

    let s = owner.cosmetics.get(ptr as usize).unwrap();
    s.words.len() as u32

    })
}
#[no_mangle]
pub unsafe extern "C" fn wire_cosmetics_bytes(ptr: *mut Session) -> *const u8 {
    super::owner::with(|owner| {
if owner.cosmetics.get(ptr as usize).is_none() {return std::ptr::null();}

    let s = owner.cosmetics.get(ptr as usize).unwrap();
    s.bytes.as_ptr()

    })
}
#[no_mangle]
pub unsafe extern "C" fn wire_cosmetics_byte_count(ptr: *mut Session) -> u32 {
    super::owner::with(|owner| {
if owner.cosmetics.get(ptr as usize).is_none() {return 0;}

    let s = owner.cosmetics.get(ptr as usize).unwrap();
    s.bytes.len() as u32

    })
}
#[no_mangle]
pub unsafe extern "C" fn wire_cosmetics_error(ptr: *mut Session) -> *const u8 {
    super::owner::with(|owner| {
if owner.cosmetics.get(ptr as usize).is_none() {return std::ptr::null();}

    let s = owner.cosmetics.get(ptr as usize).unwrap();
    s.error.unwrap_or("").as_ptr()

    })
}
#[no_mangle]
pub unsafe extern "C" fn wire_cosmetics_error_len(ptr: *mut Session) -> u32 {
    super::owner::with(|owner| {
if owner.cosmetics.get(ptr as usize).is_none() {return 0;}

    let s = owner.cosmetics.get(ptr as usize).unwrap();
    s.error.unwrap_or("").len() as u32

    })
}
#[no_mangle]
pub unsafe extern "C" fn wire_cosmetics_metric(ptr: *mut Session, index: u32) -> f64 {
    super::owner::with(|owner| {
if owner.cosmetics.get(ptr as usize).is_none() {return f64::NAN;}

    let s = owner.cosmetics.get(ptr as usize).unwrap();
    let Some(b) = s.outbox.as_ref() else {
        return 0.0;
    };
    match index {
        0 => b.dropped,
        1 => b.owed_dropped,
        2 => b.resent,
        3 => b.pruned_tick,
        4 => b.entries.size() as f64,
        _ => 0.0,
    }

    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(id: u64, schema: f64, tick: f64) -> Rc<Event> {
        Rc::new(Event {
            identity: id,
            schema,
            meta: cosmetic_metadata(schema, id as f64, tick, tick, 1.0),
            view: View {
                schema,
                position: Some([10.0, 0.0, 0.0]),
                owner: Owner::number(9.0),
                speed: 100.0,
                life: 1.0,
                direction: Some([1.0, 0.0, 0.0]),
            },
            bytes: vec![id as u8; 4],
        })
    }
    #[test]
    fn own_events_and_incoming_paths_keep_priority() {
        let mut v = event(1, 1.0, 0.0).view.clone();
        v.position = Some([450.0, 0.0, 0.0]);
        v.direction = Some([-1.0, 0.0, 0.0]);
        v.life = 4.0;
        assert_eq!(
            cosmetic_priority(&v, Some([0.0; 3]), Owner::number(7.0), 300.0),
            Some(2500.0)
        );
        v.direction = Some([1.0, 0.0, 0.0]);
        assert_eq!(
            cosmetic_priority(&v, Some([0.0; 3]), Owner::number(7.0), 300.0),
            None
        );
        assert_eq!(
            cosmetic_priority(&v, Some([0.0; 3]), Owner::number(9.0), 300.0),
            Some(-1.0)
        );
        assert_eq!(
            cosmetic_priority(&v, None, Owner::number(9.0), 300.0),
            Some(0.0)
        );
    }
    #[test]
    fn replacements_keep_map_order_but_old_ack_only_confirms_the_projectile() {
        let mut b = Outbox::new(300.0);
        let e = event(3, 6.0, 10.0);
        b.offer(vec![e.clone()], Some([0.0; 3]), Owner::number(7.0));
        assert_eq!(
            b.pack(10.0, 20.0, 0.0, true, 0.0).unwrap().first,
            [4, 0, 3, 3, 3, 3]
        );
        let mut replacement = (*e).clone();
        replacement.identity = 99;
        replacement.meta.expires = 32.0;
        b.offer(
            vec![Rc::new(replacement)],
            Some([0.0; 3]),
            Owner::number(7.0),
        );
        b.acknowledge(10.0);
        assert_eq!(b.entries.size(), 1);
        assert!(b.projectiles.get(&3.0).unwrap().confirmed);
        let newer = event(3, 6.0, 14.0);
        b.offer(vec![newer], Some([0.0; 3]), Owner::number(7.0));
        assert!(b.pack(14.0, 20.0, 0.0, true, 0.0).unwrap().first.is_empty());
        assert!(!b
            .pack(16.0, 20.0, 0.0, true, 20.0)
            .unwrap()
            .first
            .is_empty());
        assert_eq!(b.entries.size(), 0);
    }
    #[test]
    fn full_first_datagram_keeps_insertion_order_and_later_ack_resends() {
        let mut b = Outbox::new(300.0);
        b.offer(
            vec![event(8, 8.0, 10.0), event(1, 1.0, 10.0)],
            None,
            Owner::default(),
        );
        assert_eq!(
            b.pack(10.0, 12.0, 0.0, true, f64::INFINITY).unwrap().first,
            [4, 0, 8, 8, 8, 8, 4, 0, 1, 1, 1, 1]
        );
        b.acknowledge(12.0);
        assert_eq!(b.resent, 1.0);
        assert_eq!(
            b.pack(14.0, 6.0, 0.0, true, 0.0).unwrap().first,
            [4, 0, 1, 1, 1, 1]
        );
        b.resend();
        assert_eq!(b.resent, 2.0);
        b.pack(100.0, 0.0, 0.0, true, 0.0).unwrap();
        assert_eq!(b.dropped, 1.0);
        assert_eq!(b.owed_dropped, 1.0);
    }
    #[test]
    fn unknown_removals_and_capacity_limit_match_queue_rules() {
        let mut b = Outbox::new(300.0);
        b.offer(vec![event(1, 7.0, 0.0)], None, Owner::default());
        assert_eq!(b.entries.size(), 0);
        b.offer(
            (0..1100).map(|id| event(id, 5.0, 0.0)).collect(),
            None,
            Owner::default(),
        );
        assert_eq!(b.entries.size(), 1024);
        assert_eq!(b.dropped, 76.0);
        assert_eq!(b.owed_dropped, 0.0);
        assert_eq!(b.entries.keys()[0], key('e', 76.0));
        b.pack(19.0, 0.0, 0.0, true, 0.0).unwrap();
        assert_eq!(b.dropped, 1100.0);
        assert_eq!(b.owed_dropped, 1024.0);
    }
}
