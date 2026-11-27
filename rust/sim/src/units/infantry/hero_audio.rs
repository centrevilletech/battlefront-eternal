//! Zero-allocation audio snapshot of 2005 hero powers, active choke holds,
//! in-flight thrown lightsabers, and live lightsaber states (player and bots).
//!
//! Only 2005 battles install `world.units.infantry`; 2004 battles return
//! `false` immediately and never populate or allocate this snapshot.

use crate::combat::powers::{self, world::LOG_LEN, LifeKey, PowerEventKind};
use crate::world::owner::World;

pub const MAX_HERO_POWER_EVENTS: usize = LOG_LEN;
pub const MAX_HERO_HOLDS: usize = 32;
pub const MAX_HERO_THROWS: usize = 32;
pub const MAX_HERO_SABERS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeroPowerEvent {
    pub sequence: u32,
    pub tick: u32,
    pub caster_slot: u32,
    pub caster_generation: u32,
    pub caster_pos: [f64; 3],
    pub target_pos: Option<[f64; 3]>,
    pub weapon: u32,
    pub target_weapon: u32,
    pub kind: PowerEventKind,
    pub bot: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeroPowerHold {
    pub caster_slot: u32,
    pub caster_generation: u32,
    pub weapon: u32,
    pub caster_pos: [f64; 3],
    pub target_pos: [f64; 3],
    pub bot: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeroSaberThrow {
    pub owner_slot: u32,
    pub owner_generation: u32,
    pub launched: u32,
    pub weapon: u32,
    pub saber_weapon: u32,
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub bot: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeroSaberState {
    pub owner_slot: u32,
    pub owner_generation: u32,
    pub weapon_hash: u32,
    pub combo_hash: u32,
    pub state_hash: u32,
    pub blades_off: u8,
    pub thrown: bool,
    pub position: [f64; 3],
    pub human: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct HeroAudioSnapshot {
    pub power_events: [Option<HeroPowerEvent>; MAX_HERO_POWER_EVENTS],
    pub power_count: usize,
    pub power_latest: u32,
    pub holds: [Option<HeroPowerHold>; MAX_HERO_HOLDS],
    pub hold_count: usize,
    pub throws: [Option<HeroSaberThrow>; MAX_HERO_THROWS],
    pub throw_count: usize,
    pub sabers: [Option<HeroSaberState>; MAX_HERO_SABERS],
    pub saber_count: usize,
}

impl Default for HeroAudioSnapshot {
    fn default() -> Self {
        Self {
            power_events: [None; MAX_HERO_POWER_EVENTS],
            power_count: 0,
            power_latest: 0,
            holds: [None; MAX_HERO_HOLDS],
            hold_count: 0,
            throws: [None; MAX_HERO_THROWS],
            throw_count: 0,
            sabers: [None; MAX_HERO_SABERS],
            saber_count: 0,
        }
    }
}

impl HeroAudioSnapshot {
    pub fn clear(&mut self) {
        self.power_count = 0;
        self.power_latest = 0;
        self.hold_count = 0;
        self.throw_count = 0;
        self.saber_count = 0;
    }

    /// Power events strictly after `cursor`, in oldest-first sequence order.
    pub fn power_events_since(&self, cursor: u32) -> impl Iterator<Item = HeroPowerEvent> + '_ {
        self.power_events[..self.power_count]
            .iter()
            .flatten()
            .copied()
            .filter(move |ev| ev.sequence > cursor)
    }

    pub fn holds(&self) -> impl Iterator<Item = HeroPowerHold> + '_ {
        self.holds[..self.hold_count].iter().flatten().copied()
    }

    pub fn throws(&self) -> impl Iterator<Item = HeroSaberThrow> + '_ {
        self.throws[..self.throw_count].iter().flatten().copied()
    }

    pub fn sabers(&self) -> impl Iterator<Item = HeroSaberState> + '_ {
        self.sabers[..self.saber_count].iter().flatten().copied()
    }
}

