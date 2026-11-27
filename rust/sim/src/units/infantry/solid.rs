//! Clip each posed edge to the collision world before testing the swept volume.
use crate::{
    combat::{
        combo::{
            AttackDef, BladeSegment, BladeSweep, DamageSample, HitHistory, TargetId, MAX_DAMAGE_SAMPLES,
        },
        impacts::HitPacket,
    },
    gen::{Body, OrdnanceOwner, Unit},
    ordnance::world::Hit,
    store::Ref,
    units::{f3, native, shapes::HitShape},
    world::owner::World,
};

pub(super) struct Path {
    segments: [Option<BladeSegment>; MAX_DAMAGE_SAMPLES],
    len: usize,
}

pub(super) fn cast(w: &mut World, from: [f64; 3], to: [f64; 3]) -> Option<Hit> {
    let d: [f64; 3] = std::array::from_fn(|i| to[i] - from[i]);
    let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if length <= f64::EPSILON {
        return None;
    }
    native::ordnance_cast_in(
        w,
        f3::collision_world_in(&w.units),
        from,
        d.map(|v| v / length),
        length,
        None,
        false,
        false,
    )
}

impl Path {
    pub(super) fn new(
        w: &mut World,
        root: [f64; 3],
        samples: &[DamageSample],
        attack: &AttackDef,
        history: &mut HitHistory,
        packet: HitPacket,
        owner: Ref<Unit>,
    ) -> Self {
        let mut path = Self {
            segments: [None; MAX_DAMAGE_SAMPLES],
            len: samples.len(),
        };
        for (out, sample) in path.segments.iter_mut().zip(samples) {
            let mut blade = sample.to_segment(attack.damage_length);
            let shoulder = [root[0], blade.base[1], root[2]];
            let mount_hit = cast(w, shoulder, blade.base);
            let contact = mount_hit.or_else(|| cast(w, blade.base, blade.tip));
            if let Some(hit) = contact {
                if let Some(body) = hit.body {
                    strike_body(w, body, attack, history, packet, owner);
                }
                if mount_hit.is_some() {
                    continue;
                }
                blade.tip = hit.point;
            }
            *out = Some(blade);
        }
        path
    }

    pub(super) fn contact(
        &self,
        attack: &AttackDef,
        shapes: &[HitShape],
        mut visible: impl FnMut([f64; 3], [f64; 3]) -> bool,
    ) -> Option<[f64; 3]> {
        let mut previous = None;
        for &segment in &self.segments[..self.len] {
            if let Some(current) = segment {
                let sweep = BladeSweep::from_attack(previous.unwrap_or(current), current, attack);
                for shape in shapes {
                    if let Some(point) = sweep.hit_shape(shape) {
                        if visible(current.base, shape.center) {
                            return Some(point);
                        }
                    }
                }
            }
            previous = segment;
        }
        None
    }
}

/// A thick blade may touch a hurt volume on the far side of a thin solid.
pub(super) fn visible(w: &mut World, from: [f64; 3], center: [f64; 3]) -> bool {
    cast(w, from, center).is_none()
}

fn strike_body(
    w: &mut World,
    body: Ref<Body>,
    attack: &AttackDef,
    history: &mut HitHistory,
    packet: HitPacket,
    owner: Ref<Unit>,
) {
    let object = if let Some(root) = crate::units::f3_props::damage_body_in(&w.units, body) {
        root
    } else if crate::units::f3_hulls::hull_of_in(w, body).is_some() {
        body
    } else {
        return;
    };
    let id = body_target(object);
    if !history.record(attack.attack_key, attack.shared_id, 0., id) {
        return;
    }
    f3::with_services_in(w, |w, svc| {
        let Some(svc) = svc else { return };
        crate::combat::live::hit_body_packet_in(
            w,
            body,
            packet,
            OrdnanceOwner::Unit(owner),
            None,
            svc,
        );
    });
}

