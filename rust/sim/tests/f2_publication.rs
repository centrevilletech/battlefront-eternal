//! Dirty hull publication, including synchronous callbacks and slab reuse.
use bfe_sim::gen::{Store, Vehicle};
use bfe_sim::vehicles::f2_publication::Publication;

#[test]
fn a_command_publishes_only_the_hull_it_changed() {
    let mut st = Store::default(); let mut p = Publication::default();
    let a = st.vehicle.insert(Vehicle::default()); let b = st.vehicle.insert(Vehicle::default());
    p.register(&st, a, 0); p.register(&st, b, 0); p.flush(&mut st);
    st.vehicle.get_mut(a).unwrap().health = 12.0;
    p.mark(a); p.flush(&mut st);
    assert_eq!(p.hulls.rows(), 1);
    assert_eq!(p.hulls.read(0).0[0], a.slot as f64);
    assert_eq!(p.lives.rows(), 0, "health does not recopy unchanged life metadata");
    p.mark(a); p.flush(&mut st);
    assert_eq!(p.hulls.rows(), 0, "identical rows are not copied");
}

#[test]
fn nested_flushes_leave_the_next_phase_without_stale_rows() {
    let mut st = Store::default(); let mut p = Publication::default();
    let a = st.vehicle.insert(Vehicle::default()); let b = st.vehicle.insert(Vehicle::default());
    p.register(&st, a, 0); p.register(&st, b, 0); p.flush(&mut st);
    st.vehicle.get_mut(a).unwrap().health = 1.0; p.mark(a);
    st.vehicle.get_mut(b).unwrap().health = 2.0; p.mark(b);
    p.flush(&mut st);
    assert_eq!(p.hulls.rows(), 2);
    st.vehicle.get_mut(b).unwrap().health = 3.0; p.mark(b); p.flush(&mut st);
    assert_eq!(p.hulls.rows(), 1);
    assert_eq!(p.hulls.read(0).0[0], b.slot as f64);
    p.flush(&mut st); assert_eq!(p.hulls.rows(), 0);
}

#[test]
fn signed_zero_life_changes_and_retired_rows_are_not_lost() {
    let mut st = Store::default(); let mut p = Publication::default();
    let a = st.vehicle.insert(Vehicle { yaw: 0.0, ..Vehicle::default() });
    p.register(&st, a, 0); p.flush(&mut st);
    st.vehicle.get_mut(a).unwrap().yaw = -0.0; p.mark(a); p.flush(&mut st);
    assert_eq!(p.hulls.rows(), 1);
    st.vehicle.get_mut(a).unwrap().target_generation = 2.0; p.mark(a); p.flush(&mut st);
    assert_eq!(p.lives.rows(), 1);
    st.vehicle.retire(a); p.flush(&mut st);
    assert_eq!(p.hulls.words[4], 1.0);
    assert_eq!(p.hulls.read(0).0[1], a.gen as f64);
    let b = st.vehicle.insert(Vehicle::default()); p.register(&st, b, 0); p.flush(&mut st);
    assert_eq!(a.slot, b.slot); assert_ne!(a.gen, b.gen);
    assert_eq!(p.hulls.read(0).0[1], b.gen as f64);
}
