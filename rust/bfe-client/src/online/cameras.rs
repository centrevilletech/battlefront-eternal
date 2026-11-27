//! Direct joins use the same camera selector and seat camera owner as the
//! page. The host contributes published hull matrices and collision queries;
//! obstruction, eye/chase selection, easing and lenses remain in Phase.
use super::{hulls::Hulls, receiver::Receiver, replay::Collision};
use bfe_presentation::{
    cameras::{math::V8, Cast, Content, Phase},
    layout::{camera_eye as eyes, camera_frame as f, camera_output as o, VERSION},
    network_layout::{net_kind as k, net_record as r},
};
use three::{Matrix4, Quaternion, Vector3};
pub struct Cameras {
    owner: Phase,
    gunner: bfe_presentation::camera_effects::Gunner,
    collision: Collision,
    world: u32,
    ignored: i32,
    life: Option<(u32, u32, usize)>,
    remote_life: Option<(u16, u32, usize)>,
    body: Option<(f64, f64, usize)>,
    definitions: Vec<usize>,
    remote_defs: Vec<Option<(usize, [f64; 3], f64)>>,
    remote_aim: Option<[f64; 3]>,
    first_fovs: Vec<f64>,
    eye_heights: Vec<f64>,
    zoom_eyes: Vec<bool>,
    weapon_channels: Vec<usize>,
    pub eye_view: bool,
    pub scoped_eye: bool,
    was_dead: bool,
    death_serial: u32,
    death_hfov: f64,
    /// The deathCamera setting: Classic draws the original's rules, else Enhanced.
    pub death_classic: bool,
    killer: Option<(u32, u32, i32)>,
    killer_tick: f64,
}
/// What DeathShot.frame reads from the bodies this client draws.
#[derive(Clone, Copy, Default)]
pub struct DrawnDeath {
    /// The dead player's ribcage bone.
    pub chest: Option<[f64; 3]>,
    /// The killer's actor root, while he is drawn.
    pub killer_feet: Option<[f64; 3]>,
    /// A creature holds the player; its flag selects the death view immediately.
    pub held_camera: Option<bool>,
}
struct Queries {
    collision: Collision,
    world: u32,
    ignored: i32,
}
impl Cast for Queries {
    fn cast(&mut self, origin: Vector3, direction: Vector3, far: f64, _: bool) -> Option<f64> {
        let ray = bfe_sim::collision::Ray { ox: origin.x, oy: origin.y, oz: origin.z, dx: direction.x, dy: direction.y, dz: direction.z };
        self.collision.borrow_mut().ray_hit(self.world, ray, far, self.ignored).map(|(_, h)| h.distance)
    }
}
fn view_rotation(view: &crate::scene::View) -> Quaternion {
    let mut world = Matrix4::default();
    world.elements.copy_from_slice(&view.values[16..32]);
    world.invert();
    let mut rotation = Quaternion::default();
    rotation.set_from_rotation_matrix(&world);
    // Matrix round trips introduce small length errors. Feeding them back
    // through chase slerp amplifies them into a skewed, turning camera.
    rotation.normalize();
    rotation
}
fn zoom_first_person_released(weapon: &bfe_content::Value) -> bool {
    weapon.get("ZoomFirstPerson").is_some_and(|value| value.truthy())
}

fn zoom_first_person_swbf2(weapon: &bfe_content::Value) -> bool {
    match weapon.get("ZoomFirstPerson") {
        Some(bfe_content::Value::Bool(b)) => *b,
        other => bfe_content::odf::num(other, 0.) != 0.,
    }
}

fn zoom_first_person_for_family(family: bfe_family::GameFamily, weapon: &bfe_content::Value) -> bool {
    match family {
        bfe_family::GameFamily::Swbf1 => zoom_first_person_released(weapon),
        bfe_family::GameFamily::Swbf2 => zoom_first_person_swbf2(weapon),
    }
}

