//! A droideka's guns turning toward the point its aim is on. The 2004 game
//! gives each gun an aimer that turns, by at most its yaw rate across and its
//! pitch rate up and down a second and inside its limits, until the gun's
//! barrel line passes through that point; a bolt leaves along the gun as it
//! stands. There the turn moves only the aimer's own point, which carries no
//! mesh, so the guns are never seen to turn. Here the blaster's bone itself
//! turns about its joint, so the gun is drawn pointing where its bolts go.
//! The world turns its hidden rig's guns every tick; the client turns the
//! drawn guns every frame by the same step.

use crate::{js_max, js_min};
use three::{Matrix4, NodeId, Quaternion, TransformTree, Vector3};

/// rad/s: how fast a droideka's guns turn across (yaw) and up and down
/// (pitch). The 2004 aimers' defaults, which the droideka's class keeps; a
/// class's own `AimerYawRate` and `AimerPitchRate` replace them.
pub const YAW_RATE: f64 = 2.0;
pub const PITCH_RATE: f64 = 1.0;

/// A gun's aimer: how fast it turns (rad/s) and how far (rad, from the gun's
/// line in the clip's pose: yaw across, positive to the gun's left; pitch
/// up). Outside its limits the gun still fires, along the gun as it stops.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aimer {
    pub yaw_rate: f64,
    pub pitch_rate: f64,
    pub yaw: [f64; 2],
    pub pitch: [f64; 2],
}

impl Default for Aimer {
    /// The 2004 aimer's defaults: 2 and 1 rad/s, 30 degrees across and 45 up
    /// and down either way.
    fn default() -> Self {
        let (yaw, pitch) = (30f64.to_radians(), 45f64.to_radians());
        Aimer { yaw_rate: YAW_RATE, pitch_rate: PITCH_RATE, yaw: [-yaw, yaw], pitch: [-pitch, pitch] }
    }
}

/// Where a gun turns: the blaster's bone (the aimer's parent: the gun's mesh
/// and barrels hang on it) and the turn from that bone's frame to the
/// aimer's (Z along the barrels, Y up, X to the gun's left).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mount {
    pub bone: usize,
    pub frame: Quaternion,
}

impl Mount {
    /// A blaster bone whose barrels run along its X axis (where the mesh's
    /// barrel tips are found), its aimer's up being whichever of the bone's
    /// +Y and -Y points up with the body at bind (`bind_up`: the bone's Y
    /// axis's world height there).
    pub fn new(bone: usize, bind_up: f64) -> Mount {
        let s = if bind_up < 0.0 { -1.0 } else { 1.0 };
        let mut basis = Matrix4::default();
        basis.make_basis(&Vector3::new(0.0, 0.0, -s), &Vector3::new(0.0, s, 0.0), &Vector3::new(1.0, 0.0, 0.0));
        let mut frame = Quaternion::default();
        frame.set_from_rotation_matrix(&basis);
        Mount { bone, frame }
    }

    /// A gun bone whose aimer's frame at rest is the body's own at bind (Z
    /// ahead, Y up, X left), whatever the bone's axes: `bind_world` is the
    /// bone's world turn at bind.
    pub fn from_world(bone: usize, bind_world: &Quaternion) -> Mount {
        let mut frame = *bind_world;
        frame.invert();
        Mount { bone, frame }
    }

    /// The turn, in the blaster bone's own frame, that turns the gun by
    /// `yaw` across and `pitch` up in its aimer's frame.
    pub fn turn(&self, yaw: f64, pitch: f64) -> Quaternion {
        let (mut across, mut up) = (Quaternion::default(), Quaternion::default());
        across.set_from_axis_angle(&Vector3::new(0.0, 1.0, 0.0), yaw);
        up.set_from_axis_angle(&Vector3::new(1.0, 0.0, 0.0), -pitch);
        let mut back = self.frame;
        back.invert();
        let mut q = self.frame;
        q.multiply(&across).multiply(&up).multiply(&back);
        q
    }
}

