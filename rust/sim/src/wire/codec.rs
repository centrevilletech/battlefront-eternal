//! Snapshot delta encoding, in publication and field order. Integer checks
//! deliberately happen at writes, as in the JavaScript codec.
use crate::gen::{
    WireClass as Class, WireEntry as Entry, WireField as Field, WireOwn, WireSnapshot as Snapshot,
};

pub type Result<T> = std::result::Result<T, &'static str>;
pub const MAX_DATAGRAM: usize = 1100;
const ENTITIES: u32 = 0x31534e42;
const OWN: u32 = 0x314f4e42;

#[derive(Default)]
pub struct Writer(pub Vec<u8>);
impl Writer {
    pub fn int(&mut self, v: f64, min: f64, max: f64, bytes: usize) -> Result<()> {
        if !v.is_finite() || v.fract() != 0.0 || v < min || v > max {
            return Err("snapshot integer out of range");
        }
        self.0.extend_from_slice(&(v as i64).to_le_bytes()[..bytes]);
        Ok(())
    }
    pub fn u8(&mut self, v: f64) -> Result<()> {
        self.int(v, 0., 255., 1)
    }
    pub fn i8(&mut self, v: f64) -> Result<()> {
        self.int(v, -128., 127., 1)
    }
    pub fn u16(&mut self, v: f64) -> Result<()> {
        self.int(v, 0., 65535., 2)
    }
    pub fn i16(&mut self, v: f64) -> Result<()> {
        self.int(v, -32768., 32767., 2)
    }
    pub fn u32(&mut self, v: f64) -> Result<()> {
        self.int(v, 0., 4294967295., 4)
    }
    pub fn f32(&mut self, v: f64) -> Result<()> {
        if !v.is_finite() {
            return Err("snapshot nonfinite f32");
        }
        self.0.extend_from_slice(&(v as f32).to_le_bytes());
        Ok(())
    }
    pub fn varint(&mut self, mut v: usize) {
        loop {
            let b = (v & 127) as u8;
            v >>= 7;
            self.0.push(b | if v != 0 { 128 } else { 0 });
            if v == 0 {
                break;
            }
        }
    }
    pub fn raw(&mut self, v: &[u8]) {
        self.0.extend_from_slice(v);
    }
}
fn header(
    w: &mut Writer,
    magic: u32,
    epoch: f64,
    tick: f64,
    interval: f64,
    base: f64,
    index: usize,
    count: usize,
    ack: f64,
) -> Result<()> {
    if count == 0 || count > if magic == ENTITIES { 256 } else { 2 } || index >= count {
        return Err("invalid snapshot parts");
    }
    w.u32(magic as f64)?;
    w.u16(epoch)?;
    w.u32(tick)?;
    w.u8(interval)?;
    w.u32(base)?;
    w.u8(if count > 15 {
        0.
    } else {
        ((index << 4) | count) as f64
    })?;
    w.u32(ack)?;
    if count > 15 {
        w.u16(index as f64)?;
        w.u16(count as f64)?;
    }
    Ok(())
}
fn group_count(cls: Class) -> usize {
    match cls {
        Class::Turret => 2,
        Class::Item | Class::Remote => 4,
        _ => 6,
    }
}
fn same_pos(a: &Entry, b: &Entry) -> bool {
    !a.disc && a.pos == b.pos
}
fn same_vel(a: &Entry, b: &Entry) -> bool {
    a.vel.wide == b.vel.wide && a.vel.v == b.vel.v
}
fn same_status(a: &Entry, b: &Entry) -> bool {
    a.posture == b.posture
        && a.state == b.state
        && a.weapon == b.weapon
        && a.seat_slot == b.seat_slot
        && a.seat_index == b.seat_index
}
fn same_group(a: &Entry, b: &Entry, group: usize) -> bool {
    if a.cls != Class::Turret {
        if group == 0 {
            return same_pos(a, b);
        }
        if group == 1 {
            return same_vel(a, b);
        }
    }
    match (a.cls, group) {
        (Class::Soldier | Class::Droideka | Class::Remote, 2) => {
            a.yaw == b.yaw && a.pitch == b.pitch
        }
        (Class::Soldier, 3) => a.health == b.health,
        (Class::Soldier, 4) => same_status(a, b) && a.charge == b.charge,
        (Class::Droideka, 3) => same_status(a, b) && a.health == b.health,
        (Class::Droideka, 4) => {
            a.mode == b.mode
                && a.reverse == b.reverse
                && a.dead == b.dead
                && a.shield_on == b.shield_on
                && a.anim.len() == b.anim.len()
                && a.anim.iter().zip(&b.anim).all(|(x, y)| {
                    (*x as f32).to_bits() == (*y as f32).to_bits() || x.is_nan() && y.is_nan()
                })
        }
        (Class::Soldier | Class::Droideka | Class::Vehicle, 5) => {
            (jsrt::to_int32(a.fire_counter) & 255) == (jsrt::to_int32(b.fire_counter) & 255)
        }
        (Class::Vehicle, 2) => a.quat.index == b.quat.index && a.quat.v == b.quat.v,
        (Class::Vehicle, 3) => a.health == b.health && a.state == b.state && a.power == b.power,
        (Class::Vehicle, 4) => a.mount == b.mount,
        (Class::Turret, 0) => {
            a.yaw == b.yaw
                && a.pitch == b.pitch
                && a.gun_yaw == b.gun_yaw
                && a.gun_pitch == b.gun_pitch
        }
        (Class::Turret, 1) => a.health == b.health && a.state == b.state && a.team == b.team,
        (Class::Item, 2) => a.yaw == b.yaw,
        (Class::Item, 3) => {
            a.active == b.active
                && (!a.active || a.definition_id == b.definition_id && a.up == b.up)
        }
        (Class::Remote, 3) => {
            a.active == b.active
                && (!a.active
                    || a.definition_id == b.definition_id
                        && a.team == b.team
                        && a.health == b.health)
        }
        _ => unreachable!(),
    }
}
fn write_status(w: &mut Writer, e: &Entry) -> Result<()> {
    w.u8(e.state)?;
    w.u16(e.weapon)?;
    w.u16(if e.seat_slot < 0. {
        65535.
    } else {
        e.seat_slot
    })?;
    w.u8(e.seat_index)
}
fn write_group(w: &mut Writer, e: &Entry, base: Option<&Entry>, group: usize) -> Result<()> {
    if e.cls != Class::Turret {
        if group == 0 {
            let small =
                base.is_some_and(|b| e.pos.iter().zip(b.pos).all(|(a, b)| (*a - b).abs() <= 127.));
            w.u8((if small { 0 } else { 1 } | if e.disc { 128 } else { 0 }) as f64)?;
            for i in 0..3 {
                if small {
                    w.i8(e.pos[i] - base.unwrap().pos[i])?;
                } else {
                    w.u16(e.pos[i])?;
                }
            }
            return Ok(());
        }
        if group == 1 {
            w.u8(e.vel.wide as u8 as f64)?;
            for n in e.vel.v {
                if e.vel.wide {
                    w.i16(n)?;
                } else {
                    w.i8(n)?;
                }
            }
            return Ok(());
        }
    }
    match (e.cls, group) {
        (Class::Soldier | Class::Droideka | Class::Remote, 2) => {
            w.u16(e.yaw)?;
            w.i16(e.pitch)?;
        }
        (Class::Soldier, 3) => w.u16(e.health)?,
        (Class::Soldier, 4) => {
            w.u8((jsrt::to_int32(e.posture) | jsrt::to_int32(e.charge).wrapping_shl(2)) as f64)?;
            write_status(w, e)?;
        }
        (Class::Droideka, 3) => {
            w.u16(e.health)?;
            w.u8(e.posture)?;
            write_status(w, e)?;
        }
        (Class::Droideka, 4) => {
            w.u8(e.mode)?;
            w.u8((e.reverse as u8 | (e.dead as u8) << 1 | (e.shield_on as u8) << 2) as f64)?;
            for n in &e.anim {
                w.f32(*n)?;
            }
        }
        (Class::Soldier | Class::Droideka | Class::Vehicle, 5) => {
            w.u8((jsrt::to_int32(e.fire_counter) & 255) as f64)?
        }
        (Class::Vehicle, 2) => {
            let mut n =
                e.quat.index + e.quat.v[0] * 4. + e.quat.v[1] * 131072. + e.quat.v[2] * 4294967296.;
            for _ in 0..6 {
                w.u8(n % 256.)?;
                n = (n / 256.).floor();
            }
        }
        (Class::Vehicle, 3) => {
            w.u16(e.health)?;
            w.u8(e.state)?;
            w.u16(e.power)?;
        }
        (Class::Vehicle, 4) => {
            let m = &e.mount;
            w.u8(m.slot.unwrap_or(255.))?;
            w.i16(m.tilt)?;
            w.i16(m.roll)?;
            w.u16(m.walk_phase)?;
            w.u8(m.walk_blend)?;
            w.u8(m.seats.len() as f64)?;
            for s in &m.seats {
                w.i16(s[0])?;
                w.i16(s[1])?;
            }
        }
        (Class::Turret, 0) => {
            for n in [e.yaw, e.pitch, e.gun_yaw, e.gun_pitch] {
                w.i16(n)?;
            }
        }
        (Class::Turret, 1) => {
            w.u16(e.health)?;
            w.u8(e.state)?;
            w.u8(e.team)?;
        }
        (Class::Item, 2) => w.u16(e.yaw)?,
        (Class::Item, 3) => {
            w.u8(e.active as u8 as f64)?;
            if e.active {
                w.u16(e.definition_id)?;
                for n in e.up {
                    w.i16(n)?;
                }
            }
        }
        (Class::Remote, 3) => {
            w.u8(e.active as u8 as f64)?;
            if e.active {
                w.u16(e.definition_id)?;
                w.u8(e.team)?;
                w.u16(e.health)?;
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}
fn record(e: &Entry, baseline: Option<&Entry>) -> Result<Vec<u8>> {
    let groups = group_count(e.cls);
    let new = baseline.is_none_or(|b| {
        b.cls != e.cls
            || b.generation != e.generation
            || b.class_id != e.class_id
            || b.entity_id != e.entity_id
            || e.cls != Class::Turret && b.team != e.team
    });
    let mut w = Writer::default();
    w.u8(0.)?;
    if new {
        w.0[0] = 1 | (((1 << groups) - 1) << 2);
        w.u8(e.cls as u8 as f64)?;
        w.u32(e.generation)?;
        w.u32(e.entity_id)?;
        w.u16(e.class_id)?;
        w.u8(e.team)?;
        for g in 0..groups {
            write_group(&mut w, e, None, g)?;
        }
    } else {
        let base = baseline.unwrap();
        let mut mask = 0;
        for g in 0..groups {
            if !same_group(e, base, g) {
                mask |= 1 << g;
                write_group(&mut w, e, Some(base), g)?;
            }
        }
        if mask == 0 {
            return Ok(Vec::new());
        }
        w.0[0] = mask << 2;
    }
    Ok(w.0)
}
fn changed_words(current: &[u8], base: Option<&[u8]>) -> Option<usize> {
    let base = base.filter(|b| b.len() == current.len())?;
    Some(
        current
            .chunks(8)
            .zip(base.chunks(8))
            .filter(|(a, b)| a != b)
            .count(),
    )
}
fn delta_wins(len: usize, changed: Option<usize>) -> bool {
    changed.is_some_and(|n| 5 + len.div_ceil(8).div_ceil(8) + n * 8 < len + 3)
}
fn changed_delta(w: &mut Writer, current: &[u8], base: &[u8]) -> Result<()> {
    let words = current.len().div_ceil(8);
    w.u8(1.)?;
    w.u16(current.len() as f64)?;
    w.u16(words as f64)?;
    let bitmap = w.0.len();
    w.0.resize(bitmap + words.div_ceil(8), 0);
    for (i, (a, b)) in current.chunks(8).zip(base.chunks(8)).enumerate() {
        if a != b {
            w.0[bitmap + i / 8] |= 1 << (i % 8);
            w.raw(a);
            w.0.resize(w.0.len() + 8 - a.len(), 0);
        }
    }
    Ok(())
}
pub fn word_delta(current: &[u8], base: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut w = Writer::default();
    if delta_wins(current.len(), changed_words(current, base)) {
        changed_delta(&mut w, current, base.unwrap())?;
    } else {
        w.u8(0.)?;
        w.u16(current.len() as f64)?;
        w.raw(current);
    }
    Ok(w.0)
}
fn field(w: &mut Writer, f: &Field, removed: bool, base: Option<&Field>) -> Result<()> {
    w.u32(f.id)?;
    w.u16(f.schema)?;
    w.u32(f.generation)?;
    if removed {
        return w.u8(1.);
    }
    if let Some(b) = base.filter(|b| b.generation == f.generation) {
        let changed = changed_words(&f.payload, Some(&b.payload));
        if delta_wins(f.payload.len(), changed) {
            w.u8(2.)?;
            w.u16((5 + f.payload.len().div_ceil(8).div_ceil(8) + changed.unwrap() * 8) as f64)?;
            return changed_delta(w, &f.payload, &b.payload);
        }
    }
    w.u8(0.)?;
    w.u16(f.payload.len() as f64)?;
    w.raw(&f.payload);
    Ok(())
}
fn world_entries(current: &[Field], baseline: &[Field]) -> Result<Vec<Vec<u8>>> {
    let mut out = Vec::new();
    for f in current {
        let b = baseline
            .iter()
            .find(|b| b.id == f.id && b.schema == f.schema);
        if b.is_some_and(|b| b.generation == f.generation && b.payload == f.payload) {
            continue;
        }
        let mut w = Writer::default();
        field(&mut w, f, false, b)?;
        out.push(w.0);
    }
    for f in baseline {
        if !current.iter().any(|c| c.id == f.id && c.schema == f.schema) {
            let mut w = Writer::default();
            field(&mut w, f, true, None)?;
            out.push(w.0);
        }
    }
    Ok(out)
}
pub fn world_section(current: &[Field], baseline: &[Field]) -> Result<Vec<u8>> {
    let entries = world_entries(current, baseline)?;
    let mut w = Writer::default();
    w.varint(entries.len());
    for e in entries {
        w.raw(&e);
    }
    Ok(w.0)
}
#[derive(Default)]
struct Part {
    slots: Vec<usize>,
    world: Vec<usize>,
    bytes: usize,
}
fn pack(
    header_bytes: usize,
    units: &[Vec<usize>],
    records: &[Vec<u8>],
    fields: &[Vec<u8>],
) -> Result<Vec<Part>> {
    let overhead = header_bytes + 4;
    let mut out = vec![Part {
        bytes: overhead,
        ..Part::default()
    }];
    for unit in units {
        let size: usize = unit.iter().map(|s| records[*s].len() + 2).sum();
        if overhead + size > MAX_DATAGRAM {
            return Err("snapshot record exceeds datagram");
        }
        if out.last().unwrap().bytes + size > MAX_DATAGRAM {
            out.push(Part {
                bytes: overhead,
                ..Part::default()
            });
        }
        let p = out.last_mut().unwrap();
        p.bytes += size;
        p.slots.extend(unit);
    }
    for (i, f) in fields.iter().enumerate() {
        if overhead + f.len() > MAX_DATAGRAM {
            return Err("snapshot record exceeds datagram");
        }
        if out.last().unwrap().bytes + f.len() > MAX_DATAGRAM {
            out.push(Part {
                bytes: overhead,
                ..Part::default()
            });
        }
        let p = out.last_mut().unwrap();
        p.bytes += f.len();
        p.world.push(i);
    }
    Ok(out)
}
pub fn encode_entities(
    current: &Snapshot,
    baseline: Option<&Snapshot>,
    interval: f64,
) -> Result<Vec<Vec<u8>>> {
    let mut entries: Vec<Option<&Entry>> = vec![None; 512];
    let mut bases = entries.clone();
    for s in &current.slots {
        let i = slot(s.slot)?;
        entries[i] = Some(&s.entry);
    }
    if let Some(b) = baseline {
        for s in &b.slots {
            let i = slot(s.slot)?;
            bases[i] = Some(&s.entry);
        }
    }
    let mut records = vec![Vec::new(); 512];
    let mut touched = Vec::new();
    for i in 0..512 {
        records[i] = match entries[i] {
            Some(e) => record(e, bases[i])?,
            None if bases[i].is_some() => vec![2],
            _ => Vec::new(),
        };
        if !records[i].is_empty() {
            touched.push(i);
        }
    }
    let fields = world_entries(&current.world, baseline.map_or(&[], |b| &b.world))?;
    let mut claimed = [false; 512];
    let mut units = Vec::new();
    for &s in &touched {
        if claimed[s] {
            continue;
        }
        let mut unit = vec![s];
        claimed[s] = true;
        if entries[s].is_some_and(|e| e.cls == Class::Vehicle) {
            for &other in &touched {
                if !claimed[other]
                    && entries[other].is_some_and(|e| {
                        e.seat_slot == s as f64 && matches!(e.cls, Class::Soldier | Class::Droideka)
                    })
                {
                    unit.push(other);
                    claimed[other] = true;
                }
            }
        }
        if let Some(e) = entries[s]
            .filter(|e| matches!(e.cls, Class::Soldier | Class::Droideka) && e.seat_slot >= 0.)
        {
            let rider = slot(e.seat_slot)?;
            if !records[rider].is_empty() && !claimed[rider] {
                unit.push(rider);
                claimed[rider] = true;
            }
        }
        unit.sort_unstable();
        units.push(unit);
    }
    let mut parts = pack(20, &units, &records, &fields)?;
    if parts.len() > 15 {
        parts = pack(24, &units, &records, &fields)?;
    }
    if parts.len() > 256 {
        return Err("snapshot needs too many parts");
    }
    let mut out = Vec::new();
    let count = parts.len();
    for (index, p) in parts.iter_mut().enumerate() {
        let mut w = Writer::default();
        header(
            &mut w,
            ENTITIES,
            current.epoch,
            current.tick,
            interval,
            baseline.map_or(0., |b| b.tick),
            index,
            count,
            0.,
        )?;
        let mut world = Writer::default();
        world.varint(p.world.len());
        for &i in &p.world {
            world.raw(&fields[i]);
        }
        w.u16(world.0.len() as f64)?;
        w.raw(&world.0);
        p.slots.sort_unstable();
        w.varint(p.slots.len());
        let mut prev = 0;
        for (j, &s) in p.slots.iter().enumerate() {
            if j == 0 {
                w.u16(s as f64)?;
            } else {
                w.varint(s - prev);
            }
            prev = s;
            w.raw(&records[s]);
        }
        if w.0.len() > MAX_DATAGRAM {
            return Err("snapshot part overflow");
        }
        out.push(w.0);
    }
    Ok(out)
}
fn slot(v: f64) -> Result<usize> {
    if !v.is_finite() || v.fract() != 0. || !(0. ..512.).contains(&v) {
        Err("snapshot slot out of range")
    } else {
        Ok(v as usize)
    }
}
pub fn own_datagram(args: &WireOwn) -> Result<Vec<u8>> {
    let mut w = Writer::default();
    if args.own_parts.fract() != 0. || args.own_parts < 1. || args.own_parts > 2. {
        return Err("invalid snapshot parts");
    }
    header(
        &mut w,
        OWN,
        args.epoch,
        args.tick,
        args.interval,
        args.baseline_tick,
        args.second as usize,
        args.own_parts as usize,
        args.ack_input,
    )?;
    let mut flags = if args.second { 8 } else { 0 };
    let correction = if let Some(c) = &args.correction {
        flags |= 1;
        let mut s = Writer::default();
        s.u32(c.id)?;
        s.u32(c.generation)?;
        s.u16(c.schema)?;
        let delta = match &args.correction_delta {
            Some(d) => d.clone(),
            None => word_delta(&c.payload, args.baseline_payload.as_deref())?,
        };
        s.raw(&delta);
        Some(s.0)
    } else {
        None
    };
    let summaries = if let Some(s) = &args.summaries_section {
        flags |= 2;
        Some(s.clone())
    } else if let Some(s) = &args.summaries {
        flags |= 2;
        Some(world_section(
            s,
            args.baseline_summaries.as_deref().unwrap_or(&[]),
        )?)
    } else {
        None
    };
    let cosmetics = args.cosmetic_bytes.as_ref().filter(|b| !b.is_empty());
    if cosmetics.is_some() {
        flags |= 4;
    }
    w.u8(flags as f64)?;
    for section in [correction.as_ref(), summaries.as_ref(), cosmetics]
        .into_iter()
        .flatten()
    {
        w.u16(section.len() as f64)?;
        w.raw(section);
    }
    if w.0.len() > MAX_DATAGRAM {
        return Err("own datagram exceeds budget");
    }
    Ok(w.0)
}
