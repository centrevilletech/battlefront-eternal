//! Semantic entity publication, from network-entity-capture/items/remotes.js.
use crate::gen::{
    WcCall, WcResult, WcStamped, WireRecord, WireRecordKind as Kind, WireRecordState,
};
use crate::store::{StrId, Strings};
use jsrt::{js_max, js_min, js_round, num_or, same_value_zero};
type Result<T> = std::result::Result<T, String>;

fn string(strings: &Strings, id: StrId) -> String {
    String::from_utf16_lossy(strings.units(id))
}
fn health(value: f64) -> Result<f64> {
    if !value.is_finite() || value > 65535. {
        return Err("entity health outside uint16 representation".into());
    }
    Ok(js_round(js_max(0., value)))
}
fn enumerate(names: &[&str], name: &str) -> Result<f64> {
    names
        .iter()
        .position(|n| *n == name)
        .map(|i| i as f64)
        .ok_or_else(|| format!("unsupported entity state {name}"))
}
fn stamped(record: WireRecord, tick: f64) -> WcStamped {
    WcStamped {
        record,
        tick,
        ..WcStamped::default()
    }
}
fn map_set(values: &mut Vec<WcStamped>, value: WcStamped) {
    if let Some(at) = values
        .iter()
        .position(|r| same_value_zero(r.record.id, value.record.id))
    {
        values[at] = value;
    } else {
        values.push(value);
    }
}

