use super::*;
use crate::units::infantry::{
    bot_input::{self, Frame, Stamp},
    contact::{self, Cause, LifeId, Outcome, Weapon},
};

/// Keep an opposing body within reach while the registered bot's mover, pose
/// and contact solver run. The first authorized hit is lethal in this fixture.
fn drive_bot(world: &mut Root, installation: u32, tick: u32) -> Option<(LifeId, LifeId, u32)> {
    let mut attackers = Vec::new();
    world.visit_melee(|life, weapon, _, _, _| attackers.push((life, weapon)));
    let defs = crate::combat::module::defs_in(&mut world.combat);
    let (life, weapon, binding, team, position, yaw) =
        attackers.into_iter().find_map(|(life, weapon)| {
            let body = world
                .sim
                .store
                .unit
                .get_mut(Ref::new(life.slot, life.slab))?;
            if body.human || body.seat.is_some() {
                return None;
            }
            let binding = bot_input::equipped(&mut body.combat, &defs.weapons)?;
            Some((
                life,
                weapon,
                binding,
                body.team,
                body.position(),
                body.aim_yaw(),
            ))
        })?;
    let enemy = world
        .sim
        .store
        .unit
        .iter()
        .find(|(_, u)| {
            u.alive
                && !u.dead
                && !u.removed
                && u.seat.is_none()
                && u.team > 0.
                && u.team != team
                && matches!(u.soldier, UnitBody::Soldier(_))
        })?
        .0;
    let body = world.sim.store.unit.get_mut(enemy)?;
    let victim = contact::identity(enemy, body);
    let UnitBody::Soldier(soldier) = &mut body.soldier else {
        unreachable!()
    };
    let (sin, cos) = v8math::sin_cos(yaw);
    soldier.position = [position.x + 0.9 * sin, position.y, position.z + 0.9 * cos];
    soldier.previous = soldier.position;
    body.combat.health = 1.;
    let phase = tick % 18;
    assert!(world.stage_bot_combat_input(Frame {
        stamp: Stamp {
            installation,
            tick,
            life,
            binding: Some(binding)
        },
        fire: phase < 4,
        fire_pressed: phase == 0,
        guard: false,
        guard_pressed: false,
        hold_to_block: false,
        forward: 0.,
        right: 0.,
    }));
    Some((life, victim, weapon))
}

#[test]
#[ignore = "requires a prepared Hero Assault mission and character poses"]
fn a_registered_bot_swing_kills_through_its_own_pose_and_scores_once() {
    let (mut world, admitted, _) = prepared_world(hero_assault(), 31, 8);
    let mut clock = WorldClockState {
        hz: 60.,
        max_steps: 4.,
        max_debt: 8.,
        budget: f64::INFINITY,
        ..Default::default()
    };
    let installation = 11;
    world.install_bot_combat_input(installation);
    let mut cursor = 0;
    let mut kill = None;
    let mut deliveries = 0;
    let mut points = [0; 2];
    for tick in 1..=600 {
        let pair = if kill.is_none() {
            drive_bot(&mut world, installation, tick)
        } else {
            None
        };
        flat_step(&mut world, admitted.units, &mut clock, tick);
        let events = world.infantry_contacts().unwrap();
        for event in events.since(cursor) {
            let (Some((attacker, victim, binding)), Cause::Saber { weapon, .. }) =
                (pair, event.cause)
            else {
                continue;
            };
            if event.source == Some(attacker)
                && event.target == victim
                && event.outcome == Outcome::Hit
            {
                assert_eq!((weapon, event.before), (binding, 1.));
                assert!(event.after <= 0.);
                assert!(
                    world
                        .sim
                        .store
                        .unit
                        .get(Ref::new(victim.slot, victim.slab))
                        .unwrap()
                        .dead
                );
                assert!(kill.replace((attacker, victim, binding, tick)).is_none());
            }
        }
        cursor = events.latest;
        let view = world.mission_view().unwrap();
        for death in &view.host.deaths {
            if let Some(team @ 1..=2) = death.killer_team {
                if team != death.victim_team {
                    points[team as usize - 1] += 1;
                }
            }
            if let Some((attacker, victim, binding, _)) = kill {
                if death.victim == victim {
                    assert_eq!(death.killer, Some(attacker));
                    assert_ne!(death.killer_team, Some(death.victim_team));
                    assert_eq!(death.weapon, Some(Weapon::MeleeBinding(binding)));
                    assert!(
                        matches!(death.cause, Some(Cause::Saber { weapon, .. }) if weapon == binding)
                    );
                    deliveries += 1;
                }
            }
        }
        assert_eq!(view.score().teams, points);
        if kill.is_some_and(|(_, _, _, at)| tick >= at + 3) {
            assert_eq!(deliveries, 1);
            return;
        }
    }
    panic!("no registered attacker produced a credited posed-blade kill");
}

#[test]
#[ignore = "requires a prepared Hero Assault mission and character poses"]
fn mission_goal_input_scores_a_bot_blade_kill_without_a_player_trigger() {
    let (mut world, admitted, _) = prepared_world(hero_assault(), 4242, 16);
    let mut clock = WorldClockState {
        hz: 60., max_steps: 4., max_debt: 8., budget: f64::INFINITY, ..Default::default()
    };
    let mut points = [0; 2];
    for tick in 1..=7200 {
        flat_step(&mut world, admitted.units, &mut clock, tick);
        let view = world.mission_view().unwrap();
        let mut blade_kill = false;
        for death in &view.host.deaths {
            if let Some(team @ 1..=2) = death.killer_team {
                if team == death.victim_team { continue; }
                points[team as usize - 1] += 1;
                if let Some(Cause::Saber { weapon, .. }) = death.cause {
                    let killer = death.killer.expect("blade kill has an owner");
                    let body = world.sim.store.unit.get(Ref::new(killer.slot, killer.slab)).unwrap();
                    assert!(!body.human);
                    assert_eq!(death.weapon, Some(Weapon::MeleeBinding(weapon)));
                    blade_kill = true;
                }
            }
        }
        assert_eq!(view.score().teams, points);
        if blade_kill { return; }
    }
    panic!("mission opponents produced no credited blade kill");
}
