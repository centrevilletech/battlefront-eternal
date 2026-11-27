//! Read-only soldier observations for mission battle screens.
use super::{
    contact::{identity, LifeId},
    World,
};
use crate::{
    combat::{
        defs::{Js, JsObj},
        module::defs_ref,
    },
    gen::Member,
};

#[derive(Clone, Copy)]
pub struct Equipped<'a> {
    pub id: u32,
    pub name: &'a str,
    pub power: bool,
    pub base: &'a str,
    pub icon: &'a str,
    pub icon_fallback: &'a str,
    pub reticle: &'a str,
    pub rounds: Option<u32>,
    pub reserve: Option<u32>,
    pub charge: Option<f32>,
    pub ready_in: f64,
    pub clip: Option<f32>,
    pub heat: Option<f32>,
    pub overheated: bool,
    pub refire: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LockedTarget {
    pub life: LifeId,
    pub team: u8,
    pub class: Option<u16>,
    pub center: [f64; 3],
    pub radius: f64,
    pub health: f64,
    pub max_health: f64,
}

#[derive(Clone, Copy)]
pub struct Soldier<'a> {
    pub life: LifeId,
    pub team: u8,
    pub human: bool,
    pub alive: bool,
    pub awarded: bool,
    pub position: [f64; 3],
    pub yaw: f64,
    pub health: f64,
    pub max_health: f64,
    pub energy: f64,
    pub max_energy: f64,
    pub guarding: bool,
    pub slots: [Option<Equipped<'a>>; 2],
}

fn text<'a>(obj: &'a JsObj, key: &str) -> &'a str {
    if let Js::Str(value) = obj.get(key) {
        value
    } else {
        ""
    }
}

/// Primary HUD icon key (`HUDTag`, then `HUDModel`, then weapon `class` so
/// `hudtransforms` maps like `all_weap_inf_thermaldetonator -> hud_thermaldetonator`
/// resolve) paired with a fallback (`GeometryName`, then `IconTexture`) for guns
/// whose 2005 HUD slot shows the weapon's own 3D mesh.
fn weapon_icons(obj: &JsObj) -> (&str, &str) {
    let hud_tag = text(obj, "HUDTag");
    let hud_model = text(obj, "HUDModel");
    let class = text(obj, "class");
    let primary = if !hud_tag.is_empty() {
        hud_tag
    } else if !hud_model.is_empty() {
        hud_model
    } else {
        class
    };
    let geometry = text(obj, "GeometryName");
    let icon_tex = text(obj, "IconTexture");
    let fallback = if !geometry.is_empty() {
        geometry
    } else {
        icon_tex
    };
    (primary, fallback)
}

impl World {
    /// Whether this life of the unit holds a seat of a Space craft (its pilot or a gunner).
    fn in_space_craft(&self, unit: crate::store::Ref<crate::gen::Unit>) -> bool {
        self.space().is_some_and(|space| {
            space.fighters.iter().filter(|f| f.alive)
                .flat_map(|f| std::iter::once(f.pilot).chain(f.gunners))
                .flatten()
                .any(|crew| crate::combat::owners::resolve(&self.sim.store, crew.owner) == Some(unit))
        })
    }

