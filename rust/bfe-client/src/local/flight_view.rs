//! Render the space world's hulls from their bracketing fixed ticks (local
//! and online) and drive the 2005 flyer third-person chase, cockpit and gunner
//! cameras.
use super::space::{Chair, Frame, History, Hull};
use crate::{
    online::{models::Model, replay::Collision},
    render_bridge::Bridge,
    scene::MapCamera,
};
use bfe_content::Value;
use bfe_content_native::BattleContent;
use bfe_presentation::cameras::{
    math::{
        add, axis, inv, look_basis, mix, mul, norm, plus, rotate, scale, slerp, sub, transformed,
        Math, DEG, PI, UP, V8,
    },
    Cast,
};
use bfe_pose::rig::hull::HullRig;
use bfe_sim::space::flight::SpaceTrick;
use three::{CoordinateSystem, Matrix4, Quaternion, Vector3};

/// 4:3 reference vertical-to-horizontal ratio (`3 / 4`) used to convert
/// horizontal FOV into the viewport vertical FOV (Hor+ widescreen scaling).
const REFERENCE_VERTICAL_SCALE: f64 = 0.75;
/// Exponential lens smoothing rate (1/s) shared across cockpit and chase views.
const FOV_SMOOTH_RATE: f64 = 25.0;
/// The 2005 chase camera's obstruction (its chase update, which the flyer camera
/// calls): one ray a frame, cycling through these offsets in the camera's right
/// and up; the boom's clear fraction is taken in at once, a gain of up to this
/// much is held while still obstructed, and otherwise it eases out at this rate
/// per second.
const OBSTRUCTION_RAYS: [(f64, f64); 5] = [(0.0, 0.0), (0.4, 0.4), (-0.4, 0.4), (0.4, -0.4), (-0.4, -0.4)];
const OBSTRUCTION_HOLD: f64 = 0.15;
const OBSTRUCTION_RELEASE: f64 = 0.75;
/// The camera's near plane; the eye stays this far out along the boom.
const CHASE_NEAR: f64 = 0.3;
/// Trick camera detach decay rate after a maneuver ends (1/s).
const TRICK_DETACH_DECAY_RATE: f64 = 0.9;
/// Distance beyond which the chase camera snaps instead of easing (m), the 2005
/// chase update's (its square, 62500).
const CHASE_SNAP_DISTANCE: f64 = 250.0;
/// Default horizontal FOV when a flyer or seat omits an FOV key (radians).
const DEFAULT_FOV: f64 = 48.0 * DEG;
/// The pilot's chase camera always looks down by this fixed angle (radians); the 2005 game
/// replaces the section's own TiltValue with it for a flying craft.
const PILOT_CHASE_TILT: f64 = 0.087665;
/// Aim tension while a trick detaches the camera: 20/s at full detach, tightening toward 50/s as
/// the detach decays (`[at full detach, as it ends]`). Outside a trick the 2005 game turns the
/// pilot's chase camera with the hull at once, whatever the section's AimTension says.
const DETACHED_AIM_TENSION: [f64; 2] = [20.0, 50.0];

