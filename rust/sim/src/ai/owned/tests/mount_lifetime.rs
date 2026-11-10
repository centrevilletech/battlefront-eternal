//! Retained leases remain observable after their short-lived AI control retires.
use super::phase::{ALLOCATIONS, COUNTING};
use crate::{
    abi::context::Context,
    ai::owned::entities::Entity,
    gen::{WorldClockState, WorldTickInput},
    host::Capture,
    world::admit::Options,
};
use bfe_content::files::DirFiles;
use std::cell::Cell;

#[test]
fn released_mount_observation_after_real_bot_exit() {
    let Some(content) = std::env::var_os("BFE_CONTENT_DIR") else {
        return;
    };
    // Keep cold admission on a thread with an explicit stack budget so this
    // regression does not depend on platform test-thread defaults.
    std::thread::Builder::new()
        .stack_size(8 << 20)
        .spawn(move || {
            let root = std::path::PathBuf::from(content);
            let options = Options {
                map: "nab2".into(),
                side: "rep".into(),
                enhanced: false,
                seed: 1,
                heroes: false,
                unit: None,
                name: None,
            };
            let context = Context::new();
            let admitted = context
                .world_admit(&DirFiles::new(&root), &options)
                .unwrap();
            context.world_step_init(admitted.setup).unwrap();
            let h = admitted.handles;
            let (explosions, content) =
                crate::host::content_tables(&context, &root, &options, h.store_source).unwrap();
            context
                .world_host_start(
                    Some(Capture {
                        combat: h.combat,
                        presentation: h.presentation,
                        store_source: h.store_source,
                        explosions,
                    }),
                    content,
                )
                .unwrap();
            context.world_host_client(h.player, h.edges).unwrap();
            let mut input = WorldTickInput {
                clock: WorldClockState {
                    hz: 60.,
                    dt: 1. / 60.,
                    max_steps: 8.,
                    budget: 0.035,
                    max_debt: 8.,
                    ..Default::default()
                },
                ai: true,
                live: true,
                mouse_scale: 1.,
                ..Default::default()
            };
            for tick in 1..=3600 {
                input.tick = tick as f64;
                input.dt = 1. / 60.;
                input.at = 1000. + tick as f64 * 1000. / 60.;
                input.units = context.world_prepare_units().unwrap();
                let output = context.world_tick(input.clone()).unwrap();
                assert!(output.accepted);
                assert_eq!(output.clock.tick, tick as f64);
                input.clock = output.clock;
                let observed = context
                    .owned(|world| {
                        let state = &world.ai.battle().unwrap().state;
                        let store = &world.sim.store;
                        for slot in 0..store.seat_lease.slots() {
                            let Some((reference, lease)) = store.seat_lease.at(slot) else {
                                continue;
                            };
                            let retired = lease.controllable.is_some_and(|control| {
                                state.entities.find(Entity::Control(control)).is_none()
                            });
                            if !lease.released || !retired {
                                continue;
                            }
                            // VehicleAI can observe a captured lease after a
                            // real bot exit. Its released gate must remain
                            // readable without resurrecting the old control.
                            let view = state
                                .mounts
                                .lease(store, &state.entities, reference)
                                .unwrap();
                            assert!(view.released);
                            assert!(view.controllable.is_none());
                            assert_eq!(
                                view.vehicle,
                                lease
                                    .vehicle
                                    .and_then(|hull| state.entities.find(Entity::Hull(hull)))
                            );
                            assert_eq!(
                                view.turret,
                                lease
                                    .turret
                                    .and_then(|turret| state.entities.find(Entity::Turret(turret)))
                            );
                            return true;
                        }
                        false
                    })
                    .unwrap();
                if observed {
                    eprintln!("observed a released bot mount at tick {tick}");
                    context.dispose().unwrap();
                    return;
                }
            }
            panic!("no real bot exit with a retired control within 60 seconds");
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn released_mount_lease_does_not_resolve_a_retired_control() {
    use bfe_sim::{
        ai::owned::{
            entities::{Entities, Entity},
            mounts::Mounts,
        },
        gen::*,
        store::Ref,
    };
    let mut store = crate::ai::owned::tests::empty_world();
    let mut entities = Entities::new(16);
    let unit = store.unit.insert(Unit {
        alive: true,
        generation: 1.,
        ..Default::default()
    });
    let hull = store.vehicle.insert(Vehicle::default());
    let control = Ref::new(0, 1);
    let lease = store.seat_lease.insert(SeatLease {
        unit: Some(unit),
        generation: 1.,
        vehicle: Some(hull),
        controllable: Some(control),
        ..Default::default()
    });
    store.unit.get_mut(unit).unwrap().seat = Some(lease);
    let rider = entities.register(Entity::Unit(unit));
    let holder = entities.register(Entity::Hull(hull));
    entities.register(Entity::Lease(lease));
    let old = entities.register(Entity::Control(control));
    let mounts = Mounts::new(16, 3);
    let live = mounts.lease(&store, &entities, lease).unwrap();
    assert!(live.live);
    assert_eq!(live.controllable, Some(old));

    // Core cleanup can still read the bound control after physical release.
    store.seat_lease.get_mut(lease).unwrap().released = true;
    assert_eq!(
        mounts.lease(&store, &entities, lease).unwrap().controllable,
        Some(old)
    );
    assert!(entities.retire(old));
    let replacement = entities.register(Entity::Control(Ref::new(control.slot, control.gen + 1)));
    assert_ne!(old, replacement);

    // A retained lease must neither panic nor acquire the replacement's
    // control. Keep geometry for VehicleAI.mountReach's own-lease fallback.
    ALLOCATIONS.with(|count| count.set(0));
    COUNTING.with(|active| active.set(true));
    for _ in 0..120 {
        let view = mounts.lease(&store, &entities, lease).unwrap();
        assert!(view.released);
        assert!(!view.live);
        assert_eq!(view.controllable, None);
        assert_eq!(view.unit, Some(rider));
        assert_eq!(view.vehicle, Some(holder));
    }
    COUNTING.with(|active| active.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
}

#[test]
fn unreleased_lease_owns_an_inactive_control_until_release_or_replacement() {
    use crate::ai::owned::{
        seats::Occupant,
        state::{Capacity, State},
    };
    use crate::gen::*;
    // SeatUnit.alive can be false before boarding commits Unit.seat, during
    // holder/rider death, or while promotion changes the index. The browser's
    // SeatLease still owns its controllable across each synchronous boundary.
    for transition in 0..4 {
        let mut world = super::empty_world();
        let mut state = State::new(
            crate::ai::nav::Nav::empty(Default::default()),
            false,
            Capacity {
                registry: 8,
                characters: 1,
                posts: 0,
                hulls: 1,
                buildings: 0,
                controls: 2,
                scopes: 2,
                cameras: 0,
            },
        );
        let rider = world.sim.store.unit.insert(Unit {
            alive: true,
            generation: 1.,
            ..Default::default()
        });
        let hull = world.sim.store.vehicle.insert(Vehicle {
            alive: true,
            target_generation: 1.,
            seats: vec![Default::default(), Default::default()],
            ..Default::default()
        });
        let lease = world.sim.store.seat_lease.insert(SeatLease {
            unit: Some(rider),
            generation: 1.,
            vehicle: Some(hull),
            ..Default::default()
        });
        world.sim.store.vehicle.get_mut(hull).unwrap().seats[0].lease = Some(lease);
        world.sim.store.unit.get_mut(rider).unwrap().seat = Some(lease);
        state.entities.register(Entity::Unit(rider));
        state.entities.register(Entity::Hull(hull));
        let control = state
            .births
            .construct(
                &world.sim.store,
                world.fleet.seats_owner(),
                &mut state.entities,
                lease,
                None,
            )
            .unwrap();
        world
            .sim
            .store
            .seat_lease
            .get_mut(lease)
            .unwrap()
            .controllable = Some(control);
        assert!(state.controls.register(
            &world.sim.store,
            &mut state.entities,
            control,
            lease,
            AiLifeHandle {
                kind: KIND_VEHICLE.into(),
                slot: hull.slot,
                generation: hull.gen,
                life: 1.
            },
            5.,
            Default::default(),
            "craft".into(),
            None,
            Occupant {
                id: rider.slot as f64,
                hero: false
            }
        ));
        let identity = state.entities.find(Entity::Control(control)).unwrap();
        match transition {
            0 => world.sim.store.unit.get_mut(rider).unwrap().seat = None,
            1 => world.sim.store.vehicle.get_mut(hull).unwrap().alive = false,
            2 => world.sim.store.unit.get_mut(rider).unwrap().alive = false,
            _ => world.sim.store.seat_lease.get_mut(lease).unwrap().index = 1.,
        }
        let mut publication = std::mem::take(&mut state.control_views);
        // Warm publication before counting; repeated observation must neither
        // allocate nor resurrect a control under a different qualified token.
        publication.publish(
            &mut state,
            &world.sim.store,
            world.fleet.seats_owner(),
            &crate::battle::f4::live_ref(&world.battle).buildings,
        );
        ALLOCATIONS.with(|count| count.set(0));
        COUNTING.with(|active| active.set(true));
        for _ in 0..120 {
            publication.publish(
                &mut state,
                &world.sim.store,
                world.fleet.seats_owner(),
                &crate::battle::f4::live_ref(&world.battle).buildings,
            );
            assert_eq!(
                state.entities.find(Entity::Control(control)),
                Some(identity),
                "transition {transition}"
            );
            assert_eq!(
                state
                    .mounts
                    .lease(&world.sim.store, &state.entities, lease)
                    .unwrap()
                    .controllable,
                Some(identity)
            );
            assert!(!state
                .controls
                .unit(&world.sim.store, &state.entities, control)
                .is_some_and(|view| view.alive));
        }
        COUNTING.with(|active| active.set(false));
        assert_eq!(ALLOCATIONS.with(Cell::get), 0);
        // Release or replacement removes the lease's ownership. With no Core
        // binding, the old pool entry can then retire and reuse its storage.
        if transition % 2 == 0 {
            world.sim.store.seat_lease.get_mut(lease).unwrap().released = true;
        } else {
            world
                .sim
                .store
                .seat_lease
                .get_mut(lease)
                .unwrap()
                .controllable = None;
        }
        publication.publish(
            &mut state,
            &world.sim.store,
            world.fleet.seats_owner(),
            &crate::battle::f4::live_ref(&world.battle).buildings,
        );
        assert!(state.entities.find(Entity::Control(control)).is_none());
        assert!(state.births.read(control).is_none());
    }
}