    pub fn visit_screen_soldiers(&self, mut observe: impl FnMut(Soldier<'_>)) {
        let Some(infantry) = self.units.infantry.as_ref() else {
            return;
        };
        let weapons = &defs_ref(&self.combat).weapons;
        for member in &self.sim.store.units.list {
            let Member::Unit(r) = member else { continue };
            let Some(unit) = self.sim.store.unit.get(*r) else {
                continue;
            };
            let Some(life) = infantry
                .lives
                .get(r.slot as usize)
                .and_then(Option::as_ref)
                .filter(|l| l.slab == r.gen && l.generation == unit.generation)
            else {
                continue;
            };
            // A soldier in a seat, a vehicle's or a Space craft's (its pilot or a
            // gunner), has no body on the battlefield's screens.
            if unit.removed || unit.seat.is_some() || self.in_space_craft(*r) {
                continue;
            }
            let mut slots = [None; 2];
            if unit.human {
                let mut counts = [0; 2];
                for w in &unit.combat.weapons {
                    if !weapons.has(w.def) {
                        continue;
                    }
                    let Some(level) = weapons.class(w.def).levels.get(w.level as usize) else {
                        continue;
                    };
                    let Some(channel) = (0..2).find(|&c| w.in_channel(weapons, c as f64)) else {
                        continue;
                    };
                    let selected = unit.combat.selected[channel] == f64::from(counts[channel]);
                    counts[channel] += 1;
                    if !selected {
                        continue;
                    }
                    let base = text(&level.obj, "base");
                    let rounds = level.obj.get("RoundsPerClip").num();
                    let unlimited = rounds <= 0. || matches!(base, "melee" | "meleethrow");
                    let count = |v: f64| {
                        (!unlimited && v.is_finite())
                            .then(|| (v * jsrt::conv::js_max(1., rounds) + 0.5).floor() as u32)
                    };
                    let power = level.obj.get("EnergyDrain").num() > 0.
                        || base == "areaeffectweapon";
                    let heat_per_shot = level.obj.get("HeatPerShot").num();
                    let uses_heat = (heat_per_shot.is_finite() && heat_per_shot > 0.)
                        || (w.heat.is_finite() && w.heat > 0.);
                    let heat_threshold = level.obj.get("HeatThreshold").num();
                    let threshold = if heat_threshold.is_finite() { heat_threshold.clamp(0., 0.99) } else { 0. };
                    let reload_time = level.obj.get("ReloadTime").num();
                    let shot_delay = level.obj.get("ShotDelay").num();
                    let refire = if uses_heat && w.overheated {
                        let span = jsrt::conv::js_max(1. - threshold, 1e-4);
                        Some(((w.heat - threshold) / span).clamp(0., 1.) as f32)
                    } else if w.reload_left > 0. && reload_time.is_finite() && reload_time > 0. {
                        Some((1. - w.reload_left / reload_time).clamp(0., 1.) as f32)
                    } else if w.wait > 0. && shot_delay.is_finite() && shot_delay >= 0.45 {
                        Some((1. - w.wait / shot_delay).clamp(0., 1.) as f32)
                    } else {
                        None
                    };
                    let (icon, icon_fallback) = weapon_icons(&level.obj);
                    slots[channel] = Some(Equipped {
                        id: w.def.0,
                        name: text(&level.obj, "displayName"),
                        base,
                        power,
                        icon,
                        icon_fallback,
                        reticle: text(&level.obj, "ReticuleTexture"),
                        rounds: count(w.clip),
                        reserve: if w.bottomless { None } else { count(w.reserve) },
                        charge: w.charging.then_some(w.charge as f32),
                        ready_in: if power {
                            if w.reload_left > w.wait { w.reload_left } else { w.wait }
                        } else {
                            0.
                        },
                        clip: (!unlimited && !w.bottomless && w.clip.is_finite())
                            .then_some(w.clip.clamp(0., 1.) as f32),
                        heat: uses_heat.then_some(w.heat.clamp(0., 1.) as f32),
                        overheated: w.overheated,
                        refire,
                    });
                }
            }
            let pos = unit.position();
            // The guard that actually answers an attack: the combo state can still say
            // guarding after a hold dropped it.
            let guarding = life.guard.is_some();
            observe(Soldier {
                life: identity(*r, unit),
                human: unit.human,
                team: unit.team as u8,
                alive: unit.alive && !unit.dead && !unit.combat.dead,
                awarded: unit
                    .character
                    .and_then(|c| self.sim.store.character.get(c))
                    .is_some_and(|c| c.hero == Some(true)),
                position: [pos.x, pos.y, pos.z],
                yaw: unit.aim_yaw(),
                health: unit.combat.health,
                max_health: unit.combat.max_health,
                energy: life.movement.energy,
                max_energy: infantry.classes[life.class].tuning.energy_capacity,
                guarding,
                slots,
            });
        }
    }
}

impl World {
    /// The admitted connection keeps its character across death and redeployment.
    pub fn screen_local_character(&self) -> Option<(usize, u8)> {
        let player = self.units.infantry.as_ref()?.player;
        let owner = self.input.players.pool.get(player as usize)?;
        if owner.spectator || owner.disconnected_at.is_some() {
            return None;
        }
        let store = &self.sim.store;
        let character = crate::battle::f4_connections::wire_character_in(
            &self.battle, store, owner.character.as_ref()?,
        )?;
        let slot = store.battle.characters.iter().position(|r| *r == character)?;
        Some((slot, store.character.get(character)?.team as u8))
    }

