//! Synthetic UnitFactory content and reliable native input. These fixtures
//! exercise generated body/staging/rider rows without any installed game data.
use bfe_content::{actor::{build_actor, Actor}, clips::{Clip, Clips}, json::parse, Value};
use bfe_sim::{gen::*, store::{DefId, Nullable, Ref, StrId}, world::admit::bodies::{self, Bindings, Holder, Rider}};
use std::rc::Rc;

fn json(text: &str) -> Value { parse(text).expect("synthetic JSON") }
fn actor() -> Actor {
    let def = json(r#"{"bones":[{"name":"DummyRoot","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}],"parts":[{"vertexOffset":0,"vertexCount":2}]}"#);
    build_actor(&def, &[-1.,0.,-2.,0.,0.,0.,0.,0.,0., 3.,2.,4.,0.,0.,0.,0.,0.,0.]).unwrap()
}
fn clip(name: &str, duration: f64) -> Clip {
    Clip { name: name.into(), fps: 30., frames: Value::Number(1.), tracks: vec![],
        floats: Rc::from([]), start: 0, end: 0, duration, root_motion: false, stride: 0., speed: 0. }
}
fn ids() -> Bindings {
    Bindings { key: F3UnitKey { slot: 9, generation: 0, life: 0. }, side: StrId(1), class_name: StrId(2),
        class: DefId(3), health_scale: StrId(4), shield: None }
}

#[test]
fn soldier_constructor_keeps_defaults_clips_absence_and_f32_clearance() {
    let clips = Clips { list: vec![clip("human_rifle_prone_getup_stand", 0.8), clip("human_rifle_diveforward", 1.2)] };
    let body = bodies::from_parts(&json(r#"{"base":"soldier","MaxHealth":0,"CollisionRootScale":-0,"NoEnterVehicles":"2.5suffix","HealthType":"DROID"}"#),
        &actor(), &clips, None, ids(), 117., false, |_| StrId(5)).unwrap();
    assert_eq!(body.combat.max_health, 300.);
    assert_eq!(body.adoption.no_enter, 2.5);
    assert_eq!(body.adoption.category, 2);
    assert_eq!(body.unit.root_scale.to_bits(), (-0.0f64).to_bits());
    assert_eq!(body.unit.nav_radius.unwrap().to_bits(), ((0.25 * v8math::hypot(4., 6.)) as f32 as f64).to_bits());
    assert_eq!(body.unit.bounds.radius, 1.);
    assert!(body.unit.removed && !body.unit.alive && !body.unit.dead);
    assert_eq!(body.unit.targetable, None);
    assert_eq!(body.unit.network_human, None);
    assert!(matches!(body.unit.held, Nullable::Undefined));
    let UnitBody::Soldier(s) = &body.unit.soldier else { panic!("soldier") };
    assert_eq!([s.max_fly_height, s.getup_time, s.dive_time], [117., 0.8, 0.75 * 1.2]);
    assert_eq!(s.dead, None);
    let stage = body.stage(12);
    let mut row = vec![0.; F3ColdStage::WORDS];
    stage.put(&mut row, &mut Vec::new());
    assert_eq!(F3ColdStage::take(&row, &[]), stage);
    assert_eq!(&row[..8], &[12., 9., 0., 0., 0., 2., 1., 2.5]);
}

#[test]
fn droideka_constructor_prepares_hidden_rig_shield_and_water_rule() {
    let mut binding = ids(); binding.shield = Some(DefId(17));
    let body = bodies::from_parts(&json(r#"{"base":"walkerdroid","WaterDamageAmount":[10],"WaterDamageInterval":-0,"weapons":[{"base":"shield","MaxShield":150,"AddShield":-2,"AddShieldOff":4}]}"#),
        &actor(), &Clips::default(), Some(&Clips::default()), binding, 200., false, |_| StrId(5)).unwrap();
    assert_eq!(body.unit.kind, 3.);
    assert!(body.muzzles.as_ref().is_some_and(|m| !m.words.is_empty()));
    let UnitBody::Droideka(d) = body.unit.soldier else { panic!("droideka") };
    let shield = d.shield.unwrap();
    assert_eq!(shield.def, DefId(17));
    assert_eq!([shield.max, shield.value, shield.drain, shield.refill], [150.,150.,-2.,4.]);
    assert!(!shield.on);
    assert_eq!(d.def, DefId(3));
    assert_eq!(d.root_index, 0.);
    let water = body.adoption.droideka_water.unwrap();
    assert_eq!(water.interval.to_bits(), (-0.0f64).to_bits());
    assert_eq!([water.amount, water.upright, water.ball], [10.,1.5,0.]);
}

#[test]
fn crew_rider_aliases_keep_timed_and_nine_pose_rules() {
    let clips = Clips { list: vec![clip("human_minigun_9pose", 1.), clip("human_drive", 1.)] };
    let unit = Ref::new(4, 0);
    let hull = Holder::Hull { hull: Ref::new(2, 0), seats: vec![
        Some(Rider { anim: Some("man_minigun".into()), nine_pose: true, ..Default::default() }),
        Some(Rider { anim: Some("drivesnowspeeder".into()), ..Default::default() }), None] };
    let row = bodies::rider_birth(unit, 3., 1., &clips, &hull, |c| if c.name == "human_drive" { 7 } else { 8 });
    assert_eq!(row.clips.len(), 3);
    assert_eq!((row.clips[0].clip, row.clips[0].nine, row.clips[0].time), (8, true, Some(4. / 30.)));
    assert_eq!((row.clips[1].clip, row.clips[1].time), (7, Some(0.)));
    assert_eq!(row.clips[2].clip, -1);
    let turret = Holder::Turret { turret: Ref::new(5, 0), rider: None };
    let row = bodies::rider_birth(unit, 3., 1., &clips, &turret, |_| 8);
    assert_eq!((row.clips[0].clip, row.clips[0].nine, row.clips[0].time), (8, true, None));
    let turret = Holder::Turret { turret: Ref::new(5, 0), rider: Some(Rider::default()) };
    let row = bodies::rider_birth(unit, 3., 1., &clips, &turret, |_| 8);
    assert_eq!((row.clips[0].nine, row.clips[0].time), (false, Some(0.)));
}

#[test]
fn missing_and_unsupported_body_content_is_refused() {
    assert!(bodies::from_parts(&json(r#"{"base":"droid"}"#), &actor(), &Clips::default(), None, ids(), 200., false, |_| StrId(0)).is_err());
    assert!(bodies::from_parts(&json(r#"{"base":"walkerdroid"}"#), &actor(), &Clips::default(), None, ids(), 200., false, |_| StrId(0)).is_err());
    let mut missing_root=actor();missing_root.bones[0].name="root".into();
    assert!(bodies::from_parts(&json(r#"{"base":"soldier"}"#), &missing_root, &Clips::default(), None, ids(), 200., false, |_| StrId(0)).is_err());
    assert!(bodies::from_parts(&json(r#"{"base":"soldier","HealthType":true}"#), &actor(), &Clips::default(), None, ids(), 200., false, |_| StrId(0)).is_err());
}

#[cfg(all(not(target_arch = "wasm32"), panic = "unwind"))]
#[test]
fn native_host_entries_preserve_context_refusals_and_unknown_players() {
    use bfe_sim::abi::context::{Context, ErrorKind};
    let context = Context::new();
    assert_eq!(context.world_units().unwrap(), 0);
    let mut actions = PlayerActions { same_epoch: true, payload_valid: true, human_valid: true, ..Default::default() };
    for name in ["jump", "crouch", "use"] { actions.entry.actions.push(context.intern(&name.encode_utf16().collect::<Vec<_>>()).unwrap()); }
    assert!(!context.world_player_actions(0, &actions).unwrap().accepted);
    context.dispose().unwrap();
    assert_eq!(context.world_units().unwrap_err().kind(), ErrorKind::Disposed);
    assert_eq!(context.world_player_actions(0, &actions).unwrap_err().kind(), ErrorKind::Disposed);
}

#[test]
fn turret_holder_keeps_camera_parse_order_bind_rig_and_aim_limits() {
    let instance = json(r#"{"matrix":[1,0,0,0,0,1,0,0,0,0,1,0,2,3,4,1],"settings":{"TurretNodeName":"DRUM","AimerNodeName":"gun","PilotPosition":"seat","PilotAnimation":"drive","PitchLimits":"-10 35","AimerPitchLimits":"-10 35","YawLimits":"90 270","TrackCenter":"1 2 3","EyePointOffset":"0.0 1.5.0 -2.0"}}"#);
    let model = json(r#"{"bones":[{"name":"drum","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,1,0,1]},{"name":"gun","parent":0,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]},{"name":"hp_fire1","parent":1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,2,1]},{"name":"seat","parent":0,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,-1,1]}]}"#);
    let geometry = bodies::turret_geometry(&instance, &model, 6).unwrap();
    assert_eq!(geometry.settings.camera_eye, [0.,1.5,0.]);
    assert_eq!(geometry.settings.camera_center, [0.,1.5,0.]);
    assert_eq!(geometry.settings.pivot, [0.,1.,0.]);
    assert!(geometry.settings.wrap_yaw);
    assert_eq!(geometry.settings.fire_points, vec![2.]);
    assert_eq!(geometry.state.index, 6.);
    assert_eq!(&geometry.state.bones[0].world[12..15], &[2.,4.,4.]);
    assert_eq!(geometry.rider.as_ref().unwrap().clip.as_deref(), Some("human_drive"));
    let birth = bodies::turret_birth(geometry, bodies::TurretBinding { turret: Ref::new(7,0), life: 0., alive: false,
        body: None, center: [2.,3.,4.], radius: 1., ordnance: None, collide: true }, None, None).unwrap();
    assert!(birth.timed && birth.turret.rider && birth.turret.shootable);
    assert!(!birth.turret.weapon);
    let mut row = vec![0.; F2TurretBirth::WORDS]; let mut heap = Vec::new();
    birth.put(&mut row, &mut heap);
    assert_eq!(F2TurretBirth::take(&row, &heap), birth);
}