impl Cameras {
    pub fn new(content: &bfe_content_native::BattleContent, collision: Collision, world: u32) -> Result<Self, String> {
        let defs = content.defs()?;
        let mut cameras = Content::default();
        // constructs SoldierCamera with common.cameraMotions. These
        // curves belong to visual motion, never authority or prediction replay.
        for name in ["RollLeft", "RollRight", "StandToProne", "CrouchToProne", "ProneToStand", "CrouchToStand", "Land"] {
            let keys = content.content.battle.common.get("cameraMotions").and_then(|tables| tables.get(name));
            cameras.motions.push(match keys {
                Some(keys) => serde_json::from_value(bfe_content_native::loader::authored_json(keys))
                    .map_err(|error| format!("camera motion {name}: {error}"))?,
                None => Default::default(),
            });
        }
        let mut definitions = vec![usize::MAX; defs.registry.classes.len() + 1];
        let mut first_fovs = Vec::new();
        let mut eye_heights = Vec::new();
        for class in &defs.classes {
            if let Some(id) = class.registry_id {
                definitions[id as usize] = cameras.definitions.len();
                let def = bfe_sim::units::class::camera_def(&class.record);
                first_fovs.push(def.first);
                // Droideka.eyeHeight: the standing camera section's EyePointOffset.
                let eye_h = class
                    .record
                    .get("cameras")
                    .and_then(|cameras| cameras.get("STAND"))
                    .and_then(|stand| stand.get("EyePointOffset"))
                    .and_then(bfe_content::Value::as_array)
                    .and_then(|a| a.get(1))
                    .map_or(1.8, |v| bfe_content::js::to_number(Some(v)));
                eye_heights.push(eye_h);
                cameras.definitions.push(def);
            }
        }
        let mut remote_defs = vec![None; defs.registry.combat.len() + 1];
        for (i, (_, def)) in defs.registry.combat.iter().enumerate() {
            if def.get("base").and_then(bfe_content::Value::as_str) != Some("droid") {
                continue;
            }
            let (cam_def, eye, zoom_min) = remote_definition(def);
            let idx = cameras.definitions.len();
            cameras.definitions.push(cam_def);
            remote_defs[i + 1] = Some((idx, eye, zoom_min));
        }
        let family = content.content.battle.family;
        let zoom_eyes = defs
            .weapons
            .iter()
            .map(|weapon| zoom_first_person_for_family(family, &weapon.record))
            .collect();
        let weapon_channels = defs
            .weapons
            .iter()
            .map(|weapon| bfe_content::odf::num(weapon.record.get("channel"), 0.).min(1.) as usize)
            .collect();
        Ok(Self {
            owner: Phase::new(cameras),
            gunner: Default::default(),
            collision,
            world,
            ignored: -1,
            life: None,
            remote_life: None,
            body: None,
            definitions,
            remote_defs,
            remote_aim: None,
            first_fovs,
            eye_heights,
            zoom_eyes,
            weapon_channels,
            eye_view: false,
            scoped_eye: false,
            was_dead: false,
            death_serial: 0,
            death_hfov: 0.837758,
            death_classic: false,
            killer: None,
            killer_tick: f64::NEG_INFINITY,
        })
    }
    /// The unit the death camera turns to, for the caller to find where it is drawn.
    pub fn killer(&self) -> Option<(u32, u32)> {
        self.killer.map(|(id, generation, _)| (id, generation))
    }
    pub fn frame(
        &mut self,
        receiver: &Receiver,
        prediction: &super::replay::Replay,
        hulls: &mut Hulls,
        turrets: &mut super::turrets::Turrets,
        camera: &mut crate::scene::MapCamera,
        rider_eye: Option<[f64; 3]>,
        drawn: DrawnDeath,
        dt: f64,
    ) -> f64 {
        let lease = receiver.own.mount;
        if drawn.held_camera.is_some() {
            camera.first_person = false;
        }
        camera.set_flight_mode(receiver.uses_flight_controls());
        // PlayerCombat.usesEyeView also selects eyes for an authored scoped
        // weapon; zooming must not change the player's saved perspective.
        let scoped_eye = receiver.own.zoom > 1.
            && prediction
                .weapons()
                .filter(|weapon| {
                    self.weapon_channels
                        .get(weapon.def.0.saturating_sub(1) as usize)
                        .copied()
                        .unwrap_or(0)
                        == 0
                })
                .nth(prediction.selected()[0] as usize)
                .and_then(|weapon| self.zoom_eyes.get(weapon.def.0.saturating_sub(1) as usize))
                .copied()
                .unwrap_or(false);
        self.scoped_eye = scoped_eye;
        // The droideka's clip as it is drawn (the newest correction's without a drawn sample).
        let walker = receiver.own.walker.then(|| prediction.walker.map_or((receiver.own.walker_mode, receiver.own.walker_time), |w| (w.mode, w.pose[3])));
        // As in the 2004 game, a droideka hands its own eyes to the chase camera while it folds, rolls and
        // unfolds, and takes them back once it stands; the player's choice of view is kept for then.
        let rolled = walker.is_some_and(|(mode, _)| mode >= 11);
        self.eye_view = receiver.own.alive && receiver.own.remote.is_none() && (camera.first_person || scoped_eye) && !rolled;
        hulls.hidden = None;
        turrets.hidden = None;
        let body = (receiver.own.id, receiver.own.generation, receiver.own.class);
        if self.body != Some(body) {
            self.killer = None;
            self.killer_tick = f64::NEG_INFINITY;
        }
        // noteAttacker tracks the last attacker from damage (schema 3)
        // and death (schema 4) feedback so the death camera can frame the killer.
        for field in receiver.publications.iter() {
            if field.row[r::KIND] == k::COMBAT as f64
                && (field.row[r::SCHEMA] == 3. || field.row[r::SCHEMA] == 4.)
                && field.length >= 52
            {
                let b = &field.bytes[..field.length];
                let tid = u32::from_le_bytes(b[4..8].try_into().unwrap());
                let tgen = u32::from_le_bytes(b[8..12].try_into().unwrap());
                let aid = u32::from_le_bytes(b[12..16].try_into().unwrap());
                let agen = u32::from_le_bytes(b[16..20].try_into().unwrap());
                let ateam = i32::from_le_bytes(b[48..52].try_into().unwrap());
                let fatal = field.row[r::SCHEMA] == 4.;
                let tick = field.row[r::TICK];
                if tid as f64 == receiver.own.id && tgen as f64 == receiver.own.generation {
                    if fatal || tick >= self.killer_tick {
                        self.killer_tick = self.killer_tick.max(tick);
                        self.killer = (aid != 0).then_some((aid, agen, ateam));
                    }
                }
            }
        }
        let view = camera.snapshot();
        let q = view_rotation(view);
        let prev_fov = if self.owner.output[o::FOV] > 0. {
            self.owner.output[o::FOV]
        } else {
            60.
        };
        let x = &mut self.owner.input;
        x.fill(0.);
        x[f::VERSION] = VERSION as f64;
        x[f::DT] = dt;
        x[f::FIRST_PERSON] = (camera.first_person && !rolled) as u8 as f64;
        x[f::OWN_EYES] = (self.eye_view && lease.is_none()) as u8 as f64;
        x[f::ZOOM] = receiver.own.zoom.max(1.);
        x[f::CAMERA_POSITION..f::CAMERA_POSITION + 3].copy_from_slice(&camera.position);
        x[f::CAMERA_QUATERNION..f::CAMERA_QUATERNION + 4].copy_from_slice(&[q.x, q.y, q.z, q.w]);
        x[f::CAMERA_UP + 1] = 1.;
        x[f::CAMERA_FOV] = prev_fov;
        x[f::CAMERA_ASPECT] = camera.aspect();
        x[f::CAMERA_NEAR] = 0.3;
        x[f::CAMERA_FAR] = 20000.;
        let def_idx = self.definitions.get(receiver.own.class).copied().unwrap_or(usize::MAX);
        x[f::SOLDIER_DEF] = def_idx as f64;
        x[f::SOLDIER_RESET] = (self.body != Some(body) || self.life != lease) as u8 as f64;
        x[f::RESPONSIVE] = 1.;
        x[f::HAS_EYE] = 1.;
        // The displayed view can still be a mounted chase/cockpit pose on
        // exit. Infantry eyes come from the current body, not that old view.
        x[f::EYE..f::EYE + 3].copy_from_slice(&camera.subject_eye());
        x[f::FEET..f::FEET + 3].copy_from_slice(&receiver.own.position);
        x[f::YAW] = camera.aim()[0];
        x[f::PITCH] = camera.aim()[1];
        x[f::POSTURE] = receiver.own.posture as f64;
        x[f::BODY_POSTURE] = receiver.own.posture as f64;
        x[f::AIR] = receiver.own.air as f64;
        x[f::DIVE] = receiver.own.dive;
        x[f::VELOCITY..f::VELOCITY + 3].copy_from_slice(&receiver.own.velocity);
        if let Some(body) = &prediction.displayed {
            x[f::FEET..f::FEET + 3].copy_from_slice(&body.position);
            x[f::YAW] = body.aim_yaw;
            x[f::PITCH] = body.aim_pitch;
            x[f::BODY_POSTURE] = body.posture as u8 as f64;
            x[f::AIR] = body.air.map_or(0., |air| air as u8 as f64 + 1.);
            x[f::DIVE] = body.dive;
            x[f::POSTURE] = if body.air.is_some() { 0. } else { body.posture as u8 as f64 };
            x[f::VELOCITY..f::VELOCITY + 3].copy_from_slice(&body.velocity);
        }
        if let Some(walker) = &prediction.walker {
            x[f::FEET..f::FEET + 3].copy_from_slice(&walker.position);
            x[f::VELOCITY..f::VELOCITY + 3].copy_from_slice(&walker.velocity);
        }
        x.copy_within(f::FEET..f::FEET + 3, f::BODY_POSITION);
        if let Some((mode, time)) = walker {
            x[f::EYE_KIND] = eyes::DROIDEKA as f64;
            // Its camera takes the standing section, eased into the rolled one (CROUCH) by this share.
            x[f::POSTURE] = 0.;
            x[f::EYE_ROLLED] = rolled_share(mode, time);
            x[f::EYE_HEIGHT] = self.eye_heights.get(def_idx).copied().unwrap_or(1.8);
        } else {
            x[f::EYE_KIND] = eyes::INFANTRY as f64;
        }
        // startDeathCamera: while the dead soldier's corpse remains in
        // the world (4 s linger, non-walker), DeathShot frames the body and killer.
        let dead_active = (receiver.own.dead || drawn.held_camera == Some(true))
            && !receiver.own.removed && !receiver.own.walker;
        if dead_active && !self.was_dead {
            self.death_serial = self.death_serial.wrapping_add(1);
            // startDeathCamera measures the world's camera (simulationCamera),
            // not the drawn one. Only the drawn camera takes the class lens, so
            // the world's is still at its 60 degree default when he dies and
            // the shot settles wider than the chase view. Kept it.
            let across = 2. * ((60_f64.to_radians() * 0.5).tan() * camera.aspect().max(1e-6)).atan();
            let first = self.first_fovs.get(def_idx).copied().unwrap_or(70_f64.to_radians());
            let base = if receiver.own.zoom > 1. { first } else { across };
            // deadFov: one world step of 1/60 s toward DEAD_FOV at FOV_RATE.
            self.death_hfov = 0.837758 + (base - 0.837758) * (-25_f64 / 60.).exp();
        }
        self.was_dead = dead_active;
        if dead_active {
            x[f::DEATH_ACTIVE] = 1.;
            x[f::DEATH_SERIAL] = self.death_serial as f64;
            x[f::DEATH_HFOV] = self.death_hfov;
            x[f::DEATH_CLASSIC] = self.death_classic as u8 as f64;
            if let Some(chest) = drawn.chest {
                x[f::HAS_CHEST] = 1.;
                x[f::CHEST..f::CHEST + 3].copy_from_slice(&chest);
            }
            if let Some((kid, kgen, kteam)) = self.killer {
                if let Some(row) = receiver.active().find(|row| {
                    row[r::ID] == kid as f64 && row[r::GENERATION] == kgen as f64
                }) {
                    // drawnKiller, for a killer on foot: his
                    // eyes standing, crouched or prone, a walker's, or the
                    // middle of a corpse, above the feet as drawn. A killer in
                    // a seat is framed at his own row, not at the seat's eye.
                    let living = row[r::DEAD] == 0. && row[r::HEALTH] > 0.;
                    let walker = row[r::KIND] == k::DROIDEKA as f64;
                    let eyes = if !living {
                        0.5
                    } else if walker {
                        1.
                    } else {
                        match row[r::POSTURE] as u8 {
                            2 => 0.6,
                            1 => 1.2,
                            _ => 1.7,
                        }
                    };
                    let feet = drawn
                        .killer_feet
                        .unwrap_or([row[r::POSITION], row[r::POSITION + 1], row[r::POSITION + 2]]);
                    x[f::KILLER_PRESENT] = 1.;
                    x[f::KILLER_POINT..f::KILLER_POINT + 3].copy_from_slice(&[feet[0], feet[1] + eyes, feet[2]]);
                    x[f::KILLER_SIZE] = if walker { 1.6 } else { 1.9 };
                    x[f::KILLER_LIVING] = living as u8 as f64;
                    x[f::KILLER_FRIENDLY] = (kteam == receiver.own.team as i32) as u8 as f64;
                }
            }
        }
        self.remote_aim = None;
        if let Some((_, _, rdef)) = receiver.own.remote {
            let (cam_def, eye_offset, zoom_min) = self
                .remote_defs
                .get(rdef)
                .copied()
                .flatten()
                .unwrap_or((usize::MAX, [0., 0.5, 0.], 1.));
            let zoom = if receiver.own.remote_zoom_step != 0 { zoom_min } else { 1. };
            let own_eye = camera.first_person || zoom > 1.;
            let ryaw = camera.aim()[0];
            let rpitch = camera.aim()[1];
            let pos = receiver.own.remote_position;
            let eye = [pos[0] + eye_offset[0], pos[1] + eye_offset[1], pos[2] + eye_offset[2]];
            let dir = [
                bfe_pose::rig::math::sin(ryaw) * bfe_pose::rig::math::cos(rpitch),
                bfe_pose::rig::math::sin(rpitch),
                bfe_pose::rig::math::cos(ryaw) * bfe_pose::rig::math::cos(rpitch),
            ];
            x[f::REMOTE_ACTIVE] = 1.;
            x[f::REMOTE_DEF] = cam_def as f64;
            x[f::REMOTE_RESET] = (self.remote_life != receiver.own.remote) as u8 as f64;
            x[f::REMOTE_FEET..f::REMOTE_FEET + 3].copy_from_slice(&pos);
            x[f::REMOTE_YAW] = ryaw;
            x[f::REMOTE_PITCH] = rpitch;
            x[f::REMOTE_ZOOM] = zoom;
            x[f::REMOTE_FIRST] = own_eye as u8 as f64;
            if own_eye {
                x[f::REMOTE_HAS_EYE] = 1.;
                x[f::REMOTE_EYE..f::REMOTE_EYE + 3].copy_from_slice(&eye);
                x[f::REMOTE_AIM..f::REMOTE_AIM + 3].copy_from_slice(&dir);
                x[f::REMOTE_BACK] = if zoom > 1. { 0.5 } else { 0.3 };
            }
            self.remote_aim = Some(std::array::from_fn(|k| eye[k] + 1024. * dir[k]));
        }
        self.remote_life = receiver.own.remote;
        x[f::VEHICLE_RESET] = (self.life != lease) as u8 as f64;
        // A turret id can also name a live vehicle. The correction's mount
        // kind selects the camera owner before either resource bank is queried.
        let hull = lease
            .filter(|_| receiver.own.mount_kind == bfe_presentation::network_layout::net_kind::VEHICLE)
            .and_then(|lease| hulls.camera(lease.0 as f64, lease.1 as f64, lease.2, x));
        let turret = lease
            .filter(|_| receiver.own.mount_kind == bfe_presentation::network_layout::net_kind::TURRET)
            .and_then(|lease| turrets.camera(lease.0 as f64, lease.1 as f64, x));
        if let Some(eye) = rider_eye.filter(|_| hull.is_some()) {
            x[f::VEHICLE_HAS_EYE] = 1.;
            x[f::VEHICLE_EYE..f::VEHICLE_EYE + 3].copy_from_slice(&eye);
        }
        self.body = Some(body);
        self.life = lease;
        let ignored = prediction.mounted_body(receiver).map_or(-1, |body| body as i32);
        if ignored != self.ignored && std::env::var_os("BFE_HUD_TRACE").is_some() {
            eprintln!("camera subject: mount={lease:?} kind={} ignored={ignored}", receiver.own.mount_kind);
        }
        self.ignored = ignored;
        self.owner.set_near_clearance(receiver.control_family == bfe_family::GameFamily::Swbf2);
        let mut queries = Queries { collision: self.collision.clone(), world: self.world, ignored };
        self.owner.frame::<V8, _>(&mut queries);
        if self.owner.output[o::STATUS] == 0. {
            camera.accept_camera(&self.owner.output);
            if camera.first_person && self.owner.output[o::ENCLOSED] != 0. {
                hulls.hidden = hull;
            }
            if camera.first_person && self.owner.output[o::TURRET_ENCLOSED] != 0. {
                turrets.hidden = turret;
            }
        }
        if self.owner.output[o::STATUS] == 0. { self.owner.output[o::CUE] } else { 0. }
    }
    /// The 2005 chase solver stops the near plane at an obstruction and keeps
    /// the eye at least the near distance from TrackCenter. Hide the local
    /// third-person silhouette only when the soldier's torso target point is
    /// nearer the eye than the near plane (`depth < near`), the 2005 game's
    /// cutoff for drawing the soldier.
    pub fn hides_infantry(
        &self, receiver: &Receiver, camera: &crate::scene::MapCamera, scale: f64,
    ) -> bool {
        if self.eye_view {
            return true;
        }
        if !receiver.own.alive || receiver.own.mount.is_some() || receiver.own.remote.is_some() {
            return false;
        }
        let input = &self.owner.input;
        let feet = Vector3::new(input[f::FEET], input[f::FEET + 1], input[f::FEET + 2]);
        let snapshot = camera.snapshot();
        obstructs_body(
            feet, input[f::POSTURE] as u8, scale,
            camera.position, &snapshot.values[..16], &snapshot.values[16..32],
        )
    }