#[derive(Clone, Default)]
struct ItemSlot {
    generation: f64,
    token: Option<f64>,
    record: Option<WcStamped>,
    raw: Option<[f64; 10]>,
    bytes: Option<Vec<u8>>,
}
pub struct Capture {
    capacity: f64,
    known: Vec<WcStamped>,
    seats: Vec<WcStamped>,
    lease_serial: f64,
    live: Vec<WcStamped>,
    records: Vec<WcStamped>,
    items: Vec<ItemSlot>,
    objects: Vec<(f64, usize)>,
    remotes: Vec<WcStamped>,
}
impl Capture {
    pub fn new(item_capacity: usize, capacity: f64) -> Self {
        Self {
            capacity,
            known: Vec::new(),
            seats: Vec::new(),
            lease_serial: 0.,
            live: Vec::new(),
            records: Vec::new(),
            items: vec![ItemSlot::default(); item_capacity],
            objects: Vec::new(),
            remotes: Vec::new(),
        }
    }
    pub fn begin(&mut self, records: Vec<WcStamped>) {
        self.live.clear();
        self.records = records;
    }
    pub fn soldier(&mut self, mut r: WireRecord, c: &WcCall, strings: &Strings) -> Result<()> {
        let air = string(strings, c.air);
        let state = if c.dead {
            if air == "thrown" {
                "thrownDead"
            } else {
                "dead"
            }
        } else if jsrt::num_truthy(c.dive) {
            if c.dive > 0. {
                "diveRight"
            } else {
                "diveLeft"
            }
        } else if air.is_empty() {
            "ground"
        } else {
            &air
        };
        r.kind = if c.walker {
            Kind::Droideka
        } else {
            Kind::Soldier
        };
        r.health = Some(health(r.health.unwrap_or(f64::NAN))?);
        r.posture = enumerate(&["stand", "crouch", "prone"], &string(strings, c.posture))?;
        r.state = WireRecordState::F64(enumerate(
            &[
                "ground",
                "jump",
                "fall",
                "thrown",
                "jet",
                "diveRight",
                "diveLeft",
                "dead",
                "thrownDead",
            ],
            state,
        )?);
        r.charge = Some(if c.dead || !c.charging {
            0.
        } else {
            let most = num_or(c.max_pressed_time, 0.);
            if most > 0. {
                js_max(1., js_min(63., js_round((c.held / most) * 63.)))
            } else {
                0.
            }
        });
        let r = stamped(r, c.tick);
        map_set(&mut self.live, r.clone());
        self.records.push(r.clone());
        let old = self
            .seats
            .iter()
            .position(|s| same_value_zero(s.record.id, r.record.id));
        if c.mounted || old.is_some() {
            let same = old.is_some_and(|i| {
                let s = &self.seats[i];
                s.record.generation == r.record.generation
                    && s.record.vehicle_id == Some(c.vehicle_id)
                    && s.vehicle_generation == c.vehicle_generation
                    && s.record.seat_index == Some(c.seat_index)
            });
            if !same {
                self.lease_serial += 1.;
                map_set(
                    &mut self.seats,
                    WcStamped {
                        record: WireRecord {
                            kind: Kind::Seat,
                            id: r.record.id,
                            generation: r.record.generation,
                            vehicle_id: Some(c.vehicle_id),
                            seat_index: Some(c.seat_index),
                            ..WireRecord::default()
                        },
                        tick: c.tick,
                        vehicle_generation: c.vehicle_generation,
                        lease_version: self.lease_serial,
                    },
                );
            }
            self.records.push(
                self.seats
                    .iter()
                    .find(|s| same_value_zero(s.record.id, r.record.id))
                    .unwrap()
                    .clone(),
            );
        }
        Ok(())
    }
    pub fn vehicle(&mut self, mut r: WireRecord, c: &WcCall, strings: &Strings) -> Result<()> {
        r.kind = Kind::Vehicle;
        r.health = Some(health(r.health.unwrap_or(f64::NAN))?);
        let flight = string(strings, c.flight_state);
        let state = if c.dying > 0. {
            "Dying"
        } else if flight.is_empty() {
            "Ground"
        } else {
            &flight
        };
        r.state = WireRecordState::F64(enumerate(
            &[
                "Landed",
                "TakingOff",
                "Flying",
                "Landing",
                "Destroyed",
                "Ground",
                "Dying",
            ],
            state,
        )?);
        r.power = if c.dying > 0. {
            js_min(65535., (c.dying * 1000.).ceil())
        } else {
            js_round(num_or(r.power, 0.))
        };
        let value = stamped(r, c.tick);
        map_set(&mut self.live, value.clone());
        self.records.push(value);
        Ok(())
    }
    pub fn mount(&mut self, mut r: WireRecord, tick: f64) {
        r.kind = Kind::Mount;
        r.slot = r.slot.filter(|s| *s >= 0. && *s < 255.);
        r.tilt = num_or(r.tilt, 0.);
        r.roll = num_or(r.roll, 0.);
        r.walk_phase = num_or(r.walk_phase, 0.);
        r.walk_blend = num_or(r.walk_blend, 0.);
        if let Some(seats) = r.seats.as_mut() {
            for seat in seats {
                seat.yaw = num_or(seat.yaw, 0.);
                seat.pitch = num_or(seat.pitch, 0.);
            }
        }
        self.records.push(stamped(r, tick));
    }
    pub fn turret(&mut self, mut r: WireRecord, tick: f64, strings: &Strings) {
        let state = match r.state {
            WireRecordState::Str(s) => string(strings, s),
            _ => String::new(),
        };
        if r.building < 0.
            || r.building > 65535.
            || !["unbuilt", "built", "destroyed"].contains(&state.as_str())
        {
            return;
        }
        r.kind = Kind::Turret;
        r.team = Some(js_max(0., js_min(255., num_or(r.team.unwrap_or(0.), 0.))));
        let h = r.health.unwrap_or(f64::NAN);
        r.health = Some(js_round(js_max(
            0.,
            js_min(65535., if h.is_finite() { h } else { 65535. }),
        )));
        r.yaw = Some(num_or(r.yaw.unwrap_or(0.), 0.));
        r.pitch = Some(num_or(r.pitch.unwrap_or(0.), 0.));
        r.gun_yaw = num_or(r.gun_yaw, 0.);
        r.gun_pitch = num_or(r.gun_pitch, 0.);
        self.records.push(stamped(r, tick));
    }
    pub fn finalize(&mut self, tick: f64) -> Result<()> {
        for old in &self.known {
            if self
                .live
                .iter()
                .any(|r| same_value_zero(r.record.id, old.record.id))
            {
                continue;
            }
            let value = if old.record.kind == Kind::Tombstone {
                old.clone()
            } else {
                stamped(
                    WireRecord {
                        kind: Kind::Tombstone,
                        id: old.record.id,
                        generation: old.record.generation,
                        ..WireRecord::default()
                    },
                    tick,
                )
            };
            self.records.push(value.clone());
            map_set(&mut self.live, value);
        }
        if self.live.len() as f64 > self.capacity {
            return Err("semantic fixed-roster capacity exceeded".into());
        }
        self.known = self.live.clone();
        self.seats.retain(|s| {
            self.live.iter().any(|r| {
                same_value_zero(r.record.id, s.record.id) && r.record.kind != Kind::Tombstone
            })
        });
        Ok(())
    }
    pub fn finish(&mut self, tick: f64) -> Result<Vec<WcStamped>> {
        self.finalize(tick)?;
        Ok(self.records.clone())
    }
    pub fn held_records(&self) -> Vec<WireRecord> {
        self.records.iter().map(|r| r.record.clone()).collect()
    }
    pub fn item_begin(&mut self, tick: f64, live: &[f64]) {
        self.objects.retain(|(token, index)| {
            if live.iter().any(|id| same_value_zero(*id, *token)) {
                return true;
            }
            let slot = &mut self.items[*index];
            slot.token = None;
            slot.record = Some(stamped(
                WireRecord {
                    kind: Kind::Item,
                    id: *index as f64,
                    generation: slot.generation,
                    active: false,
                    ..WireRecord::default()
                },
                tick,
            ));
            slot.bytes = None;
            false
        });
    }
    pub fn item_prepare(&mut self, token: f64, r: &WireRecord) -> Result<(usize, bool)> {
        let index = if let Some((_, index)) = self
            .objects
            .iter()
            .find(|(t, _)| same_value_zero(*t, token))
        {
            *index
        } else {
            let index = self
                .items
                .iter()
                .position(|s| s.token.is_none())
                .ok_or("item capture capacity exceeded")?;
            let slot = &mut self.items[index];
            if slot.generation == 4294967295. {
                return Err("item slot generation exhausted".into());
            }
            slot.generation += 1.;
            slot.token = Some(token);
            self.objects.push((token, index));
            index
        };
        let p = r.position.unwrap_or([f64::NAN; 3]);
        let v = r.velocity.unwrap_or([f64::NAN; 3]);
        let up = r.up.unwrap_or([f64::NAN; 3]);
        let raw = [
            p[0],
            p[1],
            p[2],
            v[0],
            v[1],
            v[2],
            r.yaw.unwrap_or(f64::NAN),
            up[0],
            up[1],
            up[2],
        ];
        let slot = &mut self.items[index];
        if slot
            .record
            .as_ref()
            .is_some_and(|r| r.record.active && r.record.generation == slot.generation)
            && slot.raw == Some(raw)
        {
            return Ok((index, false));
        }
        slot.raw = Some(raw);
        Ok((index, true))
    }
    pub fn item_commit(&mut self, index: usize, mut r: WireRecord, tick: f64) -> Result<()> {
        let slot = self.items.get_mut(index).ok_or("invalid item slot")?;
        r.kind = Kind::Item;
        r.id = index as f64;
        r.generation = slot.generation;
        r.active = true;
        let bytes = item_bytes(&r, tick)?;
        if slot.bytes.as_ref().is_some_and(|b| {
            b.len() == bytes.len()
                && bytes
                    .iter()
                    .zip(b)
                    .enumerate()
                    .all(|(i, (a, b))| (9..=12).contains(&i) || a == b)
        }) {
            return Ok(());
        }
        slot.record = Some(stamped(r, tick));
        slot.bytes = Some(bytes);
        Ok(())
    }
    pub fn item_records(&self) -> Vec<WcStamped> {
        self.items.iter().filter_map(|s| s.record.clone()).collect()
    }
    pub fn remote(&mut self, mut r: WireRecord, tick: f64, dead: bool) {
        let old = self
            .remotes
            .iter()
            .find(|s| same_value_zero(s.record.id, r.id));
        if dead {
            if old.is_none_or(|s| s.record.generation != r.generation || s.record.active) {
                map_set(
                    &mut self.remotes,
                    stamped(
                        WireRecord {
                            kind: Kind::Remote,
                            id: r.id,
                            generation: r.generation,
                            active: false,
                            ..WireRecord::default()
                        },
                        tick,
                    ),
                );
            }
        } else {
            r.kind = Kind::Remote;
            r.active = true;
            r.health = Some(js_max(
                0.,
                js_min(65535., js_round(r.health.unwrap_or(f64::NAN))),
            ));
            map_set(&mut self.remotes, stamped(r, tick));
        }
    }
    pub fn run(&mut self, c: WcCall, strings: &Strings) -> Result<WcResult> {
        let mut out = WcResult::default();
        match jsrt::to_uint32(c.op) {
            0 => self.begin(c.records.clone()),
            1 => self.soldier(
                c.record.clone().ok_or("missing soldier capture")?,
                &c,
                strings,
            )?,
            2 => self.vehicle(
                c.record.clone().ok_or("missing vehicle capture")?,
                &c,
                strings,
            )?,
            3 => self.mount(c.record.clone().ok_or("missing mount capture")?, c.tick),
            4 => self.turret(
                c.record.clone().ok_or("missing turret capture")?,
                c.tick,
                strings,
            ),
            5 => out.records = self.finish(c.tick)?,
            6 => self.item_begin(c.tick, &c.live),
            7 => {
                let (index, changed) =
                    self.item_prepare(c.token, c.record.as_ref().ok_or("missing item capture")?)?;
                out.changed = changed;
                out.slot = index as f64;
                out.generation = self.items[index].generation;
            }
            8 => self.item_commit(
                jsrt::to_uint32(c.token) as usize,
                c.record.clone().ok_or("missing item capture")?,
                c.tick,
            )?,
            9 => out.records = self.item_records(),
            10 => self.remote(
                c.record.clone().ok_or("missing remote capture")?,
                c.tick,
                c.dead,
            ),
            11 => out.records = self.remotes.clone(),
            12 => {
                self.finalize(c.tick)?;
            }
            13 => {
                let mut records = self.item_records();
                records.extend(self.remotes.iter().cloned());
                self.begin(records);
            }

            _ => return Err("unknown entity capture operation".into()),
        }
        out.lease_serial = self.lease_serial;
        Ok(out)
    }
}

