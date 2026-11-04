use bfe_sim::ai::combat::{control::Objects, perception::Perception, world::{Ctl, Obj, Ref}};
use bfe_sim::ai::control::{controller::Controller, world::{AlertSource, CombatInput, ControlObs, EntRef, UKey}};
use bfe_sim::ai::nav::math::Vec3;
use three::Vector3;

struct Ids;
impl Objects for Ids {
    fn unit(&self, key: UKey) -> Obj { 1000 + key as u64 }
    fn character(&self, key: usize) -> Obj { 2000 + key as u64 }
    fn post(&self, key: usize) -> Obj { 3000 + key as u64 }
    fn unit_key(&self, obj: Obj) -> UKey { usize::try_from(obj.checked_sub(1000).unwrap()).unwrap() }
    fn alert_source(&self, obj: Obj) -> AlertSource {
        if obj >= 2000 { AlertSource::Character((obj - 2000) as usize) }
        else { AlertSource::Unit((obj - 1000) as usize) }
    }
}

#[test]
fn requests_preserve_perception_between_services_and_rebinds() {
    let mut source = Controller::new(2);
    source.unit = Some(2);
    source.generation = 4.0;
    source.aim_point = Vec3 { x: -0.0, y: 1.0, z: 2.0 };
    source.target = Some(EntRef { unit: 7, generation: 3.0 });
    source.acoustic.push(EntRef { unit: 8, generation: 1.0 });
    let mut perception = Perception::default();
    perception.heap.insert(4.0, Some(Ref { obj: 1007, generation: 3.0 }));
    perception.candidate = Some(Ref { obj: 1009, generation: 2.0 });
    perception.last_scan_count = 17.0;
    perception.aimer.started = true;
    let mut combat = Ctl { obj: 4002, perception: Some(perception.clone()), ..Ctl::default() };
    combat.acoustic.reserve(20);
    let acoustic_storage = combat.acoustic.as_ptr();
    combat.feed_control(&CombatInput::read(&source, Some(2)), &Ids);
    assert_eq!(combat.obj, 4002);
    assert_eq!((combat.unit, combat.character, combat.objective), (Some(1002), Some(2002), Some(3002)));
    assert_eq!(combat.aim_point.x.to_bits(), (-0.0f64).to_bits());
    assert_eq!(combat.target, Some(Ref { obj: 1007, generation: 3.0 }));
    assert_eq!(combat.acoustic, vec![Ref { obj: 1008, generation: 1.0 }]);
    assert_eq!(combat.perception, Some(perception.clone()));
    assert_eq!(combat.acoustic.as_ptr(), acoustic_storage);

    // A different service changes only requests; unbind does not invent
    // a perception reset. createPerception performs that at the next bind.
    source.unit = None;
    source.target = None;
    source.fire_a = true;
    source.acoustic.clear();
    combat.feed_control(&CombatInput::read(&source, None), &Ids);
    assert_eq!(combat.unit, None);
    assert!(combat.fire_a && combat.acoustic.is_empty());
    assert_eq!(combat.target, None);
    assert_eq!(combat.objective, None);
    assert_eq!(combat.perception, Some(perception));
}

#[test]
fn navigation_jet_alias_takes_precedence_over_controller_storage() {
    let mut source = Controller::new(0);
    source.jet_pending = true;
    source.jet_vector = Vec3 { x: 1.0, y: 2.0, z: 3.0 };
    let mut combat = Ctl::default();
    combat.feed_control(&CombatInput::read(&source, None), &Ids);
    assert!(combat.jet.pending);
    assert_eq!(combat.jet.vector, Vector3 { x: 1.0, y: 2.0, z: 3.0 });
    source.nav_control = Some(ControlObs { jet_pending: false, jet_vector: Vec3 { x: 4.0, y: 5.0, z: 6.0 }, stance: 2.0, ..ControlObs::default() });
    combat.feed_control(&CombatInput::read(&source, None), &Ids);
    assert!(!combat.jet.pending);
    assert_eq!(combat.jet.vector, Vector3 { x: 4.0, y: 5.0, z: 6.0 });
    assert_eq!(combat.nav_stance, Some(2.0));
}

#[test]
fn returned_alerts_keep_character_and_direct_unit_identity_distinct() {
    let mut source = Controller::new(3);
    source.shot_pending = true;
    source.shot_known = true;
    source.shot_point = Vec3 { x: 1.0, y: 2.0, z: 3.0 };
    source.dodge = Some(-1.0);
    let mut combat = Ctl::default();
    for alert_source in [AlertSource::Character(7), AlertSource::Unit(7)] {
        source.shot_source = Some(alert_source);
        combat.feed_control(&CombatInput::read(&source, None), &Ids);
        let out = combat.control_alerts(3, &Ids);
        assert_eq!(out.controller, 3);
        assert_eq!(out.shot_source, Some(alert_source));
        assert!(out.shot_pending && out.shot_known);
        assert_eq!(out.shot_point, source.shot_point);
        assert_eq!(out.dodge, Some(-1.0));
    }
    combat.target = Some(Ref { obj: 1009, generation: 12.0 });
    combat.fire_b = true;
    combat.aim_point.x = -0.0;
    let writes = combat.control_writes(&Ids);
    assert_eq!(writes.target, Some(EntRef { unit: 9, generation: 12.0 }));
    assert!(writes.fire_b);
    assert_eq!(writes.aim_point.x.to_bits(), (-0.0f64).to_bits());
}