    // PlayerCombat.updateHud projects a mounted direction from the displayed
    // camera. Ground seats project both axes; a flying pilot keeps x centered.
    pub fn mounted_aim(&mut self, camera: &crate::scene::MapCamera) -> Option<super::hud::Aim> {
        use bfe_presentation::layout::camera_mode as mode;
        use bfe_presentation::visual_effects_layout::gunner_frame as g;
        let a = &self.owner.input;
        let kind = self.owner.output[o::MODE] as u32;
        if kind == mode::REMOTE {
            return self.remote_aim.map(|point| super::hud::Aim { point, project_x: false });
        }
        if kind == mode::VEHICLE {
            let pilot = a[f::VEHICLE_FLYING] != 0. && a[f::VEHICLE_SEAT] == 0.;
            let direction = if pilot {
                let q = &a[f::VEHICLE_FLIGHT_FRAME..f::VEHICLE_FLIGHT_FRAME + 4];
                let mut direction = Vector3::new(0., 0., 1.);
                direction.apply_quaternion(&Quaternion::new(q[0], q[1], q[2], q[3]));
                direction
            } else {
                let (yaw, pitch) = (a[f::SEAT_YAW], a[f::SEAT_PITCH]);
                Vector3::new(
                    bfe_pose::rig::math::sin(yaw) * bfe_pose::rig::math::cos(pitch),
                    bfe_pose::rig::math::sin(pitch),
                    bfe_pose::rig::math::cos(yaw) * bfe_pose::rig::math::cos(pitch),
                )
            };
            return Some(super::hud::Aim {
                point: std::array::from_fn(|k| {
                    camera.position[k] + 1024. * [direction.x, direction.y, direction.z][k]
                }),
                project_x: !pilot,
            });
        }
        if kind != mode::TURRET {
            return None;
        }
        // Gunner.reticleDirection converges from the authored aim origin,
        // including the chase rest when obstruction has moved the drawn eye.
        let x = &mut self.gunner.input;
        x.fill(0.);
        x[g::ACTIVE] = 1.;
        x[g::YAW] = a[f::TURRET_FACING] + a[f::TURRET_YAW];
        x[g::PITCH] = a[f::TURRET_PITCH];
        x[g::FACING] = a[f::TURRET_FACING];
        x[g::FIRST] = camera.first_person as u8 as f64;
        x[g::TILT] = a[f::TURRET_TILT];
        x[g::CAMERA..g::CAMERA + 3].copy_from_slice(&camera.position);
        for (from, to, count) in [
            (f::TURRET_ROOT, g::ROOT, 16),
            (f::TURRET_PIVOT, g::PIVOT, 3),
            (f::TURRET_UP, g::UP, 3),
            (f::TURRET_EYE, g::EYE_OFFSET, 3),
            (f::TURRET_CENTER, g::CENTER_OFFSET, 3),
            (f::TURRET_OFFSET, g::TRACK_OFFSET, 3),
        ] {
            x[to..to + count].copy_from_slice(&a[from..from + count]);
        }
        self.gunner.frame();
        let x = &mut self.gunner.input;
        let ray = bfe_sim::collision::Ray {
            ox: x[g::ORIGIN],
            oy: x[g::ORIGIN + 1],
            oz: x[g::ORIGIN + 2],
            dx: x[g::DIRECTION],
            dy: x[g::DIRECTION + 1],
            dz: x[g::DIRECTION + 2],
        };
        if let Some((_, hit)) =
            self.collision
                .borrow_mut()
                .ray_hit(self.world, ray, x[g::REACH], self.ignored)
        {
            x[g::WALL] = 1.;
            x[g::WALL_DISTANCE] = hit.distance;
        }
        self.gunner.wall();
        self.gunner.resolve();
        let x = &self.gunner.input;
        let direction = &x[g::RETICLE..g::RETICLE + 3];
        Some(super::hud::Aim {
            point: std::array::from_fn(|k| camera.position[k] + 1024. * direction[k]),
            project_x: false,
        })
    }
}

