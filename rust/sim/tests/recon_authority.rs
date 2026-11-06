//! Whole authority phase golden fixture. Geometry and definitions are
//! synthetic, so CI exercises launches and detached strikes without game files.
use bfe_sim::{
    combat::defs::{Js, JsObj, WeaponClass},
    gen::{F3UnitKey, OwnerSnapshot, Store, Unit},
    recon::*,
    store::{DefId, Ref},
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::rc::Rc;
use three::{Triangle, Vector3};

struct Allocator;
thread_local! { static TRACK: Cell<bool> = const { Cell::new(false) }; static ALLOCATIONS: Cell<usize> = const { Cell::new(0) }; }
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        TRACK.with(|on| {
            if on.get() {
                ALLOCATIONS.with(|count| count.set(count.get() + 1));
            }
        });
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        System.dealloc(pointer, layout);
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        TRACK.with(|on| {
            if on.get() {
                ALLOCATIONS.with(|count| count.set(count.get() + 1));
            }
        });
        System.realloc(pointer, layout, size)
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

#[derive(Default)]
struct WorldFixture {
    events: Vec<f64>,
    water: bool,
    blocked: bool,
    triangles: bool,
    nested: bool,
}
impl World for WorldFixture {
    fn cast(&mut self, p: Vector3, dir: Vector3, far: f64) -> Option<Hit> {
        if dir.y >= 0.0 {
            return None;
        }
        let distance = -p.y / dir.y;
        if distance < 0.0 || distance > far {
            return None;
        }
        let mut point = p;
        point.add_scaled_vector(&dir, distance);
        Some(Hit {
            distance,
            point,
            normal: Vector3::new(0.0, 1.0, 0.0),
        })
    }
    fn static_cast(&mut self, p: Vector3, d: Vector3, far: f64) -> Option<Hit> {
        if self.blocked {
            Some(Hit {
                distance: 0.0,
                point: p,
                normal: Vector3::new(0.0, 1.0, 0.0),
            })
        } else {
            self.cast(p, d, far)
        }
    }
    fn gather(&mut self, _: Vector3, _: f64, out: &mut Vec<Triangle>) {
        if self.triangles {
            out.push(Triangle::new(
                Vector3::new(-100.0, 0.0, -100.0),
                Vector3::new(100.0, 0.0, -100.0),
                Vector3::new(0.0, 0.0, 100.0),
            ));
        }
    }
    fn water(&self, _: f64, _: f64) -> Option<f64> {
        self.water.then_some(1.0)
    }
    fn capture_owner(&mut self, store: &mut Store, owner: &F3UnitKey) -> Ref<OwnerSnapshot> {
        bfe_sim::combat::owners::capture(store,owner).unwrap()
    }
    fn fire(
        &mut self,
        _: &mut Authority,
        store: &mut Store,
        def: u32,
        at: Vector3,
        dir: Vector3,
        owner: Ref<OwnerSnapshot>,
    ) {
        let owner = store.owner_snapshot.get(owner).unwrap();
        self.events.extend([
            1.0,
            def as f64,
            at.x,
            at.y,
            at.z,
            dir.x,
            dir.y,
            dir.z,
            owner.id,
            owner.generation,
        ]);
    }
    fn fired(
        &mut self,
        droid: Ref<bfe_sim::gen::ReconDroid>,
        shot: bfe_sim::combat::weapons::Shot,
        at: Vector3,
    ) {
        self.events
            .extend([3.0, droid.slot as f64, shot.weapon as f64, at.x, at.y, at.z]);
    }
    fn explode(
        &mut self,
        authority: &mut Authority,
        store: &mut Store,
        def: u32,
        at: Vector3,
        owner: Ref<OwnerSnapshot>,
    ) {
        let owner = store.owner_snapshot.get(owner).unwrap();
        self.events.extend([
            2.0,
            def as f64,
            at.x,
            at.y,
            at.z,
            owner.id,
            owner.generation,
        ]);
        if self.nested {
            authority.blast(
                store,
                self,
                Blast {
                    damage: 1000.0,
                    inner: 20.0,
                    outer: 30.0,
                    droid_scale: 1.0,
                    ..Default::default()
                },
                at,
            );
        }
    }
}
fn owner(life: f64) -> F3UnitKey {
    F3UnitKey {
        slot: 7,
        generation: 1,
        life,
    }
}
fn definitions() -> Definitions {
    let mut defs = Definitions::default();
    for channel in 0..2 {
        let values = [
            ("channel", channel as f64),
            ("RoundsPerClip", 8.0),
            ("ammo", 5.0),
            ("ReloadTime", 0.23),
            ("ShotDelay", 0.1),
            ("InitialSalvoDelay", 0.0),
            ("SalvoCount", if channel == 1 { 3.0 } else { 1.0 }),
            ("ShotsPerSalvo", 1.0),
            ("SalvoDelay", if channel == 1 { 0.25 } else { 0.0 }),
            ("YawSpread", 1.7),
            ("PitchSpread", 1.3),
            ("StandStillSpread", 0.4),
            ("SpreadPerShot", 0.2),
            ("SpreadRecoverRate", 0.4),
            ("SpreadLimit", 2.1),
            ("ZoomMin", 2.0),
            ("ZoomMax", 4.0),
        ];
        let mut fields = vec![("base".into(), Js::Str("cannon".into()))];
        fields.extend(values.into_iter().map(|(k, v)| (k.into(), Js::Num(v))));
        if channel == 1 {
            fields.push(("MaxPressedTime".into(), Js::Num(0.1)));
        }
        defs.weapons.set(
            DefId(channel),
            WeaponClass::new(Rc::new(JsObj { fields, def: None })),
        );
        defs.levels.push(vec![WeaponLevel {
            ordnance: if channel == 0 { Some(1) } else { None },
            strike: if channel == 1 { Some(0) } else { None },
            pressed_time: 0.1,
            zoom_min: 2.0,
            zoom_max: 4.0,
        }]);
    }
    defs.droids.push(DroidDef {
        health: 400.0,
        add_health: -1.0,
        explosion: Some(3),
        loadout: vec![DefId(0), DefId(1)],
        ..Default::default()
    });
    defs.strikes.push(StrikeDef {
        ordnance: 2,
        elevation: 83.0,
        azimuth: 17.0,
        count: 3.0,
        delay: 0.25,
        scatter: 3.0,
        distance: 100.0,
    });
    defs
}
fn snapshot(auth: &Authority, store: &Store, world: &mut WorldFixture, slots: usize) -> Vec<f64> {
    let mut out = vec![
        auth.contexts().len() as f64,
        auth.contexts()
            .iter()
            .filter(|r| !store.recon_context.get(**r).unwrap().retired)
            .count() as f64,
        slots as f64,
    ];
    for r in auth.contexts() {
        let c = store.recon_context.get(*r).unwrap();
        out.extend([
            9.0,
            c.owner.life,
            c.retired as u8 as f64,
            c.last_tick,
            c.seed as f64,
            c.piloted.map_or(-1.0, |r| r.slot as f64),
            c.zoom as u8 as f64,
            c.look_x,
            c.look_y,
            c.droids.len() as f64,
            c.strikes.len() as f64,
        ]);
        if c.piloted.is_some() {
            out.push(1.0);
            out.extend(c.camera.position);
            out.extend(c.camera.rotation);
            out.extend([c.camera.reach, c.camera.fresh as u8 as f64]);
        } else {
            out.push(0.0);
        }
        for r in &c.droids {
            let d = store.recon_droid.get(*r).unwrap();
            out.extend([r.slot as f64, d.owner.life, d.dead as u8 as f64, d.health]);
            out.extend(d.position);
            out.extend(d.velocity);
            out.extend([d.yaw, d.pitch]);
            out.extend(d.eye);
            out.extend(d.aim);
            out.extend(d.target);
            out.push(d.countdown);
            if let Some(s) = &d.salvo {
                out.push(1.0);
                out.extend(s.at);
                out.extend(s.x);
                out.extend(s.y);
                out.extend(s.z);
                out.extend([s.left, s.wait]);
            } else {
                out.push(0.0);
            }
            for w in &d.weapons {
                out.extend([
                    w.level,
                    w.timer,
                    w.clip,
                    w.reserve,
                    w.heat,
                    w.floor,
                    w.kick,
                    w.wait,
                    w.reload_left,
                    w.throw_time,
                    w.charge,
                    w.limit,
                    w.salvos_left,
                    w.salvo_wait,
                    w.salvo_seed.unwrap_or(0.0),
                    w.salvo_count,
                    w.pattern,
                    w.deployed as u8 as f64,
                    w.firing as u8 as f64,
                    w.bottomless as u8 as f64,
                    w.per_shot,
                    w.overheated as u8 as f64,
                    w.spread_scale,
                    w.sniper_scale,
                    w.yaw_spread_override.unwrap_or(0.0),
                    w.released as u8 as f64,
                    w.hidden as u8 as f64,
                    w.charging as u8 as f64,
                    w.held,
                    w.lock_target_id,
                    w.lock_generation,
                    w.lock_elapsed,
                    w.lock_complete as u8 as f64,
                ]);
            }
        }
    }
    out.push(world.events.len() as f64);
    out.append(&mut world.events);
    out
}
#[test]
fn complete_authority_phases_match_golden_results() {
    for mode in ["ground", "water", "crash", "triangles"] {
        replay(mode);
    }
}
fn replay(mode: &str) {
    let transcript = include_str!("fixtures/recon-authority-golden.txt");
    let lines: Vec<_> = transcript.lines().filter(|line| line.starts_with(mode)).collect();
    let mut store = Store::default();
    store.unit.insert_at(
        7,
        1,
        Unit {
            generation: 1.0,
            alive: true,
            ..Default::default()
        },
    );
    let mut auth = Authority::new(definitions(), 1, 2);
    let mut world = WorldFixture {
        water: mode == "water",
        triangles: mode == "triangles",
        ..Default::default()
    };
    let mut context = auth.attach(&mut store, 9, owner(1.0), 123).unwrap();
    let token = auth.reserve(&store).unwrap();
    let first = auth
        .launch(
            &mut store,
            &mut world,
            context,
            token,
            0,
            0,
            Vector3::new(1.0, 2.0, 3.0),
            Vector3::new(8.0, if mode == "crash" { -12.0 } else { -1.0 }, 2.0),
            0.2,
            0.0,
            true,
        )
        .unwrap();
    let mut slots = 1;
    for (tick, line) in lines.iter().enumerate() {
        if tick == 20 {
            store.recon_context.get_mut(context).unwrap().first_person = true;
        }
        if tick == 30 {
            store.recon_context.get_mut(context).unwrap().first_person = false;
        }
        if tick == 110 {
            store.unit.get_mut(Ref::new(7, 1)).unwrap().alive = false;
        }
        if tick == 112 {
            let u = store.unit.get_mut(Ref::new(7, 1)).unwrap();
            u.generation = 2.0;
            u.alive = true;
            context = auth.rebind(&mut store, 9, owner(2.0), 999).unwrap();
        }
        if tick == 114 {
            auth.hurt(&mut store, &mut world, first, 1000.0);
        }
        if tick == 115 {
            let token = auth.reserve(&store).unwrap();
            slots = 2;
            auth.launch(
                &mut store,
                &mut world,
                context,
                token,
                0,
                0,
                Vector3::new(-1.0, 3.0, 1.0),
                Vector3::default(),
                -0.4,
                0.0,
                true,
            )
            .unwrap();
        }
        if tick == 130 {
            auth.retire(&mut store, 9);
        }
        if tick == 140 {
            auth.blast(
                &mut store,
                &mut world,
                Blast {
                    damage: 500.0,
                    droid_scale: 1.0,
                    inner: 20.0,
                    outer: 30.0,
                    push: 3.0,
                    push_inner: 20.0,
                    push_outer: 30.0,
                },
                Vector3::default(),
            );
        }
        if tick < 110 {
            auth.look(&mut store, context, 0.5, -0.2, 0.7, false);
        }
        let input = Input {
            forward: if tick < 80 { 0.7 } else { 0.0 },
            right: 0.1,
            turn: 0.2,
            primary: tick % 13 == 0,
            held: tick % 17 < 4,
            secondary: tick == 100,
            zoom: tick == 40 || tick == 60,
            ..Default::default()
        };
        auth.step(&mut store, &mut world, 9, tick as f64, 1.0 / 60.0, input)
            .unwrap();
        auth.finish(&mut store, &mut world, tick as f64, 1.0 / 60.0)
            .unwrap();
        let actual = snapshot(&auth, &store, &mut world, slots);
        let fields: Vec<_> = line.split_whitespace().collect();
        assert_eq!(fields[1].parse::<usize>().unwrap(), tick);
        assert_eq!(actual.len(), fields[2].parse::<usize>().unwrap(), "{mode} tick {tick} length");
        let mut hash = 0xcbf29ce484222325_u64;
        for value in actual {
            for byte in value.to_le_bytes() {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            }
        }
        assert_eq!(hash, u64::from_str_radix(fields[3], 16).unwrap(), "{mode} tick {tick}");
    }
    assert_eq!(lines.len(), 240);
}

#[test]
fn admission_keeps_detached_strikes_and_rejects_cancelled_tokens() {
    let mut store = Store::default();
    store.unit.insert_at(
        7,
        1,
        Unit {
            generation: 1.0,
            alive: true,
            ..Default::default()
        },
    );
    let mut auth = Authority::new(definitions(), 1, 1);
    let mut world = WorldFixture::default();
    let c = auth.attach(&mut store, 9, owner(1.0), 123).unwrap();
    assert_eq!(
        auth.attach(&mut store, 10, owner(1.0), 123),
        Err(Error::Capacity)
    );
    let cancelled = auth.reserve(&store).unwrap();
    auth.cancel(cancelled);
    let token = auth.reserve(&store).unwrap();
    auth.cancel(cancelled);
    assert_eq!(
        auth.launch(
            &mut store,
            &mut world,
            c,
            cancelled,
            0,
            0,
            Vector3::default(),
            Vector3::default(),
            0.0,
            0.0,
            false
        ),
        Err(Error::Reservation)
    );
    let d = auth
        .launch(
            &mut store,
            &mut world,
            c,
            token,
            0,
            0,
            Vector3::new(0.0, 2.0, 0.0),
            Vector3::default(),
            0.0,
            0.0,
            true,
        )
        .unwrap();
    assert!(auth.reserve(&store).is_none());
    auth.strike(&mut store, d, 0);
    auth.kill(&mut store, &mut world, d);
    auth.retire(&mut store, 9);
    assert!(auth.reserve(&store).is_none());
    assert_eq!(auth.contexts().len(), 1);
    for tick in 0..60 {
        auth.finish(&mut store, &mut world, tick as f64, 1.0 / 60.0)
            .unwrap();
    }
    assert!(auth.contexts().is_empty());
    let token = auth.reserve(&store).unwrap();
    assert_eq!(token.slot, d.slot);
    assert_eq!(token.generation, d.gen + 1);
}

#[test]
fn configured_player_capacity_controls_owner_admission() {
    let mut store = Store::default();
    let mut auth = Authority::new(definitions(), 64, 2);
    auth.configure_owners(1);
    auth.attach(&mut store, 9, owner(1.0), 123).unwrap();
    assert_eq!(auth.attach(&mut store, 10, owner(1.0), 456), Err(Error::Capacity));
    auth.retire(&mut store, 9);
    assert!(auth.attach(&mut store, 10, owner(1.0), 456).is_ok());
}

#[test]
fn finish_steps_once_and_rebind_rejects_stale_owner_lives() {
    let mut store = Store::default();
    store.unit.insert_at(
        7,
        1,
        Unit {
            generation: 1.0,
            alive: true,
            ..Default::default()
        },
    );
    let mut auth = Authority::new(definitions(), 1, 1);
    let mut world = WorldFixture::default();
    let c = auth.attach(&mut store, 9, owner(1.0), 123).unwrap();
    let t = auth.reserve(&store).unwrap();
    let d = auth
        .launch(
            &mut store,
            &mut world,
            c,
            t,
            0,
            0,
            Vector3::new(0.0, 2.0, 0.0),
            Vector3::default(),
            0.0,
            0.0,
            true,
        )
        .unwrap();
    auth.step(&mut store, &mut world, 9, 1.0, 1.0 / 60.0, Input::default())
        .unwrap();
    let health = store.recon_droid.get(d).unwrap().health;
    auth.finish(&mut store, &mut world, 1.0, 1.0 / 60.0)
        .unwrap();
    assert_eq!(store.recon_droid.get(d).unwrap().health, health);
    assert_eq!(
        auth.step(&mut store, &mut world, 9, 0.0, 1.0 / 60.0, Input::default()),
        Err(Error::Tick)
    );
    store.unit.get_mut(Ref::new(7, 1)).unwrap().generation = 2.0;
    auth.finish(&mut store, &mut world, 2.0, 1.0 / 60.0)
        .unwrap();
    assert!(store.recon_context.get(c).unwrap().piloted.is_none());
    auth.rebind(&mut store, 9, owner(2.0), 999).unwrap();
    assert_eq!(store.recon_context.get(c).unwrap().seed, 123);
    assert_eq!(store.recon_droid.get(d).unwrap().owner.life, 1.0);
    // Rebinding the context cannot rewrite the old droid's fire/blast owner.
    let launch = store.recon_droid.get(d).unwrap().launch_owner;
    assert_eq!(store.owner_snapshot.get(launch).unwrap().generation,1.0);
    assert_eq!(bfe_sim::combat::owners::resolve(&store,launch),None);
    assert!(auth.of(&store, c, 0).is_none());
}

#[test]
fn blasts_commit_death_before_depth_first_explosions_and_respect_static_cover() {
    let mut store = Store::default();
    store.unit.insert_at(
        7,
        1,
        Unit {
            generation: 1.0,
            alive: true,
            ..Default::default()
        },
    );
    let mut auth = Authority::new(definitions(), 1, 2);
    let mut world = WorldFixture {
        nested: true,
        blocked: true,
        ..Default::default()
    };
    let c = auth.attach(&mut store, 9, owner(1.0), 123).unwrap();
    let mut droids = Vec::new();
    for x in [1.0, 2.0] {
        let t = auth.reserve(&store).unwrap();
        droids.push(
            auth.launch(
                &mut store,
                &mut world,
                c,
                t,
                0,
                0,
                Vector3::new(x, 2.0, 0.0),
                Vector3::default(),
                0.0,
                0.0,
                false,
            )
            .unwrap(),
        );
    }
    let blast = Blast {
        damage: 1000.0,
        inner: 20.0,
        outer: 30.0,
        droid_scale: 1.0,
        push: 3.0,
        push_inner: 20.0,
        push_outer: 30.0,
    };
    auth.blast(&mut store, &mut world, blast, Vector3::default());
    assert!(droids
        .iter()
        .all(|r| !store.recon_droid.get(*r).unwrap().dead));
    assert!(world.events.is_empty());
    world.blocked = false;
    auth.blast(&mut store, &mut world, blast, Vector3::default());
    assert!(droids
        .iter()
        .all(|r| store.recon_droid.get(*r).unwrap().dead));
    assert_eq!(world.events.len(), 14);
    assert_eq!(world.events[2], 1.0);
    assert_eq!(world.events[9], 2.0);
}

#[test]
fn warmed_authority_flight_weapons_and_finish_allocate_nothing() {
    let mut store = Store::default();
    store.unit.insert_at(
        7,
        1,
        Unit {
            generation: 1.0,
            alive: true,
            ..Default::default()
        },
    );
    let mut auth = Authority::new(definitions(), 1, 1);
    let mut world = WorldFixture {
        events: Vec::with_capacity(30000),
        ..Default::default()
    };
    let c = auth.attach(&mut store, 9, owner(1.0), 123).unwrap();
    let t = auth.reserve(&store).unwrap();
    auth.launch(
        &mut store,
        &mut world,
        c,
        t,
        0,
        0,
        Vector3::new(1.0, 2.0, 3.0),
        Vector3::default(),
        0.2,
        0.0,
        true,
    )
    .unwrap();
    auth.step(&mut store, &mut world, 9, 0.0, 1.0 / 60.0, Input::default())
        .unwrap();
    ALLOCATIONS.with(|n| n.set(0));
    TRACK.with(|t| t.set(true));
    for tick in 1..=1200 {
        auth.step(
            &mut store,
            &mut world,
            9,
            tick as f64,
            1.0 / 60.0,
            Input {
                held: true,
                forward: 0.5,
                ..Default::default()
            },
        )
        .unwrap();
        auth.finish(&mut store, &mut world, tick as f64, 1.0 / 60.0)
            .unwrap();
    }
    TRACK.with(|t| t.set(false));
    assert_eq!(ALLOCATIONS.with(|n| n.get()), 0);
}

#[test]
fn first_launch_and_repeated_slot_reuse_allocate_nothing_after_world_preparation() {
    let mut store = Store::default();
    store.unit.insert_at(7,1,Unit { generation:1.0,alive:true,..Default::default() });
    // The coordinator reserves generated slabs alongside the authority's
    // content-sized weapon buffers, before any remote shot can be admitted.
    store.recon_droid.reserve(1);
    store.owner_snapshot.reserve(1200);
    let mut auth = Authority::new(definitions(),1,1);
    let context = auth.attach(&mut store,9,owner(1.0),123).unwrap();
    let mut world = WorldFixture { events:Vec::with_capacity(30000),..Default::default() };
    auth.clear_changes();
    ALLOCATIONS.with(|n| n.set(0));
    TRACK.with(|on| on.set(true));
    for tick in 0..1200 {
        let token = auth.reserve(&store).unwrap();
        let droid = auth.launch(
            &mut store,&mut world,context,token,0,0,
            Vector3::new(1.0,2.0,3.0),Vector3::default(),0.2,0.0,true,
        ).unwrap();
        auth.kill(&mut store,&mut world,droid);
        auth.finish(&mut store,&mut world,tick as f64,1.0/60.0).unwrap();
        auth.clear_changes();
    }
    TRACK.with(|on| on.set(false));
    assert_eq!(ALLOCATIONS.with(|n| n.get()),0);
    assert_eq!(store.recon_droid.len(),1);
    assert_eq!(store.owner_snapshot.len(),1200);
}
