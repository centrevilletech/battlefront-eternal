//! An occupied seat's instruments at one completed simulation tick.
use bfe_sim::{
    space::{
        flight::{SpaceFlightProfile, SpaceLockTargetKind},
        objects::SpaceBuildingAttachment,
        SpaceBattlefield,
    },
    world::owner::World,
};

#[derive(Debug)]
pub(crate) struct Label {
    pub key: String,
    pub fallback: String,
}

impl Label {
    fn entity(class: &str, display: Option<&str>) -> Self {
        Self {
            key: format!("entity.{}", class.replacen('_', ".", 1)),
            fallback: display.unwrap_or(class).to_owned(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Equipment {
    pub label: Label,
    pub icon: String,
}

#[derive(Debug)]
pub(crate) struct RegisteredSystem {
    pub building: usize,
    pub label: Label,
    pub show_marker: bool,
    /// Where the mission puts its mark on its ship picture (fractions across and down).
    pub hud: [f32; 2],
    /// The other parts the mission links to it, whose health its mark shows with its own.
    pub linked: Vec<usize>,
}

#[derive(Debug)]
pub(crate) struct TeamNames {
    pub id: u8,
    pub name: String,
    pub icon: String,
    pub system_icon: String,
    pub ship_icon: String,
    pub shield_icon: String,
}

#[derive(Debug)]
pub(crate) struct Names {
    pub craft: Vec<Label>,
    pub weapons: Vec<Vec<Equipment>>,
    pub turret_weapons: Vec<Option<Equipment>>,
    pub objects: Vec<Label>,
    pub mission: std::sync::OnceLock<MissionHud>,
}

#[derive(Debug)]
pub(crate) struct MissionHud {
    pub systems: Vec<RegisteredSystem>,
    pub teams: [TeamNames; 2],
}

impl Names {
    pub fn new(
        space: &SpaceBattlefield,
        definitions: Option<&bfe_content::defs::Defs>,
    ) -> Self {
        Self::from_parts(&space.profiles, &space.buildings, definitions)
    }

    /// The names from a battle's craft profiles and buildings: a local
    /// battle's own, or an online battle's client data.
    pub fn from_parts(
        profiles: &[SpaceFlightProfile],
        buildings: &[SpaceBuildingAttachment],
        definitions: Option<&bfe_content::defs::Defs>,
    ) -> Self {
        let mut craft = Vec::with_capacity(profiles.len());
        let mut weapons = Vec::with_capacity(profiles.len());
        for profile in profiles {
            let class = definitions.and_then(|d| d.class(&profile.side, &profile.class_name));
            let display = class.and_then(|c| c.record.get("displayName")?.as_str());
            craft.push(Label::entity(&profile.class_name, display));
            weapons.push(
                profile
                    .weapons
                    .iter()
                    .enumerate()
                    .map(|(i, weapon)| {
                        let record = definitions
                            .zip(class)
                            .and_then(|(defs, class)| {
                                let aligned =
                                    class.weapons.get(i).and_then(|&w| defs.weapons.get(w));
                                let matches = |w: &bfe_content::defs::WeaponDef| {
                                    w.class.is_some_and(|id| defs.names.text(id) == weapon.name)
                                };
                                aligned.filter(|w| matches(w)).or_else(|| {
                                    class
                                        .weapons
                                        .iter()
                                        .filter_map(|&w| defs.weapons.get(w))
                                        .find(|w| matches(w))
                                })
                            })
                            .map(|w| &w.record);
                        let field = |key| record.and_then(|r| r.get(key)?.as_str());
                        Equipment {
                            label: Label {
                                key: format!("weapons.{}", weapon.name.replacen('_', ".", 2)),
                                fallback: field("displayName").unwrap_or(&weapon.name).to_owned(),
                            },
                            icon: field("HUDTag")
                                .or_else(|| field("IconTexture"))
                                .unwrap_or("")
                                .to_owned(),
                        }
                    })
                    .collect(),
            );
        }
        let turret_weapons = buildings
            .iter()
            .map(|b| {
                let w = &b.turret.as_ref()?.weapon;
                Some(Equipment {
                    label: Label {
                        key: format!("weapons.{}", w.name.replacen('_', ".", 2)),
                        fallback: w.name.clone(),
                    },
                    icon: w.hud_tag.clone(),
                })
            })
            .collect();
        Self {
            craft,
            weapons,
            turret_weapons,
            objects: buildings
                .iter()
                .map(|b| Label::entity(&b.class, None))
                .collect(),
            mission: Default::default(),
        }
    }
}

impl MissionHud {
    pub fn new(world: &World, host: &bfe_sim::battle::script::state::HostState) -> Self {
        // ScriptPostLoad declares these placements while the first tick is stepping.
        // Each later publication reads their current building lives.
        let building = |row: &bfe_sim::battle::script::state::CriticalSystemRecord| {
            let bfe_sim::battle::rules::events::ObjectHandle::Prop(handle) = &row.object else { return None };
            world.space_mission_building(handle)
        };
        let systems = host.critical_systems.iter()
            .filter_map(|row| {
                let b = building(row)?;
                // A linked set's first name carries the mark for the rest.
                let linked = host.linked_critical_systems.iter()
                    .find(|names| names.first().is_some_and(|n| n.eq_ignore_ascii_case(&row.name)))
                    .map(|names| names[1..].iter()
                        .filter_map(|n| host.critical_systems.iter().find(|r| r.name.eq_ignore_ascii_case(n)))
                        .filter_map(|r| building(r).map(|b| b.building_index))
                        .collect())
                    .unwrap_or_default();
                Some(RegisteredSystem {
                    building: b.building_index,
                    label: Label::entity(&b.class, Some(&row.name)),
                    show_marker: row.show_marker,
                    hud: [row.hud_u, row.hud_v],
                    linked,
                })
            }).collect();
        let order = host.space_teams
            .filter(|ids| matches!(ids, [1, 2] | [2, 1])).unwrap_or([1, 2]);
        Self {
            systems,
            teams: std::array::from_fn(|i| {
                let team = host.setup.teams.get(&order[i]);
                TeamNames {
                    id: order[i] as u8,
                    name: team.and_then(|t| t.name.clone()).unwrap_or_default(),
                    icon: team.and_then(|t| t.icons.first().cloned()).unwrap_or_default(),
                    // Ship, shield, then critical-system bitmaps, each in mission team order.
                    system_icon: host.space_assault_bitmaps.get(4 + i)
                        .map(|s| s.to_ascii_lowercase()).unwrap_or_default(),
                    ship_icon: host.space_assault_bitmaps.get(i)
                        .map(|s| s.to_ascii_lowercase()).unwrap_or_default(),
                    shield_icon: host.space_assault_bitmaps.get(2 + i)
                        .map(|s| s.to_ascii_lowercase()).unwrap_or_default(),
                }
            }),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Weapon {
    pub index: usize,
    pub rounds: Option<u32>,
    pub clip: Option<(f32, f32)>,
    pub heat: Option<(f32, f32)>,
    pub overheated: bool,
    pub refire: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mount {
    Fighter(u32),
    Turret(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContactName {
    Craft(usize),
    Building(usize),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Tracked {
    pub id: u32,
    pub life: u32,
    pub team: u8,
    pub name: ContactName,
    pub hp: f32,
    pub max_hp: f32,
    pub position: [f64; 3],
    pub radius: f64,
    pub locked: bool,
    pub fraction: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Cockpit {
    pub mount: Mount,
    pub slot: usize,
    pub generation: u32,
    pub seat: u8,
    pub can_land: bool,
    pub team: u8,
    pub hull: f32,
    pub hull_max: f32,
    pub speed: f32,
    pub speed_max: f32,
    pub energy: f32,
    pub energy_max: f32,
    pub weapons: [Option<Weapon>; 2],
    pub target: Option<Tracked>,
}

fn overheat_refire(heat: f64, threshold: f64, overheated: bool) -> Option<f32> {
    if !overheated {
        return None;
    }
    let span = (1.0 - threshold).max(1e-4);
    Some((((heat - threshold) / span).clamp(0.0, 1.0)) as f32)
}

fn fighter_refire(
    spec: &bfe_sim::space::flight::SpaceWeaponSpec,
    rt: &bfe_sim::space::flight::SpaceWeaponRuntime,
) -> Option<f32> {
    if spec.heat_per_shot > 0.0 && rt.overheated {
        return overheat_refire(rt.heat, spec.heat_threshold, rt.overheated);
    }
    if rt.reload_timer > 0.0 && spec.reload_time > 0.0 {
        return Some(((1.0 - rt.reload_timer / spec.reload_time).clamp(0.0, 1.0)) as f32);
    }
    if spec.shot_delay >= 0.5 && rt.clock.cooldown > 0.0 {
        return Some(((1.0 - rt.clock.cooldown / spec.shot_delay).clamp(0.0, 1.0)) as f32);
    }
    None
}

fn fighter_weapon(index: usize, mount: &bfe_sim::space::SpaceSeatWeapon<'_>) -> Weapon {
    let uses_heat = mount.spec.heat_per_shot > 0.0;
    let has_clip = mount.spec.rounds_per_clip > 0;
    Weapon {
        index,
        rounds: has_clip.then_some(mount.runtime.ammo_in_clip),
        clip: has_clip.then_some((
            mount.runtime.ammo_in_clip as f32,
            mount.spec.rounds_per_clip as f32,
        )),
        heat: uses_heat.then_some((mount.runtime.heat as f32, 1.0)),
        overheated: mount.runtime.overheated,
        refire: fighter_refire(mount.spec, mount.runtime),
    }
}

/// One craft weapon of an online cockpit, as the replicated own craft gives it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CockpitWeaponInput {
    pub index: usize,
    pub selected: bool,
    pub ammo_in_clip: u32,
    pub heat: f32,
}

/// The lock an online cockpit's selected weapon builds or holds.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CockpitLockInput {
    pub kind: SpaceLockTargetKind,
    pub weapon_index: usize,
    pub target_index: usize,
    pub progress: f64,
    pub locked: bool,
}

/// What the online cockpit shows of a locked target.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CockpitTargetFacts {
    pub id: u32,
    pub life: u32,
    pub team: u8,
    pub name: ContactName,
    pub hp: f32,
    pub max_hp: f32,
    pub position: [f64; 3],
    pub radius: f64,
}

/// An online battle's own craft seat, as its replicated state gives it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CockpitInput<'a> {
    pub slot: usize,
    pub generation: u32,
    pub seat: u8,
    pub can_land: bool,
    pub team: u8,
    pub profile_index: usize,
    pub profile: &'a SpaceFlightProfile,
    pub hull: f32,
    pub hull_max: f32,
    pub speed: f32,
    pub energy: f32,
    pub weapons: &'a [CockpitWeaponInput],
    pub lock: Option<CockpitLockInput>,
}

impl Cockpit {
    /// An online battle's cockpit from the replicated own craft, by the rule
    /// `read` follows for a local one: each channel shows the seat's first
    /// weapon or its selected one, and the selected weapon's lock. The wire
    /// carries no clip or refire state, so those gauges stay empty online.
    pub fn build(
        input: CockpitInput<'_>,
        mut resolve_target: impl FnMut(SpaceLockTargetKind, usize) -> Option<CockpitTargetFacts>,
    ) -> Self {
        let mut weapons = [None; 2];
        for w in input.weapons {
            let Some(spec) = input.profile.weapons.get(w.index) else { continue };
            if spec.seat != input.seat {
                continue;
            }
            let channel = &mut weapons[usize::from(spec.channel != 0)];
            if channel.is_none() || w.selected {
                let has_clip = spec.rounds_per_clip > 0;
                *channel = Some(Weapon {
                    index: w.index,
                    rounds: has_clip.then_some(w.ammo_in_clip),
                    clip: has_clip.then_some((w.ammo_in_clip as f32, spec.rounds_per_clip as f32)),
                    heat: (spec.heat_per_shot > 0.).then_some((w.heat, 1.0)),
                    overheated: false,
                    refire: None,
                });
            }
        }
        let target = input.lock.and_then(|l| {
            if l.kind == SpaceLockTargetKind::None || !input.weapons.iter().any(|w| w.index == l.weapon_index && w.selected) {
                return None;
            }
            let spec = input.profile.weapons.get(l.weapon_index)?;
            let facts = resolve_target(l.kind, l.target_index)?;
            let fraction = if l.locked {
                1.
            } else if spec.lock_time > 0. {
                (l.progress / spec.lock_time).clamp(0., 1.) as f32
            } else {
                0.
            };
            Some(Tracked {
                id: facts.id,
                life: facts.life,
                team: facts.team,
                name: facts.name,
                hp: facts.hp,
                max_hp: facts.max_hp,
                position: facts.position,
                radius: facts.radius,
                locked: l.locked,
                fraction,
            })
        });
        Self {
            mount: Mount::Fighter(u32::try_from(input.profile_index).unwrap_or(u32::MAX)),
            slot: input.slot,
            generation: input.generation,
            seat: input.seat,
            can_land: input.can_land,
            team: input.team,
            hull: input.hull,
            hull_max: input.hull_max,
            speed: input.speed,
            speed_max: input.profile.boost_speed.max(input.profile.max_speed) as f32,
            energy: input.energy,
            energy_max: input.profile.energy_bar as f32,
            weapons,
            target,
        }
    }

    pub fn read(world: &World, space: &SpaceBattlefield, player: u32) -> Option<Self> {
        if let Some((_, gun)) = space.pilot_terminal(player) {
            let turret = gun.turret.as_ref()?;
            let building_slot = space
                .buildings
                .iter()
                .position(|b| b.building_index == gun.building_index)?;
            let snap = world.space_building_snapshot(gun.building_index)?;
            let uses_heat = turret.weapon.heat_per_shot > 0.0;
            let refire = if uses_heat {
                overheat_refire(turret.heat, turret.weapon.heat_threshold, turret.overheated)
            } else if turret.weapon.shot_delay >= 0.5 && turret.clock.cooldown > 0.0 {
                Some(((1.0 - turret.clock.cooldown / turret.weapon.shot_delay).clamp(0.0, 1.0)) as f32)
            } else {
                None
            };
            return Some(Self {
                mount: Mount::Turret(u32::try_from(building_slot).ok()?),
                slot: gun.building_index,
                generation: snap.generation,
                seat: 1,
                can_land: false,
                team: snap.team as u8,
                hull: snap.health as f32,
                hull_max: snap.max_health as f32,
                speed: 0.,
                speed_max: 0.,
                energy: 0.,
                energy_max: 0.,
                weapons: [
                    Some(Weapon {
                        index: building_slot,
                        rounds: None,
                        clip: None,
                        heat: uses_heat.then_some((turret.heat as f32, 1.0)),
                        overheated: turret.overheated,
                        refire,
                    }),
                    None,
                ],
                target: None,
            });
        }
        let seat = space.pilot_seat(player)?;
        let f = world.space_seat_fighter(seat)?;
        let profile = &space.profiles[f.profile_index];
        let mut weapons = [None; 2];
        let mut target = None;
        for i in 0..profile.weapons.len() {
            let Some(mount) = world.space_seat_weapon(seat, i) else {
                continue;
            };
            let channel = &mut weapons[usize::from(mount.spec.channel != 0)];
            if channel.is_none() || mount.selected {
                *channel = Some(fighter_weapon(i, &mount));
            }
            let l = mount.lock;
            if !mount.selected || l.kind == SpaceLockTargetKind::None {
                continue;
            }
            let index = l.target_index;
            let identity = match l.kind {
                SpaceLockTargetKind::Fighter => space
                    .fighters
                    .get(index)
                    .filter(|t| t.alive && u64::from(t.generation) == l.target_generation)
                    .map(|t| {
                        let p = &space.profiles[t.profile_index];
                        (
                            t.id,
                            t.generation,
                            t.team,
                            ContactName::Craft(t.profile_index),
                            t.health,
                            t.max_health,
                            t.position,
                            (p.bounds_max[0] - p.bounds_min[0]).abs() * 0.5,
                        )
                    }),
                SpaceLockTargetKind::Object => space
                    .buildings
                    .iter()
                    .enumerate()
                    .find(|(_, b)| b.building_index == index)
                    .and_then(|(slot, b)| {
                        let s = world.space_building_snapshot(index)?;
                        (s.alive && s.owner.life.to_bits() == l.target_generation).then_some((
                            index,
                            s.generation,
                            s.team,
                            ContactName::Building(slot),
                            s.health,
                            s.max_health,
                            s.position,
                            b.radius,
                        ))
                    }),
                SpaceLockTargetKind::None => None,
            };
            if let Some((id, life, team, name, hp, max_hp, position, radius)) = identity {
                let fraction = if l.locked {
                    1.
                } else if mount.spec.lock_time > 0. {
                    (l.progress / mount.spec.lock_time).clamp(0., 1.) as f32
                } else {
                    0.
                };
                target = Some(Tracked {
                    id: id as u32,
                    life,
                    team: team as u8,
                    name,
                    hp: hp as f32,
                    max_hp: max_hp as f32,
                    position,
                    radius,
                    locked: l.locked,
                    fraction,
                });
            }
        }
        Some(Self {
            mount: Mount::Fighter(u32::try_from(f.profile_index).ok()?),
            slot: f.id,
            generation: f.generation,
            seat: seat.seat,
            can_land: matches!(world.space_landing_permission(seat),
                bfe_sim::space::flight::SpaceLandingPermission::Allowed { .. }),
            team: f.team as u8,
            hull: f.health as f32,
            hull_max: f.max_health as f32,
            speed: f.airspeed as f32,
            speed_max: profile.boost_speed.max(profile.max_speed) as f32,
            energy: f.energy as f32,
            energy_max: profile.energy_bar as f32,
            weapons,
            target,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overheat_refire_tracks_remaining_cooldown_above_threshold() {
        assert_eq!(overheat_refire(0.9, 0.5, false), None);
        assert_eq!(overheat_refire(1.0, 0.5, true), Some(1.0));
        assert_eq!(overheat_refire(0.75, 0.5, true), Some(0.5));
        assert_eq!(overheat_refire(0.5, 0.5, true), Some(0.0));
        assert_eq!(overheat_refire(0.2, 0.5, true), Some(0.0));
    }

    #[test]
    fn fighter_refire_tracks_clip_reload_and_slow_shot_delay() {
        use bfe_sim::space::flight::{SpaceWeaponRuntime, SpaceWeaponSpec};
        let spec = SpaceWeaponSpec {
            rounds_per_clip: 8,
            reload_time: 4.0,
            shot_delay: 0.8,
            ..Default::default()
        };
        let mut rt = SpaceWeaponRuntime::default();
        assert_eq!(fighter_refire(&spec, &rt), None);

        rt.reload_timer = 4.0;
        assert_eq!(fighter_refire(&spec, &rt), Some(0.0));
        rt.reload_timer = 2.0;
        assert_eq!(fighter_refire(&spec, &rt), Some(0.5));
        rt.reload_timer = 1.0;
        assert_eq!(fighter_refire(&spec, &rt), Some(0.75));

        rt.reload_timer = 0.0;
        rt.clock.cooldown = 0.2;
        assert_eq!(fighter_refire(&spec, &rt), Some(0.75));

        let fast = SpaceWeaponSpec {
            shot_delay: 0.15,
            ..spec
        };
        assert_eq!(fighter_refire(&fast, &rt), None);
    }

    #[test]
    fn fighter_weapon_scales_clip_gauge_by_rounds_per_clip() {
        use bfe_sim::space::{
            flight::{SpaceLockState, SpaceWeaponRuntime, SpaceWeaponSpec},
            SpaceSeatWeapon,
        };
        let lock = SpaceLockState::default();
        let clip_spec = SpaceWeaponSpec {
            rounds_per_clip: 12,
            reload_time: 2.5,
            ..Default::default()
        };
        let mut rt = SpaceWeaponRuntime {
            ammo_in_clip: 6,
            ..Default::default()
        };
        let half = fighter_weapon(
            0,
            &SpaceSeatWeapon {
                spec: &clip_spec,
                runtime: &rt,
                lock: &lock,
                selected: true,
            },
        );
        assert_eq!(half.rounds, Some(6));
        assert_eq!(half.clip, Some((6.0, 12.0)));
        assert_eq!(half.heat, None);
        assert_eq!(half.refire, None);

        rt.ammo_in_clip = 0;
        rt.reload_timer = 1.25;
        let reloading = fighter_weapon(
            0,
            &SpaceSeatWeapon {
                spec: &clip_spec,
                runtime: &rt,
                lock: &lock,
                selected: true,
            },
        );
        assert_eq!(reloading.rounds, Some(0));
        assert_eq!(reloading.clip, Some((0.0, 12.0)));
        assert_eq!(reloading.refire, Some(0.5));

        let heat_spec = SpaceWeaponSpec {
            heat_per_shot: 0.2,
            heat_threshold: 0.5,
            ..Default::default()
        };
        let heat_rt = SpaceWeaponRuntime {
            heat: 0.75,
            overheated: true,
            ..Default::default()
        };
        let hot = fighter_weapon(
            1,
            &SpaceSeatWeapon {
                spec: &heat_spec,
                runtime: &heat_rt,
                lock: &lock,
                selected: false,
            },
        );
        assert_eq!(hot.rounds, None);
        assert_eq!(hot.clip, None);
        assert_eq!(hot.heat, Some((0.75, 1.0)));
        assert!(hot.overheated);
        assert_eq!(hot.refire, Some(0.5));
    }
}