/// Body slots are bounded by collision admission. A body is its own kind of target, distinct
/// from the unit lives in the attack's shared history.
fn body_target(body: Ref<Body>) -> TargetId {
    debug_assert!((body.slot as usize) < crate::collision::MAX_BODIES);
    TargetId::Body { generation: body.gen, slot: body.slot }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::{
        combo::{self, AttackGroupId, DamageWindow, MeleeHitOutcome, MeleeTarget, WeaponMeleeSpec},
        impacts::HealthScale,
    };
    use crate::units::shapes::{Half, ShapePart, ShapeType};

    #[test]
    #[ignore = "requires prepared ground and space battles"]
    fn native_bodies_receive_their_category_damage_once() {
        if std::env::var_os("BFE_SWBF2_PREPARED").is_none() {
            return;
        }
        use crate::units::f3_props;
        let attack = AttackDef {
            attack_group: AttackGroupId(0),
            attack_key: 18,
            shared_id: 0,
            edge: 0,
            window: DamageWindow::Seconds { start: 0., end: 1. },
            damage_length: 2.,
            damage_width: 0.05,
            damage: 16.,
            push: 0.,
        };
        let packet = HitPacket {
            damage: 16.,
            scales: [1., 1., 1., 0.5, 0.25],
            shield: 1.,
        };
        let mut covered = [false; 2];
        for space in [false, true] {
            let (mut w, owner) = crate::battle::script::tests::prepared_combat_scene(space);
            for i in 0..f3_props::building_count_in(&w.units) {
                let Some(body) = f3_props::melee_body_in(&w.units, i) else {
                    continue;
                };
                let Some(prop) = f3_props::wire_building_in(&w.units, &w.battle, i) else {
                    continue;
                };
                let category = usize::from(prop.turret);
                if covered[category] || prop.health <= 16. || !prop.health.is_finite() {
                    continue;
                }
                let mut history = HitHistory::new();
                let before = prop.health;
                strike_body(&mut w, body, &attack, &mut history, packet, owner);
                let after = f3_props::wire_building_in(&w.units, &w.battle, i)
                    .unwrap()
                    .health;
                if after == before {
                    continue;
                }
                assert_eq!(before - after, 4.);
                strike_body(&mut w, body, &attack, &mut history, packet, owner);
                assert_eq!(
                    f3_props::wire_building_in(&w.units, &w.battle, i)
                        .unwrap()
                        .health,
                    after
                );
                covered[category] = true;
            }
        }
        assert_eq!(covered, [true; 2], "prop, turret");
    }

    #[test]
    fn a_seated_rider_is_protected_and_the_hull_takes_one_scaled_hit() {
        let (mut world, attacker, rider, vehicle) =
            crate::battle::script::tests::seated_combat_scene(false);
        assert!(!super::super::contact::eligible(
            &world,
            rider,
            world.sim.store.unit.get(rider).unwrap(),
            1.,
            true
        ));
        let hull = world.sim.store.vehicle.get(vehicle).unwrap();
        let body = hull.body;
        let before = hull.health;
        let attack = AttackDef {
            attack_key: 4,
            attack_group: AttackGroupId(0),
            shared_id: 0,
            edge: 0,
            window: DamageWindow::Seconds { start: 0., end: 1. },
            damage_length: 2.,
            damage_width: 0.03,
            damage: 16.,
            push: 0.,
        };
        let packet = HitPacket {
            damage: 16.,
            scales: [1., 1., 1., 0.5, 0.25],
            shield: 1.,
        };
        let mut history = HitHistory::new();
        for _ in 0..2 {
            strike_body(&mut world, body, &attack, &mut history, packet, attacker);
        }
        assert_eq!(
            world.sim.store.vehicle.get(vehicle).unwrap().health,
            before - 8.
        );
        assert_eq!(world.sim.store.unit.get(rider).unwrap().combat.health, 100.);
    }

    #[test]
    fn a_rider_with_an_exposed_animated_seat_remains_a_target() {
        let (world, _, rider, _) = crate::battle::script::tests::seated_combat_scene(true);
        assert!(super::super::contact::eligible(&world, rider, world.sim.store.unit.get(rider).unwrap(), 1., true));
    }

    #[test]
    fn a_native_wall_stops_the_authored_blade_and_occludes_the_far_side() {
        let mut w = crate::battle::script::tests::combat_arena();
        let owner = w
            .sim
            .store
            .unit
            .iter()
            .find(|(_, u)| u.alive && !u.dead)
            .unwrap()
            .0;
        let attack = AttackDef {
            attack_group: AttackGroupId(0),
            attack_key: 12,
            shared_id: 0,
            edge: 0,
            window: DamageWindow::Seconds { start: 0., end: 1. },
            damage_length: 2.,
            damage_width: 0.04,
            damage: 12.,
            push: 0.,
        };
        let base = [65., 1., 3.];
        let sample = DamageSample {
            time: 0.,
            position: base,
            direction: [0., 0., 1.],
        };
        let packet = HitPacket {
            damage: 12.,
            scales: [1.; 5],
            shield: 1.,
        };
        let mut history = HitHistory::new();
        let path = Path::new(
            &mut w,
            base,
            &[sample],
            &attack,
            &mut history,
            packet,
            owner,
        );
        let segment = path.segments[0].unwrap();
        assert!((segment.tip[2] - 4.).abs() < 1e-7, "{segment:?}");
        assert!(visible(&mut w, base, [65., 1., 3.8]));
        assert!(!visible(&mut w, base, [65., 1., 4.2]));
    }

    #[test]
    fn clipped_edges_stop_at_solids_and_keep_distinct_target_lives() {
        let attack = AttackDef {
            attack_group: AttackGroupId(0),
            attack_key: 7,
            shared_id: 0,
            edge: 0,
            window: DamageWindow::Seconds { start: 0., end: 1. },
            damage_length: 2.,
            damage_width: 0.02,
            damage: 11.,
            push: 0.,
        };
        let mut path = Path {
            segments: [None; MAX_DAMAGE_SAMPLES],
            len: 1,
        };
        path.segments[0] = Some(BladeSegment {
            base: [0., 1., 0.],
            tip: [0., 1., 0.6],
        });
        let ball = |z| HitShape {
            r#type: ShapeType::Sphere,
            part: ShapePart::Body,
            center: [0., 1., z],
            radius: 0.08,
            half_size: [0.; 3],
            half: Half::F64(0.),
            yaw: 0.,
        };
        assert!(path.contact(&attack, &[ball(1.5)], |_, _| true).is_none());
        let shapes = [ball(0.2), ball(0.5)];
        assert!(path.contact(&attack, &shapes, |_, _| false).is_none());
        assert_eq!(
            path.contact(&attack, &shapes, |_, center| center[2] > 0.3),
            path.contact(&attack, &shapes[1..], |_, _| true)
        );
        let mut history = HitHistory::new();
        let weapon = WeaponMeleeSpec::default();
        for (target_id, z, should_hit) in [
            (TargetId::Life(1), 0.4, true),
            (TargetId::Life(1), 0.4, false),
            (TargetId::Life(2), 0.45, true),
            (TargetId::Life(3), 0.4, true),
        ] {
            let shapes = [ball(z)];
            let at = path.contact(&attack, &shapes, |_, _| true).unwrap();
            let target = MeleeTarget {
                target_id,
                position: [0., 1., z],
                aim_yaw: 0.,
                health_scale: HealthScale::Person,
                shapes: &shapes,
                deflect: None,
            };
            let hit = combo::sweep::apply_contact(
                [0.; 3],
                &attack,
                &weapon,
                at,
                &mut history,
                &target,
                None,
            );
            assert_eq!(matches!(hit, MeleeHitOutcome::Hit { .. }), should_hit);
        }
    }
}
