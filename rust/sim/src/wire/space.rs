//! 2005 Space Assault wire records (revision 1, little-endian):
//!
//! - Kind 16 (`PILOT`, client->server `BNI1` records section): per-command
//!   [`PilotControls`] aligned to `BNI1` command sequences.
//! - Kind 17 (`FIGHTER`, `0x0211` shared world fields + `0x91` own correction
//!   body): 8-fighter chunks (`id = 0..15`, tag `0`), Space seat occupancy map
//!   (`id = 256`, tag `1`), and lossless own cockpit/gunner/chair state (`0x91`).
//! - Kind 19 (`FRIGATES`, `0x0213` shared world field, `id = 0`): deterministic
//!   world animation group clocks `(state, origin)`.
//! - Kind 20 (`ORDNANCE`, `0x0214` shared world fields): live guided/curved
//!   rounds (`id = 0`, tag `0`) and publication-window linear launches, impacts,
//!   and destruction bursts (`id = 1`, tag `1`).
//! - Kind 21 (`SPACE_PROPS`, `0x0215` shared world fields): packed Space turret
//!   aims (`id = 0`, tag `0`) and proximity interior doors (`id = 1`, tag `1`).
//!
//! Every reader validates every length, count, flag bit, index, and float before
//! returning, rejecting trailing bytes and out-of-range values.
use super::mission::Offer;
use super::revisions::{body_schema, field_schema, FIGHTER, FRIGATES, ORDNANCE, SPACE_PROPS};
use super::PositionBounds;
use crate::space::flight::{
    SpaceFighter, SpaceFlightProfile, SpaceFlightState, SpaceLandingPermission, SpaceLockState,
    SpaceOrdnanceKind, SpacePilotInput, SpaceTrick,
};
use crate::space::local::{CarriedTaps, DoubleTapDetector, PilotControls};
use crate::space::objects::SpaceBuildingAttachment;
use crate::space::SpaceBattlefield;
use crate::world::owner::World;
use jsrt::{js_max, js_min, js_round};

pub const REVISION: u16 = 1;
pub const FIGHTER_SCHEMA: u16 = field_schema(FIGHTER);
pub const FRIGATES_SCHEMA: u16 = field_schema(FRIGATES);
pub const ORDNANCE_SCHEMA: u16 = field_schema(ORDNANCE);
pub const PROPS_SCHEMA: u16 = field_schema(SPACE_PROPS);
pub const OWN_BODY_SCHEMA: u8 = body_schema(FIGHTER);

pub const MAX_PILOT_ENTRIES: usize = 8;
pub const PILOT_ENTRY_BYTES: usize = 26;
pub const MAX_FIGHTER_CHUNKS: usize = 16;
pub const FIGHTERS_PER_CHUNK: usize = 8;
pub const MAX_FIGHTERS: usize = MAX_FIGHTER_CHUNKS * FIGHTERS_PER_CHUNK;
pub const FIGHTER_SNAP_BYTES: usize = 29;
pub const FIGHTER_CHUNK_BYTES: usize = 2 + 3 + FIGHTERS_PER_CHUNK * FIGHTER_SNAP_BYTES;
pub const SEATS_FIELD_ID: u32 = 256;
pub const MAX_SEATS: usize = 128;
pub const SEATS_BYTES: usize = 2 + 2 + MAX_SEATS * 4;
pub const MAX_ANIM_GROUPS: usize = 32;
pub const FRIGATES_BYTES: usize = 2 + 1 + MAX_ANIM_GROUPS * 5;
pub const MAX_GUIDED: usize = 44;
pub const GUIDED_BYTES: usize = 2 + 2 + MAX_GUIDED * 19;
pub const MAX_LAUNCHES: usize = 32;
pub const MAX_IMPACTS: usize = 32;
pub const MAX_DEATHS: usize = 8;
pub const EVENTS_BYTES: usize = 896;
pub const MAX_TURRETS: usize = 96;
pub const TURRETS_BYTES: usize = 2 + 2 + MAX_TURRETS * 4;
pub const MAX_DOORS: usize = 64;
pub const DOORS_BYTES: usize = 2 + 2 + MAX_DOORS * 2;
pub const MAX_SPACE_WEAPONS: usize = 128;
pub const MAX_SPACE_PROFILES: usize = 32;

const INVALID: &str = "invalid Battlefront II space record";
const MAX_U24: f64 = 16_777_215.0;
const QUAT_RANGE: f64 = std::f64::consts::FRAC_1_SQRT_2;
const PI: f64 = std::f64::consts::PI;

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], &'static str> {
        let (head, rest) = self.0.split_at_checked(n).ok_or(INVALID)?;
        self.0 = rest;
        Ok(head)
    }
    fn u8(&mut self) -> Result<u8, &'static str> {
        Ok(self.take(1)?[0])
    }
    fn i8(&mut self) -> Result<i8, &'static str> {
        Ok(self.u8()? as i8)
    }
    fn u16(&mut self) -> Result<u16, &'static str> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().map_err(|_| INVALID)?))
    }
    fn i16(&mut self) -> Result<i16, &'static str> {
        Ok(self.u16()? as i16)
    }
    fn u24(&mut self) -> Result<u32, &'static str> {
        let b = self.take(3)?;
        Ok(u32::from(b[0]) | (u32::from(b[1]) << 8) | (u32::from(b[2]) << 16))
    }
    fn u32(&mut self) -> Result<u32, &'static str> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().map_err(|_| INVALID)?))
    }
    fn f32(&mut self) -> Result<f32, &'static str> {
        let v = f32::from_bits(self.u32()?);
        if v.is_finite() { Ok(v) } else { Err(INVALID) }
    }
    fn f64(&mut self) -> Result<f64, &'static str> {
        let b = self.take(8)?;
        let v = f64::from_le_bytes(b.try_into().map_err(|_| INVALID)?);
        if v.is_finite() { Ok(v) } else { Err(INVALID) }
    }
    fn pos_codes(&mut self) -> Result<[u32; 3], &'static str> {
        Ok([self.u24()?, self.u24()?, self.u24()?])
    }
    fn vel_i16(&mut self) -> Result<[i16; 3], &'static str> {
        Ok([self.i16()?, self.i16()?, self.i16()?])
    }
    fn end(self) -> Result<(), &'static str> {
        if self.0.is_empty() { Ok(()) } else { Err(INVALID) }
    }
}

#[inline]
fn push_u24(v: u32, out: &mut Vec<u8>) {
    let b = v.to_le_bytes();
    out.extend_from_slice(&b[..3]);
}

#[inline]
pub fn encode_pos_u24(bounds: &PositionBounds, pos: [f64; 3]) -> [u32; 3] {
    let (min, extent) = bounds.padded_axes();
    std::array::from_fn(|a| {
        let p = if pos[a].is_finite() { pos[a] } else { min[a] };
        let c = js_round((p - min[a]) / extent[a] * MAX_U24);
        js_max(0.0, js_min(MAX_U24, c)) as u32
    })
}

#[inline]
pub fn decode_pos_u24(bounds: &PositionBounds, codes: [u32; 3]) -> [f64; 3] {
    let (min, extent) = bounds.padded_axes();
    std::array::from_fn(|a| min[a] + f64::from(codes[a]) / MAX_U24 * extent[a])
}

#[inline]
pub fn encode_vel(vel: [f64; 3], scale: f64) -> [i16; 3] {
    vel.map(|v| {
        let x = if v.is_finite() { v } else { 0.0 };
        js_max(-32767.0, js_min(32767.0, js_round(x * scale))) as i16
    })
}

#[inline]
pub fn decode_vel(codes: [i16; 3], scale: f64) -> [f64; 3] {
    codes.map(|c| f64::from(c) / scale)
}

#[inline]
pub fn pack_quat(q: [f64; 4]) -> [u8; 6] {
    let norm = v8math::hypot_n(&q);
    let q = if norm > 1e-9 && norm.is_finite() {
        q.map(|x| x / norm)
    } else {
        [0.0, 0.0, 0.0, 1.0]
    };
    let mut index = 0usize;
    for i in 1..4 {
        if q[i].abs() > q[index].abs() {
            index = i;
        }
    }
    let sign = if q[index] < 0.0 { -1.0 } else { 1.0 };
    let mut vals = [0u64; 3];
    let mut k = 0;
    for (i, &x) in q.iter().enumerate() {
        if i != index {
            let c = js_max(
                0.0,
                js_min(32767.0, js_round((x * sign / QUAT_RANGE * 0.5 + 0.5) * 32767.0)),
            ) as u64;
            vals[k] = c;
            k += 1;
        }
    }
    let packed = (index as u64) | (vals[0] << 2) | (vals[1] << 17) | (vals[2] << 32);
    let bytes = packed.to_le_bytes();
    [bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5]]
}

#[inline]
pub fn unpack_quat(bytes: [u8; 6]) -> Result<[f64; 4], &'static str> {
    let mut raw = [0u8; 8];
    raw[..6].copy_from_slice(&bytes);
    let packed = u64::from_le_bytes(raw);
    if packed >> 47 != 0 {
        return Err(INVALID);
    }
    let index = (packed & 3) as usize;
    let v0 = ((packed >> 2) & 0x7fff) as u32;
    let v1 = ((packed >> 17) & 0x7fff) as u32;
    let v2 = ((packed >> 32) & 0x7fff) as u32;
    let vals = [v0, v1, v2].map(|c| ((f64::from(c) / 32767.0) * 2.0 - 1.0) * QUAT_RANGE);
    let sum_sq = vals[0] * vals[0] + vals[1] * vals[1] + vals[2] * vals[2];
    if sum_sq > 1.002 {
        return Err(INVALID);
    }
    let dropped = (1.0 - sum_sq).max(0.0).sqrt();
    let mut q = [0.0; 4];
    let mut k = 0;
    for (i, slot) in q.iter_mut().enumerate() {
        if i == index {
            *slot = dropped;
        } else {
            *slot = vals[k];
            k += 1;
        }
    }
    let norm = v8math::hypot_n(&q);
    if !norm.is_finite() || norm < 0.5 {
        return Err(INVALID);
    }
    Ok(q.map(|x| x / norm))
}

#[inline]
pub fn pack_oct_normal(n: [f64; 3]) -> [i8; 2] {
    let sum = n[0].abs() + n[1].abs() + n[2].abs();
    if !sum.is_finite() || sum < 1e-9 {
        return [0, 0];
    }
    let (mut x, mut y, z) = (n[0] / sum, n[1] / sum, n[2] / sum);
    if z < 0.0 {
        let ox = (1.0 - y.abs()) * if x >= 0.0 { 1.0 } else { -1.0 };
        let oy = (1.0 - x.abs()) * if y >= 0.0 { 1.0 } else { -1.0 };
        x = ox;
        y = oy;
    }
    [
        js_max(-127.0, js_min(127.0, js_round(x * 127.0))) as i8,
        js_max(-127.0, js_min(127.0, js_round(y * 127.0))) as i8,
    ]
}

#[inline]
pub fn unpack_oct_normal(oct: [i8; 2]) -> [f64; 3] {
    let (mut x, mut y) = (f64::from(oct[0]) / 127.0, f64::from(oct[1]) / 127.0);
    let z = 1.0 - x.abs() - y.abs();
    if z < 0.0 {
        let ox = (1.0 - y.abs()) * if x >= 0.0 { 1.0 } else { -1.0 };
        let oy = (1.0 - x.abs()) * if y >= 0.0 { 1.0 } else { -1.0 };
        x = ox;
        y = oy;
    }
    let len = v8math::hypot3(x, y, z);
    if len > 1e-9 { [x / len, y / len, z / len] } else { [0.0, 1.0, 0.0] }
}

