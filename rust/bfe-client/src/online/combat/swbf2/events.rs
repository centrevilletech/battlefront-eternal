//! Translate facts from a fixed battle step into the mission's visual bindings.
use super::{Burst, Combat, MissionEffects, MountClass, MountObservation, Shot, Surface, Value};
use bfe_content::zero_hash::fnv1a;
use std::collections::BTreeMap;

/// The class hash is the one published by the battle. A weapon selects its ordered ordnance slot.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum EffectClass {
    Weapon(u32),
    Ordnance(u32),
    Entity(u32),
    Explosion(u32),
}

#[derive(Clone, Copy)]
pub enum BlastCause {
    Detonation,
    Destroyed,
    Expired,
    Triggered,
    Death,
}

impl EffectClass {
    fn parse(path: &str) -> Option<Self> {
        let (kind, name) = path.split_once('/')?;
        let hash = fnv1a(name);
        Some(match kind {
            "wpnc" => Self::Weapon(hash),
            "ordc" => Self::Ordnance(hash),
            "entc" => Self::Entity(hash),
            "expc" => Self::Explosion(hash),
            _ => return None,
        })
    }
}

#[derive(Clone, Copy)]
pub struct EffectPoint {
    /// Stable across repeat publications, unique to the event within its battle.
    pub serial: u64,
    pub tick: u32,
    pub position: [f64; 3],
    pub normal: Option<[f64; 3]>,
}

impl EffectPoint {
    fn burst(self, group: usize) -> Burst {
        Burst {
            serial: self.serial,
            tick: self.tick as f64,
            group,
            position: self.position,
            normal: self.normal,
        }
    }
}

/// World pose of a particular life at a fixed tick. Craft and component ids share the caller's namespace.
#[derive(Clone, Copy)]
pub struct EffectBody {
    pub owner: u64,
    pub generation: u32,
    pub class: u32,
    pub tick: u32,
    pub position: [f64; 3],
    pub rotation: [f64; 4],
    pub velocity: [f64; 3],
}

impl EffectBody {
    pub(super) fn sample(
        self,
        class: MountClass,
        health: f64,
        thrusting: bool,
    ) -> MountObservation {
        MountObservation {
            owner: self.owner,
            generation: self.generation,
            class,
            tick: self.tick as f64,
            position: self.position,
            rotation: self.rotation,
            velocity: self.velocity,
            health,
            thrusting,
        }
    }
}

#[derive(Clone, Copy)]
pub enum PowerEffect {
    Charge,
    Owner,
    Aimer,
}
impl PowerEffect {
    fn field(self) -> &'static str {
        match self {
            Self::Charge => "ChargeUpEffect",
            Self::Owner => "PlayEffectOnOwner",
            Self::Aimer => "PlayEffectOnOwnerAimer",
        }
    }
}

#[derive(Default)]
pub(super) struct EventBindings {
    names: BTreeMap<EffectClass, String>,
    flashes: BTreeMap<u32, super::Flash>,
    rounds: BTreeMap<EffectClass, Vec<Option<EffectClass>>>,
    blasts: BTreeMap<EffectClass, [Option<EffectClass>; 5]>,
}
impl EventBindings {
    pub fn new(pack: &Value, groups: &super::Groups) -> Self {
        let mut table = Self::default();
        for section in ["bindings", "links", "rounds"] {
            if let Some(rows) = pack.get(section).and_then(Value::as_object) {
                for (owner, fields) in rows.entries() {
                    let Some(key) = EffectClass::parse(owner) else {
                        continue;
                    };
                    table.names.entry(key).or_insert_with(|| owner.into());
                    if section == "links" {
                        if let Some(slots) = fields.get("OrdnanceName").and_then(Value::as_array) {
                            table.rounds.insert(
                                key,
                                slots
                                    .iter()
                                    .map(|v| v.as_str().and_then(EffectClass::parse))
                                    .collect(),
                            );
                        }
                        let linked = |field| {
                            fields
                                .get(field)?
                                .at(0)?
                                .as_str()
                                .and_then(EffectClass::parse)
                        };
                        let general = linked("ExplosionName").or_else(|| linked("Explosion"));
                        table.blasts.insert(
                            key,
                            [
                                general,
                                linked("ExplosionDestruct").or(general),
                                linked("ExplosionExpire").or(general),
                                linked("ExplosionTrigger").or(general),
                                linked("ExplosionDeath").or(general),
                            ],
                        );
                    }
                }
            }
        }
        for (class, path) in &table.names {
            if let EffectClass::Weapon(id) = class {
                table.flashes.insert(
                    *id,
                    super::Flash {
                        muzzle: super::rounds::Muzzle::read(pack, path),
                        groups: super::flash(groups, path),
                    },
                );
            }
        }
        table
    }
    fn projectile(&self, source: EffectClass, slot: usize) -> Option<EffectClass> {
        match source {
            EffectClass::Weapon(_) => *self.rounds.get(&source)?.get(slot)?,
            _ => Some(source),
        }
    }
}

