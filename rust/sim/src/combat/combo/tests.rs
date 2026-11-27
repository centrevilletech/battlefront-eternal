//! Invented unit and integration tests for the Battlefront II melee combo
//! state machine, swept blade hit tests, blocking/deflection, Force Push,
//! Saber Throw, prediction checkpoints, and determinism.
//!
//! Every fixture here uses invented state names, topology, timings, angles,
//! and numeric sentinels.

use bfe_wide::Wide;
use super::*;
use crate::combat::impacts::HealthScale;
use crate::gen::{BeamSource, Half, HitPart, HitShape, OrdnanceOwner, ShapePart, ShapeType, Store};
use crate::movement::soldier::{swbf2 as move2, Air, Mover, Posture, V3 as MoveV3};
use crate::ordnance::defs::{Defs, Field, OrdnanceDef};
use crate::ordnance::world::{
    DefArg, Hit, ImpactInfo, ItemModel, Out, Point, RiderMount, Shooter, Shot,
    ShotKind, UnitHit, Watched, World,
};
use crate::ordnance::Cx;
use crate::store::{DefId, Nullable, Ref, RefSet, StrId, Strings, Vec3};
use bfe_family::GameFamily;
use serde_json::json;

const DT: f64 = COMBO_FIXED_DT;

pub(super) fn stand_body() -> BodyContext {
    BodyContext {
        posture: ComboPosture::Stand,
        aim_yaw: 0.0,
        velocity_y: 0.0,
        grounded: true,
        moving: false,
    }
}

fn catch_machine() -> ComboMachine {
    ComboMachine::from_json_value(&json!({"nameHash":"a633d905","states":[
        {"name":"CATCH_SABRE","children":[{"key":"Duration","values":[0.13]}]},
        {"name":"purl","children":[{"key":"Duration","values":[0.09]},
            {"key":"Attack","children":[{"key":"Damage","values":[17.75]},{"key":"DamageTime","values":[0,0.08]}]}]},
        {"name":"IDLE","children":[{"key":"Duration","values":[0]},
            {"key":"Transition","values":["purl"],"children":[{"key":"If","children":[
                {"key":"Button","values":["Fire","Press"]}]}]}]}
    ]}),&WeaponMeleeSpec::default(),COMBO_ANIM_FPS,|_|None).unwrap()
}

fn cylinder_shape(center: [f64; 3], radius: f64, half_height: f64) -> HitShape {
    HitShape {
        r#type: ShapeType::Cylinder,
        part: ShapePart::Body,
        center,
        radius,
        half_size: [radius, half_height, radius],
        half: Half::F64(half_height),
        yaw: 0.0,
    }
}

fn sphere_shape(center: [f64; 3], radius: f64) -> HitShape {
    HitShape {
        r#type: ShapeType::Sphere,
        part: ShapePart::Head,
        center,
        radius,
        half_size: [radius; 3],
        half: Half::F64(0.0),
        yaw: 0.0,
    }
}

fn box_shape(center: [f64; 3], half: [f64; 3], yaw: f64) -> HitShape {
    HitShape {
        r#type: ShapeType::Box,
        part: ShapePart::Body,
        center,
        radius: 0.0,
        half_size: half,
        half: Half::Vec3(half),
        yaw,
    }
}

fn capsule_shape(center: [f64; 3], half: [f64; 3], radius: f64) -> HitShape {
    HitShape {
        r#type: ShapeType::Capsule,
        part: ShapePart::Body,
        center,
        radius,
        half_size: half,
        half: Half::Vec3(half),
        yaw: 0.0,
    }
}

#[test]
fn swept_blade_prevents_tunneling_and_hits_once_across_state_boundary() {
    // Fast swing sweeps from x = -1.5 to x = +1.5 in one tick; target stands at (0, 1.0, 1.2)
    // with radius 0.35. Neither prev nor curr blade segment alone touches the target!
    let prev = BladeSegment {
        base: [0.0, 1.0, 0.0],
        tip: [-1.5, 1.0, 1.2],
    };
    let curr = BladeSegment {
        base: [0.0, 1.0, 0.0],
        tip: [1.5, 1.0, 1.2],
    };
    let shape = cylinder_shape([0.0, 1.0, 1.2], 0.35, 0.8);

    // Static endpoint checks (prev-to-prev and curr-to-curr) miss the target.
    let static_prev = BladeSweep {
        prev,
        curr: prev,
        radius: 0.1,
    };
    let static_curr = BladeSweep {
        prev: curr,
        curr,
        radius: 0.1,
    };
    assert!(static_prev.hit_shape(&shape).is_none());
    assert!(static_curr.hit_shape(&shape).is_none());

    // Swept quad catches the target cleanly!
    let swept = BladeSweep {
        prev,
        curr,
        radius: 0.1,
    };
    assert!(swept.hit_shape(&shape).is_some());

    // Also test sphere, capsule, and yaw-rotated box shapes.
    assert!(swept.hit_shape(&sphere_shape([0.0, 1.0, 0.9], 0.2)).is_some());
    assert!(swept
        .hit_shape(&capsule_shape([0.0, 1.0, 0.9], [0.0, 0.4, 0.0], 0.2))
        .is_some());
    assert!(swept
        .hit_shape(&box_shape([0.0, 1.0, 0.9], [0.25, 0.3, 0.25], 0.4))
        .is_some());
    assert!(swept.hit_shape(&sphere_shape([0.0, 4.0, 0.9], 0.2)).is_none());

}

/// Minimal synthetic [`World`] backing real [`crate::ordnance::Cx`] bolt reflection tests.
struct BoltTestWorld {
    store: Store,
    strings: Strings,
    defs: Defs,
    hits: Vec<(Ref<crate::gen::Unit>, OrdnanceOwner, Vec3)>,
    explosions: Vec<(Vec3, OrdnanceOwner)>,
    shooter_unit: Ref<crate::gen::Unit>,
    defender_unit: Ref<crate::gen::Unit>,
    shooter_pos: Vec3,
}

impl BoltTestWorld {
    fn new() -> Self {
        let mut store = Store::default();
        let shooter_unit = store.unit.insert(crate::gen::Unit::default());
        let defender_unit = store.unit.insert(crate::gen::Unit::default());
        let bolt_def = OrdnanceDef::read(|k| match k {
            "base" => Field::Str("bolt".into()),
            "Velocity" => Field::Num(120.0),
            "LifeSpan" => Field::Num(2.0),
            "Damage" => Field::Num(90.0),
            _ => Field::Undefined,
        });
        let expiring_bolt_def = OrdnanceDef::read(|k| match k {
            "base" => Field::Str("bolt".into()),
            "Velocity" => Field::Num(120.0),
            "LifeSpan" => Field::Num(2.0),
            "Damage" => Field::Num(90.0),
            "ExplosionExpire" => Field::Str("inv_expire_blast".into()),
            _ => Field::Undefined,
        });
        let defs = Defs {
            ordnance: vec![bolt_def, expiring_bolt_def],
            ..Defs::default()
        };
        Self {
            store,
            strings: Strings::default(),
            defs,
            hits: Vec::new(),
            explosions: Vec::new(),
            shooter_unit,
            defender_unit,
            shooter_pos: [0.0, 1.0, 10.0],
        }
    }
}

impl World for BoltTestWorld {
    fn store(&self) -> &Store { &self.store }
    fn store_mut(&mut self) -> &mut Store { &mut self.store }
    fn strings_mut(&mut self) -> &mut Strings { &mut self.strings }
    fn definitions(&self) -> &Defs { &self.defs }
    fn cast(&mut self, _: Vec3, _: Vec3, _: f64, _: Out, _: Nullable<Ref<crate::gen::Body>>, _: Option<bool>) -> Option<Hit> { None }
    fn cast_moving(&mut self, _: Vec3, _: Vec3, _: f64, _: Out, _: Nullable<Ref<crate::gen::Body>>) -> Option<Hit> { None }
    fn raycast_units(
        &mut self,
        origin: Vec3,
        direction: Vec3,
        far: f64,
        owner: OrdnanceOwner,
        _: Option<f64>,
        _: Option<Ref<crate::gen::Body>>,
        _: Option<Ref<crate::gen::Vehicle>>,
        _: bool,
    ) -> Option<UnitHit> {
        // Reflected bolt owned by defender can strike shooter at shooter_pos (sphere r = 0.6).
        if owner == OrdnanceOwner::Unit(self.defender_unit) {
            let to = [
                self.shooter_pos[0] - origin[0],
                self.shooter_pos[1] - origin[1],
                self.shooter_pos[2] - origin[2],
            ];
            let proj = to[0] * direction[0] + to[1] * direction[1] + to[2] * direction[2];
            if proj >= 0.0 && proj <= far {
                let px = origin[0] + direction[0] * proj;
                let py = origin[1] + direction[1] * proj;
                let pz = origin[2] + direction[2] * proj;
                let dx = px - self.shooter_pos[0];
                let dy = py - self.shooter_pos[1];
                let dz = pz - self.shooter_pos[2];
                let d2 = dx * dx + dy * dy + dz * dz;
                if d2 <= 0.6 * 0.6 {
                    return Some(UnitHit {
                        unit: self.shooter_unit,
                        distance: proj,
                        point: [px, py, pz],
                        normal: [-direction[0], -direction[1], -direction[2]],
                        part: Nullable::Value(HitPart::Body),
                    });
                }
            }
        }
        None
    }
    fn rider_mount(&mut self, _: Nullable<Ref<crate::gen::Body>>) -> Option<RiderMount> { None }
    fn fat_query(&mut self, _: Vec3, _: f64, _: OrdnanceOwner, _: &RefSet<crate::gen::Unit>, _: Option<Ref<crate::gen::Body>>) -> Option<Vec<Ref<crate::gen::Unit>>> { None }
    fn unit_sphere(&mut self, _: Ref<crate::gen::Unit>) -> Option<(Vec3, f64)> { None }
    fn is_droid(&mut self, _: Ref<crate::gen::Unit>) -> bool { false }
    fn gone(&mut self, _: BeamSource) -> bool { false }
    fn beam_local_origin(&mut self, _: BeamSource, _: Vec3) -> Option<Vec3> { None }
    fn beam_pose(&mut self, _: BeamSource, _: Vec3, _: &mut Vec3, _: &mut Vec3) {}
    fn shot_tick(&mut self, _: OrdnanceOwner, _: f64) -> Nullable<f64> { Nullable::Null }
    fn history_tick(&mut self) -> f64 { 0.0 }
    fn shooter(&mut self, _: OrdnanceOwner, _: f64) -> Shooter { Shooter { fire_id: 0.0, unit: 0.0, generation: 0.0 } }
    fn missile_target(&mut self, _: f64, _: f64) -> Option<Vec3> { None }
    fn navigate_missile(&mut self, _: Vec3, _: &mut Vec3, _: Option<StrId>, id: f64, _: f64, _: f64) -> f64 { id }
    fn water_crossing(&mut self, _: Vec3, _: Vec3) -> Option<Vec3> { None }
    fn waver(&mut self) -> f64 { 0.0 }
    fn spin(&mut self) -> f64 { 0.0 }
    fn explosion(&mut self, _: &Field, point: Vec3, owner: OrdnanceOwner) {
        self.explosions.push((point, owner));
    }
    fn hit(&mut self, _: Ref<crate::gen::Body>, _: DefArg, _: OrdnanceOwner, _: Vec3) {}
    fn hit_unit(&mut self, unit: Ref<crate::gen::Unit>, _: DefArg, point: Point, owner: OrdnanceOwner, _: Nullable<HitPart>, _: Vec3) {
        self.hits.push((unit, owner, point.value()));
    }
    fn projectile(&mut self, _: Shot, _: ShotKind) {}
    fn launch(&mut self, _: Ref<crate::gen::Bolt>, _: Nullable<f64>) {}
    fn impact(&mut self, _: Shot, _: &ImpactInfo) {}
    fn grenade_contact(&mut self, _: Ref<crate::gen::Grenade>) {}
    fn bounce(&mut self, _: DefId, _: Vec3, _: f64) {}
    fn item_cast(&mut self, _: Vec3, _: Vec3, _: f64) -> Option<Hit> { None }
    fn blocked(&mut self, _: Vec3, _: Vec3) -> bool { false }
    fn item_water_crossing(&mut self, _: Vec3, _: Vec3) -> Option<Vec3> { None }
    fn item_model(&mut self, _: DefId) -> ItemModel { ItemModel { mesh: false, radius: 0.0, center: None } }
    fn owner_key(&mut self, _: crate::gen::ItemOwner) -> String { String::new() }
    fn drop_roll(&mut self) -> f64 { 0.0 }
    fn explode(&mut self, _: &Field, _: Vec3, _: crate::gen::ItemOwner) {}
    fn watch(&mut self, _: usize, _: f64) -> Watched { Watched { relation: 0.0, center: [0.0; 3], radius: 0.0, velocity: [0.0; 3] } }
    fn standing(&mut self, _: usize) -> Vec3 { [0.0; 3] }
    fn give(&mut self, _: usize, _: DefId) -> bool { false }
    fn pickup(&mut self, _: DefId, _: Vec3) {}
}

#[test]
fn redirected_bolt_hits_shooter_with_defenders_credit() {
    let mut world = BoltTestWorld::new();
    let shooter = world.shooter_unit;
    let defender = world.defender_unit;
    let bolt_def = DefId(0);

    // Shooter at z = 10 fires a bolt along -Z toward defender at z = 0 facing +Z (yaw = 0).
    let incoming_bolt = {
        let mut cx = Cx::new(&mut world);
        match cx.fire_round(
            bolt_def,
            [0.0, 1.0, 10.0],
            [0.0, 0.0, -1.0],
            OrdnanceOwner::Unit(shooter),
            None,
            None,
            BeamSource::Null,
            0.0,
            None,
            None,
        ) {
            Some(Shot::Bolt(r)) => r,
            other => panic!("expected bolt, got {other:?}"),
        }
    };

    let deflect_def = DeflectDef {
        min_angle_deg: -110.0,
        max_angle_deg: 110.0,
        energy_cost: 2.5,
        deflect_anim_start: 0,
        deflect_anim_len: 1,
        reaction_ids: [0, u16::MAX, u16::MAX, u16::MAX, u16::MAX, u16::MAX],
    };
    let defender_pose = DefenderPose {
        position: [0.0, 0.0, 0.0],
        aim_yaw: 0.0,
        aim_dir: [0.0, 0.0, 1.0],
    };
    let incoming = IncomingProjectile {
        kind: DeflectableKind::Bolt,
        impact_point: [0.0, 1.0, 0.3],
        incoming_dir: [0.0, 0.0, -1.0],
        shooter_pos: Some([0.0, 1.0, 10.0]),
        ordnance_def: bolt_def,
    };

    // Out-of-arc shot (from behind, travelling +Z while defender faces +Z) is not deflected.
    let mut energy = 10.0;
    let from_behind = IncomingProjectile {
        incoming_dir: [0.0, 0.0, 1.0],
        ..incoming
    };
    assert!(matches!(
        try_deflect(Some(&deflect_def), &defender_pose, &mut energy, &from_behind, DeflectAimMode::AimDirection),
        DeflectOutcome::OutOfArc { .. }
    ));
    assert_eq!(energy, 10.0);

    // Missile is not deflectable under provisional rules.
    let missile = IncomingProjectile {
        kind: DeflectableKind::Missile,
        ..incoming
    };
    assert_eq!(
        try_deflect(Some(&deflect_def), &defender_pose, &mut energy, &missile, DeflectAimMode::AimDirection),
        DeflectOutcome::NotDeflectableKind
    );

    // Insufficient energy refuses attack-time deflect with EnergyCost > 0.
    let mut low_energy = 1.0;
    assert_eq!(
        try_deflect(Some(&deflect_def), &defender_pose, &mut low_energy, &incoming, DeflectAimMode::AimDirection),
        DeflectOutcome::InsufficientEnergy
    );

    // Valid frontal deflection kills the incoming bolt, drains 2.5 energy, and launches
    // a real reflected bolt in ordnance that flies back and hits the original shooter!
    let (outcome, reflected) = {
        let mut cx = Cx::new(&mut world);
        reflect_in_ordnance(
            &mut cx,
            Some(Shot::Bolt(incoming_bolt)),
            OrdnanceOwner::Unit(defender),
            None,
            Some(&deflect_def),
            &defender_pose,
            &mut energy,
            &incoming,
            RedirectAim {mode:DeflectAimMode::AimDirection, spread_degrees:0.75, projectile_seed: Wide::from_u32(27)},
        )
    };
    assert!(matches!(
        outcome,
        DeflectOutcome::Deflected {
            direction: DeflectDirection::Forward,
            energy_spent,
            play_reaction: true,
            ..
        } if energy_spent == 2.5
    ));
    assert!((energy - 7.5).abs() < 1e-12);
    assert_eq!(reflected, Some(Shot::Bolt(incoming_bolt)));
    assert!(world.store.bolt.get(incoming_bolt).unwrap().life > 0.0);

    // Step ordnance until the reflected bolt crosses 9.7m at 120 m/s (~6 ticks at 60 Hz).
    {
        let mut cx = Cx::new(&mut world);
        for _ in 0..10 {
            cx.update(DT);
        }
    }
    assert_eq!(world.hits.len(), 1);
    assert_eq!(world.hits[0].0, shooter);
    assert_eq!(world.hits[0].1, OrdnanceOwner::Unit(defender));
}