/// The chase camera's aim tension at `detach` (1 at full detach, falling to 0).
fn detached_aim_tension(detach: f64) -> f64 {
    let [at_full, at_end] = DETACHED_AIM_TENSION;
    at_end + (at_full - at_end) * detach
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SeatCameraSpec {
    pub eye: Vector3,
    pub track_center: Vector3,
    pub track_offset: Vector3,
    pub tilt: f64,
    /// A gunner's seat hangs from its turret node (in the hull's frame), else
    /// from the craft's origin.
    pub pivot: Option<Vector3>,
    pub move_tension: [f64; 6],
    pub first_fov: f64,
    pub third_fov: f64,
}

impl Default for SeatCameraSpec {
    fn default() -> Self {
        Self {
            eye: Vector3::default(),
            track_center: Vector3::default(),
            track_offset: Vector3::default(),
            tilt: 0.0,
            pivot: None,
            move_tension: [30.0; 6],
            first_fov: DEFAULT_FOV,
            third_fov: DEFAULT_FOV,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CraftCameraSpec {
    pub body: SeatCameraSpec,
    pub seats: Vec<SeatCameraSpec>,
    pub fov_effect_1st: f64,
    pub fov_effect_3rd: f64,
    pub min_cam_offset_3rd: Option<Vector3>,
    pub max_cam_offset_3rd: Option<Vector3>,
    pub trick_flip_detach: f64,
    pub trick_roll_detach: f64,
}

fn parse_f64_value(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) if n.is_finite() => Some(*n),
        Value::String(s) => s
            .split_whitespace()
            .next()
            .and_then(|tok| tok.parse::<f64>().ok())
            .filter(|n| n.is_finite()),
        _ => None,
    }
}

fn parse_vec3_value(v: &Value) -> Option<Vector3> {
    match v {
        Value::Array(items) if items.len() >= 3 => {
            let x = parse_f64_value(&items[0])?;
            let y = parse_f64_value(&items[1])?;
            let z = parse_f64_value(&items[2])?;
            Some(Vector3::new(x, y, z))
        }
        Value::String(s) => {
            let mut it = s.split_whitespace();
            let x = it.next()?.parse::<f64>().ok().filter(|n| n.is_finite())?;
            let y = it.next()?.parse::<f64>().ok().filter(|n| n.is_finite())?;
            let z = it.next()?.parse::<f64>().ok().filter(|n| n.is_finite())?;
            Some(Vector3::new(x, y, z))
        }
        _ => None,
    }
}

fn parse_pair_value(v: &Value) -> Option<(f64, f64)> {
    match v {
        Value::Number(n) if n.is_finite() => Some((*n, *n)),
        Value::Array(items) if !items.is_empty() => {
            let first = parse_f64_value(&items[0])?;
            let second = items.get(1).and_then(parse_f64_value).unwrap_or(first);
            Some((first, second))
        }
        Value::String(s) => {
            let mut it = s.split_whitespace();
            let first = it.next()?.parse::<f64>().ok().filter(|n| n.is_finite())?;
            let second = it
                .next()
                .and_then(|tok| tok.parse::<f64>().ok())
                .filter(|n| n.is_finite())
                .unwrap_or(first);
            Some((first, second))
        }
        _ => None,
    }
}

fn non_zero(v: Vector3) -> Option<Vector3> {
    (v.x != 0.0 || v.y != 0.0 || v.z != 0.0).then_some(v)
}

fn apply_seat_prop(seat: &mut SeatCameraSpec, has_eye: &mut bool, has_center: &mut bool, key: &str, value: &Value) {
    if key.eq_ignore_ascii_case("EyePointOffset") {
        if let Some(v) = parse_vec3_value(value) {
            seat.eye = v;
            *has_eye = true;
            if !*has_center {
                seat.track_center = v;
            }
        }
    } else if key.eq_ignore_ascii_case("TrackCenter") {
        if let Some(v) = parse_vec3_value(value) {
            seat.track_center = v;
            *has_center = true;
        }
    } else if key.eq_ignore_ascii_case("TrackOffset") {
        if let Some(v) = parse_vec3_value(value) {
            seat.track_offset = v;
        }
    } else if key.eq_ignore_ascii_case("TiltValue") {
        if let Some(deg) = parse_f64_value(value) {
            seat.tilt = deg * DEG;
        }
    } else if key.eq_ignore_ascii_case("MoveTension") {
        if let Some(rate) = parse_f64_value(value) {
            if rate > 0.0 {
                seat.move_tension = [rate; 6];
            }
        }
    } else if key.eq_ignore_ascii_case("MoveTensionX") {
        if let Some((pos, neg)) = parse_pair_value(value) {
            if pos > 0.0 {
                seat.move_tension[0] = pos;
            }
            if neg > 0.0 {
                seat.move_tension[1] = neg;
            }
        }
    } else if key.eq_ignore_ascii_case("MoveTensionY") {
        if let Some((pos, neg)) = parse_pair_value(value) {
            if pos > 0.0 {
                seat.move_tension[2] = pos;
            }
            if neg > 0.0 {
                seat.move_tension[3] = neg;
            }
        }
    } else if key.eq_ignore_ascii_case("MoveTensionZ") {
        if let Some((pos, neg)) = parse_pair_value(value) {
            if pos > 0.0 {
                seat.move_tension[4] = pos;
            }
            if neg > 0.0 {
                seat.move_tension[5] = neg;
            }
        }
    } else if key.eq_ignore_ascii_case("FirstPersonFOV") {
        if let Some(deg) = parse_f64_value(value) {
            if deg > 0.0 {
                seat.first_fov = deg * DEG;
            }
        }
    } else if key.eq_ignore_ascii_case("ThirdPersonFOV") {
        if let Some(deg) = parse_f64_value(value) {
            if deg > 0.0 {
                seat.third_fov = deg * DEG;
            }
        }
    }
}

fn apply_body_extra_prop(spec: &mut CraftCameraSpec, key: &str, value: &Value) {
    if key.eq_ignore_ascii_case("FOVEffect1st") {
        if let Some(deg) = parse_f64_value(value) {
            if deg > 0.0 {
                spec.fov_effect_1st = deg * DEG;
            }
        }
    } else if key.eq_ignore_ascii_case("FOVEffect3rd") {
        if let Some(deg) = parse_f64_value(value) {
            if deg > 0.0 {
                spec.fov_effect_3rd = deg * DEG;
            }
        }
    } else if key.eq_ignore_ascii_case("FOVEFFectMinCamOffset3rd") {
        if let Some(v) = parse_vec3_value(value).and_then(non_zero) {
            spec.min_cam_offset_3rd = Some(v);
        }
    } else if key.eq_ignore_ascii_case("FOVEFFectMaxCamOffset3rd") {
        if let Some(v) = parse_vec3_value(value).and_then(non_zero) {
            spec.max_cam_offset_3rd = Some(v);
        }
    } else if key.eq_ignore_ascii_case("TrickFlipCameraDetach") {
        if let Some(v) = parse_f64_value(value) {
            spec.trick_flip_detach = v.max(0.0);
        }
    } else if key.eq_ignore_ascii_case("TrickSideRollCameraDetach") {
        if let Some(v) = parse_f64_value(value) {
            spec.trick_roll_detach = v.max(0.0);
        }
    }
}

/// Parses a 2005 flyer unit record into body and gunner camera specifications.
pub(crate) fn parse_craft_camera(
    def: Option<&Value>,
    fallback_eye: [f64; 3],
    fallback_chase: [f64; 3],
) -> CraftCameraSpec {
    let mut spec = CraftCameraSpec::default();
    let mut body_has_eye = false;
    let mut body_has_center = false;
    let mut has_authored_chase = false;

    if let Some(def) = def {
        if let Some(sections) = def.get("vehicleSections").and_then(Value::as_array) {
            if !sections.is_empty() {
                for sec in sections {
                    let kind = sec.get("kind").and_then(Value::as_str).unwrap_or("");
                    let props = sec
                        .get("properties")
                        .and_then(Value::as_array)
                        .unwrap_or(&[]);
                    if kind.eq_ignore_ascii_case("chassis") {
                        for pair in props {
                            let (Some(k), Some(v)) = (pair.at(0).and_then(Value::as_str), pair.at(1))
                            else {
                                continue;
                            };
                            apply_seat_prop(
                                &mut spec.body,
                                &mut body_has_eye,
                                &mut body_has_center,
                                k,
                                v,
                            );
                            apply_body_extra_prop(&mut spec, k, v);
                        }
                    } else if kind.eq_ignore_ascii_case("seat") {
                        let mut seat = SeatCameraSpec::default();
                        let mut seat_eye = false;
                        let mut seat_center = false;
                        for pair in props {
                            let (Some(k), Some(v)) = (pair.at(0).and_then(Value::as_str), pair.at(1))
                            else {
                                continue;
                            };
                            apply_seat_prop(&mut seat, &mut seat_eye, &mut seat_center, k, v);
                        }
                        spec.seats.push(seat);
                    }
                }
            }
        } else if let Some(props) = def.get("properties").and_then(Value::as_array) {
            let mut active_seat: Option<usize> = None;
            let mut seat_flags: Vec<(bool, bool)> = Vec::new();
            for pair in props {
                let (Some(k), Some(v)) = (pair.at(0).and_then(Value::as_str), pair.at(1)) else {
                    continue;
                };
                if k.eq_ignore_ascii_case("FlyerSection") || k.eq_ignore_ascii_case("4cbe283c") {
                    if v.as_str()
                        .is_some_and(|s| s.trim().eq_ignore_ascii_case("BODY"))
                    {
                        active_seat = None;
                    } else {
                        spec.seats.push(SeatCameraSpec::default());
                        seat_flags.push((false, false));
                        active_seat = Some(spec.seats.len() - 1);
                    }
                    continue;
                }
                if k.eq_ignore_ascii_case("TurretSection") {
                    spec.seats.push(SeatCameraSpec::default());
                    seat_flags.push((false, false));
                    active_seat = Some(spec.seats.len() - 1);
                    continue;
                }
                if let Some(idx) = active_seat {
                    let (se, sc) = &mut seat_flags[idx];
                    apply_seat_prop(&mut spec.seats[idx], se, sc, k, v);
                } else {
                    apply_seat_prop(
                        &mut spec.body,
                        &mut body_has_eye,
                        &mut body_has_center,
                        k,
                        v,
                    );
                    apply_body_extra_prop(&mut spec, k, v);
                }
            }
        }
        has_authored_chase = body_has_center
            || non_zero(spec.body.track_offset).is_some()
            || spec.min_cam_offset_3rd.is_some()
            || spec.max_cam_offset_3rd.is_some();
    }

    if !body_has_eye {
        spec.body.eye = Vector3::new(fallback_eye[0], fallback_eye[1], fallback_eye[2]);
    }
    if !has_authored_chase {
        spec.body.track_center =
            Vector3::new(fallback_chase[0], fallback_chase[1], fallback_chase[2]);
        spec.body.track_offset = Vector3::default();
    }
    spec
}

struct WorldCast {
    collision: Collision,
    world: u32,
}

impl Cast for WorldCast {
    fn cast(&mut self, origin: Vector3, direction: Vector3, far: f64, _: bool) -> Option<f64> {
        let ray = bfe_sim::collision::Ray {
            ox: origin.x,
            oy: origin.y,
            oz: origin.z,
            dx: direction.x,
            dy: direction.y,
            dz: direction.z,
        };
        self.collision
            .borrow_mut()
            .ray_hit(self.world, ray, far, -1)
            .map(|(_, h)| h.distance)
    }
}

/// A gunner's seat frame: its origin (the turret node, else the craft's
/// origin) and axes, forward along his aim, up as near the hull's as the aim
/// allows, and the left side (the hull's +X) completing them.
struct SeatFrame {
    origin: Vector3,
    left: Vector3,
    up: Vector3,
    forward: Vector3,
}

impl SeatFrame {
    fn new(sec: &SeatCameraSpec, aim: Vector3, hull_up: Vector3, hull: &Matrix4) -> Self {
        let cross = |a: Vector3, b: Vector3| Vector3::new(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x);
        let side = cross(hull_up, aim);
        let left = if side.length_sq() > 1e-12 {
            norm(side)
        } else {
            norm(sub(transformed(Vector3::new(1.0, 0.0, 0.0), hull), transformed(Vector3::default(), hull)))
        };
        Self {
            origin: transformed(sec.pivot.unwrap_or_default(), hull),
            left,
            up: cross(aim, left),
            forward: aim,
        }
    }

    fn point(&self, local: Vector3) -> Vector3 {
        add(add(add(self.origin, scale(self.left, local.x)), scale(self.up, local.y)), scale(self.forward, local.z))
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ChaseOutput {
    pub position: Vector3,
    pub rotation: Quaternion,
    pub hfov: f64,
    pub near: f64,
    pub aim_dir: Vector3,
    pub project_x: bool,
}

#[derive(Default)]
pub(crate) struct ChaseSolver {
    mount: Option<(usize, u32, u8)>,
    first_person: bool,
    position: Vector3,
    rotation: Quaternion,
    hfov: f64,
    /// The filtered clear fraction of the boom, and the last five rays'.
    kept: f64,
    rays: [f64; 5],
    ray: usize,
    detach: f64,
    aim: Option<(Vector3, bool)>,
    /// Where the view was before this camera took it over: a fresh chase eases
    /// out from there (the game keeps one camera per player across a change of
    /// subject and only refills the boom).
    carried: Option<Vector3>,
}

impl ChaseSolver {
    /// The eye the chase camera eases toward this frame: `target` drawn in
    /// along the boom from `origin` by the filtered clear fraction, never
    /// nearer than the near plane.
    fn obstruct<C: Cast>(&mut self, world: &mut C, origin: Vector3, target: Vector3, dt: f64) -> Vector3 {
        let boom = sub(target, origin);
        let length = boom.length();
        if length * length <= 1e-4 {
            return target;
        }
        let direction = scale(boom, 1.0 / length);
        let span = (length - CHASE_NEAR).max(0.0);
        let (a, b) = OBSTRUCTION_RAYS[self.ray];
        let start = add(origin, rotate(Vector3::new(a, b, 0.0), self.rotation));
        let clear = if span > 0.0 {
            world.cast(start, direction, span, false).map_or(1.0, |d| (d / span).clamp(0.0, 1.0))
        } else {
            1.0
        };
        self.rays[self.ray] = clear;
        self.ray = (self.ray + 1) % OBSTRUCTION_RAYS.len();
        let m = self.rays.iter().fold(1.0_f64, |m, &r| m.min(r));
        if m < self.kept {
            self.kept = m;
        } else if m != 1.0 && m <= self.kept + OBSTRUCTION_HOLD {
            // Held while the boom is still obstructed.
        } else {
            self.kept = m.min(self.kept + OBSTRUCTION_RELEASE * dt);
        }
        plus(origin, direction, CHASE_NEAR + self.kept * span)
    }

    pub fn reset(&mut self) {
        self.mount = None;
        self.aim = None;
        self.carried = None;
    }

    /// The view's position before the next fresh step (the soldier's eye on
    /// boarding): that step eases from it instead of jumping to the chase point.
    pub fn carry(&mut self, from: Vector3) {
        self.carried = Some(from);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn step<C: Cast>(
        &mut self,
        spec: &CraftCameraSpec,
        seat: u8,
        unbanked_pos: Vector3,
        unbanked_q: Quaternion,
        drawn_q: Quaternion,
        velocity: Vector3,
        max_speed: f64,
        boost_speed: f64,
        trick: SpaceTrick,
        gunner_aim: Option<Vector3>,
        first_person: bool,
        mount_key: (usize, u32, u8),
        dt: f64,
        world: &mut C,
    ) -> ChaseOutput {
        let dt = if dt.is_finite() { dt.clamp(0.0, 0.25) } else { 0.0 };
        let fresh = self.mount != Some(mount_key);
        let view_changed = !fresh && self.first_person != first_person;
        let carried = self.carried.take().filter(|p| fresh && p.x.is_finite() && p.y.is_finite() && p.z.is_finite());
        let sec = if seat == 0 {
            &spec.body
        } else {
            spec.seats
                .get(seat as usize - 1)
                .unwrap_or(&spec.body)
        };

        let nose = norm(rotate(Vector3::new(0.0, 0.0, 1.0), unbanked_q));
        let hull_up = norm(rotate(UP, unbanked_q));
        let mut unbanked_mat = Matrix4::default();
        unbanked_mat.compose(&unbanked_pos, &unbanked_q, &Vector3::new(1.0, 1.0, 1.0));

        let fwd_speed = velocity.x * nose.x + velocity.y * nose.y + velocity.z * nose.z;
        let speed_cap = max_speed.max(boost_speed);
        let speed_frac = if fwd_speed > 0.0 && speed_cap > 0.0 {
            (fwd_speed / speed_cap).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let target_hfov = if seat == 0 {
            if first_person {
                if spec.fov_effect_1st > 0.0 && fwd_speed > 0.0 {
                    sec.first_fov + (spec.fov_effect_1st - sec.first_fov) * speed_frac
                } else {
                    sec.first_fov
                }
            } else if spec.fov_effect_3rd > 0.0 && fwd_speed > 0.0 {
                sec.third_fov + (spec.fov_effect_3rd - sec.third_fov) * speed_frac
            } else {
                sec.third_fov
            }
        } else if first_person {
            sec.first_fov
        } else {
            sec.third_fov
        };

        if fresh || self.hfov <= 0.0 {
            self.hfov = target_hfov;
        } else {
            self.hfov += (target_hfov - self.hfov) * (1.0 - V8::exp(-FOV_SMOOTH_RATE * dt));
        }

        if first_person {
            let (position, rotation, aim_dir) = if seat == 0 {
                let pos = add(rotate(sec.eye, drawn_q), unbanked_pos);
                let rot = mul(drawn_q, axis::<V8>(UP, PI));
                let aim = norm(rotate(Vector3::new(0.0, 0.0, 1.0), drawn_q));
                (pos, rot, aim)
            } else {
                let aim = gunner_aim.map(norm).unwrap_or(nose);
                let frame = SeatFrame::new(sec, aim, hull_up, &unbanked_mat);
                let rot = look_basis(scale(aim, -1.0), frame.up);
                (frame.point(sec.eye), rot, aim)
            };
            self.position = position;
            self.rotation = rotation;
            self.kept = 1.0;
            self.rays = [1.0; 5];
            self.detach = 0.0;
            self.mount = Some(mount_key);
            self.first_person = true;
            let project_x = seat > 0;
            self.aim = Some((aim_dir, project_x));
            return ChaseOutput {
                position,
                rotation,
                hfov: self.hfov,
                near: 0.05,
                aim_dir,
                project_x,
            };
        }

        if let Some(from) = carried {
            self.position = from;
        }
        let snap = (fresh && carried.is_none()) || view_changed;
        // A gunner's camera hangs from his seat's frame, at its turret node and looking
        // along his aim; the pilot's from the hull.
        let gunner_frame = (seat > 0)
            .then(|| SeatFrame::new(sec, gunner_aim.map(norm).unwrap_or(nose), hull_up, &unbanked_mat));
        let pivot = match &gunner_frame {
            Some(frame) => frame.point(sec.track_center),
            None => transformed(sec.track_center, &unbanked_mat),
        };
        let jump = snap || self.position.distance_to(&pivot) > CHASE_SNAP_DISTANCE;
        let (aim_dir, project_x, offset) = if seat == 0 {
            let trick_detaches = match trick {
                SpaceTrick::Loop | SpaceTrick::Immelmann => spec.trick_flip_detach > 0.0,
                SpaceTrick::BarrelLeft | SpaceTrick::BarrelRight => spec.trick_roll_detach > 0.0,
                SpaceTrick::None => false,
            };
            if trick_detaches {
                self.detach = 1.0;
            } else if jump || fresh {
                self.detach = 0.0;
            } else {
                self.detach = (self.detach - TRICK_DETACH_DECAY_RATE * dt).max(0.0);
            }

            let look_dir = if !jump && self.detach > 0.0 {
                let ahead = transformed(Vector3::new(0.0, 0.0, 5.0), &unbanked_mat);
                let to_ahead = sub(ahead, self.position);
                if to_ahead.length_sq() > 1e-8 {
                    let blended = mix(nose, norm(to_ahead), self.detach);
                    if blended.length_sq() > 1e-8 {
                        norm(blended)
                    } else {
                        nose
                    }
                } else {
                    nose
                }
            } else {
                nose
            };

            let want_q = mul(
                look_basis(scale(look_dir, -1.0), hull_up),
                axis::<V8>(Vector3::new(1.0, 0.0, 0.0), -PILOT_CHASE_TILT),
            );
            if !jump && self.detach > 0.0 {
                let k = 1.0 - V8::exp(-detached_aim_tension(self.detach) * dt);
                self.rotation = slerp::<V8>(self.rotation, want_q, k);
            } else {
                self.rotation = want_q;
            }

            let offset = if let Some(min_off) = spec.min_cam_offset_3rd {
                if fwd_speed > 0.0 && spec.fov_effect_3rd > 0.0 {
                    let max_off = spec.max_cam_offset_3rd.unwrap_or(min_off);
                    mix(min_off, max_off, speed_frac)
                } else {
                    min_off
                }
            } else {
                sec.track_offset
            };
            (nose, false, offset)
        } else {
            self.detach = 0.0;
            let aim = gunner_aim.map(norm).unwrap_or(nose);
            let up = gunner_frame.as_ref().map_or(hull_up, |f| f.up);
            let mut want_q = look_basis(scale(aim, -1.0), up);
            if sec.tilt != 0.0 {
                want_q = mul(want_q, axis::<V8>(Vector3::new(1.0, 0.0, 0.0), -sec.tilt));
            }
            self.rotation = want_q;
            (aim, true, sec.track_offset)
        };

        let target = add(pivot, rotate(offset, self.rotation));
        let origin = transformed(Vector3::new(0.0, 1.5, 0.0), &unbanked_mat);
        if jump || fresh {
            self.kept = 1.0;
            self.rays = [1.0; 5];
            self.ray = 0;
        }
        let obstructed = self.obstruct(world, origin, target, dt);

        if jump {
            self.position = obstructed;
        } else {
            let mut local = rotate(sub(obstructed, self.position), inv(self.rotation));
            local.x *= 1.0 - V8::exp(-sec.move_tension[if local.x > 0.0 { 0 } else { 1 }] * dt);
            local.y *= 1.0 - V8::exp(-sec.move_tension[if local.y > 0.0 { 2 } else { 3 }] * dt);
            local.z *= 1.0 - V8::exp(-sec.move_tension[if local.z > 0.0 { 4 } else { 5 }] * dt);
            self.position = add(self.position, rotate(local, self.rotation));
        }

        self.mount = Some(mount_key);
        self.first_person = false;
        self.aim = Some((aim_dir, project_x));
        ChaseOutput {
            position: self.position,
            rotation: self.rotation,
            hfov: self.hfov,
            near: 0.3,
            aim_dir,
            project_x,
        }
    }
}

/// Live 60 Hz own-craft or chair-turret prediction override supplied by the online
/// client's [`crate::online::space_receiver::SpacePredictor`].
#[derive(Clone, Copy, Debug)]
pub(crate) enum OwnSpaceOverride {
    Pilot {
        fighter_id: usize,
        class: u32,
        matrix: Matrix4,
        gunner_aim: Option<[f64; 3]>,
    },
    Chair {
        chair: Chair,
        turret_aim: [f64; 2],
    },
}

struct Craft {
    class: u32,
    model: Model,
    /// The class's takeoff clip on its model's rig, when the content has both:
    /// then the rig's parts are drawn instead of `model`.
    rig: Option<CraftRig>,
    camera: CraftCameraSpec,
    /// The guided rocket a seat of this craft steers: its class's own camera
    /// and MaxSpeed. While it flies the player's view follows it.
    rocket: Option<(CraftCameraSpec, f64, DeathCamSpec)>,
}

/// The guided rocket an ordnance record describes (`GuidedMissile` set): its
/// camera and MaxSpeed.
fn rocket_camera(unit_def: Option<&Value>) -> Option<(CraftCameraSpec, f64, DeathCamSpec)> {
    let property = |ord: &Value, key: &str| {
        ord.get("properties")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|p| p.at(0).and_then(Value::as_str).is_some_and(|k| k.eq_ignore_ascii_case(key)))
            .map(|p| bfe_content::js::to_number(p.at(1)))
    };
    let ord = unit_def?
        .get("weapons")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|w| w.get("ordnance"))
        .find(|o| property(o, "GuidedMissile").is_some_and(|g| g != 0.0))?;
    let top = property(ord, "MaxSpeed").filter(|v| *v > 0.0).unwrap_or(100.0);
    let death = DeathCamSpec {
        min_distance: property(ord, "GuidedMissileDeathCamMinDistance").unwrap_or(0.0),
        pullback: property(ord, "GuidedMissileDeathCamPullbackSpeed").unwrap_or(0.0),
        pause: property(ord, "GuidedMissileDeathCamPause").unwrap_or(0.0),
    };
    Some((parse_craft_camera(Some(ord), [0.0; 3], [0.0, 2.0, -8.0]), top, death))
}

/// A guided rocket class's death camera keys (0 when the class has none:
/// the view goes back to the craft at once).
#[derive(Clone, Copy, Debug, Default)]
struct DeathCamSpec {
    min_distance: f64,
    pullback: f64,
    pause: f64,
}

/// The view after a steered rocket has gone (the 2005 game's death camera):
/// for GuidedMissileDeathCamPause seconds it stays turned as it was and only
/// backs off along the rocket's last line, at GuidedMissileDeathCamPullbackSpeed,
/// while it is nearer than GuidedMissileDeathCamMinDistance behind where the
/// rocket ended and the way back is clear; then the craft's own camera returns.
#[derive(Clone, Copy, Debug)]
struct DeathCam {
    view: ChaseOutput,
    /// Where the rocket ended and the way back along its line (unit length).
    end: Vector3,
    back: Vector3,
    spec: DeathCamSpec,
    left: f64,
    /// The seat it belongs to: another seat or craft ends it.
    seat: (usize, u32, u8),
}

impl DeathCam {
    /// Advances by `dt`; false once its pause is over.
    fn step<C: Cast>(&mut self, dt: f64, world: &mut C) -> bool {
        self.left -= dt;
        if self.left <= 0.0 {
            return false;
        }
        let p = self.view.position;
        let behind = (p.x - self.end.x) * self.back.x + (p.y - self.end.y) * self.back.y + (p.z - self.end.z) * self.back.z;
        let step = self.spec.pullback * dt;
        if behind < self.spec.min_distance && step > 0.0 && world.cast(p, self.back, step, false).is_none() {
            self.view.position = Vector3::new(p.x + self.back.x * step, p.y + self.back.y * step, p.z + self.back.z * step);
        }
        true
    }
}

struct Drawn {
    craft: usize,
    matrix: Matrix4,
    /// Where this craft's posed bone matrices start in `Fleet::bone_matrices`.
    bones: usize,
}

/// A craft's model as a rig posed by its class's takeoff clip. The 2005 game
/// sets the clip to frame (frames - 1) x lift: its first frame on the deck
/// (lift 0), its last in flight (lift 1), between while it lifts off or
/// settles (wings, flaps, gear, doors, the vulture droid's legs).
struct CraftRig {
    rig: HullRig,
    clip: bfe_pose::Clip,
    duration: f64,
    pose: bfe_pose::Pose,
    /// Each rig bone's parts as one model, in that bone's space.
    bones: Vec<(usize, Model)>,
    /// Parts hung from no bone the rig knows, in the model's space.
    unboned: Option<Model>,
}

impl CraftRig {
    fn load(
        content: &BattleContent,
        bridge: &mut Bridge,
        textures: &[u32],
        class: &str,
        cascades: usize,
    ) -> Result<Option<Self>, String> {
        let Some(found) = content
            .craft_rigs
            .iter()
            .find(|r| r.class.eq_ignore_ascii_case(class))
        else {
            return Ok(None);
        };
        let Some(clip) = found.takeoff.as_ref() else {
            return Ok(None);
        };
        let bones = Value::Object(std::rc::Rc::new(bfe_content::value::Object::from_written(vec![(
            std::rc::Rc::from("bones"),
            found.bones.clone(),
        )])));
        let desc = bfe_pose::rig::content::hull_records(&Value::Null, &bones, &[], Vec::new(), true);
        let root = desc
            .bones
            .iter()
            .position(|b| matches!(b.name.to_ascii_lowercase().as_str(), "dummyroot" | "dummy_root"))
            .unwrap_or(0);
        let mut rig = HullRig::new(&desc);
        let node = rig.tree.node_mut(rig.root);
        node.matrix_auto_update = false;
        node.matrix = Matrix4::default();
        let names = found.bone_names();
        let mut models = Vec::new();
        let mut unboned = None;
        for (bone, parts) in found.groups() {
            let key = found.group_name(bone);
            let model = Model::from_parts(content, bridge, textures, &key, &parts, cascades)?;
            match (bone.and_then(|i| rig.by_name.get(names[i])), model) {
                (Some(i), Some(model)) if i < rig.bones.len() => models.push((i, model)),
                (_, model) => unboned = model.or(unboned),
            }
        }
        let mut out = Self {
            pose: bfe_pose::Pose::new(rig.bones.len(), root as i32),
            clip: bfe_pose::rig::content::clip(clip),
            duration: clip.duration,
            rig,
            bones: models,
            unboned,
        };
        out.pose_at(0.0);
        Ok(Some(out))
    }

    /// Poses the rig at a lift (0 on the deck, 1 in flight).
    fn pose_at(&mut self, lift: f64) {
        self.pose.clear();
        self.pose.add(&self.clip, lift.clamp(0.0, 1.0) * self.duration, 1.0, false);
        self.pose.apply_out();
        for (i, &id) in self.rig.bones.iter().enumerate() {
            let node = self.rig.tree.node_mut(id);
            if self.pose.w[i] > 1e-6 {
                let q = &self.pose.out[7 * i..];
                node.set_quaternion(q[0], q[1], q[2], q[3]);
                node.position.set(q[4], q[5], q[6]);
            } else {
                node.copy_quaternion(&self.rig.bind_q[i]);
                node.position = self.rig.bind_p[i];
            }
        }
        self.rig.tree.update_matrix_world(self.rig.root, true);
    }

    /// A posed bone's matrix in the model's space.
    fn bone_matrix(&self, bone: usize) -> Matrix4 {
        self.rig.tree.node(self.rig.bones[bone]).matrix_world
    }

    fn rows(&self) -> usize {
        self.bones.iter().map(|(_, m)| m.rows.len()).sum::<usize>() + self.unboned.as_ref().map_or(0, |m| m.rows.len())
    }
}

pub(crate) struct Fleet {
    craft: Vec<Craft>,
    gun_eyes: std::collections::BTreeMap<u32, [f64; 3]>,
    drawn: Vec<Drawn>,
    /// The drawn crafts' posed bone matrices in the world, this frame.
    bone_matrices: Vec<Matrix4>,
    props: Vec<(usize, usize, Matrix4)>,
    collision: Collision,
    world: u32,
    chase: ChaseSolver,
    /// The camera on a guided rocket the player steers.
    rocket_chase: ChaseSolver,
    /// While a rocket is steered: its last view, where it was and the way it
    /// flew, and its class's death camera keys; the death camera once it is gone.
    rocket_last: Option<(ChaseOutput, Vector3, Vector3, DeathCamSpec, (usize, u32, u8))>,
    death_cam: Option<DeathCam>,
    /// The last sampled frame drew the soldier's own view (no craft, no chair).
    foot_view: bool,
    /// The gun's forward axis while the player sits in a turret chair: the reticle's aim.
    chair_aim: Option<Vector3>,
    /// `BFE_DEV_SWBF2_CAMERA_TRACE`: one line per drawn frame while seated in a
    /// craft, the frame's step, the craft's drawn frame and the camera.
    trace: Option<std::io::BufWriter<std::fs::File>>,
}

fn find_unit_def<'a>(content: &'a BattleContent, class_name: &str) -> Option<&'a Value> {
    for side in &content.content.battle.sides {
        if let Some(units) = side.units.as_object() {
            for (name, def) in units.entries() {
                if name.eq_ignore_ascii_case(class_name) {
                    return Some(def);
                }
            }
        }
    }
    None
}

fn projection_from_hfov(hfov: f64, aspect: f64, near: f64, far: f64) -> [f64; 16] {
    let aspect = aspect.max(1e-6);
    let top = near * (hfov * 0.5).tan() * REFERENCE_VERTICAL_SCALE;
    let right = top * aspect;
    let mut projection = Matrix4::default();
    projection.make_perspective(
        -right,
        right,
        top,
        -top,
        near,
        far.max(near + 1.0),
        CoordinateSystem::WebGL,
    );
    projection.elements
}

impl Fleet {
    pub fn load(
        content: &BattleContent,
        bridge: &mut Bridge,
        textures: &[u32],
        cascades: usize,
        collision: Collision,
        world: u32,
    ) -> Result<Option<Self>, String> {
        let Some(map) = content
            .content
            .battle
            .swbf2
            .as_ref()
            .map(|b| &b.map_admission)
        else {
            return Ok(None);
        };
        let Some(pack) = map.vehicle_pack.as_ref().filter(|_| !map.has_terrain) else {
            return Ok(None);
        };
        let mut classes: Vec<_> = pack
            .classes
            .values()
            .filter(|c| matches!(c.base.as_str(), "flyer" | "commandflyer"))
            .collect();
        classes.sort_by(|a, b| a.name.cmp(&b.name));
        let mut craft = Vec::with_capacity(classes.len());
        for class in classes {
            let Some(name) = class.model.as_deref() else {
                continue;
            };
            let Some(model) = Model::new(content, bridge, textures, name, cascades)? else {
                continue;
            };
            let fallback_eye = class
                .points
                .iter()
                .find(|p| p.property.eq_ignore_ascii_case("EyePointOffset"))
                .and_then(|p| p.offset)
                .or_else(|| {
                    class
                        .points
                        .iter()
                        .find(|p| p.property.eq_ignore_ascii_case("PilotPosition"))
                        .and_then(|p| p.model_position())
                })
                .unwrap_or([0.; 3]);
            // Frame the model from its bounds when the class lacks authored chase keys.
            let mut bounds = three::Box3::default();
            for part in &model.bounds {
                bounds.expand_by_point(&part.min);
                bounds.expand_by_point(&part.max);
            }
            let length = bounds.max.z - bounds.min.z;
            let fallback_chase = [
                fallback_eye[0],
                bounds.max.y + length * 0.25,
                bounds.min.z - length,
            ];
            let unit_def = find_unit_def(content, &class.name);
            let mut camera = parse_craft_camera(unit_def, fallback_eye, fallback_chase);
            // The pack's seats follow the sections: the hull's first, then each gunner's.
            for (spec, seat) in camera.seats.iter_mut().zip(class.seats.iter().skip(1)) {
                spec.pivot = seat
                    .points
                    .iter()
                    .filter_map(|&i| class.points.get(i))
                    .find(|p| p.property.eq_ignore_ascii_case("TurretNodeName"))
                    .and_then(|p| p.model_position())
                    .map(|[x, y, z]| Vector3::new(x, y, z));
            }
            let rig = CraftRig::load(content, bridge, textures, &class.name, cascades)?;
            craft.push(Craft {
                class: audio_control::swbf2::zero_hash(&class.name),
                model,
                rig,
                camera,
                rocket: rocket_camera(unit_def),
            });
        }
        let gun_eyes = pack
            .classes
            .values()
            .filter_map(|c| {
                let eye = c
                    .points
                    .iter()
                    .find(|p| p.property.eq_ignore_ascii_case("EyePointOffset"))?;
                let point = eye.model_position()?;
                Some((audio_control::swbf2::zero_hash(&c.name), point))
            })
            .collect();
        Ok(Some(Self {
            gun_eyes,
            craft,
            drawn: Vec::new(),
            bone_matrices: Vec::new(),
            props: Vec::new(),
            collision,
            world,
            chase: ChaseSolver::default(),
            rocket_chase: ChaseSolver::default(),
            rocket_last: None,
            death_cam: None,
            foot_view: false,
            chair_aim: None,
            trace: std::env::var_os("BFE_DEV_SWBF2_CAMERA_TRACE")
                .and_then(|path| std::fs::File::create(path).ok())
                .map(std::io::BufWriter::new),
        }))
    }

    pub fn bind_props(&mut self, content: &BattleContent, scene: &crate::static_scene::Admission) {
        use bfe_render::instances::REC_WORLD;
        let Some(map) = content
            .content
            .battle
            .swbf2
            .as_ref()
            .map(|s| &s.map_admission)
        else {
            return;
        };
        for object in map
            .placed
            .iter()
            .filter(|o| map.world_animations.animates_object(&o.name))
        {
            let mut inverse = Matrix4 {
                elements: object.matrix,
            };
            inverse.invert();
            for (index, name) in scene.record_names.iter().enumerate() {
                if name
                    .as_ref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(&object.name))
                {
                    let mut world = Matrix4::default();
                    world
                        .elements
                        .copy_from_slice(&scene.records[index][REC_WORLD..REC_WORLD + 16]);
                    let mut local = inverse;
                    local.multiply(&world);
                    self.props.push((object.index, index, local));
                }
            }
        }
    }

    pub fn place_props(
        &self,
        scene: &mut crate::static_scene::Admission,
        frame: &Frame,
        alpha: f64,
    ) {
        use bfe_render::instances::REC_WORLD;
        for &(instance, record, local) in &self.props {
            let Some(elements) = frame.placement(instance, alpha) else {
                continue;
            };
            let mut m = Matrix4 { elements };
            m.multiply(&local);
            scene.records[record][REC_WORLD..REC_WORLD + 16].copy_from_slice(&m.elements);
        }
    }

    pub fn reserve(&mut self, fighters: usize) {
        self.drawn.reserve(fighters);
        let bones = self.craft.iter().filter_map(|c| c.rig.as_ref()).map(|r| r.bones.len()).max().unwrap_or(0);
        self.bone_matrices.reserve(fighters * bones);
    }

    /// Draws a craft at `matrix`, its rig posed at `lift`.
    fn push_drawn(&mut self, craft: usize, matrix: Matrix4, lift: f64) {
        let bones = self.bone_matrices.len();
        if let Some(rig) = self.craft[craft].rig.as_mut() {
            rig.pose_at(lift);
            for &(bone, _) in &rig.bones {
                let mut world = matrix;
                world.multiply(&rig.bone_matrix(bone));
                self.bone_matrices.push(world);
            }
        }
        self.drawn.push(Drawn { craft, matrix, bones });
    }

    /// World-space reticle aim target for the currently piloted or gunned space craft.
    pub fn pilot_aim(&self, camera: &MapCamera) -> Option<crate::online::hud::Aim> {
        let (dir, project_x) = self.chase.aim.or(self.chair_aim.map(|dir| (dir, true)))?;
        Some(crate::online::hud::Aim {
            point: [
                camera.position[0] + 1024.0 * dir.x,
                camera.position[1] + 1024.0 * dir.y,
                camera.position[2] + 1024.0 * dir.z,
            ],
            project_x,
        })
    }

    /// The camera at a chair gun aimed at `aim` (yaw, pitch), its gun placed
    /// as `frame` places it, or as the chair was found.
    fn apply_chair_camera(&mut self, chair: Chair, aim: [f64; 2], frame: Option<&Frame>, alpha: f64, camera: &mut MapCamera) {
        let base = Matrix4 {
            elements: frame
                .and_then(|f| f.placement(chair.instance, alpha))
                .unwrap_or(chair.matrix),
        };
        let eye = self
            .gun_eyes
            .get(&chair.class)
            .copied()
            .unwrap_or(chair.center);
        let mut pivot = Vector3::new(eye[0], eye[1], eye[2]);
        pivot.apply_matrix4(&base);
        let mut facing = base;
        let mut rot = Matrix4::default();
        facing
            .multiply(rot.make_rotation_y(aim[0]))
            .multiply(rot.make_rotation_x(-aim[1]))
            .set_position_v3(&pivot);
        // The chair's reticle: the camera looks along the gun, so the gun's forward
        // axis is the aim the HUD projects (as a piloted craft's chase aim is).
        let [fx, fy, fz] = [facing.elements[8], facing.elements[9], facing.elements[10]];
        let length = (fx * fx + fy * fy + fz * fz).sqrt();
        if length > 1e-9 {
            self.chair_aim = Some(Vector3::new(fx / length, fy / length, fz / length));
        }
        let mut position = if camera.first_person {
            Vector3::default()
        } else {
            Vector3::new(0., chair.radius * 0.75, -chair.radius * 2.2)
        };
        position.apply_matrix4(&facing);
        facing
            .multiply(rot.make_rotation_y(std::f64::consts::PI))
            .set_position_v3(&position);
        let mut view = *camera.snapshot();
        view.values[16..32].copy_from_slice(&facing.invert().elements);
        use bfe_render::cameras::layout::shader_frame::EYE;
        view.values[EYE..EYE + 3].copy_from_slice(&[position.x, position.y, position.z]);
        camera.drawn_view(view);
    }

    /// Draws the sampled frame's craft and sets the view. Online,
    /// `own_override` is the predicted own craft or chair, which stands in
    /// for the replicated one.
    pub fn sample(
        &mut self,
        history: &History,
        shown: f64,
        camera: &mut MapCamera,
        dt: f64,
        own_override: Option<&OwnSpaceOverride>,
    ) {
        self.drawn.clear();
        self.bone_matrices.clear();
        self.chair_aim = None;
        let Some((frame, alpha)) = history.sample(shown) else {
            self.chase.reset();
            self.foot_view = false;
            return;
        };
        if let Some(OwnSpaceOverride::Chair { chair, turret_aim }) = own_override {
            // Online: the predicted chair gun's aim.
            self.chase.reset();
            self.apply_chair_camera(*chair, *turret_aim, Some(frame), alpha, camera);
        } else if let Some(chair) = frame.chair {
            self.chase.reset();
            if let Ok(i) = frame
                .turrets
                .binary_search_by_key(&chair.building, |t| t.building)
            {
                let aim = &frame.turrets[i];
                let angle = |axis: usize| {
                    let step = aim.current[axis] - aim.previous[axis];
                    aim.previous[axis] + step.sin().atan2(step.cos()) * alpha
                };
                self.apply_chair_camera(chair, [angle(0), angle(1)], Some(frame), alpha, camera);
            }
        }
        let mut mounted_found = false;
        let mut own_pilot_drawn = false;
        for hull in &frame.hulls {
            let Some(index) = self.craft.iter().position(|c| c.class == hull.class) else {
                continue;
            };
            let craft = &self.craft[index];
            // Online, the predicted own craft is drawn where the prediction puts it.
            let predicted = match own_override {
                Some(&OwnSpaceOverride::Pilot { fighter_id, matrix, gunner_aim, .. }) if fighter_id == hull.id => {
                    own_pilot_drawn = true;
                    Some((matrix, gunner_aim))
                }
                _ => None,
            };
            let matrix = predicted.map_or_else(|| hull.previous.matrix(&hull.current, alpha), |(matrix, _)| matrix);
            let lift = 1.0 - (hull.previous.gear + alpha * (hull.current.gear - hull.previous.gear));
            if frame.mount == Some(hull.id) || predicted.is_some() {
                mounted_found = true;
                let bank = hull.previous.bank + alpha * (hull.current.bank - hull.previous.bank);
                let mut bank_q = Quaternion::default();
                bank_q.set_from_axis_angle(&Vector3::new(0.0, 0.0, 1.0), bank);
                let (unbanked_q, drawn_q, unbanked_pos) = match predicted {
                    Some((matrix, _)) => {
                        let (mut position, mut drawn, mut scale) = (Vector3::default(), Quaternion::default(), Vector3::default());
                        matrix.decompose(&mut position, &mut drawn, &mut scale);
                        let mut unbanked = drawn;
                        unbanked.multiply(bank_q.clone().invert());
                        (unbanked, drawn, position)
                    }
                    None => {
                        let unbanked_q = hull.previous.unbanked_quaternion(&hull.current, alpha);
                        let mut drawn_q = unbanked_q;
                        drawn_q.multiply(&bank_q);
                        let unbanked_pos = Vector3::new(
                            hull.previous.position[0]
                                + alpha * (hull.current.position[0] - hull.previous.position[0]),
                            hull.previous.position[1]
                                + alpha * (hull.current.position[1] - hull.previous.position[1]),
                            hull.previous.position[2]
                                + alpha * (hull.current.position[2] - hull.previous.position[2]),
                        );
                        (unbanked_q, drawn_q, unbanked_pos)
                    }
                };
                let gunner_aim = match predicted {
                    Some((_, aim)) => aim.map(|d| Vector3::new(d[0], d[1], d[2])),
                    None => frame.gunner_aim.map(|[previous, current]| {
                        Vector3::new(
                            previous[0] + (current[0] - previous[0]) * alpha,
                            previous[1] + (current[1] - previous[1]) * alpha,
                            previous[2] + (current[2] - previous[2]) * alpha,
                        )
                    }),
                };
                let mut cast = WorldCast {
                    collision: self.collision.clone(),
                    world: self.world,
                };
                // Steering a guided rocket, the view is the rocket's own chase
                // camera; when it is gone, its death camera, then the craft's.
                let seat = (hull.id, hull.generation, frame.mount_seat);
                if frame.own_rocket.is_none() {
                    if let Some((view, end, forward, spec, owner)) = self.rocket_last.take() {
                        if spec.pause > 0.0 && owner == seat {
                            let back = Vector3::new(-forward.x, -forward.y, -forward.z);
                            self.death_cam = Some(DeathCam { view, end, back, spec, left: spec.pause, seat });
                        }
                    }
                }
                if let Some(mut death) = self.death_cam.take().filter(|d| d.seat == seat && frame.own_rocket.is_none()) {
                    if death.step(dt, &mut cast) {
                        self.chase.reset();
                        let out = death.view;
                        let mut eye = Matrix4::default();
                        eye.compose(&out.position, &out.rotation, &Vector3::new(1.0, 1.0, 1.0));
                        let mut view = *camera.snapshot();
                        let projection = projection_from_hfov(out.hfov, camera.aspect(), out.near, camera.far());
                        view.values[..16].copy_from_slice(&projection);
                        view.values[16..32].copy_from_slice(&eye.invert().elements);
                        use bfe_render::cameras::layout::shader_frame::EYE;
                        view.values[EYE..EYE + 3].copy_from_slice(&[out.position.x, out.position.y, out.position.z]);
                        camera.drawn_view(view);
                        self.death_cam = Some(death);
                        self.push_drawn(index, matrix, lift);
                        continue;
                    }
                }
                if let (Some([previous, current, velocity]), Some((rocket, top, death))) = (frame.own_rocket, &craft.rocket) {
                    let at = |k: usize| previous[k] + (current[k] - previous[k]) * alpha;
                    let position = Vector3::new(at(0), at(1), at(2));
                    let v = Vector3::new(velocity[0], velocity[1], velocity[2]);
                    let facing = if v.length_sq() > 1e-8 { look_basis(norm(v), UP) } else { unbanked_q };
                    self.chase.reset();
                    let out = self.rocket_chase.step(
                        rocket,
                        0,
                        position,
                        facing,
                        facing,
                        v,
                        *top,
                        0.0,
                        SpaceTrick::None,
                        None,
                        camera.first_person,
                        (hull.id, hull.generation, u8::MAX),
                        dt,
                        &mut cast,
                    );
                    self.chase.aim = Some((out.aim_dir, out.project_x));
                    let forward = if v.length_sq() > 1e-8 { norm(v) } else { rotate(Vector3::new(0.0, 0.0, 1.0), facing) };
                    self.rocket_last = Some((out, position, forward, *death, seat));
                    let mut eye = Matrix4::default();
                    eye.compose(&out.position, &out.rotation, &Vector3::new(1.0, 1.0, 1.0));
                    let mut view = *camera.snapshot();
                    let projection = projection_from_hfov(out.hfov, camera.aspect(), out.near, camera.far());
                    view.values[..16].copy_from_slice(&projection);
                    view.values[16..32].copy_from_slice(&eye.invert().elements);
                    use bfe_render::cameras::layout::shader_frame::EYE;
                    view.values[EYE..EYE + 3].copy_from_slice(&[out.position.x, out.position.y, out.position.z]);
                    camera.drawn_view(view);
                    self.push_drawn(index, matrix, lift);
                    continue;
                }
                self.rocket_chase.reset();
                if self.foot_view {
                    // Boarding: the soldier's view is where the pilot's chase
                    // camera starts, and it eases out to its point from there.
                    self.chase.carry(Vector3::new(camera.position[0], camera.position[1], camera.position[2]));
                }
                let out = self.chase.step(
                    &craft.camera,
                    frame.mount_seat,
                    unbanked_pos,
                    unbanked_q,
                    drawn_q,
                    Vector3::new(hull.velocity[0], hull.velocity[1], hull.velocity[2]),
                    hull.max_speed,
                    hull.boost_speed,
                    hull.trick,
                    gunner_aim,
                    camera.first_person,
                    (hull.id, hull.generation, frame.mount_seat),
                    dt,
                    &mut cast,
                );
                self.chase.aim = Some((out.aim_dir, out.project_x));
                if let Some(trace) = &mut self.trace {
                    use std::io::Write;
                    let axis = |v: [f64; 3], q: Quaternion| rotate(Vector3::new(v[0], v[1], v[2]), q);
                    let (f, u) = (axis([0.0, 0.0, 1.0], unbanked_q), axis([0.0, 1.0, 0.0], unbanked_q));
                    let look = axis([0.0, 0.0, -1.0], out.rotation);
                    let _ = writeln!(
                        trace,
                        "{dt:.6},{:?},{},{},{:.4},{:.4},{:.4},{:.5},{:.5},{:.5},{:.5},{:.5},{:.5},{:.4},{:.4},{:.4},{:.5},{:.5},{:.5},{:.4}",
                        hull.state, frame.mount_seat, u8::from(camera.first_person),
                        unbanked_pos.x, unbanked_pos.y, unbanked_pos.z, f.x, f.y, f.z, u.x, u.y, u.z,
                        out.position.x, out.position.y, out.position.z, look.x, look.y, look.z, out.hfov,
                    );
                }
                let mut eye = Matrix4::default();
                eye.compose(&out.position, &out.rotation, &Vector3::new(1.0, 1.0, 1.0));
                let mut view = *camera.snapshot();
                let projection =
                    projection_from_hfov(out.hfov, camera.aspect(), out.near, camera.far());
                view.values[..16].copy_from_slice(&projection);
                view.values[16..32].copy_from_slice(&eye.invert().elements);
                use bfe_render::cameras::layout::shader_frame::EYE;
                view.values[EYE..EYE + 3]
                    .copy_from_slice(&[out.position.x, out.position.y, out.position.z]);
                camera.drawn_view(view);
                if camera.first_person {
                    continue;
                }
            }
            self.push_drawn(index, matrix, lift);
        }
        // The predicted own craft before the replicated frame holds it (its
        // own body came first): drawn where the prediction puts it.
        if let (false, Some(&OwnSpaceOverride::Pilot { class, matrix, .. })) = (own_pilot_drawn, own_override) {
            if let Some(index) = self.craft.iter().position(|c| c.class == class) {
                if !camera.first_person {
                    self.push_drawn(index, matrix, 1.0);
                }
            }
        }
        if !mounted_found {
            self.chase.reset();
        }
        self.foot_view = !mounted_found && frame.chair.is_none();
    }

    pub fn budget(&self) -> usize {
        self.drawn
            .iter()
            .map(|d| {
                let craft = &self.craft[d.craft];
                craft.rig.as_ref().map_or(craft.model.rows.len(), CraftRig::rows)
            })
            .sum()
    }

    pub fn append(
        &self,
        bridge: &mut Bridge,
        mut records: usize,
        mut bones: usize,
    ) -> (usize, usize) {
        for d in &self.drawn {
            let craft = &self.craft[d.craft];
            let Some(rig) = &craft.rig else {
                (records, bones) = craft.model.append(bridge, &d.matrix.elements, records, bones);
                continue;
            };
            if let Some(model) = &rig.unboned {
                (records, bones) = model.append(bridge, &d.matrix.elements, records, bones);
            }
            for (i, (_, model)) in rig.bones.iter().enumerate() {
                (records, bones) = model.append(bridge, &self.bone_matrices[d.bones + i].elements, records, bones);
            }
        }
        (records, bones)
    }
}

pub(crate) fn mounted(frame: &Frame) -> Option<&Hull> {
    let id = frame.mount?;
    frame.hulls.iter().find(|f| f.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The chase camera in open flight for one prepared class, at three frame
    /// steps: level at MidSpeed, a full-rate turn, a roll and BoostSpeed. Reads
    /// the prepared side units file `BFE_CAMERA_UNITS` and the class
    /// `BFE_CAMERA_CLASS`; prints metres behind and above the craft, metres
    /// beside it and the aim below the nose.
    #[test]
    #[ignore]
    fn chase_camera_moments_probe() {
        let (Ok(file), Ok(class)) = (std::env::var("BFE_CAMERA_UNITS"), std::env::var("BFE_CAMERA_CLASS")) else {
            panic!("the probe needs BFE_CAMERA_UNITS (a prepared side's units file) and BFE_CAMERA_CLASS");
        };
        let units = bfe_content::json::parse(&std::fs::read_to_string(file).unwrap()).unwrap();
        let def = units.get("units").and_then(|u| u.get(&class)).expect("the class");
        let number = |key: &str| {
            def.get("properties").and_then(Value::as_array).into_iter().flatten()
                .find(|p| p.at(0).and_then(Value::as_str).is_some_and(|k| k.eq_ignore_ascii_case(key)))
                .map(|p| bfe_content::js::to_number(p.at(1)))
                .unwrap_or(0.0)
        };
        let (mid, max, boost, turn, roll) =
            (number("MidSpeed"), number("MaxSpeed"), number("BoostSpeed"), number("TurnRate"), number("RollRate"));
        let spec = parse_craft_camera(Some(def), [0.0; 3], [0.0, 2.0, -8.0]);
        for steps in [60.0, 144.0, 300.0] {
            let dt = 1.0 / steps;
            let mut solver = ChaseSolver::default();
            let (mut position, mut yaw, mut bank, mut speed) = (Vector3::new(0.0, 0.0, 0.0), 0.0_f64, 0.0_f64, mid);
            let mut t = 0.0;
            let mut report = |name: &str, out: &ChaseOutput, q: Quaternion, position: Vector3| {
                let f = rotate(Vector3::new(0.0, 0.0, 1.0), q);
                let u = rotate(UP, q);
                let left = rotate(Vector3::new(1.0, 0.0, 0.0), q);
                let d = sub(out.position, position);
                let look = rotate(Vector3::new(0.0, 0.0, -1.0), out.rotation);
                println!(
                    "{class} {steps} {name}: behind {:.2} above {:.2} beside {:.2} aim down {:.2} deg aside {:.2} deg hfov {:.1}",
                    -(d.x * f.x + d.y * f.y + d.z * f.z), d.x * u.x + d.y * u.y + d.z * u.z, d.x * left.x + d.y * left.y + d.z * left.z,
                    (-(look.x * u.x + look.y * u.y + look.z * u.z)).asin().to_degrees(),
                    (look.x * left.x + look.y * left.y + look.z * left.z).atan2(look.x * f.x + look.y * f.y + look.z * f.z).to_degrees(),
                    out.hfov.to_degrees()
                );
            };
            let phases: [(&str, f64, f64, f64, f64); 4] =
                [("level flight", 4.0, 0.0, 0.0, mid), ("full turn", 1.0, turn, 0.0, mid), ("roll", 0.5, 0.0, roll, mid), ("boost", 3.0, 0.0, 0.0, boost)];
            for (name, length, yaw_rate, roll_rate, target) in phases {
                let end = t + length;
                let mut last = None;
                while t < end - 1e-9 {
                    t += dt;
                    yaw += yaw_rate * dt;
                    bank += roll_rate * dt;
                    speed = target;
                    let mut q = Quaternion::default();
                    q.set_from_axis_angle(&UP, yaw);
                    let mut r = Quaternion::default();
                    r.set_from_axis_angle(&Vector3::new(0.0, 0.0, 1.0), bank);
                    q.multiply(&r);
                    let f = rotate(Vector3::new(0.0, 0.0, 1.0), q);
                    position = add(position, scale(f, speed * dt));
                    let out = solver.step(&spec, 0, position, q, q, scale(f, speed), max, boost, SpaceTrick::None, None, false,
                        (0, 1, 0), dt, &mut ClearSpace);
                    last = Some((out, q, position));
                }
                let (out, q, at) = last.unwrap();
                report(name, &out, q, at);
                if name == "roll" {
                    bank = 0.0;
                }
            }
        }
    }

    struct ClearSpace;
    impl Cast for ClearSpace {
        fn cast(&mut self, _: Vector3, _: Vector3, _: f64, _: bool) -> Option<f64> {
            None
        }
    }

    struct CeilingWall {
        max_y: f64,
    }
    impl Cast for CeilingWall {
        fn cast(&mut self, origin: Vector3, direction: Vector3, far: f64, _: bool) -> Option<f64> {
            if direction.y <= 1e-6 {
                return None;
            }
            let d = (self.max_y - origin.y) / direction.y;
            (d > 0.0 && d <= far).then_some(d)
        }
    }

    fn synthetic_fighter_def() -> Value {
        bfe_content::json::parse(
            r#"{
                "vehicleSections": [
                    {
                        "kind": "chassis",
                        "name": "BODY",
                        "properties": [
                            ["FirstPersonFOV", "54"],
                            ["ThirdPersonFOV", "46"],
                            ["FOVEffect1st", "88"],
                            ["FOVEffect3rd", "86"],
                            ["EyePointOffset", "0.0 1.2 -1.5"],
                            ["TrackCenter", "0.0 2.0 -6.0"],
                            ["FOVEFFectMinCamOffset3rd", "0.0 3.0 6.0"],
                            ["FOVEFFectMaxCamOffset3rd", "0.0 3.0 -11.0"],
                            ["TiltValue", "6.5"],
                            ["AimTension", "17.0"],
                            ["MoveTensionX", "2.5"],
                            ["MoveTensionY", "2.0 10.0"],
                            ["MoveTensionZ", "4.0"],
                            ["TrickFlipCameraDetach", "0.3"],
                            ["TrickSideRollCameraDetach", "0.6"]
                        ]
                    },
                    {
                        "kind": "seat",
                        "name": "TURRET1",
                        "properties": [
                            ["EyePointOffset", "0.0 4.5 0.5"],
                            ["TrackCenter", "0.0 5.0 -4.0"],
                            ["TrackOffset", "0.0 1.5 11.0"],
                            ["TiltValue", "6.0"],
                            ["ThirdPersonFOV", "58"]
                        ]
                    }
                ]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn a_rockets_death_camera_holds_its_turn_backs_off_to_its_distance_and_ends_after_its_pause() {
        // Invented keys: 20 m, 64 m/s, a one-second pause; steps of 1/16 s.
        let start = || DeathCam {
            view: ChaseOutput {
                position: Vector3::new(0.0, 0.0, -8.0),
                rotation: Quaternion::new(0.0, 0.6, 0.0, 0.8),
                hfov: 1.0,
                near: 0.3,
                aim_dir: Vector3::new(0.0, 0.0, 1.0),
                project_x: false,
            },
            end: Vector3::new(0.0, 0.0, 0.0),
            back: Vector3::new(0.0, 0.0, -1.0),
            spec: DeathCamSpec { min_distance: 20.0, pullback: 64.0, pause: 1.0 },
            left: 1.0,
            seat: (3, 1, 1),
        };
        let mut free = start();
        let mut path = Vec::new();
        while free.step(1.0 / 16.0, &mut ClearSpace) {
            path.push(free.view.position.z);
        }
        assert_eq!(path.len(), 15, "the pause ends the view");
        assert_eq!(&path[..4], &[-12.0, -16.0, -20.0, -20.0]);
        assert_eq!(free.view.position.z, -20.0);
        assert_eq!((free.view.rotation.y, free.view.rotation.w), (0.6, 0.8));
        let mut held = start();
        while held.step(1.0 / 16.0, &mut Share(0.5)) {}
        assert_eq!(held.view.position.z, -8.0);
    }

    #[test]
    fn a_crafts_rig_takes_the_takeoff_clips_first_frame_on_the_deck_and_its_last_in_flight() {
        // An invented rig: a root and a wing; the clip turns the wing about
        // its z axis from 0 to 30 degrees over three frames.
        let bones = bfe_content::json::parse_bytes(
            br#"{"bones":[{"name":"dummyroot","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]},
                {"name":"wing","parent":0,"local":[1,0,0,0,0,1,0,0,0,0,1,0,2,0,0,1]}]}"#,
        )
        .unwrap();
        let desc = bfe_pose::rig::content::hull_records(&Value::Null, &bones, &[], Vec::new(), true);
        let mut rig = HullRig::new(&desc);
        let node = rig.tree.node_mut(rig.root);
        node.matrix_auto_update = false;
        node.matrix = Matrix4::default();
        let mut data = Vec::new();
        for degrees in [0.0_f64, 15.0, 30.0] {
            let half = degrees.to_radians() / 2.0;
            data.extend([0.0, 0.0, half.sin() as f32, half.cos() as f32, 2.0, 0.0, 0.0]);
        }
        let mut craft = CraftRig {
            pose: bfe_pose::Pose::new(2, 0),
            clip: bfe_pose::Clip { frames: 3, fps: 30.0, root_motion: false, tracks: vec![1], data },
            duration: 2.0 / 30.0,
            rig,
            bones: Vec::new(),
            unboned: None,
        };
        let wing = craft.rig.by_name.get("wing").unwrap();
        for (lift, degrees) in [(0.0, 0.0), (0.5, 15.0), (1.0, 30.0), (1.4, 30.0)] {
            craft.pose_at(lift);
            let m = craft.bone_matrix(wing).elements;
            let angle = m[1].atan2(m[0]).to_degrees();
            assert!((angle - degrees).abs() < 1e-3, "lift {lift}: {angle} degrees");
            assert!((m[12] - 2.0).abs() < 1e-9);
        }
    }

    #[test]
    fn parses_body_and_gunner_camera_sections() {
        let def = synthetic_fighter_def();
        let spec = parse_craft_camera(Some(&def), [0.0; 3], [0.0, 5.0, -10.0]);
        assert!((spec.body.first_fov - 54.0 * DEG).abs() < 1e-9);
        assert!((spec.body.third_fov - 46.0 * DEG).abs() < 1e-9);
        assert!((spec.fov_effect_1st - 88.0 * DEG).abs() < 1e-9);
        assert!((spec.fov_effect_3rd - 86.0 * DEG).abs() < 1e-9);
        assert_eq!(spec.body.track_center, Vector3::new(0.0, 2.0, -6.0));
        assert_eq!(spec.min_cam_offset_3rd, Some(Vector3::new(0.0, 3.0, 6.0)));
        assert_eq!(spec.max_cam_offset_3rd, Some(Vector3::new(0.0, 3.0, -11.0)));
        assert!((spec.body.tilt - 6.5 * DEG).abs() < 1e-9);
        assert_eq!(spec.body.move_tension, [2.5, 2.5, 2.0, 10.0, 4.0, 4.0]);
        assert_eq!(spec.seats.len(), 1);
        assert_eq!(spec.seats[0].track_center, Vector3::new(0.0, 5.0, -4.0));
        assert_eq!(spec.seats[0].track_offset, Vector3::new(0.0, 1.5, 11.0));
        assert!((spec.seats[0].third_fov - 58.0 * DEG).abs() < 1e-9);
    }

    #[test]
    fn chase_camera_trails_by_move_tension_and_widens_fov_with_speed() {
        let def = synthetic_fighter_def();
        let spec = parse_craft_camera(Some(&def), [0.0; 3], [0.0; 3]);
        let mut solver = ChaseSolver::default();
        let mut world = ClearSpace;
        let q = Quaternion::new(0.0, 0.0, 0.0, 1.0);
        let dt = 1.0 / 60.0;

        // Rest (0 m/s): sits at TrackCenter + rotate(MinCamOffset3rd, cam_q).
        let rest = solver.step(
            &spec,
            0,
            Vector3::default(),
            q,
            q,
            Vector3::default(),
            80.0,
            120.0,
            SpaceTrick::None,
            None,
            false,
            (1, 1, 0),
            dt,
            &mut world,
        );
        assert!((rest.hfov - 46.0 * DEG).abs() < 1e-9);
        assert!(rest.position.z < -11.0 && rest.position.z > -12.5);
        assert!(rest.position.y > 5.0 && rest.position.y < 6.0);

        // Steady boost (120 m/s): TrackOffset moves to (0,3,-11), MoveTensionZ=4.0 lags 30 m behind.
        let mut ship_z = 0.0;
        let mut last = rest;
        for _ in 0..360 {
            ship_z += 120.0 * dt;
            last = solver.step(
                &spec,
                0,
                Vector3::new(0.0, 0.0, ship_z),
                q,
                q,
                Vector3::new(0.0, 0.0, 120.0),
                80.0,
                120.0,
                SpaceTrick::None,
                None,
                false,
                (1, 1, 0),
                dt,
                &mut world,
            );
        }
        let rel_z = last.position.z - ship_z;
        // Expected steady-state trail: TrackCenter.z (-6) + rotated MaxOffset.z (11 cos t + 3 sin t = +11.22 at the
        // pilot tilt) - 120/4.0 (-30.0) = -24.78 m.
        assert!((rel_z - (-24.78)).abs() < 1.0, "rel_z={rel_z}");
        assert!((last.hfov - 86.0 * DEG).abs() < 1e-3, "hfov={}", last.hfov / DEG);
    }

    #[test]
    fn a_boarding_pilots_camera_eases_out_from_the_soldiers_view_to_its_chase_point() {
        let def = synthetic_fighter_def();
        let spec = parse_craft_camera(Some(&def), [0.0; 3], [0.0; 3]);
        let q = Quaternion::new(0.0, 0.0, 0.0, 1.0);
        let dt = 1.0 / 60.0;
        let mut world = ClearSpace;
        let step = |solver: &mut ChaseSolver, world: &mut ClearSpace| {
            solver.step(&spec, 0, Vector3::default(), q, q, Vector3::default(), 80.0, 120.0, SpaceTrick::None, None,
                false, (1, 1, 0), dt, world)
        };
        let point = step(&mut ChaseSolver::default(), &mut world);

        let soldier = Vector3::new(1.5, 1.7, -2.0);
        let gap = soldier.distance_to(&point.position);
        let mut solver = ChaseSolver::default();
        solver.carry(soldier);
        let first = step(&mut solver, &mut world);
        assert!(first.position.distance_to(&soldier) < 0.1 * gap, "{:?}", first.position);
        assert!((first.rotation.dot(&point.rotation).abs() - 1.0).abs() < 1e-9, "the aim turns at once");
        let mut out = first;
        for _ in 1..15 {
            out = step(&mut solver, &mut world);
        }
        let left = out.position.distance_to(&point.position);
        assert!(left > 0.3 * gap && left < 0.9 * gap, "a quarter second in: {left} of {gap}");
        for _ in 15..180 {
            out = step(&mut solver, &mut world);
        }
        assert!(out.position.distance_to(&point.position) < 0.05, "{:?}", out.position);

        // A view farther than the snap distance from the craft is not carried.
        let mut solver = ChaseSolver::default();
        solver.carry(Vector3::new(0.0, 0.0, -400.0));
        assert!(step(&mut solver, &mut world).position.distance_to(&point.position) < 1e-9);
        // Nor is one carried into a camera that already follows this seat.
        let mut solver = ChaseSolver::default();
        step(&mut solver, &mut world);
        solver.carry(soldier);
        assert!(step(&mut solver, &mut world).position.distance_to(&point.position) < 1e-9);
    }

    #[test]
    fn chase_camera_ignores_visual_bank_and_pitches_down_by_tilt() {
        let def = synthetic_fighter_def();
        let spec = parse_craft_camera(Some(&def), [0.0; 3], [0.0; 3]);
        let mut solver = ChaseSolver::default();
        let mut world = ClearSpace;
        let unbanked = Quaternion::new(0.0, 0.0, 0.0, 1.0);
        let mut banked = Quaternion::default();
        banked.set_from_axis_angle(&Vector3::new(0.0, 0.0, 1.0), 0.7);

        let out = solver.step(
            &spec,
            0,
            Vector3::default(),
            unbanked,
            banked,
            Vector3::default(),
            80.0,
            120.0,
            SpaceTrick::None,
            None,
            false,
            (2, 1, 0),
            1.0 / 60.0,
            &mut world,
        );
        // Camera right axis stays completely horizontal despite 0.7 rad visual hull bank.
        let cam_right = rotate(Vector3::new(1.0, 0.0, 0.0), out.rotation);
        assert!(cam_right.y.abs() < 1e-9, "camera banked: {cam_right:?}");
        // Camera forward (-Z) pitches down by the pilot camera's fixed tilt, not the section's 5 deg.
        let cam_fwd = rotate(Vector3::new(0.0, 0.0, -1.0), out.rotation);
        assert!(
            (cam_fwd.y - -PILOT_CHASE_TILT.sin()).abs() < 1e-9,
            "cam_fwd={cam_fwd:?}"
        );
    }

    #[test]
    fn chase_camera_turns_with_the_hull_at_once_outside_a_trick() {
        let def = synthetic_fighter_def();
        let spec = parse_craft_camera(Some(&def), [0.0; 3], [0.0; 3]);
        let mut solver = ChaseSolver::default();
        let mut world = ClearSpace;
        let level = Quaternion::new(0.0, 0.0, 0.0, 1.0);
        let mut turned = Quaternion::default();
        turned.set_from_axis_angle(&Vector3::new(0.0, 1.0, 0.0), 0.3);
        for (q, key) in [(level, 0.0), (turned, 1.0)] {
            let out = solver.step(
                &spec,
                0,
                Vector3::new(0.0, 0.0, key),
                q,
                q,
                Vector3::new(0.0, 0.0, 60.0),
                80.0,
                120.0,
                SpaceTrick::None,
                None,
                false,
                (5, 1, 0),
                1.0 / 144.0,
                &mut world,
            );
            let nose = rotate(Vector3::new(0.0, 0.0, 1.0), q);
            let cam_fwd = rotate(Vector3::new(0.0, 0.0, -1.0), out.rotation);
            let horizontal = norm(Vector3::new(cam_fwd.x, 0.0, cam_fwd.z));
            assert!(horizontal.distance_to(&nose) < 1e-9, "camera {cam_fwd:?} hull {nose:?}");
        }
    }

    #[test]
    fn a_tricks_camera_detach_eases_the_aim_at_twenty_per_second_tightening_to_fifty() {
        assert_eq!(detached_aim_tension(1.0), 20.0);
        assert_eq!(detached_aim_tension(0.5), 35.0);
        assert_eq!(detached_aim_tension(0.0), 50.0);
        // Through the solver: with the hull held still in a flip that detaches
        // the camera, each frame leaves about exp(-20 dt) of the gap to the aim
        // (the point it looks toward moves a little with the camera; at 1 or
        // 50 per second the share left would be 0.98 or 0.43).
        let def = synthetic_fighter_def();
        let spec = parse_craft_camera(Some(&def), [0.0; 3], [0.0; 3]);
        assert!(spec.trick_flip_detach > 0.0);
        let mut solver = ChaseSolver::default();
        let mut world = ClearSpace;
        let q = Quaternion::new(0.0, 0.0, 0.0, 1.0);
        let dt = 1.0 / 60.0;
        let mut step = |trick| {
            solver
                .step(
                    &spec,
                    0,
                    Vector3::new(0.0, 0.0, 0.0),
                    q,
                    q,
                    Vector3::new(0.0, 0.0, 60.0),
                    80.0,
                    120.0,
                    trick,
                    None,
                    false,
                    (5, 1, 0),
                    dt,
                    &mut world,
                )
                .rotation
        };
        let angle = |a: Quaternion, b: Quaternion| {
            2.0 * (a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w).abs().min(1.0).acos()
        };
        // Settled behind the hull first, so the point it looks toward holds still.
        for _ in 0..1200 {
            step(SpaceTrick::None);
        }
        let rest = step(SpaceTrick::None);
        let first = step(SpaceTrick::Loop);
        let second = step(SpaceTrick::Loop);
        let third = step(SpaceTrick::Loop);
        let (a1, a2, a3) = (angle(rest, first), angle(first, second), angle(second, third));
        assert!(a1 > 1e-4, "the detached aim moves off the nose: {a1}");
        assert!((a2 / a1 - (-20.0 * dt).exp()).abs() < 0.02, "{a1} {a2}");
        assert!((a3 / a2 - (-20.0 * dt).exp()).abs() < 0.02, "{a2} {a3}");
    }

    #[test]
    fn cockpit_view_places_eye_on_drawn_hull_and_scales_hor_plus_projection() {
        let def = synthetic_fighter_def();
        let spec = parse_craft_camera(Some(&def), [0.0; 3], [0.0; 3]);
        let mut solver = ChaseSolver::default();
        let mut world = ClearSpace;
        let q = Quaternion::new(0.0, 0.0, 0.0, 1.0);
        let out = solver.step(
            &spec,
            0,
            Vector3::new(10.0, 20.0, 30.0),
            q,
            q,
            Vector3::new(0.0, 0.0, 120.0),
            80.0,
            120.0,
            SpaceTrick::None,
            None,
            true,
            (3, 1, 0),
            1.0 / 60.0,
            &mut world,
        );
        assert!((out.position.x - 10.0).abs() < 1e-9);
        assert!((out.position.y - 21.2).abs() < 1e-9);
        assert!((out.position.z - 28.5).abs() < 1e-9);
        assert_eq!(out.near, 0.05);
        assert!((out.hfov - 88.0 * DEG).abs() < 1e-9);

        // Hor+ scaling preserves vertical FOV across 4:3 and 16:9 viewports.
        let p43 = projection_from_hfov(48.0 * DEG, 4.0 / 3.0, 0.3, 1000.0);
        let p169 = projection_from_hfov(48.0 * DEG, 16.0 / 9.0, 0.3, 1000.0);
        assert!((p43[5] - p169[5]).abs() < 1e-12);
        assert!(((p169[5] / p169[0]) - (16.0 / 9.0)).abs() < 1e-12);
    }

    #[test]
    fn a_gunners_camera_hangs_from_his_turret_node_and_looks_along_his_aim() {
        let seat = SeatCameraSpec {
            track_center: Vector3::new(0.0, 0.5, -1.0),
            pivot: Some(Vector3::new(2.0, -1.0, 3.0)),
            ..Default::default()
        };
        let spec = CraftCameraSpec { seats: vec![seat], ..Default::default() };
        let mut solver = ChaseSolver::default();
        let q = Quaternion::new(0.0, 0.0, 0.0, 1.0);
        // Aimed out to the left (the hull's +X): one metre back along the aim from the
        // node, half a metre above it.
        let out = solver.step(
            &spec,
            1,
            Vector3::new(10.0, 0.0, 0.0),
            q,
            q,
            Vector3::default(),
            80.0,
            120.0,
            SpaceTrick::None,
            Some(Vector3::new(1.0, 0.0, 0.0)),
            false,
            (6, 1, 1),
            1.0 / 60.0,
            &mut ClearSpace,
        );
        assert!(out.position.distance_to(&Vector3::new(11.0, -0.5, 3.0)) < 1e-9, "{:?}", out.position);
        let look = rotate(Vector3::new(0.0, 0.0, -1.0), out.rotation);
        assert!(look.distance_to(&Vector3::new(1.0, 0.0, 0.0)) < 1e-9, "{look:?}");
    }

    /// A wall that every ray meets at a set share of its length.
    struct Share(f64);
    impl Cast for Share {
        fn cast(&mut self, _: Vector3, _: Vector3, far: f64, _: bool) -> Option<f64> {
            (self.0 < 1.0).then_some(self.0 * far)
        }
    }

    #[test]
    fn the_boom_pulls_in_at_once_holds_small_gains_and_lets_out_at_three_quarters_a_second() {
        let mut solver = ChaseSolver { kept: 1.0, rays: [1.0; 5], ..Default::default() };
        let (origin, target) = (Vector3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, -10.3));
        let dt = 1.0 / 60.0;
        let eye_z = |s: &ChaseSolver| -(CHASE_NEAR + s.kept * 10.0);
        // Pulled in at once to the nearest of the last five rays.
        let eye = solver.obstruct(&mut Share(0.4), origin, target, dt);
        assert_eq!(solver.kept, 0.4);
        assert!((eye.z - eye_z(&solver)).abs() < 1e-12);
        // A gain of no more than 0.15 is held while anything is still in the way.
        for _ in 0..4 {
            solver.obstruct(&mut Share(0.5), origin, target, dt);
        }
        assert_eq!(solver.kept, 0.4);
        // Clear again: the old hit stays among the five rays for five frames, then
        // the boom lets out at 0.75 per second.
        for _ in 0..5 {
            solver.obstruct(&mut Share(1.0), origin, target, dt);
        }
        let before = solver.kept;
        solver.obstruct(&mut Share(1.0), origin, target, dt);
        assert!((solver.kept - (before + 0.75 * dt)).abs() < 1e-12, "{before} -> {}", solver.kept);
    }

    #[test]
    fn obstruction_pulls_camera_below_ceiling_and_gunner_uses_seat_section() {
        let def = synthetic_fighter_def();
        let spec = parse_craft_camera(Some(&def), [0.0; 3], [0.0; 3]);
        let mut solver = ChaseSolver::default();
        let mut ceiling = CeilingWall { max_y: 3.5 };
        let q = Quaternion::new(0.0, 0.0, 0.0, 1.0);
        let out = solver.step(
            &spec,
            0,
            Vector3::default(),
            q,
            q,
            Vector3::default(),
            80.0,
            120.0,
            SpaceTrick::None,
            None,
            false,
            (4, 1, 0),
            1.0 / 60.0,
            &mut ceiling,
        );
        // As the 2005 chase update has it, the hit stops the near plane: the eye
        // sits the near distance on along the boom, its near plane on the ceiling.
        let origin = Vector3::new(0.0, 1.5, 0.0);
        let boom = norm(sub(out.position, origin));
        let near_plane = sub(out.position, scale(boom, CHASE_NEAR));
        assert!(near_plane.y <= 3.5 + 1e-9, "near plane through the ceiling: {}", near_plane.y);
        assert!(out.position.y <= 3.5 + CHASE_NEAR, "eye too far past the ceiling: {}", out.position.y);

        let mut clear = ClearSpace;
        let gun = solver.step(
            &spec,
            1,
            Vector3::default(),
            q,
            q,
            Vector3::default(),
            80.0,
            120.0,
            SpaceTrick::None,
            Some(Vector3::new(1.0, 0.0, 0.0)),
            false,
            (4, 1, 1),
            1.0 / 60.0,
            &mut clear,
        );
        assert!(gun.project_x);
        assert!((gun.hfov - 58.0 * DEG).abs() < 1e-9);
        assert!((gun.aim_dir.x - 1.0).abs() < 1e-9);
    }
}
