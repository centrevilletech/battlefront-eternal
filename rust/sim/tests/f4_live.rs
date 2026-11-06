//! Whole native live phase fixtures, without models or retail game content.
use bfe_sim::battle::{animated, live_phase::{self, Building, Live, SupplyTargets, Trap}, props, traps};
use bfe_sim::gen::{Attacker, F4ObjectKind, Vehicle};
use bfe_sim::store::{Ref, StrId, Strings};
use three::{Matrix4, Quaternion, Vector3};
#[derive(Default)]
struct World { events: Vec<&'static str>, alive: [bool; 2], supplied: f64 }
impl props::World for World {
    fn show(&mut self, _: &props::Building) { self.events.push("show"); }
    fn crew_destroyed(&mut self, _: &mut props::Building) { self.events.push("crew"); }
    fn destroyed(&mut self, _: &mut props::Building) { self.events.push("destroyed"); }
    fn explode(&mut self, _: &mut props::Building, _: Vector3) { self.events.push("explode"); }
    fn damaged(&mut self, _: &mut props::Building, _: f64, _: props::Identity, _: props::Identity) { self.events.push("damaged"); }
    fn served(&mut self, _: &props::Station, _: &mut props::Target, _: u8) { self.events.push("served"); }
    fn serve_sound(&mut self, _: &props::Station, _: Option<usize>) { self.events.push("supplySound"); }
    fn target_changed(&mut self, t: &props::Target) { self.supplied = t.health; self.events.push("target"); }
}
impl traps::World for World {
    type Vehicle = Ref<Vehicle>;
    fn alive(&mut self, v: Self::Vehicle) -> bool { self.alive[v.slot as usize] }
    fn sphere(&mut self, _: Self::Vehicle) -> ([f64; 3], f64) { ([0.0; 3], 2.0) }
    fn side(&mut self, _: Self::Vehicle) -> Option<StrId> { None }
    fn clear_attacker(&mut self, _: &mut traps::Trap, _: Self::Vehicle) { self.events.push("clearAttacker"); }
    fn set_health(&mut self, _: &mut traps::Trap, v: Self::Vehicle, _: f64) {
        self.events.push("crush"); self.alive[v.slot as usize] = false;
        // First death mutates the next hull: selection must already be frozen.
        self.alive[1] = false;
    }
    fn set_dying(&mut self, _: &mut traps::Trap, _: Self::Vehicle, _: f64) { self.events.push("dying"); }
    fn hide_root(&mut self, _: &mut traps::Trap, _: Self::Vehicle) { self.events.push("hide"); }
    fn show(&mut self, _: &mut traps::Trap, _: bool) { self.events.push("trapShow"); }
    fn sound(&mut self, _: &mut traps::Trap, _: StrId, _: [f64; 3]) { self.events.push("trapSound"); }
    fn effect(&mut self, _: &mut traps::Trap, _: StrId, _: [f64; 3]) { self.events.push("trapEffect"); }
}
impl animated::World for World {
    fn sound(&mut self, _: &animated::Animated, _: StrId, _: Vector3) { self.events.push("animatedSound"); }
    fn let_go(&mut self, _: &animated::Animated, _: &mut animated::Body) {}
    fn damage(&mut self, _: &animated::Animated, _: &mut animated::Body, _: f64) {}
    fn remove(&mut self, _: &animated::Animated, _: &mut animated::Body) {}
    fn rider_root(&mut self, _: &animated::Animated, _: &animated::Body, _: StrId, _: f64) -> Option<(Vector3, Quaternion)> { None }
}
impl live_phase::World for World {
    fn idle_turrets(&mut self, _: f64) { self.events.push("idle"); }
}
fn settings(kind: props::Kind) -> props::Settings {
    props::Settings { kind, max_health: 100.0, respawn_time: 1.0, built_add_health: 0.0,
        regenerates: true, has_destroyed: true, has_unbuilt: true, has_crew: kind == props::Kind::Turret,
        has_explosion: true, has_damaged: false, matrix: Matrix4::default(), models: [None, None, None] }
}
fn state() -> props::Station {
    props::Station { building: props::Building { state: props::State::Built, health: 100.0,
        add_health: 0.0, last_attacker: props::Identity::Null, target_generation: Some(1.0) },
        active_mode: false, active: 0.0, pulse: 0.0, served_player: false, served_health: false, served_ammo: false }
}
fn supply() -> props::Supply {
    props::Supply { position: Vector3::default(), radius: 10.0, delay: 1.0,
        health: 10.0, ammo: 5.0, vehicle_health: 0.0, vehicle_ammo: 0.0 }
}
fn trap() -> Trap {
    Trap::new(traps::Trap { built: true, health: 100.0, phase: traps::Phase::Swing,
        started: Some(0.0), time: 0.0, sprung: true, tick: 0.0, crushed: 0.0 },
        traps::Settings { max_health: 100.0, reach: 10.0, min_radius: 1.0, death_time: 0.5,
        build_frames: Some(30.0), trigger_frames: Some(300.0), trigger_team: None, sound: None,
        death_effect: None, point: [0.0; 3], effect_at: [0.0; 3] }, 2)
}
fn targets() -> Vec<props::Target> {
    vec![props::Target { id: 0, position: Vector3::default(), vehicle: false, alive: true,
        player: true, radius: 1.0, health: 50.0, max_health: 100.0,
        weapons: vec![props::Weapon { bottomless: false, ammo: 10.0, clip: 1.0, per_shot: 1.0, reserve: 2.0 }] }]
}
#[test]
fn station_supply_and_turret_regeneration_precede_snapshot_trap_deaths() {
    let mut live = Live::new(2, 1, 0);
    let station = live.adopt(1.0, Building { attacker: Attacker::Null, state: state(), settings: settings(props::Kind::Building), supply: Some(supply()) });
    let mut turret = state(); turret.building.state = props::State::Destroyed;
    turret.building.health = 50.0; turret.building.add_health = 50.0;
    let turret = live.adopt(1.0, Building { attacker: Attacker::OwnerSnapshot(Ref::new(4, 7)), state: turret, settings: settings(props::Kind::Turret), supply: None });
    let trap = live.traps.adopt(1.0, trap());
    let mut targets = targets(); let mut w = World { alive: [true; 2], ..World::default() };
    let hulls = [Ref::new(0, 1), Ref::new(1, 1)];
    live.step(1.0, 60.0, &mut SupplyTargets { targets: &mut targets, soldiers: &[0], vehicles: &[] },
        &[], &mut [], &[], &Strings::default(), &hulls, &mut w);
    assert_eq!(targets[0].health, 60.0); assert_eq!(targets[0].weapons[0].reserve, 7.0);
    assert_eq!(w.events, ["target", "served", "supplySound", "show", "idle",
        "clearAttacker", "crush", "dying", "hide", "clearAttacker", "crush", "dying", "hide"]);
    assert!(live.buildings.get(&station).unwrap().state.served_player);
    let rebuilt = live.buildings.current(&turret).unwrap();
    assert_eq!(rebuilt.life, turret.life + 1.0);
    assert!(live.buildings.get(&turret).is_none());
    assert!(live.buildings.get(&rebuilt).unwrap().state.building.alive());
    assert_eq!(live.buildings.get(&rebuilt).unwrap().attacker, Attacker::Null);
    assert_eq!(live.traps.get(&trap).unwrap().state.crushed, 2.0);
}
#[test]
fn destroyed_objective_does_not_regenerate_and_old_kind_commands_are_rejected() {
    let mut live = Live::new(1, 0, 0); let mut s = state(); s.building.add_health = 10.0;
    let h = live.adopt(1.0, Building { attacker: Attacker::Null, state: s, settings: settings(props::Kind::Objective), supply: None });
    let mut w = World::default();
    assert!(live.damage(&h, 200.0, Attacker::Null, props::Identity::Null, &mut w));
    assert_eq!(w.events, ["destroyed", "show", "explode"]);
    assert_eq!(live.buildings.get(&h).unwrap().state.building.add_health, 0.0);
    let mut wrong = h.clone(); wrong.kind = F4ObjectKind::Trap;
    assert!(!live.health(&wrong, 100.0, &mut w));
}
fn door() -> live_phase::Animated {
    live_phase::Animated {
        state: animated::Animated { index: 0, slot: 0, time: 0.0, weight: 1.0, looping: true,
            hold: 0.0, started: 0.0, start_time: 0.0, victim: None, openness: 0.0,
            body_enabled: Some(true), stand_q: vec![], stand_t: vec![],
            pose: Some(bfe_sim::pose::Pose::new(0, -1)),
            rig: bfe_sim::seats::turrets::TurretRig::new(&Matrix4::default().elements, &[], None), bind: vec![] },
        settings: animated::Settings { door: true, clips: vec![], door_clip: None, door_duration: 1.0,
            idle_delay: 0.0, open_slots: vec![], triggers: vec![animated::Trigger { slot: 0,
                point: Vector3::default(), radius: 5.0, sound: None, opens: true }],
            grabs: vec![], position: Vector3::default(), kill_sound: None },
    }
}
#[test]
fn native_door_opens_collision_and_publishes_only_its_changed_row() {
    let mut live = Live::new(0, 0, 1); let h = live.animated.adopt(1.0, door()); live.flush();
    let mut w = World::default();
    assert_eq!(live.step(0.5, 30.0, &mut SupplyTargets { targets: &mut [], soldiers: &[], vehicles: &[] },
        &[Vector3::default()], &mut [], &[], &Strings::default(), &[], &mut w), 1);
    let d = &live.animated.get(&h).unwrap().state;
    assert_eq!(d.openness, 0.5); assert_eq!(d.body_enabled, Some(false));
    assert_eq!(live.buildings.publication.rows(), 0);
    assert_eq!(live.traps.publication.rows(), 0);
}
#[path = "support/allocations.rs"]
mod allocation;
#[global_allocator]
static ALLOCATOR: allocation::Counted = allocation::Counted;
#[test]
fn warmed_supply_door_and_crush_phase_allocates_nothing() {
    let mut live = Live::new(2, 1, 1);
    live.adopt(1.0, Building { attacker: Attacker::Null, state: state(), settings: settings(props::Kind::Building), supply: Some(supply()) });
    live.adopt(1.0, Building { attacker: Attacker::Null, state: state(), settings: settings(props::Kind::Turret), supply: None });
    let trap = live.traps.adopt(1.0, trap()); live.animated.adopt(1.0, door());
    let mut targets = targets(); let strings = Strings::default(); let hulls = [Ref::new(0, 1), Ref::new(1, 1)];
    let mut w = World { events: Vec::with_capacity(32), alive: [true; 2], supplied: 0.0 };
    live.flush();
    allocation::start();
    for tick in 1..=1200 {
        w.events.clear(); w.alive = [true; 2]; targets[0].health = 50.0;
        live.traps.command(&trap, |t| { t.state.time = 0.0; t.state.built = true; t.state.phase = traps::Phase::Swing; });
        live.step(1.0, tick as f64, &mut SupplyTargets { targets: &mut targets, soldiers: &[0], vehicles: &[] },
            &[Vector3::default()], &mut [], &[], &strings, &hulls, &mut w);
    }
    let requests = allocation::stop(); assert_eq!(requests, 0);
    assert_eq!(live.traps.get(&trap).unwrap().state.crushed, 2400.0);
}

#[test]
fn generated_births_prepare_station_trap_and_door_for_the_owned_phase() {
    use bfe_sim::battle::prepared;
    use bfe_sim::gen::*;
    let birth = F4BuildingBirth {
        settings: PbSettings { kind: PbKind::Building, max_health: 100.0, respawn_time: 1.0,
            regenerates: true, has_unbuilt: true, matrix: Matrix4::default().elements,
            models: vec![None, None, None], ..PbSettings::default() },
        state: (&state()).into(),
        supply: Some(PbSupply { radius: 10.0, delay: 1.0, health: 10.0, ammo: 5.0, ..PbSupply::default() }),
        ..F4BuildingBirth::default()
    };
    let mut row = vec![0.0; F4BuildingBirth::WORDS]; let mut heap = Vec::new(); birth.put(&mut row, &mut heap);
    let station = prepared::building(F4BuildingBirth::take(&row, &heap));
    let trap = prepared::trap(F4TrapBirth { settings: TrSettings { max_health: 100.0, reach: 10.0, min_radius: 1.0,
        death_time: 0.5, trigger_frames: Some(300.0), ..TrSettings::default() },
        state: TrState { built: true, health: 100.0, phase: TrPhase::Swing, sprung: true, ..TrState::default() }, ..F4TrapBirth::default() }, 2);
    let door = prepared::animated(F4AnimatedBirth {
        settings: PaSettings { door: true, door_duration: 1.0, root: Matrix4::default().elements,
            parents: vec![-1.0], bind: vec![PaBind { q: [0.0, 0.0, 0.0, 1.0], p: [1.0, 2.0, 3.0] }],
            triggers: vec![PaTrigger { radius: 5.0, opens: true, ..PaTrigger::default() }], ..PaSettings::default() },
        state: PaState { slot: 0.0, weight: 1.0, r#loop: true, body_enabled: Some(true),
            root_world: Matrix4::default().elements,
            bones: vec![PaBone { p: [1.0, 2.0, 3.0], q: [0.0, 0.0, 0.0, 1.0], s: [1.0; 3], world: Matrix4::default().elements }],
            pose: Some(PaPose { root: -1.0, q: vec![0.0, 0.0, 0.0, 1.0], t: vec![1.0, 2.0, 3.0], w: vec![0.0] }), ..PaState::default() }
    });
    let mut live = Live::new(1, 1, 1); live.adopt(1.0, station); let trap = live.traps.adopt(1.0, trap);
    let door = live.animated.adopt(1.0, door); let mut targets = targets(); let mut w = World { alive: [true; 2], ..World::default() };
    live.step(1.0, 60.0, &mut SupplyTargets { targets: &mut targets, soldiers: &[0], vehicles: &[] },
        &[Vector3::default()], &mut [], &[], &Strings::default(), &[Ref::new(0, 1), Ref::new(1, 1)], &mut w);
    assert_eq!(targets[0].health, 60.0); assert_eq!(live.traps.get(&trap).unwrap().state.crushed, 2.0);
    let animated = &live.animated.get(&door).unwrap().state; assert_eq!(animated.openness, 1.0);
    assert_eq!(animated.body_enabled, Some(false));
    assert_eq!(animated.rig.tree.node(animated.rig.bones[0]).position, Vector3::new(1.0, 2.0, 3.0));
}

#[test]
fn trap_damage_springs_its_current_life_and_rebuild_rejects_delayed_health_commands() {
    let mut live = Live::new(0, 1, 0); let h = live.traps.adopt(1.0, trap()); let mut w = World::default();
    assert!(live.damage(&h, 100.0, Attacker::OwnerSnapshot(Ref::new(3, 7)), props::Identity::Null, &mut w));
    let t = live.traps.get(&h).unwrap(); assert!(t.state.built && t.state.sprung); assert_eq!(t.state.health, 0.0);
    assert_eq!(t.attacker, Attacker::OwnerSnapshot(Ref::new(3, 7))); assert!(w.events.is_empty());
    live.crush(10.0, 600.0, &Strings::default(), &[], &mut w);
    assert!(!live.traps.get(&h).unwrap().state.built);
    assert!(live.health(&h, 100.0, &mut w)); let rebuilt = live.traps.current(&h).unwrap();
    assert_eq!(rebuilt.life, 2.0); assert!(!live.health(&h, 0.0, &mut w));
    let t = live.traps.get(&rebuilt).unwrap(); assert!(t.state.built && !t.state.sprung);
    assert_eq!(t.state.phase, traps::Phase::Reset); assert_eq!(t.attacker, Attacker::OwnerSnapshot(Ref::new(3, 7)));
}

struct BodyWorld { near: bool, position: Vector3, removed: bool, writes: usize, removals: usize, sounds: usize }
impl animated::World for BodyWorld {
    fn near(&mut self, _: &animated::Animated, _: &animated::Settings, _: &[Vector3]) -> bool { self.near }
    fn refresh_body(&mut self, _: &animated::Animated, body: &mut animated::Body) {
        body.position = self.position; body.removed = self.removed;
    }
    fn body_changed(&mut self, _: &animated::Animated, _: &animated::Body) { self.writes += 1; }
    fn sound(&mut self, _: &animated::Animated, _: StrId, _: Vector3) { self.sounds += 1; }
    fn let_go(&mut self, _: &animated::Animated, _: &mut animated::Body) { panic!("no release method"); }
    fn damage(&mut self, _: &animated::Animated, _: &mut animated::Body, _: f64) { self.removed = true; }
    fn remove(&mut self, _: &animated::Animated, body: &mut animated::Body) {
        assert!(body.removed); self.removals += 1;
    }
    fn rider_root(&mut self, _: &animated::Animated, _: &animated::Body, _: StrId, _: f64) -> Option<(Vector3, Quaternion)> { None }
}
fn body() -> animated::Body {
    animated::Body { reference: None, key: 0, id: 1.0, generation: 1.0, soldier: true, walker: false, removed: false,
        alive: true, dead: false, seated: false, held: None, combat: true, mounted: false,
        position: Vector3::new(100.0, 0.0, 0.0), previous: None, settled: None, velocity: Vector3::default(),
        air: false, dive: 0.0, tick_count: None, let_go: false, remove: true, rider_clips: vec![] }
}
#[test]
fn door_and_trigger_read_current_positions_with_fixed_body_membership() {
    let mut prop = door();
    let mut w = BodyWorld { near: true, position: Vector3::default(), removed: false, writes: 0, removals: 0, sounds: 0 };
    // The selected position moved after membership was frozen, before this door.
    prop.step(0.5, 30.0, &[Vector3::new(100.0, 0.0, 0.0)], &mut [], &[], &mut w);
    assert_eq!(prop.state.openness, 0.5);
    prop.settings.door = false; prop.settings.triggers[0].slot = 1;
    let mut bodies = [body()];
    prop.step(0.0, 30.0, &[], &mut bodies, &[0], &mut w);
    assert_eq!(prop.state.slot, 1);
    // A death during the preceding prop excludes this member at its next read.
    prop.state.slot = 0; prop.state.looping = true; prop.state.weight = 1.0; w.removed = true;
    prop.step(0.0, 31.0, &[], &mut bodies, &[0], &mut w);
    assert_eq!(prop.state.slot, 0);
}
#[test]
fn swallow_commits_release_and_refreshes_after_nested_damage_before_removal() {
    let mut prop = door(); prop.settings.kill_sound = Some(StrId(0));
    let mut bodies = [body()];
    prop.state.seize(0, &mut bodies[0], 0, 1.0);
    let mut w = BodyWorld { near: false, position: Vector3::default(), removed: false, writes: 0, removals: 0, sounds: 0 };
    prop.state.swallow(&mut bodies, &prop.settings, &mut w);
    assert_eq!(w.writes, 1); assert_eq!(w.removals, 1); assert_eq!(w.sounds, 1);
    assert!(bodies[0].held.is_none()); assert!(prop.state.victim.is_none());
}

#[test]
fn fleet_birth_after_traps_reserves_the_first_crush_snapshot() {
    let mut live = Live::new(0, 1, 0); let t = trap();
    live.traps.adopt(1.0, Trap::new(t.state, t.settings, 0));
    live.prepare_hulls(2); live.flush();
    let mut w = World { events: Vec::with_capacity(16), alive: [true; 2], supplied: 0.0 };
    let hulls = [Ref::new(0, 1), Ref::new(1, 1)]; let strings = Strings::default();
    allocation::start();
    live.crush(1.0, 60.0, &strings, &hulls, &mut w);
    assert_eq!(allocation::stop(), 0);
    assert_eq!(w.events.iter().filter(|&&e| e == "crush").count(), 2);
}