#[test]
fn force_push_and_saber_throw_boomerang_work_deterministically() {
    // 1. Force Push cone check, max target cap, friend filtering, energy drain.
    let push_def = ForcePushDef {
        energy_drain: 30.0,
        cone_length: 15.0,
        cone_angle_deg: 90.0,
        max_targets: 2,
        push: 12.0,
        damage: 1.0,
        affect_friends: false,
    };
    let candidates = [
        ForcePushCandidate { target_id: TargetId::Life(1), position: [0.0, 0.0, 8.0], team: 2.0, alive: true, los_clear: true },
        ForcePushCandidate { target_id: TargetId::Life(2), position: [0.0, 0.0, 4.0], team: 2.0, alive: true, los_clear: true },
        ForcePushCandidate { target_id: TargetId::Life(3), position: [0.0, 0.0, 10.0], team: 2.0, alive: true, los_clear: true },
        ForcePushCandidate { target_id: TargetId::Life(4), position: [0.0, 0.0, 3.0], team: 1.0, alive: true, los_clear: true }, // friend skipped
        ForcePushCandidate { target_id: TargetId::Life(5), position: [0.0, 0.0, -5.0], team: 2.0, alive: true, los_clear: true }, // behind skipped
    ];
    let mut energy = 50.0;
    let res = execute_force_push(&push_def, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0, &mut energy, &candidates)
        .expect("should execute Force Push");
    assert_eq!(energy, 20.0);
    assert_eq!(res.hits().len(), 2, "capped to max_targets=2");
    assert_eq!(res.hits()[0].target_id, TargetId::Life(2), "nearest target first");
    assert_eq!(res.hits()[1].target_id, TargetId::Life(1));
    assert!(res.hits()[0].impulse[2] > 11.9 && res.hits()[0].enters_tumble);
    // Not enough energy for a second push.
    assert!(execute_force_push(&push_def, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0, &mut energy, &candidates).is_none());

    // 2. Saber Throw outbound -> target hit once -> return to moving caster -> enters CATCH_SABRE.
    let m = catch_machine();
    let mut combo = ComboState::new(&m);
    let throw_def = SaberThrowDef {
        energy_drain: 40.0,
        velocity: 24.0,
        life_span: 0.5,
        return_duration: 0.4,
        rotation_rate: 3.0,
        damage: 250.0,
        damage_length: 1.2,
        damage_width: 0.7,
        push: 0.0,
        ..SaberThrowDef::default()
    };
    let tuning = test_move_tuning();
    let mut life = move2::State::new(&tuning);
    life.energy = 60.0;
    let mut throw_st = SaberThrowState::new();
    assert!(throw_st.launch(&throw_def, [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], &mut SoldierEnergy { state: &mut life, tuning: &tuning }));
    assert_eq!(life.energy, 20.0);

    let enemy_shapes = [cylinder_shape([0.0, 1.0, 6.0], 0.5, 0.9)];
    let near_shapes = [cylinder_shape([1.0, 1.0, 0.8], 0.5, 0.9)];
    let mut outbound_hits = 0usize;
    let mut return_hits = 0usize;
    let mut near_return_hits = 0usize;
    let mut caught = false;
    for tick in 0..80 {
        // Hero strafes slowly along +X while blade flies out and returns.
        let hand_pos = [tick as f64 * 0.02, 1.0, 0.0];
        let step_out = throw_st.step_with_combo(&throw_def, &m, &mut combo, hand_pos, true, DT);
        if let Some(sweep) = step_out.sweep {
            if throw_st.test_target(&sweep, TargetId::Life(99), &enemy_shapes).is_some() {
                if step_out.phase == SaberThrowPhase::Returning {
                    return_hits += 1;
                } else {
                    outbound_hits += 1;
                }
            }
            if throw_st.test_target(&sweep, TargetId::Life(100), &near_shapes).is_some()
                && step_out.phase == SaberThrowPhase::Returning
            {
                near_return_hits += 1;
            }
        }
        if step_out.caught {
            caught = true;
            break;
        }
    }
    assert!(caught, "thrown saber must boomerang back to caster");
    assert_eq!(outbound_hits, 1, "outbound pass hits target once");
    assert!(return_hits <= 1, "return pass hits at most once");
    assert_eq!(
        near_return_hits, 1,
        "catch-tick sweep must hit near target once and deduplicate against the return pass"
    );
    assert_eq!(combo.state, m.catch_state.unwrap(), "catching saber enters CATCH_SABRE");
    for _ in 0..8 {
        step(GameFamily::Swbf2,&m,&mut combo,&Default::default(),&stand_body(),&mut life,&tuning,false,DT);
    }
    assert!(combo.is_idle(&m));
    let press = ComboInput::default().press(ComboButton::Fire);
    step(GameFamily::Swbf2,&m,&mut combo,&press,&stand_body(),&mut life,&tuning,false,DT);
    let next = step(GameFamily::Swbf2,&m,&mut combo,&Default::default(),&stand_body(),&mut life,&tuning,false,DT);
    assert!(!next.active_attacks.is_empty());

}

#[test]
fn swbf1_family_bypasses_combo_machine_completely() {
    let m = catch_machine();
    let tuning = test_move_tuning();
    let body = stand_body();
    let mut st = ComboState::new(&m);
    let before = st;
    let mut energy = move2::State::new(&tuning);
    let out = step(
        GameFamily::Swbf1,
        &m,
        &mut st,
        &ComboInput::default().press(ComboButton::Fire),
        &body,
        &mut energy,
        &tuning,
        true,
        DT,
    );
    assert!(!out.active);
    assert_eq!(st, before);
    assert_eq!(energy.energy, tuning.energy_capacity);
}

#[test]
fn sweep_reads_canonical_half_dedup_never_evicts_and_deflect_powers_fail_closed() {
    // 1. SavedShape::ray_shape layout (half_size = [0; 3], canonical half = Half::Vec3)
    //    must hit cleanly for both Box and Capsule!
    let sweep = BladeSweep {
        prev: BladeSegment { base: [0.0, 1.0, 0.0], tip: [-0.5, 1.0, 1.0] },
        curr: BladeSegment { base: [0.0, 1.0, 0.0], tip: [0.5, 1.0, 1.0] },
        radius: 0.05,
    };
    let history_box = HitShape {
        r#type: ShapeType::Box,
        part: ShapePart::Body,
        center: [0.0, 1.0, 1.4],
        radius: 0.0,
        half_size: [0.0; 3],
        half: Half::Vec3([0.5, 0.5, 0.5]),
        yaw: 0.0,
    };
    assert!(
        sweep.hit_shape(&history_box).is_some(),
        "BladeSweep must read canonical HitShape.half_vector(), not duplicated half_size"
    );

    let history_capsule = HitShape {
        r#type: ShapeType::Capsule,
        part: ShapePart::Body,
        center: [0.0, 0.5, 1.0],
        radius: 0.1,
        half_size: [0.0; 3],
        half: Half::Vec3([0.0, 0.6, 0.0]),
        yaw: 0.0,
    };
    assert!(
        sweep.hit_shape(&history_capsule).is_some(),
        "BladeSweep capsule must read canonical HitShape.half_vector()"
    );

    // 2. HitHistory at per-attack capacity (8 targets) must NOT evict earlier targets
    //    and must never suppress an unrelated attack group.
    let mut history = HitHistory::new();
    let key_a = pack_attack_key(StateId(1), 0);
    let key_b = pack_attack_key(StateId(2), 0);
    for id in 1..=(MAX_TARGETS_PER_ATTACK as u32 + 5) {
        history.record(key_a, 0, 1.0, TargetId::Life(id));
    }
    assert!(
        history.has_hit(key_a, 0, TargetId::Life(1)),
        "target 1 must never be evicted from HitHistory"
    );
    assert!(
        history.record(key_b, 0, 1.0, TargetId::Life(99)),
        "filling attack A's 8-target list must not suppress attack B"
    );

    // 3. DeflectableKind fails closed on sticky/grenade/unknown bases, and zero incoming_dir fails.
    let grenade_def = OrdnanceDef::read(|k| match k {
        "base" => Field::Str("grenade".into()),
        _ => Field::Undefined,
    });
    assert!(
        !DeflectableKind::from_ordnance_def(&grenade_def).is_deflectable(),
        "grenade must not default to deflectable Bolt"
    );
    let sticky_def = OrdnanceDef::read(|k| match k {
        "base" => Field::Str("sticky".into()),
        _ => Field::Undefined,
    });
    assert!(
        !DeflectableKind::from_ordnance_def(&sticky_def).is_deflectable(),
        "sticky ordnance must not default to deflectable Bolt"
    );

    let deflect_def = DeflectDef {
        min_angle_deg: -110.0,
        max_angle_deg: 110.0,
        energy_cost: 2.5,
        deflect_anim_start: 0,
        deflect_anim_len: 1,
        reaction_ids: [0, u16::MAX, u16::MAX, u16::MAX, u16::MAX, u16::MAX],
    };
    let pose = DefenderPose {
        position: [0.0; 3],
        aim_yaw: 0.0,
        aim_dir: [0.0, 0.0, 1.0],
    };
    let zero_dir = IncomingProjectile {
        kind: DeflectableKind::Bolt,
        impact_point: [0.0, 1.0, 0.3],
        incoming_dir: [0.0, 0.0, 0.0],
        shooter_pos: None,
        ordnance_def: DefId(0),
    };
    let mut energy = 50.0;
    assert!(
        !matches!(
            try_deflect(Some(&deflect_def), &pose, &mut energy, &zero_dir, DeflectAimMode::AimDirection),
            DeflectOutcome::Deflected { .. }
        ),
        "zero incoming_dir must not deflect"
    );
    assert_eq!(energy, 50.0);

    // 4. Negative energy_drain must never mint energy in Force Push or Saber Throw.
    let bad_push = ForcePushDef {
        energy_drain: -30.0,
        cone_length: 15.0,
        cone_angle_deg: 90.0,
        max_targets: 4,
        push: 10.0,
        damage: 1.0,
        affect_friends: false,
    };
    let mut push_energy = 90.0;
    assert!(
        execute_force_push(&bad_push, [0.0; 3], [0.0, 0.0, 1.0], 1.0, &mut push_energy, &[]).is_none(),
        "negative ForcePushDef.energy_drain must be rejected"
    );
    assert_eq!(push_energy, 90.0);

    let bad_throw = SaberThrowDef {
        energy_drain: -20.0,
        velocity: 20.0,
        life_span: 1.0,
        return_duration: 0.5,
        rotation_rate: 3.0,
        damage: 200.0,
        damage_length: 1.2,
        damage_width: 0.6,
        push: 0.0,
        ..SaberThrowDef::default()
    };
    let mut ts = SaberThrowState::new();
    let mut throw_energy = 90.0;
    assert!(
        !ts.launch(&bad_throw, [0.0; 3], [0.0, 0.0, 1.0], &mut throw_energy),
        "negative SaberThrowDef.energy_drain must be rejected"
    );
    assert_eq!(throw_energy, 90.0);

    // 5. Deflect edge cases: exact 0.0 energy with cost > 0 refused, Forward cutoff at 22.5 deg,
    //    Shot::Round (bullet) termination in reflect_in_ordnance, and deflect-twice ping-pong!
    assert_eq!(deflect::classify_direction(20.0), DeflectDirection::Forward);
    assert_eq!(deflect::classify_direction(25.0), DeflectDirection::FrontRight);
    assert_eq!(deflect::classify_direction(-25.0), DeflectDirection::FrontLeft);
    assert_eq!(deflect::classify_direction(80.0), DeflectDirection::Right);
    assert_eq!(deflect::classify_direction(-80.0), DeflectDirection::Left);

    let mut zero_e = 0.0;
    let valid_bolt = IncomingProjectile {
        kind: DeflectableKind::Bolt,
        impact_point: [0.0, 1.0, 0.3],
        incoming_dir: [0.0, 0.0, -1.0],
        shooter_pos: Some([0.0, 1.0, 10.0]),
        ordnance_def: DefId(0),
    };
    assert_eq!(
        try_deflect(Some(&deflect_def), &pose, &mut zero_e, &valid_bolt, DeflectAimMode::AimDirection),
        DeflectOutcome::InsufficientEnergy
    );
    assert_eq!(zero_e, 0.0);

    // Terminate Shot::Round (bullet) and deflect twice (hero A -> hero B -> hero A).
    let mut world = BoltTestWorld::new();
    let bullet_def = OrdnanceDef::read(|k| match k {
        "base" => Field::Str("bullet".into()),
        "Velocity" => Field::Num(100.0),
        "LifeSpan" => Field::Num(2.0),
        "Damage" => Field::Num(40.0),
        _ => Field::Undefined,
    });
    world.defs.ordnance.push(bullet_def);
    let bullet_id = DefId((world.defs.ordnance.len() - 1) as u32);
    let shooter = world.shooter_unit;
    let defender = world.defender_unit;
    let incoming_round = {
        let mut cx = Cx::new(&mut world);
        match cx.fire_round(
            bullet_id,
            [0.0, 1.0, 10.0],
            [0.0, 0.0, -1.0],
            OrdnanceOwner::Unit(shooter),
            None,
            None,
            BeamSource::Null,
            0.0,
            None,
            None,
        ) {
            Some(Shot::Round(r)) => r,
            other => panic!("expected bullet round, got {other:?}"),
        }
    };
    let incoming_bullet = IncomingProjectile {
        kind: DeflectableKind::Bullet,
        impact_point: [0.0, 1.0, 0.3],
        incoming_dir: [0.0, 0.0, -1.0],
        shooter_pos: Some([0.0, 1.0, 10.0]),
        ordnance_def: bullet_id,
    };
    let mut def_e = 50.0;
    let (res1, shot1) = {
        let mut cx = Cx::new(&mut world);
        reflect_in_ordnance(
            &mut cx,
            Some(Shot::Round(incoming_round)),
            OrdnanceOwner::Unit(defender),
            None,
            Some(&deflect_def),
            &pose,
            &mut def_e,
            &incoming_bullet,
            RedirectAim {mode:DeflectAimMode::AimDirection, spread_degrees:0.75, projectile_seed: Wide::from_u32(27)},
        )
    };
    assert!(matches!(res1, DeflectOutcome::Deflected { .. }));
    let reflected_round = match shot1 {
        Some(Shot::Round(r)) => r,
        other => panic!("expected reflected bullet round, got {other:?}"),
    };
    assert_eq!(reflected_round, incoming_round, "incoming bullet must be redirected in place");
    let orig = world.store.round.get(incoming_round).unwrap();
    assert!(
        !orig.dead && orig.gone.is_none() && orig.life > 0.0 && orig.owner == OrdnanceOwner::Unit(defender),
        "redirected bullet must stay live under the defender's ownership"
    );

    // Second hero at z = 10 facing -Z (yaw = PI) deflects the reflected bullet right back!
    let pose_b = DefenderPose {
        position: [0.0, 0.0, 10.0],
        aim_yaw: std::f64::consts::PI,
        aim_dir: [0.0, 0.0, -1.0],
    };
    let incoming_back = IncomingProjectile {
        kind: DeflectableKind::Bullet,
        impact_point: [0.0, 1.0, 9.7],
        incoming_dir: [0.0, 0.0, 1.0],
        shooter_pos: Some([0.0, 1.0, 0.0]),
        ordnance_def: bullet_id,
    };
    let mut def_e_b = 50.0;
    let (res2, shot2) = {
        let mut cx = Cx::new(&mut world);
        reflect_in_ordnance(
            &mut cx,
            Some(Shot::Round(reflected_round)),
            OrdnanceOwner::Unit(shooter),
            None,
            Some(&deflect_def),
            &pose_b,
            &mut def_e_b,
            &incoming_back,
            RedirectAim {mode:DeflectAimMode::AimDirection, spread_degrees:0.75, projectile_seed: Wide::from_u32(27)},
        )
    };
    assert!(matches!(res2, DeflectOutcome::Deflected { direction: DeflectDirection::Forward, .. }));
    assert_eq!(shot2, Some(Shot::Round(reflected_round)));
    let back = world.store.round.get(reflected_round).unwrap();
    assert!(!back.dead && back.owner == OrdnanceOwner::Unit(shooter));

    // 6. ForcePush occluded candidate skipped via los_clear = false, and SaberThrow resolve_hit
    //    applies damage/scales/push with per-pass deduplication.
    let valid_push = ForcePushDef {
        energy_drain: 10.0,
        cone_length: 15.0,
        cone_angle_deg: 90.0,
        max_targets: 4,
        push: 12.0,
        damage: 5.0,
        affect_friends: false,
    };
    let los_cands = [
        ForcePushCandidate { target_id: TargetId::Life(1), position: [0.0, 0.0, 5.0], team: 2.0, alive: true, los_clear: false },
        ForcePushCandidate { target_id: TargetId::Life(2), position: [0.0, 0.0, 6.0], team: 2.0, alive: true, los_clear: true },
    ];
    let mut e = 50.0;
    let push_res = execute_force_push(&valid_push, [0.0; 3], [0.0, 0.0, 1.0], 1.0, &mut e, &los_cands).unwrap();
    assert_eq!(push_res.hits().len(), 1);
    assert_eq!(push_res.hits()[0].target_id, TargetId::Life(2), "occluded target 1 must be skipped");

    let mut invalid_throw = bad_throw;
    invalid_throw.energy_drain = 10.0;
    invalid_throw.velocity = f64::NAN;
    assert!(invalid_throw.validate().is_err());
    invalid_throw.velocity = 20.0;
    invalid_throw.life_span = 0.0;
    assert!(invalid_throw.validate().is_err());

    let mut valid_throw = SaberThrowDef {
        energy_drain: 20.0,
        velocity: 20.0,
        life_span: 0.5,
        return_duration: 0.4,
        rotation_rate: 3.0,
        damage: 200.0,
        damage_length: 1.4,
        damage_width: 0.8,
        push: 5.0,
        ..SaberThrowDef::default()
    };
    valid_throw.damage_scales[HealthScale::Droid as usize] = 0.5;
    valid_throw.shield_scale = 2.0;
    let mut ts2 = SaberThrowState::new();
    let mut te = 50.0;
    assert!(ts2.launch(&valid_throw, [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], &mut te));
    let target_shapes = [cylinder_shape([0.0, 1.0, 1.0], 0.5, 0.9)];
    let target = MeleeTarget {
        target_id: TargetId::Life(77),
        position: [0.0, 0.0, 1.0],
        aim_yaw: 0.0,
        health_scale: HealthScale::Droid,
        shapes: &target_shapes,
        deflect: None,
    };
    let mut hit_count = 0usize;
    for _ in 0..8 {
        let s = ts2.step(&valid_throw, [0.0, 1.0, 0.0], true, DT);
        if let Some(sw) = s.sweep {
            if let MeleeHitOutcome::Hit { damage, shield_damage, push_impulse, .. } =
                ts2.resolve_hit(&valid_throw, &sw, &target, None)
            {
                assert_eq!(damage, 100.0, "200 * Droid scale 0.5");
                assert_eq!(shield_damage, 400.0, "200 * shield scale 2.0");
                assert!(push_impulse[2] > 4.9);
                hit_count += 1;
            }
        }
    }
    assert_eq!(hit_count, 1, "SaberThrowState::resolve_hit must apply damage once per pass");
}