fn life_pos(w: &World, key: LifeKey) -> Option<[f64; 3]> {
    let u = key
        .unit(&w.sim.store)
        .or_else(|| w.sim.store.unit.at(key.slot).map(|(_, u)| u))?;
    let p = u.position();
    Some([p.x, p.y, p.z])
}

impl World {
    /// Captures all 2005 hero power events, active choke holds, thrown sabers,
    /// and live hero saber states (player and bots) into `out` with zero heap
    /// allocations. Returns `false` for battles without 2005 infantry state.
    pub fn hero_audio(&self, out: &mut HeroAudioSnapshot) -> bool {
        let Some(infantry) = self.units.infantry.as_deref() else {
            return false;
        };
        out.clear();

        if let Some(powers) = powers::powers_in(self) {
            let log = powers.log();
            out.power_latest = log.latest;
            for ev in log.since(0) {
                if out.power_count >= MAX_HERO_POWER_EVENTS {
                    break;
                }
                let caster_pos = life_pos(self, ev.caster).unwrap_or([0.0; 3]);
                let target_pos = ev.target.and_then(|t| life_pos(self, t));
                let target_weapon = ev
                    .target
                    .and_then(|t| {
                        let slot = t.slot as usize;
                        let hero = infantry.heroes.get(slot)?.as_ref()?;
                        let life = infantry.lives.get(slot)?.as_ref()?;
                        if life.slab != t.generation || life.generation != t.life {
                            return None;
                        }
                        infantry.weapons.get(hero.class)?.as_ref().map(|w| w.weapon)
                    })
                    .unwrap_or(0);
                out.power_events[out.power_count] = Some(HeroPowerEvent {
                    sequence: ev.sequence,
                    tick: ev.tick,
                    caster_slot: ev.caster.slot,
                    caster_generation: ev.caster.life as u32,
                    caster_pos,
                    target_pos,
                    weapon: ev.weapon,
                    target_weapon,
                    kind: ev.kind,
                    bot: ev.bot,
                });
                out.power_count += 1;
            }

            powers.visit_holds(|caster, weapon, target| {
                if out.hold_count >= MAX_HERO_HOLDS {
                    return;
                }
                let Some(u) = caster.unit(&self.sim.store) else {
                    return;
                };
                if !u.alive || u.dead || u.removed {
                    return;
                }
                let cp = u.position();
                let caster_pos = [cp.x, cp.y, cp.z];
                let target_pos = target
                    .and_then(|t| life_pos(self, t))
                    .unwrap_or(caster_pos);
                out.holds[out.hold_count] = Some(HeroPowerHold {
                    caster_slot: caster.slot,
                    caster_generation: caster.life as u32,
                    weapon,
                    caster_pos,
                    target_pos,
                    bot: !u.human,
                });
                out.hold_count += 1;
            });
        }

        for (slot, hero) in infantry.heroes.iter().enumerate() {
            let Some(hero) = hero else { continue };
            let Some((r, u)) = self.sim.store.unit.at(slot as u32) else {
                continue;
            };
            if !u.alive || u.dead || u.removed {
                continue;
            }
            let Some(life) = infantry
                .lives
                .get(slot)
                .and_then(Option::as_ref)
                .filter(|l| l.slab == r.gen && l.generation == u.generation)
            else {
                continue;
            };
            let Some(weapon) = infantry.weapons.get(hero.class).and_then(Option::as_ref) else {
                continue;
            };
            let p = u.position();
            let pos = [p.x, p.y, p.z];
            let thrown = hero.flight.is_active();
            if out.saber_count < MAX_HERO_SABERS {
                let state_idx = usize::from(hero.state.state.0);
                let state_hash = weapon.state_hashes.get(state_idx).copied().unwrap_or(0);
                out.sabers[out.saber_count] = Some(HeroSaberState {
                    owner_slot: r.slot,
                    owner_generation: life.generation as u32,
                    weapon_hash: weapon.weapon,
                    combo_hash: weapon.machine.name_hash,
                    state_hash,
                    blades_off: hero.state.blades_off_mask,
                    thrown,
                    position: pos,
                    human: u.human,
                });
                out.saber_count += 1;
            }
            if thrown && out.throw_count < MAX_HERO_THROWS {
                let prev = hero.flight.prev_position;
                let cur = hero.flight.position;
                let vel = [
                    (cur[0] - prev[0]) * 60.0,
                    (cur[1] - prev[1]) * 60.0,
                    (cur[2] - prev[2]) * 60.0,
                ];
                let throw_weapon = weapon.offhand.as_ref().map_or(0, |o| o.weapon);
                out.throws[out.throw_count] = Some(HeroSaberThrow {
                    owner_slot: r.slot,
                    owner_generation: life.generation as u32,
                    launched: hero.launched,
                    weapon: throw_weapon,
                    saber_weapon: weapon.weapon,
                    position: cur,
                    velocity: vel,
                    bot: !u.human,
                });
                out.throw_count += 1;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::powers::{
        world::{self, Forward, NoHost},
        PowerInput,
    };
    use crate::gen::{Member, Unit, UnitBody};
    use crate::movement::soldier::swbf2::{ControlSpeed, State, Tuning};
    use crate::units::f3;
    use bfe_content::json;

    fn tuning() -> Tuning {
        let row = |forward, strafe, turn| ControlSpeed { forward, strafe, turn };
        Tuning {
 recover_from_tumble: false,
 getup_front_seconds: crate::movement::soldier::swbf2::DEFAULT_GETUP_FRONT_SECONDS,
 getup_back_seconds: crate::movement::soldier::swbf2::DEFAULT_GETUP_BACK_SECONDS,
            stand: row(1.0, 1.0, 1.0),
            crouch: row(0.6, 0.6, 1.0),
            sprint: row(1.8, 0.4, 0.5),
            jump: row(0.05, 0.05, 0.5),
            jet: row(0.9, 0.7, 0.5),
            jetpack: None,
            roll: row(0.0, 0.0, 0.3),
            tumble: row(0.0, 0.0, 0.1),
            sprint_accelerate_time: 0.3,
            energy_capacity: 100.0,
            energy_floor: 0.0,
            energy_restore: 7.0,
            energy_restore_idle: 11.0,
            energy_drain_sprint: 13.0,
            energy_min_sprint: 9.0,
            energy_cost_jump: 0.0,
            energy_cost_jump_sprint: None,
            energy_cost_roll: 17.0,
            jump_height: 2.5,
            jump_forward_speed_factor: 1.1,
            jump_strafe_speed_factor: 0.9,
            roll_speed_factor: 1.4,
            roll_frames: 18,
        }
    }

    fn soldier(team: f64, at: [f64; 3], yaw: f64, human: bool) -> Unit {
        let mut u = Unit { generation: 1.0, team, alive: true, human, ..Default::default() };
        u.combat.health = 500.0;
        u.combat.max_health = 500.0;
        if let UnitBody::Soldier(s) = &mut u.soldier {
            s.position = at;
            s.aim_yaw = yaw;
            s.normal = [0.0, 1.0, 0.0];
            s.grounded = true;
        }
        u
    }

    #[test]
    fn blast_emits_cast_with_zero_victims_and_marks_bot_flag() {
        let mut w = World::new();
        w.movement.select_family(bfe_family::GameFamily::Swbf2).unwrap();
        f3::f3_init_in(&mut w, 0, f64::NAN, 0);
        f3::reserve_native_slots_in(&mut w, 8);
        let bot_unit = soldier(2.0, [5.0, 0.0, 2.0], 0.0, false);
        let caster = LifeKey::from(&f3::with_owner_rows(&mut w, |s, o| o.build(s, bot_unit)));
        w.sim.store.units.list.push(Member::Unit(caster.reference()));

        let class = json::parse(
            r#"{"class":"inv_slam_hero","weapons":[{"class":"inv_slam_blade","base":"melee","slot":1,"ExplosionName":"inv_slam_exp"}]}"#,
        )
        .unwrap();
        let exp = json::parse(
            r#"{"Damage":"200","DamageRadiusInner":"1","DamageRadiusOuter":"6","Push":"8","PushRadiusInner":"1","PushRadiusOuter":"5","HurtOwner":"0"}"#,
        )
        .unwrap();
        world::install_in(&mut w, 8, [("inv_slam_hero", &class)], |_| Some(exp.clone())).unwrap();
        let kit = world::powers_in(&w).unwrap().kit_of("inv_slam_hero").unwrap();

        world::play_explosion_in(&mut w, &mut NoHost, caster.reference(), kit);
        let events: Vec<_> = world::powers_in(&w).unwrap().log().since(0).collect();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, PowerEventKind::Cast);
        assert!(events[0].bot, "bot caster must be marked bot=true");
        assert_eq!(events[0].weapon, crate::combat::combo::zero_fnv1a_lower("inv_slam_blade"));
    }

    #[test]
    fn visit_holds_tracks_active_choke_until_released() {
        let mut w = World::new();
        w.movement.select_family(bfe_family::GameFamily::Swbf2).unwrap();
        f3::f3_init_in(&mut w, 0, f64::NAN, 0);
        f3::reserve_native_slots_in(&mut w, 8);
        let caster = LifeKey::from(&f3::with_owner_rows(&mut w, |s, o| {
            o.build(s, soldier(1.0, [0.0, 0.0, 0.0], 0.0, true))
        }));
        let victim = LifeKey::from(&f3::with_owner_rows(&mut w, |s, o| {
            o.build(s, soldier(2.0, [0.0, 0.0, 6.0], 0.0, false))
        }));
        w.sim.store.units.list.push(Member::Unit(caster.reference()));
        w.sim.store.units.list.push(Member::Unit(victim.reference()));

        let grip = json::parse(
            r#"{"class":"inv_choker","weapons":[{"class":"inv_grip","base":"areaeffectweapon","slot":2,"channel":1,"FireType":"Hold","EffectType":"Choke","AreaType":"Ray","AreaRange":"14","AreaRadius":"8","AreaHeight":"3","MaxTargets":"1","EffectDamage":"50","EffectStrength":"4","ThrustFactor":"0.7","EnergyDrain":"10","EnergyDrainRate":"5","InitialSalvoDelay":"0.05"}]}"#,
        )
        .unwrap();
        world::install_in(&mut w, 8, [("inv_choker", &grip)], |_| None).unwrap();
        let kit = world::powers_in(&w).unwrap().kit_of("inv_choker").unwrap();
        let t = tuning();
        let mut energy = State::new(&t);

        for tick in 0..10 {
            let input = Forward {
                power: PowerInput { selected: Some(0), pressed: tick == 0, held: true },
                ..Forward::default()
            };
            world::forward_in(&mut w, &mut NoHost, caster.reference(), kit, input, &mut energy, &t);
            world::finish_tick_in(&mut w, 1.0 / 60.0);
        }
        let mut holds = Vec::new();
        world::powers_in(&w).unwrap().visit_holds(|c, weapon, tgt| holds.push((c, weapon, tgt)));
        assert_eq!(holds.len(), 1);
        assert_eq!(holds[0].0, caster);
        assert_eq!(holds[0].1, crate::combat::combo::zero_fnv1a_lower("inv_grip"));
        assert_eq!(holds[0].2, Some(victim));

        // Release trigger: hold disappears immediately.
        world::forward_in(&mut w, &mut NoHost, caster.reference(), kit, Forward::default(), &mut energy, &t);
        world::finish_tick_in(&mut w, 1.0 / 60.0);
        holds.clear();
        world::powers_in(&w).unwrap().visit_holds(|c, weapon, tgt| holds.push((c, weapon, tgt)));
        assert!(holds.is_empty(), "released choke must clear immediately");
    }

    #[test]
    fn world_without_2005_infantry_returns_false() {
        let w = World::new();
        let mut snap = HeroAudioSnapshot::default();
        assert!(!w.hero_audio(&mut snap));
    }
}
