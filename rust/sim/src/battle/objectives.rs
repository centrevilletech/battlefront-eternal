//! Destructible command posts (src/objectives.js): a post's KillRegion as
//! its building is destroyed. 15 explosion, med_explosion or small_explosion
//! effects at points drawn through the region's box (each axis uniform over
//! its extent: 60 draws of the gameplay stream), then every soldier and
//! vehicle whose origin is inside dies (at most 64), no one credited.

use super::regions::{build_region, in_region};
use crate::gen::{Built, RegionDesc};
use three::{Matrix4, Vector3};

pub const KILL_REGION_BLASTS: usize = 15;
pub const BLASTS: [&str; 3] = ["explosion", "med_explosion", "small_explosion"];

/// What a kill region asks of the battle (killRegion's options): its
/// random draws (`random()`), an effect played, the victims to test (asked
/// once, after the blasts: where each one's origin is) and a kill.
pub trait KillRegionHost {
    fn random(&mut self) -> f64;
    fn play(&mut self, name: &str, point: Vector3);
    fn victims(&mut self) -> Vec<Vector3>;
    fn victims_into(&mut self, into: &mut Vec<Vector3>) {
        into.clear(); into.extend(self.victims());
    }
    /// Membership is frozen, but source positions are object references: an
    /// earlier death can move a later seated body before its containment test.
    fn position(&mut self, _victim: usize, sampled: Vector3) -> Vector3 { sampled }
    fn kill(&mut self, victim: usize);
}

/// killRegion(region, { random, play, victims }) (objectives.js:91): the
/// effects' points (`random()` draws), then whoever stands inside dies.
/// Returns how many died.
pub fn kill_region(region: Option<&RegionDesc>, host: &mut dyn KillRegionHost) -> usize {
    let Some(region) = region else { return 0 };
    let built = build_region(region);
    kill_region_built(&built, host, &mut Vec::new())
}

/// The authority retains both its cooked region and victim-position buffer.
/// Effects still precede victim selection, and selection precedes every death.
pub fn kill_region_built(built: &Built, host: &mut dyn KillRegionHost, points: &mut Vec<Vector3>) -> usize {
    let mut matrix = Matrix4::default();
    matrix.from_array(&built.inverse, 0);
    matrix.invert();
    let [a, b, c] = built.half;
    for _ in 0..KILL_REGION_BLASTS {
        let x = (host.random() * 2.0 - 1.0) * a;
        let y = (host.random() * 2.0 - 1.0) * b;
        let z = (host.random() * 2.0 - 1.0) * c;
        let mut point = Vector3::new(x, y, z);
        point.apply_matrix4(&matrix);
        let pick = v8math::floor(host.random() * 3.0) % 3.0;
        host.play(BLASTS[pick as usize], point);
    }
    let mut killed = 0;
    host.victims_into(points);
    for (i, at) in points.iter().enumerate() {
        if killed >= 64 {
            break;
        }
        let at = host.position(i, *at);
        if !in_region(built, &at) {
            continue;
        }
        host.kill(i);
        killed += 1;
    }
    killed
}