    /// Whether the admitted connection's character has what he controls
    /// outside the battlefield: his leaving countdown runs (at or above 0).
    pub fn screen_local_leaving(&self) -> bool {
        let Some(player) = self.units.infantry.as_ref().map(|i| i.player) else { return false };
        let Some(connection) = self.input.players.pool.get(player as usize) else { return false };
        if connection.spectator || connection.disconnected_at.is_some() {
            return false;
        }
        let store = &self.sim.store;
        let Some(handle) = connection.character.as_ref() else { return false };
        crate::battle::f4_connections::wire_character_in(&self.battle, store, handle)
            .and_then(|character| store.character.get(character))
            .is_some_and(|c| c.out_of_bounds >= 0.0)
    }

    /// Characters outlive their bodies, including while they wait for a spawn.
    pub fn visit_screen_roster(&self, mut observe: impl FnMut(usize, bool, Option<u16>)) {
        let Some(infantry) = self.units.infantry.as_ref() else { return };
        let store = &self.sim.store;
        for (slot, handle) in store.battle.characters.iter().enumerate() {
            if let Some(character) = store.character.get(*handle) {
                let name = character.unit.and_then(|u| store.unit.get(u))
                    .map(|u| u.class_name).or(character.class_name);
                let class = name.and_then(|name| infantry.classes.iter()
                    .find(|class| class.name == name)?.registry_id);
                observe(slot, character.human, class);
            }
        }
    }

    pub fn screen_character(&self, life: LifeId) -> Option<usize> {
        let r = crate::store::Ref::new(life.slot, life.slab);
        let unit = self.sim.store.unit.get(r)?;
        if identity(r, unit) != life {
            return None;
        }
        let owner = unit.character?;
        self.sim
            .store
            .battle
            .characters
            .iter()
            .position(|c| *c == owner)
    }

    /// The local player has chosen what to deploy as and waits for a body.
    pub fn local_deployment_chosen(&self) -> bool {
        self.sim.store.character.iter().map(|(_, c)| c).find(|c| c.human)
            .is_some_and(|c| c.unit.is_none() && c.spawn.as_ref().is_some_and(|spawn| spawn.ready && spawn.timer <= 0.))
    }

    pub fn local_spawn_due(&self, tick: u32) -> Option<u32> {
        // The connection that owns local input, never another human place
        // (a parked one waits for a player and has no countdown).
        let (has_body, seconds) = self.local_mission_spawn_clock()?;
        (!has_body && seconds.is_finite() && seconds > 0.)
            .then(|| tick.saturating_add((seconds * 60.).ceil() as u32))
    }
}

impl World {
    pub fn screen_unit_capacity(&self) -> usize {
        self.units
            .infantry
            .as_ref()
            .map_or(0, |profile| profile.lives.len())
    }
}

impl World {
    /// Resolve durable weapon identity while the admitted definition tables are available.
    pub fn screen_weapon(&self, weapon: super::contact::Weapon) -> Option<&str> {
        use super::contact::Weapon;
        let defs = &defs_ref(&self.combat).weapons;
        match weapon {
            Weapon::MeleeBinding(hash) => defs
                .classes
                .iter()
                .flatten()
                .map(|c| text(&c.base().obj, "class"))
                .find(|name| {
                    !name.is_empty() && crate::combat::combo::zero_fnv1a_lower(name) == hash
                }),
            Weapon::RangedClass(id) if defs.has(id) => {
                Some(text(&defs.class(id).base().obj, "class"))
            }
            Weapon::SpaceBuilding(slot) => self
                .space
                .as_deref()?
                .buildings
                .get(usize::try_from(slot).ok()?)?
                .turret
                .as_ref()
                .map(|t| t.weapon.name.as_str())
                .filter(|name| !name.is_empty()),
            Weapon::SpaceFighter { profile, weapon } => self
                .space
                .as_deref()?
                .profiles
                .get(usize::try_from(profile).ok()?)?
                .weapons
                .get(usize::from(weapon))
                .map(|w| w.name.as_str())
                .filter(|name| !name.is_empty()),
            _ => None,
        }
    }

