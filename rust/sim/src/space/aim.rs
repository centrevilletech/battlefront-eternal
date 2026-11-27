//! How a bot aims a ship's gun in the 2005 game.
//!
//! A gun that fires on its own (`PilotType = self`) and a gun a bot works
//! from a turret chair are both driven by the game's stationary bot
//! controller. Each tick it turns the gun, at the gun's own rates, toward the
//! target's lead point moved by the target's sphere of miss: the same sphere
//! the game's soldier bots aim with. Each pilot or crew carries that sphere for
//! the craft it rides; it grows with the craft's size, its speed and sudden
//! changes of its velocity, and shrinks with the skill of the bots shooting at
//! it. The shooting side contributes only a direction that drifts smoothly
//! over the sphere. The weapon scales the result by its `AIBubble*` keys. A
//! target with nobody human aboard takes three tenths of it. A ship object has
//! no controller and no sphere: it is aimed at exactly.

use crate::ai::combat::aim::AimMiss;
use three::Vector3;

use super::combat::SpaceRng;
use super::flight::{SpaceFighter, SpaceFlightProfile, SpaceFlightState};

/// A weapon's three bot aim keys.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MissBubble {
    /// `AIBubbleSizeMultiplier`: the share of the target's sphere taken.
    pub size: f64,
    /// `AIBubbleScaleDistDivider`: metres over which the sphere grows by one.
    pub divider: f64,
    /// `AIBubbleScaleClamp`: the most the distance may grow it.
    pub clamp: f64,
}

/// The weapon class's values when the keys are absent.
pub const RELEASED_MISS_BUBBLE: MissBubble = MissBubble { size: 1.0, divider: 20.0, clamp: 10.0 };

/// The object kind a craft hands the sphere: a flyer's doubles its radius.
pub const CRAFT_SPHERE_KIND: f64 = 2.0;

/// The share of the sphere a target with nobody human aboard takes.
pub const BOT_TARGET_SHARE: f64 = 0.3;

/// The opposing skill a sphere shrinks by when the battle has no bot
/// settings: the released medium enemy skill.
pub const RELEASED_MEDIUM_SKILL: f64 = 14.0;

/// How high above a target that is not flying the offset may reach: a draw
/// between 2 and 3 metres.
pub fn offset_ceiling(rng: &mut SpaceRng) -> f64 {
    2.0 + rng.next_unit()
}

/// Advances a craft's sphere of miss by one tick: the craft's velocity, top
/// speed and model sphere, its pilot's opponents' skill. A craft nobody rides
/// has none.
pub fn step_craft_sphere(sphere: &mut AimMiss, f: &SpaceFighter, p: &SpaceFlightProfile, opposing_skill: f64, dt: f64, rng: &mut SpaceRng) {
    if !f.alive || !f.occupied() {
        *sphere = AimMiss::default();
        return;
    }
    let draw = draw_box(rng);
    let velocity = Vector3::new(f.velocity[0], f.velocity[1], f.velocity[2]);
    sphere.step(dt, &velocity, p.max_speed, craft_bound(p), CRAFT_SPHERE_KIND, true, opposing_skill, draw, 0);
}

/// The radius a craft hands its sphere of miss: a quarter of its box's
/// diagonal across (length and width, not height), as the 2005 game takes it
/// from the model's bounds and keeps it for the class.
pub fn craft_bound(p: &SpaceFlightProfile) -> f64 {
    BOUND_SHARE * v8math::hypot(p.bounds_max[0] - p.bounds_min[0], p.bounds_max[2] - p.bounds_min[2])
}

/// The share of a box's diagonal across that the game takes as its radius.
pub const BOUND_SHARE: f64 = 0.25;

/// Advances the drifting direction of a gun a bot works by one tick (the gun
/// stands still). A gun with no bot at it has none.
pub fn step_gun_sphere(sphere: &mut AimMiss, worked: bool, opposing_skill: f64, dt: f64, rng: &mut SpaceRng) {
    if !worked {
        *sphere = AimMiss::default();
        return;
    }
    let draw = draw_box(rng);
    sphere.step(dt, &Vector3::default(), 0.0, 0.0, 0.0, false, opposing_skill, draw, 0);
}

/// One draw in [-1, 1) per axis, the third axis first.
fn draw_box(rng: &mut SpaceRng) -> (f64, f64, f64) {
    let z = (rng.next_unit() - 0.5) * 2.0;
    let y = (rng.next_unit() - 0.5) * 2.0;
    let x = (rng.next_unit() - 0.5) * 2.0;
    (x, y, z)
}