fn item_bytes(r: &WireRecord, tick: f64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fn int(bytes: &mut Vec<u8>, value: f64, min: f64, max: f64, width: usize) -> Result<()> {
        if !value.is_finite() || value.fract() != 0. || value < min || value > max {
            return Err("entity integer out of range".into());
        }
        bytes.extend_from_slice(&(value as i64).to_le_bytes()[..width]);
        Ok(())
    }
    fn quant(bytes: &mut Vec<u8>, value: f64, scale: f64, width: usize) -> Result<()> {
        if !value.is_finite() {
            return Err("nonfinite entity field".into());
        }
        let max = if width == 2 { 32767. } else { 2147483647. };
        int(bytes, js_round(value * scale), -max, max, width)
    }
    int(&mut bytes, 9., 0., 255., 1)?;
    for n in [r.id, r.generation, tick] {
        int(&mut bytes, n, 0., 4294967295., 4)?;
    }
    if r.id > 255. {
        return Err("entity integer out of range".into());
    }
    bytes.push(1);
    let up = r.up.ok_or("invalid item surface normal")?;
    if up.iter().any(|n| !n.is_finite()) || (v8math::hypot3(up[0], up[1], up[2]) - 1.).abs() > 0.001
    {
        return Err("invalid item surface normal".into());
    }
    int(
        &mut bytes,
        r.definition_id.unwrap_or(f64::NAN),
        0.,
        65535.,
        2,
    )?;
    for n in r.position.ok_or("invalid entity vector")? {
        quant(&mut bytes, n, 256., 4)?;
    }
    for n in r.velocity.ok_or("invalid entity vector")? {
        quant(&mut bytes, n, 32., 2)?;
    }
    let yaw = r.yaw.unwrap_or(f64::NAN);
    quant(
        &mut bytes,
        v8math::atan2(v8math::sin(yaw), v8math::cos(yaw)),
        32767. / std::f64::consts::PI,
        2,
    )?;
    for n in up {
        quant(&mut bytes, n, 32767., 2)?;
    }
    Ok(bytes)
}