/// Deterministic sorted table of Space weapon name hashes (`<= 128`) shared by server & client.
pub fn weapon_hash_table(
    profiles: &[SpaceFlightProfile],
    buildings: &[SpaceBuildingAttachment],
) -> Vec<u32> {
    let mut hashes = Vec::with_capacity(32);
    for p in profiles {
        for w in &p.weapons {
            if !w.name.is_empty() {
                hashes.push(crate::combat::combo::zero_fnv1a_lower(&w.name));
            }
        }
    }
    for b in buildings {
        if let Some(t) = &b.turret {
            if !t.weapon.name.is_empty() {
                hashes.push(crate::combat::combo::zero_fnv1a_lower(&t.weapon.name));
            }
        }
    }
    hashes.sort_unstable();
    hashes.dedup();
    hashes.truncate(MAX_SPACE_WEAPONS);
    hashes
}

// ---- Kind 16 (PILOT): client -> server BNI1 records section ----

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PilotEntry {
    pub sequence: u32,
    pub controls: PilotControls,
}

fn encode_ternary(v: i8) -> u16 {
    match v {
        1.. => 1,
        ..=-1 => 2,
        0 => 0,
    }
}

fn decode_ternary(bits: u16) -> Result<i8, &'static str> {
    match bits & 3 {
        0 => Ok(0),
        1 => Ok(1),
        2 => Ok(-1),
        _ => Err(INVALID),
    }
}

#[inline]
fn encode_unit_i16(v: f64) -> i16 {
    let x = if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 };
    js_max(-32767.0, js_min(32767.0, js_round(x * 32767.0))) as i16
}

#[inline]
fn decode_unit_i16(v: i16) -> f64 {
    (f64::from(v) / 32767.0).clamp(-1.0, 1.0)
}

#[inline]
fn encode_delta_i16(v: f64) -> i16 {
    let x = if v.is_finite() { v.clamp(-4.0, 4.0) } else { 0.0 };
    js_max(-32767.0, js_min(32767.0, js_round(x * 8191.0))) as i16
}

#[inline]
fn decode_delta_i16(v: i16) -> f64 {
    (f64::from(v) / 8191.0).clamp(-4.0, 4.0)
}

/// Applies the exact `PILOT` wire quantization and clamping round-trip to `c`
/// with zero heap allocations so client own-craft replay steps with the exact
/// controls the server decodes.
#[inline]
pub fn quantize_pilot_controls(c: PilotControls) -> PilotControls {
    let f = &c.flight;
    let clamp_ternary = |v: i8| match v {
        1.. => 1,
        ..=-1 => -1,
        0 => 0,
    };
    let trick_flip = if f.trick_flip <= 2 { f.trick_flip } else { 0 };
    PilotControls {
        use_pressed: c.use_pressed,
        cycle_weapon: clamp_ternary(c.cycle_weapon),
        terminal_cycle: clamp_ternary(c.terminal_cycle),
        turret_look: [
            decode_delta_i16(encode_delta_i16(c.turret_look[0])),
            decode_delta_i16(encode_delta_i16(c.turret_look[1])),
        ],
        turret_turn: decode_unit_i16(encode_unit_i16(c.turret_turn)),
        mouse: [
            decode_delta_i16(encode_delta_i16(c.mouse[0])),
            decode_delta_i16(encode_delta_i16(c.mouse[1])),
        ],
        flight: SpacePilotInput {
            throttle: decode_unit_i16(encode_unit_i16(f.throttle)),
            roll: decode_unit_i16(encode_unit_i16(f.roll)),
            stick_x: decode_unit_i16(encode_unit_i16(f.stick_x)),
            stick_y: decode_unit_i16(encode_unit_i16(f.stick_y)),
            boost: f.boost,
            takeoff_or_land: f.takeoff_or_land,
            eject_held: f.eject_held,
            fire_primary: f.fire_primary,
            fire_secondary: f.fire_secondary,
            lock_target: f.lock_target,
            trick_roll: clamp_ternary(f.trick_roll),
            trick_flip,
            ..Default::default()
        },
    }
}

pub fn write_pilot_entries(entries: &[PilotEntry], out: &mut Vec<u8>) -> Result<(), &'static str> {
    if entries.is_empty() || entries.len() > MAX_PILOT_ENTRIES {
        return Err(INVALID);
    }
    out.push(entries.len() as u8);
    for e in entries {
        let c = &e.controls;
        let f = &c.flight;
        let flip = match f.trick_flip {
            0 => 0u16,
            1 => 1,
            2 => 2,
            _ => return Err(INVALID),
        };
        let flags: u16 = u16::from(c.use_pressed)
            | (u16::from(f.takeoff_or_land) << 1)
            | (u16::from(f.boost) << 2)
            | (u16::from(f.eject_held) << 3)
            | (u16::from(f.fire_primary) << 4)
            | (u16::from(f.fire_secondary) << 5)
            | (u16::from(f.lock_target) << 6)
            | (encode_ternary(c.cycle_weapon) << 7)
            | (encode_ternary(f.trick_roll) << 9)
            | (flip << 11)
            | (encode_ternary(c.terminal_cycle) << 13);
        out.extend(e.sequence.to_le_bytes());
        out.extend(flags.to_le_bytes());
        for axis in [
            encode_unit_i16(f.throttle),
            encode_unit_i16(f.roll),
            encode_unit_i16(f.stick_x),
            encode_unit_i16(f.stick_y),
            encode_delta_i16(c.mouse[0]),
            encode_delta_i16(c.mouse[1]),
            encode_delta_i16(c.turret_look[0]),
            encode_delta_i16(c.turret_look[1]),
            encode_unit_i16(c.turret_turn),
        ] {
            out.extend(axis.to_le_bytes());
        }
        out.extend(0u16.to_le_bytes());
    }
    Ok(())
}

pub fn read_pilot_entries_into(
    body: &[u8],
    out: &mut [PilotEntry; MAX_PILOT_ENTRIES],
) -> Result<usize, &'static str> {
    let mut r = Reader(body);
    let count = usize::from(r.u8()?);
    if !(1..=MAX_PILOT_ENTRIES).contains(&count) || body.len() != 1 + count * PILOT_ENTRY_BYTES {
        return Err(INVALID);
    }
    for slot in out.iter_mut().take(count) {
        let sequence = r.u32()?;
        let flags = r.u16()?;
        if flags & 0x8000 != 0 {
            return Err(INVALID);
        }
        let cycle_weapon = decode_ternary(flags >> 7)?;
        let trick_roll = decode_ternary(flags >> 9)?;
        let trick_flip = match (flags >> 11) & 3 {
            0 => 0,
            1 => 1,
            2 => 2,
            _ => return Err(INVALID),
        };
        let terminal_cycle = decode_ternary(flags >> 13)?;
        let throttle = decode_unit_i16(r.i16()?);
        let roll = decode_unit_i16(r.i16()?);
        let stick_x = decode_unit_i16(r.i16()?);
        let stick_y = decode_unit_i16(r.i16()?);
        let mouse = [decode_delta_i16(r.i16()?), decode_delta_i16(r.i16()?)];
        let turret_look = [decode_delta_i16(r.i16()?), decode_delta_i16(r.i16()?)];
        let turret_turn = decode_unit_i16(r.i16()?);
        let reserved = r.u16()?;
        if reserved != 0 {
            return Err(INVALID);
        }
        *slot = PilotEntry {
            sequence,
            controls: PilotControls {
                use_pressed: flags & 1 != 0,
                cycle_weapon,
                terminal_cycle,
                turret_look,
                turret_turn,
                mouse,
                flight: SpacePilotInput {
                    throttle,
                    roll,
                    stick_x,
                    stick_y,
                    boost: flags & (1 << 2) != 0,
                    takeoff_or_land: flags & (1 << 1) != 0,
                    eject_held: flags & (1 << 3) != 0,
                    fire_primary: flags & (1 << 4) != 0,
                    fire_secondary: flags & (1 << 5) != 0,
                    lock_target: flags & (1 << 6) != 0,
                    trick_roll,
                    trick_flip,
                    ..Default::default()
                },
            },
        };
    }
    r.end()?;
    Ok(count)
}

pub fn read_pilot_entries(body: &[u8]) -> Result<Vec<PilotEntry>, &'static str> {
    let mut scratch = [PilotEntry::default(); MAX_PILOT_ENTRIES];
    let count = read_pilot_entries_into(body, &mut scratch)?;
    Ok(scratch[..count].to_vec())
}

