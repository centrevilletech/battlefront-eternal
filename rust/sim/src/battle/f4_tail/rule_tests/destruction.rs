use super::*;
use crate::battle::{f4, f4_live, stats};
use crate::vehicles::module::f2::live_seats;

fn hit_hull(w: &mut Root, vehicle: Ref<Vehicle>, shooter: Ref<Unit>, damage: f64) {
    let body = w.sim.store.vehicle.get(vehicle).unwrap().body;
    w.tick.running = true;
    f3::with_services_in(w, |w, services| {
        let packet = crate::combat::impacts::HitPacket { damage, scales: [1.; 5], shield: 1. };
        crate::units::f3_hulls::struck_in(w, body, packet, OrdnanceOwner::Unit(shooter), services.unwrap());
    });
}

fn scratch_hull(w: &mut Root, vehicle: Ref<Vehicle>, shooter: Ref<Unit>) {
    hit_hull(w, vehicle, shooter, 10.);
    let v = w.sim.store.vehicle.get(vehicle).unwrap();
    assert_eq!((v.health, v.last_attacker), (90., OrdnanceOwner::Unit(shooter)));
}

fn assert_no_hull_credit(w: &mut Root, vehicle: Ref<Vehicle>, shooter: Ref<Character>) {
    let v = w.sim.store.vehicle.get(vehicle).unwrap();
    assert!(!v.alive);
    assert_eq!(v.last_attacker, OrdnanceOwner::Null);
    let c = w.sim.store.character.get_mut(shooter).unwrap();
    assert_eq!(c.kills, 0.);
    let slot = stats::slot_of(c.team);
    assert_eq!(stats::record_of(c).slots[slot].vehicles_destroyed, 0.);
}

#[test]
fn conquest_boundary_wreck_credits_nobody() {
    check_boundary_credit(false, false);
}

#[test]
fn event_driven_boundary_wreck_credits_nobody() {
    check_boundary_credit(true, false);
}

#[test]
fn lethal_hit_before_the_boundary_wreck_credits_the_shooter() {
    for event_driven in [false, true] {
        check_boundary_credit(event_driven, true);
    }
}

fn check_boundary_credit(event_driven: bool, lethal_hit: bool) {
    let mut w = world();
    let (rider, victim) = soldier(&mut w, 2.);
    let (shooter, attacker) = soldier(&mut w, 1.);
    w.sim.store.character.get_mut(victim).unwrap().out_of_bounds = -1.;
    let UnitBody::Soldier(body) = &mut w.sim.store.unit.get_mut(shooter).unwrap().soldier else { unreachable!() };
    body.position = [12., 0., 0.];
    let vehicle = crewed_hull(&mut w, rider);
    // The shooter is inside (x >= 4), the occupied hull at the origin is outside.
    prepare_tail_with_boundary(&mut w, BfConfig {
        polygons: vec![BfPolygon { edges: vec![[4., 0., -1., 0.]] }],
    });
    let events = event_driven.then(|| record(&mut w));
    scratch_hull(&mut w, vehicle, shooter);
    // A 2005 (event-driven) battle gives 6.5 s to come back, a 2004 one 10 s.
    if event_driven {
        for _ in 0..13 { tick(&mut w, 0.5); }
    } else {
        for _ in 0..10 { tick(&mut w, 1.); }
    }
    assert!(w.sim.store.vehicle.get(vehicle).unwrap().alive);
    assert_eq!(w.sim.store.character.get(victim).unwrap().out_of_bounds, 0.);
    if lethal_hit {
        hit_hull(&mut w, vehicle, shooter, 90.);
    }
    tick(&mut w, 1.);
    tick(&mut w, 1.);

    let hull = w.sim.store.vehicle.get(vehicle).unwrap();
    assert!(!hull.alive);
    assert_eq!(w.sim.store.character.get(victim).unwrap().deaths, 1.);
    let c = w.sim.store.character.get_mut(attacker).unwrap();
    let slot = stats::slot_of(c.team);
    let credit = if lethal_hit { 1. } else { 0. };
    assert_eq!((c.kills, stats::record_of(c).slots[slot].vehicles_destroyed), (credit, credit));
    assert_eq!(hull.last_attacker, if lethal_hit { OrdnanceOwner::Unit(shooter) } else { OrdnanceOwner::Null });
    if let Some(events) = events {
        let killer = lethal_hit.then_some(attacker);
        assert_eq!(*events.borrow(), [
            unit_death(&w, rider, victim, killer, lethal_hit.then_some(1.), if lethal_hit { -1. } else { 0. }),
            Event::ObjectKilled { object: ObjectHandle::Vehicle { vehicle, life: 1. }, killer },
        ]);
    }
}

