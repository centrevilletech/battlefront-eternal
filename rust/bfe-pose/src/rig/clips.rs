//! What the simulation reads of an animation clip besides its samples
//! (src/anim.js Clip): its duration, its stride (the ground a locomotion
//! cycle's DummyRoot travels) and its own speed; and the two soldier timings
//! made from clip durations (src/units.js buildBody).

use v8math::hypot as js_hypot2;

/// A clip's timings, as the Clip constructor works them out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipTimes {
    /// `Math.max(frames - 1, 0) / fps`.
    pub duration: f64,
    /// `Math.hypot(travel[0], travel[2])`.
    pub stride: f64,
    /// `duration > 0 ? stride / duration : 0`.
    pub speed: f64,
}

impl ClipTimes {
    /// From the clip's description: its frame count, the bank's frame rate
    /// and its root's travel over the clip.
    pub fn new(frames: f64, fps: f64, travel: [f64; 3]) -> ClipTimes {
        let duration = crate::js_max(frames - 1.0, 0.0) / fps;
        let stride = js_hypot2(travel[0], travel[2]);
        let speed = if duration > 0.0 {
            stride / duration
        } else {
            0.0
        };
        ClipTimes {
            duration,
            stride,
            speed,
        }
    }
}

/// The share of the dive clip a dive lasts (src/soldier.js ENGINE.diveLength).
pub const DIVE_LENGTH: f64 = 0.75;

/// A soldier's `getupTime`: the prone get-up clip's duration
/// (`human_rifle_prone_getup_stand`), 0 without it.
pub fn getup_time(getup: Option<&ClipTimes>) -> f64 {
    getup.map_or(0.0, |c| c.duration)
}

/// A soldier's `diveTime`: three quarters of the forward dive clip
/// (`human_rifle_diveforward`), 0 without it.
pub fn dive_time(dive: Option<&ClipTimes>) -> f64 {
    DIVE_LENGTH * dive.map_or(0.0, |c| c.duration)
}
