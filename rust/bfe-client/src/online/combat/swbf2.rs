//! Resolve authored names at admission. Display frames consume numeric bindings and observed poses.
use super::{Bolt, Combat, Receiver};
use crate::online::effects::Spawner;
use bfe_content::Value;
use bfe_content_native::{loader::authored_json, BattleContent};
use bfe_presentation::network_layout::{net_kind as kind, net_limits, net_record as r};
use std::collections::BTreeMap;
use three::{Matrix4, Quaternion, Vector3};

mod beams;
mod blast_lights;
mod dust;
mod events;
pub(super) mod fade;
pub use events::{BlastCause, EffectBody, EffectClass, EffectPoint, PowerEffect};
mod mounts;
pub(in crate::online) mod powers;
mod rounds;
pub mod space;
use mounts::Mounts;
pub use mounts::{MountClass, MountObservation};
pub(in crate::online) use mounts::RemoteEngine;
pub(in crate::online) use rounds::{beam_glow_width, load_round_textures};
pub(super) use rounds::{prepare_rounds, BeamDraw};
pub use rounds::{Contact, RoundClass, Shot, Surface};

pub(super) fn particle_limit(pack: &Value) -> usize {
    const LIMIT: usize = 4096;
    pack.get("particleBudget")
        .and_then(Value::as_f64)
        .map_or(LIMIT, |n| n.clamp(1., LIMIT as f64) as usize)
}

type Groups = BTreeMap<String, BTreeMap<String, Vec<Option<usize>>>>;

#[derive(Clone, Copy)]
pub struct Burst {
    /// Stable across retransmissions, scoped to the battle epoch.
    pub serial: u64,
    pub tick: f64,
    pub group: usize,
    pub position: [f64; 3],
    pub normal: Option<[f64; 3]>,
}

#[derive(Clone, Default, PartialEq)]
struct Flash {
    groups: Vec<usize>,
    muzzle: rounds::Muzzle,
}

pub struct MissionEffects {
    groups: Groups,
    events: events::EventBindings,
    weapon_flash: Vec<(Flash, Vec<usize>)>,
    round_flash: Vec<Flash>,
    launches: Vec<Option<(u32, u32)>>,
    owners: Vec<((u32, u32), f64, usize)>,
    barrel_flashes: Vec<((u32, u32), rounds::MeshFlash)>,
    waiting: Vec<(Burst, Option<f64>)>,
    stopping: Vec<(Option<u64>, u32, f64)>,
    group_count: usize,
    recent: Vec<(u64, usize, f64)>,
    recent_at: usize,
    epoch: Option<f64>,
    mounts: Mounts,
    pub(super) rounds: rounds::Rounds,
    /// Bolt classes whose glow changes in flight, with their prepared styles.
    pub(super) fades: Vec<fade::GlowFade>,
    blast_lights: blast_lights::BlastLights,
    pub(in crate::online) powers: powers::PowerEffects,
    beams: Option<beams::Beams>,
    dust: Option<dust::SpaceDust>,
    pub(super) display_dt: f64,
    pub bursts: u32,
    pub refused: u32,
}

fn group(handles: &[(String, usize)], name: &str) -> Option<usize> {
    handles
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|pair| pair.1)
}
fn resolve_group(
    handles: &[(String, usize)],
    name: &str,
    owner: &str,
    field: &str,
    logged: &mut std::collections::BTreeSet<String>,
) -> Option<usize> {
    let found = group(handles, name);
    if found.is_none() && !name.is_empty() && logged.insert(name.to_ascii_lowercase()) {
        eprintln!(
            "Missing mission effect '{name}' in loaded effect pack (referenced by {owner}.{field})"
        );
    }
    found
}
fn values<'a>(value: &'a Value, key: &str) -> impl Iterator<Item = &'a str> {
    value
        .get(key)
        .and_then(Value::as_array)
        .unwrap_or(&[])
        .iter()
        .filter_map(Value::as_str)
}
fn flash(groups: &Groups, owner: &str) -> Vec<usize> {
    let mut result = Vec::new();
    if let Some(fields) = groups.get(owner) {
        for &g in fields.get("MuzzleFlashEffect").into_iter().flatten().flatten() {
            if !result.contains(&g) {
                result.push(g);
            }
        }
    }
    result
}
fn hit_groups(def: &Value, names: &[(String, usize)]) -> [Option<usize>; 6] {
    let field = |key| {
        def.get(key)
            .and_then(Value::as_str)
            .and_then(|n| group(names, n))
    };
    let any = field("ImpactEffect");
    let terrain = field("ImpactEffectTerrain").or(any);
    let solid = field("ImpactEffectStatic").or(terrain);
    [
        terrain,
        solid,
        field("ImpactEffectSoft").or(solid),
        field("ImpactEffectRigid").or(solid),
        field("ImpactEffectWater").or(any),
        None,
    ]
}

