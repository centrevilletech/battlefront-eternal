//! Destructible command posts (src/objectives.js): a post's KillRegion as
//! its building is destroyed. 15 explosion, med_explosion or small_explosion
//! effects at points drawn through the region's box (each axis uniform over
//! its extent: 60 draws of the gameplay stream), then every soldier and
//! vehicle whose origin is inside dies (at most 64), no one credited.

use super::regions::{build_region, in_region, Region};
use three::Vector3;

pub const KILL_REGION_BLASTS: usize = 15;
pub const BLASTS: [&str; 3] = ["explosion", "med_explosion", "small_explosion"];

/// killRegion(region, { random, play, victims }) (objectives.js:91): the
/// effects' points (`random()` draws), then whoever stands inside dies
/// (`kill(i)`, the victims' places). Returns how many died.
pub fn kill_region(
    region: Option<&Region>,
    mut random: impl FnMut() -> f64,
    mut play: impl FnMut(&str, Vector3),
    victims: &[Vector3],
    mut kill: impl FnMut(usize),
) -> usize {
    let Some(region) = region else { return 0 };
    let built = build_region(region);
    let mut matrix = built.inverse;
    matrix.invert();
    let [a, b, c] = built.half;
    for _ in 0..KILL_REGION_BLASTS {
        let x = (random() * 2.0 - 1.0) * a;
        let y = (random() * 2.0 - 1.0) * b;
        let z = (random() * 2.0 - 1.0) * c;
        let mut point = Vector3::new(x, y, z);
        point.apply_matrix4(&matrix);
        let pick = v8math::floor(random() * 3.0) % 3.0;
        play(BLASTS[pick as usize], point);
    }
    let mut killed = 0;
    for (i, at) in victims.iter().enumerate() {
        if killed >= 64 {
            break;
        }
        if !in_region(&built, at) {
            continue;
        }
        kill(i);
        killed += 1;
    }
    killed
}
