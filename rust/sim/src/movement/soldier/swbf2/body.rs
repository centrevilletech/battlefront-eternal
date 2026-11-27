//! A 2005 soldier's own-body correction: body schema `0x82` (record kind 2,
//! `crate::wire::revisions`), its revision (u16), then the compact soldier body
//! of schema 6, unchanged, followed by this profile's per-life state in exact
//! little-endian doubles, so a client restores the same bits the authority
//! stepped and replays from them.
//!
//! Suffix (revision 1): `energy:f64` (from the class's floor to its capacity),
//! `moves:u8` (bit 0 sprint, bits 1-2 roll direction, bits 3-4 reserved for
//! the combat checkpoint, bit 5 exhausted, bit 6 tumbling, bit 7 unsupported),
//! then `sprintTime:f64` while sprinting, `rollElapsed:f64` while rolling,
//! `landCooldown:f64`, `jumpCount:u32`, `unsupportedTime:f64` while
//! unsupported, and while tumbling `phase:u8` (1 thrown, 2 tumble, 3 landed,
//! 4 getting up, 5 recovering), `flags:u8` (bit 0 face down, bit 1 bounced,
//! bit 2 died tumbling), `flyDir:u8` (0 to 3) and six doubles: elapsed, air
//! time, angle, spin rate, heading offset, cycle phase. 21 bytes idle, at
//! most 96.
use super::{Roll, RollDirection, State, Tuning, Tumble, TumblePhase};
use crate::prediction::corrections::CompactSoldierState;

/// The own-correction body schema of a 2005 soldier: record kind 2.
pub const SCHEMA: u8 = crate::wire::revisions::body_schema(crate::wire::revisions::SOLDIER);
/// The revision of the soldier body and of the display field this build writes and reads.
pub const REVISION: u16 = 1;

/// The compact body and suffix of a soldier body, after its revision, which
/// must be this build's.
pub fn revision_body(body: &[u8]) -> Result<&[u8], &'static str> {
    match crate::wire::revisions::split_revision(body)? {
        (REVISION, rest) => Ok(rest),
        _ => Err(INVALID),
    }
}
const SPRINT: u8 = 1;
const ROLL: u8 = 0b110;
/// Reserved for the combo state and a thrown saber; must be zero until the
/// combat checkpoint is part of the format.
const COMBAT: u8 = 0b1_1000;
const EXHAUSTED: u8 = 0b10_0000;
const TUMBLING: u8 = 0b100_0000;
const UNSUPPORTED: u8 = 0b1000_0000;
/// The longest suffix: every optional part present.
pub const MAX_SUFFIX: usize = 21 + 8 + 8 + 8 + TUMBLE_BYTES;
const TUMBLE_BYTES: usize = 3 + 6 * 8;
const TUMBLE_FLAGS: u8 = 0b111;
const INVALID: &str = "invalid Battlefront II soldier correction";
/// The compact posture code of lying prone, which this profile never does.
const PRONE: u8 = 2;

/// The compact body's length, from its flag byte: 124 bytes, plus the settled
/// position when it differs and the dive origin while diving.
pub fn compact_len(body: &[u8]) -> Option<usize> {
    let flags = *body.first()?;
    if flags > 3 {
        return None;
    }
    Some(124 + usize::from(flags & 1) * 24 + usize::from(flags & 2 != 0))
}

/// The movement state's bytes at the start of `bytes`, by its moves byte;
/// none for bytes too short to have one. What follows them in the soldier's
/// own correction is his power lock (see [`read_lock`]).
pub fn state_len(bytes: &[u8]) -> Option<usize> {
    let moves = *bytes.get(8)?;
    Some(21 + 8 * usize::from(moves & SPRINT != 0) + 8 * usize::from(moves & ROLL != 0)
        + 8 * usize::from(moves & UNSUPPORTED != 0) + if moves & TUMBLING != 0 { TUMBLE_BYTES } else { 0 })
}

/// The bytes of a power lock in the soldier's own correction, after his
/// movement state: the target's unit slot (u16) and life (u32).
pub const LOCK_BYTES: usize = 6;

/// Appends the soldier's power lock target, if one holds.
pub fn write_lock(lock: Option<(u16, u32)>, out: &mut Vec<u8>) {
    if let Some((slot, life)) = lock {
        out.extend(slot.to_le_bytes());
        out.extend(life.to_le_bytes());
    }
}

