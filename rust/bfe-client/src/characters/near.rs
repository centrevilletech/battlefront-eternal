//! The 2005 game's soldier near rule, in third-person views: a soldier whose torso point lies nearer along the
//! view than the near distance is not drawn, and within 1.2 m past it the soldier is drawn faded with that
//! distance, never under 0.06 opacity. The cloth and the weapon it holds are drawn with the soldier's faded
//! colour, so they fade with it: a camera pressed against a wall behind a caped hero sees through the cape.
//! First-person views skip the rule.

use crate::render_bridge::Bridge;
use bfe_render::instances as draw;

/// How far past the near distance a soldier's torso point is when it is drawn whole again, in metres.
const FADE: f64 = 1.2;
/// The least opacity a fading soldier is drawn with.
const FLOOR: f64 = 0.06;

/// A soldier's torso point above its feet: 1.4 m standing, 0.6 m crouched.
pub(super) fn torso(feet: [f64; 3], crouch: bool) -> [f64; 3] {
    [feet[0], feet[1] + if crouch { 0.6 } else { 1.4 }, feet[2]]
}

/// How far a soldier has faded: 0 drawn whole, 1 not drawn. `forward` is the view's unit direction, `near` its
/// near distance.
pub(super) fn fade(torso: [f64; 3], eye: [f64; 3], forward: [f64; 3], near: f64) -> f64 {
    let depth: f64 = (0..3).map(|i| (torso[i] - eye[i]) * forward[i]).sum();
    let past = depth - near;
    if past < 0. {
        1.
    } else if past < FADE {
        1. - (past / FADE).max(FLOOR)
    } else {
        0.
    }
}

/// The view's forward direction and near distance, from the frame's projection and view matrices
/// (column-major; the view's third row is the camera's back axis).
pub(super) fn view(projection: &[f64], view: &[f64; 16]) -> ([f64; 3], f64) {
    ([-view[2], -view[6], -view[10]], projection[14] / (projection[10] - 1.))
}

/// The blend each opaque program of a fading soldier draws with in the blended pass (ordinary alpha blending,
/// depth tested and written), made once per program and sidedness.
#[derive(Default)]
pub(super) struct Blends(Vec<(u32, bool, f64)>);

impl Blends {
    fn get(&mut self, bridge: &mut Bridge, program: u32, both_sides: bool) -> f64 {
        if let Some(&(_, _, id)) = self.0.iter().find(|b| b.0 == program && b.1 == both_sides) {
            return id;
        }
        let blend = [
            1,
            glow::FUNC_ADD,
            glow::FUNC_ADD,
            glow::SRC_ALPHA,
            glow::ONE_MINUS_SRC_ALPHA,
            glow::ONE,
            glow::ONE_MINUS_SRC_ALPHA,
        ];
        let passes = if both_sides {
            vec![program, 1, program, 0]
        } else {
            vec![program, 0]
        };
        let id = f64::from(bridge.inst_add_blend(&blend, 3, glow::LEQUAL, &passes));
        self.0.push((program, both_sides, id));
        id
    }

    /// Gives the records `first..end` a fading soldier wrote how far it has faded, moving its opaque ones to the
    /// blended pass with their own programs (alpha to coverage off: the fade is the coverage there).
    pub(super) fn apply(&mut self, bridge: &mut Bridge, first: usize, end: usize, fade: f64) {
        for r in first..end.min(draw::MAX_RECORDS) {
            let at = draw::IN_RECORDS + r * draw::REC_STRIDE;
            let (kind, program, flags) = {
                let row = &bridge.r.core.input[at..at + draw::REC_STRIDE];
                (row[draw::REC_KIND], row[draw::REC_PROGRAM] as u32, row[draw::REC_FLAGS] as u32)
            };
            let blended = match kind {
                1. => Some(draw::KIND_BLENDED),
                2. => Some(draw::KIND_BLENDED_RIGID),
                _ => None,
            };
            let blend = blended.map(|_| self.get(bridge, program, flags & 3 == 2));
            let row = &mut bridge.r.core.input[at..at + draw::REC_STRIDE];
            row[draw::REC_FADE] = fade;
            if let (Some(kind), Some(blend)) = (blended, blend) {
                row[draw::REC_KIND] = kind;
                row[draw::REC_BLEND] = blend;
                row[draw::REC_FLAGS] = f64::from(flags & !4);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AHEAD: [f64; 3] = [0., 0., -1.];

    #[test]
    fn a_soldier_fades_over_the_game_distances() {
        let eye = [0., 1.7, 0.];
        let at = |depth: f64| fade(torso([0., 0.3, -depth], false), eye, AHEAD, 0.3);
        assert_eq!(at(0.29), 1., "nearer than the near distance: not drawn");
        assert_eq!(at(1.5), 0., "1.2 m past the near distance: whole");
        assert_eq!(at(5.), 0.);
        assert!((at(0.9) - 0.5).abs() < 1e-12, "halfway: half faded, got {}", at(0.9));
        assert!((at(0.31) - (1. - FLOOR)).abs() < 1e-12, "just past the near distance: the least opacity");
        assert!((at(0.6) - 0.75).abs() < 1e-12);
    }

    #[test]
    fn a_crouched_soldier_measures_from_its_lower_torso() {
        let eye = [0., 1.0, 0.];
        let feet = [0., 0., -1.0];
        let down = {
            let (x, y, z) = (0., -0.6f64, -0.8f64);
            let n = (x * x + y * y + z * z).sqrt();
            [x / n, y / n, z / n]
        };
        assert!(fade(torso(feet, true), eye, down, 0.3) < fade(torso(feet, false), eye, down, 0.3));
    }

    #[test]
    fn the_view_gives_forward_and_near_distance() {
        let (near, far) = (0.3, 120.);
        let mut projection = [0.; 16];
        projection[10] = -(far + near) / (far - near);
        projection[14] = -2. * far * near / (far - near);
        let mut view = [0.; 16];
        (view[0], view[5], view[10], view[15]) = (1., 1., 1., 1.);
        let (forward, n) = super::view(&projection, &view);
        assert_eq!(forward, [-0., -0., -1.]);
        assert!((n - near).abs() < 1e-12, "near {n}");
    }
}