#[test]
fn single_attack_hits_stationary_target_once_across_exit_and_scopes_by_low20_group() {
    // Purely invented 3-state machine exercising:
    // 1. A 3-frame attack spanning into state exit must hit a stationary target once (not twice).
    // 2. Two invented AttackIds sharing the same nonzero low-20-bit FNV-1a hash share one hit group.
    // 3. Filling one attack's hit list must never suppress an unrelated attack group.
    let doc = json!({
        "nameHash": "12345678",
        "animations": [
            { "values": ["inv_short_cut"], "duration": 0.10 },
            { "values": ["inv_follow_cut"], "duration": 0.10 }
        ],
        "states": [
            {
                "name": "IDLE",
                "children": [
                    { "key": "Duration", "values": [0.0] },
                    {
                        "key": "Transition",
                        "values": ["SHORT_CUT"],
                        "children": [{
                            "key": "If",
                            "children": [
                                { "key": "Break", "values": [] },
                                { "key": "Button", "values": ["Fire", "Press"] }
                            ]
                        }]
                    }
                ]
            },
            {
                "name": "SHORT_CUT",
                "children": [
                    { "key": "Animation", "values": ["inv_short_cut"] },
                    { "key": "TurnFactor", "values": [0.73] },
                    { "key": "StrafeFactor", "values": [0.61] },
                    {
                        "key": "Attack",
                        "children": [
                            { "key": "AttackId", "values": ["cut_grp_429"] },
                            { "key": "DamageTime", "values": [0, 3, "Frames"] },
                            { "key": "DamageLength", "values": [1.4] },
                            { "key": "DamageWidth", "values": [0.6] },
                            { "key": "Damage", "values": [220.0] },
                            { "key": "Push", "values": [2.0] }
                        ]
                    },
                    { "key": "Transition", "values": ["FOLLOW_CUT"] }
                ]
            },
            {
                "name": "FOLLOW_CUT",
                "children": [
                    { "key": "Animation", "values": ["inv_follow_cut"] },
                    { "key": "TurnFactor", "values": [0.74] },
                    { "key": "StrafeFactor", "values": [0.62] },
                    {
                        "key": "Attack",
                        "children": [
                            { "key": "AttackId", "values": ["cut_grp_3200"] },
                            { "key": "DamageTime", "values": [0, 3, "Frames"] },
                            { "key": "DamageLength", "values": [1.4] },
                            { "key": "DamageWidth", "values": [0.6] },
                            { "key": "Damage", "values": [220.0] },
                            { "key": "Push", "values": [2.0] }
                        ]
                    },
                    { "key": "Transition", "values": ["IDLE"] }
                ]
            }
        ]
    });
    let weapon = WeaponMeleeSpec::default();
    let m = ComboMachine::from_json_value(&doc, &weapon, COMBO_ANIM_FPS, |_| None).unwrap();
    let short_cut = m.find_state("SHORT_CUT").unwrap();
    let follow_cut = m.find_state("FOLLOW_CUT").unwrap();

    // Two invented names sharing low-20-bit hash 0xea4fd must map to the same group.
    assert_eq!(
        m.state(short_cut).attacks[0].attack_group,
        m.state(follow_cut).attacks[0].attack_group,
        "AttackIds with identical nonzero low-20-bit FNV-1a hash must share one hit group"
    );

    // Hold a stationary intersecting sphere across the full SHORT_CUT -> FOLLOW_CUT -> READY span.
    let shapes = [sphere_shape([0.0, 1.0, 0.8], 0.6)];
    let target = MeleeTarget {
        target_id: TargetId::Life(77),
        position: [0.0, 0.0, 0.8],
        aim_yaw: 0.0,
        health_scale: HealthScale::Person,
        shapes: &shapes,
        deflect: None,
    };
    let seg = BladeSegment {
        base: [0.0, 1.0, 0.0],
        tip: [0.0, 1.0, 1.4],
    };

    let tuning = test_move_tuning();
    let body = stand_body();
    let mut st = ComboState::new(&m);
    let mut energy = move2::State::new(&tuning);
    let mut hit_count = 0usize;

    for tick in 0..16 {
        let inp = if tick == 0 {
            ComboInput::default().press(ComboButton::Fire)
        } else {
            ComboInput::default()
        };
        let out = step(
            GameFamily::Swbf2,
            &m,
            &mut st,
            &inp,
            &body,
            &mut energy,
            &tuning,
            true,
            DT,
        );
        for active in out.active_attacks.as_slice() {
            let atk = m.state(active.state_id).attacks[active.attack_index as usize];
            let sweep = BladeSweep::from_attack(seg, seg, &atk);
            let res = resolve_melee_hit([0.0; 3], &atk, &weapon, &sweep, &mut st.hit_history, &target, None);
            if matches!(res, MeleeHitOutcome::Hit { .. }) {
                hit_count += 1;
            }
        }
    }
    assert_eq!(hit_count, 1, "stationary target must be hit exactly once across SHORT_CUT -> FOLLOW_CUT");

    // Verify per-attack 8-target capacity does not suppress an unrelated attack,
    // blocked contacts are recorded, and rebased lists retire after HIT_LIST_LINGER_SECS (0.30 s).
    let atk_a = m.state(short_cut).attacks[0];
    let atk_b = AttackDef {
        attack_group: AttackGroupId(9),
        attack_key: pack_attack_key(StateId(9), 0),
        shared_id: 0x0001_2345,
        ..atk_a
    };
    let mut hist = HitHistory::new();
    hist.ensure_list(atk_a.attack_key, atk_a.shared_id, 0.10);
    for id in 1..=(MAX_TARGETS_PER_ATTACK as u32) {
        assert!(hist.record(atk_a.attack_key, atk_a.shared_id, 0.10, TargetId::Life(id)));
    }
    assert!(!hist.record(atk_a.attack_key, atk_a.shared_id, 0.10, TargetId::Life(999)), "9th target in same list is rejected");
    assert!(hist.record(atk_b.attack_key, atk_b.shared_id, 0.10, TargetId::Life(999)), "unrelated attack group still records hits");

    // Blocked contact must be recorded so a repeat sweep returns AlreadyHit.
    let mut block_hist = HitHistory::new();
    let deflect = DeflectDef {
        min_angle_deg: -90.0,
        max_angle_deg: 90.0,
        energy_cost: 1.0,
        deflect_anim_start: 0,
        deflect_anim_len: 0,
        reaction_ids: [u16::MAX;6],
    };
    let blocking_target = MeleeTarget {
        target_id: TargetId::Life(500),
        position: [0.0, 0.0, 0.8],
        aim_yaw: std::f64::consts::PI,
        health_scale: HealthScale::Person,
        shapes: &shapes,
        deflect: Some(deflect),
    };
    let mut def_e = 20.0;
    let sweep = BladeSweep::from_attack(seg, seg, &atk_a);
    assert!(matches!(
        resolve_melee_hit([0.0; 3], &atk_a, &weapon, &sweep, &mut block_hist, &blocking_target, Some(&mut def_e)),
        MeleeHitOutcome::Blocked { .. }
    ));
    assert_eq!(
        resolve_melee_hit([0.0; 3], &atk_a, &weapon, &sweep, &mut block_hist, &blocking_target, Some(&mut def_e)),
        MeleeHitOutcome::AlreadyHit,
        "blocked contact must be recorded in the hit list"
    );

    // Rebase and retire check: survives just under 0.30 s, retires at 0.30 s.
    hist.rebase(0.10, 0.10); // expires_at becomes 0.0
    hist.retire(0.29);
    assert!(hist.has_hit(atk_a.attack_key, atk_a.shared_id, TargetId::Life(1)));
    hist.retire(0.30 + 1e-6);
    assert!(!hist.has_hit(atk_a.attack_key, atk_a.shared_id, TargetId::Life(1)));
    assert!(hist.is_empty());
}

