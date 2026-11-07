//! CameraShake from src/camera-shake.js. Four shakes survive between frames;
//! the PC game's duplicated explosion view remains two admissions in order.
// Camera shake, as the 2004 game shakes the view (docs/formats.md,
// "Camera shake"). It is the page's: an explosion shakes the view of the
// player whose camera follows something near it, where the page shows the
// explosion. The world keeps none of it.
//
// - Explode, after the blast has been dealt:
// what the camera follows (his soldier, the hull he drives, the recon
// droid he flies) is d metres from the blast at its origin, r its bounding
// sphere's radius. Past 0.0001 m, f = falloff(d - r / 2, ShakeRadiusInner,
// ShakeRadiusOuter), and with f > 0 the camera takes a shake of
// Shake x f that dies away over ShakeLength x f seconds (both also times
// the class's ShakeScale, which no stock class sets: 1). The PC build
// asks twice: its loop over the two split-screen views reads view 0 both
// times, so each explosion takes two shakes.
// - The camera holds at most four shakes; one more is dropped.
// - Each frame the shakes' amounts are summed, then each falls by dt x its
// amount / its length, and one at 0 is gone.
// - The view then turns about each of its own axes by (2r - 1)^3 x 0.1 x
// the sum radians, r random in 0..1. The death camera is not shaken.
// The share of an explosion's shake for a thing `distance` metres from it
// at its origin, with a bounding sphere of `radius`.
// One shake of `amount`, gone after `length` seconds.
// An explosion at `position` (an array or a vector) as seen by what the
// camera follows: { position, radius }. Returns the share it shook by.
// The frame's sum, then the decay.
// The frame: sum and decay, and the camera turned by the sum unless it
// is the death camera's (`still`). Paused (dt 0), nothing moves.

use super::{math::*, Camera, Cast};
use three::Vector3 as V;
#[derive(Default)]
pub struct Shake {
    amounts: [f64; 4],
    rates: [f64; 4],
    count: usize,
    pub total: f64,
    pub refused: u64,
}
impl Shake {
    pub fn clear(&mut self) {
        self.count = 0;
        self.total = 0.0;
    }
    pub fn add(&mut self, amount: f64, length: f64) -> bool {
        if !(amount > 0.0) {
            return false;
        }
        if self.count == 4 {
            self.refused += 1;
            return false;
        }
        self.amounts[self.count] = amount;
        self.rates[self.count] = if length > 0.0 {
            amount / length
        } else {
            f64::INFINITY
        };
        self.count += 1;
        true
    }
    pub fn explosion<M: Math>(
        &mut self,
        position: V,
        target: V,
        radius: f64,
        amount: f64,
        length: f64,
        inner: f64,
        outer: f64,
    ) -> f64 {
        let d = sub(target, position);
        let distance = M::hypot(d.x, d.y, d.z);
        if !(amount > 0.0 && outer > 0.0 && distance > 1e-4) {
            return 0.0;
        }
        let d = distance - radius / 2.0;
        let share = if d < inner {
            1.0
        } else if d < outer && outer > inner {
            (outer - d) / (outer - inner)
        } else {
            0.0
        };
        if !(share > 0.0) {
            return 0.0;
        }
        for _ in 0..2 {
            self.add(amount * share, length * share);
        }
        share
    }
    pub fn frame<M: Math, C: Cast>(
        &mut self,
        cam: &mut Camera,
        dt: f64,
        still: bool,
        random: &mut C,
    ) {
        if !(dt > 0.0) || self.count == 0 {
            return;
        }
        let mut total = 0.0;
        let mut out = 0;
        // Sum before decay, compacting survivors in their original order.
        // This is CameraShake.update's splice loop without moving storage.
        for i in 0..self.count {
            total += self.amounts[i];
            let amount = self.amounts[i] - dt * self.rates[i];
            if !(amount <= 0.0) {
                self.amounts[out] = amount;
                self.rates[out] = self.rates[i];
                out += 1;
            }
        }
        self.count = out;
        self.total = total;
        if still || !(total > 0.0) {
            return;
        }
        for v in [V::new(1.0, 0.0, 0.0), UP, V::new(0.0, 0.0, 1.0)].iter() {
            let r = 2.0 * random.random() - 1.0;
            let turn = M::pow(r, 3.0) * total * 0.1;
            cam.q = mul(cam.q, axis::<M>(*v, turn));
        }
    }
    pub fn active(&self) -> bool {
        self.count > 0
    }
}
