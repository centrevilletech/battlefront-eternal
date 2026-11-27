//! A 2005 class's movement numbers as the words of one admission call, so
//! a recorded admission carries them: a host that admits a world call by
//! call registers exactly the numbers the original admission used.
//!
//! The order is fixed: the seven control rows (stand, crouch, sprint, jump,
//! jet, roll, tumble), each forward, strafe and turn; the jetpack (present,
//! jump, push, recharge, cost, initial cost, minimum border, hover, then
//! acceleration, the energy one ignition costs and the Force jump flag); then
//! the scalars in the struct's order, with the sprint jump cost as present and
//! value; then the roll's frame count; then the tumble recovery flag and the
//! two stand-up lengths. A flag is exactly 0 or 1 and an absent value is
//! written as 0.
use super::{ControlSpeed, Jet, Jetpack, Tuning};

/// Words one class's numbers take.
pub const WORDS: usize = 7 * 3 + 11 + 8 + 2 + 5 + 1 + 3;

/// The numbers' words, in the documented order.
pub fn put(t: &Tuning) -> [f64; WORDS] {
    let mut out = [0.0; WORDS];
    let mut at = 0;
    let mut push = |value: f64| { out[at] = value; at += 1; };
    for row in [t.stand, t.crouch, t.sprint, t.jump, t.jet, t.roll, t.tumble] {
        push(row.forward);
        push(row.strafe);
        push(row.turn);
    }
    match t.jetpack {
        Some(pack) => {
            let j = pack.physics;
            for value in [1.0, j.jump, j.push, j.recharge, j.cost, j.initial, j.min_border, f64::from(u8::from(j.hover)), pack.acceleration,
                pack.energy_cost, f64::from(u8::from(pack.force_jump))]
            {
                push(value);
            }
        }
        None => for _ in 0..11 { push(0.0); },
    }
    for value in [t.sprint_accelerate_time, t.energy_capacity, t.energy_floor, t.energy_restore, t.energy_restore_idle,
        t.energy_drain_sprint, t.energy_min_sprint, t.energy_cost_jump]
    {
        push(value);
    }
    match t.energy_cost_jump_sprint {
        Some(cost) => { push(1.0); push(cost); }
        None => { push(0.0); push(0.0); }
    }
    for value in [t.energy_cost_roll, t.jump_height, t.jump_forward_speed_factor, t.jump_strafe_speed_factor, t.roll_speed_factor] {
        push(value);
    }
    push(f64::from(t.roll_frames));
    push(f64::from(u8::from(t.recover_from_tumble)));
    push(t.getup_front_seconds);
    push(t.getup_back_seconds);
    out
}

// Exactly +0 or 1: a negative zero is no writer's flag.
fn flag(value: f64) -> Option<bool> {
    match value.to_bits() { 0 => Some(false), bits if bits == 1f64.to_bits() => Some(true), _ => None }
}