// ---- Kind 17 (0x0211 FIGHTER): shared fighter chunks & seat occupancy ----

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FighterSnap {
    pub slot_in_chunk: u8,
    pub generation: u16,
    pub profile: u8,
    pub team: u8,
    pub state: u8,
    pub thrusting: bool,
    pub boosting: bool,
    pub mobile_spawn: bool,
    pub trick: u8,
    pub pos: [u32; 3],
    pub vel: [i16; 3],
    pub quat: [u8; 6],
    pub health: u16,
    pub bank: i8,
    pub gear: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FighterChunk {
    pub chunk: u8,
    pub alive_mask: u8,
    pub fighters: Vec<FighterSnap>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SeatEntry {
    pub unit_slot: u16,
    pub is_chair: bool,
    pub seat: u8,
    pub mount_index: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FighterFieldKind {
    Chunk { chunk: u8, alive_mask: u8, count: usize },
    Seats(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FighterField {
    Chunk(FighterChunk),
    Seats(Vec<SeatEntry>),
}

pub fn write_fighter_chunk(
    chunk: u8,
    fighters: &[SpaceFighter],
    bounds: &PositionBounds,
    out: &mut Vec<u8>,
) -> Result<(), &'static str> {
    if usize::from(chunk) >= MAX_FIGHTER_CHUNKS {
        return Err(INVALID);
    }
    let base = usize::from(chunk) * FIGHTERS_PER_CHUNK;
    let slice = fighters
        .get(base..)
        .map(|s| &s[..s.len().min(FIGHTERS_PER_CHUNK)])
        .unwrap_or(&[]);
    let mut alive_mask = 0u8;
    for (b, f) in slice.iter().enumerate() {
        if f.alive {
            alive_mask |= 1 << b;
        }
    }
    out.extend([0u8, chunk, alive_mask]);
    for (b, f) in slice.iter().enumerate() {
        if alive_mask & (1 << b) == 0 {
            continue;
        }
        if f.profile_index >= MAX_SPACE_PROFILES {
            return Err(INVALID);
        }
        let gen = (f.generation & 0xffff) as u16;
        let team = f.team.clamp(0, 7) as u8;
        let profile_team = (f.profile_index as u8 & 0x1f) | (team << 5);
        let thrusting = f.state == SpaceFlightState::TakingOff
            || (f.state == SpaceFlightState::Flying && (f.airspeed > 5.0 || f.boosting));
        let flags = (f.state as u8 & 3)
            | (u8::from(thrusting) << 2)
            | (u8::from(f.boosting) << 3)
            | (u8::from(f.mobile_spawn_active) << 4)
            | ((f.trick as u8 & 7) << 5);
        let pos = encode_pos_u24(bounds, f.position);
        let vel = encode_vel(f.velocity, 32.0);
        let quat = pack_quat(f.quaternion);
        let health = js_max(0.0, js_min(65535.0, js_round(f.health))) as u16;
        let bank = js_max(-127.0, js_min(127.0, js_round(f.bank / (PI * 0.5) * 127.0))) as i8;
        let gear = js_max(0.0, js_min(255.0, js_round(f.gear * 255.0))) as u8;
        out.extend(gen.to_le_bytes());
        out.extend([profile_team, flags]);
        for c in pos {
            push_u24(c, out);
        }
        for v in vel {
            out.extend(v.to_le_bytes());
        }
        out.extend(quat);
        out.extend(health.to_le_bytes());
        out.extend([bank as u8, gear]);
    }
    Ok(())
}

pub fn write_seats(seats: &[SeatEntry], out: &mut Vec<u8>) -> Result<(), &'static str> {
    if seats.len() > MAX_SEATS {
        return Err(INVALID);
    }
    out.extend([1u8, seats.len() as u8]);
    for s in seats {
        let kind_seat = (u8::from(s.is_chair) << 7) | (s.seat & 0x7f);
        out.extend(s.unit_slot.to_le_bytes());
        out.extend([kind_seat, s.mount_index]);
    }
    Ok(())
}

/// Reads and validates a `0x0211` fighter chunk or seat-map field into fixed
/// caller-owned buffers with zero heap allocations.
pub fn read_fighter_field_into(
    id: u32,
    body: &[u8],
    chunk_out: &mut [FighterSnap; FIGHTERS_PER_CHUNK],
    seats_out: &mut [SeatEntry; MAX_SEATS],
) -> Result<FighterFieldKind, &'static str> {
    let mut r = Reader(body);
    match r.u8()? {
        0 => {
            let chunk = r.u8()?;
            let alive_mask = r.u8()?;
            if usize::from(chunk) >= MAX_FIGHTER_CHUNKS || id != u32::from(chunk) {
                return Err(INVALID);
            }
            let count = alive_mask.count_ones() as usize;
            if body.len() != 3 + count * FIGHTER_SNAP_BYTES {
                return Err(INVALID);
            }
            let mut written = 0usize;
            for b in 0..8u8 {
                if alive_mask & (1 << b) == 0 {
                    continue;
                }
                let generation = r.u16()?;
                let profile_team = r.u8()?;
                let flags = r.u8()?;
                let profile = profile_team & 0x1f;
                let team = profile_team >> 5;
                let state = flags & 3;
                let trick = flags >> 5;
                if usize::from(profile) >= MAX_SPACE_PROFILES || team > 7 || trick > 4 {
                    return Err(INVALID);
                }
                let pos = r.pos_codes()?;
                let vel = r.vel_i16()?;
                let quat: [u8; 6] = r.take(6)?.try_into().map_err(|_| INVALID)?;
                let _ = unpack_quat(quat)?;
                let health = r.u16()?;
                let bank = r.i8()?;
                let gear = r.u8()?;
                chunk_out[written] = FighterSnap {
                    slot_in_chunk: b,
                    generation,
                    profile,
                    team,
                    state,
                    thrusting: flags & (1 << 2) != 0,
                    boosting: flags & (1 << 3) != 0,
                    mobile_spawn: flags & (1 << 4) != 0,
                    trick,
                    pos,
                    vel,
                    quat,
                    health,
                    bank,
                    gear,
                };
                written += 1;
            }
            r.end()?;
            Ok(FighterFieldKind::Chunk {
                chunk,
                alive_mask,
                count: written,
            })
        }
        1 => {
            if id != SEATS_FIELD_ID {
                return Err(INVALID);
            }
            let count = usize::from(r.u8()?);
            if count > MAX_SEATS || body.len() != 2 + count * 4 {
                return Err(INVALID);
            }
            for i in 0..count {
                let unit_slot = r.u16()?;
                let kind_seat = r.u8()?;
                let mount_index = r.u8()?;
                let is_chair = kind_seat & 0x80 != 0;
                let seat = kind_seat & 0x7f;
                if usize::from(unit_slot) >= super::records::SLOT_COUNT {
                    return Err(INVALID);
                }
                if is_chair {
                    if seat != 0 || usize::from(mount_index) >= MAX_TURRETS {
                        return Err(INVALID);
                    }
                } else if seat >= 6 || usize::from(mount_index) >= MAX_FIGHTERS {
                    return Err(INVALID);
                }
                if seats_out[..i].iter().any(|e| {
                    e.unit_slot == unit_slot
                        || (e.is_chair == is_chair
                            && e.mount_index == mount_index
                            && e.seat == seat)
                }) {
                    return Err(INVALID);
                }
                seats_out[i] = SeatEntry {
                    unit_slot,
                    is_chair,
                    seat,
                    mount_index,
                };
            }
            r.end()?;
            Ok(FighterFieldKind::Seats(count))
        }
        _ => Err(INVALID),
    }
}

/// Validates a `0x0211` fighter chunk or seat-map field with zero heap allocations.
pub fn validate_fighter_field(id: u32, body: &[u8]) -> Result<(), &'static str> {
    let mut chunk_buf = [FighterSnap::default(); FIGHTERS_PER_CHUNK];
    let mut seats_buf = [SeatEntry::default(); MAX_SEATS];
    read_fighter_field_into(id, body, &mut chunk_buf, &mut seats_buf).map(drop)
}

pub fn read_fighter_field(id: u32, body: &[u8]) -> Result<FighterField, &'static str> {
    let mut chunk_buf = [FighterSnap::default(); FIGHTERS_PER_CHUNK];
    let mut seats_buf = [SeatEntry::default(); MAX_SEATS];
    match read_fighter_field_into(id, body, &mut chunk_buf, &mut seats_buf)? {
        FighterFieldKind::Chunk { chunk, alive_mask, count } => Ok(FighterField::Chunk(FighterChunk {
            chunk,
            alive_mask,
            fighters: chunk_buf[..count].to_vec(),
        })),
        FighterFieldKind::Seats(count) => Ok(FighterField::Seats(seats_buf[..count].to_vec())),
    }
}