/// How much the sphere grows at `distance`: `distance / divider + 0.5`, held
/// within [1, clamp] from half the divider on.
pub fn distance_factor(distance: f64, bubble: MissBubble) -> f64 {
    if !(bubble.divider > 0.0) {
        return 1.0;
    }
    let raw = distance / bubble.divider + 0.5;
    if distance >= bubble.divider * 0.5 {
        v8math::min(v8math::max(raw, 1.0), v8math::max(1.0, bubble.clamp))
    } else {
        raw
    }
}

/// The point a bot turns a gun at `from` toward: `lead` moved along the gun's
/// drifting direction `dir` by the target's sphere `radius`, times the
/// weapon's share, the bot-target share and the distance factor. A target
/// that is not flying keeps the offset above it, at most `ceiling` up.
#[allow(clippy::too_many_arguments)]
pub fn miss_point(from: [f64; 3], lead: [f64; 3], dir: Vector3, radius: f64, human_aboard: bool, flying: bool, ceiling: f64, bubble: MissBubble) -> [f64; 3] {
    if !(radius > 0.0) {
        return lead;
    }
    let mut offset = [dir.x * radius * bubble.size, dir.y * radius * bubble.size, dir.z * radius * bubble.size];
    if !flying {
        offset[1] = v8math::min(v8math::max(offset[1], 0.0), ceiling);
    }
    if !human_aboard {
        offset = offset.map(|v| v * BOT_TARGET_SHARE);
    }
    let distance = v8math::hypot3(lead[0] - from[0], lead[1] - from[1], lead[2] - from[2]);
    let k = distance_factor(distance, bubble);
    [lead[0] + offset[0] * k, lead[1] + offset[1] * k, lead[2] + offset[2] * k]
}

/// Whether a craft is flying, for the offset's floor.
pub fn craft_flying(f: &SpaceFighter) -> bool {
    f.state == SpaceFlightState::Flying
}

/// Whether anyone human rides the craft.
pub fn human_aboard(f: &SpaceFighter) -> bool {
    f.pilot.is_some_and(|p| p.is_human) || f.gunners.iter().flatten().any(|g| g.is_human)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUBBLE: MissBubble = MissBubble { size: 0.5, divider: 40.0, clamp: 4.0 };

    #[test]
    fn a_craft_hands_its_sphere_a_quarter_of_its_box_diagonal_across() {
        let p = SpaceFlightProfile { bounds_min: [-7.0, -3.0, -4.5], bounds_max: [5.0, 9.0, 11.5], ..Default::default() };
        // 12 m wide and 16 m long: 20 m across, whatever its height.
        assert_eq!(craft_bound(&p), 5.0);
    }

    #[test]
    fn the_sphere_grows_with_distance_from_half_the_divider_and_stops_at_the_clamp() {
        assert_eq!(distance_factor(10.0, BUBBLE), 10.0 / 40.0 + 0.5);
        assert_eq!(distance_factor(20.0, BUBBLE), 1.0);
        assert_eq!(distance_factor(60.0, BUBBLE), 2.0);
        assert_eq!(distance_factor(400.0, BUBBLE), 4.0);
    }

    #[test]
    fn a_gun_aims_off_its_lead_by_the_targets_sphere_and_by_less_at_a_bot() {
        let up = Vector3::new(0.0, 1.0, 0.0);
        let from = [0.0, 0.0, 0.0];
        let lead = [0.0, 0.0, 100.0];
        // 6 m of sphere, half of it for this weapon, 3.0 times at 100 m.
        let human = miss_point(from, lead, up, 6.0, true, true, 2.5, BUBBLE);
        assert!((human[1] - 9.0).abs() < 1e-9, "{human:?}");
        let bot = miss_point(from, lead, up, 6.0, false, true, 2.5, BUBBLE);
        assert!((bot[1] - 9.0 * BOT_TARGET_SHARE).abs() < 1e-9, "{bot:?}");
        // No sphere, no offset: a ship object is aimed at exactly.
        assert_eq!(miss_point(from, lead, up, 0.0, true, true, 2.5, BUBBLE), lead);
    }

    #[test]
    fn a_target_that_is_not_flying_keeps_the_offset_above_it() {
        let down = Vector3::new(0.0, -1.0, 0.0);
        let lead = [0.0, 0.0, 100.0];
        assert_eq!(miss_point([0.0; 3], lead, down, 6.0, true, false, 2.5, BUBBLE)[1], 0.0);
        let up = Vector3::new(0.0, 1.0, 0.0);
        assert_eq!(miss_point([0.0; 3], lead, up, 6.0, true, false, 2.5, BUBBLE)[1], 2.5 * 3.0);
    }
}
