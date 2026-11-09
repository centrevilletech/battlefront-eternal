//! One page camera phase, replacing game.js's exclusive camera branch,
//! camera-shake.js and the spawn-shot override. First-person attachments
//! consume the pre-shake record; rendering and sound consume the final one.
pub mod death;
pub mod math;
pub mod motion;
pub mod shake;
pub mod soldier;
pub mod spawn;
pub mod vehicle;
pub mod world_death;
use crate::layout::{
    self, camera_command as c, camera_command_kind as k, camera_eye as eyes, camera_frame as f,
    camera_limits as limit, camera_mode as modes, camera_output as o,
};
use math::*;
use serde::Deserialize;
use three::{Matrix4, Quaternion, Vector3};

pub trait Cast {
    fn random(&mut self) -> f64 {
        0.5
    }
    fn cast(
        &mut self,
        origin: Vector3,
        direction: Vector3,
        far: f64,
        static_only: bool,
    ) -> Option<f64>;
}
#[derive(Clone, Copy)]
pub struct Camera {
    pub p: Vector3,
    pub q: Quaternion,
    pub up: Vector3,
    pub fov: f64,
    pub aspect: f64,
    pub near: f64,
    pub far: f64,
}
#[derive(Default, Deserialize)]
pub struct Content {
    pub definitions: Vec<soldier::Def>,
    pub motions: Vec<motion::Keys>,
    #[serde(default, rename = "spawnShots")]
    pub spawn_shots: Vec<spawn::Shot>,
}
pub struct Phase {
    pub input: Vec<f64>,
    pub commands: Vec<f64>,
    pub output: Vec<f64>,
    content: Content,
    fallback: soldier::Def,
    soldier: soldier::Soldier,
    remote: soldier::Soldier,
    motion: motion::Motion,
    vehicle: vehicle::Vehicle,
    turret: vehicle::Turret,
    death: death::Death,
    shake: shake::Shake,
    last_turret: f64,
    pub director: spawn::Director,
    pub skipped: u64,
}
// Cold registration refuses a new class with -1. Float-to-usize saturation
// would otherwise select row zero; rejected and malformed ids use the empty
// section instead, without evicting an admitted camera definition.
fn content_id(value: f64) -> usize {
    if value.is_finite() && value >= 0.0 && value.fract() == 0.0 {
        value as usize
    } else {
        usize::MAX
    }
}
fn vector(a: &[f64], i: usize) -> Vector3 {
    Vector3::new(a[i], a[i + 1], a[i + 2])
}
fn quaternion(a: &[f64], i: usize) -> Quaternion {
    Quaternion::new(a[i], a[i + 1], a[i + 2], a[i + 3])
}
fn matrix(a: &[f64], i: usize) -> Matrix4 {
    let mut m = Matrix4::default();
    m.elements.copy_from_slice(&a[i..i + 16]);
    m
}
fn flag(a: &[f64], i: usize) -> bool {
    a[i] != 0.0
}
fn put_v(a: &mut [f64], i: usize, v: Vector3) {
    a[i..i + 3].copy_from_slice(&[v.x, v.y, v.z]);
}
fn put_q(a: &mut [f64], i: usize, q: Quaternion) {
    a[i..i + 4].copy_from_slice(&[q.x, q.y, q.z, q.w]);
}
// Seat descriptor extraction across camera pose, aim, and weapon tables.
#[allow(clippy::too_many_arguments)]
fn seat(
    a: &[f64],
    eye: usize,
    center: usize,
    offset: usize,
    tilt: usize,
    first: usize,
    third: usize,
    rates: usize,
    aim: usize,
) -> vehicle::Seat {
    let mut r = [0.0; 6];
    r.copy_from_slice(&a[rates..rates + 6]);
    vehicle::Seat {
        eye: vector(a, eye),
        center: vector(a, center),
        offset: vector(a, offset),
        tilt: a[tilt],
        first: a[first],
        third: a[third],
        rates: r,
        aim: a[aim],
    }
}
impl Phase {
    // Ports cold camera construction, independent of a browser or scene.
    // Generated capacities exceed all authored soldier and spawn-shot tables.
    // Excess rows are dropped as complete records, preserving admitted ids.
    // Definitions never compact or move when another class is registered.
    // Authored shot quaternions are normalized once, as ShotDirector did.
    // Motion component arrays share one bounded segment-count rule.
    // The four view machines and all spring storage are constructed here.
    // Camera input/output and ordered command buffers retain their lengths.
    // Registration may allocate; frame advancement below cannot.
    pub fn new(mut content: Content) -> Self {
        // Malformed cold content cannot create an unbounded lookup on a
        // frame. Excess definitions are refused whole, keeping stable ids.
        let skipped = content.definitions.len().saturating_sub(limit::DEFINITIONS)
            + content.spawn_shots.len().saturating_sub(limit::SPAWN_SHOTS)
            + content.motions.len().saturating_sub(limit::MOTIONS);
        content.definitions.truncate(limit::DEFINITIONS);
        content.spawn_shots.truncate(limit::SPAWN_SHOTS);
        for shot in &mut content.spawn_shots {
            shot.normalize();
        }
        content.motions.truncate(limit::MOTIONS);
        for keys in &mut content.motions {
            keys.time.truncate(limit::MOTION_SEGMENTS);
            keys.position.truncate(limit::MOTION_SEGMENTS);
            keys.slope.truncate(limit::MOTION_SEGMENTS);
            keys.turn.truncate(limit::MOTION_SEGMENTS);
            keys.turn_slope.truncate(limit::MOTION_SEGMENTS);
        }
        Self {
            input: vec![0.0; f::STRIDE],
            commands: vec![0.0; c::STRIDE * limit::COMMANDS],
            output: vec![0.0; o::STRIDE],
            content,
            fallback: soldier::Def::default(),
            soldier: soldier::Soldier::new(),
            remote: soldier::Soldier::new(),
            motion: motion::Motion::new(),
            vehicle: vehicle::Vehicle::new(),
            turret: vehicle::Turret::new(),
            death: death::Death::default(),
            shake: shake::Shake::default(),
            last_turret: -1.0,
            director: spawn::Director::default(),
            skipped: skipped as u64,
        }
    }
    // Cold preview/test lifecycle reset, with all linear buffers retained.
    // This mirrors recreating the old camera controllers on a fresh scene.
    // Resource ids remain stable because cold content has not been replaced.
    // A displayed frame never calls reset or discards its accumulated clocks.
    // Shot selection is separately controlled by explicit screen events.
    pub fn reset_cameras(&mut self) {
        self.soldier = soldier::Soldier::new();
        self.remote = soldier::Soldier::new();
        self.motion = motion::Motion::new();
        self.vehicle = vehicle::Vehicle::new();
        self.turret = vehicle::Turret::new();
        self.death = death::Death::default();
        self.shake = shake::Shake::default();
        self.last_turret = -1.0;
    }
    // ShotDirector.show/follow/bestFor are semantic spawn-screen events.
    // Their numeric selection and aim live with the subsequent native pose.
    // A committed revision may still be awaiting the next rendered frame.
    // These calls preserve it before changing the pending selection.
    // They never return a per-frame vector, quaternion or allocated object.
    pub fn show_shot(&mut self, index: f64) -> i32 {
        self.director.show(
            index,
            &self.content.spawn_shots,
            self.input[f::DIRECTOR_REVISION] as u32,
        )
    }
    pub fn follow_shot<T: Math>(&mut self, point: Vector3) -> i32 {
        self.director.follow::<T>(
            point,
            &self.content.spawn_shots,
            self.input[f::DIRECTOR_REVISION] as u32,
        )
    }
    pub fn best_shot(&self, point: Vector3) -> i32 {
        self.director
            .best(point, &self.content.spawn_shots)
            .map_or(-1, |i| i as i32)
    }
    // SoldierCamera.setDef used to retain a class object on the page.
    // The host now resolves that identity to this stable cold numeric handle.
    // Refusing a full table never renames or replaces an earlier definition.
    // Invalid frame handles use the immutable default section instead.
    // The refusal is returned to the cold caller, without a frame exception.
    pub fn add_definition(&mut self, def: soldier::Def) -> Option<usize> {
        if self.content.definitions.len() == limit::DEFINITIONS {
            self.skipped += 1;
            return None;
        }
        let id = self.content.definitions.len();
        self.content.definitions.push(def);
        Some(id)
    }
    // Ports the camera block in game.js, including its pre-shake consumers.
    // This is the only displayed camera advancement for a page frame.
    // Both camera matrices and inverse/projection matrices are outputs.
    // The host never reconstructs a lens or resolves a camera collision.
    // All candidate inputs are facts from already-owned clocks and poses.
    // The raw simulation instance hosts this crate beside the collision store,
    // avoiding one imported cast per corner ray or per death boom.
    // Browser scalar Math imports preserve the host engine's last bits.
    // Native callers use the same policy with their selected scalar library.
    // No content registration, growth or temporary heap storage occurs here.
    // A malformed count can only shorten a prefix or increment diagnostics.
    // Unknown definition ids use the fixed empty-content camera section.
    // Shake births are processed before a camera wins, as they were in JS.
    // A death decays surviving shakes but consumes no cosmetic random turns.
    // A frozen page advances neither motion nor death-shot/shake clocks.
    // Remote, vehicle and own-camera dt retain their original clock rules.
    // Motion observes posture edges even while another perspective is shown.
    // Thus returning to first person never replays an old landing or stance.
    // The motion cue is returned before first-person animation is sampled.
    // Own-eye input may come from prediction rather than the authority basis.
    // These two bases are intentionally not merged or interpolated here.
    // The output first records the view used by arms, charge and death trace.
    // Shake follows, and the spawn-screen pose overrides only the final view.
    // The projection remains the selected underlying camera's lens.
    // This ordering also preserves the listener's spawn-screen position.
    pub fn frame<T: Math, C: Cast>(&mut self, world: &mut C) {
        if self.input[f::VERSION] != layout::VERSION as f64 {
            self.output[o::STATUS] = 1.0;
            return;
        }
        self.output[o::STATUS] = 0.0;
        // Invalid clocks advance nothing. Every externally supplied count
        // and content id is clamped or checked before it indexes storage.
        let dt = if self.input[f::DT].is_finite() {
            self.input[f::DT].max(0.0)
        } else {
            0.0
        };
        let frozen = flag(&self.input, f::FROZEN);
        let active_dt = if frozen { 0.0 } else { dt };
        let n = (self.input[f::COMMANDS] as usize).min(limit::COMMANDS);
        if self.input[f::COMMANDS] > limit::COMMANDS as f64 {
            self.skipped += 1
        }
        for row in 0..n {
            let a = &self.commands[row * c::STRIDE..][..c::STRIDE];
            match a[c::KIND] as u32 {
                k::ADD_SHAKE => {
                    self.shake.add(a[c::AMOUNT], a[c::LENGTH]);
                }
                k::EXPLOSION => {
                    self.shake.explosion::<T>(
                        vector(a, c::POSITION),
                        vector(a, c::TARGET),
                        a[c::RADIUS],
                        a[c::AMOUNT],
                        a[c::LENGTH],
                        a[c::INNER],
                        a[c::OUTER],
                    );
                }
                k::CLEAR_SHAKE => self.shake.clear(),
                _ => self.skipped += 1,
            }
        }
        // The phase reads one immutable input view after consuming commands.
        // The incoming pose is the previously drawn camera for handoff.
        // Aspect is a viewport fact; a nonpositive external value uses one.
        // Near and far retain the selected owner's previous lens values.
        // A flyer cockpit is the only path that changes the near plane here.
        // Every temporary Camera, vector and matrix in this method is inline.
        // Command processing never mutates or reallocates the input buffer.
        let a = &self.input;
        let mut cam = Camera {
            p: vector(a, f::CAMERA_POSITION),
            q: quaternion(a, f::CAMERA_QUATERNION),
            up: vector(a, f::CAMERA_UP),
            fov: a[f::CAMERA_FOV],
            aspect: if a[f::CAMERA_ASPECT] > 0.0 {
                a[f::CAMERA_ASPECT]
            } else {
                1.0
            },
            near: a[f::CAMERA_NEAR],
            far: a[f::CAMERA_FAR],
        };
        let id = content_id(a[f::SOLDIER_DEF]);
        let def = self.content.definitions.get(id).unwrap_or(&self.fallback);
        if self.soldier.definition != id || flag(a, f::SOLDIER_RESET) {
            self.soldier.reset(def);
            self.soldier.definition = id;
        }
        // FirstPersonMotion.update formerly ran before any camera branch.
        // Its air and posture inputs belong to the displayed predicted body.
        // Camera section posture separately follows the live soldier contract.
        // Authority and correction replay do not own these visual state edges.
        // A freeze suppresses cue delivery as well as curve advancement.
        // Returning to a displayed frame resumes the same retained segment.
        // This prevents a paused load from consuming an arms transition.
        if !frozen && !flag(a, f::AUTHORITY) {
            self.motion.update(
                dt,
                (a[f::BODY_POSTURE] as usize).min(2),
                (a[f::AIR] as usize).min(4),
                a[f::DIVE],
                vector(a, f::VELOCITY),
                flag(a, f::OWN_EYES),
                &self.content.motions,
            )
        } else {
            self.motion.cue = 0.0
        }
        let dead = flag(a, f::DEATH_ACTIVE);
        let mode;
        // game.js: death, flown recon, driven vehicle, manned turret, own
        // soldier. These are exclusive even if stale leases overlap a death.
        // DeathCamera.update/apply in camera.js is an authority camera too.
        // Classic AI schedules and aim queries can observe it in a world tick.
        // That clock is independent of DeathShot's visual spring integration.
        // A cold start derives its anchor from the current camera heading.
        // Later ticks consume body and killer facts sampled by the world host.
        // When the body is gone its last middle remains the framed point.
        // The page's death shot instead follows its current ribcage node.
        // The serial identifies a new death even across a missing alive frame.
        // Its killer carries generation-checked identity facts from the host.
        // A stale or absent killer is represented as absence, never a zero id.
        // The fallback pose therefore remains the body shot.
        // Project-only restores change no camera continuation state.
        let mut authoritative_death = None;
        // Cold lens restoration formerly called SoldierCamera.applyFov.
        // It uses the restored horizontal FOV and the current viewport aspect.
        // It does not reset reach, fresh state or the selected body motion.
        // The same output record also composes restored camera matrices.
        // Thus a correction no longer needs a Three camera matrix update.
        // Death-start is another cold command, with the current camera as input.
        if flag(a, f::PROJECT_ONLY) {
            mode = modes::REMOTE as f64;
            cam.fov = 2.0 * T::atan(T::tan(a[f::REMOTE_FOV] / 2.0) / cam.aspect) * (180.0 / PI);
        } else if dead && flag(a, f::AUTHORITY) {
            mode = modes::DEATH as f64;
            let mut state = world_death::State {
                time: a[f::WORLD_DEATH_TIME],
                position: vector(a, f::WORLD_DEATH_POSITION),
                quaternion: quaternion(a, f::WORLD_DEATH_QUATERNION),
                previous_position: Vector3::default(),
                previous_quaternion: Quaternion::default(),
                body: vector(a, f::WORLD_DEATH_BODY),
                anchor: vector(a, f::WORLD_DEATH_ANCHOR),
                killed: flag(a, f::WORLD_DEATH_KILLED),
                hfov: a[f::DEATH_HFOV],
            };
            if flag(a, f::WORLD_START) {
                state = world_death::State::start::<T>(
                    &cam,
                    vector(a, f::FEET),
                    flag(a, f::WORLD_DEATH_KILLED),
                    if flag(a, f::DERIVE_DEATH_FOV) {
                        2.0 * T::atan(T::tan(cam.fov * DEG / 2.0) * cam.aspect)
                    } else {
                        a[f::DEATH_HFOV]
                    },
                    a[f::START_DEAD_DT],
                );
            } else {
                state.frame::<T, C>(
                    dt,
                    flag(a, f::WORLD_HAS_FEET).then(|| vector(a, f::FEET)),
                    flag(a, f::WORLD_HAS_KILLER).then(|| vector(a, f::WORLD_KILLER)),
                    &mut cam,
                    world,
                );
            }
            authoritative_death = Some(state);
        } else if dead {
            mode = modes::DEATH as f64;
            let killer = flag(a, f::KILLER_PRESENT).then(|| death::Killer {
                point: vector(a, f::KILLER_POINT),
                size: a[f::KILLER_SIZE],
                living: flag(a, f::KILLER_LIVING),
                friendly: flag(a, f::KILLER_FRIENDLY),
            });
            self.death.frame::<T, C>(
                active_dt,
                &mut cam,
                &death::Input {
                    serial: a[f::DEATH_SERIAL],
                    hfov: a[f::DEATH_HFOV],
                    classic: flag(a, f::DEATH_CLASSIC),
                    feet: Some(vector(a, f::FEET)),
                    chest: flag(a, f::HAS_CHEST).then(|| vector(a, f::CHEST)),
                    killer,
                },
                world,
            );
        // RemoteDroids.updateCamera used an eased SoldierCamera section.
        // Its first-person eye is behind the droid's canonical aim origin.
        // The explicit continuation override belongs to prediction replay.
        // It restores only that context's fov/reach/fresh scalars.
        // A new content handle or piloted lease starts fresh as before.
        // No first-person body motion is added to a recon camera.
        // Those motions remain specific to the player's soldier body.
        } else if flag(a, f::REMOTE_ACTIVE) {
            mode = modes::REMOTE as f64;
            let id = content_id(a[f::REMOTE_DEF]);
            let def = self.content.definitions.get(id).unwrap_or(&self.fallback);
            if self.remote.definition != id || flag(a, f::REMOTE_RESET) {
                self.remote.reset(def);
                self.remote.definition = id;
            }
            if flag(a, f::REMOTE_OVERRIDE) {
                self.remote.fov = a[f::REMOTE_FOV];
                self.remote.reach = a[f::REMOTE_REACH];
                self.remote.fresh = flag(a, f::REMOTE_FRESH);
            }
            self.remote.frame::<T, C>(
                dt,
                &soldier::Input {
                    feet: vector(a, f::REMOTE_FEET),
                    yaw: a[f::REMOTE_YAW],
                    pitch: a[f::REMOTE_PITCH],
                    posture: 0,
                    zoom: a[f::REMOTE_ZOOM],
                    first: flag(a, f::REMOTE_FIRST),
                    responsive: false,
                    eye: flag(a, f::REMOTE_HAS_EYE).then(|| {
                        let eye = vector(a, f::REMOTE_EYE);
                        if a[f::REMOTE_BACK] != 0.0 {
                            plus(eye, vector(a, f::REMOTE_AIM), -a[f::REMOTE_BACK])
                        } else {
                            eye
                        }
                    }),
                },
                def,
                &motion::Motion::new(),
                &mut cam,
                world,
            );
        } else if flag(a, f::VEHICLE_ACTIVE) {
            mode = modes::VEHICLE as f64;
            let mut bounds = [0.0; 6];
            bounds.copy_from_slice(&a[f::VEHICLE_BOUNDS..f::VEHICLE_BOUNDS + 6]);
            // VehicleCamera.update receives authored section and current hull
            // facts together. Seat indexes are clamped before any arithmetic.
            // Canonical authority seats already contain their native F2 frame.
            // Page seats reconstruct their frame from the displayed hull pose.
            // A repulsor pilot's chase leaves hull lean and bob out of that frame.
            // A non-pilot seat adds its local pivot and heading-relative aim.
            // The first-person eye may instead live in the complete hull frame.
            // This distinction keeps open riders and enclosed cockpits faithful.
            // Flyer orientation, pilot look, boost and damage are independent
            // inputs; their authored blends live in Vehicle::flight below.
            // Collision bounds are fixed content, not a scene query per ray.
            // Self-body exclusions are consumed directly by the ray adapter.
            let mut input = vehicle::Input {
                id: a[f::VEHICLE_ID],
                reset: flag(a, f::VEHICLE_RESET),
                flying: flag(a, f::VEHICLE_FLYING),
                walking: flag(a, f::VEHICLE_WALKING),
                seat: (a[f::VEHICLE_SEAT] as usize).min(255),
                eye_hull: flag(a, f::VEHICLE_EYE_HULL),
                first: flag(a, f::FIRST_PERSON),
                frame: matrix(a, f::VEHICLE_FRAME),
                hull: matrix(a, f::VEHICLE_HULL),
                position: vector(a, f::VEHICLE_POSITION),
                drawn: quaternion(a, f::VEHICLE_DRAWN),
                flight_frame: quaternion(a, f::VEHICLE_FLIGHT_FRAME),
                eye_node: flag(a, f::VEHICLE_HAS_EYE).then(|| vector(a, f::VEHICLE_EYE)),
                direction: vector(a, f::VEHICLE_DIRECTION),
                preview: flag(a, f::VEHICLE_HAS_PREVIEW).then(|| vector(a, f::VEHICLE_PREVIEW)),
                velocity: vector(a, f::VEHICLE_VELOCITY),
                center: vector(a, f::VEHICLE_CENTER),
                size: vector(a, f::VEHICLE_SIZE),
                bounds,
                yaw: a[f::VEHICLE_YAW],
                tilt: a[f::VEHICLE_TILT],
                roll: a[f::VEHICLE_ROLL],
                spin: a[f::VEHICLE_SPIN],
                max_speed: a[f::VEHICLE_MAX_SPEED],
                human: flag(a, f::VEHICLE_HUMAN),
                look_yaw: flag(a, f::VEHICLE_HAS_LOOK_YAW).then_some(a[f::VEHICLE_LOOK_YAW]),
                look_pitch: a[f::VEHICLE_LOOK_PITCH],
                boosting: flag(a, f::VEHICLE_BOOSTING),
                age: a[f::VEHICLE_AGE],
                health: a[f::VEHICLE_HEALTH],
                blend_duration: a[f::VEHICLE_BLEND_DURATION],
                bike: flag(a, f::VEHICLE_BIKE),
                view: seat(
                    a,
                    f::SEAT_EYE,
                    f::SEAT_CENTER,
                    f::SEAT_OFFSET,
                    f::SEAT_TILT,
                    f::SEAT_FIRST,
                    f::SEAT_THIRD,
                    f::SEAT_RATES,
                    f::SEAT_AIM,
                ),
            };
            // Vehicle.seatFrame formerly composed this presented subject.
            // Its pilot frame is just position and heading for a ground hull.
            // Flyer pilots instead use the full displayed/controller turn.
            // A turret seat adds its section pivot and relative heading.
            // Wrapping the heading with atan2 preserves the old angle helper.
            // Canonical F2 seats bypass reconstruction, preserving their tick.
            // A passenger's frame is not the vehicle's complete hull lean.
            // That complete hull is supplied separately for rigid-seat eyes.
            if !flag(a, f::CANONICAL_SEAT) {
                let base_q = if input.flying {
                    if flag(a, f::AUTHORITY) {
                        input.flight_frame
                    } else {
                        input.drawn
                    }
                } else {
                    axis::<T>(UP, input.yaw)
                };
                input.frame.compose(
                    &vector(a, f::SEAT_POSITION),
                    &base_q,
                    &Vector3::new(1.0, 1.0, 1.0),
                );
                if input.seat != 0 {
                    let yaw = a[f::SEAT_YAW] - input.yaw;
                    let yaw = T::atan2(T::sin(yaw), T::cos(yaw));
                    let pitch = a[f::SEAT_PITCH];
                    let forward = Vector3::new(
                        T::sin(yaw) * T::cos(pitch),
                        T::sin(pitch),
                        T::cos(yaw) * T::cos(pitch),
                    );
                    let q = look_basis(forward, vector(a, f::SEAT_UP));
                    let mut local = Matrix4::default();
                    local.compose(&vector(a, f::SEAT_PIVOT), &q, &Vector3::new(1.0, 1.0, 1.0));
                    input.frame.multiply(&local);
                }
            }
            // Vehicle.seatForward already belonged to the native seat owner.
            // When that owner is absent in a synthetic scene, raw angles are
            // sufficient to construct the identical forward vector here.
            // A flyer pilot's forward is its controller quaternion's local z.
            // Display preview has its own yaw/pitch and overrides only aim.
            // It must not replace the hull or seat frame used to place the eye.
            if !flag(a, f::CANONICAL_DIRECTION) {
                let yaw = a[f::SEAT_YAW];
                let pitch = a[f::SEAT_PITCH];
                input.direction = if input.flying && input.seat == 0 {
                    rotate(Vector3::new(0.0, 0.0, 1.0), input.flight_frame)
                } else {
                    Vector3::new(
                        T::sin(yaw) * T::cos(pitch),
                        T::sin(pitch),
                        T::cos(yaw) * T::cos(pitch),
                    )
                };
            }
            if input.preview.is_some() {
                let yaw = a[f::VEHICLE_PREVIEW_YAW];
                let pitch = a[f::VEHICLE_PREVIEW_PITCH];
                input.preview = Some(Vector3::new(
                    T::sin(yaw) * T::cos(pitch),
                    T::sin(pitch),
                    T::cos(yaw) * T::cos(pitch),
                ));
            }
            if flag(a, f::AUTHORITY) {
                let q = if input.flying {
                    input.flight_frame
                } else {
                    euler::<T>(-input.tilt, input.yaw, input.roll, false)
                };
                input
                    .hull
                    .compose(&input.position, &q, &Vector3::new(1.0, 1.0, 1.0));
                if input.flying {
                    input.drawn = mul(
                        input.flight_frame,
                        axis::<T>(Vector3::new(0.0, 0.0, 1.0), a[f::VEHICLE_BANK]),
                    );
                }
            }
            self.vehicle
                .frame::<T, C>(&input, &mut cam, active_dt, world);
        } else if flag(a, f::TURRET_ACTIVE) {
            mode = modes::TURRET as f64;
            if self.last_turret != a[f::TURRET_ID] {
                self.turret = vehicle::Turret::new();
                self.last_turret = a[f::TURRET_ID];
            }
            // TurretCamera.update uses the placed building's up, not the
            // reconstructed aim frame's pitched up column for its view turn.
            // Using the latter applies pitch twice on sloped emplacements.
            // The frame itself is needed to place TrackCenter and EyePointOffset.
            // Canonical seat eyes and directions retain their authority tick.
            // Preview-only angles retain the page's own input clock.
            // The enclosure test uses the building's model-local bounds.
            // This result controls retained drum/gun visibility on the page.
            // It is published only when the turret is the selected owner.
            let root = matrix(a, f::TURRET_ROOT);
            let pivot = transformed(vector(a, f::TURRET_PIVOT), &root);
            let mut up = vector(a, f::TURRET_UP);
            up.transform_direction(&root);
            let direction = if flag(a, f::TURRET_CANONICAL_DIRECTION) {
                vector(a, f::TURRET_DIRECTION)
            } else {
                let yaw = a[f::TURRET_FACING] + a[f::TURRET_YAW];
                let pitch = a[f::TURRET_PITCH];
                Vector3::new(
                    T::sin(yaw) * T::cos(pitch),
                    T::sin(pitch),
                    T::cos(yaw) * T::cos(pitch),
                )
            };
            let mut frame = Matrix4::default();
            frame.compose(
                &pivot,
                &look_basis(direction, up),
                &Vector3::new(1.0, 1.0, 1.0),
            );
            let view = seat(
                a,
                f::TURRET_EYE,
                f::TURRET_CENTER,
                f::TURRET_OFFSET,
                f::TURRET_TILT,
                f::TURRET_FIRST,
                f::TURRET_THIRD,
                f::TURRET_RATES,
                f::TURRET_AIM,
            );
            let eye = if flag(a, f::TURRET_CANONICAL_EYE) {
                vector(a, f::TURRET_EYE)
            } else {
                transformed(view.eye, &frame)
            };
            let mut inverse = root;
            inverse.invert();
            let local = transformed(eye, &inverse);
            let bounds = &a[f::TURRET_BOUNDS..f::TURRET_BOUNDS + 6];
            self.output[o::TURRET_ENCLOSED] = (flag(a, f::TURRET_HAS_BOUNDS)
                && local.x > bounds[0] + 0.15
                && local.x < bounds[3] - 0.15
                && local.y > bounds[1] + 0.15
                && local.y < bounds[4] - 0.15
                && local.z > bounds[2] + 0.15
                && local.z < bounds[5] - 0.15) as u8
                as f64;
            self.turret.frame::<T, C>(
                &mut cam,
                dt,
                view,
                frame,
                up,
                direction,
                eye,
                flag(a, f::FIRST_PERSON),
                1.0,
                world,
            );
        } else {
            mode = modes::SOLDIER as f64;
            self.soldier.frame::<T, C>(
                dt,
                &soldier::Input {
                    feet: vector(a, f::FEET),
                    yaw: a[f::YAW],
                    pitch: a[f::PITCH],
                    posture: (a[f::POSTURE] as usize).min(2),
                    zoom: a[f::ZOOM],
                    first: flag(a, f::OWN_EYES),
                    responsive: flag(a, f::RESPONSIVE),
                    // PlayerCombat.viewEye formerly built this first-person eye.
                    // Displayed body position can differ from smoothed feet.
                    // Authority can supply its already-sampled combat basis.
                    // A droideka has a class eye height and a separate roll height.
                    // Explicit eyes are retained for foreign/preview inputs.
                    // No eye expression runs for an ordinary chase camera.
                    eye: flag(a, f::HAS_EYE).then(|| match a[f::EYE_KIND] as u32 {
                        eyes::INFANTRY | eyes::BASIS => soldier::combat_eye::<T>(
                            vector(a, f::FEET),
                            vector(a, f::BODY_POSITION),
                            a[f::YAW],
                            a[f::PITCH],
                            a[f::BODY_POSTURE] as usize,
                            flag(a, f::AIR),
                            a[f::ZOOM],
                            (a[f::EYE_KIND] == eyes::BASIS as f64).then(|| {
                                [
                                    vector(a, f::EYE_PIVOT),
                                    vector(a, f::EYE_ORIGIN),
                                    vector(a, f::EYE_LEFT),
                                    vector(a, f::EYE_DIRECTION),
                                ]
                            }),
                        ),
                        eyes::DROIDEKA => {
                            let feet = vector(a, f::FEET);
                            Vector3::new(
                                feet.x,
                                feet.y
                                    + if flag(a, f::EYE_ROLLED) {
                                        1.0
                                    } else {
                                        a[f::EYE_HEIGHT]
                                    },
                                feet.z,
                            )
                        }
                        _ => vector(a, f::EYE),
                    }),
                },
                def,
                &self.motion,
                &mut cam,
                world,
            );
        }
        if !dead && !flag(a, f::PROJECT_ONLY) {
            self.death.watch::<T>(&cam, active_dt)
        }
        // Publish one bounded output record; consumers may read it in stages.
        // All matrices use the same f64 compose/invert order as Three cameras.
        // Pre-shake inverse is independent of the final render inverse.
        // Copying either matrix on the host cannot advance any visual clock.
        // Diagnostic fields are scalars, so saturation needs no event objects.
        // Continuation values support authority replay and network capture.
        // They are not alternate host implementations of the camera state.
        let out = &mut self.output;
        out[o::MODE] = mode;
        if let Some(state) = authoritative_death {
            out[o::WORLD_DEATH_TIME] = state.time;
            put_v(out, o::WORLD_DEATH_POSITION, state.position);
            put_q(out, o::WORLD_DEATH_QUATERNION, state.quaternion);
            put_v(out, o::WORLD_PREVIOUS_POSITION, state.previous_position);
            put_q(out, o::WORLD_PREVIOUS_QUATERNION, state.previous_quaternion);
            put_v(out, o::WORLD_DEATH_BODY, state.body);
            put_v(out, o::WORLD_DEATH_ANCHOR, state.anchor);
            out[o::WORLD_DEATH_HFOV] = state.hfov;
        }
        // game.js exposed this pose to first-person arms before camera shake.
        // Its Euler mirror is produced beside the quaternion, not in Three.
        // Compose precedes inverse, with no parent transform on these cameras.
        // The host applies these fields with callbacks disabled by raw writes.
        // The final render pose below may still differ through shake or spawn.
        // Both publications use the same fixed record, with no extra export.
        put_v(out, o::PRE_POSITION, cam.p);
        put_q(out, o::PRE_QUATERNION, cam.q);
        put_v(out, o::PRE_EULER, angles::<T>(cam.q));
        let mut m = Matrix4::default();
        m.compose(&cam.p, &cam.q, &Vector3::new(1.0, 1.0, 1.0));
        out[o::PRE_MATRIX..o::PRE_MATRIX + 16].copy_from_slice(&m.elements);
        m.invert();
        out[o::PRE_INVERSE..o::PRE_INVERSE + 16].copy_from_slice(&m.elements);
        // CameraShake.apply occurred after arms and the trace consumed camera.
        // Its output is computed now but exposed by the host only at finish.
        // Death keeps the selected pose while shakes decay in their birth order.
        // Live turns draw three values from the independent cosmetic source.
        // A frozen frame draws none and cannot alter the previous random seed.
        // A spawn pose follows shake, as in the old game.js final override.
        self.shake.frame::<T, C>(&mut cam, active_dt, dead, world);
        // The spawn screen overrides only the rendered and heard pose. Arms
        // and the death handoff keep the underlying pre-shake player view.
        if flag(a, f::HAS_SHOT) {
            if flag(a, f::DIRECTOR) {
                if let Some((p, q)) = self.director.pose::<T>(
                    a[f::DIRECTOR_AGE],
                    a[f::DIRECTOR_REVISION] as u32,
                    &self.content.spawn_shots,
                ) {
                    cam.p = p;
                    cam.q = q;
                }
            } else {
                cam.p = vector(a, f::SHOT_POSITION);
                cam.q = quaternion(a, f::SHOT_QUATERNION);
            }
        }
        put_v(out, o::POSITION, cam.p);
        put_q(out, o::QUATERNION, cam.q);
        put_v(out, o::EULER, angles::<T>(cam.q));
        put_v(out, o::UP, cam.up);
        out[o::FOV] = cam.fov;
        out[o::NEAR] = cam.near;
        out[o::FAR] = cam.far;
        m.compose(&cam.p, &cam.q, &Vector3::new(1.0, 1.0, 1.0));
        out[o::MATRIX..o::MATRIX + 16].copy_from_slice(&m.elements);
        m.invert();
        out[o::INVERSE..o::INVERSE + 16].copy_from_slice(&m.elements);
        // PerspectiveCamera.updateProjectionMatrix used these same dimensions.
        // The selected horizontal lens has already become vertical degrees.
        // Half-angle tangent is evaluated before the viewport-width multiply.
        // Keep left/right and top/bottom construction, including zero signs.
        // Native makePerspective uses the WebGL depth convention of the page.
        // Projection inverse follows that exact matrix, without host inversion.
        // Temporal jitter and renderer-specific derived views remain consumers.
        let top = cam.near * T::tan(DEG * 0.5 * cam.fov);
        let height = 2.0 * top;
        let width = cam.aspect * height;
        let left = -0.5 * width;
        m.make_perspective(
            left,
            left + width,
            top,
            top - height,
            cam.near,
            cam.far,
            three::matrix::CoordinateSystem::WebGL,
        );
        out[o::PROJECTION..o::PROJECTION + 16].copy_from_slice(&m.elements);
        m.invert();
        out[o::PROJECTION_INVERSE..o::PROJECTION_INVERSE + 16].copy_from_slice(&m.elements);
        out[o::CUE] = self.motion.cue;
        out[o::REMOTE_FOV] = self.remote.fov;
        out[o::REMOTE_REACH] = self.remote.reach;
        out[o::REMOTE_FRESH] = self.remote.fresh as u8 as f64;
        out[o::SOLDIER_FOV] = self.soldier.fov;
        out[o::SOLDIER_REACH] = self.soldier.reach;
        out[o::SOLDIER_FRESH] = self.soldier.fresh as u8 as f64;
        out[o::VEHICLE_SET] = self.vehicle.set as u8 as f64;
        out[o::SPEED_EFFECT] = self.vehicle.speed_effect;
        out[o::ENCLOSED] = self.vehicle.enclosed as u8 as f64;
        put_v(out, o::LOOK_AT, self.vehicle.look_at);
        out[o::DEATH_ACTIVE] = self.death.active as u8 as f64;
        out[o::DEATH_TIME] = self.death.time;
        out[o::DEATH_KNOWN] = self.death.known as u8 as f64;
        let trace = &self.death.trace;
        out[o::TRACE_VISIBLE] = trace.visible as u8 as f64;
        put_v(out, o::TRACE_FROM, trace.from);
        put_v(out, o::TRACE_TO, trace.to);
        out[o::TRACE_FRIENDLY] = trace.friendly as u8 as f64;
        out[o::TRACE_HEAD] = trace.head;
        out[o::TRACE_FADE] = trace.fade;
        out[o::TRACE_SINCE] = trace.since;
        out[o::SHAKE_TOTAL] = self.shake.total;
        out[o::SHAKE_ACTIVE] = self.shake.active() as u8 as f64;
        out[o::REFUSED] = self.shake.refused as f64;
        out[o::SKIPPED] = (self.skipped + self.motion.skipped) as f64;
    }
}