// ---- Kind 17 Own Body (0x91 inside schema 11 BNO1) ----

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OwnWeaponState {
    pub ammo_in_clip: u16,
    pub heat: f32,
    pub cooldown: f32,
    pub reload_timer: f32,
    pub overheated: bool,
    pub trigger_was_held: bool,
    pub shots_fired: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OwnLockState {
    pub kind: u8,
    pub weapon_idx: u8,
    pub target_idx: u16,
    pub target_gen: u32,
    pub progress: f32,
    pub locked: bool,
    pub lost_los: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OwnPilotState {
    pub is_gunner: bool,
    pub fighter_id: u16,
    pub fighter_gen: u32,
    pub seat: u8,
    pub profile: u8,
    pub state: u8,
    pub trick: u8,
    pub boosting: bool,
    pub boost_exhausted: bool,
    pub action_held: bool,
    pub oob_warning: bool,
    pub can_land: bool,
    pub mobile_spawn: bool,
    pub boarding_use: bool,
    pub selected_weapon: Option<u8>,
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub quaternion: [f64; 4],
    pub airspeed: f64,
    pub yaw_rate: f64,
    pub pitch_rate: f64,
    pub roll_rate: f64,
    pub bank: f64,
    pub gear: f64,
    pub state_time: f64,
    pub energy: f64,
    pub health: f64,
    pub trick_progress: f64,
    pub trick_cooldown: f64,
    pub eject_timer: f64,
    pub oob_timer: f64,
    pub stick: [f64; 2],
    /// The stick shaping the craft carries between ticks: its roll state,
    /// the levelling timer and the yaw and pitch buildups.
    pub roll_state: f64,
    pub level_timer: f64,
    pub yaw_buildup: f64,
    pub pitch_buildup: f64,
    /// The pilot's double taps, so a replay pairs the same taps as the authority.
    pub taps: DoubleTapDetector,
    pub trick_axis_right: [f64; 3],
    pub gunner_angles: [f64; 2],
    pub weapon_count: u8,
    pub weapons: [OwnWeaponState; 5],
    pub lock: Option<OwnLockState>,
}

impl OwnPilotState {
    #[inline]
    pub fn weapons(&self) -> &[OwnWeaponState] {
        &self.weapons[..usize::from(self.weapon_count).min(5)]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OwnChairState {
    pub chair_building: u16,
    pub turret_building: u16,
    pub aim: [f64; 2],
    pub feet: [f64; 3],
    pub turret_yaw: f64,
    pub turret_pitch: f64,
    pub cooldown: f32,
    pub heat: f32,
    pub overheated: bool,
    pub shots_fired: u16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum OwnSpaceBody {
    Pilot(OwnPilotState),
    Chair(OwnChairState),
}

pub fn write_own_pilot(
    f: &SpaceFighter,
    profile: &SpaceFlightProfile,
    seat: u8,
    selected_weapon: Option<u8>,
    stick: [f64; 2],
    boarding_use: bool,
    gunner_angles: [f64; 2],
    can_land: bool,
    taps: &DoubleTapDetector,
    out: &mut Vec<u8>,
) -> Result<(), &'static str> {
    if f.id >= MAX_FIGHTERS || f.profile_index >= MAX_SPACE_PROFILES || seat >= 6 {
        return Err(INVALID);
    }
    let mode = u8::from(seat != 0);
    out.extend(REVISION.to_le_bytes());
    out.push(mode);
    out.extend((f.id as u16).to_le_bytes());
    out.extend(f.generation.to_le_bytes());
    let flags = u8::from(f.boosting)
        | (u8::from(f.boost_exhausted) << 1)
        | (u8::from(f.action_held) << 2)
        | (u8::from(f.oob_warning) << 3)
        | (u8::from(can_land) << 4)
        | (u8::from(f.mobile_spawn_active) << 5)
        | (u8::from(boarding_use) << 6);
    out.extend([
        seat,
        f.profile_index as u8,
        f.state as u8,
        f.trick as u8,
        flags,
        selected_weapon.filter(|&w| w < 5).unwrap_or(255),
    ]);
    let nums = [
        f.position[0],
        f.position[1],
        f.position[2],
        f.velocity[0],
        f.velocity[1],
        f.velocity[2],
        f.quaternion[0],
        f.quaternion[1],
        f.quaternion[2],
        f.quaternion[3],
        f.airspeed.max(0.0),
        f.yaw_rate,
        f.pitch_rate,
        f.roll_rate,
        f.bank,
        f.gear.clamp(0.0, 1.0),
        f.state_time,
        f.energy.max(0.0),
        f.health.max(0.0),
        f.trick_progress,
        f.trick_cooldown,
        f.eject_timer,
        f.oob_timer,
        stick[0],
        stick[1],
        f.roll_state,
        f.level_timer,
        f.yaw_buildup,
        f.pitch_buildup,
    ];
    for n in nums {
        if !n.is_finite() {
            return Err(INVALID);
        }
        out.extend(n.to_le_bytes());
    }
    write_taps(&taps.carried(), out)?;
    if f.trick != SpaceTrick::None {
        for n in f.trick_axis_right {
            if !n.is_finite() {
                return Err(INVALID);
            }
            out.extend(n.to_le_bytes());
        }
    }
    if mode == 1 {
        for n in gunner_angles {
            if !n.is_finite() {
                return Err(INVALID);
            }
            out.extend(n.to_le_bytes());
        }
    }
    let wcount = profile.weapons.len().min(5);
    out.push(wcount as u8);
    for rt in &f.weapons[..wcount] {
        let ammo = rt.ammo_in_clip.min(u32::from(u16::MAX)) as u16;
        let heat = rt.heat as f32;
        let cooldown = rt.clock.cooldown as f32;
        let reload = rt.reload_timer as f32;
        if !heat.is_finite() || !cooldown.is_finite() || !reload.is_finite() {
            return Err(INVALID);
        }
        let wflags = u8::from(rt.overheated) | (u8::from(rt.trigger_was_held) << 1);
        out.extend(ammo.to_le_bytes());
        out.extend(heat.to_le_bytes());
        out.extend(cooldown.to_le_bytes());
        out.extend(reload.to_le_bytes());
        out.push(wflags);
        out.extend((rt.shots_fired as u16).to_le_bytes());
    }
    let active_lock = f.selected_lock(&profile.weapons, seat, selected_weapon);
    write_own_lock(&active_lock, out)
}

/// A pilot's double taps: the detector's clock, one byte of directions (bits
/// 0-1 the roll held, 2-3 the throttle held, 4-5 a roll tap that can still
/// pair, 6-7 a throttle tap; each 0 none, 1 positive, 2 negative), then the
/// time of each such tap.
fn write_taps(taps: &CarriedTaps, out: &mut Vec<u8>) -> Result<(), &'static str> {
    if !taps.valid() {
        return Err(INVALID);
    }
    let code = |dir: i8| match dir { 1 => 1u8, -1 => 2, _ => 0 };
    let tap = |t: Option<(i8, f64)>| t.map_or(0, |(dir, _)| code(dir));
    out.extend(taps.clock.to_le_bytes());
    out.push(code(taps.held[0]) | code(taps.held[1]) << 2 | tap(taps.roll) << 4 | tap(taps.throttle) << 6);
    for (_, at) in [taps.roll, taps.throttle].into_iter().flatten() {
        out.extend(at.to_le_bytes());
    }
    Ok(())
}

fn read_taps(r: &mut Reader<'_>) -> Result<DoubleTapDetector, &'static str> {
    let clock = r.f64()?;
    let byte = r.u8()?;
    let dir = |code: u8| match code & 3 { 0 => Ok(0i8), 1 => Ok(1), 2 => Ok(-1), _ => Err(INVALID) };
    let held = [dir(byte)?, dir(byte >> 2)?];
    let mut tap = |code: u8| -> Result<Option<(i8, f64)>, &'static str> {
        match dir(code)? { 0 => Ok(None), d => Ok(Some((d, r.f64()?))) }
    };
    let roll = tap(byte >> 4)?;
    let throttle = tap(byte >> 6)?;
    let carried = CarriedTaps { clock, held, roll, throttle };
    if !carried.valid() {
        return Err(INVALID);
    }
    Ok(DoubleTapDetector::resumed(carried))
}

fn write_own_lock(lock: &SpaceLockState, out: &mut Vec<u8>) -> Result<(), &'static str> {
    let kind = lock.kind as u8;
    out.push(kind);
    if kind != 0 {
        let progress = lock.progress as f32;
        let lost_los = lock.lost_los_time as f32;
        if !progress.is_finite() || !lost_los.is_finite() {
            return Err(INVALID);
        }
        out.push(lock.weapon_idx_safe());
        out.extend((lock.target_index.min(usize::from(u16::MAX)) as u16).to_le_bytes());
        out.extend((lock.target_generation as u32).to_le_bytes());
        out.extend(progress.to_le_bytes());
        out.push(u8::from(lock.locked));
        out.extend(lost_los.to_le_bytes());
    }
    Ok(())
}

trait LockIdxExt {
    fn weapon_idx_safe(&self) -> u8;
}
impl LockIdxExt for SpaceLockState {
    fn weapon_idx_safe(&self) -> u8 {
        self.weapon_index.min(4)
    }
}

pub fn write_own_chair(chair: &OwnChairState, out: &mut Vec<u8>) -> Result<(), &'static str> {
    if chair.chair_building == u16::MAX || chair.turret_building == u16::MAX {
        return Err(INVALID);
    }
    out.extend(REVISION.to_le_bytes());
    out.push(2u8);
    out.extend(chair.chair_building.to_le_bytes());
    out.extend(chair.turret_building.to_le_bytes());
    for n in [
        chair.aim[0],
        chair.aim[1],
        chair.feet[0],
        chair.feet[1],
        chair.feet[2],
        chair.turret_yaw,
        chair.turret_pitch,
    ] {
        if !n.is_finite() {
            return Err(INVALID);
        }
        out.extend(n.to_le_bytes());
    }
    if !chair.cooldown.is_finite() || !chair.heat.is_finite() {
        return Err(INVALID);
    }
    out.extend(chair.cooldown.to_le_bytes());
    out.extend(chair.heat.to_le_bytes());
    out.push(u8::from(chair.overheated));
    out.extend(chair.shots_fired.to_le_bytes());
    Ok(())
}

/// Restores a [`SpaceFighter`] from an authoritative `0x91` [`OwnPilotState`]
/// snapshot for client own-craft prediction replay.
pub fn restore_fighter_from_own(
    own: &OwnPilotState,
    profile: &SpaceFlightProfile,
    pad: &crate::space::flight::SpaceVehiclePad,
    team: i32,
) -> SpaceFighter {
    let mut f = SpaceFighter::spawned_on_pad(
        usize::from(own.fighter_id),
        own.fighter_gen,
        0,
        pad,
        usize::from(own.profile),
        profile,
        pad.floor_y,
        None,
        None,
    );
    f.team = team;
    f.alive = true;
    f.state = match own.state {
        0 => SpaceFlightState::Landed,
        1 => SpaceFlightState::TakingOff,
        2 => SpaceFlightState::Flying,
        _ => SpaceFlightState::Landing,
    };
    f.trick = match own.trick {
        1 => SpaceTrick::BarrelLeft,
        2 => SpaceTrick::BarrelRight,
        3 => SpaceTrick::Loop,
        4 => SpaceTrick::Immelmann,
        _ => SpaceTrick::None,
    };
    f.position = own.position;
    f.previous_position = own.position;
    f.velocity = own.velocity;
    f.quaternion = own.quaternion;
    f.previous_quaternion = own.quaternion;
    f.airspeed = own.airspeed;
    f.yaw_rate = own.yaw_rate;
    f.pitch_rate = own.pitch_rate;
    f.roll_rate = own.roll_rate;
    f.bank = own.bank;
    f.prev_bank = own.bank;
    f.roll_state = own.roll_state;
    f.level_timer = own.level_timer;
    f.yaw_buildup = own.yaw_buildup;
    f.pitch_buildup = own.pitch_buildup;
    f.gear = own.gear;
    f.prev_gear = own.gear;
    f.state_time = own.state_time;
    f.energy = own.energy;
    f.health = own.health;
    f.max_health = profile.max_health;
    f.boosting = own.boosting;
    f.boost_exhausted = own.boost_exhausted;
    f.action_held = own.action_held;
    f.oob_warning = own.oob_warning;
    f.mobile_spawn_active = own.mobile_spawn;
    f.trick_progress = own.trick_progress;
    f.trick_cooldown = own.trick_cooldown;
    f.trick_axis_right = own.trick_axis_right;
    f.eject_timer = own.eject_timer;
    f.oob_timer = own.oob_timer;
    f.pilot = Some(crate::space::flight::SpacePilotId {
        owner: crate::store::Ref::new(0, 1),
        team,
        is_human: true,
        repair_capable: false,
    });
    for (i, w) in own.weapons().iter().enumerate() {
        f.weapons[i].ammo_in_clip = u32::from(w.ammo_in_clip);
        f.weapons[i].heat = f64::from(w.heat);
        f.weapons[i].clock.cooldown = f64::from(w.cooldown);
        f.weapons[i].reload_timer = f64::from(w.reload_timer);
        f.weapons[i].overheated = w.overheated;
        f.weapons[i].trigger_was_held = w.trigger_was_held;
        f.weapons[i].shots_fired = u32::from(w.shots_fired);
    }
    f.recompute_matrix();
    f
}

/// Reads an own Space body (`0x91`), after its 2-byte revision has been split off.
pub fn read_own_body(body: &[u8]) -> Result<OwnSpaceBody, &'static str> {
    let mut r = Reader(body);
    let mode = r.u8()?;
    match mode {
        0 | 1 => {
            let fighter_id = r.u16()?;
            let fighter_gen = r.u32()?;
            let seat = r.u8()?;
            let profile = r.u8()?;
            let state = r.u8()?;
            let trick = r.u8()?;
            let flags = r.u8()?;
            let selected_raw = r.u8()?;
            if usize::from(fighter_id) >= MAX_FIGHTERS
                || seat >= 6
                || (mode == 0 && seat != 0)
                || (mode == 1 && seat == 0)
                || usize::from(profile) >= MAX_SPACE_PROFILES
                || state > 3
                || trick > 4
                || flags & 0x80 != 0
                || (selected_raw != 255 && selected_raw >= 5)
            {
                return Err(INVALID);
            }
            let selected_weapon = (selected_raw != 255).then_some(selected_raw);
            let position = [r.f64()?, r.f64()?, r.f64()?];
            let velocity = [r.f64()?, r.f64()?, r.f64()?];
            let quaternion = [r.f64()?, r.f64()?, r.f64()?, r.f64()?];
            let qnorm = v8math::hypot_n(&quaternion);
            if !qnorm.is_finite() || (qnorm - 1.0).abs() > 0.01 {
                return Err(INVALID);
            }
            let airspeed = r.f64()?;
            let yaw_rate = r.f64()?;
            let pitch_rate = r.f64()?;
            let roll_rate = r.f64()?;
            let bank = r.f64()?;
            let gear = r.f64()?;
            let state_time = r.f64()?;
            let energy = r.f64()?;
            let health = r.f64()?;
            let trick_progress = r.f64()?;
            let trick_cooldown = r.f64()?;
            let eject_timer = r.f64()?;
            let oob_timer = r.f64()?;
            let stick = [r.f64()?, r.f64()?];
            let (roll_state, level_timer, yaw_buildup, pitch_buildup) = (r.f64()?, r.f64()?, r.f64()?, r.f64()?);
            let taps = read_taps(&mut r)?;
            if airspeed < 0.0 || !(0.0..=1.0).contains(&gear) || energy < 0.0 || health < 0.0 {
                return Err(INVALID);
            }
            let trick_axis_right = if trick != 0 {
                [r.f64()?, r.f64()?, r.f64()?]
            } else {
                [1.0, 0.0, 0.0]
            };
            let gunner_angles = if mode == 1 {
                [r.f64()?, r.f64()?]
            } else {
                [0.0, 0.0]
            };
            let weapon_count = usize::from(r.u8()?);
            if weapon_count > 5 {
                return Err(INVALID);
            }
            let mut weapons = [OwnWeaponState::default(); 5];
            for slot in weapons.iter_mut().take(weapon_count) {
                let ammo_in_clip = r.u16()?;
                let heat = r.f32()?;
                let cooldown = r.f32()?;
                let reload_timer = r.f32()?;
                let wflags = r.u8()?;
                let shots_fired = r.u16()?;
                if wflags >> 2 != 0 {
                    return Err(INVALID);
                }
                *slot = OwnWeaponState {
                    ammo_in_clip,
                    heat,
                    cooldown,
                    reload_timer,
                    overheated: wflags & 1 != 0,
                    trigger_was_held: wflags & 2 != 0,
                    shots_fired,
                };
            }
            let lock_kind = r.u8()?;
            if lock_kind > 2 {
                return Err(INVALID);
            }
            let lock = if lock_kind != 0 {
                let weapon_idx = r.u8()?;
                let target_idx = r.u16()?;
                let target_gen = r.u32()?;
                let progress = r.f32()?;
                let locked = r.u8()?;
                let lost_los = r.f32()?;
                if weapon_idx >= 5 || locked > 1 {
                    return Err(INVALID);
                }
                Some(OwnLockState {
                    kind: lock_kind,
                    weapon_idx,
                    target_idx,
                    target_gen,
                    progress,
                    locked: locked == 1,
                    lost_los,
                })
            } else {
                None
            };
            r.end()?;
            Ok(OwnSpaceBody::Pilot(OwnPilotState {
                is_gunner: mode == 1,
                fighter_id,
                fighter_gen,
                seat,
                profile,
                state,
                trick,
                boosting: flags & 1 != 0,
                boost_exhausted: flags & (1 << 1) != 0,
                action_held: flags & (1 << 2) != 0,
                oob_warning: flags & (1 << 3) != 0,
                can_land: flags & (1 << 4) != 0,
                mobile_spawn: flags & (1 << 5) != 0,
                boarding_use: flags & (1 << 6) != 0,
                selected_weapon,
                position,
                velocity,
                quaternion,
                airspeed,
                yaw_rate,
                pitch_rate,
                roll_rate,
                bank,
                gear,
                state_time,
                energy,
                health,
                trick_progress,
                trick_cooldown,
                eject_timer,
                oob_timer,
                stick,
                roll_state,
                level_timer,
                yaw_buildup,
                pitch_buildup,
                taps,
                trick_axis_right,
                gunner_angles,
                weapon_count: weapon_count as u8,
                weapons,
                lock,
            }))
        }
        2 => {
            let chair_building = r.u16()?;
            let turret_building = r.u16()?;
            if chair_building == u16::MAX || turret_building == u16::MAX {
                return Err(INVALID);
            }
            let aim = [r.f64()?, r.f64()?];
            let feet = [r.f64()?, r.f64()?, r.f64()?];
            let turret_yaw = r.f64()?;
            let turret_pitch = r.f64()?;
            let cooldown = r.f32()?;
            let heat = r.f32()?;
            let overheated = r.u8()?;
            let shots_fired = r.u16()?;
            if overheated > 1 {
                return Err(INVALID);
            }
            r.end()?;
            Ok(OwnSpaceBody::Chair(OwnChairState {
                chair_building,
                turret_building,
                aim,
                feet,
                turret_yaw,
                turret_pitch,
                cooldown,
                heat,
                overheated: overheated == 1,
                shots_fired,
            }))
        }
        _ => Err(INVALID),
    }
}

