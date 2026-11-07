//! Shot counters describe actual simulation shots across unit generations.
//! A stale source or saturated table never invents additional shots.
#[derive(Default)]
pub struct Counters {
    pub capacity: f64,
    pub overflow: f64,
    pub units: Vec<(f64, f64, f64)>,
}
fn integer(n: f64) -> bool {
    n.is_finite() && n.fract() == 0. && n.abs() <= 9007199254740991.
}
impl Counters {
    pub fn observe(&mut self, id: f64, generation: f64, live_generation: f64) {
        if !integer(id) || !integer(generation) || live_generation != generation {
            return;
        }
        let at = self.units.iter().position(|x| x.0 == id);
        if at.is_some_and(|i| generation < self.units[i].1) {
            return;
        }
        if at.is_none() && self.units.len() as f64 >= self.capacity {
            self.overflow += 1.;
            return;
        }
        let i = at.unwrap_or_else(|| {
            self.units.push((id, generation, 0.));
            self.units.len() - 1
        });
        let state = &mut self.units[i];
        if state.1 != generation {
            *state = (id, generation, 0.);
        }
        if state.2 == 4294967295. {
            self.overflow += 1.;
            return;
        }
        state.2 += 1.;
    }
    pub fn counter(&self, id: f64, generation: f64) -> f64 {
        self.units
            .iter()
            .find(|x| x.0 == id && x.1 == generation)
            .map_or(0., |x| x.2)
    }
    pub fn reset(&mut self) {
        self.units.clear();
        self.overflow = 0.;
    }
}
// Canonical fired notices resolve counters in this battle's owned bank.
// Registration is cold, bounded, and never retains a sibling allocation.
pub(crate) fn native_observe_in(w: &mut crate::world::owner::World, id:f64,generation:f64,live:f64) {
    w.wire.as_mut().expect("wire observer outside publication").observe_shot(id,generation,live);
}
pub(crate) fn wire_shots_attach_in(w: &mut crate::world::owner::World, counter:usize) ->u32 {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
    let key=counter as usize;if owner.shots.get(key).is_none(){return 0;}
    if owner.attached_shots.contains(&key){return 1;}
    if owner.attached_shots.len()==16{return 0;}
    owner.attached_shots.reserve(16usize.saturating_sub(owner.attached_shots.len()));
    owner.attached_shots.push(key);1

}
pub(crate) fn wire_shots_create_in(w: &mut crate::world::owner::World, capacity:f64) ->usize {
    if !integer(capacity)||!(1. ..=65536.).contains(&capacity){return 0;}
    super::owner::with_in(w, |owner|owner.shots.insert(Counters {capacity,units:Vec::with_capacity(capacity as usize),..Default::default()}).unwrap_or(0))
}
pub(crate) fn wire_shots_drop_in(w: &mut crate::world::owner::World, p:usize) {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");let key=p as usize;owner.attached_shots.retain(|&h|h!=key);owner.shots.remove(key);
}
pub(crate) fn wire_shots_observe_in(w: &mut crate::world::owner::World, p:usize,id:f64,generation:f64,live:f64) {
    super::owner::with_in(w, |owner|if let Some(c)=owner.shots.get_mut(p as usize){c.observe(id,generation,live)});
}
pub(crate) fn wire_shots_counter_in(w: &mut crate::world::owner::World, p:usize,id:f64,generation:f64) ->f64 {
    super::owner::with_in(w, |owner|owner.shots.get(p as usize).map_or(0.,|c|c.counter(id,generation)))
}
pub(crate) fn wire_shots_reset_in(w: &mut crate::world::owner::World, p:usize) {super::owner::with_in(w, |owner|if let Some(c)=owner.shots.get_mut(p as usize){c.reset()});}
pub(crate) fn wire_shots_overflow_in(w: &mut crate::world::owner::World, p:usize) ->f64 {super::owner::with_in(w, |owner|owner.shots.get(p as usize).map_or(0.,|c|c.overflow))}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_generations_capacity_and_saturation_never_increment() {
        let mut c = Counters {
            capacity: 1.,
            ..Counters::default()
        };
        c.observe(1., 2., 2.);
        c.observe(1., 1., 1.);
        c.observe(1., 3., 2.);
        c.observe(2., 1., 1.);
        assert_eq!(c.counter(1., 2.), 1.);
        assert_eq!(c.overflow, 1.);
        c.observe(1., 3., 3.);
        assert_eq!(c.counter(1., 2.), 0.);
        assert_eq!(c.counter(1., 3.), 1.);
        c.units[0].2 = 4294967295.;
        c.observe(1., 3., 3.);
        assert_eq!(c.overflow, 2.);
        assert_eq!(c.counter(1., 3.), 4294967295.);
        c.reset();
        assert_eq!(c.overflow, 0.);
        assert!(c.units.is_empty());
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn native_observe(id:f64,generation:f64,live:f64) { native_observe_in(crate::abi::world(),id,generation,live); }

#[cfg(test)]
mod owner_tests {
    use super::*;
    use crate::world::owner::World;

    #[test]
    fn counters_follow_the_explicit_world_and_retire_their_generation() {
        let (mut a, mut b) = (World::new(), World::new());
        let ka = wire_shots_create_in(&mut a, 2.);
        let kb = wire_shots_create_in(&mut b, 2.);
        assert_eq!(ka, kb);
        unsafe {
            assert_eq!(wire_shots_attach_in(&mut a, ka), 1);
            assert_eq!(wire_shots_attach_in(&mut b, kb), 1);
            native_observe_in(&mut a, 7., 1., 1.);
            assert_eq!(wire_shots_counter_in(&mut a, ka, 7., 1.), 1.);
            assert_eq!(wire_shots_counter_in(&mut b, kb, 7., 1.), 0.);
            native_observe_in(&mut b, 7., 1., 1.);
            native_observe_in(&mut a, 7., 2., 2.);
            assert_eq!(wire_shots_counter_in(&mut a, ka, 7., 2.), 1.);
            assert_eq!(wire_shots_counter_in(&mut b, kb, 7., 1.), 1.);
            wire_shots_drop_in(&mut a, ka);
            let replacement = wire_shots_create_in(&mut a, 2.);
            assert_ne!(ka, replacement);
            assert_eq!(wire_shots_attach_in(&mut a, ka), 0);
            native_observe_in(&mut a, 7., 3., 3.);
            assert_eq!(wire_shots_counter_in(&mut a, replacement, 7., 3.), 0.);
            assert_eq!(wire_shots_counter_in(&mut b, kb, 7., 1.), 1.);
        }
    }
}

// The WASM boundary owns addresses; shared services borrow their owners.
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    #[no_mangle]
    pub extern "C" fn wire_shots_attach(counter:*mut Counters) ->u32 { wire_shots_attach_in(crate::abi::facade::world(), counter as usize) }
    #[no_mangle]
    pub extern "C" fn wire_shots_create(capacity:f64) ->*mut Counters { wire_shots_create_in(crate::abi::facade::world(), capacity) as *mut Counters }
    #[no_mangle]
    pub extern "C" fn wire_shots_drop(p:*mut Counters) { wire_shots_drop_in(crate::abi::facade::world(), p as usize) }
    #[no_mangle]
    pub extern "C" fn wire_shots_observe(p:*mut Counters,id:f64,generation:f64,live:f64) { wire_shots_observe_in(crate::abi::facade::world(), p as usize, id, generation, live) }
    #[no_mangle]
    pub extern "C" fn wire_shots_counter(p:*const Counters,id:f64,generation:f64) ->f64 { wire_shots_counter_in(crate::abi::facade::world(), p as usize, id, generation) }
    #[no_mangle]
    pub extern "C" fn wire_shots_reset(p:*mut Counters) { wire_shots_reset_in(crate::abi::facade::world(), p as usize) }
    #[no_mangle]
    pub extern "C" fn wire_shots_overflow(p:*const Counters) ->f64 { wire_shots_overflow_in(crate::abi::facade::world(), p as usize) }
}
#[cfg(target_arch = "wasm32")]
pub use exports::{wire_shots_attach, wire_shots_create, wire_shots_drop, wire_shots_observe, wire_shots_counter, wire_shots_reset, wire_shots_overflow};