impl MissionEffects {
    pub(super) fn new(
        battle: &BattleContent,
        combat: &mut Combat,
        bridge: &mut crate::render_bridge::Bridge,
        textures: &[u32],
        prepared_rounds: Vec<rounds::PreparedRound>,
    ) -> Result<Self, String> {
        let beams = beams::Beams::new(battle, bridge, textures, &mut combat.particles.words)?;
        let dust = dust::SpaceDust::new(battle, bridge, textures, &mut combat.particles.words)?;
        let defs = battle.defs()?;
        let pack = battle.visual_effects();
        let names = &combat.effect_names;
        let mut missing_logged = std::collections::BTreeSet::new();
        let mut groups = Groups::new();
        if let Some(owners) = pack.get("bindings").and_then(Value::as_object) {
            for (owner, props) in owners.entries() {
                let Some(props) = props.as_object() else {
                    eprintln!("Skipping invalid mission effect bindings for {owner}");
                    continue;
                };
                let fields = props
                    .entries()
                    .map(|(key, v)| {
                        let resolved = v
                            .as_array()
                            .unwrap_or(&[])
                            .iter()
                            .map(|v| {
                                v.as_str()
                                    .and_then(|n| resolve_group(names, n, owner, key, &mut missing_logged))
                            })
                            .collect();
                        (key.to_owned(), resolved)
                    })
                    .collect();
                groups.insert(owner.to_owned(), fields);
            }
        }
        let make_flash = |weapon: &str| Flash {
            groups: flash(&groups, weapon),
            muzzle: rounds::Muzzle::read(pack, weapon),
        };
        let mut weapon_flash = vec![(Flash::default(), Vec::new())];
        for name in &defs.registry.weapons {
            let owner = format!("wpnc/{name}");
            let mut ids = Vec::new();
            if let Some(fields) = pack.get("links").and_then(|links| links.get(&owner)) {
                for round in values(fields, "OrdnanceName") {
                    if let Some(id) = defs.registry.combat.iter().position(|(_, def)| {
                        def.get("class")
                            .and_then(Value::as_str)
                            .is_some_and(|c| round.strip_prefix("ordc/") == Some(c))
                    }) {
                        ids.push(id + 1);
                    }
                }
            }
            weapon_flash.push((make_flash(&owner), ids));
        }
        let mut by_round: BTreeMap<String, Option<Flash>> = BTreeMap::new();
        if let Some(links) = pack.get("links").and_then(Value::as_object) {
            for (weapon, fields) in links.entries().filter(|(k, _)| k.starts_with("wpnc/")) {
                let found = make_flash(weapon);
                for round in values(fields, "OrdnanceName") {
                    let slot = by_round
                        .entry(round.to_owned())
                        .or_insert_with(|| Some(found.clone()));
                    if slot.as_ref() != Some(&found) {
                        *slot = None;
                    }
                }
            }
        }
        let mut round_flash = vec![Flash::default()];
        for (i, (_, def)) in defs.registry.combat.iter().enumerate() {
            let name = def
                .get("class")
                .or_else(|| def.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let owner = format!("ordc/{name}");
            round_flash.push(
                by_round
                    .get(&owner)
                    .and_then(Option::as_ref)
                    .cloned()
                    .unwrap_or_default(),
            );
            if let Some(style) = combat.definitions[i + 1] {
                for key in [
                    "ImpactEffect",
                    "ImpactEffectTerrain",
                    "ImpactEffectStatic",
                    "ImpactEffectSoft",
                    "ImpactEffectRigid",
                    "ImpactEffectWater",
                ] {
                    if let Some(n) = def.get(key).and_then(Value::as_str) {
                        resolve_group(names, n, &owner, key, &mut missing_logged);
                    }
                }
                combat.styles[style].impact = hit_groups(def, names);
                let mut get = |key| {
                    def.get(key)
                        .and_then(Value::as_str)
                        .and_then(|n| resolve_group(names, n, &owner, key, &mut missing_logged))
                };
                combat.styles[style].trail = get("TrailEffect").or_else(|| get("OrdnanceEffect"));
                let dry = get("Effect");
                combat.styles[style].explosion = [dry, get("WaterEffect").or(dry)];
            }
        }
        let mounts = Mounts::new(pack, &defs.registry.classes, names, battle, combat.effects.owner.content())?;
        combat.effects.enable_mounts();
        let rounds = rounds::Rounds::new(
            pack,
            &defs.registry.combat,
            combat,
            prepared_rounds,
            &groups,
        );
        let fades = fade::install(combat, &defs.registry.combat);
        if !fades.is_empty() {
            eprintln!("Mission bolts that change colour in flight: {}", fades.len());
        }
        let powers = powers::PowerEffects::new(battle, &combat.effect_names)?;
        let events = events::EventBindings::new(pack, &groups);
        Ok(Self {
            groups,
            events,
            weapon_flash,
            round_flash,
            launches: vec![None; combat.bolts.len()],
            owners: Vec::with_capacity(net_limits::RECORDS),
            barrel_flashes: Vec::with_capacity(net_limits::RECORDS),
            waiting: Vec::with_capacity(net_limits::RECORDS),
            recent: Vec::with_capacity(net_limits::RECORDS),
            recent_at: 0,
            group_count: battle
                .effects
                .iter()
                .find(|(n, _)| n == "battle")
                .and_then(|(_, lib)| lib.descriptor["groups"].as_array())
                .map_or(0, Vec::len),
            stopping: Vec::with_capacity(crate::online::effects::SLOTS),
            epoch: None,
            mounts,
            rounds,
            fades,
            blast_lights: blast_lights::BlastLights::new(&defs.registry.combat),
            powers,
            beams,
            dust,
            display_dt: 0.,
            bursts: 0,
            refused: 0,
        })
    }

    pub(super) fn draw_environment(
        &mut self,
        bridge: &mut crate::render_bridge::Bridge,
        camera: &[f64],
    ) {
        self.mounts.camera(camera);
        if let Some(dust) = &mut self.dust {
            dust.frame(bridge, camera, self.display_dt);
        }
        if let Some(beams) = &self.beams {
            beams.draw(bridge, camera);
        }
    }

    /// Flash the light of every explosion due on the display clock `tick`.
    pub(in crate::online) fn flash_blast_lights(&mut self, tick: f64, mut flash: impl FnMut([f64; 3], [f64; 3], f64, f64)) {
        self.blast_lights.flash_due(tick, &mut flash);
        self.powers.take_lights(|position, light| light.flash(position, &mut flash));
    }

    pub(in crate::online) fn weapon_muzzle(&self, id: usize) -> Option<rounds::Muzzle> {
        self.weapon_flash.get(id).map(|(flash, _)| flash.muzzle)
    }

    pub(in crate::online) fn draw_barrel_flashes(
        &self,
        tick: f64,
        particles: &mut crate::online::particles::Particles,
        bridge: &mut crate::render_bridge::Bridge,
        posed: impl Fn((u32, u32)) -> bool,
    ) -> u64 {
        self.barrel_flashes
            .iter()
            .filter(|(owner, flash)| !posed(*owner) && flash.draw(tick, particles, bridge))
            .count() as u64
    }

    /// Resolve once for a contact, deflection or force-power source. Repeated fields retain their order.
    pub fn binding(&self, owner: &str, property: &str, variant: usize) -> Option<usize> {
        self.groups
            .get(owner)
            .and_then(|s| s.get(property))
            .and_then(|groups| groups.get(variant))
            .copied()
            .flatten()
    }
    /// Submit a contact plane or activation at its observed simulation tick.
    pub fn observe(&mut self, event: Burst) {
        self.enqueue(event, None);
    }
    /// Sustained activations stop emitting on the observed release tick. Existing particles finish their tracks.
    pub fn observe_until(&mut self, event: Burst, release: f64) {
        if release.is_finite() && release > event.tick {
            self.enqueue(event, Some(release));
        }
    }
    fn enqueue(&mut self, event: Burst, release: Option<f64>) -> bool {
        if event.group >= self.group_count {
            return false;
        }
        if !event.tick.is_finite()
            || !event.position.iter().all(|x| x.is_finite())
            || event
                .normal
                .is_some_and(|n| !n.iter().all(|x| x.is_finite()))
        {
            return false;
        }
        if self.waiting.len() == self.waiting.capacity() {
            self.refused = self.refused.saturating_add(1);
            return false;
        }
        if !self.remember((event.serial, event.group, event.tick)) {
            return false;
        }
        self.waiting.push((event, release));
        true
    }
    fn remember(&mut self, key: (u64, usize, f64)) -> bool {
        if self.recent.contains(&key) {
            return false;
        }
        if self.recent.len() == self.recent.capacity() {
            self.recent[self.recent_at] = key;
            self.recent_at = (self.recent_at + 1) % self.recent.capacity();
        } else {
            self.recent.push(key);
        }
        true
    }
    pub fn mount_class(&self, name: &str) -> Option<MountClass> {
        self.mounts.class(name)
    }
    /// Samples from owners without a published vehicle row use the same mounting points and display clock.
    pub fn observe_mount(&mut self, sample: MountObservation) {
        self.mounts.observe(sample);
    }
    pub fn retire_mount(&mut self, owner: u64, generation: u32) {
        self.mounts.retire(owner, generation);
    }

    pub(super) fn frame(
        &mut self,
        receiver: &Receiver,
        tick: f64,
        bolts: &[Bolt],
        effects: &mut Spawner,
    ) {
        effects.begin_mount_frame();
        if self
            .epoch
            .replace(receiver.epoch)
            .is_some_and(|e| e != receiver.epoch)
        {
            self.launches.fill(None);
            self.waiting.clear();
            for (_, handle, _) in self.stopping.drain(..) {
                effects.stop(handle);
            }
            self.recent.clear();
            self.recent_at = 0;
            self.owners.clear();
            self.barrel_flashes.clear();
            if let Some(mounts) = effects.mount_player() { self.mounts.reset(mounts); }
            self.rounds.clear();
            self.blast_lights.clear();
            if let Some(dust) = &mut self.dust {
                dust.reset();
            }
        }
        self.blast_lights.read(receiver);
        // A salvo can publish several pellets, or retransmit its launch after the muzzle has gone.
        self.owners.retain(|(_, stamp, _)| *stamp >= tick - 60.);
        self.barrel_flashes.retain(|(_, flash)| tick < flash.ends());
        for (i, b) in bolts.iter().enumerate() {
            if !b.active || b.flight > tick || self.launches[i] == Some((b.id, b.generation)) {
                continue;
            }
            self.launches[i] = Some((b.id, b.generation));
            let owner = b.owner;
            let key = (owner, b.flight, b.definition);
            if self.owners.contains(&key) {
                continue;
            }
            if self.owners.len() == self.owners.capacity() {
                self.refused = self.refused.saturating_add(1);
                continue;
            }
            self.owners.push(key);
            let held = [kind::SOLDIER, kind::DROIDEKA, kind::TURRET]
                .into_iter()
                .find_map(|k| receiver.record(k, owner.0 as f64, owner.1 as f64))
                .and_then(|row| self.weapon_flash.get(row[r::WEAPON] as usize))
                .filter(|(_, ids)| ids.contains(&b.definition))
                .map(|(flash, _)| flash);
            let Some(flash) = held.or_else(|| self.round_flash.get(b.definition)) else {
                continue;
            };
            let end = b.flight + flash.muzzle.ticks;
            if end <= tick {
                continue;
            }
            if let Some(mesh) = rounds::MeshFlash::at_barrel(b, flash.muzzle) {
                if self.barrel_flashes.len() < self.barrel_flashes.capacity() {
                    self.barrel_flashes.push((owner, mesh));
                } else {
                    self.refused = self.refused.saturating_add(1);
                }
            }
            for &g in &flash.groups {
                if self.stopping.len() == self.stopping.capacity() {
                    self.refused = self.refused.saturating_add(1);
                    continue;
                }
                let at = std::array::from_fn(|j| b.position[j] + b.offset[j]);
                if let Some(handle) = effects.start(g, at, [0.; 3], Some(b.direction), b.id) {
                    self.stopping.push((None, handle, end));
                    self.bursts = self.bursts.saturating_add(1);
                } else {
                    self.refused = self.refused.saturating_add(1);
                }
            }
        }
        self.stopping.retain(|&(_, handle, release)| {
            if release <= tick {
                return !effects.try_stop(handle);
            }
            effects.playing(handle)
        });
        self.waiting.retain(|(event, release)| {
            if event.tick > tick {
                return true;
            }
            if release.is_some_and(|end| end <= tick) {
                return false;
            }
            if release.is_some() && self.stopping.len() == self.stopping.capacity() {
                self.refused = self.refused.saturating_add(1);
                return false;
            }
            if let Some(handle) = effects.start(
                event.group,
                event.position,
                [0.; 3],
                event.normal,
                event.serial as u32,
            ) {
                if let Some(end) = release {
                    self.stopping.push((Some(event.serial), handle, *end));
                }
                self.bursts = self.bursts.saturating_add(1);
            } else {
                self.refused = self.refused.saturating_add(1);
            }
            false
        });
        if let Some(mounts) = effects.mount_player() {
            self.mounts.frame(receiver, tick, mounts);
        }
        self.refused = self.refused.saturating_add(std::mem::take(&mut self.mounts.refused));
    }
}

#[cfg(test)]
mod missing_effect_tests {
    use super::*;
    #[test]
    fn resolve_group_records_missing_effect_once_and_skips_present_ones() {
        let handles = vec![("fx_present_glow".into(), 3)];
        let mut logged = std::collections::BTreeSet::new();
        assert_eq!(
            resolve_group(&handles, "FX_PRESENT_GLOW", "ordc/test_bolt", "TrailEffect", &mut logged),
            Some(3)
        );
        assert!(logged.is_empty());
        assert_eq!(
            resolve_group(&handles, "fx_missing_trail", "ordc/test_rocket", "TrailEffect", &mut logged),
            None
        );
        assert_eq!(logged.len(), 1);
        assert!(logged.contains("fx_missing_trail"));
        assert_eq!(
            resolve_group(&handles, "FX_MISSING_TRAIL", "ordc/test_rocket2", "TrailEffect", &mut logged),
            None
        );
        assert_eq!(logged.len(), 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generic_contact_fills_unassigned_surfaces_and_air_has_no_contact() {
        let v = bfe_content::json::parse(r#"{"ImpactEffect":"Mote","ImpactEffectRigid":"Pebble"}"#)
            .unwrap();
        let hits = hit_groups(&v, &[("mote".into(), 2), ("pebble".into(), 9)]);
        assert_eq!(hits, [Some(2), Some(2), Some(2), Some(9), Some(2), None]);
    }
    #[test]
    fn weapon_fire_effect_reads_muzzle_flash_effect_only() {
        let mut fields = BTreeMap::new();
        fields.insert("MuzzleFlash".to_owned(), vec![Some(1)]);
        fields.insert("Discharge".to_owned(), vec![Some(2)]);
        fields.insert("MuzzleFlashEffect".to_owned(), vec![Some(3), Some(3), Some(4)]);
        let mut groups = Groups::new();
        groups.insert("wpnc/lance".to_owned(), fields);
        assert_eq!(flash(&groups, "wpnc/lance"), vec![3, 4]);
    }
}