// ---- Kind 19 (0x0213 FRIGATES): world animation group clocks ----

pub fn write_frigates(clocks: &[(u8, u32)], out: &mut Vec<u8>) -> Result<(), &'static str> {
    if clocks.len() > MAX_ANIM_GROUPS {
        return Err(INVALID);
    }
    out.push(clocks.len() as u8);
    for &(state, origin) in clocks {
        if state > 2 || (state == 0 && origin != 0) {
            return Err(INVALID);
        }
        out.push(state);
        out.extend(origin.to_le_bytes());
    }
    Ok(())
}

fn parse_frigates(
    id: u32,
    body: &[u8],
    mut emit: impl FnMut((u8, u32)),
) -> Result<(), &'static str> {
    if id != 0 {
        return Err(INVALID);
    }
    let mut r = Reader(body);
    let count = usize::from(r.u8()?);
    if count > MAX_ANIM_GROUPS || body.len() != 1 + count * 5 {
        return Err(INVALID);
    }
    for _ in 0..count {
        let state = r.u8()?;
        let origin = r.u32()?;
        if state > 2 || (state == 0 && origin != 0) {
            return Err(INVALID);
        }
        emit((state, origin));
    }
    r.end()
}

pub fn validate_frigates(id: u32, body: &[u8]) -> Result<(), &'static str> {
    parse_frigates(id, body, |_| {})
}

pub fn read_frigates_into(
    id: u32,
    body: &[u8],
    out: &mut Vec<(u8, u32)>,
) -> Result<(), &'static str> {
    out.clear();
    parse_frigates(id, body, |entry| out.push(entry))
}

pub fn read_frigates(id: u32, body: &[u8]) -> Result<Vec<(u8, u32)>, &'static str> {
    let mut out = Vec::with_capacity(MAX_ANIM_GROUPS);
    read_frigates_into(id, body, &mut out)?;
    Ok(out)
}

// ---- Kind 20 (0x0214 ORDNANCE): live guided rounds + launches/impacts/deaths ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuidedSnap {
    pub slot: u8,
    pub serial: u16,
    pub weapon_idx: u8,
    pub pos: [u32; 3],
    pub vel: [i16; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaunchSnap {
    pub serial: u16,
    pub weapon_idx: u8,
    pub tick_offset: u8,
    pub is_beam: bool,
    pub team: u8,
    pub origin: [u32; 3],
    pub vel: [i16; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImpactSnap {
    pub serial: u16,
    pub weapon_idx: u8,
    pub tick_offset: u8,
    pub shield: bool,
    pub rigid: bool,
    pub blast: bool,
    pub pos: [u32; 3],
    pub oct_normal: Option<[i8; 2]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeathSnap {
    pub class_idx: u16,
    pub is_building: bool,
    pub pos: [u32; 3],
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventsSnap {
    pub launches: Vec<LaunchSnap>,
    pub impacts: Vec<ImpactSnap>,
    pub deaths: Vec<DeathSnap>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrdnanceFieldKind {
    Guided,
    Events,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrdnanceField {
    Guided(Vec<GuidedSnap>),
    Events(EventsSnap),
}

pub fn write_guided(rounds: &[GuidedSnap], out: &mut Vec<u8>) -> Result<(), &'static str> {
    if rounds.len() > MAX_GUIDED {
        return Err(INVALID);
    }
    out.extend([0u8, rounds.len() as u8]);
    for g in rounds {
        out.push(g.slot & 63);
        out.extend(g.serial.to_le_bytes());
        out.push(g.weapon_idx & 0x7f);
        for c in g.pos {
            push_u24(c, out);
        }
        for v in g.vel {
            out.extend(v.to_le_bytes());
        }
    }
    Ok(())
}

fn parse_ordnance_field(
    id: u32,
    body: &[u8],
    mut on_guided: impl FnMut(GuidedSnap),
    mut on_launch: impl FnMut(LaunchSnap),
    mut on_impact: impl FnMut(ImpactSnap),
    mut on_death: impl FnMut(DeathSnap),
) -> Result<OrdnanceFieldKind, &'static str> {
    let mut r = Reader(body);
    match r.u8()? {
        0 => {
            if id != 0 {
                return Err(INVALID);
            }
            let count = usize::from(r.u8()?);
            if count > MAX_GUIDED || body.len() != 2 + count * 19 {
                return Err(INVALID);
            }
            for _ in 0..count {
                let slot = r.u8()?;
                let serial = r.u16()?;
                let weapon_idx = r.u8()?;
                if slot >= 64 || usize::from(weapon_idx) >= MAX_SPACE_WEAPONS {
                    return Err(INVALID);
                }
                let pos = r.pos_codes()?;
                let vel = r.vel_i16()?;
                on_guided(GuidedSnap {
                    slot,
                    serial,
                    weapon_idx,
                    pos,
                    vel,
                });
            }
            r.end()?;
            Ok(OrdnanceFieldKind::Guided)
        }
        1 => {
            if id != 1 || body.len() + 2 > 900 {
                return Err(INVALID);
            }
            let launch_count = usize::from(r.u8()?);
            let impact_count = usize::from(r.u8()?);
            let death_count = usize::from(r.u8()?);
            if launch_count > MAX_LAUNCHES
                || impact_count > MAX_IMPACTS
                || death_count > MAX_DEATHS
            {
                return Err(INVALID);
            }
            for _ in 0..launch_count {
                let serial = r.u16()?;
                let weapon_idx = r.u8()?;
                let flags = r.u8()?;
                let tick_offset = flags & 7;
                let team = (flags >> 4) & 7;
                if usize::from(weapon_idx) >= MAX_SPACE_WEAPONS
                    || tick_offset > 5
                    || flags & 0x80 != 0
                {
                    return Err(INVALID);
                }
                let origin = r.pos_codes()?;
                let vel = r.vel_i16()?;
                on_launch(LaunchSnap {
                    serial,
                    weapon_idx,
                    tick_offset,
                    is_beam: flags & (1 << 3) != 0,
                    team,
                    origin,
                    vel,
                });
            }
            for _ in 0..impact_count {
                let serial = r.u16()?;
                let weapon_idx = r.u8()?;
                let flags = r.u8()?;
                let tick_offset = flags & 7;
                if usize::from(weapon_idx) >= MAX_SPACE_WEAPONS
                    || tick_offset > 5
                    || flags & 0x80 != 0
                {
                    return Err(INVALID);
                }
                let pos = r.pos_codes()?;
                let oct_normal = if flags & (1 << 5) != 0 {
                    Some([r.i8()?, r.i8()?])
                } else {
                    None
                };
                on_impact(ImpactSnap {
                    serial,
                    weapon_idx,
                    tick_offset,
                    shield: flags & (1 << 3) != 0,
                    rigid: flags & (1 << 4) != 0,
                    blast: flags & (1 << 6) != 0,
                    pos,
                    oct_normal,
                });
            }
            for _ in 0..death_count {
                let class_idx = r.u16()?;
                let flags = r.u8()?;
                if flags & !1 != 0 {
                    return Err(INVALID);
                }
                let pos = r.pos_codes()?;
                on_death(DeathSnap {
                    class_idx,
                    is_building: flags & 1 != 0,
                    pos,
                });
            }
            r.end()?;
            Ok(OrdnanceFieldKind::Events)
        }
        _ => Err(INVALID),
    }
}

pub fn validate_ordnance_field(id: u32, body: &[u8]) -> Result<(), &'static str> {
    parse_ordnance_field(id, body, |_| {}, |_| {}, |_| {}, |_| {}).map(drop)
}

pub fn read_ordnance_field_into(
    id: u32,
    body: &[u8],
    guided_out: &mut Vec<GuidedSnap>,
    events_out: &mut EventsSnap,
) -> Result<OrdnanceFieldKind, &'static str> {
    if body.first() == Some(&0) {
        guided_out.clear();
    } else if body.first() == Some(&1) {
        events_out.launches.clear();
        events_out.impacts.clear();
        events_out.deaths.clear();
    }
    parse_ordnance_field(
        id,
        body,
        |g| guided_out.push(g),
        |l| events_out.launches.push(l),
        |i| events_out.impacts.push(i),
        |d| events_out.deaths.push(d),
    )
}

pub fn read_ordnance_field(id: u32, body: &[u8]) -> Result<OrdnanceField, &'static str> {
    let mut guided = Vec::new();
    let mut events = EventsSnap::default();
    match read_ordnance_field_into(id, body, &mut guided, &mut events)? {
        OrdnanceFieldKind::Guided => Ok(OrdnanceField::Guided(guided)),
        OrdnanceFieldKind::Events => Ok(OrdnanceField::Events(events)),
    }
}

// ---- Kind 21 (0x0215 SPACE_PROPS): turret aims & proximity doors ----

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DoorSnap {
    pub open: u8,
    pub locked: bool,
    pub opening: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropsFieldKind {
    Turrets,
    Doors,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PropsField {
    Turrets(Vec<[i16; 2]>),
    Doors(Vec<DoorSnap>),
}

#[inline]
pub fn encode_turret_angle(rad: f64) -> i16 {
    let wrapped = ((rad + PI).rem_euclid(2.0 * PI)) - PI;
    let x = if wrapped.is_finite() { wrapped } else { 0.0 };
    js_max(-32767.0, js_min(32767.0, js_round(x * 32767.0 / PI))) as i16
}

#[inline]
pub fn decode_turret_angle(code: i16) -> f64 {
    f64::from(code) * PI / 32767.0
}

fn parse_props_field(
    id: u32,
    body: &[u8],
    mut on_turret: impl FnMut([i16; 2]),
    mut on_door: impl FnMut(DoorSnap),
) -> Result<PropsFieldKind, &'static str> {
    let mut r = Reader(body);
    match r.u8()? {
        0 => {
            if id != 0 {
                return Err(INVALID);
            }
            let count = usize::from(r.u8()?);
            if count > MAX_TURRETS || body.len() != 2 + count * 4 {
                return Err(INVALID);
            }
            for _ in 0..count {
                on_turret([r.i16()?, r.i16()?]);
            }
            r.end()?;
            Ok(PropsFieldKind::Turrets)
        }
        1 => {
            if id != 1 {
                return Err(INVALID);
            }
            let count = usize::from(r.u8()?);
            if count > MAX_DOORS || body.len() != 2 + count * 2 {
                return Err(INVALID);
            }
            for _ in 0..count {
                let open = r.u8()?;
                let flags = r.u8()?;
                if flags & !0x03 != 0 {
                    return Err(INVALID);
                }
                on_door(DoorSnap {
                    open,
                    locked: flags & 1 != 0,
                    opening: flags & 2 != 0,
                });
            }
            r.end()?;
            Ok(PropsFieldKind::Doors)
        }
        _ => Err(INVALID),
    }
}

pub fn validate_props_field(id: u32, body: &[u8]) -> Result<(), &'static str> {
    parse_props_field(id, body, |_| {}, |_| {}).map(drop)
}

pub fn read_props_field_into(
    id: u32,
    body: &[u8],
    turrets_out: &mut Vec<[i16; 2]>,
    doors_out: &mut Vec<DoorSnap>,
) -> Result<PropsFieldKind, &'static str> {
    if body.first() == Some(&0) {
        turrets_out.clear();
    } else if body.first() == Some(&1) {
        doors_out.clear();
    }
    parse_props_field(id, body, |t| turrets_out.push(t), |d| doors_out.push(d))
}

pub fn read_props_field(id: u32, body: &[u8]) -> Result<PropsField, &'static str> {
    let mut turrets = Vec::new();
    let mut doors = Vec::new();
    match read_props_field_into(id, body, &mut turrets, &mut doors)? {
        PropsFieldKind::Turrets => Ok(PropsField::Turrets(turrets)),
        PropsFieldKind::Doors => Ok(PropsField::Doors(doors)),
    }
}