/// Reads what follows the movement state: nothing, or a power lock target.
pub fn read_lock(tail: &[u8]) -> Result<Option<(u16, u32)>, &'static str> {
    match tail {
        [] => Ok(None),
        [s0, s1, l0, l1, l2, l3] => Ok(Some((u16::from_le_bytes([*s0, *s1]), u32::from_le_bytes([*l0, *l1, *l2, *l3])))),
        _ => Err(INVALID),
    }
}

/// Splits what follows a soldier body's compact prefix into his movement
/// state and his power lock.
pub fn split_lock(suffix: &[u8]) -> Result<(&[u8], Option<(u16, u32)>), &'static str> {
    let (state, tail) = suffix.split_at_checked(state_len(suffix).ok_or(INVALID)?).ok_or(INVALID)?;
    Ok((state, read_lock(tail)?))
}

/// Suffix bytes for `state`.
pub fn suffix_len(state: &State) -> usize {
    21 + 8 * usize::from(state.sprint_time.is_some()) + 8 * usize::from(state.roll.is_some())
        + 8 * usize::from(state.unsupported_time.is_some()) + if state.tumble.is_some() { TUMBLE_BYTES } else { 0 }
}

fn phase_code(phase: TumblePhase) -> u8 {
    match phase {
        TumblePhase::Thrown => 1,
        TumblePhase::Tumble => 2,
        TumblePhase::Landed => 3,
        TumblePhase::GetUp => 4,
        TumblePhase::Recover => 5,
    }
}

/// The suffix for `state` in a fixed buffer, with its length. A state with a
/// value no step writes (a nonfinite number) fails before anything is
/// written.
pub fn suffix(state: &State) -> Result<([u8; MAX_SUFFIX], usize), &'static str> {
    let mut moves = if state.exhausted { EXHAUSTED } else { 0 };
    if state.sprint_time.is_some() { moves |= SPRINT; }
    if let Some(roll) = state.roll { moves |= (roll.direction as u8) << 1; }
    if state.tumble.is_some() { moves |= TUMBLING; }
    if state.unsupported_time.is_some() { moves |= UNSUPPORTED; }
    let mut out = [0u8; MAX_SUFFIX];
    let mut at = 0;
    let mut put = |bytes: &[u8]| { out[at..at + bytes.len()].copy_from_slice(bytes); at += bytes.len(); };
    let mut finite = state.energy.is_finite() && state.land_cooldown.is_finite();
    put(&state.energy.to_le_bytes());
    put(&[moves]);
    for value in [state.sprint_time, state.roll.map(|r| r.elapsed)].into_iter().flatten() {
        finite &= value.is_finite();
        put(&value.to_le_bytes());
    }
    put(&state.land_cooldown.to_le_bytes());
    put(&state.jump_count.to_le_bytes());
    if let Some(time) = state.unsupported_time {
        finite &= time.is_finite();
        put(&time.to_le_bytes());
    }
    if let Some(t) = state.tumble {
        let flags = u8::from(t.front) | u8::from(t.bounce) << 1 | u8::from(t.dead) << 2;
        put(&[phase_code(t.phase), flags, t.fly_dir]);
        for value in [t.elapsed, t.air_time, t.angle, t.spin_rate, t.heading_offset, t.cycle_phase] {
            finite &= value.is_finite();
            put(&value.to_le_bytes());
        }
    }
    if !finite {
        return Err(INVALID);
    }
    Ok((out, at))
}

/// Appends the suffix to a reserved buffer.
pub fn write_suffix(state: &State, out: &mut Vec<u8>) -> Result<(), &'static str> {
    let (bytes, length) = suffix(state)?;
    out.extend_from_slice(&bytes[..length]);
    Ok(())
}

/// What the compact prefix says about the body, for the suffix's checks.
#[derive(Clone, Copy, Debug)]
pub struct Body {
    pub grounded: bool,
    /// The compact air code: zero for none.
    pub air: u8,
    /// The compact posture code.
    pub posture: u8,
    /// The movement lock timer.
    pub lock: f64,
    /// Whether the compact body is diving, which this profile never does.
    pub diving: bool,
}

impl Body {
    /// The facts of a decoded compact body.
    pub fn of(c: &CompactSoldierState) -> Self {
        Self { grounded: c.grounded, air: c.air, posture: c.posture, lock: c.numbers[5], diving: c.numbers[3] != 0.0 }
    }
}