    /// Fallback entity class name when a mounted ship weapon has no weapon localization.
    pub fn screen_weapon_entity(&self, weapon: super::contact::Weapon) -> Option<&str> {
        use super::contact::Weapon;
        match weapon {
            Weapon::SpaceBuilding(slot) => self
                .space
                .as_deref()?
                .buildings
                .get(usize::try_from(slot).ok()?)
                .map(|b| b.class.as_str())
                .filter(|name| !name.is_empty()),
            _ => None,
        }
    }

    /// The living target currently locked by the local on-foot player in a 2005 battle.
    pub fn screen_local_lock(&self) -> Option<LockedTarget> {
        let powers = crate::combat::powers::powers_in(self)?;
        let store = &self.sim.store;
        let (caster_ref, caster) = store.units.list.iter().find_map(|member| {
            let Member::Unit(r) = *member else { return None };
            let u = store.unit.get(r)?;
            (u.human && u.alive && !u.dead && !u.combat.dead && !u.removed && u.seat.is_none())
                .then_some((r, u))
        })?;
        let target_key = powers.lock(crate::combat::powers::LifeKey::of(caster_ref, caster))?;
        let target_ref = target_key.reference();
        let target = target_key
            .unit(store)
            .filter(|u| u.alive && !u.dead && !u.combat.dead && !u.removed)?;
        let posture = match &target.soldier {
            crate::gen::UnitBody::Soldier(s) => s.posture,
            crate::gen::UnitBody::Droideka(_) => crate::gen::Posture::Stand,
        };
        let p = target.position();
        let [middle, radius, _] = crate::combat::combat::blast_body(posture);
        let class = self.units.infantry.as_ref().and_then(|infantry| {
            infantry
                .classes
                .iter()
                .find(|c| c.name == target.class_name)
                .and_then(|c| c.registry_id)
        });
        Some(LockedTarget {
            life: identity(target_ref, target),
            team: target.team as u8,
            class,
            center: [p.x, p.y + middle, p.z],
            radius,
            health: target.combat.health,
            max_health: target.combat.max_health,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::powers::{self, Forward, LifeKey, NoHost};
    use crate::gen::{Member, Unit, UnitBody};
    use crate::movement::soldier::swbf2::{ControlSpeed, State, Tuning};
    use crate::units::f3;

    fn test_tuning() -> Tuning {
        let row = ControlSpeed {
            forward: 1.0,
            strafe: 1.0,
            turn: 1.0,
        };
        Tuning {
 recover_from_tumble: false,
 getup_front_seconds: crate::movement::soldier::swbf2::DEFAULT_GETUP_FRONT_SECONDS,
 getup_back_seconds: crate::movement::soldier::swbf2::DEFAULT_GETUP_BACK_SECONDS,
            stand: row,
            crouch: row,
            sprint: row,
            jump: row,
            jet: row,
            jetpack: None,
            roll: row,
            tumble: row,
            sprint_accelerate_time: 0.3,
            energy_capacity: 100.0,
            energy_floor: 0.0,
            energy_restore: 10.0,
            energy_restore_idle: 10.0,
            energy_drain_sprint: 10.0,
            energy_min_sprint: 5.0,
            energy_cost_jump: 0.0,
            energy_cost_jump_sprint: None,
            energy_cost_roll: 10.0,
            jump_height: 2.0,
            jump_forward_speed_factor: 1.0,
            jump_strafe_speed_factor: 1.0,
            roll_speed_factor: 1.0,
            roll_frames: 12,
        }
    }

    fn soldier(team: f64, human: bool, pos: [f64; 3], health: f64) -> Unit {
        let mut u = Unit {
            generation: 1.0,
            team,
            human,
            alive: true,
            ..Default::default()
        };
        u.combat.health = health;
        u.combat.max_health = 1000.0;
        if let UnitBody::Soldier(s) = &mut u.soldier {
            s.position = pos;
            s.normal = [0.0, 1.0, 0.0];
            s.grounded = true;
        }
        u
    }

    #[test]
    fn local_lock_tracks_only_living_human_on_foot_lock() {
        let legacy = World::new();
        assert!(legacy.screen_local_lock().is_none());

        let mut w = World::new();
        w.movement
            .select_family(bfe_family::GameFamily::Swbf2)
            .unwrap();
        f3::f3_init_in(&mut w, 0, f64::NAN, 0);
        f3::reserve_native_slots_in(&mut w, 8);
        let spawn = |w: &mut World, u: Unit| {
            let k = LifeKey::from(&f3::with_owner_rows(w, |store, owner| owner.build(store, u)));
            if !w.sim.store.units.list.contains(&Member::Unit(k.reference())) {
                w.sim.store.units.list.push(Member::Unit(k.reference()));
            }
            k
        };
        let human = spawn(&mut w, soldier(1.0, true, [0.0, 0.0, 0.0], 1000.0));
        let bot = spawn(&mut w, soldier(1.0, false, [0.5, 0.0, 0.0], 1000.0));
        let enemy = spawn(&mut w, soldier(2.0, false, [0.0, 0.0, 10.0], 750.0));

        let class_record = bfe_content::json::parse(
            r#"{"class":"inv_duelist","weapons":[{"class":"inv_blade","base":"melee","slot":1,"LockOnRange":"35"}]}"#,
        )
        .unwrap();
        powers::install_in(&mut w, 8, [("inv_duelist", &class_record)], |_| None).unwrap();
        let kit = powers::powers_in(&w).unwrap().kit_of("inv_duelist").unwrap();
        let t = test_tuning();
        let mut energy = State::new(&t);

        assert!(w.screen_local_lock().is_none());

        // A bot taking a lock must never show on the local player's HUD.
        powers::forward_in(
            &mut w,
            &mut NoHost,
            bot.reference(),
            kit,
            Forward {
                lock_pressed: true,
                bot: true,
                ..Forward::default()
            },
            &mut energy,
            &t,
        );
        assert!(powers::powers_in(&w).unwrap().lock(bot) == Some(enemy));
        assert!(w.screen_local_lock().is_none());

        // The local human pressing lock acquires the enemy ahead.
        powers::forward_in(
            &mut w,
            &mut NoHost,
            human.reference(),
            kit,
            Forward {
                lock_pressed: true,
                ..Forward::default()
            },
            &mut energy,
            &t,
        );
        let expected_life = identity(enemy.reference(), enemy.unit(&w.sim.store).unwrap());
        let locked = w.screen_local_lock().expect("human lock observed");
        assert_eq!(locked.life, expected_life);
        assert_eq!(locked.team, 2);
        assert_eq!(locked.center, [0.0, 1.0, 10.0]);
        assert_eq!(locked.radius, 1.4626);
        assert_eq!(locked.health, 750.0);
        assert_eq!(locked.max_health, 1000.0);

        // Seated local human does not emit an on-foot lock.
        human.unit_mut(&mut w.sim.store).unwrap().seat = Some(crate::store::Ref::new(1, 1));
        assert!(w.screen_local_lock().is_none());
        human.unit_mut(&mut w.sim.store).unwrap().seat = None;
        assert!(w.screen_local_lock().is_some());

        // Pressing lock again releases it.
        powers::forward_in(
            &mut w,
            &mut NoHost,
            human.reference(),
            kit,
            Forward {
                lock_pressed: true,
                ..Forward::default()
            },
            &mut energy,
            &t,
        );
        assert!(w.screen_local_lock().is_none());
    }

    #[test]
    fn weapon_icons_fall_back_from_hud_tag_to_class_and_geometry() {
        let obj = |pairs: &[(&str, &str)]| JsObj {
            fields: pairs
                .iter()
                .map(|(k, v)| (std::rc::Rc::from(*k), Js::Str(std::rc::Rc::from(*v))))
                .collect(),
            def: None,
        };
        assert_eq!(
            weapon_icons(&obj(&[
                ("class", "imp_weap_hero_flamethrower"),
                ("HUDTag", "hud_imp_flamethrower"),
                ("GeometryName", "imp_weap_inf_rifle"),
            ])),
            ("hud_imp_flamethrower", "imp_weap_inf_rifle")
        );
        assert_eq!(
            weapon_icons(&obj(&[
                ("class", "all_weap_inf_thermaldetonator"),
                ("GeometryName", "com_weap_inf_grenadethermal"),
            ])),
            ("all_weap_inf_thermaldetonator", "com_weap_inf_grenadethermal")
        );
        assert_eq!(
            weapon_icons(&obj(&[
                ("class", "imp_weap_hero_bobarifle"),
                ("GeometryName", "imp_weap_inf_rifle"),
            ])),
            ("imp_weap_hero_bobarifle", "imp_weap_inf_rifle")
        );
    }
}
