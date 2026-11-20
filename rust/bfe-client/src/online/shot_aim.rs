//! Fire samples use the camera and target poses that were actually drawn.
use super::{receiver::Own, replay::Collision};
use bfe_presentation::{aim::{CommandAim, View}, cameras::math::V8};
use bfe_sim::{collision::Ray, units::shapes::{HitShape, segment_shape}};
use std::{cell::RefCell, rc::Rc};
use three::Vector3;

pub type Shared = Rc<RefCell<Scene>>;
#[derive(Clone, Copy)]
pub struct Class {
    pub track: [f64; 3],
    pub scale: f64,
    /// A droideka class's eye height and STAND camera, which its aim line
    /// starts from (`bfe_presentation::aim::camera_line`).
    pub walker: Option<(f64, bfe_presentation::cameras::soldier::Section)>,
    /// The class's cameras, which a soldier's crosshair ray starts from.
    pub sections: Option<[bfe_presentation::cameras::soldier::Section; 6]>,
}
pub struct Scene {
    collision: Collision,
    world: u32,
    pub camera: [f64; 3],
    pub eye_view: bool,
    pub view_projection: [f64;32],
    pub position: [f64; 3],
    pub classes: Vec<Class>,
    pub targets: Vec<(u32, HitShape)>,
}
impl Scene {
    pub fn new(collision: Collision, world: u32, classes: Vec<Class>) -> Shared {
        Rc::new(RefCell::new(Self { collision, world, classes, camera: [0.;3], eye_view: false, view_projection:[0.;32],
            position: [0.;3], targets: Vec::with_capacity(4096) }))
    }
    pub fn sample(&self, own: &Own, yaw: f64, pitch: f64, first: bool) -> (CommandAim, f64, Option<u32>) {
        let class = self.classes.get(own.class).copied().unwrap_or(Class {track: [3.5;3], scale: 1., walker: None, sections: None});
        let position = Vector3::new(self.position[0],self.position[1],self.position[2]);
        let mut aim = View { position, feet: position, yaw, pitch, posture: own.posture,
            air: own.air != 0, zoom: own.zoom, camera: Some(Vector3::new(self.camera[0],self.camera[1],self.camera[2])),
            uses_eye: first || (self.eye_view && own.zoom>1.) || own.walker || own.mount.is_some() || own.remote.is_some(),
            track_offsets: class.track, sections: class.sections, ..Default::default() }.sample::<V8>();
        if !aim.needs_reach {aim.from=aim.eye;}
        let from = aim.from; let dir = aim.basis.direction;
        let ray = Ray {ox:from.x,oy:from.y,oz:from.z,dx:dir.x,dy:dir.y,dz:dir.z};
        let mut reach = self.collision.borrow_mut().ray_hit(self.world, ray, 60., -1).map_or(60., |(_, hit)| hit.distance);
        let mut target = None;
        for (id, shape) in &self.targets {
            if *id == own.id as u32 { continue; }
            if let Some(distance) = segment_shape(&from, &dir, reach, shape) {
                if distance < reach { reach = distance; target = Some(*id); }
            }
        }
        aim.resolve::<V8>(reach);
        (aim, reach, target)
    }
    /// The point a droideka player's rounds converge on, as the world finds
    /// it: the first surface or drawn body along the line its reticle sits
    /// on, within 512 m, its own body left out (`eyes`: first person or
    /// zoomed, the eye's own line). None for a class that is no droideka.
    pub fn walker_target(&self, own: &Own, feet: [f64; 3], yaw: f64, pitch: f64, eyes: bool) -> Option<[f64; 3]> {
        self.walker_point(own.class, own.id as u32, feet, yaw, pitch, eyes)
    }
    /// The same for any drawn droideka of class `class` (unit `id`, left
    /// out of its own line).
    pub fn walker_point(&self, class: usize, id: u32, feet: [f64; 3], yaw: f64, pitch: f64, eyes: bool) -> Option<[f64; 3]> {
        let (height, stand) = self.classes.get(class)?.walker?;
        let feet = Vector3::new(feet[0], feet[1], feet[2]);
        let eye = Vector3::new(feet.x, feet.y + height, feet.z);
        let (from, dir) = bfe_presentation::aim::camera_line::<V8>(feet, yaw, pitch, eye, (!eyes).then_some(&stand));
        let ray = Ray {ox:from.x,oy:from.y,oz:from.z,dx:dir.x,dy:dir.y,dz:dir.z};
        let mut reach = self.collision.borrow_mut().ray_hit(self.world, ray, REACH, -1).map_or(REACH, |(_, hit)| hit.distance);
        for (target, shape) in &self.targets {
            if *target == id { continue; }
            if let Some(distance) = segment_shape(&from, &dir, reach, shape) { reach = reach.min(distance); }
        }
        let mut point = from;
        point.add_scaled_vector(&dir, reach);
        Some([point.x, point.y, point.z])
    }
}
// How far a droideka's aim line looks (the 2004 game's 512 m).
const REACH: f64 = 512.;