fn obstructs_body(
    feet: Vector3, posture: u8, scale: f64, eye: [f64; 3], projection: &[f64], view: &[f64],
) -> bool {
    let target_y = match posture {
        2 => 0.2,
        1 => 0.6,
        _ => 1.4,
    } * scale.max(0.1);
    let dx = feet.x - eye[0];
    let dy = (feet.y + target_y) - eye[1];
    let dz = feet.z - eye[2];
    // Row 2 of the column-major view matrix is camera +Z (back); negate for forward depth.
    let depth = -(dx * view[2] + dy * view[6] + dz * view[10]);
    let near = projection[14] / (projection[10] - 1.);
    depth < near
}

// RemoteDroids.enter builds a SoldierCamera from the droid's STAND parameters.
fn remote_definition(def: &bfe_content::Value) -> (bfe_presentation::cameras::soldier::Def, [f64; 3], f64) {
    use bfe_content::Value;
    use bfe_presentation::cameras::soldier::{Def, Section};
    let vector = |value: Option<&Value>, default: [f64; 3]| {
        value
            .and_then(Value::as_array)
            .filter(|v| v.len() >= 3)
            .map_or(default, |v| std::array::from_fn(|i| bfe_content::js::to_number(Some(&v[i]))))
    };
    let center = vector(def.get("TrackCenter").or_else(|| def.get("EyePointOffset")), [0., 0.5, 0.]);
    let eye = vector(def.get("EyePointOffset"), center);
    let offset = vector(def.get("TrackOffset"), [0., -0.25, 1.5]);
    let tilt = bfe_content::odf::num(def.get("TiltValue"), 5.) * std::f64::consts::PI / 180.;
    let section = Section { eye, center, offset, tilt };
    let first = bfe_content::odf::num(def.get("FirstPersonFOV"), 48.) * std::f64::consts::PI / 180.;
    let third = bfe_content::odf::num(def.get("ThirdPersonFOV"), 48.) * std::f64::consts::PI / 180.;
    let zoom_min = def
        .get("weapons")
        .and_then(Value::as_array)
        .and_then(|w| w.first())
        .and_then(|w| w.get("ZoomMin"))
        .map_or(1., |v| bfe_content::js::to_number(Some(v)))
        .max(1.);
    (Def { first, third, sections: [section; 6] }, eye, zoom_min)
}