#[test]
fn transitions_use_event_ring_earliest_match_continuation_grace_priority_and_expiry() {
    let weapon = WeaponMeleeSpec::default();
    let tuning = test_move_tuning();
    let body = stand_body();

    // Earliest match, authored ties and an optional visible bridge frame.
    {
        let doc = json!({
            "nameHash": "00007702",
            "states": [
                {
                    "name": "IDLE",
                    "children": [
                        { "key": "Duration", "values": [0.0] },
                        { "key": "ThrustFactor", "values": [0.97] },
                        { "key": "StrafeFactor", "values": [0.93] },
                        { "key": "TurnFactor", "values": [0.91] },
                        { "key": "EnergyRestoreRate", "values": [0.0] },
                        {
                            "key": "Transition",
                            "values": ["WINDOW_PICK"],
                            "children": [{
                                "key": "If",
                                "children": [
                                    { "key": "Break", "values": [] },
                                    { "key": "Button", "values": ["Fire", "Press"] }
                                ]
                            }]
                        },
                        {
                            "key": "Transition",
                            "values": ["FAST_BRIDGE"],
                            "children": [{
                                "key": "If",
                                "children": [
                                    { "key": "Break", "values": [] },
                                    { "key": "Button", "values": ["Jump", "Press"] }
                                ]
                            }]
                        },
                        {
                            "key": "Transition",
                            "values": ["HELD_BRIDGE"],
                            "children": [{
                                "key": "If",
                                "children": [
                                    { "key": "Break", "values": [] },
                                    { "key": "Button", "values": ["Crouch", "Press"] }
                                ]
                            }]
                        }
                    ]
                },
                {
                    "name": "WINDOW_PICK",
                    "children": [
                        { "key": "Duration", "values": [0.50] },
                        {
                            "key": "Transition",
                            "values": ["PICK_JUMP"],
                            "children": [{
                                "key": "If",
                                "children": [{ "key": "Button", "values": ["Jump", "Press"] }]
                            }]
                        },
                        {
                            "key": "Transition",
                            "values": ["PICK_FIRE"],
                            "children": [{
                                "key": "If",
                                "children": [{ "key": "Button", "values": ["Fire", "Press"] }]
                            }]
                        }
                    ]
                },
                {
                    "name": "PICK_JUMP",
                    "children": [{ "key": "Duration", "values": [0.0] }]
                },
                {
                    "name": "PICK_FIRE",
                    "children": [{ "key": "Duration", "values": [0.0] }]
                },
                {
                    "name": "FAST_BRIDGE",
                    "children": [
                        { "key": "Transition", "values": ["LANDING_STATE"] }
                    ]
                },
                {
                    "name": "HELD_BRIDGE",
                    "children": [
                        { "key": "MustShowOneFrame", "values": [] },
                        { "key": "Transition", "values": ["LANDING_STATE"] }
                    ]
                },
                {
                    "name": "LANDING_STATE",
                    "children": [{ "key": "Duration", "values": [0.0] }]
                }
            ]
        });
        let m = ComboMachine::from_json_value(&doc, &weapon, COMBO_ANIM_FPS, |_| None).unwrap();
        let pick_fire = m.find_state("PICK_FIRE").unwrap();
        let pick_jump = m.find_state("PICK_JUMP").unwrap();
        let held_bridge = m.find_state("HELD_BRIDGE").unwrap();
        let landing = m.find_state("LANDING_STATE").unwrap();

        // Fire pressed at tick 5 beats Jump pressed at tick 10 even though PICK_JUMP is authored first.
        let mut st = ComboState::new(&m);
        let mut e = move2::State::new(&tuning);
        step(GameFamily::Swbf2, &m, &mut st, &ComboInput::default().press(ComboButton::Fire), &body, &mut e, &tuning, true, DT);
        for tick in 1..=32 {
            let inp = match tick {
                5 => ComboInput::default().press(ComboButton::Fire),
                10 => ComboInput::default().press(ComboButton::Jump),
                _ => ComboInput::default(),
            };
            step(GameFamily::Swbf2, &m, &mut st, &inp, &body, &mut e, &tuning, true, DT);
        }
        assert_eq!(st.state, pick_fire, "earliest timestamp (Fire at tick 5) must beat later Jump at tick 10");

        // Tie on the same tick breaks by authored order (PICK_JUMP before PICK_FIRE).
        let mut st_tie = ComboState::new(&m);
        step(GameFamily::Swbf2, &m, &mut st_tie, &ComboInput::default().press(ComboButton::Fire), &body, &mut e, &tuning, true, DT);
        for tick in 1..=32 {
            let inp = if tick == 5 {
                ComboInput::default().press(ComboButton::Fire).press(ComboButton::Jump)
            } else {
                ComboInput::default()
            };
            step(GameFamily::Swbf2, &m, &mut st_tie, &inp, &body, &mut e, &tuning, true, DT);
        }
        assert_eq!(st_tie.state, pick_jump, "same-tick tie must break by authored transition order");

        // Zero-duration bridge without MustShowOneFrame settles to LANDING_STATE on tick 0 (LAST, LAST, LAST).
        let mut st_fast = ComboState::new(&m);
        let mut seq_fast = Vec::new();
        for tick in 0..3 {
            let inp = if tick == 0 {
                ComboInput::default().press(ComboButton::Jump)
            } else {
                ComboInput::default()
            };
            step(GameFamily::Swbf2, &m, &mut st_fast, &inp, &body, &mut e, &tuning, true, DT);
            seq_fast.push(st_fast.state);
        }
        assert_eq!(seq_fast, vec![landing, landing, landing], "zero-duration bridge without MustShowOneFrame continues same-tick");

        // Zero-duration bridge WITH MustShowOneFrame shows HELD_BRIDGE once then LANDING_STATE (BRIDGE, LAST, LAST).
        let mut st_held = ComboState::new(&m);
        let mut seq_held = Vec::new();
        for tick in 0..3 {
            let inp = if tick == 0 {
                ComboInput::default().press(ComboButton::Crouch)
            } else {
                ComboInput::default()
            };
            step(GameFamily::Swbf2, &m, &mut st_held, &inp, &body, &mut e, &tuning, true, DT);
            seq_held.push(st_held.state);
        }
        assert_eq!(seq_held, vec![held_bridge, landing, landing], "MustShowOneFrame stops same-tick continuation for one tick");
    }

    // 3. Carry-over (grace_state) evaluated BEFORE current-state interrupts + no-match expiry rules.
    {
        let doc = json!({
            "nameHash": "00007703",
            "states": [
                {
                    "name": "ZERO_ANCHOR",
                    "children": [{ "key": "Duration", "values": [0.0] }]
                },
                {
                    "name": "IDLE",
                    "children": [
                        { "key": "Duration", "values": [0.0] },
                        { "key": "ThrustFactor", "values": [0.97] },
                        { "key": "StrafeFactor", "values": [0.93] },
                        { "key": "TurnFactor", "values": [0.91] },
                        { "key": "EnergyRestoreRate", "values": [0.0] },
                        {
                            "key": "Transition",
                            "values": ["LEAD_SWING"],
                            "children": [{
                                "key": "If",
                                "children": [
                                    { "key": "Break", "values": [] },
                                    { "key": "Button", "values": ["Fire", "Press"] }
                                ]
                            }]
                        },
                        {
                            "key": "Transition",
                            "values": ["COND_ONLY"],
                            "children": [{
                                "key": "If",
                                "children": [
                                    { "key": "Break", "values": [] },
                                    { "key": "Button", "values": ["Jump", "Press"] }
                                ]
                            }]
                        },
                        {
                            "key": "Transition",
                            "values": ["EMPTY_FINITE"],
                            "children": [{
                                "key": "If",
                                "children": [
                                    { "key": "Break", "values": [] },
                                    { "key": "Button", "values": ["Crouch", "Press"] }
                                ]
                            }]
                        }
                    ]
                },
                {
                    "name": "LEAD_SWING",
                    "children": [
                        { "key": "Duration", "values": [0.10] },
                        {
                            "key": "Transition",
                            "values": ["GRACE_DEST"],
                            "children": [{
                                "key": "If",
                                "children": [
                                    { "key": "TimeStart", "values": [0.0] },
                                    { "key": "TimeEnd", "values": [0.20, "FromEnd"] },
                                    { "key": "Button", "values": ["Fire", "Press"] }
                                ]
                            }]
                        },
                        { "key": "Transition", "values": ["COOLDOWN"] }
                    ]
                },
                {
                    "name": "COOLDOWN",
                    "children": [
                        { "key": "Duration", "values": [0.0] },
                        {
                            "key": "Transition",
                            "values": ["COOLDOWN_INTERRUPT"],
                            "children": [{
                                "key": "If",
                                "children": [
                                    { "key": "Break", "values": [] },
                                    { "key": "Button", "values": ["Fire", "Press"] }
                                ]
                            }]
                        }
                    ]
                },
                { "name": "GRACE_DEST", "children": [{ "key": "Duration", "values": [0.0] }] },
                { "name": "COOLDOWN_INTERRUPT", "children": [{ "key": "Duration", "values": [0.0] }] },
                {
                    "name": "COND_ONLY",
                    "children": [
                        { "key": "Duration", "values": [0.10] },
                        {
                            "key": "Transition",
                            "values": ["FIRST_OPT"],
                            "children": [{
                                "key": "If",
                                "children": [{ "key": "Button", "values": ["Fire", "Press"] }]
                            }]
                        },
                        {
                            "key": "Transition",
                            "values": ["LAST_OPT"],
                            "children": [
                                { "key": "EnergyCost", "values": [15.0] },
                                {
                                    "key": "If",
                                    "children": [{ "key": "Button", "values": ["Jump", "Press"] }]
                                }
                            ]
                        }
                    ]
                },
                { "name": "FIRST_OPT", "children": [{ "key": "Duration", "values": [0.0] }] },
                { "name": "LAST_OPT", "children": [{ "key": "Duration", "values": [0.0] }] },
                {
                    "name": "EMPTY_FINITE",
                    "children": [{ "key": "Duration", "values": [0.10] }]
                }
            ]
        });
        let m = ComboMachine::from_json_value(&doc, &weapon, COMBO_ANIM_FPS, |_| None).unwrap();
        let grace_dest = m.find_state("GRACE_DEST").unwrap();
        let last_opt = m.find_state("LAST_OPT").unwrap();


        // Carry-over priority: LEAD_SWING expires into COOLDOWN; pressing Fire inside grace window must pick GRACE_DEST over COOLDOWN_INTERRUPT.
        let mut st = ComboState::new(&m);
        let mut e = move2::State::new(&tuning);
        step(GameFamily::Swbf2, &m, &mut st, &ComboInput::default().press(ComboButton::Fire), &body, &mut e, &tuning, true, DT);
        for _ in 0..8 {
            step(GameFamily::Swbf2, &m, &mut st, &ComboInput::default(), &body, &mut e, &tuning, true, DT);
        }
        step(GameFamily::Swbf2, &m, &mut st, &ComboInput::default().press(ComboButton::Fire), &body, &mut e, &tuning, true, DT);
        assert_eq!(st.state, grace_dest, "carried-over grace_state must be evaluated before current-state Break interrupts");

        // No-match expiry with conditional transitions takes last transition target (LAST_OPT) with 0 energy cost.
        let mut st_cond = ComboState::new(&m);
        let mut e_cond = move2::State::new(&tuning);
        e_cond.energy = 40.0;
        step(GameFamily::Swbf2, &m, &mut st_cond, &ComboInput::default().press(ComboButton::Jump), &body, &mut e_cond, &tuning, false, DT);
        for _ in 0..8 {
            step(GameFamily::Swbf2, &m, &mut st_cond, &ComboInput::default(), &body, &mut e_cond, &tuning, false, DT);
        }
        assert_eq!(st_cond.state, last_opt, "no-match expiry must fall back to last stored transition target");
        assert_eq!(e_cond.energy, 40.0, "no-match expiry fallback must not charge the unmatched transition's EnergyCost");

        // Empty states return to neutral regardless of file order.
        let mut st_empty = ComboState::new(&m);
        step(GameFamily::Swbf2, &m, &mut st_empty, &ComboInput::default().press(ComboButton::Crouch), &body, &mut e, &tuning, false, DT);
        for _ in 0..8 {
            step(GameFamily::Swbf2, &m, &mut st_empty, &ComboInput::default(), &body, &mut e, &tuning, false, DT);
        }
        assert_eq!(st_empty.state, m.idle_state);
    }
}

#[test]
fn timing_quantization_strict_types_package_banks_and_loader_validation() {
    let weapon = WeaponMeleeSpec {
        combo_hash: 0x0000_aaaa,
        num_edges: 1,
        edges: [
            EdgeSpec {
                default_length: 1.25,
                default_width: 0.30,
            },
            EdgeSpec::default(),
            EdgeSpec::default(),
            EdgeSpec::default(),
        ],
        ..WeaponMeleeSpec::default()
    };

    let candidates = [
        ClipBindingCandidate { bank: "reed", frames: 97, fps: 30.0 },
        ClipBindingCandidate { bank: "copper", frames: 38, fps: 30.0 },
        ClipBindingCandidate { bank: "pine", frames: 63, fps: 30.0 },
    ];
    assert_eq!(
        resolve_binding_clip_duration(Some("copper"), &["pine", "copper"], &candidates),
        Some(37.0 / 30.0)
    );
    assert_eq!(
        resolve_binding_clip_duration(Some("copper"), &["pine"], &candidates),
        Some(62.0 / 30.0)
    );
    assert_eq!(resolve_binding_clip_duration(Some("reed"), &["larch"], &candidates), None);

    // 2. Fixed 30 Hz centisecond truncation & bounds + strict JSON types + signed FromEdge.
    let doc = json!({
        "nameHash": "0000aaaa",
        "animations": [{ "values": ["inv_clip"], "duration": 0.60 }],
        "states": [
            {
                "name": "IDLE",
                "children": [
                    { "key": "Duration", "values": [25.0] },
                    { "key": "Animation", "values": ["inv_clip"] },
                    {
                        "key": "Attack",
                        "children": [
                            { "key": "DamageTime", "values": [1, 7, "Frames"] },
                            { "key": "DamageLength", "values": [-0.5] },
                            { "key": "DamageWidth", "values": [1.5, "FromEdge"] },
                            { "key": "Damage", "values": [180.0] }
                        ]
                    },
                    {
                        "key": "Transition",
                        "values": ["IDLE"],
                        "children": [{
                            "key": "If",
                            "children": [
                                { "key": "Break", "values": [34, "Frames"] },
                                { "key": "TimeStart", "values": ["-7", "Frames"] },
                                { "key": "TimeEnd", "values": [900, "Frames"] }
                            ]
                        }]
                    },
                    {
                        "key": "Transition",
                        "values": ["IDLE"],
                        "children": [{
                            "key": "If",
                            "children": [
                                { "key": "TimeStart", "values": [-900, "Frames"] }
                            ]
                        }]
                    }
                ]
            }
        ]
    });
    let m = ComboMachine::from_json_value(&doc, &weapon, 60.0, |_| None).unwrap();
    let st = m.state(StateId(0));
    assert_eq!(st.duration, StateDuration::Seconds(10.22), "Duration clamps to 10.22 s");
    assert_eq!(
        st.transitions[0].branches[0].break_rule,
        Some(BreakRule::AfterSeconds(1.13)),
        "the frame delay truncates to whole centiseconds at fixed 30 Hz"
    );
    assert_eq!(
        st.transitions[0].branches[0].time_start,
        Some(TimePoint::FromStart(0.0)),
        "JSON string in TimeStart slot 0 must read as 0.0, not parse string to number"
    );
    assert_eq!(
        st.transitions[0].branches[0].time_end,
        Some(TimePoint::FromStart(5.11)),
        "TimeEnd(900, Frames) must clamp to 5.11 s"
    );
    assert_eq!(
        st.transitions[1].branches[0].time_start,
        Some(TimePoint::FromStart(-5.12)),
        "TimeStart(-900, Frames) must clamp to -5.12 s"
    );
    assert!((st.attacks[0].damage_length - 0.625).abs() < 1e-9, "negative bare DamageLength(-0.5) scales edge length (0.5 * 1.25 = 0.625)");
    assert!((st.attacks[0].damage_width - 0.45).abs() < 1e-9, "FromEdge scales edge width (1.5 * 0.30 = 0.45)");

    // 3. Reject JSON strings in numeric fields, non-positive FromEdge, non-array children/values,
    //    Edge >= num_edges, clipless/durationless FromAnim, conflicting nameHash, bad deflect children.
    let bad_cases = [
        json!({
            "nameHash": "0000aaaa",
            "states": [{ "name": "IDLE", "children": [{ "key": "Duration", "values": ["0.5"] }] }]
        }),
        json!({
            "nameHash": "0000aaaa",
            "states": [{ "name": "IDLE", "children": "not-an-array" }]
        }),
        json!({
            "nameHash": "0000aaaa",
            "states": [{ "name": "IDLE", "children": [{ "key": "Duration", "values": 1.0 }] }]
        }),
        json!({
            "nameHash": "0000aaaa",
            "states": [{
                "name": "IDLE",
                "children": [
                    { "key": "Duration", "values": [0.5] },
                    { "key": "Attack", "children": [
                        { "key": "Edge", "values": [1] },
                        { "key": "DamageTime", "values": [0.0, 0.2] }
                    ]}
                ]
            }]
        }),
        json!({
            "nameHash": "0000aaaa",
            "states": [{
                "name": "IDLE",
                "children": [
                    { "key": "Duration", "values": [0.5] },
                    { "key": "Attack", "children": [
                        { "key": "DamageTime", "values": [0.0, 0.2] },
                        { "key": "DamageLength", "values": [-1.25, "FromEdge"] }
                    ]}
                ]
            }]
        }),
        json!({
            "nameHash": "0000aaaa",
            "states": [{
                "name": "IDLE",
                "children": [
                    { "key": "Duration", "values": [0.0] },
                    { "key": "Attack", "children": [
                        { "key": "DamageTime", "values": [0.2, 0.8, "FromAnim"] }
                    ]}
                ]
            }]
        }),
        json!({
            "nameHash": "0000aaaa",
            "states": [{
                "name": "IDLE",
                "children": [{
                    "key": "Deflect",
                    "children": [
                        { "key": "DeflectAngle", "values": [-90, 90] },
                        { "key": "DeflectAnimation", "values": ["inv_def", "Forward"], "children": "bad" }
                    ]
                }]
            }]
        }),
    ];
    for (i, bad) in bad_cases.iter().enumerate() {
        assert!(
            ComboMachine::from_json_value(bad, &weapon, COMBO_ANIM_FPS, |_| None).is_err(),
            "malformed combo case {i} must be rejected"
        );
    }

    // Conflicting outer key vs inner nameHash must fail in load_combo_by_hash.
    let conflict_json = r#"{
        "0000aaaa": {
            "nameHash": "0000bbbb",
            "states": [{ "name": "IDLE", "children": [{ "key": "Duration", "values": [0.0] }] }]
        }
    }"#;
    assert!(
        ComboMachine::load_combo_by_hash(conflict_json, &weapon, COMBO_ANIM_FPS, |_| None).is_err(),
        "conflicting outer hash key and inner nameHash must be rejected"
    );
}