// ---- Server zero-allocation publication bank ----

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    })
}

/// Pre-allocated server-side Space field publisher (0 heap allocations per tick).
pub struct Bank {
    bounds: PositionBounds,
    weapon_hashes: Vec<u32>,
    turret_slots: Vec<usize>,
    chair_slots: Vec<usize>,
    offers: Vec<Offer>,
    used: usize,
    seats_scratch: Vec<SeatEntry>,
    guided_scratch: Vec<GuidedSnap>,
    clocks_scratch: Vec<(u8, u32)>,
    events_seq: u64,
}

impl Bank {
    /// Builds and pre-allocates the Space field bank once at match admission.
    pub fn new(space: &SpaceBattlefield, bounds: PositionBounds) -> Self {
        let weapon_hashes = weapon_hash_table(&space.profiles, &space.buildings);
        let turret_slots: Vec<usize> = space
            .buildings
            .iter()
            .enumerate()
            .filter(|(_, b)| b.turret.is_some())
            .map(|(i, _)| i)
            .take(MAX_TURRETS)
            .collect();
        let chair_slots: Vec<usize> = space
            .buildings
            .iter()
            .enumerate()
            .filter(|(_, b)| b.terminal.is_some())
            .map(|(i, _)| i)
            .take(MAX_TURRETS)
            .collect();
        let chunks = space.fighters.len().div_ceil(FIGHTERS_PER_CHUNK).min(MAX_FIGHTER_CHUNKS);
        let max_offers = chunks + 6;
        let mut offers = Vec::with_capacity(max_offers);
        for _ in 0..max_offers {
            offers.push(Offer {
                schema: 0,
                id: 0,
                state: 0,
                cadence: 3.0,
                payload: Vec::with_capacity(EVENTS_BYTES),
            });
        }
        Self {
            bounds,
            weapon_hashes,
            turret_slots,
            chair_slots,
            offers,
            used: 0,
            seats_scratch: Vec::with_capacity(MAX_SEATS),
            guided_scratch: Vec::with_capacity(MAX_GUIDED),
            clocks_scratch: Vec::with_capacity(MAX_ANIM_GROUPS),
            events_seq: 0,
        }
    }

    pub(crate) fn offers(&self) -> &[Offer] {
        &self.offers[..self.used]
    }

    pub fn offer_count(&self) -> usize {
        self.used
    }

    fn weapon_index(&self, hash: u32) -> u8 {
        self.weapon_hashes
            .binary_search(&hash)
            .map(|i| i as u8)
            .unwrap_or(0)
    }

    fn next_offer(&mut self, schema: u16, id: u32, cadence: f64) -> Option<&mut Offer> {
        let offer = self.offers.get_mut(self.used)?;
        self.used += 1;
        offer.schema = schema;
        offer.id = id;
        offer.cadence = cadence;
        offer.payload.clear();
        offer.payload.extend(REVISION.to_le_bytes());
        Some(offer)
    }

    /// Samples all Space world fields for publication at authority `tick`.
    /// Drains the Space wire observation ring with zero heap allocations.
    pub fn sample(&mut self, world: &mut World, tick: u32) {
        self.used = 0;
        let Some(space) = world.space.as_deref() else {
            return;
        };
        let bounds = self.bounds;

        // 1. Fighter chunks (`0x0211`, id = 0..chunks).
        let chunks = space
            .fighters
            .len()
            .div_ceil(FIGHTERS_PER_CHUNK)
            .min(MAX_FIGHTER_CHUNKS);
        for chunk in 0..chunks {
            if let Some(offer) = self.next_offer(FIGHTER_SCHEMA, chunk as u32, 3.0) {
                if write_fighter_chunk(chunk as u8, &space.fighters, &bounds, &mut offer.payload)
                    .is_ok()
                {
                    offer.state = fnv(&offer.payload);
                }
            }
        }

        // 2. Space seat occupancy map (`0x0211`, id = 256).
        self.seats_scratch.clear();
        for f in space.fighters.iter().take(MAX_FIGHTERS) {
            if !f.alive {
                continue;
            }
            for (seat_idx, occupant) in std::iter::once(f.pilot)
                .chain(f.gunners)
                .enumerate()
            {
                let Some(p) = occupant else { continue };
                if let Some(unit_ref) = crate::combat::owners::resolve(&world.sim.store, p.owner) {
                    if self.seats_scratch.len() < MAX_SEATS
                        && (unit_ref.slot as usize) < super::records::SLOT_COUNT
                    {
                        self.seats_scratch.push(SeatEntry {
                            unit_slot: unit_ref.slot as u16,
                            is_chair: false,
                            seat: seat_idx as u8,
                            mount_index: f.id as u8,
                        });
                    }
                }
            }
        }
        for (chair_idx, &b_slot) in self.chair_slots.iter().enumerate() {
            let Some(occ) = space
                .buildings
                .get(b_slot)
                .and_then(|b| b.terminal.as_ref())
                .and_then(|t| t.occupant)
            else {
                continue;
            };
            if let Some(unit_ref) = crate::combat::owners::resolve(&world.sim.store, occ.owner) {
                if self.seats_scratch.len() < MAX_SEATS
                    && (unit_ref.slot as usize) < super::records::SLOT_COUNT
                {
                    self.seats_scratch.push(SeatEntry {
                        unit_slot: unit_ref.slot as u16,
                        is_chair: true,
                        seat: 0,
                        mount_index: chair_idx as u8,
                    });
                }
            }
        }
        let seat_count = self.seats_scratch.len();
        if let Some(offer) = self.offers.get_mut(self.used) {
            self.used += 1;
            offer.schema = FIGHTER_SCHEMA;
            offer.id = SEATS_FIELD_ID;
            offer.cadence = 60.0;
            offer.payload.clear();
            offer.payload.extend(REVISION.to_le_bytes());
            let _ = write_seats(&self.seats_scratch[..seat_count], &mut offer.payload);
            offer.state = fnv(&offer.payload);
        }

        // 3. World animation clocks (`0x0213`, id = 0).
        self.clocks_scratch.clear();
        let group_count = space.world_anims.group_count().min(MAX_ANIM_GROUPS);
        for g in 0..group_count {
            if let Some(clock) = space.world_anims.group_clock(g, tick) {
                self.clocks_scratch.push(clock);
            }
        }
        let clock_count = self.clocks_scratch.len();
        if let Some(offer) = self.offers.get_mut(self.used) {
            self.used += 1;
            offer.schema = FRIGATES_SCHEMA;
            offer.id = 0;
            offer.cadence = 60.0;
            offer.payload.clear();
            offer.payload.extend(REVISION.to_le_bytes());
            let _ = write_frigates(&self.clocks_scratch[..clock_count], &mut offer.payload);
            offer.state = fnv(&offer.payload);
        }

        // 4. Live guided / curved ordnance (`0x0214`, id = 0).
        self.guided_scratch.clear();
        for (slot_idx, proj) in space.ordnance.slots.iter().enumerate() {
            if !proj.active {
                continue;
            }
            if !matches!(
                proj.kind,
                SpaceOrdnanceKind::Missile
                    | SpaceOrdnanceKind::BombShell
                    | SpaceOrdnanceKind::GuidedRocket
                    | SpaceOrdnanceKind::TimeBomb
            ) {
                continue;
            }
            if self.guided_scratch.len() >= MAX_GUIDED {
                break;
            }
            let (serial, weapon_hash) = space
                .ordnance
                .observations
                .as_ref()
                .and_then(|o| o.rounds.get(slot_idx))
                .map(|r| ((r.serial & 0xffff) as u16, r.weapon))
                .unwrap_or(((proj.generation & 0xffff) as u16, 0));
            let weapon_idx = self.weapon_index(weapon_hash);
            self.guided_scratch.push(GuidedSnap {
                slot: (slot_idx & 63) as u8,
                serial,
                weapon_idx,
                pos: encode_pos_u24(&bounds, proj.position),
                vel: encode_vel(proj.velocity, 8.0),
            });
        }
        let guided_count = self.guided_scratch.len();
        if let Some(offer) = self.offers.get_mut(self.used) {
            self.used += 1;
            offer.schema = ORDNANCE_SCHEMA;
            offer.id = 0;
            offer.cadence = 3.0;
            offer.payload.clear();
            offer.payload.extend(REVISION.to_le_bytes());
            let _ = write_guided(&self.guided_scratch[..guided_count], &mut offer.payload);
            offer.state = fnv(&offer.payload);
        }

        // 5. Recent ordnance launches, impacts, and destruction bursts (`0x0214`, id = 1).
        if let Some(offer_idx) = (self.used < self.offers.len()).then_some(self.used) {
            self.used += 1;
            let space_tick = space.tick;
            let mut launch_written = 0u8;
            let mut impact_written = 0u8;
            let mut death_written = 0u8;
            {
                let offer = &mut self.offers[offer_idx];
                offer.schema = ORDNANCE_SCHEMA;
                offer.id = 1;
                offer.cadence = 3.0;
                offer.payload.clear();
                offer.payload.extend(REVISION.to_le_bytes());
                // Header placeholders: tag=1, launch_count, impact_count, death_count.
                offer.payload.extend([1u8, 0, 0, 0]);
                if let Some(obs) = space.ordnance.observations.as_ref() {
                    // Reserve budget for deaths first (up to 8 * 12 = 96 B), reserve space for
                    // fresh impacts (up to half of remaining bytes) so neither list starves,
                    // and pack fresh events before carried-over ones (age 0..=5 ticks) <= EVENTS_BYTES.
                    let deaths_len = obs
                        .wire_deaths
                        .iter()
                        .filter(|e| space_tick.saturating_sub(e.tick) <= 5)
                        .count()
                        .min(MAX_DEATHS);
                    let death_budget = deaths_len * 12;
                    let remaining_for_shots = EVENTS_BYTES.saturating_sub(6 + death_budget);
                    let fresh_impact_bytes: usize = obs
                        .wire_impacts
                        .iter()
                        .filter(|e| !e.published_once && space_tick.saturating_sub(e.tick) <= 5)
                        .take(MAX_IMPACTS)
                        .map(|e| if e.normal.is_some() { 15 } else { 13 })
                        .sum();
                    let reserved_impacts = fresh_impact_bytes.min(remaining_for_shots / 2);
                    let launch_limit_bytes = remaining_for_shots.saturating_sub(reserved_impacts);
                    for carry_pass in [false, true] {
                        for l in obs.wire_launches.iter().filter(|e| {
                            e.published_once == carry_pass && space_tick.saturating_sub(e.tick) <= 5
                        }) {
                            if usize::from(launch_written) >= MAX_LAUNCHES
                                || offer.payload.len() + 19 > 6 + launch_limit_bytes
                            {
                                break;
                            }
                            let tick_off = space_tick.saturating_sub(l.tick) as u8;
                            let widx = self
                                .weapon_hashes
                                .binary_search(&l.weapon_hash)
                                .map(|i| i as u8)
                                .unwrap_or(0);
                            let flags = (tick_off & 7)
                                | (u8::from(l.is_beam) << 3)
                                | ((l.team & 7) << 4);
                            let origin = encode_pos_u24(&bounds, l.origin);
                            let vel = encode_vel(l.velocity, 8.0);
                            offer.payload.extend(l.serial.to_le_bytes());
                            offer.payload.extend([widx, flags]);
                            for c in origin {
                                push_u24(c, &mut offer.payload);
                            }
                            for v in vel {
                                offer.payload.extend(v.to_le_bytes());
                            }
                            launch_written += 1;
                        }
                    }
                    for carry_pass in [false, true] {
                        for imp in obs.wire_impacts.iter().filter(|e| {
                            e.published_once == carry_pass && space_tick.saturating_sub(e.tick) <= 5
                        }) {
                            let entry_len = if imp.normal.is_some() { 15 } else { 13 };
                            if usize::from(impact_written) >= MAX_IMPACTS
                                || offer.payload.len() + entry_len + death_budget > EVENTS_BYTES
                            {
                                break;
                            }
                            let tick_off = space_tick.saturating_sub(imp.tick) as u8;
                            let widx = self
                                .weapon_hashes
                                .binary_search(&imp.weapon_hash)
                                .map(|i| i as u8)
                                .unwrap_or(0);
                            let flags = (tick_off & 7)
                                | (u8::from(imp.shield) << 3)
                                | (u8::from(imp.rigid) << 4)
                                | (u8::from(imp.normal.is_some()) << 5)
                                | (u8::from(imp.blast) << 6);
                            let pos = encode_pos_u24(&bounds, imp.position);
                            offer.payload.extend(imp.serial.to_le_bytes());
                            offer.payload.extend([widx, flags]);
                            for c in pos {
                                push_u24(c, &mut offer.payload);
                            }
                            if let Some(n) = imp.normal {
                                let oct = pack_oct_normal(n);
                                offer.payload.extend([oct[0] as u8, oct[1] as u8]);
                            }
                            impact_written += 1;
                        }
                    }
                    for carry_pass in [false, true] {
                        for d in obs.wire_deaths.iter().filter(|e| {
                            e.published_once == carry_pass && space_tick.saturating_sub(e.tick) <= 5
                        }) {
                            if usize::from(death_written) >= deaths_len
                                || offer.payload.len() + 12 > EVENTS_BYTES
                            {
                                break;
                            }
                            let pos = encode_pos_u24(&bounds, d.position);
                            offer.payload.extend(d.class_idx.to_le_bytes());
                            offer.payload.push(u8::from(d.is_building));
                            for c in pos {
                                push_u24(c, &mut offer.payload);
                            }
                            death_written += 1;
                        }
                    }
                }
                offer.payload[3] = launch_written;
                offer.payload[4] = impact_written;
                offer.payload[5] = death_written;
            }
            if launch_written > 0 || impact_written > 0 || death_written > 0 {
                self.events_seq = self.events_seq.wrapping_add(1).max(1);
                self.offers[offer_idx].state = self.events_seq;
            } else {
                self.offers[offer_idx].state = 0;
            }
        }

        // 6. Space turret aims (`0x0215`, id = 0).
        if let Some(offer_idx) = (self.used < self.offers.len()).then_some(self.used) {
            self.used += 1;
            let offer = &mut self.offers[offer_idx];
            offer.schema = PROPS_SCHEMA;
            offer.id = 0;
            offer.cadence = 3.0;
            offer.payload.clear();
            offer.payload.extend(REVISION.to_le_bytes());
            offer.payload.extend([0u8, self.turret_slots.len() as u8]);
            for &b_slot in &self.turret_slots {
                let (yaw, pitch) = space
                    .buildings
                    .get(b_slot)
                    .and_then(|b| b.turret.as_ref())
                    .map(|t| (encode_turret_angle(t.yaw), encode_turret_angle(t.pitch)))
                    .unwrap_or((0, 0));
                offer.payload.extend(yaw.to_le_bytes());
                offer.payload.extend(pitch.to_le_bytes());
            }
            offer.state = fnv(&offer.payload);
        }

        // 7. Space proximity doors (`0x0215`, id = 1).
        if let Some(offer_idx) = (self.used < self.offers.len()).then_some(self.used) {
            self.used += 1;
            let offer = &mut self.offers[offer_idx];
            offer.schema = PROPS_SCHEMA;
            offer.id = 1;
            offer.cadence = 3.0;
            offer.payload.clear();
            offer.payload.extend(REVISION.to_le_bytes());
            let dcount = space.doors.len().min(MAX_DOORS);
            offer.payload.extend([1u8, dcount as u8]);
            for d in &space.doors[..dcount] {
                let open = js_max(0.0, js_min(255.0, js_round(d.open_fraction * 255.0))) as u8;
                let flags = u8::from(d.locked) | (u8::from(d.door_open) << 1);
                offer.payload.extend([open, flags]);
            }
            offer.state = fnv(&offer.payload);
        }

        // Carry freshly published events across one extra 20 Hz publication so a
        // single dropped or budget-skipped snapshot loses zero shots or bursts.
        if let Some(obs) = world
            .space
            .as_deref_mut()
            .and_then(|s| s.ordnance.observations.as_mut())
        {
            obs.advance_wire_window();
        }
    }
}

