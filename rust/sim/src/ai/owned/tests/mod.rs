//! Canonical AI fixtures enter through one World, including their stores,
//! seats and navigation observations. Test callbacks retain explicit scopes.
mod phase;
mod native_phase;

impl std::ops::Deref for crate::world::owner::World {
    type Target=crate::gen::Store;
    fn deref(&self)->&Self::Target { &self.sim.store }
}
impl std::ops::DerefMut for crate::world::owner::World {
    fn deref_mut(&mut self)->&mut Self::Target { &mut self.sim.store }
}

pub(crate) fn empty_world()->Box<crate::world::owner::World> {
    let mut w=crate::world::owner::World::new();
    *crate::battle::f4::live_in(&mut w.battle)=crate::battle::live_phase::Live::new(0,0,0);
    crate::vehicles::module::f2::live_seats::admit_test_seats(&mut w.fleet,Default::default());
    w
}

#[cfg(all(not(target_arch = "wasm32"), panic = "unwind"))]
mod mount_lifetime;
