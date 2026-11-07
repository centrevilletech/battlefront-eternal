//! Classic's camera facts are captured once after camera update. Every
//! controller in that decision/input phase observes this same compact block.
use crate::gen::{AiLifeHandle, AiPhaseCamera, Store};
use super::super::{control::{scheduler::position_visible_or_near, world::CameraView}, nav::math::Vec3};

pub struct Lod {
    cameras: Vec<CameraView>,
    network: bool,
    disabled: bool,
}
impl Lod {
    pub fn new(max_views: usize) -> Self {
        Self { cameras: Vec::with_capacity(max_views), network: false, disabled: false }
    }
    /// A connection may reserve more room outside a running phase. Ordinary
    /// camera input cannot grow memory or truncate a network player's view.
    pub fn reserve(&mut self, max_views: usize) {
        self.cameras.reserve(max_views.saturating_sub(self.cameras.len()));
    }
    pub fn update(&mut self, cameras: &[AiPhaseCamera], network: bool, disabled: bool) {
        assert!(cameras.len() <= self.cameras.capacity(), "unreserved AI camera input");
        self.cameras.clear();
        for camera in cameras { self.cameras.push(CameraView::from(camera)); }
        self.network = network; self.disabled = disabled;
    }
    pub fn cameras(&self) -> &[CameraView] { &self.cameras }
    pub fn visible(&self, store: &Store, key: &AiLifeHandle) -> Option<bool> {
        let unit = key.unit(store)?;
        let p = unit.position(); let v = unit.velocity();
        Some(position_visible_or_near(Vec3::new(p.x,p.y,p.z), Some(Vec3::new(v.x,v.y,v.z)),
            &self.cameras,self.network,self.disabled))
    }
}