/// The direction (yaw, pitch) of a gun's aimer frame point along.
pub fn direction(rest: &Quaternion, yaw: f64, pitch: f64) -> Vector3 {
    let (sp, cp) = v8math::sin_cos(pitch);
    let (sy, cy) = v8math::sin_cos(yaw);
    let mut d = Vector3::new(sy * cp, sp, cy * cp);
    d.apply_quaternion(rest);
    d
}

/// One step of a gun's aimer, `dt` seconds long, from `aim` (its yaw and
/// pitch now). `rest`: the world turn of its frame with the gun at rest (the
/// clip's pose); `pivot`: where it turns; `line` and `along`: a point of the
/// barrel line as the gun stands and that line's direction; `target`: the
/// point to turn to (None: back to the rest line). As in the 2004 game the
/// gun turns so that its barrel line, not its joint, comes to pass through
/// the point: the line's offset from the joint is taken out first.
#[allow(clippy::too_many_arguments)]
pub fn step(aim: [f64; 2], rest: &Quaternion, pivot: &Vector3, line: &Vector3, along: &Vector3,
    target: Option<&Vector3>, dt: f64, aimer: &Aimer) -> [f64; 2]
{
    let want = match target {
        None => [0.0, 0.0],
        Some(point) => {
            let mut off = *line;
            off.sub(pivot);
            let mut nearest = *line;
            nearest.add_scaled_vector(along, -off.dot(along));
            let mut e = *point;
            e.sub(&nearest);
            if e.length_sq() < 1e-12 {
                return aim;
            }
            e.normalize();
            let mut back = *rest;
            back.invert();
            e.apply_quaternion(&back);
            [v8math::atan2(e.x, e.z), v8math::asin(js_max(-1.0, js_min(1.0, e.y)))]
        }
    };
    let toward = |now: f64, to: f64, rate: f64| {
        let most = rate * dt;
        now + js_max(-most, js_min(most, to - now))
    };
    let within = |x: f64, limits: [f64; 2]| js_max(limits[0], js_min(limits[1], x));
    [within(toward(aim[0], want[0], aimer.yaw_rate), aimer.yaw), within(toward(aim[1], want[1], aimer.pitch_rate), aimer.pitch)]
}

/// A droideka class's gun aimer: its blaster's (the first weapon with
/// aimers) `AimerYawRate`, `AimerPitchRate` (rad/s), `AimerYawLimits` and
/// `AimerPitchLimits` (degrees) where it names them; else, for the limits,
/// the class's `YawLimits` and `PitchLimits` (the droideka's weapon section
/// gives the same numbers to both); else the 2004 aimer's defaults.
pub fn aimer_of(class: &bfe_content::Value) -> Aimer {
    use bfe_content::Value;
    let blaster = class.get("weapons").and_then(Value::as_array).and_then(|list| {
        list.iter().find(|w| matches!(w.get("aimers"), Some(Value::Array(a)) if !a.is_empty()))
    });
    let named = |key: &str| blaster.and_then(|b| b.get(key)).filter(|v| v.truthy());
    let number = |v: &Value| bfe_content::js::to_number(Some(v));
    let rate = |key: &str, default: f64| named(key).map(number).filter(|r| r.is_finite() && *r > 0.0).unwrap_or(default);
    let limits = |key: &str, class_key: &str, default: [f64; 2]| {
        named(key).or_else(|| class.get(class_key).filter(|v| v.truthy())).and_then(Value::as_array)
            .filter(|a| a.len() >= 2)
            .map_or(default, |a| [number(&a[0]).to_radians(), number(&a[1]).to_radians()])
    };
    let d = Aimer::default();
    Aimer {
        yaw_rate: rate("AimerYawRate", d.yaw_rate),
        pitch_rate: rate("AimerPitchRate", d.pitch_rate),
        yaw: limits("AimerYawLimits", "YawLimits", d.yaw),
        pitch: limits("AimerPitchLimits", "PitchLimits", d.pitch),
    }
}

