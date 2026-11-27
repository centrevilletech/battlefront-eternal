//! Canonical AI fixtures enter through one World, including their stores,
//! seats and navigation observations. Test callbacks retain explicit scopes.
pub(crate) mod phase;
mod native_phase;
pub(crate) use native_phase::soldier_requests_recovery;

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

mod mount_lifetime;
mod goal_bots;
mod external_walk;


thread_local! { static COUNT_RULES: std::cell::Cell<bool> = const {std::cell::Cell::new(false)}; }

/// The global test allocator measures only the rules handler in the selected world ticks.
pub(crate) fn count_rule_allocations(f:impl FnOnce()) -> usize {
    COUNT_RULES.with(|armed| {
        assert!(!armed.replace(true));
        phase::ALLOCATIONS.with(|n| n.set(0));
        f();
        armed.set(false);
        phase::ALLOCATIONS.with(std::cell::Cell::get)
    })
}
pub(crate) fn rules_handler(f:impl FnOnce()) {
    if COUNT_RULES.with(std::cell::Cell::get) {
        phase::COUNTING.with(|on| {on.set(true); f(); on.set(false);});
    } else { f(); }
}