impl MissionEffects {
    /// Preallocate external launches during admission, within the presentation's record budget.
    pub fn reserve_weapon_lives(&mut self, count: usize) {
        self.rounds
            .reserve_live(count.min(bfe_presentation::ordnance_layout::ordnance_limits::RECORDS));
    }
    /// End the flying bolt on the published termination tick, keeping its authored fade.
    pub fn weapon_end(&mut self, serial: u64, tick: u32) {
        self.rounds.finish_live(serial, tick as f64, None);
    }
    fn group_for(&self, class: EffectClass, property: &str, variant: usize) -> Option<usize> {
        self.binding(self.events.names.get(&class)?, property, variant)
    }
    fn emit_at(&mut self, group: Option<usize>, at: EffectPoint) -> bool {
        group.is_some_and(|group| self.enqueue(at.burst(group), None))
    }
    pub fn bolt_impact(
        &mut self,
        source: EffectClass,
        slot: usize,
        surface: Surface,
        at: EffectPoint,
    ) -> bool {
        let Some(round) = self.events.projectile(source, slot) else {
            return false;
        };
        let field = |key| self.group_for(round, key, 0);
        let generic = field("ImpactEffect");
        let terrain = field("ImpactEffectTerrain").or(generic);
        let solid = field("ImpactEffectStatic").or(terrain);
        let group = match surface {
            Surface::Terrain => terrain,
            Surface::Static => solid,
            Surface::Soft => field("ImpactEffectSoft").or(solid),
            Surface::Rigid => field("ImpactEffectRigid").or(solid),
            Surface::Water => field("ImpactEffectWater").or(generic),
            Surface::Shield => field("ImpactEffectShield").or(generic),
        };
        self.emit_at(group, at)
    }
    /// A displayed shot ended on a surface at this fixed step. Expiry in air uses weapon_end.
    pub fn weapon_contact(
        &mut self,
        shot_serial: u64,
        source: EffectClass,
        slot: usize,
        surface: Surface,
        at: EffectPoint,
    ) -> bool {
        self.weapon_end(shot_serial, at.tick);
        self.bolt_impact(source, slot, surface, at)
    }

    /// A destroyed object, detonated round or explosion class at the actual point of detonation.
    pub fn explosion(
        &mut self,
        source: EffectClass,
        slot: usize,
        cause: BlastCause,
        water: bool,
        at: EffectPoint,
    ) -> bool {
        let Some(source) = self.events.projectile(source, slot) else {
            return false;
        };
        let blast = match source {
            EffectClass::Explosion(_) => Some(source),
            _ => self
                .events
                .blasts
                .get(&source)
                .and_then(|v| v[cause as usize]),
        };
        let group = blast
            .and_then(|b| {
                water
                    .then(|| self.group_for(b, "WaterEffect", 0))
                    .flatten()
                    .or_else(|| self.group_for(b, "Effect", 0))
            })
            .or_else(|| self.group_for(source, "DeathEffect", 0));
        self.emit_at(group, at)
    }
    /// Retire this life even when its class has no destruction effect.
    pub fn body_destroyed(
        &mut self,
        owner: u64,
        generation: u32,
        class: u32,
        cause: BlastCause,
        water: bool,
        at: EffectPoint,
    ) -> bool {
        self.retire_mount(owner, generation);
        self.explosion(EffectClass::Entity(class), 0, cause, water, at)
    }

    /// A complete fixed-step hull snapshot, including the observed engine and damage state.
    pub fn ship_state(&mut self, body: EffectBody, thrusting: bool, hull_fraction: f64) -> bool {
        self.mounts.ship(body, thrusting, hull_fraction)
    }

    /// A stationary charge began emitting its ordnance class's trail at the armed position.
    pub fn arm_charge(&mut self, source: EffectClass, slot: usize, at: EffectPoint) -> bool {
        let group = self
            .events
            .projectile(source, slot)
            .and_then(|ordnance| self.group_for(ordnance, "TrailEffect", 0));
        group.is_some_and(|g| self.enqueue(at.burst(g), Some(f64::INFINITY)))
    }

    /// Remove or disarm the charge at the published tick.
    pub fn disarm_charge(&mut self, armed_serial: u64, tick: u32) {
        self.stop_activation(armed_serial, tick);
    }

    /// End the armed emission and draw the triggered explosion at its own event position.
    pub fn trigger_charge(
        &mut self,
        armed_serial: u64,
        source: EffectClass,
        slot: usize,
        water: bool,
        at: EffectPoint,
    ) -> bool {
        self.disarm_charge(armed_serial, at.tick);
        self.explosion(source, slot, BlastCause::Triggered, water, at)
    }