/// A posed rig's guns: each blaster bone's turn as the clip left it (what
/// `turn` builds on), kept between the pose and the turns of one tick.
#[derive(Clone, Debug, Default)]
pub struct Guns {
    pub mounts: Vec<Mount>,
    pub rest: Vec<Quaternion>,
    /// Each gun's barrel tips in its bone's frame, where its rounds leave
    /// (none: from the bone's own line).
    pub tips: Vec<Vec<Vector3>>,
    /// The turn last written on each gun's bone: a bone the clip has not
    /// posed again since still holds it, and its rest is the one kept.
    written: Vec<Option<Quaternion>>,
}

impl Guns {
    pub fn new(mounts: Vec<Mount>) -> Guns {
        let rest = vec![Quaternion::default(); mounts.len()];
        let written = vec![None; mounts.len()];
        let tips = vec![Vec::new(); mounts.len()];
        Guns { mounts, rest, tips, written }
    }

    /// Back to a new life's state: the clip's rest taken afresh at the next
    /// pose, nothing written.
    pub fn reset(&mut self) {
        self.rest.fill(Quaternion::default());
        self.written.fill(None);
    }

    /// Where gun `g`'s round leaves, in the world, as the tree stands (its
    /// matrices up to date down to the gun's bone): the tip of `barrel`, the
    /// middle of its tips where that barrel is not known, else the bone's
    /// own place.
    fn tip(&self, tree: &TransformTree, bones: &[NodeId], g: usize, barrel: usize) -> Vector3 {
        let world = &tree.node(bones[self.mounts[g].bone]).matrix_world;
        let tips = &self.tips[g];
        let mut at = match tips.get(barrel) {
            Some(tip) => *tip,
            None if !tips.is_empty() => {
                let mut sum = Vector3::default();
                for tip in tips {
                    sum.add(tip);
                }
                sum.multiply_scalar(1.0 / tips.len() as f64);
                sum
            }
            None => Vector3::default(),
        };
        at.apply_matrix4(world);
        at
    }

    /// After the clip's pose: keep each blaster's turn as posed (a bone the
    /// pose left alone keeps its earlier rest), and turn the guns by `aims`
    /// (yaw, pitch a gun; a gun with none stays at rest).
    pub fn pose(&mut self, tree: &mut TransformTree, bones: &[NodeId], aims: &[[f64; 2]]) {
        for (g, mount) in self.mounts.iter().enumerate() {
            let now = tree.node(bones[mount.bone]).quaternion;
            if self.written[g] != Some(now) {
                self.rest[g] = now;
            }
        }
        self.turn(tree, bones, aims);
    }

    /// Turn the guns by `aims` from the pose kept by `pose`.
    pub fn turn(&mut self, tree: &mut TransformTree, bones: &[NodeId], aims: &[[f64; 2]]) {
        for (g, mount) in self.mounts.iter().enumerate() {
            let [yaw, pitch] = aims.get(g).copied().unwrap_or([0.0, 0.0]);
            let mut q = self.rest[g];
            q.multiply(&mount.turn(yaw, pitch));
            tree.node_mut(bones[mount.bone]).copy_quaternion(&q);
            self.written[g] = Some(tree.node(bones[mount.bone]).quaternion);
        }
    }

    /// Gun `g` as it stands: where it turns, the world turn of its aimer's
    /// frame at rest, and the direction its barrels point along. The tree's
    /// world matrices are brought up to date along the way.
    pub fn frame(&self, tree: &mut TransformTree, bones: &[NodeId], g: usize) -> Option<(Vector3, Quaternion, Vector3)> {
        let mount = self.mounts.get(g)?;
        let node = bones[mount.bone];
        tree.update_world_matrix(node, true, false);
        let world = tree.node(node).matrix_world;
        let mut pivot = Vector3::default();
        pivot.set_from_matrix_position(&world);
        let mut along = Vector3::default();
        along.set_from_matrix_column(&world, 0).normalize();
        // The parent's world turn, then the clip's turn of the blaster, then
        // the aimer's frame on it.
        let mut parent = Quaternion::default();
        if let Some(p) = tree.node(node).parent {
            let mut rotation = Matrix4::default();
            rotation.extract_rotation(&tree.node(p).matrix_world);
            parent.set_from_rotation_matrix(&rotation);
        }
        let mut rest = parent;
        rest.multiply(&self.rest[g]).multiply(&mount.frame);
        Some((pivot, rest, along))
    }