/// Reads a whole suffix against the soldier's class tuning and the facts of
/// the compact prefix it follows. Every value is checked before the state is
/// returned; nothing partial escapes, and only the bytes `suffix` writes for
/// that state are read.
pub fn read_suffix(bytes: &[u8], tuning: &Tuning, body: Body) -> Result<State, &'static str> {
    let mut at = 0;
    let number = |at: &mut usize| -> Result<f64, &'static str> {
        let b: [u8; 8] = bytes.get(*at..*at + 8).ok_or(INVALID)?.try_into().map_err(|_| INVALID)?;
        *at += 8;
        let x = f64::from_le_bytes(b);
        if x.is_finite() { Ok(x) } else { Err(INVALID) }
    };
    // This profile never lies prone or dives.
    if body.posture == PRONE || body.diving || !body.lock.is_finite() {
        return Err(INVALID);
    }
    let energy = number(&mut at)?;
    let moves = *bytes.get(at).ok_or(INVALID)?;
    at += 1;
    if moves & COMBAT != 0 || !(tuning.energy_floor..=tuning.energy_capacity).contains(&energy) {
        return Err(INVALID);
    }
    let direction = match (moves & ROLL) >> 1 {
        0 => None,
        1 => Some(RollDirection::Forward),
        2 => Some(RollDirection::Left),
        _ => Some(RollDirection::Right),
    };
    let sprint_time = if moves & SPRINT != 0 {
        let time = number(&mut at)?;
        if !(0.0..=tuning.sprint_accelerate_time).contains(&time) { return Err(INVALID); }
        Some(time)
    } else { None };
    let roll = match direction {
        Some(direction) => {
            let elapsed = number(&mut at)?;
            // A roll exists only for a class with a roll action, and ends once
            // its elapsed time passes the action's window.
            if tuning.roll_frames == 0 || !(0.0..=tuning.roll_seconds()).contains(&elapsed) { return Err(INVALID); }
            Some(Roll { direction, elapsed })
        }
        None => None,
    };
    let land_cooldown = number(&mut at)?;
    let count: [u8; 4] = bytes.get(at..at + 4).ok_or(INVALID)?.try_into().map_err(|_| INVALID)?;
    at += 4;
    let unsupported_time = if moves & UNSUPPORTED != 0 { Some(number(&mut at)?) } else { None };
    if land_cooldown < 0.0 || unsupported_time.is_some_and(|t| t < 0.0) {
        return Err(INVALID);
    }
    let tumble = if moves & TUMBLING != 0 {
        let head = bytes.get(at..at + 3).ok_or(INVALID)?;
        at += 3;
        let phase = match head[0] {
            1 => TumblePhase::Thrown,
            2 => TumblePhase::Tumble,
            3 => TumblePhase::Landed,
            4 => TumblePhase::GetUp,
            5 => TumblePhase::Recover,
            _ => return Err(INVALID),
        };
        if head[1] & !TUMBLE_FLAGS != 0 || head[2] > 3 { return Err(INVALID); }
        let mut values = [0.0; 6];
        for value in &mut values { *value = number(&mut at)?; }
        if values[0] < 0.0 || values[1] < 0.0 { return Err(INVALID); }
        Some(Tumble {
            phase, elapsed: values[0], air_time: values[1], angle: values[2], spin_rate: values[3],
            front: head[1] & 1 != 0, bounce: head[1] & 2 != 0, fly_dir: head[2], heading_offset: values[4],
            cycle_phase: values[5], dead: head[1] & 4 != 0,
        })
    } else { None };
    if at != bytes.len() {
        return Err(INVALID);
    }
    Ok(State {
        energy, exhausted: moves & EXHAUSTED != 0, sprint_time, roll, land_cooldown, unsupported_time,
        jump_count: u32::from_le_bytes(count), tumble,
    })
}

/// The shared world field (`0x0203`, record kind 3) that shows a 2005 soldier
/// to every client: what the HUD and the remote poses need, quantized. Its
/// payload is the revision (u16), then the display bytes below. It is for
/// display only, never prediction input.
pub const DISPLAY_SCHEMA: u16 = crate::wire::revisions::field_schema(crate::wire::revisions::SOLDIER_DISPLAY);
const DISPLAY_EXHAUSTED: u8 = 0b1000;

/// A soldier as the display field shows him.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Display {
    /// Energy as a fraction of the class capacity, to 1/65535, never below 0.
    pub energy: f64,
    pub sprint: bool,
    pub exhausted: bool,
    /// The roll in flight: its direction, the authority tick of its first
    /// step and the body's current facing in 65,536 steps a turn.
    pub roll: Option<(RollDirection, u32, u16)>,
}

