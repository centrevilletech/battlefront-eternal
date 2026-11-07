//! captureOwnPlayer reads the authority's body and arsenal without a facade.
//! Mailbox view and held-input facts are separate from the canonical body.
use crate::{gen::*, combat::defs::{K, WeaponDefs}, store::DefId};
use super::{codec::Result, store_capture::{Capture as Maps, current}};

const NAN: f64 = f64::from_bits(0x7ff8_0000_0000_0000);
pub struct Bytes { pub value: Vec<u8>, limit: usize }
impl Bytes {
    pub fn new(limit: usize) -> Self { Self { value: Vec::with_capacity(limit), limit } }
    // encodeOwnPlayer checks the remaining envelope budget before copying.
    // A rejected optional section must leave the reserved allocation intact;
    // the next publication may retry the same body after the fault clears.
    pub fn raw(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > self.limit.saturating_sub(self.value.len()) { return Err("own-player envelope exceeds budget"); }
        self.value.extend_from_slice(bytes); Ok(())
    }
    pub fn integer(&mut self, n: f64, size: usize) -> Result<()> {
        let max = match size { 1 => 255., 2 => 65535., _ => 4294967295. };
        if !n.is_finite() || n < 0. || n.fract() != 0. || n > max { return Err("invalid own-player integer"); }
        self.raw(&(n as u32).to_le_bytes()[..size])
    }
    // encodeOwnPlayer stores IEEE doubles directly, including signed zero.
    // Only explicitly documented sentinels bypass the finite-number check.
    // Do not quantize these values with the shared entity pose codec.
    pub fn number(&mut self, n: f64, error: &'static str) -> Result<()> {
        if !n.is_finite() { return Err(error); } self.raw(&n.to_le_bytes())
    }
    pub fn flag(&mut self, value: bool) -> Result<()> { self.raw(&[u8::from(value)]) }
}