    /// The direction gun `g` points along, as the tree stands (its world
    /// matrices up to date down to the blaster's bone).
    pub fn along(&self, tree: &mut TransformTree, bones: &[NodeId], g: usize) -> Option<Vector3> {
        let node = bones[self.mounts.get(g)?.bone];
        tree.update_world_matrix(node, true, false);
        let mut along = Vector3::default();
        along.set_from_matrix_column(&tree.node(node).matrix_world, 0).normalize();
        Some(along)
    }

    /// Step every gun's aimer toward `target` (None: back to rest) and turn
    /// them: `aims` in and out. `barrels`: the barrel each gun fires next (a
    /// number past its barrels where that is not known), whose line is the
    /// one brought onto the point, its offset from the joint taken out.
    #[allow(clippy::too_many_arguments)]
    pub fn aim(&mut self, tree: &mut TransformTree, bones: &[NodeId], aims: &mut [[f64; 2]], barrels: &[usize],
        target: Option<&Vector3>, dt: f64, aimer: &Aimer)
    {
        for g in 0..self.mounts.len().min(aims.len()) {
            let Some((pivot, rest, along)) = self.frame(tree, bones, g) else { continue };
            let line = self.tip(tree, bones, g, barrels.get(g).copied().unwrap_or(usize::MAX));
            aims[g] = step(aims[g], &rest, &pivot, &line, &along, target, dt, aimer);
        }
        self.turn(tree, bones, aims);
    }
}

/// A bone's world turn at bind: its parents' bind turns, root first.
fn bind_world(bone: usize, parents: &[i32], bind: &[Quaternion]) -> Option<Quaternion> {
    let mut chain = vec![bone];
    while let Some(&p) = chain.last().and_then(|&b| parents.get(b)) {
        let Ok(p) = usize::try_from(p) else { break };
        chain.push(p);
    }
    let mut world = Quaternion::default();
    for &b in chain.iter().rev() {
        world.multiply(bind.get(b)?);
    }
    Some(world)
}

/// The mounts of gun bones whose aimers rest on the body's own frame at bind
/// (a first person model's arms), in the order given.
pub fn body_mounts(bones: &[usize], parents: &[i32], bind: &[Quaternion]) -> Vec<Mount> {
    bones.iter().filter_map(|&b| Some(Mount::from_world(b, &bind_world(b, parents, bind)?))).collect()
}