/// The display field's payload at authority tick `tick` for a body facing
/// `yaw`: 3 bytes, or 9 while rolling. A receiver places the roll pose at its
/// own frame time from the start tick and the class's roll window.
pub fn display(state: &State, tuning: &Tuning, tick: u32, yaw: f64) -> ([u8; 9], usize) {
    let fraction = if tuning.energy_capacity > 0.0 { state.energy / tuning.energy_capacity } else { 0.0 };
    let energy = jsrt::js_round(jsrt::js_min(1.0, jsrt::js_max(0.0, fraction)) * 65535.0) as u16;
    let mut out = [0u8; 9];
    out[..2].copy_from_slice(&energy.to_le_bytes());
    out[2] = u8::from(state.sprint_time.is_some()) | if state.exhausted { DISPLAY_EXHAUSTED } else { 0 };
    let Some(roll) = state.roll else { return (out, 3) };
    out[2] |= (roll.direction as u8) << 1;
    // The entry tick is the roll's first step, so a displayed roll has
    // stepped at least once.
    let stepped = jsrt::js_max(1.0, jsrt::js_round(roll.elapsed * 60.0)) as u32;
    let start = tick.wrapping_add(1).wrapping_sub(stepped);
    let turn = yaw / std::f64::consts::TAU;
    let heading = if turn.is_finite() { (jsrt::js_round(turn.rem_euclid(1.0) * 65536.0) as u32 % 65536) as u16 } else { 0 };
    out[3..7].copy_from_slice(&start.to_le_bytes());
    out[7..9].copy_from_slice(&heading.to_le_bytes());
    (out, 9)
}

/// Reads a display payload, refusing any length, flag, direction or
/// combination no authority writes.
pub fn read_display(payload: &[u8]) -> Result<Display, &'static str> {
    const FIELD: &str = "invalid Battlefront II soldier field";
    if payload.len() != 3 && payload.len() != 9 {
        return Err(FIELD);
    }
    let flags = payload[2];
    if flags & !(SPRINT | ROLL | DISPLAY_EXHAUSTED) != 0 {
        return Err(FIELD);
    }
    let energy = f64::from(u16::from_le_bytes([payload[0], payload[1]])) / 65535.0;
    let direction = match (flags & ROLL) >> 1 {
        0 => None,
        1 => Some(RollDirection::Forward),
        2 => Some(RollDirection::Left),
        _ => Some(RollDirection::Right),
    };
    let sprint = flags & SPRINT != 0;
    let exhausted = flags & DISPLAY_EXHAUSTED != 0;
    if direction.is_some() != (payload.len() == 9) || sprint && (direction.is_some() || exhausted) {
        return Err(FIELD);
    }
    let roll = direction.map(|d| (d, u32::from_le_bytes(payload[3..7].try_into().unwrap()),
        u16::from_le_bytes([payload[7], payload[8]])));
    Ok(Display { energy, sprint, exhausted, roll })
}

#[cfg(test)]
mod tests {
    use super::super::ControlSpeed;
    use super::*;

    const STAND: u8 = 0;
    const CROUCH: u8 = 1;

    // Invented class numbers with a debt floor, to reach a negative balance.
    fn tuning() -> Tuning {
        let row = ControlSpeed { forward: 1.0, strafe: 1.0, turn: 1.0 };
        Tuning { stand: row, crouch: row, sprint: row, jump: row, jet: row, jetpack: None, roll: row, tumble: row,
            sprint_accelerate_time: 0.5, energy_capacity: 80.0, energy_floor: -15.0, energy_restore: 4.0,
            energy_restore_idle: 6.0, energy_drain_sprint: 9.0, energy_min_sprint: 12.0, energy_cost_jump: 3.0,
            energy_cost_jump_sprint: None, energy_cost_roll: 11.0, jump_height: 1.2, jump_forward_speed_factor: 1.1,
            jump_strafe_speed_factor: 0.9, roll_speed_factor: 1.7, roll_frames: 29, recover_from_tumble: true,
            getup_front_seconds: 1.0, getup_back_seconds: 1.0 }
    }
    const STANDING: Body = Body { grounded: true, air: 0, posture: STAND, lock: 0.0, diving: false };
    const CROUCHED: Body = Body { posture: CROUCH, ..STANDING };