/// How far a droideka's chase camera has gone from its standing section to its rolled one (CROUCH), 0 to 1, at
/// `time` into its drawn mode. The 2004 game takes the rolled section for the fold and the ball and the standing
/// one again as the unfold begins, its camera place following at 27/s (as ours does); here each change of section
/// is also spread over a tenth of a second, so the camera does not set off with a jolt.
fn rolled_share(mode: u8, time: f64) -> f64 {
    let ease = |x: f64| {
        let x = (x / 0.1).clamp(0., 1.);
        x * x * (3. - 2. * x)
    };
    match mode {
        11 => ease(time),
        12 => 1.,
        13 => 1. - ease(time),
        _ => 0.,
    }
}

#[cfg(test)]
mod mounted_tests {
    use super::*;
    use bfe_content::install::CollisionKernel;

    fn view_looking_at(eye: [f64; 3], target: [f64; 3]) -> [f64; 16] {
        let mut back = Vector3::new(eye[0] - target[0], eye[1] - target[1], eye[2] - target[2]);
        let len = back.length();
        if len > 1e-9 {
            back.multiply_scalar(1. / len);
        } else {
            back = Vector3::new(0., 0., 1.);
        }
        let mut view = [0.; 16];
        view[2] = back.x;
        view[6] = back.y;
        view[10] = back.z;
        view[15] = 1.;
        view
    }

