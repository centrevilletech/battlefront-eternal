//! Fire samples use the camera and target poses that were actually drawn.
use super::{receiver::Own, replay::Collision};
use bfe_presentation::{aim::{CommandAim, View}, cameras::math::V8};
use bfe_sim::{collision::Ray, units::shapes::{HitShape, segment_shape}};
use std::{cell::RefCell, rc::Rc};
use three::Vector3;

pub type Shared = Rc<RefCell<Scene>>;
#[derive(Clone, Copy)]
pub struct Class { pub track: [f64; 3], pub scale: f64 }
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
        let class = self.classes.get(own.class).copied().unwrap_or(Class {track: [3.5;3], scale: 1.});
        let position = Vector3::new(self.position[0],self.position[1],self.position[2]);
        let mut aim = View { position, feet: position, yaw, pitch, posture: own.posture,
            air: own.air != 0, zoom: own.zoom, camera: Some(Vector3::new(self.camera[0],self.camera[1],self.camera[2])),
            uses_eye: first || (self.eye_view && own.zoom>1.) || own.walker || own.mount.is_some() || own.remote.is_some(),
            track_offsets: class.track, ..Default::default() }.sample::<V8>();
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
}