    /// Thrust or takeoff powers exhaust. A coasting craft can still leave speed-driven contrails.
    pub fn engine_throttle(&mut self, body: EffectBody, thrusting: bool) -> bool {
        self.mounts.engine(body, thrusting)
    }
    /// A component or craft's own hull fraction, in the inclusive range zero to one.
    pub fn damage_smoke(&mut self, body: EffectBody, hull_fraction: f64) -> bool {
        self.mounts.damage(body, hull_fraction)
    }
    /// A particle-based power begins. Release uses the same activation serial.
    pub fn power_start(
        &mut self,
        weapon: u32,
        field: PowerEffect,
        variant: usize,
        at: EffectPoint,
    ) -> bool {
        let Some(group) = self.group_for(EffectClass::Weapon(weapon), field.field(), variant)
        else {
            return false;
        };
        self.enqueue(at.burst(group), Some(f64::INFINITY))
    }
    pub fn power_stop(&mut self, serial: u64, tick: u32) {
        self.stop_activation(serial, tick);
    }
    fn stop_activation(&mut self, serial: u64, tick: u32) {
        for (event, until) in &mut self.waiting {
            if event.serial == serial {
                if let Some(end) = until {
                    *end = end.min(tick as f64);
                }
            }
        }
        for (id, _, until) in &mut self.stopping {
            if *id == Some(serial) {
                *until = until.min(tick as f64);
            }
        }
    }
}

impl Combat {
    /// A gun without a replicated launch fired this weapon slot from this world-space fire point.
    pub fn weapon_fire(
        &mut self,
        weapon: u32,
        slot: usize,
        at: EffectPoint,
        direction: [f64; 3],
    ) -> bool {
        let Some(fx) = &mut self.mission_effects else {
            return false;
        };
        let class = EffectClass::Weapon(weapon);
        let Some(name) = fx.events.names.get(&class) else {
            return false;
        };
        if let Some(round) = fx.round(name, slot) {
            return self.observe_shot(Shot {
                round,
                serial: at.serial,
                tick: at.tick as f64,
                position: at.position,
                direction,
                contact: None,
            });
        }
        if fx.events.projectile(class, slot).is_none()
            || !at.position.iter().chain(&direction).all(|v| v.is_finite())
        {
            return false;
        }
        let Some(flash) = fx.events.flashes.get(&weapon) else {
            return false;
        };
        let (muzzle, groups) = (flash.muzzle, flash.groups.len());
        let length = direction.iter().map(|v| v * v).sum::<f64>().sqrt();
        if !length.is_finite() || length <= 0. {
            return false;
        }
        let direction = direction.map(|v| v / length);
        let tick = at.tick as f64;
        if (groups == 0 && muzzle.radius <= 0.) || !fx.remember((at.serial, usize::MAX, tick)) {
            return false;
        }
        let bolt = super::Bolt {
            flight: tick,
            position: at.position,
            direction,
            ..Default::default()
        };
        let end = tick + muzzle.ticks;
        let mut accepted = false;
        if let Some(mesh) = super::rounds::MeshFlash::at_barrel(&bolt, muzzle) {
            if fx.barrel_flashes.len() < fx.barrel_flashes.capacity() {
                fx.barrel_flashes.push(((u32::MAX, u32::MAX), mesh));
                accepted = true;
            } else {
                fx.refused = fx.refused.saturating_add(1);
            }
        }
        for i in 0..groups {
            let g = fx.events.flashes[&weapon].groups[i];
            accepted |= fx.enqueue(
                EffectPoint {
                    normal: Some(direction),
                    ..at
                }
                .burst(g),
                Some(end),
            );
        }
        accepted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn weapon_slots_and_destruction_causes_resolve_the_published_class_hash() {
        let pack = bfe_content::json::parse(
            r#"{"links":{
            "wpnc/iris":{"OrdnanceName":["ordc/bud","entc/pod"]},
            "entc/pod":{"ExplosionDestruct":["expc/mist"],"ExplosionExpire":["expc/dew"]}}}"#,
        )
        .unwrap();
        let binds = EventBindings::new(&pack, &Default::default());
        let weapon = EffectClass::Weapon(fnv1a("IRIS"));
        let pod = binds.projectile(weapon, 1).unwrap();
        assert_eq!(pod, EffectClass::Entity(fnv1a("pod")));
        assert_eq!(binds.projectile(weapon, 2), None);
        assert_eq!(
            binds.blasts[&pod][BlastCause::Expired as usize],
            EffectClass::parse("expc/dew")
        );
        assert_eq!(
            binds.blasts[&pod][BlastCause::Destroyed as usize],
            EffectClass::parse("expc/mist")
        );
    }
}