    #[test]
    fn chase_hides_the_subject_when_the_near_plane_reaches_its_body() {
        let mut projection = [0.; 16];
        projection[0] = 1.;
        projection[5] = 1.8;
        projection[10] = -1.;
        projection[14] = -0.6;
        let mut forward_neg_z = [0.; 16];
        forward_neg_z[0] = 1.;
        forward_neg_z[5] = 1.;
        forward_neg_z[10] = 1.;
        forward_neg_z[15] = 1.;
        // Eye 0.20 m behind torso along -Z (depth 0.20 m < near 0.30 m): hidden.
        assert!(obstructs_body(Vector3::default(), 0, 1., [0.3, 1.8, 0.2], &projection, &forward_neg_z));
        // Normal chase and overhead views looking at the soldier stay visible.
        assert!(!obstructs_body(Vector3::default(), 0, 1., [0., 2.9, 5.], &projection, &view_looking_at([0., 2.9, 5.], [0., 1.7, 0.])));
        assert!(!obstructs_body(Vector3::default(), 0, 1., [0., 5., 0.], &projection, &view_looking_at([0., 5., 0.], [0., 1.4, 0.])));
    }

    #[test]
    fn chase_keeps_soldier_visible_when_wall_pulls_camera_within_one_metre() {
        let mut projection = [0.; 16];
        projection[0] = 1.;
        projection[5] = 1.8;
        projection[10] = -1.;
        projection[14] = -0.6;
        let blocked = |posture, eye: [f64; 3], aim_y: f64| {
            let view = view_looking_at(eye, [0., aim_y, 0.]);
            obstructs_body(Vector3::default(), posture, 1., eye, &projection, &view)
        };
        // Wall pullback distances (0.35 m..1.40 m behind standing torso) stay above the 0.30 m near plane.
        assert!(!blocked(0, [0., 1.73, 0.28], 1.70), "soldier should stay visible when backed flush against wall");
        assert!(!blocked(0, [0., 1.85, 0.65], 1.70), "soldier should stay visible at 0.65 m wall pullback");
        assert!(!blocked(0, [0., 1.90, 1.00], 1.70), "soldier should stay visible at 1.0 m wall pullback");
        assert!(!blocked(0, [0., 2.00, 1.40], 1.70), "soldier should stay visible at 1.4 m wall pullback");
        // Crouch (0.6 m) and prone (0.2 m) targets stay visible at 0.65 m and hide when depth < 0.30 m.
        assert!(!blocked(1, [0., 1.15, 0.65], 1.10));
        assert!(!blocked(2, [0., 0.55, 0.65], 0.50));
        assert!(blocked(0, [0., 1.40, 0.20], 1.40));
    }