/// The host's decoded input context, not another body checkpoint. Optional
/// numeric fields retain presence separately from their IEEE value, so a bad
/// present value faults at the original correction-build point.
#[derive(Clone, Copy, Default)]
pub struct Facts {
    pub held: [u8; 16], pub flags: u16, pub view: [f64; 5], pub axes: [f64; 4],
    pub buttons: u16, pub binding: u16, pub discarded: f64, pub invalid_held: bool, pub presentation_first: Option<bool>,
}
// captureArsenalWeapon uses this exact property order for its bit mask.
// Absent yawSpreadOverride has a NaN sentinel, unlike a present zero.
// Do not reorder this table when the native Weapon struct changes.
fn numbers(w: &Weapon) -> [f64; 20] {
    [w.timer, w.clip, w.reserve, w.heat, w.kick, w.spread_scale, w.sniper_scale, w.yaw_spread_override.unwrap_or(NAN),
        w.wait, w.reload_left, w.throw_time, w.held, w.charge, w.pattern, w.salvos_left, w.salvo_wait,
        w.lock_target_id, w.lock_generation, w.lock_elapsed, w.limit]
}
// Object.is distinguishes signed zero but considers all NaNs equal.
// Both differences affect whether a default weapon word is transmitted.
// Numeric equality alone changes held checkpoint bytes.
fn same(a: f64, b: f64) -> bool { a.to_bits() == b.to_bits() || a.is_nan() && b.is_nan() }
pub struct Capture {
    pub body: Bytes, pub mounted: Bytes, pub remote: Bytes, pub weapon: Bytes, pub payload: Bytes,
    defaults: Vec<Option<[f64; 20]>>,
}
impl Capture {
    pub fn new(defs: &WeaponDefs) -> Self {
        // captureArsenalWeapon compares with new Weapon(definition), whose
        // default player=true affects kick even for a mounted AI weapon.
        let defaults = defs.classes.iter().enumerate().map(|(id, class)| class.as_ref()
            .map(|_| numbers(&Weapon::create(DefId(id as u32), defs, true)))).collect();
        Self { body: Bytes::new(800), mounted: Bytes::new(1100), remote: Bytes::new(900),
            weapon: Bytes::new(200), payload: Bytes::new(1100), defaults }
    }
    /// Cold class admission can extend native definitions. No default weapon
    /// or numeric table is constructed during a delivered publication.
    pub fn rebind(&mut self, defs: &WeaponDefs) {
        self.defaults.clear(); self.defaults.extend(defs.classes.iter().enumerate().map(|(id, class)| class.as_ref()
            .map(|_| numbers(&Weapon::create(DefId(id as u32), defs, true)))));
    }
    pub // captureArsenalWeapon writes only values differing from cold defaults.
// Infinite reserve and absent spread override are permitted sentinels.
// Other nonfinite values fail before a partially built checkpoint escapes.
fn arsenal(&mut self, weapon: &Weapon, defs: &WeaponDefs) -> Result<()> {
        arsenal(weapon, defs, &self.defaults, &mut self.weapon)
    }
    // captureSoldierCorrection and captureDroidekaCorrection preserve replay
    // state, including the settled position and dive origin when conditional.
    // This is a checkpoint read: it must not settle or animate the body.
    pub fn foot(&mut self, body: &UnitBody) -> Result<u8> {
        let out = &mut self.body; out.value.clear();
        match body {
            UnitBody::Soldier(s) => {
                let settled = s.position.iter().zip(s.settled).any(|(a, b)| !same(*a, b));
                let diving = s.dive != 0.;
                out.raw(&[u8::from(settled) | u8::from(diving) << 1, s.posture as u8,
                    s.air.map_or(0, |a| a as u8 + 1), u8::from(s.grounded)])?;
                for n in [s.fuel, s.aim_yaw, s.aim_pitch, s.dive, s.state_time, s.lock] { out.number(n, "nonfinite Soldier correction")?; }
                for vector in [s.position, s.velocity, s.normal] { for n in vector { out.number(n, "nonfinite Soldier correction")?; } }
                if settled { for n in s.settled { out.number(n, "nonfinite Soldier correction")?; } }
                if diving { out.raw(&[s.dive_from as u8])?; } Ok(6)
            }
            // captureDroidekaCorrection validates mode and barrel before shield state.
            // Keep that order so malformed checkpoints report the original fault.
            // A missing shield and a present disabled shield have different layouts.
            UnitBody::Droideka(d) => {
                if !d.mode.is_finite() || d.mode.fract() != 0. || !(0. ..=13.).contains(&d.mode) { return Err("invalid droideka mode or barrel"); }
                if !d.gun.is_finite() || d.gun < 0. || d.gun.fract() != 0. || d.gun > 9007199254740991. { return Err("invalid droideka mode or barrel"); }
                if d.shield.as_ref().is_some_and(|s| s.value < 0. || s.value > s.max) { return Err("invalid droideka shield"); }
                let mut flags = u8::from(d.grounded) | u8::from(d.reverse) << 1 | u8::from(d.dead) << 2;
                if let Some(s) = &d.shield { flags |= 8 | u8::from(s.on) << 4; }
                if d.last_gun.is_some() { flags |= 32; }
                out.raw(&[d.mode as u8, flags])?;
                for n in [d.aim_yaw, d.aim_pitch, d.state_time, d.time, d.legs, d.spin, d.roll, d.lean, d.blast_clock, d.look_x, d.look_y, d.gun] {
                    out.number(n, "nonfinite droideka state")?;
                }
                for v in [d.position, d.velocity, d.avoidance] { for n in v { out.number(n, "nonfinite droideka state")?; } }
                if let Some(s) = &d.shield { out.number(s.value, "nonfinite droideka state")?; }
                if let Some(g) = &d.last_gun { out.number(g.index, "nonfinite droideka state")?; out.number(g.barrel, "nonfinite droideka state")?; }
                Ok(10)
            }
        }
    }
    /// captureGroundCorrection includes nullable seat history. These values
    /// belong to the canonical pose owner, separately from the physics row.
    pub fn ground(&mut self, v: &Vehicle, seat: [Option<f64>; 6]) -> Result<u8> {
        let out = &mut self.body; out.value.clear();
        for n in [v.health, v.team.unwrap_or(NAN), v.yaw, v.tilt, v.roll, v.yaw_rate, v.forward_input,
            v.right_input, v.tilt_rate, v.roll_rate, v.support].into_iter().chain(v.position).chain(v.velocity).chain(v.quaternion) {
            out.number(n, "nonfinite ground correction")?;
        }
        out.flag(v.alive)?; out.flag(v.sleeping.ok_or("invalid ground state")?)?;
        seat_bytes(out, seat, "nonfinite ground correction")?; Ok(10)
    }
    /// captureCompactFlight keeps transition and trick history conditional,
    /// but never rounds timers, targeting state or its permitted floor NaN.
    pub fn flight(&mut self, v: &Vehicle, enums: [u8; 6], seat: [Option<f64>; 6]) -> Result<u8> {
        let out = &mut self.body; out.value.clear(); out.raw(&enums)?;
        let optional = |v: Option<f64>| v.unwrap_or(NAN);
        for n in [v.health, optional(v.team), v.yaw, v.tilt, v.roll, v.yaw_rate, v.forward_input, v.right_input,
            v.altitude, v.throttle, v.airspeed, v.tilt_rate, v.state_time] { out.number(n, "nonfinite flight correction")?; }
        if v.floor_y.is_nan() { out.raw(&v.floor_y.to_le_bytes())?; } else { out.number(v.floor_y, "nonfinite flight correction")?; }
        for n in [v.bank, optional(v.power), optional(v.stick_x), optional(v.stick_y), optional(v.flight_roll_rate),
            optional(v.trick_cooldown), optional(v.trick_pending), optional(v.gear), optional(v.landing_message_time),
            optional(v.flight_clock), optional(v.flight_assist_id), optional(v.flight_lock_id), optional(v.flight_lock_generation),
            optional(v.flight_lock_timer), optional(v.flight_lost_los)] { out.number(n, "nonfinite flight correction")?; }
        if enums[1] != 0 { for n in [optional(v.trick_time), optional(v.trick_direction)] { out.number(n, "nonfinite flight correction")?; } }
        for vector in [v.position, v.velocity] { for n in vector { out.number(n, "nonfinite flight correction")?; } }
        // captureCompactFlight emits path endpoints only during takeoff/landing.
        // Stale vectors left on a flying craft are deliberately not serialized.
        // A barrel roll and an immelmann retain different orientation histories.
        let transition = enums[0] == 1 || enums[0] == 3;
        if transition {
            for vector in [v.flight_start, v.flight_end] { for n in vector.ok_or("missing flight transition vector")? { out.number(n, "nonfinite flight correction")?; } }
        }
        if enums[1] == 1 { for n in v.flight_trick_right.ok_or("missing flight trick vector")? { out.number(n, "nonfinite flight correction")?; } }
        for n in v.quaternion { out.number(n, "nonfinite flight correction")?; }
        if transition { for n in v.flight_start_q.ok_or("missing flight transition orientation")? { out.number(n, "nonfinite flight correction")?; } }
        if enums[1] == 3 {
            for q in [v.flight_trick_loop_q, v.flight_trick_end_q] { for n in q.ok_or("missing flight trick orientation")? { out.number(n, "nonfinite flight correction")?; } }
        }
        for flag in [Some(v.alive), v.sleeping, Some(v.grounded), v.roll_lock, v.boosting, v.boost_exhausted,
            v.terrain_warning, v.flight_action_held, v.trick_held, v.immelmann_held] { out.raw(&[flag.map_or(2, u8::from)])?; }
        seat_bytes(out, seat, "nonfinite flight correction")?; Ok(7)
    }
    /// captureMountedControl names the lease's mount, not whichever member
    /// now occupies its registry slot. Weapon/barrel order is the seat order.
    pub fn mount(&mut self, mode: u8, id: f64, generation: f64, index: f64, aim: &[f64], flags: u8,
        weapons: &[(&Weapon, f64)], defs: &WeaponDefs) -> Result<()> {
        if weapons.len() > 2 { return Err("mounted weapon count exceeds correction budget"); }
        let out = &mut self.mounted; out.value.clear();
        out.raw(&[mode])?; out.integer(id, 4)?; out.integer(generation, 4)?; out.integer(index, 1)?; out.raw(&[flags])?;
        for &n in aim { out.number(n, "nonfinite mounted control")?; } out.integer(weapons.len() as f64, 1)?;
        for &(weapon, barrel) in weapons {
            if !barrel.is_finite() || barrel < 0. || barrel.fract() != 0. || barrel > 9007199254740991. { return Err("invalid mounted barrel"); }
            arsenal(weapon, defs, &self.defaults, &mut self.weapon)?;
            out.raw(&barrel.to_le_bytes())?; out.integer(self.weapon.value.len() as f64, 2)?; out.raw(&self.weapon.value)?;
        }
        Ok(())
    }
    /// encodeRemoteCorrection shares the arsenal format but retains the
    /// connection's independent camera, RNG and pending orbital salvo frame.
    /// The enclosing own envelope supplies its launch slot and generation.
    pub fn recon(&mut self, d: &ReconDroid, c: &ReconContext, defs: &WeaponDefs, aspect: f64,
        salvo: Option<(u32, u32, f64)>) -> Result<()> {
        if d.weapons.len() > 2 { return Err("unsupported recon arsenal"); }
        if d.dead { return Err("invalid recon control state"); }
        for channel in 0..2 {
            let count = d.weapons.iter().filter(|w| w.channel(defs) == channel as f64).count().max(1);
            let selected = d.selected[channel];
            if !selected.is_finite() || selected < 0. || selected.fract() != 0. || selected >= count as f64 { return Err("invalid recon selected weapon"); }
        }
        let out = &mut self.remote; out.value.clear();
        out.raw(&[2, 2 | u8::from(c.first_person) << 2])?;
        for n in [d.yaw, d.pitch, d.health, d.countdown, c.look_x, c.look_y] { out.number(n, "invalid recon number")?; }
        out.flag(c.zoom)?; out.integer(c.seed as f64, 4)?;
        for vector in [d.position, d.velocity] { for n in vector { out.number(n, "invalid recon number")?; } }
        for n in d.selected { out.integer(n, 1)?; } out.integer(d.weapons.len() as f64, 1)?;
        for weapon in &d.weapons {
            arsenal(weapon, defs, &self.defaults, &mut self.weapon)?;
            out.integer(self.weapon.value.len() as f64, 2)?; out.raw(&self.weapon.value)?;
        }
        // encodeRemoteCorrection retains the orbital salvo frame after launch.
        // It is continuation state, not merely the current selected weapon.
        // A subsequent replay must use these same axes and remaining shot count.
        out.flag(d.salvo.is_some())?;
        if let Some(s) = &d.salvo {
            let (weapon, level, maximum) = salvo.ok_or("recon salvo definition missing")?;
            if s.left > maximum { return Err("invalid recon salvo count"); }
            out.integer(weapon as f64, 1)?; out.integer(level as f64, 1)?; out.integer(s.left, 4)?; out.number(s.wait, "invalid recon number")?;
            for vector in [s.at, s.x, s.y, s.z] { for n in vector { out.number(n, "invalid recon number")?; } }
        }
        if !(aspect > 0.) || (v8math::hypot_n(&c.camera.rotation) - 1.).abs() > 0.001 {
            return Err("invalid recon camera");
        }
        out.flag(true)?;
        for n in c.camera.position.into_iter().chain(c.camera.rotation).chain([aspect, c.camera.fov, c.camera.reach]) { out.number(n, "invalid recon number")?; }
        out.flag(c.camera.fresh)
    }
    /// captureOwnPlayer preserves every byte of replay state. The enclosing
    /// publication chooses the body and control mode from the qualified lease;
    /// only this client's input context arrives as foreign facts.
    pub fn envelope(&mut self, unit: &Unit, schema: u8, vehicle: (f64, f64), epoch: f64,
        content_epoch: f64, maps: &Maps, defs: &WeaponDefs, facts: &Facts, remote: Option<(f64, f64, f64)>) -> Result<()> {
        if unit.team != 1. && unit.team != 2. { return Err("invalid own team"); }
        let class = maps.class_id(unit.class_name); if class == 0. { return Err("player class prerequisite missing"); }
        let out = &mut self.payload; out.value.clear();
        out.integer(epoch, 4)?; out.integer(content_epoch, 4)?; out.integer(class, 2)?;
        out.integer(unit.team, 1)?; out.raw(&[schema])?; out.integer(vehicle.0, 4)?; out.integer(vehicle.1, 4)?;
        out.integer(self.body.value.len() as f64, 2)?; out.raw(&self.body.value)?;
        let c = &unit.combat;
        out.number(c.health, "invalid own-player number")?; out.flag(c.dead)?;
        out.number(c.death_time, "invalid own-player number")?; out.number(unit.dead_time, "invalid own-player number")?; out.flag(unit.removed)?;
        for n in [c.kick.rate, c.kick.hold, c.kick.offset] { out.number(n, "invalid own-player number")?; }
        out.integer(c.zoom_step, 1)?; out.number(zoom(c, defs), "invalid own-player number")?;
        out.raw(&[u8::from(facts.presentation_first.unwrap_or(c.first_person)) | u8::from(c.gun_underwater) << 1])?;
        for channel in 0..2 { out.integer(if current(&c.weapons, c.selected[channel], channel as f64, defs).is_some() { c.selected[channel] } else { 255. }, 1)?; }
        out.integer(c.weapons.len() as f64, 1)?;
        for weapon in &c.weapons {
            arsenal(weapon, defs, &self.defaults, &mut self.weapon)?;
            out.integer(maps.weapon_id(weapon.def.0).ok_or("weapon definition prerequisite missing")?, 2)?;
            out.integer(self.weapon.value.len() as f64, 2)?; out.raw(&self.weapon.value)?;
        }
        if facts.invalid_held { return Err("held code missing from registry"); }
        // encodeOwnPlayer appends held keys after all canonical arsenal bytes.
        // Presence flags decide whether view numbers are validated and emitted.
        // An absent field must not fault merely because its scratch word is NaN.
        out.raw(&facts.held)?; out.integer(facts.flags as f64, 2)?;
        for i in 0..3 { if facts.flags & (1 << (5 + i)) != 0 { out.number(facts.view[i], "invalid own-player number")?; } }
        out.integer(facts.binding as f64, 2)?;
        for i in 3..5 { if facts.flags & (1 << (5 + i)) != 0 { out.number(facts.view[i], "invalid own-player number")?; } }
        if facts.flags & 16 != 0 { for n in facts.axes { out.number(n, "invalid own-player number")?; } out.integer(facts.buttons as f64, 2)?; }
        out.integer(facts.discarded, 4)?;
        // captureMountedControl is independent of the driver's body schema.
        // A passenger still has a foot body but names the occupied seat lease.
        // Recon control adds its own identity after the same held-input envelope.
        let mode = remote.map_or_else(|| self.mounted.value.first().copied().unwrap_or(0), |_| 1);
        out.raw(&[mode])?;
        if !self.mounted.value.is_empty() { out.integer(self.mounted.value.len() as f64, 2)?; out.raw(&self.mounted.value)?; }
        if let Some((id, life, definition)) = remote {
            out.integer(id, 2)?; out.integer(life, 4)?; out.integer(definition, 2)?;
            out.integer(self.remote.value.len() as f64, 2)?; out.raw(&self.remote.value)?;
        }
        if out.value.len() > 1051 { return Err("entity record cannot fit one message"); }
        Ok(())
    }
}
// captureSeatHistory distinguishes absent history from a stored zero.
// The presence marker precedes each numeric value in checkpoint order.
// Serialize zeros for absent values without changing their marker.
fn seat_bytes(out: &mut Bytes, values: [Option<f64>; 6], error: &'static str) -> Result<()> {
    for n in values { out.raw(&[u8::from(n.is_none())])?; out.number(n.unwrap_or(0.), error)?; } Ok(())
}
fn arsenal(weapon: &Weapon, defs: &WeaponDefs, defaults: &[Option<[f64; 20]>], out: &mut Bytes) -> Result<()> {
    let base = defaults.get(weapon.def.0 as usize).and_then(Option::as_ref).ok_or("weapon definition prerequisite missing")?;
    let class = defs.classes.get(weapon.def.0 as usize).and_then(Option::as_ref).ok_or("weapon state definition mismatch")?;
    if !weapon.level.is_finite() || weapon.level < 0. || weapon.level.fract() != 0. || weapon.level > 255.
        || weapon.level as usize >= class.levels.len() { return Err("weapon state definition mismatch"); }
    let values = numbers(weapon); let mut mask = 0;
    for (i, n) in values.iter().enumerate() {
        if !n.is_finite() && !(i == 2 && *n == f64::INFINITY) && !(i == 7 && weapon.yaw_spread_override.is_none()) { return Err("invalid weapon state number"); }
        if !same(*n, base[i]) { mask |= 1 << i; }
    }
    let flags = u8::from(weapon.deployed) | u8::from(weapon.firing) << 1 | u8::from(weapon.overheated) << 2
        | u8::from(weapon.released) << 3 | u8::from(weapon.hidden) << 4 | u8::from(weapon.charging) << 5 | u8::from(weapon.lock_complete) << 6;
    out.value.clear(); out.raw(&[weapon.level as u8, flags])?; out.raw(&u32::to_le_bytes(mask))?;
    for (i, n) in values.iter().enumerate() { if mask & (1 << i) != 0 { out.raw(&n.to_le_bytes())?; } }
    if let Some(melee) = &weapon.melee { out.raw(&melee.time.to_le_bytes())?; out.raw(&[jsrt::to_uint32(melee.index) as u8, u8::from(melee.active)])?; }
    Ok(())
}
// captureOwnPlayer derives zoomScale from authored weapon levels.
// A continuous ZoomRate suppresses the second discrete zoom step.
// Do not use the page camera, which can be between visual transitions.
fn zoom(c: &Combat, defs: &WeaponDefs) -> f64 {
    if c.zoom_step == 0. { return 1.; }
    let Some(w) = current(&c.weapons, c.selected[0], 0., defs) else { return 1. };
    let Some(class) = defs.classes.get(w.def.0 as usize).and_then(Option::as_ref) else { return 1. };
    let Some(level) = class.levels.get(w.level as usize) else { return 1. };
    let mut count = 0.; let mut last = 1.;
    for n in [level.get(K::ZoomMin).or(1.), if level.get(K::ZoomRate).truthy() { 0. } else { level.get(K::ZoomMax).or(1.) }] {
        if n > last { count += 1.; if c.zoom_step == count { return n; } last = n; }
    }
    1.
}