#[test]
fn unwrapped_state_clock_resolves_from_anim_point_windows_and_explicit_loop_expiry() {
    let doc = json!({
        "nameHash": "00008801",
        "animations": [
            { "values": ["inv_one_sec"], "duration": 1.0 },
            { "values": ["inv_wrap_clip"], "duration": 1.0, "children": [{ "key": "Loop", "values": [] }] },
            { "values": ["inv_hold_clip"], "duration": 1.0, "children": [{ "key": "Loop", "values": ["FinalFrame"] }] },
            { "values": ["inv_half_hold"], "duration": 0.5, "children": [{ "key": "Loop", "values": ["FinalFrame"] }] }
        ],
        "states": [
            {
                "name": "IDLE",
                "children": [
                    { "key": "Duration", "values": [0.0] },
                    { "key": "ThrustFactor", "values": [0.97] },
                    { "key": "StrafeFactor", "values": [0.93] },
                    { "key": "TurnFactor", "values": [0.91] },
                    { "key": "EnergyRestoreRate", "values": [17.5] },
                    {
                        "key": "Transition",
                        "values": ["SHORT_FROM_ANIM"],
                        "children": [{ "key": "If", "children": [
                            { "key": "Break" },
                            { "key": "Button", "values": ["Fire", "Press"] }
                        ]}]
                    },
                    {
                        "key": "Transition",
                        "values": ["FINITE_WRAP"],
                        "children": [{ "key": "If", "children": [
                            { "key": "Break" },
                            { "key": "Button", "values": ["Jump", "Press"] }
                        ]}]
                    },
                    {
                        "key": "Transition",
                        "values": ["FINITE_HOLD"],
                        "children": [{ "key": "If", "children": [
                            { "key": "Break" },
                            { "key": "Button", "values": ["Crouch", "Press"] }
                        ]}]
                    },
                    {
                        "key": "Transition",
                        "values": ["INDEF_HOLD"],
                        "children": [{ "key": "If", "children": [
                            { "key": "Break" },
                            { "key": "Button", "values": ["Reload", "Press"] }
                        ]}]
                    }
                ]
            },
            {
                "name": "SHORT_FROM_ANIM",
                "children": [
                    { "key": "Animation", "values": ["inv_one_sec"] },
                    { "key": "Duration", "values": [0.50] },
                    { "key": "ThrustFactor", "values": [0.84] },
                    { "key": "StrafeFactor", "values": [0.63] },
                    { "key": "TurnFactor", "values": [0.77] },
                    {
                        "key": "Attack",
                        "children": [
                            { "key": "DamageTime", "values": [0.5, 1.0, "FromAnim"] },
                            { "key": "Damage", "values": [120.0] }
                        ]
                    },
                    {
                        "key": "Attack",
                        "children": [
                            { "key": "DamageTime", "values": [0.20, 0.20, "Seconds"] },
                            { "key": "Damage", "values": [90.0] }
                        ]
                    },
                    { "key": "Transition", "values": ["IDLE"] }
                ]
            },
            {
                "name": "FINITE_WRAP",
                "children": [
                    { "key": "Animation", "values": ["inv_wrap_clip"] },
                    { "key": "Duration", "values": [1.50] },
                    { "key": "TurnFactor", "values": [0.74] },
                    { "key": "Transition", "values": ["IDLE"] }
                ]
            },
            {
                "name": "FINITE_HOLD",
                "children": [
                    { "key": "Animation", "values": ["inv_hold_clip"] },
                    { "key": "Duration", "values": [1.50] },
                    { "key": "TurnFactor", "values": [0.73] },
                    { "key": "Transition", "values": ["IDLE"] }
                ]
            },
            {
                "name": "INDEF_HOLD",
                "children": [
                    { "key": "Animation", "values": ["inv_half_hold"] },
                    { "key": "ThrustFactor", "values": [0.82] },
                    { "key": "TurnFactor", "values": [0.71] },
                    {
                        "key": "Attack",
                        "children": [
                            { "key": "DamageTime", "values": [0.5, 1.0, "FromAnim"] },
                            { "key": "Damage", "values": [150.0] }
                        ]
                    },
                    { "key": "Transition", "values": ["IDLE"] }
                ]
            }
        ]
    });
    let weapon = WeaponMeleeSpec::default();
    let m = ComboMachine::from_json_value(&doc, &weapon, COMBO_ANIM_FPS, |_| None).unwrap();
    let neutral = m.find_state("IDLE").unwrap();
    let indef_hold = m.find_state("INDEF_HOLD").unwrap();
    let tuning = test_move_tuning();
    let body = stand_body();

    // 1. Explicit 0.50 s state on 1.0 s clip: FromAnim(0.5, 1.0) fires in [0.25, 0.50],
    //    and zero-width point window [0.20, 0.20] fires cleanly on its crossing tick.
    {
        let mut st = ComboState::new(&m);
        let mut e = move2::State::new(&tuning);
        step(GameFamily::Swbf2, &m, &mut st, &ComboInput::default().press(ComboButton::Fire), &body, &mut e, &tuning, true, DT);
        let mut from_anim_ticks = 0usize;
        let mut point_ticks = 0usize;
        for _ in 0..32 {
            let out = step(GameFamily::Swbf2, &m, &mut st, &ComboInput::default(), &body, &mut e, &tuning, true, DT);
            for a in out.active_attacks.as_slice() {
                assert!(a.window_progress_prev.is_finite() && a.window_progress_curr.is_finite());
                if a.damage == 120.0 {
                    from_anim_ticks += 1;
                } else if a.damage == 90.0 {
                    point_ticks += 1;
                }
            }
        }
        assert!(
            from_anim_ticks >= 14,
            "FromAnim(0.5, 1.0) on explicit 0.50 s state must emit second-half ticks, got {from_anim_ticks}"
        );
        assert_eq!(point_ticks, 1, "zero-width point window [0.20, 0.20] must trigger once on crossing tick");
        assert_eq!(st.state, neutral);
    }

    // 2. Explicit 1.50 s Wrap state on 1.0 s clip must finish at 1.50 s (90 ticks), not wedge!
    {
        let mut st = ComboState::new(&m);
        let mut e = move2::State::new(&tuning);
        step(GameFamily::Swbf2, &m, &mut st, &ComboInput::default().press(ComboButton::Jump), &body, &mut e, &tuning, true, DT);
        let mut saw_wrapped = false;
        for tick in 1..=95 {
            step(GameFamily::Swbf2, &m, &mut st, &ComboInput::default(), &body, &mut e, &tuning, true, DT);
            if tick == 75 {
                assert!(st.anim_time < 0.5, "rendered anim_time wraps inside 1.0 s clip, got {}", st.anim_time);
                saw_wrapped = true;
            }
        }
        assert!(saw_wrapped);
        assert_eq!(st.state, neutral, "explicit-duration Wrap state must finish at 1.50 s");
    }

    // 3. Explicit 1.50 s FinalFrame state on 1.0 s clip must finish at 1.50 s (90 ticks), not wedge!
    {
        let mut st = ComboState::new(&m);
        let mut e = move2::State::new(&tuning);
        step(GameFamily::Swbf2, &m, &mut st, &ComboInput::default().press(ComboButton::Crouch), &body, &mut e, &tuning, true, DT);
        for tick in 1..=95 {
            step(GameFamily::Swbf2, &m, &mut st, &ComboInput::default(), &body, &mut e, &tuning, true, DT);
            if tick == 75 {
                assert!((st.anim_time - 1.0).abs() < 1e-6, "rendered anim_time clamps at 1.0 s clip end");
            }
        }
        assert_eq!(st.state, neutral, "explicit-duration FinalFrame state must finish at 1.50 s");
    }

    // 4. Indefinite FinalFrame state (ClipLength) stays active and keeps its attack live past clip end.
    {
        let mut st = ComboState::new(&m);
        let mut e = move2::State::new(&tuning);
        step(GameFamily::Swbf2, &m, &mut st, &ComboInput::default().press(ComboButton::Reload), &body, &mut e, &tuning, true, DT);
        let mut late_active = 0usize;
        for tick in 1..=100 {
            let out = step(GameFamily::Swbf2, &m, &mut st, &ComboInput::default(), &body, &mut e, &tuning, true, DT);
            assert_eq!(st.state, indef_hold);
            if tick >= 50 && !out.active_attacks.is_empty() {
                late_active += 1;
            }
        }
        assert_eq!(late_active, 51, "indefinite FinalFrame attack stays live past clip end");
    }
}

pub(super) fn flat_move_world() -> (u32, crate::collision::State) {
    use bfe_content::{
        bvh::{build, Positions},
        collision::{triangle_bounds, MapCollision, TriangleGrid},
    };
    let floor = vec![
        -40.0f32, 0.0, -40.0, 40.0, 0.0, -40.0, 40.0, 0.0, 40.0, -40.0, 0.0, 40.0,
    ];
    let positions = vec![
        -6.0f32, -100.0, -6.0, 6.0, -100.0, -6.0, 6.0, -100.0, 6.0, -6.0, -100.0, 6.0,
    ];
    let indices = vec![0u32, 2, 1, 0, 3, 2];
    let buildings = build(Positions::F32(&positions), &indices);
    let terrain = build(Positions::F32(&floor), &indices);
    let map = MapCollision {
        tri_bounds: triangle_bounds(&positions, &indices),
        grid: Some(TriangleGrid::new(&floor, &indices)),
        positions: positions.into(),
        indices: indices.clone().into(),
        polygons: vec![0, 1].into(),
        masks: Vec::new().into(),
        has_terrain: true,
        terrain_positions: floor.into(),
        terrain_indices: indices.into(),
        kill_y: -200.0,
    };
    let mut col = crate::collision::CollisionOwner::new();
    let world = col
        .native_install(&map, &buildings, Some(&terrain))
        .unwrap()
        .world;
    (world, col.state)
}

pub(super) fn test_move_tuning() -> move2::Tuning {
    let cs = |forward, strafe, turn| move2::ControlSpeed {
        forward,
        strafe,
        turn,
    };
    move2::Tuning {
        stand: cs(1.0, 1.0, 1.0),
        crouch: cs(0.6, 0.6, 1.0),
        sprint: cs(1.6, 0.5, 0.8),
        jump: cs(0.4, 0.4, 0.6),
        jet: cs(0.8, 0.6, 0.5),
        jetpack: None,
        roll: cs(0.0, 0.0, 0.5),
        tumble: cs(0.0, 0.0, 0.2),
        sprint_accelerate_time: 0.25,
        energy_capacity: 80.0,
        energy_floor: 0.0,
        energy_restore: 10.0,
        energy_restore_idle: 20.0,
        energy_drain_sprint: 16.0,
        energy_min_sprint: 10.0,
        energy_cost_jump: 10.0,
        energy_cost_jump_sprint: None,
        energy_cost_roll: 25.0,
        jump_height: 2.0,
        jump_forward_speed_factor: 1.0,
        jump_strafe_speed_factor: 1.0,
        roll_speed_factor: 1.5,
        roll_frames: 24,
        recover_from_tumble: false,
        getup_front_seconds: move2::DEFAULT_GETUP_FRONT_SECONDS,
        getup_back_seconds: move2::DEFAULT_GETUP_BACK_SECONDS,
    }
}

#[test]
fn mover_hook_applies_same_tick_velocity_ramp_locks_authored_jump_and_single_stamina() {
    use crate::collision::spheres::{GatherCache, Gathered};
    use crate::movement::scene::{GroundCache, Scene};
    use crate::movement::soldier::{Input as MoveInput, PART_CACHES};

    let doc = json!({
        "nameHash": "00009901",
        "states": [
            {
                "name": "IDLE",
                "children": [
                    { "key": "Duration", "values": [0.0] },
                    { "key": "ThrustFactor", "values": [0.97] },
                    { "key": "StrafeFactor", "values": [0.93] },
                    { "key": "TurnFactor", "values": [0.91] },
                    {
                        "key": "Transition",
                        "values": ["LUNGE_FORWARD"],
                        "children": [{ "key": "If", "children": [
                            { "key": "Break" },
                            { "key": "Button", "values": ["Fire", "Press"] }
                        ]}]
                    },
                    {
                        "key": "Transition",
                        "values": ["LOCKED_WARD"],
                        "children": [{ "key": "If", "children": [
                            { "key": "Break" },
                            { "key": "Button", "values": ["FireSecondary", "Press"] }
                        ]}]
                    },
                    {
                        "key": "Transition",
                        "values": ["AUTHORED_LEAP"],
                        "children": [
                            { "key": "EnergyCost", "values": [12.0] },
                            { "key": "If", "children": [
                                { "key": "Break" },
                                { "key": "Button", "values": ["Reload", "Press"] }
                            ]}
                        ]
                    },
                    {
                        "key": "Transition",
                        "values": ["AUTHORED_ROLL"],
                        "children": [
                            { "key": "EnergyCost", "values": [15.0] },
                            { "key": "If", "children": [
                                { "key": "Break" },
                                { "key": "Button", "values": ["Crouch", "Press"] }
                            ]}
                        ]
                    }
                ]
            },
            {
                "name": "LUNGE_FORWARD",
                "children": [
                    { "key": "Duration", "values": [0.40] },
                    { "key": "TurnFactor", "values": [0.73] },
                    { "key": "StrafeFactor", "values": [0.61] },
                    { "key": "AnimatedMove", "children": [{ "key": "VelocityZ", "values": [12.0] }] },
                    { "key": "Transition", "values": ["IDLE"] }
                ]
            },
            {
                "name": "LOCKED_WARD",
                "children": [
                    { "key": "Duration", "values": [0.0] },
                    { "key": "TurnFactor", "values": [0.75] },
                    { "key": "InputLock", "values": ["All", "!FireSecondary"] },
                    { "key": "EnergyRestoreRate", "values": [-10.0] },
                    {
                        "key": "Transition",
                        "values": ["IDLE"],
                        "children": [{ "key": "If", "children": [
                            { "key": "Break" },
                            { "key": "Button", "values": ["FireSecondary", "Press"] }
                        ]}]
                    }
                ]
            },
            {
                "name": "AUTHORED_LEAP",
                "children": [
                    { "key": "Posture", "values": ["Jump"] },
                    { "key": "TurnFactor", "values": [0.71] },
                    { "key": "StrafeFactor", "values": [0.67] },
                    { "key": "MustShowOneFrame" },
                    { "key": "Transition", "values": ["IDLE"] }
                ]
            },
            {
                "name": "AUTHORED_ROLL",
                "children": [
                    { "key": "Posture", "values": ["Roll"] },
                    { "key": "TurnFactor", "values": [0.72] },
                    { "key": "StrafeFactor", "values": [0.68] },
                    { "key": "MustShowOneFrame" },
                    { "key": "Transition", "values": ["IDLE"] }
                ]
            }
        ]
    });
    let weapon = WeaponMeleeSpec::default();
    let m = ComboMachine::from_json_value(&doc, &weapon, COMBO_ANIM_FPS, |_| None).unwrap();
    let base_tuning = test_move_tuning();
    let (world, mut col) = flat_move_world();

    let make_mover = || {
        let mut mv = Mover::for_family(world as usize, GameFamily::Swbf2);
        mv.grounded = true;
        mv.position = MoveV3::new(0.0, 0.0, 0.0);
        mv
    };

    // 1. Same-tick AnimatedMove (+12 m/s): moves on tick 0, ramps blend over ~0.15 s,
    //    and holds exact 12.0 m/s (0.20 m/tick) with ZERO ground braking once at weight 1.0.
    {
        let mut mv = make_mover();
        let mut mstate = move2::State::new(&base_tuning);
        let mut cst = ComboState::new(&m);
        let mut gathered = Gathered::default();
        let mut caches = std::array::from_fn::<_, PART_CACHES, _>(|_| GatherCache::new(0.25));
        let mut gcache = GroundCache::default();
        let mut scene = Scene {
            s: &mut col,
            world: world as usize,
            flags: &[],
            gathered: &mut gathered,
            caches: &mut caches,
            ground_cache: &mut gcache,
        };
        let (out0, _) = step_soldier_combo_and_move(
            &m,
            &mut cst,
            &ComboInput::default().press(ComboButton::Fire),
            &mut mv,
            &mut mstate,
            &base_tuning,
            &MoveInput::default(),
            move2::Actions::default(),
            &mut scene,
            DT,
        )
        .unwrap();
        assert!(out0.movement.velocity_blend > 0.0 && out0.movement.velocity_blend < 0.2);
        assert!(mv.position.z > 0.0, "tick 0 must displace on the same tick, got {}", mv.position.z);

        let mut prev_z = mv.position.z;
        for tick in 1..=14 {
            let (out, _) = step_soldier_combo_and_move(
                &m,
                &mut cst,
                &ComboInput::default(),
                &mut mv,
                &mut mstate,
                &base_tuning,
                &MoveInput::default(),
                move2::Actions::default(),
                &mut scene,
                DT,
            )
            .unwrap();
            let dz = mv.position.z - prev_z;
            if tick >= 9 {
                assert!((out.movement.velocity_blend - 1.0).abs() < 1e-6);
                assert!(
                    (mv.velocity.z - 12.0).abs() < 1e-6,
                    "full-weight AnimatedMove must hold exact 12.0 m/s without ground braking, got {} at tick {tick}",
                    mv.velocity.z
                );
                assert!(
                    (dz - 12.0 * DT).abs() < 1e-6,
                    "full-weight AnimatedMove displacement must be 0.20 m/tick, got {dz}"
                );
            }
            prev_z = mv.position.z;
        }
    }

    // 2. Entry-tick InputLock blocks raw jump & movement on the real 2005 mover, and
    //    EnergyRestoreRate(-10) drains without baseline regen double-applying.
    {
        let mut mv = make_mover();
        let mut mstate = move2::State::new(&base_tuning);
        mstate.energy = 50.0;
        let mut cst = ComboState::new(&m);
        let mut gathered = Gathered::default();
        let mut caches = std::array::from_fn::<_, PART_CACHES, _>(|_| GatherCache::new(0.25));
        let mut gcache = GroundCache::default();
        let mut scene = Scene {
            s: &mut col,
            world: world as usize,
            flags: &[],
            gathered: &mut gathered,
            caches: &mut caches,
            ground_cache: &mut gcache,
        };
        let raw_jump_and_run = MoveInput {
            forward: 1.0,
            jump: true,
            ..MoveInput::default()
        };
        let (out0, _) = step_soldier_combo_and_move(
            &m,
            &mut cst,
            &ComboInput::default().press(ComboButton::FireSecondary),
            &mut mv,
            &mut mstate,
            &base_tuning,
            &raw_jump_and_run,
            move2::Actions { sprint: true, roll: true, ai: false },
            &mut scene,
            DT,
        )
        .unwrap();
        assert_eq!(out0.input_lock, InputLockMask::ALL.without(InputLockMask::FIRE_SECONDARY));
        assert!(mv.grounded && mv.air == Air::None, "InputLock(All) must prevent raw jump on entry tick");
        assert_eq!(mv.position.z, 0.0, "InputLock(All) must block forward motion on entry tick");
        assert_eq!(mstate.energy, 50.0, "entry tick of override state suppresses baseline mover regen");

        // Next tick drains strictly -10 * DT with zero baseline mover regen.
        let _ = step_soldier_combo_and_move(
            &m,
            &mut cst,
            &ComboInput::default(),
            &mut mv,
            &mut mstate,
            &base_tuning,
            &raw_jump_and_run,
            move2::Actions { sprint: true, roll: true, ai: false },
            &mut scene,
            DT,
        )
        .unwrap();
        assert!(mv.grounded && mv.air == Air::None);
        assert!((mstate.energy - (50.0 - 10.0 * DT)).abs() < 1e-9);
    }

    // 3. Paid authored Posture("Jump") launches Air::Jump on the real 2005 mover and charges stamina once.
    {
        let mut mv = make_mover();
        let mut mstate = move2::State::new(&base_tuning);
        let mut cst = ComboState::new(&m);
        let mut gathered = Gathered::default();
        let mut caches = std::array::from_fn::<_, PART_CACHES, _>(|_| GatherCache::new(0.25));
        let mut gcache = GroundCache::default();
        let mut scene = Scene {
            s: &mut col,
            world: world as usize,
            flags: &[],
            gathered: &mut gathered,
            caches: &mut caches,
            ground_cache: &mut gcache,
        };
        let _ = step_soldier_combo_and_move(
            &m,
            &mut cst,
            &ComboInput::default().press(ComboButton::Reload),
            &mut mv,
            &mut mstate,
            &base_tuning,
            &MoveInput::default(),
            move2::Actions::default(),
            &mut scene,
            DT,
        )
        .unwrap();
        assert_eq!(mv.air, Air::Jump, "authored Posture(Jump) must actually launch the 2005 mover");
        assert!(!mv.grounded);
        assert!(mv.velocity.y > 0.0);
        assert!(
            (mstate.energy - (base_tuning.energy_capacity - 12.0)).abs() < 1e-9,
            "authored Jump must charge Transition EnergyCost once, not also mover jump cost"
        );
    }

    // 4. Paid authored Posture("Roll") starts a real roll and charges stamina once.
    {
        let mut mv = make_mover();
        let mut mstate = move2::State::new(&base_tuning);
        let mut cst = ComboState::new(&m);
        let mut gathered = Gathered::default();
        let mut caches = std::array::from_fn::<_, PART_CACHES, _>(|_| GatherCache::new(0.25));
        let mut gcache = GroundCache::default();
        let mut scene = Scene {
            s: &mut col,
            world: world as usize,
            flags: &[],
            gathered: &mut gathered,
            caches: &mut caches,
            ground_cache: &mut gcache,
        };
        let _ = step_soldier_combo_and_move(
            &m,
            &mut cst,
            &ComboInput::default().press(ComboButton::Crouch).stick(1.0, 0.0),
            &mut mv,
            &mut mstate,
            &base_tuning,
            &MoveInput { forward: 1.0, ..MoveInput::default() },
            move2::Actions::default(),
            &mut scene,
            DT,
        )
        .unwrap();
        assert!(mstate.roll.is_some(), "authored Posture(Roll) must initiate a real mover roll");
        assert!(
            (mstate.energy - (base_tuning.energy_capacity - 15.0)).abs() < 1e-9,
            "authored Roll must charge Transition EnergyCost once, not also mover roll cost"
        );
    }

    // 5. Negative energy_floor + exhausted coordination stays valid for validate_swbf2.
    {
        let mut floor_tuning = base_tuning;
        floor_tuning.energy_floor = -20.0;
        let mut mv = make_mover();
        let mut mstate = move2::State::new(&floor_tuning);
        mstate.energy = 0.05;
        let mut cst = ComboState::new(&m);
        let mut gathered = Gathered::default();
        let mut caches = std::array::from_fn::<_, PART_CACHES, _>(|_| GatherCache::new(0.25));
        let mut gcache = GroundCache::default();
        let mut scene = Scene {
            s: &mut col,
            world: world as usize,
            flags: &[],
            gathered: &mut gathered,
            caches: &mut caches,
            ground_cache: &mut gcache,
        };
        step_soldier_combo_and_move(
            &m,
            &mut cst,
            &ComboInput::default().press(ComboButton::FireSecondary),
            &mut mv,
            &mut mstate,
            &floor_tuning,
            &MoveInput::default(),
            move2::Actions::default(),
            &mut scene,
            DT,
        )
        .unwrap();
        step_soldier_combo_and_move(
            &m,
            &mut cst,
            &ComboInput::default(),
            &mut mv,
            &mut mstate,
            &floor_tuning,
            &MoveInput::default(),
            move2::Actions::default(),
            &mut scene,
            DT,
        )
        .unwrap();
        assert!(mstate.energy < 0.0 && mstate.exhausted, "draining below zero must latch exhausted and respect energy_floor");
        // Exit ward and recover past energy_min_sprint -> clears exhausted.
        step_soldier_combo_and_move(
            &m,
            &mut cst,
            &ComboInput::default().press(ComboButton::FireSecondary),
            &mut mv,
            &mut mstate,
            &floor_tuning,
            &MoveInput::default(),
            move2::Actions::default(),
            &mut scene,
            DT,
        )
        .unwrap();
        for _ in 0..40 {
            step_soldier_combo_and_move(
                &m,
                &mut cst,
                &ComboInput::default(),
                &mut mv,
                &mut mstate,
                &floor_tuning,
                &MoveInput::default(),
                move2::Actions::default(),
                &mut scene,
                DT,
            )
            .unwrap();
        }
        assert!(mstate.energy >= floor_tuning.energy_min_sprint && !mstate.exhausted);
    }
}