    // A finite wall catches convergence from an obstructed camera without
    // retail content or a GL context. It faces the turret's +Z sight line.
    #[inline(never)]
    fn cameras() -> Cameras {
        let mut collision = Box::new(bfe_sim::collision::CollisionOwner::new());
        let vertices = [-10., -10., 20., 10., -10., 20., 0., 10., 20.];
        let index = [0, 2, 1];
        let bvh = bfe_content::bvh::build(bfe_content::bvh::Positions::F32(&vertices), &index);
        let positions = collision.positions(&vertices);
        let tree = collision.tree(positions, &index, &bvh);
        let world = collision.world(tree, -1, -1);
        Cameras {
            owner: Phase::new(Content::default()),
            gunner: Default::default(),
            collision: std::rc::Rc::new(std::cell::RefCell::new(collision)),
            world,
            ignored: -1,
            life: None,
            remote_life: None,
            body: None,
            definitions: vec![],
            remote_defs: vec![],
            remote_aim: None,
            first_fovs: vec![],
            eye_heights: vec![],
            zoom_eyes: vec![],
            weapon_channels: vec![],
            eye_view: false,
            scoped_eye: false,
            was_dead: false,
            death_serial: 0,
            death_hfov: 0.837758,
            death_classic: false,
            killer: None,
            killer_tick: f64::NEG_INFINITY,
        }
    }

    #[test]
    fn idle_chase_keeps_its_heading_across_view_matrix_round_trips() {
        use bfe_presentation::cameras::math::slerp;
        let camera = crate::scene::MapCamera::new([181., -1.2, -19.8], [1280, 720]).unwrap();
        let mut view = *camera.snapshot();
        let mut target = Quaternion::default();
        target.set_from_euler(&three::Euler::new(-0.35, 1.86, 0., three::EulerOrder::YXZ));
        let mut rotation = target;
        rotation.x *= 1. + f64::EPSILON;
        for _ in 0..1800 {
            let mut matrix = Matrix4::default();
            matrix.compose(&Vector3::new(181., -1.2, -19.8), &rotation, &Vector3::new(1., 1., 1.));
            matrix.invert();
            view.values[16..32].copy_from_slice(&matrix.elements);
            rotation = slerp::<V8>(view_rotation(&view), target, 1. - (-9. / 60_f64).exp());
            assert!((rotation.length_sq() - 1.).abs() < 1e-12);
            assert!((rotation.dot(&target).abs() - 1.).abs() < 1e-12);
        }
    }