/// The numbers `put` wrote, or None for any other words: a wrong length, a
/// flag other than 0 or 1, a nonzero absent value, a frame count that is not
/// a whole u32, or numbers the movement code refuses.
pub fn take(words: &[f64]) -> Option<Tuning> {
    if words.len() != WORDS { return None; }
    let row = |at: usize| ControlSpeed { forward: words[at], strafe: words[at + 1], turn: words[at + 2] };
    let absent = |range: std::ops::Range<usize>| words[range].iter().all(|&v| v.to_bits() == 0);
    let jetpack = if flag(words[21])? {
        let physics = Jet { jump: words[22], push: words[23], recharge: words[24], cost: words[25], initial: words[26],
            min_border: words[27], hover: flag(words[28])? };
        Some(Jetpack { physics, acceleration: words[29], energy_cost: words[30], force_jump: flag(words[31])? })
    } else if absent(22..32) {
        None
    } else {
        return None;
    };
    let energy_cost_jump_sprint = if flag(words[40])? { Some(words[41]) } else if absent(41..42) { None } else { return None };
    let frames = words[47];
    if !(frames.is_finite() && frames.is_sign_positive() && frames.fract() == 0.0 && frames <= f64::from(u32::MAX)) { return None; }
    let tuning = Tuning {
        stand: row(0), crouch: row(3), sprint: row(6), jump: row(9), jet: row(12), roll: row(15), tumble: row(18), jetpack,
        sprint_accelerate_time: words[32], energy_capacity: words[33], energy_floor: words[34], energy_restore: words[35],
        energy_restore_idle: words[36], energy_drain_sprint: words[37], energy_min_sprint: words[38], energy_cost_jump: words[39],
        energy_cost_jump_sprint, energy_cost_roll: words[42], jump_height: words[43], jump_forward_speed_factor: words[44],
        jump_strafe_speed_factor: words[45], roll_speed_factor: words[46], roll_frames: frames as u32,
        recover_from_tumble: flag(words[48])?, getup_front_seconds: words[49], getup_back_seconds: words[50],
    };
    tuning.validate().ok()?;
    Some(tuning)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tuning(jetpack: Option<Jetpack>, sprint_jump: Option<f64>) -> Tuning {
        let row = |a: f64| ControlSpeed { forward: a, strafe: a * 0.5, turn: a * 0.25 };
        Tuning { stand: row(1.0), crouch: row(0.7), sprint: row(1.5), jump: row(0.2), jet: row(0.4), jetpack, roll: row(0.1), tumble: row(0.05),
            sprint_accelerate_time: 0.35, energy_capacity: 80.0, energy_floor: -20.0, energy_restore: 6.0, energy_restore_idle: 9.0,
            energy_drain_sprint: 14.0, energy_min_sprint: 18.0, energy_cost_jump: 4.0, energy_cost_jump_sprint: sprint_jump,
            energy_cost_roll: 12.0, jump_height: 1.4, jump_forward_speed_factor: 1.05, jump_strafe_speed_factor: 0.95,
            roll_speed_factor: 1.6, roll_frames: 23, recover_from_tumble: true, getup_front_seconds: 1.25, getup_back_seconds: 0.9 }
    }
    fn pack(energy_cost: f64, force_jump: bool) -> Jetpack {
        Jetpack { physics: Jet { jump: 3.0, push: 7.0, recharge: 0.2, cost: 0.15, initial: 0.1, min_border: 0.25, hover: !force_jump },
            acceleration: 11.0, energy_cost, force_jump }
    }
    fn same(a: &Tuning, b: &Tuning) -> bool { put(a).iter().zip(put(b)).all(|(x, y)| x.to_bits() == y.to_bits()) }

    #[test]
    fn every_shape_of_numbers_comes_back_exactly() {
        for t in [tuning(None, None), tuning(None, Some(5.0)), tuning(Some(pack(0.0, false)), None),
            tuning(Some(pack(9.5, true)), Some(0.0))]
        {
            let back = take(&put(&t)).expect("round trip");
            assert!(same(&t, &back));
            assert_eq!(format!("{t:?}"), format!("{back:?}"));
        }
    }

    #[test]
    fn words_no_writer_makes_are_refused() {
        let base = put(&tuning(None, None));
        assert!(take(&base[..WORDS - 1]).is_none());
        assert!(take(&[base.as_slice(), &[0.0]].concat()).is_none());
        // A flag other than 0 or 1, a value under an absent flag, a negative
        // zero standing for an absent value, a fractional or huge frame count.
        for (at, value) in [(21, 0.5), (21, -0.0), (22, 1.0), (40, 2.0), (41, 3.0), (41, -0.0), (47, 2.5), (47, 1e12), (47, -1.0), (47, -0.0), (47, f64::NAN), (48, 0.5), (48, -0.0)] {
            let mut words = base;
            words[at] = value;
            assert!(take(&words).is_none(), "word {at} = {value}");
        }
        // Numbers the movement code refuses: a positive floor, a negative rate.
        for (at, value) in [(34, 1.0), (35, -1.0), (0, f64::INFINITY)] {
            let mut words = base;
            words[at] = value;
            assert!(take(&words).is_none(), "word {at} = {value}");
        }
        let jet = put(&tuning(Some(pack(0.0, false)), None));
        for (at, value) in [(28, 0.5), (31, 3.0), (31, -0.0)] {
            let mut words = jet;
            words[at] = value;
            assert!(take(&words).is_none(), "jet word {at} = {value}");
        }
    }
}