    fn idle(energy: f64) -> State {
        State { energy, exhausted: false, sprint_time: None, roll: None, land_cooldown: 0.0, unsupported_time: None,
            jump_count: 0, tumble: None }
    }
    fn thrown() -> Tumble {
        Tumble { phase: TumblePhase::Tumble, elapsed: 0.4, air_time: 0.25, angle: -1.5, spin_rate: 7.25, front: true,
            bounce: false, fly_dir: 3, heading_offset: 0.125, cycle_phase: 0.5, dead: true }
    }
    fn bits(state: &State) -> Vec<u8> {
        let (bytes, length) = suffix(state).unwrap();
        bytes[..length].to_vec()
    }

    #[test]
    fn every_movement_state_round_trips_exactly() {
        let sprint = State { sprint_time: Some(0.125), jump_count: 7, ..idle(0.0) };
        let roll = State { roll: Some(Roll { direction: RollDirection::Left, elapsed: 0.2 }), land_cooldown: 0.3, ..idle(79.5) };
        let tired = State { exhausted: true, roll: Some(Roll { direction: RollDirection::Forward, elapsed: 0.0 }), ..idle(-1.0) };
        let falling = State { unsupported_time: Some(0.05), jump_count: u32::MAX, ..idle(31.25) };
        let tumbling = State { tumble: Some(thrown()), exhausted: true, ..idle(-15.0) };
        let everything = State { sprint_time: Some(0.5), unsupported_time: Some(0.0), tumble: Some(thrown()), ..roll };
        for (state, body, length) in [(idle(31.25), STANDING, 21), (sprint, STANDING, 29), (roll, CROUCHED, 29), (tired, CROUCHED, 29),
            (falling, Body { grounded: false, air: 2, ..STANDING }, 29), (tumbling, STANDING, 72), (everything, STANDING, MAX_SUFFIX)]
        {
            assert_eq!(suffix_len(&state), length);
            let mut out = Vec::new();
            write_suffix(&state, &mut out).unwrap();
            assert_eq!(out.len(), length);
            let back = read_suffix(&out, &tuning(), body).unwrap();
            assert_eq!(bits(&back), out, "{state:?}");
            assert_eq!(format!("{back:?}"), format!("{state:?}"));
        }
        // A state with a number no step writes is not written.
        for bad in [idle(f64::NAN), State { land_cooldown: f64::INFINITY, ..idle(1.0) },
            State { tumble: Some(Tumble { angle: f64::NAN, ..thrown() }), ..idle(1.0) }]
        {
            assert!(suffix(&bad).is_err(), "{bad:?}");
        }
    }

    // Every movement state, with and without a power lock after it: the
    // state's length comes from its moves byte, so the lock splits off whole;
    // a tail of any other length is refused.
    #[test]
    fn a_power_lock_follows_any_movement_state_and_splits_off_whole() {
        let roll = State { roll: Some(Roll { direction: RollDirection::Right, elapsed: 0.1 }), ..idle(5.0) };
        let everything = State { sprint_time: Some(0.5), unsupported_time: Some(0.0), tumble: Some(thrown()), ..roll };
        for state in [idle(0.0), roll, State { tumble: Some(thrown()), ..idle(1.0) }, everything] {
            for lock in [None, Some((0, 0)), Some((511, 7)), Some((u16::MAX, u32::MAX))] {
                let mut out = Vec::new();
                write_suffix(&state, &mut out).unwrap();
                assert_eq!(state_len(&out), Some(out.len()));
                write_lock(lock, &mut out);
                let (moves, back) = split_lock(&out).unwrap();
                assert_eq!((moves.len(), back), (suffix_len(&state), lock));
                for cut in 1..LOCK_BYTES {
                    let mut short = out.clone();
                    short.truncate(suffix_len(&state) + LOCK_BYTES - cut);
                    if lock.is_some() { assert!(split_lock(&short).is_err(), "{cut}"); }
                }
                let mut long = out.clone();
                long.push(0);
                assert!(split_lock(&long).is_err());
            }
        }
        assert!(split_lock(&[0; 8]).is_err());
    }

