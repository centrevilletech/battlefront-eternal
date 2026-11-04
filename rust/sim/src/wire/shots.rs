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
#[no_mangle]
pub extern "C" fn wire_shots_create(capacity: f64) -> *mut Counters {
    Box::into_raw(Box::new(Counters {
        capacity,
        ..Counters::default()
    }))
}
#[no_mangle]
pub unsafe extern "C" fn wire_shots_drop(p: *mut Counters) {
    if !p.is_null() {
        drop(Box::from_raw(p));
    }
}
#[no_mangle]
pub unsafe extern "C" fn wire_shots_observe(p: *mut Counters, id: f64, generation: f64, live: f64) {
    (&mut *p).observe(id, generation, live);
}
#[no_mangle]
pub unsafe extern "C" fn wire_shots_counter(p: *const Counters, id: f64, generation: f64) -> f64 {
    (&*p).counter(id, generation)
}
#[no_mangle]
pub unsafe extern "C" fn wire_shots_reset(p: *mut Counters) {
    (&mut *p).reset();
}
#[no_mangle]
pub unsafe extern "C" fn wire_shots_overflow(p: *const Counters) -> f64 {
    (*p).overflow
}
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