#[test]
fn script_destruction_of_a_scratched_hull_credits_no_character() {
    for kill in [true, false] {
        let mut w = world();
        let (rider, victim) = soldier(&mut w, 2.);
        let (shooter, attacker) = soldier(&mut w, 1.);
        let vehicle = crewed_hull(&mut w, rider);
        let object = ObjectHandle::Vehicle { vehicle, life: 1. };
        prepare_tail(&mut w);
        let events = Rc::new(RefCell::new(Vec::new()));
        let seen = events.clone();
        let hull = object.clone();
        let mut first = true;
        callback(&mut w, 1, move |batch, actions| {
            seen.borrow_mut().extend_from_slice(batch.events());
            if first {
                if kill { actions.kill_object(&hull).unwrap(); } else { actions.set_health(&hull, 0.).unwrap(); }
                first = false;
            }
        });
        scratch_hull(&mut w, vehicle, shooter);
        for _ in 0..3 { tick(&mut w, 1.); }
        assert_eq!(*events.borrow(), [
            unit_death(&w, rider, victim, None, None, 0.),
            Event::ObjectKilled { object, killer: None },
        ]);
        assert_no_hull_credit(&mut w, vehicle, attacker);
    }
}

#[test]
fn region_destruction_of_a_scratched_hull_credits_no_character() {
    check_region_credit(true);
}

#[test]
fn conquest_keeps_its_region_credit_on_a_scratched_hull() {
    check_region_credit(false);
}

fn check_region_credit(event_driven: bool) {
    let mut w = world();
    let (rider, victim) = soldier(&mut w, 2.);
    let (shooter, attacker) = soldier(&mut w, 1.);
    if let UnitBody::Soldier(body) = &mut w.sim.store.unit.get_mut(shooter).unwrap().soldier {
        body.position = [100., 0., 0.];
    }
    let vehicle = crewed_hull(&mut w, rider);
    let (building, objective) = prop(&mut w, false);
    let post = w.sim.store.post.insert(Post {
        active: true, building: Some(building), team: 2.,
        kill_region: crate::store::Nullable::Value(crate::battle::regions::region_desc(RegionType::Sphere, None, None, [5.,0.,0.])),
        ..Default::default()
    });
    w.sim.store.command_posts.posts.push(post);
    prepare_tail(&mut w);
    let events = event_driven.then(|| record(&mut w));
    scratch_hull(&mut w, vehicle, shooter);
    f3::with_services_in(&mut w, |w, s| {
        crate::units::f3_props::command_hit_in(w, 0, 0., None, None, s.unwrap());
    });
    for _ in 0..2 { tick(&mut w, 1.); }
    let Some(events) = events else {
        let v = w.sim.store.vehicle.get(vehicle).unwrap();
        assert!(!v.alive);
        assert_eq!(v.last_attacker, OrdnanceOwner::Unit(shooter));
        let c = w.sim.store.character.get_mut(attacker).unwrap();
        let slot = stats::slot_of(c.team);
        assert_eq!(stats::record_of(c).slots[slot].vehicles_destroyed, 1.);
        return;
    };
    assert_eq!(*events.borrow(), [
        Event::ObjectKilled { object: ObjectHandle::Prop(objective), killer: None },
        Event::PostLost { post, from: 2. },
        unit_death(&w, rider, victim, None, None, 0.),
        Event::ObjectKilled { object: ObjectHandle::Vehicle { vehicle, life: 1. }, killer: None },
    ]);
    assert_no_hull_credit(&mut w, vehicle, attacker);
}

#[test]
fn vehicle_triggered_trap_cannot_credit_a_nonlethal_hit() { vehicle_triggered_trap(false); }

#[test]
fn vehicle_triggered_trap_cannot_credit_the_previous_life() { vehicle_triggered_trap(true); }

