//! Animated prop clocks shared by the world's props (rust/sim
//! battle/animated.rs) and the page's prop presentation
//! (bfe-presentation props.rs): AnimatedProp.advance, stand, a door's
//! openness and the trigger reach of src/props.js. Each host keeps its own
//! control flow (the world seizes and swallows; the page only draws), and
//! both reach these numerics, so a door or an idle drawn on the page moves
//! exactly as the world's.
use crate::{Clip, Pose};

/// One world tick.
pub const TICK: f64 = 1.0 / 60.0;
// A soldier's collision sphere: its centre above his feet and its radius
// (src/units.js), for the trigger region test.
const SOLDIER_MIDDLE: f64 = 1.048;
const SOLDIER_RADIUS: f64 = 1.278;

/// Math.min with the world's canonical NaN bits, as rust/sim writes them.
pub fn min(a: f64, b: f64) -> f64 {
    let value = v8math::min(a, b);
    if value.is_nan() {
        f64::from_bits(0xfff8_0000_0000_0000)
    } else {
        value
    }
}
/// Math.max with the world's canonical NaN bits.
pub fn max(a: f64, b: f64) -> f64 {
    let value = v8math::max(a, b);
    if value.is_nan() {
        f64::from_bits(0xfff8_0000_0000_0000)
    } else {
        value
    }
}

/// props.js length(): the original game's length of a clip, its frames
/// at 30 a second (one frame past the port's duration).
pub fn length(clip: Option<&Clip>) -> f64 {
    clip.map_or(0.0, |c| c.frames as f64 / c.fps)
}

/// props.js within(): a soldier with his feet at `feet` is within
/// `reach` of `point` as the 2004 game's region query has it, his
/// collision sphere's box against the box `reach` round the point.
pub fn within(point: [f64; 3], reach: f64, feet: [f64; 3]) -> bool {
    let r = reach + SOLDIER_RADIUS;
    (feet[0] - point[0]).abs() <= r
        && (feet[1] + SOLDIER_MIDDLE - point[1]).abs() <= r
        && (feet[2] - point[2]).abs() <= r
}

/// AnimatedProp.step's door: its openness after `dt`, opening while
/// anyone's feet are near its trigger and closing otherwise, over the
/// open clip's length (`duration`, when it has one; else a second).
pub fn door_openness(openness: f64, near: bool, dt: f64, duration: Option<f64>) -> f64 {
    let span = duration.map_or(1.0, |d| max(d, 1e-3));
    max(0.0, min(1.0, openness + (if near { dt } else { -dt }) / span))
}

/// AnimatedProp.advance's clock: the time by `dt`, an action once, the
/// idle looped, held at its end for IdleDelay before it starts again.
/// Returns the step the pose eases by (0 while held).
pub fn advance(
    time: &mut f64,
    hold: &mut f64,
    looping: bool,
    dt: f64,
    idle_span: f64,
    idle_delay: f64,
) -> f64 {
    let mut step = dt;
    if !looping {
        *time += dt;
    } else {
        if *time + dt < idle_span {
            *time += dt;
        } else {
            *hold += dt;
            if *hold > idle_delay {
                *hold = 0.0;
                *time += dt;
            } else {
                step = 0.0;
            }
        }
        if idle_span > 0.0 && *time > idle_span {
            *time -= idle_span;
        }
    }
    step
}

/// AnimatedProp.stand: the pose eased toward the clip's at `time`, at 60
/// steps a second `weight` of the way each step (a frame of `dt` takes the
/// share as many 60 Hz steps would), all of it once the weight is 1. The
/// stand pose is Float32Array storage in the JavaScript: every store here
/// rounds to f32 at the same point.
#[allow(clippy::too_many_arguments)]
pub fn stand(
    stand_q: &mut [f32],
    stand_t: &mut [f32],
    pose: &mut Pose,
    clip: Option<&Clip>,
    time: f64,
    looping: bool,
    weight: f64,
    dt: f64,
) {
    pose.clear();
    if let Some(clip) = clip {
        pose.add(clip, time, 1.0, looping);
    }
    let share = if weight >= 1.0 {
        1.0
    } else if dt > 0.0 {
        1.0 - v8math::pow(1.0 - max(weight, 0.0), dt / TICK)
    } else {
        weight
    };
    let (q, t) = (stand_q, stand_t);
    for i in 0..pose.w.len() {
        let w = pose.w[i] as f64;
        if w <= 1e-6 {
            continue;
        }
        let qi = i * 4;
        let ti = i * 3;
        let mut target = [
            pose.q[qi] as f64,
            pose.q[qi + 1] as f64,
            pose.q[qi + 2] as f64,
            pose.q[qi + 3] as f64,
        ];
        // The shorter way round: the clip's sign flipped onto the stand's.
        if q[qi] as f64 * target[0]
            + q[qi + 1] as f64 * target[1]
            + q[qi + 2] as f64 * target[2]
            + q[qi + 3] as f64 * target[3]
            < 0.0
        {
            for x in &mut target {
                *x = -*x;
            }
        }
        for k in 0..4 {
            q[qi + k] = (q[qi + k] as f64 + (target[k] - q[qi + k] as f64) * share) as f32;
        }
        let len = (q[qi] as f64 * q[qi] as f64
            + q[qi + 1] as f64 * q[qi + 1] as f64
            + q[qi + 2] as f64 * q[qi + 2] as f64
            + q[qi + 3] as f64 * q[qi + 3] as f64)
            .sqrt();
        // The JavaScript's `|| 1`: a zero or NaN length divides by one.
        let len = if len == 0.0 || len.is_nan() { 1.0 } else { len };
        for k in 0..4 {
            q[qi + k] = (q[qi + k] as f64 / len) as f32;
        }
        for k in 0..3 {
            t[ti + k] = (t[ti + k] as f64 + (pose.t[ti + k] as f64 / w - t[ti + k] as f64) * share) as f32;
        }
    }
}