    #[test]
    fn mounted_camera_excludes_its_body_but_keeps_the_map_wall() {
        let cameras = cameras();
        let body = {
            let mut collision = cameras.collision.borrow_mut();
            let vertices = [-10., -10., 2., 10., -10., 2., 0., 10., 2.];
            let index = [0, 2, 1];
            let bvh = bfe_content::bvh::build(bfe_content::bvh::Positions::F32(&vertices), &index);
            let positions = collision.positions(&vertices);
            let tree = collision.tree(positions, &index, &bvh);
            let body = collision.body_new(tree, 0) as u32;
            collision.io_mut()[20..26].copy_from_slice(&[-10., -10., 2., 10., 10., 2.]);
            collision.body_set(body);
            let world = &mut collision.state.worlds[cameras.world as usize];
            world.order.push(body);
            world.version += 1;
            collision.enable_body(body, true);
            body
        };
        let mut queries = Queries { collision: cameras.collision, world: cameras.world, ignored: -1 };
        let from = Vector3::new(0., 0., 0.);
        let ahead = Vector3::new(0., 0., 1.);
        assert_eq!(queries.cast(from, ahead, 30., false), Some(2.));
        queries.ignored = body as i32;
        assert_eq!(queries.cast(from, ahead, 30., false), Some(20.));
        queries.ignored = -1;
        assert_eq!(queries.cast(from, ahead, 30., false), Some(2.));
    }

    #[test]
    fn ground_seat_and_flight_pilot_use_their_own_aim_frames() {
        let mut cameras = cameras();
        let camera = crate::scene::MapCamera::new([1., 2., 3.], [1280, 720]).unwrap();
        cameras.owner.output[o::MODE] = bfe_presentation::layout::camera_mode::VEHICLE as f64;
        cameras.owner.input[f::SEAT_YAW] = std::f64::consts::FRAC_PI_2;
        let ground = cameras.mounted_aim(&camera).unwrap();
        assert!(ground.project_x);
        assert!((ground.point[0] - 1025.).abs() < 1e-9);
        assert!((ground.point[2] - 3.).abs() < 1e-9);
        // A pilot's reticle follows the shown nose, independently of the
        // retained gunner/infantry angles, and stays horizontally centered.
        cameras.owner.input[f::VEHICLE_FLYING] = 1.;
        cameras.owner.input[f::VEHICLE_FLIGHT_FRAME + 3] = 1.;
        let flight = cameras.mounted_aim(&camera).unwrap();
        assert!(!flight.project_x);
        assert_eq!(flight.point, [1., 2., 1027.]);
    }

    #[test]
    fn turret_reticle_meets_the_world_hit_after_camera_obstruction() {
        let mut cameras = cameras();
        let camera = crate::scene::MapCamera::new([1., 1., -2.], [1280, 720]).unwrap();
        cameras.owner.output[o::MODE] = bfe_presentation::layout::camera_mode::TURRET as f64;
        let x = &mut cameras.owner.input;
        x[f::TURRET_ROOT..f::TURRET_ROOT + 16].copy_from_slice(&Matrix4::default().elements);
        x[f::TURRET_UP + 1] = 1.;
        x[f::TURRET_EYE..f::TURRET_EYE + 3].copy_from_slice(&[0., 1.5, 1.]);
        x[f::TURRET_CENTER..f::TURRET_CENTER + 3].copy_from_slice(&[0., 1.5, -2.]);
        x[f::TURRET_OFFSET..f::TURRET_OFFSET + 3].copy_from_slice(&[0., 0.5, 1.5]);
        let aim = cameras.mounted_aim(&camera).unwrap();
        let mut actual = Vector3::new(aim.point[0] - 1., aim.point[1] - 1., aim.point[2] + 2.);
        actual.normalize();
        let mut expected = Vector3::new(-1., 1., 22.);
        expected.normalize();
        assert!(actual.distance_to(&expected) < 1e-10);
        assert!(!aim.project_x);
    }

    #[test]
    fn zoom_first_person_keeps_released_truthy_for_2004_and_parses_strings_for_2005() {
        let parse = |text: &str| bfe_content::json::parse(text).unwrap();
        let flag_true = parse(r#"{"ZoomFirstPerson":true}"#);
        let str_zero = parse(r#"{"ZoomFirstPerson":"0"}"#);
        let str_one = parse(r#"{"ZoomFirstPerson":"1"}"#);
        assert!(zoom_first_person_for_family(bfe_family::GameFamily::Swbf1, &flag_true));
        // Released 2004 truthy() treats any non-empty string as truthy.
        assert!(zoom_first_person_for_family(bfe_family::GameFamily::Swbf1, &str_zero));
        assert!(zoom_first_person_for_family(bfe_family::GameFamily::Swbf1, &str_one));
        // 2005 parses numeric strings and booleans accurately.
        assert!(zoom_first_person_for_family(bfe_family::GameFamily::Swbf2, &flag_true));
        assert!(!zoom_first_person_for_family(bfe_family::GameFamily::Swbf2, &str_zero));
        assert!(zoom_first_person_for_family(bfe_family::GameFamily::Swbf2, &str_one));
    }
}