fn vehicle_triggered_trap(rebuild: bool) {
    let mut w = world();
    let (rider, _) = soldier(&mut w, 2.);
    let (shooter, attacker) = soldier(&mut w, 1.);
    let vehicle = crewed_hull(&mut w, rider);
    w.sim.store.vehicle.get_mut(vehicle).unwrap().matrix = three::Matrix4::default().elements;
    let (_, trap) = prop(&mut w, true);
    prepare_tail(&mut w);
    let side = w.sim.strings.intern_str("test driver side");
    w.sim.calls.result(F4HullClass::WORDS, |r,h| F4HullClass {
        hull: vehicle, side: Some(side), ..Default::default()
    }.put(r,h));
    f4_live::f4_hull_class_in(&mut w);
    f4::live_in(&mut w.battle).traps.command(&trap, |t| {
        t.state.phase = crate::battle::traps::Phase::Armed;
        t.settings.trigger_team = Some(side);
        t.settings.min_radius = -1.;
        t.settings.reach = 10.;
        t.settings.trigger_frames = Some(1.);
    });
    let events = record(&mut w);
    w.tick.running = true;
    f3::with_services_in(&mut w, |w, s| crate::units::f3_props::command_hit_in(
        w, 0, if rebuild { 100. } else { 10. }, Some(Attacker::Unit(shooter)), None, s.unwrap()));
    if rebuild {
        live_step(&mut w, 1.); live_step(&mut w, 2.);
        assert!(!f4::trap_in(&w.battle, &trap).unwrap().built);
        f3::with_services_in(&mut w, |w, s| crate::units::f3_props::command_hit_in(w, 0, 100., None, None, s.unwrap()));
    }
    let current = f4::live_in(&mut w.battle).traps.current(&trap).unwrap();
    assert_eq!(current.life, if rebuild { 2. } else { 1. });
    for n in 3..=7 { live_step(&mut w, n as f64); }
    assert!(!f4::trap_in(&w.battle, &current).unwrap().built);
    tick(&mut w, 1.);
    let mut expected = Vec::new();
    if rebuild { expected.push(Event::ObjectKilled { object: ObjectHandle::Prop(trap), killer: Some(attacker) }); }
    expected.push(Event::ObjectKilled { object: ObjectHandle::Prop(current), killer: None });
    assert_eq!(*events.borrow(), expected);
}

#[test]
fn manned_turret_kill_reports_the_mount_before_its_gunner() {
    let mut w = world();
    let (gunner, victim) = soldier(&mut w, 1.);
    let (shooter, attacker) = soldier(&mut w, 2.);
    let turret = Ref::new(0, 1);
    let object = configured_prop(&mut w, false, |p| {
        p.target = RepairTarget::Turret(turret);
        p.settings.kind = PbKind::Turret;
        p.live.settings = p.settings.clone();
    }).1;
    w.sim.calls.result(F2TurretBirth::WORDS, |r,h| F2TurretBirth {
        turret: PcTurret { turret, alive: true, ..Default::default() }, ..Default::default()
    }.put(r,h));
    live_seats::f2_turret_adopt_in(&mut w);
    // Install an occupied native lease. Destruction traverses the actual crew solver.
    let lease = w.sim.store.seat_lease.insert(SeatLease {
        turret: Some(turret), unit: Some(gunner), team: 1., ..Default::default()
    });
    w.sim.store.turret_seats.leases.push(lease);
    w.sim.store.unit.get_mut(gunner).unwrap().seat = Some(lease);
    let mount = &mut w.fleet.seats_owner().crews.turrets[0];
    mount.lease = Some(lease);
    mount.gunner = Some(gunner.slot);
    prepare_tail(&mut w);
    let events = record(&mut w);
    w.tick.running = true;
    f3::with_services_in(&mut w, |w, s| crate::units::f3_props::command_hit_in(
        w, 0, 200., Some(Attacker::Unit(shooter)), None, s.unwrap()));
    for _ in 0..2 { tick(&mut w, 1.); }
    assert_eq!(*events.borrow(), [
        Event::ObjectKilled { object: ObjectHandle::Prop(object), killer: Some(attacker) },
        unit_death(&w, gunner, victim, Some(attacker), Some(2.), -1.),
    ]);
}