#[no_mangle]
pub extern "C" fn wire_capture_source_words() -> u32 {
    WcCall::WORDS as u32
}
#[no_mangle]
pub extern "C" fn wire_capture_source_create(item_capacity: u32, capacity: f64) -> *mut Capture {
    Box::into_raw(Box::new(Capture::new(item_capacity as usize, capacity)))
}
/// # Safety
/// The capture pointer belongs to this instance and is released once.
#[no_mangle]
pub unsafe extern "C" fn wire_capture_source_drop(capture: *mut Capture) {
    if !capture.is_null() {
        drop(Box::from_raw(capture));
    }
}
/// # Safety
/// The capture pointer remains live; all input is owned before execution.
#[no_mangle]
pub unsafe extern "C" fn wire_capture_source_run(capture: *mut Capture) -> *const f64 {
    let input = {
        let (row, heap) = crate::abi::sim().calls.args();
        WcCall::take(row, heap)
    };
    let result = (*capture).run(input, &crate::abi::sim().strings);
    let s = crate::abi::sim();
    let out = result.unwrap_or_else(|error| WcResult {
        error: Some(s.strings.intern_str(&error)),
        ..WcResult::default()
    });
    s.calls
        .result(WcResult::WORDS, |row, heap| out.put(row, heap));
    s.calls.block.ptr()
}