#[test]
fn deflection_redirects_in_place_without_expiry_explosion_and_supports_wrapped_arcs() {
    // 1. Deflecting a live bolt with ExplosionExpire redirects IN PLACE with 0 expiry explosions,
    //    and calling twice for the same defender neither double-debits nor spawns a second bolt,
    //    while a second distinct defender can still reflect it back.
    let mut world = BoltTestWorld::new();
    let shooter = world.shooter_unit;
    let defender = world.defender_unit;
    let exp_def = DefId(1);

    let bolt_ref = {
        let mut cx = Cx::new(&mut world);
        match cx.fire_round(
            exp_def,
            [0.0, 1.0, 10.0],
            [0.0, 0.0, -1.0],
            OrdnanceOwner::Unit(shooter),
            None,
            None,
            BeamSource::Null,
            0.0,
            None,
            None,
        ) {
            Some(Shot::Bolt(r)) => r,
            other => panic!("expected bolt, got {other:?}"),
        }
    };

    let deflect_def = DeflectDef {
        min_angle_deg: -110.0,
        max_angle_deg: 110.0,
        energy_cost: 4.0,
        deflect_anim_start: 0,
        deflect_anim_len: 1,
        reaction_ids: [0, u16::MAX, u16::MAX, u16::MAX, u16::MAX, u16::MAX],
    };
    let defender_pose = DefenderPose {
        position: [0.0, 0.0, 0.0],
        aim_yaw: 0.0,
        aim_dir: [0.0, 0.0, 1.0],
    };
    let incoming = IncomingProjectile {
        kind: DeflectableKind::Bolt,
        impact_point: [0.0, 1.0, 0.4],
        incoming_dir: [0.0, 0.0, -1.0],
        shooter_pos: Some([0.0, 1.0, 10.0]),
        ordnance_def: exp_def,
    };

    let mut energy = 20.0;
    for _ in 0..2 {
        let (result, id) = reflect_in_ordnance(&mut Cx::new(&mut world), None,
            OrdnanceOwner::Unit(defender), None, Some(&deflect_def), &defender_pose,
            &mut energy, &incoming, RedirectAim { mode:DeflectAimMode::AimDirection,
                spread_degrees:0.75, projectile_seed: Wide::from_u32(27) });
        assert_eq!((result,id),(DeflectOutcome::InvalidInput,None));
    }
    assert_eq!(energy,20.0);
    assert_eq!(world.store.ordnance.bolts.len(),1);
    world.store.bolt.get_mut(bolt_ref).unwrap().life = 0.73;
    let (out1, shot1) = {
        let mut cx = Cx::new(&mut world);
        reflect_in_ordnance(
            &mut cx,
            Some(Shot::Bolt(bolt_ref)),
            OrdnanceOwner::Unit(defender),
            None,
            Some(&deflect_def),
            &defender_pose,
            &mut energy,
            &incoming,
            RedirectAim {mode:DeflectAimMode::AimDirection, spread_degrees:0.75, projectile_seed: Wide::from_u32(27)},
        )
    };
    assert!(matches!(out1, DeflectOutcome::Deflected { .. }));
    let redirected = world.store.bolt.get(bolt_ref).unwrap();
    assert_eq!(redirected.life,0.73);
    assert_ne!(redirected.direction,defender_pose.aim_dir);
    assert_eq!(shot1, Some(Shot::Bolt(bolt_ref)), "must redirect the live bolt in-place");
    assert_eq!(world.store.ordnance.bolts.len(), 1, "must not spawn a duplicate bolt");
    assert!((energy - 16.0).abs() < 1e-9);

    // Calling reflect_in_ordnance a second time for the SAME defender must reject without debiting.
    let (out_dup, shot_dup) = {
        let mut cx = Cx::new(&mut world);
        reflect_in_ordnance(
            &mut cx,
            Some(Shot::Bolt(bolt_ref)),
            OrdnanceOwner::Unit(defender),
            None,
            Some(&deflect_def),
            &defender_pose,
            &mut energy,
            &incoming,
            RedirectAim {mode:DeflectAimMode::AimDirection, spread_degrees:0.75, projectile_seed: Wide::from_u32(27)},
        )
    };
    assert_eq!(out_dup, DeflectOutcome::InvalidInput, "duplicate reflection for same defender must fail");
    assert!(shot_dup.is_none());
    assert!((energy - 16.0).abs() < 1e-9, "duplicate reflection must not double-charge stamina");
    assert_eq!(world.store.ordnance.bolts.len(), 1);

    // Step ordnance: redirected bolt must fly without firing ExplosionExpire!
    {
        let mut cx = Cx::new(&mut world);
        for _ in 0..8 {
            cx.update(DT);
        }
    }
    assert!(
        world.explosions.is_empty(),
        "in-place deflection must not trigger ExplosionExpire, got {:?}",
        world.explosions
    );
    assert!(!world.hits.is_empty(), "redirected bolt must still strike the shooter");

    // 2. Non-unit incoming_dir (length 3) must be rejected by try_deflect without spending energy.
    let bad_incoming = IncomingProjectile {
        incoming_dir: [0.0, 0.0, -3.0],
        ..incoming
    };
    let mut e_unit = 15.0;
    assert_eq!(
        try_deflect(
            Some(&deflect_def),
            &defender_pose,
            &mut e_unit,
            &bad_incoming,
            DeflectAimMode::AimDirection,
        ),
        DeflectOutcome::InvalidInput
    );
    assert_eq!(e_unit, 15.0);

    // 3. Wrapped DeflectAngle(150, -150) loads cleanly and matches rear arc while rejecting front.
    let wrapped_doc = json!({
        "nameHash": "00009902",
        "offhandAnimations": [{
            "values": ["inv_oh_cast", "FIRE"],
            "children": [{ "key": "AimType", "values": ["FullBody"] }]
        }],
        "states": [{
            "name": "IDLE",
            "children": [
                { "key": "Duration", "values": [0.0] },
                {
                    "key": "Deflect",
                    "children": [
                        { "key": "DeflectAngle", "values": [150.0, -150.0] },
                        { "key": "EnergyCost", "values": [1.0] },
                        {
                            "key": "DeflectAnimation",
                            "values": ["inv_blk_f", "Forward"],
                            "children": [
                                { "key": "BlendInTime", "values": [0.05] },
                                { "key": "BlendOutTime", "values": [0.09] },
                                { "key": "LowResPose", "values": [3, "Frames"] },
                                { "key": "AimType", "values": ["Torso"] }
                            ]
                        }
                    ]
                }
            ]
        }]
    });
    let m = ComboMachine::from_json_value(&wrapped_doc, &WeaponMeleeSpec::default(), COMBO_ANIM_FPS, |_| None)
        .expect("wrapped DeflectAngle(150, -150) must load");
    let rear_def = m.state(StateId(0)).deflect.unwrap();
    let dclips = m.deflect_animations_for(&rear_def);
    assert_eq!(dclips.len(), 1);
    assert!((dclips[0].blend_in - 0.05).abs() < 1e-9);
    assert!((dclips[0].blend_out - 0.09).abs() < 1e-9);
    assert!((dclips[0].low_res_pose_secs.unwrap() - 0.10).abs() < 1e-9);
    assert_eq!(dclips[0].aim_type, Some(AimType::Torso));
    assert_eq!(m.offhand_animations[0].aim_type, Some(AimType::FullBody));

    let rear_shot = IncomingProjectile {
        incoming_dir: [0.0, 0.0, 1.0], // traveling +Z from behind hero facing +Z -> 180 deg!
        ..incoming
    };
    let mut e_rear = 10.0;
    assert!(matches!(
        try_deflect(Some(&rear_def), &defender_pose, &mut e_rear, &rear_shot, DeflectAimMode::AimDirection),
        DeflectOutcome::Deflected { .. }
    ));
    assert!(matches!(
        try_deflect(Some(&rear_def), &defender_pose, &mut e_rear, &incoming, DeflectAimMode::AimDirection),
        DeflectOutcome::OutOfArc { .. }
    ));

    // 4. Second distinct defender can reflect the redirected bolt back (ping-pong),
    //    6 reaction sectors use 22.5 / 67.5 / 135 deg cutoffs with available-clip fallback,
    //    and seeded aim-basis spread is deterministic.
    {
        let mut ping_world = BoltTestWorld::new();
        let u1 = ping_world.shooter_unit;
        let u2 = ping_world.defender_unit;
        let b = {
            let mut cx = Cx::new(&mut ping_world);
            match cx.fire_round(DefId(0), [0.0, 1.0, 8.0], [0.0, 0.0, -1.0], OrdnanceOwner::Unit(u1), None, None, BeamSource::Null, 0.0, None, None) {
                Some(Shot::Bolt(r)) => r,
                _ => panic!("expected bolt"),
            }
        };
        let mut e1 = 20.0;
        let (o1, s1) = {
            let mut cx = Cx::new(&mut ping_world);
            reflect_in_ordnance(&mut cx, Some(Shot::Bolt(b)), OrdnanceOwner::Unit(u2), None, Some(&deflect_def), &defender_pose, &mut e1, &IncomingProjectile { ordnance_def: DefId(0), ..incoming }, RedirectAim {mode:DeflectAimMode::AimDirection, spread_degrees:0.75, projectile_seed: Wide::from_u32(27)})
        };
        assert!(matches!(o1, DeflectOutcome::Deflected { .. }));
        assert_eq!(s1, Some(Shot::Bolt(b)));

        let pose_u1 = DefenderPose {
            position: [0.0, 0.0, 8.0],
            aim_yaw: std::f64::consts::PI,
            aim_dir: [0.0, 0.0, -1.0],
        };
        let ret_incoming = IncomingProjectile {
            impact_point: [0.0, 1.0, 7.6],
            incoming_dir: [0.0, 0.0, 1.0],
            ordnance_def: DefId(0),
            ..incoming
        };
        let mut e2 = 20.0;
        let (o2, s2) = {
            let mut cx = Cx::new(&mut ping_world);
            reflect_in_ordnance(&mut cx, Some(Shot::Bolt(b)), OrdnanceOwner::Unit(u1), None, Some(&deflect_def), &pose_u1, &mut e2, &ret_incoming, RedirectAim {mode:DeflectAimMode::AimDirection, spread_degrees:0.75, projectile_seed: Wide::from_u32(27)})
        };
        assert!(matches!(o2, DeflectOutcome::Deflected { .. }));
        assert_eq!(s2, Some(Shot::Bolt(b)));
        assert_eq!(ping_world.store.ordnance.bolts.len(), 1);
    }

    assert_eq!(classify_direction(0.0), DeflectDirection::Forward);
    assert_eq!(classify_direction(22.5), DeflectDirection::FrontRight);
    assert_eq!(classify_direction(53.0), DeflectDirection::FrontRight);
    assert_eq!(classify_direction(-53.0), DeflectDirection::FrontLeft);
    assert_eq!(classify_direction(90.0), DeflectDirection::Right);
    assert_eq!(classify_direction(-90.0), DeflectDirection::Left);
    assert_eq!(classify_direction(150.0), DeflectDirection::Back);
    assert_eq!(
        select_deflect_direction(53.0, dclips),
        DeflectDirection::Forward,
        "at 53 deg with only Forward authored, must fall back to Forward"
    );

    let mut es1 = 20.0;
    let mut es2 = 20.0;
    let d1 = try_deflect_with_spread(Some(&deflect_def), &defender_pose, &mut es1, &incoming, DeflectAimMode::AimDirection, 5.0, Some(Wide::from_u32(42)));
    let d2 = try_deflect_with_spread(Some(&deflect_def), &defender_pose, &mut es2, &incoming, DeflectAimMode::AimDirection, 5.0, Some(Wide::from_u32(42)));
    let mut es3 = 20.0;
    let d3 = try_deflect_with_spread(Some(&deflect_def), &defender_pose, &mut es3, &incoming, DeflectAimMode::AimDirection, 5.0, Some(Wide::from_u32(99)));
    assert_eq!(d1, d2, "same projectile seed must produce identical aim-basis reflection");
    assert_ne!(d1, d3, "distinct projectile seeds must produce distinct deterministic offsets");
}