/// The mounts of a skeleton's guns, given each gun's blaster bone (its
/// aimer's parent): each with its frame, from the bones' bind turns (`bind`,
/// each bone's in its parent's frame) and parents.
pub fn mounts(blasters: &[usize], parents: &[i32], bind: &[Quaternion]) -> Vec<Mount> {
    blasters
        .iter()
        .filter_map(|&blaster| {
            if blaster >= parents.len() {
                return None;
            }
            let world = bind_world(blaster, parents, bind)?;
            let mut up = Vector3::new(0.0, 1.0, 0.0);
            up.apply_quaternion(&world);
            Some(Mount::new(blaster, up.y))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use three::TransformNode;

    // A droideka's left gun as its model has it: the arm's end 0.5 m left
    // of the body, 0.8 m up and 0.72 m ahead, the blaster's X along the
    // barrels (forward) and its Y down.
    fn rig() -> (TransformTree, Vec<NodeId>, Guns) {
        let mut tree = TransformTree::new();
        let root = tree.add_node(TransformNode::default());
        let mut blaster = TransformNode::placed(Vector3::new(0.5, 0.8, 0.72), Vector3::new(1.0, 1.0, 1.0));
        let mut m = Matrix4::default();
        m.make_basis(&Vector3::new(0.0, 0.0, 1.0), &Vector3::new(0.0, -1.0, 0.0), &Vector3::new(1.0, 0.0, 0.0));
        let mut q = Quaternion::default();
        q.set_from_rotation_matrix(&m);
        blaster.copy_quaternion(&q);
        let bone = tree.add_node(blaster);
        tree.add_child(root, bone);
        let aimer = tree.add_node(TransformNode::placed(Vector3::new(0.745, 0.0, 0.0), Vector3::new(1.0, 1.0, 1.0)));
        tree.add_child(bone, aimer);
        let bones = vec![bone, aimer];
        let mounts = mounts(&[0], &[-1, 0], &[q, Quaternion::default()]);
        assert_eq!(mounts.len(), 1);
        let mut guns = Guns::new(mounts);
        guns.pose(&mut tree, &bones, &[[0.0, 0.0]]);
        (tree, bones, guns)
    }

    // At rest the aimer's frame is the body's (Z ahead, Y up, X left) and
    // the gun points straight ahead; turned, it points by its yaw and pitch.
    #[test]
    fn a_gun_turns_about_its_joint_by_its_aimers_angles() {
        let (mut tree, bones, mut guns) = rig();
        let (pivot, rest, along) = guns.frame(&mut tree, &bones, 0).unwrap();
        assert!(pivot.distance_to(&Vector3::new(0.5, 0.8, 0.72)) < 1e-12);
        assert!(along.distance_to(&Vector3::new(0.0, 0.0, 1.0)) < 1e-12);
        assert!(direction(&rest, 0.0, 0.0).distance_to(&Vector3::new(0.0, 0.0, 1.0)) < 1e-12);
        for (yaw, pitch) in [(0.3, 0.0), (0.0, 0.2), (-0.5, -0.3)] {
            guns.turn(&mut tree, &bones, &[[yaw, pitch]]);
            let along = guns.along(&mut tree, &bones, 0).unwrap();
            let want = Vector3::new(yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos());
            assert!(along.distance_to(&want) < 1e-12, "{yaw} {pitch}: {along:?}");
            // Its muzzle swings with it about the joint.
            tree.update_world_matrix(bones[1], true, false);
            let mut muzzle = Vector3::default();
            muzzle.set_from_matrix_position(&tree.node(bones[1]).matrix_world);
            let mut out = pivot;
            out.add_scaled_vector(&want, 0.745);
            assert!(muzzle.distance_to(&out) < 1e-12);
        }
    }

    // From the rest line, a point 20 m off and 3 degrees left and 5 up: the
    // gun turns at 2 rad/s across and 1 up, so it is there after the slower
    // of 0.026 s and 0.087 s; its line then passes through the point.
    #[test]
    fn an_aimer_turns_at_its_rates_until_its_line_meets_the_point() {
        let (mut tree, bones, mut guns) = rig();
        let aimer = Aimer::default();
        let (yaw, pitch) = (3f64.to_radians(), 5f64.to_radians());
        let mut point = Vector3::new(0.5, 0.8, 0.72);
        point.add_scaled_vector(&Vector3::new(yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos()), 20.0);
        let mut aims = [[0.0, 0.0]];
        let mut ticks = 0;
        loop {
            guns.aim(&mut tree, &bones, &mut aims, &[usize::MAX], Some(&point), 1.0 / 60.0, &aimer);
            ticks += 1;
            let (pivot, _, along) = guns.frame(&mut tree, &bones, 0).unwrap();
            let mut to = point;
            to.sub(&pivot);
            let mut miss = pivot;
            miss.add_scaled_vector(&along, to.dot(&along));
            if miss.distance_to(&point) < 1e-9 {
                break;
            }
            assert!(ticks < 60);
        }
        // pitch 5 degrees at 1 rad/s: 0.087 s, the sixth tick.
        assert_eq!(ticks, 6);
        assert!((aims[0][0] - yaw).abs() < 1e-12 && (aims[0][1] - pitch).abs() < 1e-12);
        // Back to rest when the aim is gone, as fast.
        for _ in 0..6 {
            guns.aim(&mut tree, &bones, &mut aims, &[usize::MAX], None, 1.0 / 60.0, &aimer);
        }
        assert_eq!(aims[0], [0.0, 0.0]);
    }

    // A pose that leaves the gun's bone alone does not stack the turn: the
    // rest kept is the clip's, and a pose that moves it takes the new rest.
    #[test]
    fn a_turn_is_not_stacked_on_a_bone_the_pose_left_alone() {
        let (mut tree, bones, mut guns) = rig();
        let rest = guns.rest[0];
        for _ in 0..3 {
            guns.pose(&mut tree, &bones, &[[0.3, 0.1]]);
        }
        assert_eq!(guns.rest[0], rest);
        let along = guns.along(&mut tree, &bones, 0).unwrap();
        assert!(along.distance_to(&Vector3::new(0.3f64.sin() * 0.1f64.cos(), 0.1f64.sin(), 0.3f64.cos() * 0.1f64.cos())) < 1e-12);
        let mut posed = rest;
        posed.multiply(&Quaternion::new(0.0, 0.0, 0.0998334166, 0.9950041653));
        tree.node_mut(bones[0]).copy_quaternion(&posed);
        guns.pose(&mut tree, &bones, &[[0.0, 0.0]]);
        assert_eq!(guns.rest[0], tree.node(bones[0]).quaternion);
    }

    // The droideka's barrels sit either side of its gun's line (0.07 m
    // across on the bone's Z): the aimer brings the line of the barrel that
    // fires next, not the joint, onto the point, so that barrel's round,
    // leaving its tip along the gun, crosses the point itself, at 5, 20 and
    // 50 m, for either barrel.
    #[test]
    fn the_firing_barrels_line_not_the_joint_meets_the_point() {
        for (k, distance) in [5.0, 20.0, 50.0].into_iter().enumerate() {
            for barrel in 0..2 {
                let (mut tree, bones, mut guns) = rig();
                guns.tips = vec![vec![Vector3::new(0.722, -0.008, -0.07), Vector3::new(0.722, -0.008, 0.07)]];
                let aimer = Aimer::default();
                let (yaw, pitch): (f64, f64) = ([0.05, -0.02, 0.01][k], [0.2, 0.08, 0.03][k]);
                let mut point = Vector3::new(0.5, 0.8, 0.72);
                point.add_scaled_vector(&Vector3::new(yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos()), distance);
                let mut aims = [[0.0, 0.0]];
                for _ in 0..90 {
                    guns.aim(&mut tree, &bones, &mut aims, &[barrel], Some(&point), 1.0 / 60.0, &aimer);
                }
                tree.update_world_matrix(bones[0], true, false);
                let mut tip = guns.tips[0][barrel];
                tip.apply_matrix4(&tree.node(bones[0]).matrix_world);
                let along = guns.along(&mut tree, &bones, 0).unwrap();
                let mut to = point;
                to.sub(&tip);
                let mut miss = tip;
                miss.add_scaled_vector(&along, to.dot(&along));
                assert!(miss.distance_to(&point) < 1e-9, "{distance} m, barrel {barrel}: misses by {}", miss.distance_to(&point));
                // The joint's own line passes beside it, by about the offset.
                let (pivot, _, _) = guns.frame(&mut tree, &bones, 0).unwrap();
                let mut to = point;
                to.sub(&pivot);
                let mut beside = pivot;
                beside.add_scaled_vector(&along, to.dot(&along));
                assert!(beside.distance_to(&point) > 0.06, "{}", beside.distance_to(&point));
            }
        }
    }

    // A point behind the gun turns it to its limits and no further; a point
    // on its own line's nearest place to the joint leaves it as it is. No
    // angle comes out undefined.
    #[test]
    fn a_point_behind_the_gun_or_at_its_joint_leaves_it_defined() {
        let (mut tree, bones, mut guns) = rig();
        let aimer = Aimer { yaw: [-40f64.to_radians(), 40f64.to_radians()], pitch: [-25f64.to_radians(), 25f64.to_radians()], ..Default::default() };
        let mut aims = [[0.0, 0.0]];
        let behind = Vector3::new(0.6, 0.9, -10.0);
        for _ in 0..120 {
            guns.aim(&mut tree, &bones, &mut aims, &[usize::MAX], Some(&behind), 1.0 / 60.0, &aimer);
            assert!(aims[0][0].is_finite() && aims[0][1].is_finite());
            assert!(aims[0][0].abs() <= 40f64.to_radians() + 1e-12 && aims[0][1].abs() <= 25f64.to_radians() + 1e-12);
        }
        assert!(aims[0][0].abs() > 39f64.to_radians(), "it swings to a limit: {:?}", aims);
        let (mut tree, bones, mut guns) = rig();
        let mut aims = [[0.1, 0.05]];
        let at = Vector3::new(0.5, 0.8, 0.72);
        guns.aim(&mut tree, &bones, &mut aims, &[usize::MAX], Some(&at), 1.0 / 60.0, &aimer);
        assert_eq!(aims, [[0.1, 0.05]]);
    }

    // The world's gun counter runs 0..3 for two guns of two barrels: gun 0
    // barrel 0, gun 1 barrel 0, gun 0 barrel 1, gun 1 barrel 1. Each gun's
    // next barrel is the one of its next turn.
    #[test]
    fn each_gun_aims_the_barrel_of_its_next_turn() {
        use super::super::droideka::next_barrels;
        assert_eq!(next_barrels(0.0, &[2, 2]), [0, 0]);
        assert_eq!(next_barrels(1.0, &[2, 2]), [1, 0]);
        assert_eq!(next_barrels(2.0, &[2, 2]), [1, 1]);
        assert_eq!(next_barrels(3.0, &[2, 2]), [0, 1]);
        // A client's counter counts on past the world's turn of four.
        assert_eq!(next_barrels(6.0, &[2, 2]), next_barrels(2.0, &[2, 2]));
        // A gun without barrels, and none at all.
        assert_eq!(next_barrels(1.0, &[2, 0]), [1, usize::MAX]);
        assert_eq!(next_barrels(1.0, &[]), [usize::MAX; 2]);
        assert_eq!(next_barrels(f64::NAN, &[2, 2]), [usize::MAX; 2]);
    }

    // The droideka's class names no rates (2 and 1 rad/s) and gives its
    // weapon section's limits as the class's YawLimits and PitchLimits.
    #[test]
    fn a_droidekas_aimer_comes_from_its_class() {
        let class = bfe_content::json::parse_bytes(br#"{"YawLimits":[-40,40],"PitchLimits":[-25,25],
            "weapons":[{"aimers":["hp_gun_1","hp_gun_2"]},{"base":"shield"}]}"#).unwrap();
        let a = aimer_of(&class);
        assert_eq!((a.yaw_rate, a.pitch_rate), (YAW_RATE, PITCH_RATE));
        assert_eq!((a.yaw, a.pitch), ([-40f64.to_radians(), 40f64.to_radians()], [-25f64.to_radians(), 25f64.to_radians()]));
        let own = bfe_content::json::parse_bytes(br#"{"YawLimits":[-40,40],"weapons":[{"aimers":["a"],
            "AimerYawRate":3,"AimerYawLimits":[-10,20]}]}"#).unwrap();
        let a = aimer_of(&own);
        assert_eq!((a.yaw_rate, a.pitch_rate, a.yaw), (3.0, PITCH_RATE, [-10f64.to_radians(), 20f64.to_radians()]));
        assert_eq!(a.pitch, Aimer::default().pitch);
    }

    // A point beyond the limits: the gun stops at them.
    #[test]
    fn an_aimer_stops_at_its_limits() {
        let (mut tree, bones, mut guns) = rig();
        let aimer = Aimer { yaw: [-40f64.to_radians(), 40f64.to_radians()], pitch: [-25f64.to_radians(), 25f64.to_radians()], ..Default::default() };
        let mut aims = [[0.0, 0.0]];
        let point = Vector3::new(10.0, -8.0, 1.5);
        for _ in 0..120 {
            guns.aim(&mut tree, &bones, &mut aims, &[usize::MAX], Some(&point), 1.0 / 60.0, &aimer);
        }
        assert_eq!(aims[0], [40f64.to_radians(), -25f64.to_radians()]);
    }
}