/// Checks whether the player at `mailbox` currently occupies a Space fighter seat
/// or remote turret chair, and encodes their `0x91` own correction body into `out`.
pub(crate) fn write_own_space_body(
    space: &SpaceBattlefield,
    mailbox: u32,
    out: &mut Vec<u8>,
) -> Result<bool, &'static str> {
    let Some(conn) = space.pilot_connection_by_mailbox(mailbox) else {
        return Ok(false);
    };
    if let Some(seat) = conn.mount() {
        if let Some(f) = space.fighters.get(seat.fighter).filter(|f| {
            f.alive
                && f.generation == seat.generation
                && f.occupant(seat.seat).is_some_and(|p| p.owner == seat.owner)
        }) {
            let profile = &space.profiles[f.profile_index];
            let can_land = seat.seat == 0
                && matches!(
                    f.landing_at(&space.bounds),
                    SpaceLandingPermission::Allowed { .. }
                );
            out.clear();
            write_own_pilot(
                f,
                profile,
                seat.seat,
                // A seat fires its own first and second weapons; nothing is selected.
                None,
                conn.stick(),
                conn.boarding_use(),
                conn.gunner_angles(),
                can_land,
                &conn.double_tap(),
                out,
            )?;
            return Ok(true);
        }
    }
    if let Some(term) = conn.terminal() {
        let receipt = term.receipt();
        if let Some(gun) = space
            .building_slot(receipt.turret)
            .and_then(|s| space.buildings.get(s))
            .and_then(|b| b.turret.as_ref())
            .filter(|t| {
                t.remote_operator
                    .is_some_and(|p| p.owner == receipt.owner)
            })
        {
            let chair = OwnChairState {
                chair_building: receipt.chair as u16,
                turret_building: receipt.turret as u16,
                aim: term.aim(),
                feet: term.feet,
                turret_yaw: gun.yaw,
                turret_pitch: gun.pitch,
                cooldown: gun.clock.cooldown as f32,
                heat: gun.heat as f32,
                overheated: gun.overheated,
                shots_fired: gun.shots_fired as u16,
            };
            out.clear();
            write_own_chair(&chair, out)?;
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::flight::SpaceLockTargetKind;
    use crate::space::flight::SpaceVehiclePad;

    #[test]
    fn pilot_record_roundtrips_and_rejects_malformed_inputs() {
        let mut c = PilotControls {
            use_pressed: true,
            cycle_weapon: -1,
            terminal_cycle: 1,
            turret_look: [1.25, -2.5],
            turret_turn: -0.5,
            mouse: [0.5, -0.75],
            flight: SpacePilotInput {
                throttle: 0.75,
                roll: -1.0,
                stick_x: 0.25,
                stick_y: -0.5,
                boost: true,
                takeoff_or_land: true,
                eject_held: false,
                fire_primary: true,
                fire_secondary: false,
                lock_target: true,
                trick_roll: 1,
                trick_flip: 2,
                ..Default::default()
            },
        };
        let mut bytes = Vec::new();
        write_pilot_entries(&[PilotEntry { sequence: 42, controls: c }], &mut bytes).unwrap();
        assert_eq!(bytes.len(), 1 + PILOT_ENTRY_BYTES);
        let decoded = read_pilot_entries(&bytes).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].sequence, 42);
        assert_eq!(decoded[0].controls.use_pressed, true);
        assert_eq!(decoded[0].controls.cycle_weapon, -1);
        assert_eq!(decoded[0].controls.terminal_cycle, 1);
        assert_eq!(decoded[0].controls.flight.trick_roll, 1);
        assert_eq!(decoded[0].controls.flight.trick_flip, 2);
        assert!((decoded[0].controls.flight.throttle - 0.75).abs() < 1e-3);
        assert!((decoded[0].controls.mouse[0] - 0.5).abs() < 1e-3);
        assert!((decoded[0].controls.turret_look[1] + 2.5).abs() < 1e-3);

        // Reject zero count, > 8 count, trailing bytes, reserved bit, ternary 3, nonzero reserved word.
        assert!(read_pilot_entries(&[0]).is_err());
        assert!(read_pilot_entries(&[9]).is_err());
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(read_pilot_entries(&extra).is_err());
        let mut bad_flag = bytes.clone();
        bad_flag[6] |= 0x80; // bit 15 reserved
        assert!(read_pilot_entries(&bad_flag).is_err());
        let mut bad_ternary = bytes.clone();
        bad_ternary[5] |= 0x80;
        bad_ternary[6] |= 0x01; // cycle_weapon = 3
        assert!(read_pilot_entries(&bad_ternary).is_err());
        let mut bad_reserved = bytes.clone();
        *bad_reserved.last_mut().unwrap() = 1;
        assert!(read_pilot_entries(&bad_reserved).is_err());
        c.flight.trick_flip = 3;
        let mut tmp = Vec::new();
        assert!(write_pilot_entries(&[PilotEntry { sequence: 1, controls: c }], &mut tmp).is_err());
    }

    #[test]
    fn quat_oct_and_pos_quantizers_roundtrip_accurately() {
        let bounds = PositionBounds::from_map_extents([-2785.3, -1800.0, -3079.5], [2000.0, 1800.0, 3069.2]).unwrap();
        let pos = [1234.567, -456.789, 2100.123];
        let back = decode_pos_u24(&bounds, encode_pos_u24(&bounds, pos));
        for a in 0..3 {
            assert!((back[a] - pos[a]).abs() < 0.001, "axis {a}: {back:?} vs {pos:?}");
        }
        let q = [0.2, -0.4, 0.6, 0.66332495807108];
        let qback = unpack_quat(pack_quat(q)).unwrap();
        let dot: f64 = q.iter().zip(qback).map(|(a, b)| a * b).sum();
        assert!(dot.abs() > 0.99999, "quat dot {dot}");

        let n = [0.3, -0.5, 0.812403840463596];
        let nback = unpack_oct_normal(pack_oct_normal(n));
        let ndot: f64 = n.iter().zip(nback).map(|(a, b)| a * b).sum();
        assert!(ndot > 0.999, "normal dot {ndot}");
    }

    #[test]
    fn shared_space_fields_validate_all_bounds() {
        // Frigates
        let mut buf = Vec::new();
        write_frigates(&[(0, 0), (1, 10), (2, 250)], &mut buf).unwrap();
        assert_eq!(read_frigates(0, &buf).unwrap(), vec![(0, 0), (1, 10), (2, 250)]);
        assert!(read_frigates(1, &buf).is_err());
        let mut bad_stopped = buf.clone();
        bad_stopped[2] = 1; // state=0 with origin!=0
        assert!(read_frigates(0, &bad_stopped).is_err());

        // Fighter chunks: field id must match the chunk number inside the payload.
        let bounds = PositionBounds::default();
        let mut cbuf = Vec::new();
        write_fighter_chunk(1, &[], &bounds, &mut cbuf).unwrap();
        assert!(matches!(
            read_fighter_field(1, &cbuf),
            Ok(FighterField::Chunk(c)) if c.chunk == 1 && c.alive_mask == 0
        ));
        assert!(read_fighter_field(0, &cbuf).is_err());
        assert!(read_fighter_field(2, &cbuf).is_err());

        // Seats: reject duplicate unit or duplicate seat
        let mut sbuf = Vec::new();
        write_seats(
            &[
                SeatEntry { unit_slot: 5, is_chair: false, seat: 0, mount_index: 2 },
                SeatEntry { unit_slot: 6, is_chair: true, seat: 0, mount_index: 1 },
            ],
            &mut sbuf,
        )
        .unwrap();
        assert!(matches!(read_fighter_field(SEATS_FIELD_ID, &sbuf), Ok(FighterField::Seats(v)) if v.len() == 2));
        assert!(read_fighter_field(0, &sbuf).is_err());

        let mut dup = Vec::new();
        write_seats(
            &[
                SeatEntry { unit_slot: 5, is_chair: false, seat: 0, mount_index: 2 },
                SeatEntry { unit_slot: 5, is_chair: false, seat: 1, mount_index: 2 },
            ],
            &mut dup,
        )
        .unwrap();
        assert!(read_fighter_field(SEATS_FIELD_ID, &dup).is_err());

        // Ordnance events: tick_offset 0..=5 valid, 6..=7 and bit 7 rejected.
        for tick_off in 0..=5u8 {
            let mut ev = vec![1u8, 1, 1, 0];
            ev.extend(7u16.to_le_bytes());
            ev.extend([0u8, tick_off | (1 << 3) | (2 << 4)]);
            ev.extend([0u8; 15]);
            ev.extend(7u16.to_le_bytes());
            ev.extend([0u8, tick_off | (1 << 3) | (1 << 4) | (1 << 6)]);
            ev.extend([0u8; 9]);
            let OrdnanceField::Events(parsed) = read_ordnance_field(1, &ev).unwrap() else {
                panic!("expected events");
            };
            assert_eq!(parsed.launches[0].tick_offset, tick_off);
            assert!(parsed.launches[0].is_beam && parsed.launches[0].team == 2);
            assert_eq!(parsed.impacts[0].tick_offset, tick_off);
            assert!(parsed.impacts[0].shield && parsed.impacts[0].rigid && parsed.impacts[0].blast);
        }
        for bad_flags in [6u8, 7u8, 0x80u8] {
            let mut bad_l = vec![1u8, 1, 0, 0];
            bad_l.extend(7u16.to_le_bytes());
            bad_l.extend([0u8, bad_flags]);
            bad_l.extend([0u8; 15]);
            assert!(read_ordnance_field(1, &bad_l).is_err());

            let mut bad_i = vec![1u8, 0, 1, 0];
            bad_i.extend(7u16.to_le_bytes());
            bad_i.extend([0u8, bad_flags]);
            bad_i.extend([0u8; 9]);
            assert!(read_ordnance_field(1, &bad_i).is_err());
        }

        // Props: turrets & doors
        let mut tbuf = vec![0u8, 2];
        tbuf.extend(encode_turret_angle(1.2).to_le_bytes());
        tbuf.extend(encode_turret_angle(-0.4).to_le_bytes());
        tbuf.extend(encode_turret_angle(0.0).to_le_bytes());
        tbuf.extend(encode_turret_angle(0.5).to_le_bytes());
        assert!(matches!(read_props_field(0, &tbuf), Ok(PropsField::Turrets(v)) if v.len() == 2));
        assert!(read_props_field(1, &tbuf).is_err());

        let dbuf = vec![1u8, 1, 128, 3];
        assert!(matches!(read_props_field(1, &dbuf), Ok(PropsField::Doors(v)) if v[0].locked && v[0].opening));
        assert!(read_props_field(1, &[1u8, 1, 128, 4]).is_err());
    }

    #[test]
    fn own_space_body_roundtrips_pilot_gunner_and_chair_and_rejects_bad_data() {
        let chair = OwnChairState {
            chair_building: 12,
            turret_building: 34,
            aim: [0.25, -0.15],
            feet: [10.0, 2.0, -5.0],
            turret_yaw: 0.24,
            turret_pitch: -0.14,
            cooldown: 0.1,
            heat: 0.3,
            overheated: false,
            shots_fired: 7,
        };
        let mut raw = Vec::new();
        write_own_chair(&chair, &mut raw).unwrap();
        let (rev, body) = super::super::revisions::split_revision(&raw).unwrap();
        assert_eq!(rev, REVISION);
        assert_eq!(read_own_body(body).unwrap(), OwnSpaceBody::Chair(chair));

        // Corrupt overheated flag to 2 -> rejected
        let mut bad = raw.clone();
        bad[raw.len() - 3] = 2;
        assert!(read_own_body(&bad[2..]).is_err());

        // Pilot with trick + lock + 2 weapons
        let mut profile = SpaceFlightProfile::default();
        profile.weapons.push(crate::space::flight::PROVISIONAL_WEAPON_DEFAULTS);
        profile.weapons.push(crate::space::flight::PROVISIONAL_WEAPON_DEFAULTS);
        let pad = SpaceVehiclePad {
            name: String::new(),
            team: 1,
            profile_index: 1,
            control_zone: String::new(),
            position: [0.0; 3],
            yaw: 0.0,
            matrix: [0.0; 16],
            spawn_count: 0,
            spawn_time: 10.0,
            expire_time_enemy: 0.0,
            expire_time_field: 0.0,
            decay_time: 0.0,
            respawn_timer: 0.0,
            floor_y: 0.0,
        };
        let mut fighter = SpaceFighter::spawned_on_pad(3, 9, 0, &pad, 1, &profile, 0.0, None, None);
        fighter.state = SpaceFlightState::Flying;
        fighter.trick = SpaceTrick::BarrelRight;
        fighter.position = [100.0, 20.0, -300.0];
        fighter.velocity = [0.0, 0.0, 105.0];
        fighter.quaternion = [0.0, 0.0, 0.0, 1.0];
        fighter.airspeed = 105.0;
        fighter.energy = 80.0;
        fighter.health = 1200.0;
        fighter.gear = 0.0;
        fighter.trick_axis_right = [1.0, 0.0, 0.0];
        fighter.weapon_locks[1] = SpaceLockState {
            weapon_index: 1,
            kind: SpaceLockTargetKind::Fighter,
            target_index: 7,
            target_generation: 2,
            progress: 0.8,
            locked: false,
            lost_los_time: 0.0,
        };
        fighter.lock = fighter.weapon_locks[1];
        let mut pbuf = Vec::new();
        write_own_pilot(&fighter, &profile, 0, Some(1), [0.1, -0.2], false, [0.0, 0.0], true, &crate::space::local::DoubleTapDetector::default(), &mut pbuf).unwrap();
        let (rev, body) = super::super::revisions::split_revision(&pbuf).unwrap();
        assert_eq!(rev, REVISION);
        let parsed = read_own_body(body).unwrap();
        match parsed {
            OwnSpaceBody::Pilot(p) => {
                assert_eq!((p.fighter_id, p.fighter_gen, p.seat, p.profile, p.can_land), (3, 9, 0, 1, true));
                assert_eq!(p.weapons().len(), 2);
                assert!(p.lock.is_some());
                assert_eq!(p.position, fighter.position);
            }
            _ => panic!("expected pilot"),
        }
        // Selecting weapon 0 (which holds no lock) must emit no lock, never falling back to weapon 1 or `f.lock`.
        let mut unselected_buf = Vec::new();
        write_own_pilot(&fighter, &profile, 0, Some(0), [0.1, -0.2], false, [0.0, 0.0], true, &crate::space::local::DoubleTapDetector::default(), &mut unselected_buf).unwrap();
        let (_, body0) = super::super::revisions::split_revision(&unselected_buf).unwrap();
        match read_own_body(body0).unwrap() {
            OwnSpaceBody::Pilot(p) => assert!(p.lock.is_none(), "unselected weapon must not carry another weapon's lock"),
            _ => panic!("expected pilot"),
        }
        // Reject invalid lock_kind (3) even when full lock payload bytes follow.
        let lock_kind_offset = body.len() - 17;
        assert_eq!(body[lock_kind_offset], 1);
        let mut bad_lock = body.to_vec();
        bad_lock[lock_kind_offset] = 3;
        assert!(read_own_body(&bad_lock).is_err());

        // Reject non-unit quaternion
        let mut badq = body.to_vec();
        // quaternion starts at offset 1 + 12 + 48 = 61
        badq[61..69].copy_from_slice(&5.0f64.to_le_bytes());
        assert!(read_own_body(&badq).is_err());

        // The pilot's double taps travel with the body: the clock, the keys
        // held and a tap still able to pair; one that cannot is left out.
        let mut taps = crate::space::local::DoubleTapDetector::default();
        for (roll, throttle) in [(0., 0.); 600].into_iter().chain([(1., 1.), (0., 1.)]) {
            taps.update(1. / 60., roll, throttle, true);
        }
        let mut tbuf = Vec::new();
        write_own_pilot(&fighter, &profile, 0, Some(1), [0.1, -0.2], false, [0.0, 0.0], true, &taps, &mut tbuf).unwrap();
        let (_, tbody) = super::super::revisions::split_revision(&tbuf).unwrap();
        assert_eq!(tbody.len(), body.len() + 16, "a roll tap and a throttle tap pending");
        let OwnSpaceBody::Pilot(p) = read_own_body(tbody).unwrap() else { panic!("expected pilot") };
        let carried = taps.carried();
        assert_eq!(p.taps.carried(), carried);
        assert!(carried.roll.is_some() && carried.throttle.is_some() && carried.held == [0, 1]);
        for _ in 0..30 {
            taps.update(1. / 60., 0., 1., true);
        }
        let mut stale = Vec::new();
        write_own_pilot(&fighter, &profile, 0, Some(1), [0.1, -0.2], false, [0.0, 0.0], true, &taps, &mut stale).unwrap();
        assert_eq!(stale.len(), pbuf.len(), "taps past the window are not carried");
        // The taps byte sits after the 29 numbers; refuse an unknown direction
        // code, a tap later than the clock and one older than the window.
        let at = 1 + 12 + 29 * 8 + 8;
        assert_eq!(tbody[at], 0b0101_0100);
        let mut bad = tbody.to_vec();
        bad[at] |= 0b11;
        assert!(read_own_body(&bad).is_err());
        let clock = f64::from_le_bytes(tbody[at - 8..at].try_into().unwrap());
        for when in [clock + 1e-9, clock - 0.31] {
            let mut bad = tbody.to_vec();
            bad[at + 1..at + 9].copy_from_slice(&when.to_le_bytes());
            assert!(read_own_body(&bad).is_err(), "{when}");
        }
    }
}
