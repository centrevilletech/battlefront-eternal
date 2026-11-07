//! Direct joins use the same camera selector and seat camera owner as the
//! page. The host contributes published hull matrices and collision queries;
//! obstruction, eye/chase selection, easing and lenses remain in Phase.
use super::{hulls::Hulls, receiver::Receiver, replay::Collision};
use bfe_presentation::{
    cameras::{math::V8, Cast, Content, Phase},
    layout::{camera_frame as f, camera_output as o, VERSION},
};
use three::{Matrix4, Quaternion, Vector3};
pub struct Cameras {
    owner: Phase,
    collision: Collision,
    world: u32,
    life: Option<(u32, u32, usize)>,
    body: Option<(f64, f64, usize)>,
    definitions: Vec<usize>,
}
struct Queries {
    collision: Collision,
    world: u32,
}
impl Cast for Queries {
    fn cast(&mut self, origin: Vector3, direction: Vector3, far: f64, _: bool) -> Option<f64> {
        let ray = bfe_sim::collision::Ray { ox: origin.x, oy: origin.y, oz: origin.z, dx: direction.x, dy: direction.y, dz: direction.z };
        self.collision.borrow_mut().ray_hit(self.world, ray, far, -1).map(|(_, h)| h.distance)
    }
}
impl Cameras {
    pub fn new(content: &bfe_content_native::BattleContent, collision: Collision, world: u32) -> Result<Self, String> {
        let defs = bfe_content::defs::Defs::build(&content.content)?;
        let mut cameras = Content::default();
        let mut definitions = vec![usize::MAX; defs.registry.classes.len() + 1];
        for class in &defs.classes {
            if let Some(id) = class.registry_id {
                definitions[id as usize] = cameras.definitions.len();
                cameras.definitions.push(definition(&class.record));
            }
        }
        Ok(Self { owner: Phase::new(cameras), collision, world, life: None, body: None, definitions })
    }
    pub fn frame(
        &mut self,
        receiver: &Receiver,
        prediction: &super::replay::Replay,
        hulls: &mut Hulls,
        turrets: &mut super::turrets::Turrets,
        camera: &mut crate::scene::MapCamera,
        dt: f64,
    ) {
        let lease = receiver.own.mount;
        hulls.hidden = None;
        turrets.hidden = None;
        let view = camera.snapshot();
        let mut world = Matrix4::default();
        world.elements.copy_from_slice(&view.values[16..32]);
        world.invert();
        let mut q = Quaternion::default();
        q.set_from_rotation_matrix(&world);
        let x = &mut self.owner.input;
        x.fill(0.);
        x[f::VERSION] = VERSION as f64;
        x[f::DT] = dt;
        x[f::FIRST_PERSON] = 1.;
        x[f::OWN_EYES] = 1.;
        x[f::ZOOM] = receiver.own.zoom.max(1.);
        x[f::CAMERA_POSITION..f::CAMERA_POSITION + 3].copy_from_slice(&camera.position);
        x[f::CAMERA_QUATERNION..f::CAMERA_QUATERNION + 4].copy_from_slice(&[q.x, q.y, q.z, q.w]);
        x[f::CAMERA_UP + 1] = 1.;
        x[f::CAMERA_FOV] = 60.;
        x[f::CAMERA_ASPECT] = camera.aspect();
        x[f::CAMERA_NEAR] = 0.3;
        x[f::CAMERA_FAR] = 20000.;
        let body = (receiver.own.id, receiver.own.generation, receiver.own.class);
        x[f::SOLDIER_DEF] = self.definitions.get(receiver.own.class).copied().unwrap_or(usize::MAX) as f64;
        x[f::SOLDIER_RESET] = (self.body != Some(body) || self.life != lease) as u8 as f64;
        x[f::RESPONSIVE] = 1.;
        x[f::HAS_EYE] = 1.;
        x[f::EYE..f::EYE + 3].copy_from_slice(&camera.position);
        x[f::FEET..f::FEET + 3].copy_from_slice(&receiver.own.position);
        x[f::YAW] = receiver.own.yaw;
        x[f::PITCH] = receiver.own.pitch;
        x[f::POSTURE] = receiver.own.posture as f64;
        if let Some(body) = &prediction.body {
            x[f::FEET..f::FEET + 3].copy_from_slice(&body.position);
            x[f::YAW] = body.aim_yaw;
            x[f::PITCH] = body.aim_pitch;
            x[f::POSTURE] = if body.air.is_some() { 0. } else { body.posture as u8 as f64 };
            x[f::VELOCITY..f::VELOCITY + 3].copy_from_slice(&body.velocity);
        }
        x[f::VEHICLE_RESET] = (self.life != lease) as u8 as f64;
        let hull = lease.and_then(|lease| hulls.camera(lease.0 as f64, lease.1 as f64, lease.2, x));
        let turret = if hull.is_none() { lease.and_then(|lease| turrets.camera(lease.0 as f64, lease.1 as f64, x)) } else { None };
        self.body = Some(body);
        self.life = lease;
        let mut queries = Queries { collision: self.collision.clone(), world: self.world };
        self.owner.frame::<V8, _>(&mut queries);
        if self.owner.output[o::STATUS] == 0. {
            camera.accept_camera(&self.owner.output);
            if self.owner.output[o::ENCLOSED] != 0. {
                hulls.hidden = hull;
            }
            if self.owner.output[o::TURRET_ENCLOSED] != 0. {
                turrets.hidden = turret;
            }
        }
    }
}

// Cameras.definition resolves the class's six posture/zoom sections once.
// SoldierCamera owns horizontal FOV conversion, zoom and eye/chase behavior;
// entering or leaving a seat selects another branch of that same phase.
fn definition(class: &bfe_content::Value) -> bfe_presentation::cameras::soldier::Def {
    use bfe_content::Value;
    use bfe_presentation::cameras::soldier::{Def, Section};
    let source = class.get("cameras");
    let vector = |value: Option<&Value>, default: [f64; 3]| {
        value
            .and_then(Value::as_array)
            .filter(|v| v.len() >= 3)
            .map_or(default, |v| std::array::from_fn(|i| bfe_content::js::to_number(Some(&v[i]))))
    };
    let sections = std::array::from_fn(|index| {
        let name = ["STAND", "CROUCH", "PRONE"][index % 3];
        let zoom = format!("{name}ZOOM");
        let section = source
            .and_then(|s| if index >= 3 { s.get(&zoom).filter(|v| v.truthy()) } else { None })
            .or_else(|| source.and_then(|s| s.get(name)).filter(|v| v.truthy()))
            .or_else(|| source.and_then(|s| s.get("STAND")));
        let get = |key: &str| section.and_then(|s| s.get(key));
        Section {
            eye: vector(get("EyePointOffset").or_else(|| get("TrackCenter")), [0., 1.8, 0.]),
            center: vector(get("TrackCenter").or_else(|| get("EyePointOffset")), [0., 1.8, 0.]),
            offset: vector(get("TrackOffset"), [0., 0., 3.5]),
            tilt: bfe_content::odf::num(get("TiltValue"), 5.) * std::f64::consts::PI / 180.,
        }
    });
    Def {
        first: bfe_content::odf::num(class.get("FirstPersonFOV"), 70.) * std::f64::consts::PI / 180.,
        third: bfe_content::odf::num(class.get("ThirdPersonFOV"), 65.) * std::f64::consts::PI / 180.,
        sections,
    }
}