    #[test]
    fn the_display_field_carries_energy_the_latch_and_the_roll_start() {
        let t = tuning();
        let idle = idle(40.0);
        let (bytes, n) = display(&idle, &t, 100, 0.0);
        assert_eq!(n, 3);
        assert_eq!(read_display(&bytes[..n]).unwrap(), Display { energy: 32768.0 / 65535.0, sprint: false, exhausted: false, roll: None });
        // A debt shows an empty bar and the latch.
        let (bytes, n) = display(&State { energy: -4.0, exhausted: true, ..idle }, &t, 100, 0.0);
        assert_eq!(read_display(&bytes[..n]).unwrap(), Display { energy: 0.0, sprint: false, exhausted: true, roll: None });
        // Three ticks into a roll that started at tick 98, facing a quarter turn.
        let roll = Roll { direction: RollDirection::Right, elapsed: 3.0 / 60.0 };
        let (bytes, n) = display(&State { roll: Some(roll), ..idle }, &t, 100, std::f64::consts::FRAC_PI_2);
        assert_eq!(n, 9);
        assert_eq!(read_display(&bytes[..n]).unwrap().roll, Some((RollDirection::Right, 98, 16384)));
        // The start tick wraps with the authority's tick counter.
        let (bytes, _) = display(&State { roll: Some(Roll { elapsed: 5.0 / 60.0, ..roll }), ..idle }, &t, 2, -std::f64::consts::FRAC_PI_2);
        assert_eq!(read_display(&bytes).unwrap().roll, Some((RollDirection::Right, u32::MAX - 1, 49152)));
        for bad in [&bytes[..4], &[0, 0, 16][..], &[0, 0, 2][..], &[0, 0, 3, 0, 0, 0, 0, 0, 0][..], &[0, 0, 9][..]] {
            assert!(read_display(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn impossible_or_out_of_class_values_are_refused() {
        let t = tuning();
        assert!(read_suffix(&bits(&idle(10.0)), &t, STANDING).is_ok());
        let edit = |state: &State, at: usize, value: u8| { let mut b = bits(state); b[at] = value; b };
        let number = |state: &State, at: usize, value: f64| { let mut b = bits(state); b[at..at + 8].copy_from_slice(&value.to_le_bytes()); b };
        let tumbling = State { tumble: Some(thrown()), ..idle(10.0) };
        let roll = State { roll: Some(Roll { direction: RollDirection::Left, elapsed: 0.2 }), ..idle(10.0) };
        for (bytes, body) in [
            (bits(&idle(81.0)), STANDING),                          // above capacity
            (bits(&idle(-15.5)), STANDING),                         // below the class's floor
            (number(&idle(1.0), 0, f64::NAN), STANDING),
            (edit(&idle(10.0), 8, 8), STANDING),                    // combat bit before it exists
            (bits(&State { sprint_time: Some(0.6), ..idle(10.0) }), STANDING), // sprint time beyond its ramp
            (bits(&State { roll: Some(Roll { direction: RollDirection::Left, elapsed: t.roll_seconds() + 1e-9 }), ..idle(10.0) }), CROUCHED),
            (bits(&State { roll: Some(Roll { direction: RollDirection::Left, elapsed: -0.01 }), ..idle(10.0) }), CROUCHED),
            (number(&idle(10.0), 9, -0.1), STANDING),               // a negative landing lock
            (bits(&State { unsupported_time: Some(-1.0), ..idle(10.0) }), STANDING),
            (edit(&tumbling, 21, 0), STANDING),                     // no such tumble phase
            (edit(&tumbling, 21, 6), STANDING),
            (edit(&tumbling, 22, 8), STANDING),                     // an unknown tumble flag
            (edit(&tumbling, 23, 4), STANDING),                     // no such fly direction
            (bits(&idle(10.0)), Body { posture: PRONE, ..STANDING }), // no 2005 prone
            (bits(&idle(10.0)), Body { diving: true, ..STANDING }),
            ([bits(&idle(10.0)), vec![0]].concat(), STANDING),      // trailing bytes
            (bits(&idle(10.0))[..20].to_vec(), STANDING),           // short
            (edit(&idle(10.0), 8, TUMBLING), STANDING),             // a tumble with no bytes
        ] {
            assert!(read_suffix(&bytes, &t, body).is_err(), "{bytes:?} {body:?}");
        }
        // A class without a roll action can carry no roll.
        let no_roll = Tuning { roll_frames: 0, ..t };
        assert!(read_suffix(&bits(&roll), &no_roll, CROUCHED).is_err());
        assert_eq!(compact_len(&[0]), Some(124));
        assert_eq!(compact_len(&[3]), Some(149));
        assert_eq!(compact_len(&[4]), None);
    }
}