#[test]
fn cylinder_intersection_adaptive_samples_strict_restore_and_power_contracts() {
    // 1. Exact upright cylinder hit: segment (0, 1.5, 0) -> (1.5, 0.5, 0) passes
    //    through (0.9, 0.9, 0), strictly inside unit cylinder (R=1, H=1).
    let cyl = cylinder_shape([0.0, 0.0, 0.0], 1.0, 1.0);
    let seg = BladeSegment {
        base: [0.0, 1.5, 0.0],
        tip: [1.5, 0.5, 0.0],
    };
    assert!(
        (BladeSweep { prev: seg, curr: seg, radius: 0.05 }).hit_shape(&cyl).is_some(),
        "segment entering upright cylinder slab must hit (blade_radius=0.05)"
    );
    assert!(
        (BladeSweep { prev: seg, curr: seg, radius: 0.0 }).hit_shape(&cyl).is_some(),
        "segment entering upright cylinder slab must hit (blade_radius=0.0)"
    );
    // Large sloped triangle whose vertices all sit outside the cylinder, slicing its top rim.
    let big_tri = BladeSweep {
        prev: BladeSegment { base: [-3.0, 1.8, -3.0], tip: [3.0, 0.2, -3.0] },
        curr: BladeSegment { base: [-3.0, 1.8, 3.0], tip: [3.0, 0.2, 3.0] },
        radius: 0.0,
    };
    assert!(
        big_tri.hit_shape(&cyl).is_some(),
        "sloped triangle slicing cylinder cap/slab must hit"
    );
    // Clearly disjoint segment outside both cap and radius must miss.
    let miss_seg = BladeSegment {
        base: [1.5, 1.5, 0.0],
        tip: [2.5, 0.5, 0.0],
    };
    assert!(
        (BladeSweep { prev: miss_seg, curr: miss_seg, radius: 0.05 }).hit_shape(&cyl).is_none(),
        "segment clearing cylinder outside slab+radius must miss"
    );

    // 2. Adaptive blade pose sampling: 2-unit blade sweeping -X -> +Z -> +X in one
    //    60 Hz tick misses a sphere at (0, 1.0, 1.8) with single endpoint quad,
    //    but hits via build_damage_samples + resolve_melee_hit_samples.
    let target_sphere = [sphere_shape([0.0, 1.0, 1.8], 0.2)];
    let pose_at = |t: f64| -> ([f64; 3], [f64; 3]) {
        let frac = (t / DT).clamp(0.0, 1.0);
        let theta = std::f64::consts::PI * (1.0 - frac); // PI (-X) -> PI/2 (+Z) -> 0 (+X)
        let (s, c) = v8math::sin_cos(theta);
        ([0.0, 1.0, 0.0], [c, 0.0, s])
    };
    let (p0, d0) = pose_at(0.0);
    let (p1, d1) = pose_at(DT);
    let endpoint_sweep = BladeSweep {
        prev: BladeSegment::from_firepoint(p0, d0, 2.0),
        curr: BladeSegment::from_firepoint(p1, d1, 2.0),
        radius: 0.05,
    };
    assert!(
        endpoint_sweep.hit_shapes(&target_sphere).is_none(),
        "single endpoint quad across -X..+X chord must miss (0, 1.0, 1.8)"
    );

    let samples = build_damage_samples(0.0, DT, 2.0, pose_at);
    assert_eq!(samples.as_slice()[1].time, DT / 4.0);
    assert!(samples.len() >= 4, "fast curved swing must refine samples, got {}", samples.len());
    assert!(samples.len() <= MAX_DAMAGE_SAMPLES);
    assert!((samples.as_slice().last().unwrap().time - DT).abs() < 1e-9, "must clip at end_time");
    let atk = AttackDef {
        attack_group: AttackGroupId(0),
        attack_key: 0,
        shared_id: 0,
        edge: 0,
        window: DamageWindow::Seconds { start: 0.0, end: DT },
        damage_length: 2.0,
        damage_width: 0.10,
        damage: 150.0,
        push: 2.0,
    };
    let target = MeleeTarget {
        target_id: TargetId::Life(88),
        position: [0.0, 0.0, 1.8],
        aim_yaw: std::f64::consts::PI,
        health_scale: HealthScale::Person,
        shapes: &target_sphere,
        deflect: None,
    };
    let mut history = HitHistory::new();
    let outcome = resolve_melee_hit_samples(
        [0.0; 3],
        &atk,
        &WeaponMeleeSpec::default(),
        samples.as_slice(),
        &mut history,
        &target,
        None,
    );
    assert!(
        matches!(outcome, MeleeHitOutcome::Hit { damage, .. } if damage == 150.0),
        "adaptive sample sequence must hit target at (0, 1.0, 1.8), got {outcome:?}"
    );
    // Cap check at MAX_DAMAGE_SAMPLES = 255 for very long fast spin.
    let capped = build_damage_samples(0.0, 4.0, 2.0, |t| {
        let (s, c) = v8math::sin_cos(t * 40.0);
        ([0.0, 1.0, 0.0], [c, 0.0, s])
    });
    assert_eq!(capped.len(), MAX_DAMAGE_SAMPLES, "sample list must cap at 255");

    // 3. Strict ComboState::try_restore rejects inconsistent flags, clocks, blend, and shared_id.
    let m = catch_machine();
    let mut st = ComboState::new(&m);
    let baseline = st;
    for bad_cp in [
        { let mut c = st.checkpoint(); c.deflecting = true; c },
        { let mut c = st.checkpoint(); c.blades_off_mask = 0x70; c },
        { let mut c = st.checkpoint(); c.animated_move_blend = 1.5; c },
        { let mut c = st.checkpoint(); c.state_time = 0.5; c.eval_time = 0.1; c },
        { let mut c = st.checkpoint(); c.state_origin_time = 2.0; c.button_history.now = 0.5; c },
        {
            let mut c = st.checkpoint();
            let sw3 = m.find_state("purl").unwrap();
            let atk_key = pack_attack_key(sw3, 0);
            c.hit_history.record(atk_key, 0x0001_2345, 0.5, TargetId::Life(99));
            c
        },
    ] {
        assert!(
            st.try_restore(&m, bad_cp).is_err(),
            "inconsistent checkpoint must be rejected: {bad_cp:?}"
        );
        assert_eq!(st, baseline, "failed try_restore must leave state unchanged");
    }

    // 4. Strict SaberThrowState::try_restore rejects non-unit active direction.
    let mut ts = SaberThrowState::new();
    let mut bad_tcp = ts.checkpoint();
    bad_tcp.phase = SaberThrowPhase::Outbound;
    bad_tcp.last_sweep_phase = SaberThrowPhase::Outbound;
    bad_tcp.direction = [4.0, 0.0, 0.0];
    assert_eq!(
        ts.try_restore(bad_tcp),
        Err(CheckpointError::InvalidDirection),
        "SaberThrowState::try_restore must reject non-unit active direction"
    );

    // 5. SaberThrowDef owns its own damage/shield scales, resolves FromEdge from primary
    //    weapon edge, respects PROVISIONAL_DEFLECT_SABER_THROW, and supports Validated* wrappers.
    let mut carried_saber = WeaponMeleeSpec::default();
    carried_saber.edges[0] = EdgeSpec { default_length: 1.5, default_width: 0.4 };
    carried_saber.shield_scale = 2.75; // must NOT bleed into thrown saber shield damage!
    let throw_def = SaberThrowDef {
        energy_drain: 30.0,
        velocity: 25.0,
        life_span: 0.8,
        return_duration: 0.5,
        rotation_rate: 4.0,
        damage: 137.5,
        damage_length: 1.0,
        damage_width: 0.6,
        push: 0.0,
        damage_scales: [1.0; 5],
        shield_scale: 1.0,
    }
    .with_primary_edge_dimensions(&carried_saber, Some(1.2), None);
    assert!((throw_def.damage_length - 1.8).abs() < 1e-9, "1.2 * primary edge 1.5 = 1.8");
    let v_throw = ValidatedSaberThrowDef::try_new(throw_def).expect("valid throw def");

    let mut throw_st = SaberThrowState::new();
    let mut throw_e = 50.0;
    assert!(throw_st.launch_validated(&v_throw, [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], &mut throw_e));
    let defender_shapes = [cylinder_shape([0.0, 1.0, 1.0], 0.5, 0.9)];
    let blocking_target = MeleeTarget {
        target_id: TargetId::Life(901),
        position: [0.0, 0.0, 1.0],
        aim_yaw: std::f64::consts::PI,
        health_scale: HealthScale::Person,
        shapes: &defender_shapes,
        deflect: Some(DeflectDef {
            min_angle_deg: -110.0,
            max_angle_deg: 110.0,
            energy_cost: 0.0,
            deflect_anim_start: 0,
            deflect_anim_len: 0,
            reaction_ids: [u16::MAX;6],
        }),
    };
    let mut def_e = 100.0;
    let mut observed = None;
    for _ in 0..8 {
        let s = throw_st.step_validated(&v_throw, [0.0, 1.0, 0.0], true, DT);
        if let Some(sw) = s.sweep {
            let res = throw_st.resolve_hit_validated(&v_throw, &sw, &blocking_target, Some(&mut def_e));
            if !matches!(res, MeleeHitOutcome::Missed | MeleeHitOutcome::AlreadyHit) {
                observed = Some(res);
                break;
            }
        }
    }
    assert!(
        matches!(observed, Some(MeleeHitOutcome::Hit { damage, shield_damage, .. }) if damage == 137.5 && shield_damage == 137.5),
        "thrown saber must use its own shield_scale=1.0 (137.5 dmg) and respect PROVISIONAL_DEFLECT_SABER_THROW=false, got {observed:?}"
    );

    // Zero-alloc Copy PowerValidationError check.
    let err: PowerValidationError = SaberThrowDef { velocity: -1.0, ..throw_def }
        .validate()
        .unwrap_err();
    assert_eq!(err, PowerValidationError::InvalidVelocity);
    let err_copy = err;
    assert_eq!(err, err_copy);
}

#[test]
fn thrown_saber_return_spins_at_half_rate() {
    let def = SaberThrowDef {
        energy_drain: 14.0,
        velocity: 19.0,
        life_span: 0.12,
        return_duration: 0.35,
        rotation_rate: 3.25,
        ..SaberThrowDef::default()
    };
    let mut energy = 50.0;
    let mut flight = SaberThrowState::new();
    // A short outbound pass: the opening step is outbound, a later one begins the return.
    assert!(flight.launch_reaching(&def, [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], &mut energy, 0.5));
    let out_step = flight.step(&def, [0.0, 1.0, 0.0], true, DT);
    assert_eq!(out_step.phase, SaberThrowPhase::Outbound);
    let outbound_delta = flight.spin_angle - flight.prev_spin_angle;
    let mut return_delta = 0.0;
    for _ in 0..20 {
        let step = flight.step(&def, [0.0, 1.0, 0.0], true, DT);
        if step.phase == SaberThrowPhase::Returning {
            return_delta = flight.spin_angle - flight.prev_spin_angle;
            break;
        }
    }
    assert!(return_delta > 0.0, "the return pass never started");
    assert!((outbound_delta - 3.25 * 2.0 * std::f64::consts::PI * DT).abs() < 1e-9);
    assert!((return_delta - 0.5 * outbound_delta).abs() < 1e-9);
}

#[test]
fn thrown_saber_wall_bounce_death_and_second_throw_never_freeze_or_relaunch() {
    let def = SaberThrowDef {
        energy_drain: 42.0,
        velocity: 22.0,
        life_span: 0.9,
        return_duration: 0.35,
        rotation_rate: 3.25,
        ..SaberThrowDef::default()
    };
    let hand = [-0.15, 1.18, 0.42];
    // 1. Wall bounce triggers immediate return and reaches catch with no frozen ticks.
    let mut energy = 90.0;
    let mut flight = SaberThrowState::new();
    // A wall a quarter of the way along the range.
    assert!(flight.launch_reaching(&def, hand, [0.0, 0.0, 1.0], &mut energy, 0.25));
    assert_eq!(energy, 48.0);
    let step1 = flight.step(&def, hand, true, DT);
    assert_eq!(step1.phase, SaberThrowPhase::Outbound);
    // Second throw attempt while already in flight is rejected without charging energy.
    let mut extra_energy = 90.0;
    assert!(!flight.launch(&def, hand, [1.0, 0.0, 0.0], &mut extra_energy));
    assert_eq!(extra_energy, 90.0);
    let mut caught = false;
    let mut prev = flight.position;
    for _ in 0..40 {
        let s = flight.step(&def, hand, true, DT);
        if s.caught {
            caught = true;
            assert!(!flight.is_active());
            break;
        }
        let dx = flight.position[0] - prev[0];
        let dy = flight.position[1] - prev[1];
        let dz = flight.position[2] - prev[2];
        let moved = (dx * dx + dy * dy + dz * dz).sqrt();
        assert!(moved > 1e-4, "saber froze on a wall throw");
        prev = flight.position;
    }
    assert!(caught, "wall-bounced saber never reached catch");

    // 2. Owner death mid-flight cancels immediately and leaves the flight inactive.
    let mut energy2 = 90.0;
    assert!(flight.launch(&def, hand, [0.0, 0.0, 1.0], &mut energy2));
    let _ = flight.step(&def, hand, true, DT);
    let death = flight.step(&def, hand, false, DT);
    assert!(death.cancelled && !death.active && !flight.is_active());
}

fn return_def() -> SaberThrowDef {
    SaberThrowDef {
        energy_drain: 5.0,
        velocity: 18.0,
        life_span: 1.2,
        return_duration: 0.6,
        rotation_rate: 2.5,
        ..SaberThrowDef::default()
    }
}

/// Run a throw to its catch with a fixed release point; returns (outbound ticks, return ticks,
/// last blade position, farthest distance from the origin).
fn fly_to_catch(reach: f64) -> (u32, u32, V3, f64) {
    let def = return_def();
    let origin = [2.0, 1.5, -3.0];
    let mut flight = SaberThrowState::new();
    let mut energy = 20.0;
    assert!(flight.launch_reaching(&def, origin, [0.0, 0.0, 1.0], &mut energy, reach));
    let (mut out, mut back, mut far) = (0u32, 0u32, 0.0f64);
    for _ in 0..400 {
        let step = flight.step(&def, origin, true, DT);
        match step.phase {
            SaberThrowPhase::Outbound => out += 1,
            SaberThrowPhase::Returning => back += 1,
            SaberThrowPhase::Inactive => {}
        }
        far = far.max(flight.position[2] - origin[2]);
        if step.caught {
            return (out, back, flight.position, far);
        }
    }
    panic!("the throw was never caught");
}

#[test]
fn a_throw_that_meets_a_wall_is_caught_again_in_proportion_to_the_distance_flown() {
    let (out_full, back_full, _, far_full) = fly_to_catch(1.0);
    // Full range: 1.2 s out (72 ticks), 0.6 s back (36 ticks, the catch tick included).
    assert_eq!(out_full, 72);
    assert_eq!(back_full, 36);
    assert!((far_full - 18.0 * 1.2).abs() < 1e-9, "outbound ends at the end of the range");
    let (out, back, last, far) = fly_to_catch(0.25);
    // A quarter of the range: 0.3 s out (18 ticks) and 0.15 s back (9 ticks).
    assert_eq!(out, 18);
    assert_eq!(back, 9);
    assert!((far - 18.0 * 0.3).abs() < 1e-9, "the blade ends exactly where the free range ends: {far}");
    assert!(last[2] - -3.0 < far, "the blade is on its way back when the throw ends");
    // The return time is the full time scaled by the share, not a fixed time.
    assert!(back_full > 3 * back);
}

#[test]
fn a_short_throw_is_caught_when_its_return_time_is_used_up_not_when_it_is_near() {
    // 0.65 m of free range but a long return time: the blade creeps home and is not
    // taken in early for being within reach of the hand.
    let def = SaberThrowDef { return_duration: 8.0, ..return_def() };
    let origin = [0.0, 1.0, 0.0];
    let mut flight = SaberThrowState::new();
    let mut energy = 20.0;
    assert!(flight.launch_reaching(&def, origin, [0.0, 0.0, 1.0], &mut energy, 0.03));
    let mut returning = 0;
    for _ in 0..200 {
        let step = flight.step(&def, origin, true, DT);
        returning += u32::from(step.phase == SaberThrowPhase::Returning);
        if step.caught {
            break;
        }
    }
    // 8 s x 0.03 = 0.24 s of return: 14 return ticks, the catch tick included (the opening tick of
    // the 14.4 is spent finishing the outbound pass).
    assert_eq!(returning, 14);
    assert!(!flight.is_active());
}

#[test]
fn a_throw_that_starts_against_a_wall_is_caught_at_once_without_moving() {
    let def = return_def();
    let origin = [0.5, 1.0, 0.5];
    let mut flight = SaberThrowState::new();
    let mut energy = 20.0;
    assert!(flight.launch_reaching(&def, origin, [1.0, 0.0, 0.0], &mut energy, 0.0));
    let first = flight.step(&def, origin, true, DT);
    assert_eq!(first.phase, SaberThrowPhase::Outbound);
    assert!(!first.caught);
    assert_eq!(flight.position, origin);
    let second = flight.step(&def, origin, true, DT);
    assert_eq!(second.phase, SaberThrowPhase::Returning);
    assert!(second.caught, "a return time of zero ends on the opening return tick");
    assert_eq!(flight.position, origin);
}

#[test]
fn the_returning_blade_chases_the_release_point_with_the_rule_of_the_clock() {
    let def = return_def();
    let origin = [0.0, 1.0, 0.0];
    let mut flight = SaberThrowState::new();
    let mut energy = 20.0;
    assert!(flight.launch_reaching(&def, origin, [0.0, 0.0, 1.0], &mut energy, 0.5));
    // Outbound to the end of the free range (half of 21.6 m), no target needed yet.
    let mut tick = 0;
    while flight.phase == SaberThrowPhase::Outbound {
        flight.step(&def, origin, true, DT);
        tick += 1;
        assert!(tick < 200);
    }
    // The release point now moves 6 m/s along x while the blade returns.
    let mut model = flight.position;
    let mut clock = flight.return_elapsed;
    let mut start = flight.position;
    let mut owner = origin;
    let duration = flight.return_time;
    assert!((duration - 0.3).abs() < 1e-12);
    let mut caught = false;
    let mut differs_from_lerp = 0.0f64;
    for _ in 0..40 {
        owner[0] += 6.0 * DT;
        let step = flight.step(&def, owner, true, DT);
        clock += DT;
        if step.caught {
            assert!(clock + 1e-6 >= duration);
            caught = true;
            break;
        }
        // The rule: share = (now - before) / (1 - before) of the way to the release point.
        let now = clock / duration;
        let before = (clock - DT) / duration;
        let share = (now - before) / (1.0 - before);
        for i in 0..3 {
            model[i] += (owner[i] - model[i]) * share;
        }
        for i in 0..3 {
            assert!((flight.position[i] - model[i]).abs() < 1e-9, "axis {i}: {} vs {}", flight.position[i], model[i]);
        }
        // Not the straight lerp from the start toward the release point as it is now.
        let lerp_x = start[0] + (owner[0] - start[0]) * now;
        differs_from_lerp = differs_from_lerp.max((flight.position[0] - lerp_x).abs());
        start = start;
    }
    assert!(caught);
    assert!(differs_from_lerp > 0.01, "chasing a moving point differs from lerping toward it: {differs_from_lerp}");
}

#[test]
fn jump_attack_from_energy_jet_cancels_jet_preserves_momentum_and_lands() {
    use crate::collision::spheres::{GatherCache, Gathered};
    use crate::movement::scene::{GroundCache, Scene};
    use crate::movement::soldier::{Input as MoveInput, Jet, PART_CACHES};

    let doc = serde_json::json!({
        "states": [
            {
                "name": "IDLE",
                "children": [
                    { "key": "Duration", "values": [0.0] },
                    {
                        "key": "Transition",
                        "values": ["SLAM_FALL"],
                        "children": [{
                            "key": "If",
                            "children": [
                                { "key": "Break" },
                                { "key": "Posture", "values": ["Jump", "Jet"] },
                                { "key": "Button", "values": ["Fire", "Press"] }
                            ]
                        }]
                    }
                ]
            },
            {
                "name": "SLAM_FALL",
                "children": [
                    { "key": "Duration", "values": [0.0] },
                    { "key": "Posture", "values": ["Jump"] },
                    { "key": "TurnFactor", "values": [0.67] },
                    { "key": "StrafeFactor", "values": [0.58] },
                    { "key": "Gravity", "values": [2.3] },
                    { "key": "GravityVelocityTarget", "values": [-17.5] },
                    { "key": "AnimatedMove", "children": [] },
                    { "key": "InputLock", "values": ["Jump", "Sprint", "Crouch"] },
                    {
                        "key": "Transition",
                        "values": ["SLAM_LAND"],
                        "children": [{
                            "key": "If",
                            "children": [
                                { "key": "Break" },
                                { "key": "Posture", "values": ["Stand"] }
                            ]
                        }]
                    }
                ]
            },
            {
                "name": "SLAM_LAND",
                "children": [
                    { "key": "Posture", "values": ["Stand"] },
                    { "key": "Duration", "values": [0.35] },
                    { "key": "TurnFactor", "values": [0.41] },
                    { "key": "StrafeFactor", "values": [0.39] },
                    { "key": "AnimatedMove", "children": [] },
                    { "key": "InputLock", "values": ["Jump", "Sprint"] },
                    { "key": "Transition", "values": ["IDLE"] }
                ]
            }
        ]
    });
    let weapon = WeaponMeleeSpec::default();
    let m = ComboMachine::from_json_value(&doc, &weapon, COMBO_ANIM_FPS, |_| None).unwrap();
    let mut base_tuning = test_move_tuning();
    base_tuning.jetpack = Some(move2::Jetpack {
        energy_cost: 36.0,
        force_jump: false,
        acceleration: 10.0,
        physics: Jet {
            jump: 10.0,
            push: 6.0,
            recharge: 0.0,
            cost: 0.0,
            initial: 0.0,
            min_border: 0.0,
            hover: false,
        },
    });
    let (world, mut col) = flat_move_world();
    let mut mv = Mover::for_family(world as usize, GameFamily::Swbf2);
    mv.grounded = false;
    mv.air = crate::movement::soldier::Air::Jet;
    mv.position = MoveV3::new(0.0, 4.0, 0.0);
    mv.settled = mv.position;
    mv.velocity = MoveV3::new(0.0, 5.0, 11.0);
    let mut mstate = move2::State::new(&base_tuning);
    mstate.energy = 80.0;
    let mut cst = ComboState::new(&m);
    let mut gathered = Gathered::default();
    let mut caches = std::array::from_fn::<_, PART_CACHES, _>(|_| GatherCache::new(0.25));
    let mut gcache = GroundCache::default();
    let mut scene = Scene {
        s: &mut col,
        world: world as usize,
        flags: &[],
        gathered: &mut gathered,
        caches: &mut caches,
        ground_cache: &mut gcache,
    };

    let slam_fall = m.find_state("SLAM_FALL").unwrap();
    let slam_land = m.find_state("SLAM_LAND").unwrap();
    let (out0, _) = step_soldier_combo_and_move(
        &m,
        &mut cst,
        &ComboInput::default().press(ComboButton::Fire),
        &mut mv,
        &mut mstate,
        &base_tuning,
        &MoveInput { jump: true, ..MoveInput::default() },
        move2::Actions::default(),
        &mut scene,
        DT,
    )
    .unwrap();
    assert_eq!(out0.entered_state, Some(slam_fall));
    assert_eq!(mv.air, crate::movement::soldier::Air::Fall, "jump attack must cancel Air::Jet");
    assert!((mstate.energy - 80.0).abs() < 1e-9, "jet energy drain must stop immediately");
    assert!(mv.velocity.z > 10.5, "empty AnimatedMove must preserve jump momentum, got {}", mv.velocity.z);

    let mut landed_vz = None;
    for _ in 0..90 {
        step_soldier_combo_and_move(
            &m,
            &mut cst,
            &ComboInput::default(),
            &mut mv,
            &mut mstate,
            &base_tuning,
            &MoveInput::default(),
            move2::Actions::default(),
            &mut scene,
            DT,
        )
        .unwrap();
        assert_ne!(mv.air, crate::movement::soldier::Air::Jet);
        if cst.state == slam_land {
            landed_vz = Some(mv.velocity.z);
            break;
        }
        assert!(mv.velocity.z > 8.0, "momentum must persist through fall, got {}", mv.velocity.z);
    }
    assert!(landed_vz.is_some(), "must transition to SLAM_LAND on touchdown");
    assert!(landed_vz.unwrap().abs() < 1e-6, "grounded SLAM_LAND plants feet");
    assert!(mv.position.z > 4.0, "jump attack must travel forward across the arc, got z={}", mv.position.z);
    assert!((mstate.energy - 80.0).abs() < 1e-9, "zero jet drain across entire slam");
}

#[test]
fn airborne_animated_move_clamps_acceleration_and_latches_gravity_targets() {
    use crate::collision::spheres::{GatherCache, Gathered};
    use crate::movement::scene::{GroundCache, Scene};
    use crate::movement::soldier::{Input as MoveInput, PART_CACHES};

    let doc = serde_json::json!({
        "states": [
            {
                "name": "IDLE",
                "children": [
                    { "key": "Duration", "values": [12.5] },
                    { "key": "TurnFactor", "values": [0.83] },
                    { "key": "Gravity", "values": [1.13] },
                    {
                        "key": "Transition",
                        "values": ["DIVE_THRUST"],
                        "children": [{ "key": "If", "children": [
                            { "key": "Break" },
                            { "key": "Button", "values": ["Fire", "Press"] }
                        ]}]
                    },
                    {
                        "key": "Transition",
                        "values": ["FLOAT_LATCH"],
                        "children": [{ "key": "If", "children": [
                            { "key": "Break" },
                            { "key": "Button", "values": ["FireSecondary", "Press"] }
                        ]}]
                    },
                    {
                        "key": "Transition",
                        "values": ["IMPULSE_DROP"],
                        "children": [{ "key": "If", "children": [
                            { "key": "Break" },
                            { "key": "Button", "values": ["Jump", "Press"] }
                        ]}]
                    }
                ]
            },
            {
                "name": "DIVE_THRUST",
                "children": [
                    { "key": "Duration", "values": [0.0] },
                    { "key": "Posture", "values": ["Jump"] },
                    { "key": "TurnFactor", "values": [0.53] },
                    { "key": "Gravity", "values": [3.7] },
                    { "key": "GravityVelocityTarget", "values": [-13.5] },
                    { "key": "AnimatedMove", "children": [
                        { "key": "VelocityFromThrust", "values": [73.0] },
                        { "key": "VelocityFromStrafe", "values": [-4.5] }
                    ]}
                ]
            },
            {
                "name": "FLOAT_LATCH",
                "children": [
                    { "key": "Duration", "values": [0.0] },
                    { "key": "TurnFactor", "values": [0.47] },
                    { "key": "Gravity", "values": [3.4] },
                    { "key": "GravityVelocityTarget", "values": [-8.5, "ZeroGravity"] },
                    { "key": "AnimatedMove", "children": [
                        { "key": "VelocityZ", "values": [112.0] },
                        { "key": "VelocityX", "values": [-88.0] }
                    ]}
                ]
            },
            {
                "name": "IMPULSE_DROP",
                "children": [
                    { "key": "Duration", "values": [0.0] },
                    { "key": "TurnFactor", "values": [0.61] },
                    { "key": "Gravity", "values": [0.45] },
                    { "key": "GravityVelocityTarget", "values": [-3.25, "Impulse", "ZeroGravity"] }
                ]
            }
        ]
    });
    let m = ComboMachine::from_json_value(&doc, &WeaponMeleeSpec::default(), COMBO_ANIM_FPS, |_| None).unwrap();
    let dive_am = m.state(m.find_state("DIVE_THRUST").unwrap()).animated_move.as_ref().unwrap();
    assert!((dive_am.velocity_from_thrust - 25.5).abs() < 1e-9);
    assert!((dive_am.velocity_from_strafe - 0.0).abs() < 1e-9);
    let float_am = m.state(m.find_state("FLOAT_LATCH").unwrap()).animated_move.as_ref().unwrap();
    assert!((float_am.velocity_z - 40.94).abs() < 1e-9);
    assert!((float_am.velocity_x - (-40.96)).abs() < 1e-9);

    let tuning = test_move_tuning();
    // 1. DIVE_THRUST accelerates in air from clamped 25.5 m/s^2 instead of setting 90 m/s.
    let (world, mut col) = flat_move_world();
    let mut mv = Mover::for_family(world as usize, GameFamily::Swbf2);
    mv.grounded = false;
    mv.air = crate::movement::soldier::Air::Jump;
    mv.position = MoveV3::new(0.0, 20.0, 0.0);
    mv.settled = mv.position;
    mv.velocity = MoveV3::new(0.0, 2.0, 8.0);
    let mut mstate = move2::State::new(&tuning);
    let mut cst = ComboState::new(&m);
    let mut gathered = Gathered::default();
    let mut caches = std::array::from_fn::<_, PART_CACHES, _>(|_| GatherCache::new(0.25));
    let mut gcache = GroundCache::default();
    let mut scene = Scene {
        s: &mut col,
        world: world as usize,
        flags: &[],
        gathered: &mut gathered,
        caches: &mut caches,
        ground_cache: &mut gcache,
    };
    let ci = ComboInput::default().press(ComboButton::Fire).stick(1.0, 0.0);
    step_soldier_combo_and_move(&m, &mut cst, &ci, &mut mv, &mut mstate, &tuning, &MoveInput::default(), move2::Actions::default(), &mut scene, DT).unwrap();
    for _ in 0..15 {
        step_soldier_combo_and_move(&m, &mut cst, &ComboInput::default().stick(1.0, 0.0), &mut mv, &mut mstate, &tuning, &MoveInput::default(), move2::Actions::default(), &mut scene, DT).unwrap();
    }
    assert!(mv.velocity.z > 8.5 && mv.velocity.z < 18.0, "dive must accelerate smoothly, got {}", mv.velocity.z);

    // 2. FLOAT_LATCH uses 3.4g until vy <= -8.5, then latches 0g and preserves latch across checkpoint restore.
    let mut st = ComboState::new(&m);
    let mut life = move2::State::new(&tuning);
    let body_hi = BodyContext { posture: ComboPosture::Jump, aim_yaw: 0.0, velocity_y: -4.0, grounded: false, moving: false };
    let out1 = step(GameFamily::Swbf2, &m, &mut st, &ComboInput::default().press(ComboButton::FireSecondary), &body_hi, &mut life, &tuning, false, DT);
    assert!((out1.movement.gravity_scale - 3.4).abs() < 1e-9, "must use 3.4g before reaching target");
    assert!(!st.gravity_target_latched);
    let body_lo = BodyContext { velocity_y: -9.0, ..body_hi };
    let out2 = step(GameFamily::Swbf2, &m, &mut st, &ComboInput::default(), &body_lo, &mut life, &tuning, false, DT);
    assert!((out2.movement.gravity_scale - 0.0).abs() < 1e-9, "must switch to 0g once target reached");
    assert!(st.gravity_target_latched);
    let cp = st.checkpoint();
    let mut restored = ComboState::new(&m);
    restored.try_restore(&m, cp).unwrap();
    assert!(restored.gravity_target_latched);
    // Even if vy rises above -8.5 later, latch persists until state change.
    let out3 = step(GameFamily::Swbf2, &m, &mut restored, &ComboInput::default(), &body_hi, &mut life, &tuning, false, DT);
    assert!((out3.movement.gravity_scale - 0.0).abs() < 1e-9);

    // 3. IMPULSE_DROP sets vertical_impulse once on entry and uses 1.0g afterward (ignoring 0.45 and ZeroGravity).
    let mut imp_st = ComboState::new(&m);
    let mut imp_life = move2::State::new(&tuning);
    let imp0 = step(GameFamily::Swbf2, &m, &mut imp_st, &ComboInput::default().press(ComboButton::Jump), &body_hi, &mut imp_life, &tuning, false, DT);
    assert_eq!(imp0.movement.vertical_impulse, Some(-3.25));
    assert!((imp0.movement.gravity_scale - 1.0).abs() < 1e-9);
    let imp1 = step(GameFamily::Swbf2, &m, &mut imp_st, &ComboInput::default(), &body_hi, &mut imp_life, &tuning, false, DT);
    assert_eq!(imp1.movement.vertical_impulse, None);
    assert!((imp1.movement.gravity_scale - 1.0).abs() < 1e-9);
}
