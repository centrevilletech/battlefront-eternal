//! Battlefront II power visual effects: explosions, lightning bolts, bone smolders and blade
//! landing blasts. They are drawn from the battle's published power events, which an Instant
//! Action client and a server's clients receive alike.
use super::{blast_lights::Light, group, Receiver};
use crate::{
    characters::Characters,
    online::{
        effects::Spawner,
        lightning::{Arcs, Casts},
    },
};
use bfe_content::{zero_hash, Value};
use bfe_content_native::BattleContent;
use bfe_presentation::network_layout::{net_kind as kind, net_record as r};
use bfe_sim::combat::{
    combo::zero_fnv1a_lower,
    powers::{world::LOG_LEN, PowerEventKind},
};
use serde::Deserialize;

/// A power event as every viewer receives it: what happened, the weapon's class
/// hash, the caster and target as (slot, life), and where the caster stood.
#[derive(Clone, Copy, Debug, PartialEq)]
struct NetPower {
    /// The publication's id, unique within the battle.
    sequence: u32,
    tick: f64,
    caster: (u32, u32),
    target: Option<(u32, u32)>,
    weapon: u32,
    kind: PowerEventKind,
    position: Option<[f64; 3]>,
}

#[derive(Deserialize)]
struct WirePower<'a> {
    kind: &'a str,
    action: &'a str,
    /// A burn's effect and the seconds it has left.
    #[serde(default)]
    name: Option<&'a str>,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(rename = "weaponId")]
    weapon: f64,
    #[serde(rename = "ownerId")]
    owner_id: f64,
    #[serde(rename = "ownerGeneration")]
    owner_generation: f64,
    #[serde(default, rename = "targetId")]
    target_id: Option<f64>,
    #[serde(default, rename = "targetGeneration")]
    target_generation: Option<f64>,
    #[serde(default)]
    position: Option<[f64; 3]>,
}

/// A combat event's payload as a power event, when it is one.
fn parse_power(bytes: &[u8], sequence: u32, tick: f64) -> Option<NetPower> {
    let w: WirePower = serde_json::from_slice(bytes).ok()?;
    if w.kind != "power" {
        return None;
    }
    let kind = match w.action {
        "cast" => PowerEventKind::Cast,
        "hit" => PowerEventKind::Hit,
        "thrown" => PowerEventKind::Thrown,
        "blocked" => PowerEventKind::Blocked,
        _ => return None,
    };
    let target = match (w.target_id, w.target_generation) {
        (Some(id), Some(life)) => Some((id as u32, life as u32)),
        _ => None,
    };
    Some(NetPower {
        sequence,
        tick,
        caster: (w.owner_id as u32, w.owner_generation as u32),
        target,
        weapon: w.weapon as u32,
        kind,
        position: w.position.filter(|p| p.iter().all(|v| v.is_finite())),
    })
}

const MAX_SMOLDER_SLOTS: usize = 5;
const MAX_ACTIVE_SMOLDERS: usize = 128;
const MAX_PENDING_BOLTS: usize = 64;
const BLOCKED_PULLBACK: f64 = 0.75;
/// How far above the wielder's feet a landing blast's effect starts: the gap
/// the game keeps between a blast and the surface it sits on, so a flat
/// effect (a crack on the ground) is not buried in the floor.
const BLAST_LIFT: f64 = 0.2;

#[derive(Clone, Debug)]
struct PowerVisual {
    weapon: u32,
    explosion: [Option<usize>; 2],
    cast_effect: Option<usize>,
    lightning: Option<String>,
    lightning_scale: f64,
    cone_length: f64,
    no_target_count: u32,
    no_target_spread: f64,
    smolder_timer: f64,
    smolders: Vec<(u32, usize)>,
}

/// A blade whose combo sets off an explosion where its wielder stands
/// (`PlayExplosion` with the blade's `ExplosionName`): the explosion's dry and
/// wet effect groups.
#[derive(Clone, Copy, Debug, PartialEq)]
struct BladeBlast {
    weapon: u32,
    groups: [Option<usize>; 2],
    /// The explosion class's light, when it names one.
    light: Option<Light>,
}

/// The effect groups of the explosion class `explosion` as the mission's pack
/// binds them: `Effect`, then `WaterEffect` falling back to it.
fn blast_groups(explosion: &str, pack: &Value, names: &[(String, usize)]) -> [Option<usize>; 2] {
    let binding = pack
        .get("bindings")
        .and_then(|b| b.get(&format!("expc/{explosion}")));
    let named = |field: &str| {
        binding?
            .get(field)?
            .at(0)?
            .as_str()
            .and_then(|n| group(names, n))
    };
    let dry = named("Effect");
    [dry, named("WaterEffect").or(dry)]
}

/// A melee weapon record's landing blast, when it names one with an effect.
fn blade_blast(weapon: &Value, pack: &Value, names: &[(String, usize)]) -> Option<BladeBlast> {
    if !prop_str(weapon, "base").is_some_and(|b| b.eq_ignore_ascii_case("melee")) {
        return None;
    }
    let class = prop_str(weapon, "class").or_else(|| prop_str(weapon, "weapon"))?;
    let explosion = prop_str(weapon, "ExplosionName")?;
    let groups = blast_groups(explosion, pack, names);
    let weapon = zero_fnv1a_lower(class);
    (weapon != 0 && groups.iter().any(Option::is_some)).then_some(BladeBlast { weapon, groups, light: None })
}

#[derive(Clone, Copy, Debug)]
struct PendingBolt {
    visual: usize,
    shooter: (u32, u32),
    from: [f64; 3],
    to: [f64; 3],
    charge: f64,
}

#[derive(Clone, Copy, Debug)]
struct ActiveSmolder {
    target: (u32, u32),
    bone_crc: u32,
    group: usize,
    handle: u32,
    remaining: f64,
    /// Gone with its body's death (`SmolderVanishDeath`, on unless a weapon turns it off).
    vanish: bool,
}

/// An emitter weapon's lasting effect on what it hits: its `SmolderEffect` and
/// the `SmolderBone` each copy of it follows.
#[derive(Clone, Debug, PartialEq)]
struct BurnSet {
    effect: String,
    pairs: Vec<(u32, usize)>,
    vanish: bool,
}

pub(crate) struct PowerEffects {
    visuals: Vec<PowerVisual>,
    blasts: Vec<BladeBlast>,
    class_cones: Vec<(usize, f64)>,
    water_height: Option<f64>,
    epoch: f64,
    rng: jsrt::rng::ClassicRandom,
    smolders: Vec<ActiveSmolder>,
    pending_bolts: Vec<PendingBolt>,
    scratch_events: Vec<NetPower>,
    /// Blast lights to flash this frame: where, and the explosion's light.
    lights: Vec<([f64; 3], Light)>,
    burn_sets: Vec<BurnSet>,
    /// Burns published this frame: the burning life, its set, the seconds left, the event's id.
    burns_due: Vec<((u32, u32), usize, f64, u32)>,
    pub bursts: u32,
    pub bolts: u32,
    pub smolder_starts: u32,
}

fn prop_str<'a>(record: &'a Value, key: &str) -> Option<&'a str> {
    record
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            record
                .get("properties")
                .and_then(Value::as_array)?
                .iter()
                .rev()
                .filter_map(Value::as_array)
                .find_map(|pair| {
                    (pair.first()?.as_str()?.eq_ignore_ascii_case(key))
                        .then(|| pair.get(1)?.as_str())
                        .flatten()
                        .filter(|s| !s.is_empty())
                })
        })
}

fn prop_f64(record: &Value, key: &str) -> Option<f64> {
    record
        .get(key)
        .and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse().ok()))
        .or_else(|| prop_str(record, key)?.parse().ok())
        .filter(|n| n.is_finite())
}

fn prop_u32(record: &Value, key: &str) -> Option<u32> {
    prop_f64(record, key)
        .filter(|&n| n >= 0.)
        .map(|n| n.round() as u32)
}

fn smolder_pairs(ordnance: &Value, names: &[(String, usize)]) -> Vec<(u32, usize)> {
    let mut effects: [Option<usize>; MAX_SMOLDER_SLOTS] = [None; MAX_SMOLDER_SLOTS];
    let mut bones: [u32; MAX_SMOLDER_SLOTS] = [0; MAX_SMOLDER_SLOTS];
    let (mut e_count, mut b_count) = (0usize, 0usize);
    if let Some(props) = ordnance.get("properties").and_then(Value::as_array) {
        for pair in props.iter().filter_map(Value::as_array) {
            let (Some(k), Some(v)) = (
                pair.first().and_then(Value::as_str),
                pair.get(1).and_then(Value::as_str),
            ) else {
                continue;
            };
            if v.is_empty() {
                continue;
            }
            if k.eq_ignore_ascii_case("SmolderEffect") && e_count < MAX_SMOLDER_SLOTS {
                effects[e_count] = group(names, v);
                e_count += 1;
            } else if k.eq_ignore_ascii_case("SmolderBone") && b_count < MAX_SMOLDER_SLOTS {
                bones[b_count] = zero_hash::crc(v);
                b_count += 1;
            }
        }
    }
    if e_count == 0 {
        if let Some(name) = ordnance.get("SmolderEffect").and_then(Value::as_str) {
            effects[0] = group(names, name);
            e_count = 1;
        }
        if let Some(bone) = ordnance.get("SmolderBone").and_then(Value::as_str) {
            bones[0] = zero_hash::crc(bone);
        }
    }
    let fallback_bone = zero_hash::crc("bone_ribcage");
    let mut out = Vec::new();
    for i in 0..e_count {
        if let Some(g) = effects[i] {
            let bone = if bones[i] != 0 { bones[i] } else { fallback_bone };
            out.push((bone, g));
        }
    }
    out
}

fn explosion_groups(
    ordnance: &Value,
    pack: &Value,
    names: &[(String, usize)],
) -> [Option<usize>; 2] {
    let mut dry = None;
    let mut wet = None;
    if let Some(expl) = ordnance.get("Explosion").filter(|v| v.as_object().is_some()) {
        dry = prop_str(expl, "Effect").and_then(|n| group(names, n));
        wet = prop_str(expl, "WaterEffect").and_then(|n| group(names, n));
    }
    if dry.is_none() || wet.is_none() {
        let ord_name = ordnance
            .get("class")
            .and_then(Value::as_str)
            .unwrap_or("");
        let exp_owner = ordnance
            .get("Explosion")
            .and_then(|v| v.get("class").or(Some(v)))
            .and_then(Value::as_str)
            .map(|c| {
                if c.starts_with("expc/") {
                    c.to_owned()
                } else {
                    format!("expc/{c}")
                }
            })
            .or_else(|| {
                let key = format!("ordc/{ord_name}");
                pack.get("links")?
                    .get(&key)?
                    .get("Explosion")?
                    .at(0)?
                    .as_str()
                    .map(str::to_owned)
            });
        if let Some(owner) = exp_owner {
            if let Some(bind) = pack.get("bindings").and_then(|b| b.get(&owner)) {
                if dry.is_none() {
                    dry = bind
                        .get("Effect")
                        .and_then(|v| v.at(0))
                        .and_then(Value::as_str)
                        .and_then(|n| group(names, n));
                }
                if wet.is_none() {
                    wet = bind
                        .get("WaterEffect")
                        .and_then(|v| v.at(0))
                        .and_then(Value::as_str)
                        .and_then(|n| group(names, n));
                }
            }
        }
    }
    [dry, wet.or(dry)]
}

fn build_visual(
    weapon_class: &str,
    ordnance: &Value,
    pack: &Value,
    names: &[(String, usize)],
) -> Option<PowerVisual> {
    if !ordnance
        .get("base")
        .and_then(Value::as_str)
        .is_some_and(|b| b.eq_ignore_ascii_case("emitterordnance"))
    {
        return None;
    }
    let weapon = zero_fnv1a_lower(weapon_class);
    if weapon == 0 {
        return None;
    }
    let explosion = explosion_groups(ordnance, pack, names);
    let cast_effect = prop_str(ordnance, "Effect").and_then(|n| group(names, n));
    let lightning = prop_str(ordnance, "LightningEffect").map(str::to_owned);
    let lightning_scale = prop_f64(ordnance, "LightningEffectScale").unwrap_or(1.0);
    let cone_length = prop_f64(ordnance, "ConeLength").unwrap_or(10.0);
    let no_target_count = prop_u32(ordnance, "NoTargetLightningEffectCount").unwrap_or(1);
    let no_target_spread = prop_f64(ordnance, "NoTargetLightningEffectRandomSpread").unwrap_or(0.0);
    let smolder_timer = prop_f64(ordnance, "SmolderEffectTimer").unwrap_or(0.0);
    let smolders = smolder_pairs(ordnance, names);
    if explosion[0].is_none()
        && explosion[1].is_none()
        && cast_effect.is_none()
        && lightning.is_none()
        && smolders.is_empty()
    {
        return None;
    }
    Some(PowerVisual {
        weapon,
        explosion,
        cast_effect,
        lightning,
        lightning_scale,
        cone_length,
        no_target_count,
        no_target_spread,
        smolder_timer,
        smolders,
    })
}

fn aim_direction(yaw: f64, pitch: f64) -> [f64; 3] {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    [sy * cp, sp, cy * cp]
}


fn record_feet(receiver: &Receiver, id: (u32, u32)) -> Option<[f64; 3]> {
    let row = receiver.record(kind::SOLDIER, id.0 as f64, id.1 as f64)?;
    Some([row[r::POSITION], row[r::POSITION + 1], row[r::POSITION + 2]])
}

fn record_aim(receiver: &Receiver, id: (u32, u32)) -> Option<(f64, f64)> {
    let row = receiver.record(kind::SOLDIER, id.0 as f64, id.1 as f64)?;
    Some((row[r::YAW], row[r::PITCH]))
}

trait PowerBodies {
    fn is_alive(&self, id: (u32, u32)) -> bool;
    fn feet_world(&self, id: (u32, u32)) -> Option<[f64; 3]>;
    fn aim_angles(&self, id: (u32, u32)) -> Option<(f64, f64)>;
    fn hand_world(&mut self, id: (u32, u32)) -> Option<[f64; 3]>;
    fn chest_world(&mut self, id: (u32, u32)) -> Option<[f64; 3]>;
    fn bone_world(&mut self, id: (u32, u32), bone_crc: u32) -> Option<[f64; 3]>;
}

impl PowerBodies for Characters {
    fn is_alive(&self, id: (u32, u32)) -> bool {
        Characters::is_alive(self, id)
    }
    fn feet_world(&self, id: (u32, u32)) -> Option<[f64; 3]> {
        Characters::feet_world(self, id)
    }
    fn aim_angles(&self, id: (u32, u32)) -> Option<(f64, f64)> {
        Characters::aim_angles(self, id)
    }
    fn hand_world(&mut self, id: (u32, u32)) -> Option<[f64; 3]> {
        Characters::hand_world(self, id)
    }
    fn chest_world(&mut self, id: (u32, u32)) -> Option<[f64; 3]> {
        Characters::chest_world(self, id)
    }
    fn bone_world(&mut self, id: (u32, u32), bone_crc: u32) -> Option<[f64; 3]> {
        Characters::bone_world(self, id, bone_crc)
    }
}

impl PowerEffects {
    pub fn new(battle: &BattleContent, names: &[(String, usize)]) -> Result<Self, String> {
        let defs = battle.defs()?;
        let pack = battle.visual_effects();
        let mut visuals: Vec<PowerVisual> = Vec::new();
        let mut blasts: Vec<BladeBlast> = Vec::new();
        let mut class_cones: Vec<(usize, f64)> = Vec::new();
        let mut blast_classes: Vec<(u32, String)> = Vec::new();
        let mut burn_sets: Vec<BurnSet> = Vec::new();
        for weapon in &defs.weapons {
            if let Some(blast) = blade_blast(&weapon.record, pack, names) {
                if !blasts.iter().any(|b| b.weapon == blast.weapon) {
                    if let Some(explosion) = prop_str(&weapon.record, "ExplosionName") {
                        blast_classes.push((blast.weapon, explosion.to_owned()));
                    }
                    blasts.push(blast);
                }
            }
            let Some(class_name) = weapon
                .record
                .get("class")
                .or_else(|| weapon.record.get("weapon"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            let Some(ordnance) = weapon.record.get("ordnance") else {
                continue;
            };
            let Some(visual) = build_visual(class_name, ordnance, pack, names) else {
                continue;
            };
            if let (Some(_), Some(class_id)) = (
                &visual.lightning,
                defs.classes
                    .get(weapon.owner)
                    .and_then(|c| c.registry_id)
                    .map(usize::from),
            ) {
                if !class_cones.iter().any(|(id, _)| *id == class_id) {
                    class_cones.push((class_id, visual.cone_length));
                }
            }
            if let Some(effect) = prop_str(ordnance, "SmolderEffect").filter(|_| !visual.smolders.is_empty()) {
                if !burn_sets.iter().any(|b: &BurnSet| b.effect.eq_ignore_ascii_case(effect)) {
                    let vanish = prop_f64(ordnance, "SmolderVanishDeath").is_none_or(|v| v != 0.);
                    burn_sets.push(BurnSet { effect: effect.to_owned(), pairs: visual.smolders.clone(), vanish });
                }
            }
            if !visuals.iter().any(|v| v.weapon == visual.weapon) {
                visuals.push(visual);
            }
        }
        let water_height = battle
            .content
            .battle
            .world
            .get("water")
            .and_then(|w| w.get("height"))
            .and_then(Value::as_f64)
            .filter(|h| h.is_finite());
        // A blade's explosion is no combat record of its own: its light comes
        // from the class files the battle reads it from.
        let wanted: Vec<&str> = blast_classes.iter().map(|(_, n)| n.as_str()).collect();
        let found = super::blast_lights::class_lights(battle, &wanted);
        for blast in &mut blasts {
            let name = blast_classes.iter().find(|(w, _)| *w == blast.weapon).map(|(_, n)| n.as_str());
            blast.light = name.and_then(|n| found.iter().find(|(f, _)| f.eq_ignore_ascii_case(n)).map(|(_, l)| *l));
        }
        Ok(Self {
            visuals,
            blasts,
            class_cones,
            water_height,
            epoch: f64::NAN,
            rng: jsrt::rng::ClassicRandom::new(17.0),
            smolders: Vec::with_capacity(MAX_ACTIVE_SMOLDERS),
            pending_bolts: Vec::with_capacity(MAX_PENDING_BOLTS),
            scratch_events: Vec::with_capacity(LOG_LEN),
            lights: Vec::with_capacity(8),
            burn_sets,
            burns_due: Vec::with_capacity(LOG_LEN),
            bursts: 0,
            bolts: 0,
            smolder_starts: 0,
        })
    }

    pub fn class_lightning_cone(&self, class_registry: usize) -> Option<f64> {
        self.class_cones
            .iter()
            .find(|(id, _)| *id == class_registry)
            .map(|(_, cone)| *cone)
    }

    fn reset(&mut self, effects: &mut Spawner) {
        for active in self.smolders.drain(..) {
            effects.stop(active.handle);
        }
        self.pending_bolts.clear();
        self.lights.clear();
        self.burns_due.clear();
    }

    /// This frame's published power events (the journal already drops a
    /// repeated publication).
    fn read(&mut self, receiver: &Receiver) {
        self.scratch_events.clear();
        for field in receiver.publications.iter() {
            let row = &field.row;
            if row[r::KIND] != kind::COMBAT as f64 || row[r::SCHEMA] as u32 != 8 {
                continue;
            }
            if self.scratch_events.len() == self.scratch_events.capacity() {
                break;
            }
            let bytes = &field.bytes[..field.length];
            if let Some(burn) = self.parse_burn(bytes, row[r::ID] as u32) {
                if self.burns_due.len() < self.burns_due.capacity() {
                    self.burns_due.push(burn);
                }
            } else if let Some(event) = parse_power(bytes, row[r::ID] as u32, row[r::TICK]) {
                self.scratch_events.push(event);
            }
        }
    }

    /// A published burn: the burning life, the weapon's burn set its effect names,
    /// the seconds it has left, the event's id.
    fn parse_burn(&self, bytes: &[u8], sequence: u32) -> Option<((u32, u32), usize, f64, u32)> {
        let w: WirePower = serde_json::from_slice(bytes).ok()?;
        if w.kind != "power" || w.action != "burn" {
            return None;
        }
        let set = self.burn_sets.iter().position(|b| w.name.is_some_and(|n| b.effect.eq_ignore_ascii_case(n)))?;
        let target = (w.target_id? as u32, w.target_generation? as u32);
        let seconds = w.duration.filter(|d| d.is_finite() && *d > 0.)?;
        Some((target, set, seconds, sequence))
    }

    /// The blast lights asked for since the last call.
    pub(super) fn take_lights(&mut self, mut flash: impl FnMut([f64; 3], Light)) {
        for (position, light) in self.lights.drain(..) {
            flash(position, light);
        }
    }

    fn visual_index(&self, weapon: u32) -> Option<usize> {
        self.visuals.iter().position(|v| v.weapon == weapon)
    }

    fn start_or_refresh_smolder(
        &mut self,
        target: (u32, u32),
        bone_crc: u32,
        group: usize,
        duration: f64,
        position: [f64; 3],
        seed: u32,
        vanish: bool,
        effects: &mut Spawner,
    ) {
        if let Some(existing) = self
            .smolders
            .iter_mut()
            .find(|s| s.target == target && s.bone_crc == bone_crc && s.group == group)
        {
            existing.remaining = existing.remaining.max(duration);
            return;
        }
        if self.smolders.len() >= MAX_ACTIVE_SMOLDERS {
            return;
        }
        if let Some(handle) = effects.start(group, position, [0.; 3], None, seed) {
            self.smolders.push(ActiveSmolder {
                target,
                bone_crc,
                group,
                handle,
                remaining: duration,
                vanish,
            });
            self.smolder_starts = self.smolder_starts.saturating_add(1);
        }
    }

    fn push_bolt(&mut self, bolt: PendingBolt) {
        if self.pending_bolts.len() < MAX_PENDING_BOLTS {
            self.pending_bolts.push(bolt);
            self.bolts = self.bolts.saturating_add(1);
        }
    }

    /// Consume newly observed power events, emit immediate bursts and bone smolders into
    /// `effects`, and queue any lightning arcs for the arc pass of this same frame.
    pub fn frame(
        &mut self,
        receiver: &Receiver,
        characters: &mut Characters,
        dt: f64,
        effects: &mut Spawner,
        ray_cast: &mut dyn FnMut([f64; 3], [f64; 3], f64) -> Option<f64>,
    ) {
        self.frame_with_bodies(receiver, characters, dt, effects, ray_cast);
    }

    fn frame_with_bodies(
        &mut self,
        receiver: &Receiver,
        characters: &mut dyn PowerBodies,
        dt: f64,
        effects: &mut Spawner,
        ray_cast: &mut dyn FnMut([f64; 3], [f64; 3], f64) -> Option<f64>,
    ) {
        if self.epoch != receiver.epoch {
            self.epoch = receiver.epoch;
            self.reset(effects);
        }
        self.read(receiver);
        self.dispatch_scratch(receiver, characters, effects, ray_cast);
        // A burn's effect on every bone its weapon names, for the seconds it has left.
        for i in 0..self.burns_due.len() {
            let (target, set, seconds, sequence) = self.burns_due[i];
            for p in 0..self.burn_sets[set].pairs.len() {
                let (bone_crc, group) = self.burn_sets[set].pairs[p];
                let vanish = self.burn_sets[set].vanish;
                let Some(at) = characters.bone_world(target, bone_crc).or_else(|| characters.chest_world(target)) else { continue };
                self.start_or_refresh_smolder(target, bone_crc, group, seconds, at, sequence.wrapping_add(p as u32), vanish, effects);
            }
        }
        self.burns_due.clear();
        // Sync active smolders to each victim's live posed bones and stop expired or dead ones.
        let mut i = 0;
        while i < self.smolders.len() {
            let entry = &mut self.smolders[i];
            entry.remaining -= dt.max(0.);
            let alive = entry.remaining > 0.
                && effects.playing(entry.handle)
                && (!entry.vanish || characters.is_alive(entry.target));
            if !alive {
                effects.stop(entry.handle);
                self.smolders.swap_remove(i);
                continue;
            }
            if let Some(pos) = characters
                .bone_world(entry.target, entry.bone_crc)
                .or_else(|| characters.chest_world(entry.target))
            {
                effects.sync(entry.handle, pos, None, None);
            } else if !entry.vanish {
                // The body it burns on is gone.
                effects.stop(entry.handle);
                self.smolders.swap_remove(i);
                continue;
            }
            i += 1;
        }
    }

    fn dispatch_scratch(
        &mut self,
        receiver: &Receiver,
        characters: &mut dyn PowerBodies,
        effects: &mut Spawner,
        ray_cast: &mut dyn FnMut([f64; 3], [f64; 3], f64) -> Option<f64>,
    ) {
        let count = self.scratch_events.len();
        let mut visited = [false; LOG_LEN];
        for i in 0..count {
            if visited[i] {
                continue;
            }
            let head = self.scratch_events[i];
            // Aura effects already exist and are intentionally left untouched.
            if head.kind == PowerEventKind::Buffed || head.kind == PowerEventKind::Held {
                visited[i] = true;
                continue;
            }
            // A blade's landing blast shows its explosion once, upright, 0.2 m
            // above where its wielder stood when the battle set it off (the
            // published position), and flashes its light; the bodies it
            // reaches show nothing.
            if let Some(blast) = self.blasts.iter().find(|b| b.weapon == head.weapon).copied() {
                visited[i] = true;
                if head.kind == PowerEventKind::Cast {
                    let caster = head.caster;
                    if let Some(feet) = head
                        .position
                        .or_else(|| characters.feet_world(caster))
                        .or_else(|| record_feet(receiver, caster))
                    {
                        let at = [feet[0], feet[1] + BLAST_LIFT, feet[2]];
                        let in_water = self.water_height.is_some_and(|h| feet[1] < h);
                        if let Some(group) = blast.groups[usize::from(in_water)] {
                            if effects.start(group, at, [0.; 3], None, head.sequence).is_some() {
                                self.bursts = self.bursts.saturating_add(1);
                            }
                        }
                        if let Some(light) = blast.light.filter(|_| self.lights.len() < self.lights.capacity()) {
                            self.lights.push((at, light));
                        }
                    }
                }
                continue;
            }
            let Some(v_idx) = self.visual_index(head.weapon) else {
                visited[i] = true;
                continue;
            };
            let caster = head.caster;
            let has_lightning = self.visuals[v_idx].lightning.is_some();
            let explosion = self.visuals[v_idx].explosion;
            let cast_effect = self.visuals[v_idx].cast_effect;
            let lightning_scale = self.visuals[v_idx].lightning_scale;
            let cone_length = self.visuals[v_idx].cone_length;
            let no_target_count = self.visuals[v_idx].no_target_count;
            let no_target_spread = self.visuals[v_idx].no_target_spread;
            let smolder_timer = self.visuals[v_idx].smolder_timer;
            let caster_hand = characters.hand_world(caster).or_else(|| {
                let mut p = record_feet(receiver, caster)?;
                p[1] += 1.35;
                Some(p)
            });
            let caster_aim = characters
                .aim_angles(caster)
                .or_else(|| record_aim(receiver, caster));

            let mut saw_cast = false;
            let mut struck = 0u32;
            let mut cast_seq = head.sequence;
            for j in i..count {
                if visited[j] {
                    continue;
                }
                let ev = self.scratch_events[j];
                if ev.tick != head.tick
                    || ev.weapon != head.weapon
                    || ev.caster != caster
                {
                    continue;
                }
                visited[j] = true;
                match ev.kind {
                    PowerEventKind::Cast => {
                        saw_cast = true;
                        cast_seq = ev.sequence;
                    }
                    PowerEventKind::Hit | PowerEventKind::Thrown => {
                        let Some(target_key) = ev.target else {
                            continue;
                        };
                        let target = target_key;
                        let feet = characters
                            .feet_world(target)
                            .or_else(|| record_feet(receiver, target));
                        let chest = characters
                            .chest_world(target)
                            .or_else(|| feet.map(|p| [p[0], p[1] + 1.0, p[2]]));
                        let Some(target_pos) = (if has_lightning { chest } else { feet }) else {
                            continue;
                        };
                        struck = struck.saturating_add(1);
                        let in_water = self.water_height.is_some_and(|h| target_pos[1] < h);
                        if let Some(group) = explosion[usize::from(in_water)] {
                            if effects
                                .start(group, target_pos, [0.; 3], None, ev.sequence)
                                .is_some()
                            {
                                self.bursts = self.bursts.saturating_add(1);
                            }
                        }
                        if has_lightning {
                            if let (Some(from), Some(to)) = (caster_hand, chest) {
                                self.push_bolt(PendingBolt {
                                    visual: v_idx,
                                    shooter: caster,
                                    from,
                                    to,
                                    charge: lightning_scale,
                                });
                            }
                        }
                        if smolder_timer > 0. {
                            let n = self.visuals[v_idx].smolders.len();
                            for s_i in 0..n {
                                let (bone_crc, group) = self.visuals[v_idx].smolders[s_i];
                                let bone_pos = characters
                                    .bone_world(target, bone_crc)
                                    .or(chest)
                                    .unwrap_or(target_pos);
                                self.start_or_refresh_smolder(
                                    target,
                                    bone_crc,
                                    group,
                                    smolder_timer,
                                    bone_pos,
                                    ev.sequence.wrapping_add(s_i as u32),
                                    true,
                                    effects,
                                );
                            }
                        }
                    }
                    PowerEventKind::Blocked => {
                        let Some(target_key) = ev.target else {
                            continue;
                        };
                        let target = target_key;
                        let chest = characters.chest_world(target).or_else(|| {
                            let mut p = record_feet(receiver, target)?;
                            p[1] += 1.0;
                            Some(p)
                        });
                        struck = struck.saturating_add(1);
                        if has_lightning {
                            if let (Some(from), Some(chest)) = (caster_hand, chest) {
                                let d = [
                                    chest[0] - from[0],
                                    chest[1] - from[1],
                                    chest[2] - from[2],
                                ];
                                let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                                let to = if len > 1e-4 {
                                    let keep = (len - BLOCKED_PULLBACK).max(len * 0.25);
                                    let s = keep / len;
                                    [
                                        from[0] + d[0] * s,
                                        from[1] + d[1] * s,
                                        from[2] + d[2] * s,
                                    ]
                                } else {
                                    chest
                                };
                                self.push_bolt(PendingBolt {
                                    visual: v_idx,
                                    shooter: caster,
                                    from,
                                    to,
                                    charge: lightning_scale,
                                });
                            }
                        }
                    }
                    PowerEventKind::Held | PowerEventKind::Buffed => {}
                }
            }
            if saw_cast {
                if let (Some(group), Some(from)) = (cast_effect, caster_hand) {
                    let normal = caster_aim.map(|(y, p)| aim_direction(y, p));
                    if effects.start(group, from, [0.; 3], normal, cast_seq).is_some() {
                        self.bursts = self.bursts.saturating_add(1);
                    }
                }
                if has_lightning && struck < no_target_count {
                    if let (Some(from), Some((yaw, pitch))) = (caster_hand, caster_aim) {
                        let extra = no_target_count - struck;
                        for _ in 0..extra {
                            let dir = if no_target_spread > 0. {
                                let dyaw = (1. - 2. * self.rng.uniform()) * no_target_spread;
                                let dpitch = (1. - 2. * self.rng.uniform()) * no_target_spread;
                                aim_direction(yaw + dyaw, (pitch + dpitch).clamp(-1.45, 1.45))
                            } else {
                                aim_direction(yaw, pitch)
                            };
                            let dist = ray_cast(from, dir, cone_length).unwrap_or(cone_length);
                            let to = [
                                from[0] + dir[0] * dist,
                                from[1] + dir[1] * dist,
                                from[2] + dir[2] * dist,
                            ];
                            self.push_bolt(PendingBolt {
                                visual: v_idx,
                                shooter: caster,
                                from,
                                to,
                                charge: lightning_scale,
                            });
                        }
                    }
                }
            }
        }
    }

    pub fn drain_bolts(&mut self, arcs: &mut Arcs, casts: &mut Casts) {
        for bolt in self.pending_bolts.drain(..) {
            if let Some(name) = self
                .visuals
                .get(bolt.visual)
                .and_then(|v| v.lightning.as_deref())
            {
                arcs.spawn_power_bolt(
                    name,
                    Some(bolt.shooter),
                    bolt.from,
                    bolt.to,
                    bolt.charge,
                    casts,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_push_pull_and_lightning_visuals_preserving_multiple_smolder_bones() {
        let names = vec![
            ("ring_out".to_string(), 1),
            ("spark_burst".to_string(), 2),
            ("splash_burst".to_string(), 3),
            ("crackle_loop".to_string(), 4),
            ("ember_loop".to_string(), 5),
        ];
        let push_ord = bfe_content::json::parse(
            r#"{
                "class": "gust_shove_ord",
                "base": "emitterordnance",
                "Explosion": {
                    "class": "gust_shove_exp",
                    "Effect": "ring_out"
                },
                "ConeLength": "9.5"
            }"#,
        )
        .unwrap();
        let push = build_visual("gust_shove", &push_ord, &Value::Null, &names).unwrap();
        assert_eq!(push.explosion, [Some(1), Some(1)]);
        assert_eq!(push.cone_length, 9.5);
        assert!(push.lightning.is_none());
        assert!(push.smolders.is_empty());

        let arc_ord = bfe_content::json::parse(
            r#"{
                "class": "coil_discharge_ord",
                "base": "emitterordnance",
                "properties": [
                    ["Explosion", "coil_discharge_exp"],
                    ["ConeLength", "23.5"],
                    ["LightningEffect", "coil_bolt"],
                    ["LightningEffectScale", "0.62"],
                    ["NoTargetLightningEffectCount", "4"],
                    ["NoTargetLightningEffectRandomSpread", "0.22"],
                    ["SmolderEffectTimer", "4.75"],
                    ["SmolderEffect", "crackle_loop"],
                    ["SmolderBone", "joint_alpha"],
                    ["SmolderEffect", "ember_loop"],
                    ["SmolderBone", "joint_beta"],
                    ["SmolderEffect", "crackle_loop"],
                    ["SmolderBone", "joint_crest"]
                ]
            }"#,
        )
        .unwrap();
        let pack = bfe_content::json::parse(
            r#"{
                "links": {"ordc/coil_discharge_ord": {"Explosion": ["expc/coil_discharge_exp"]}},
                "bindings": {"expc/coil_discharge_exp": {"Effect": ["spark_burst"], "WaterEffect": ["splash_burst"]}}
            }"#,
        )
        .unwrap();
        let arc = build_visual("coil_discharge", &arc_ord, &pack, &names).unwrap();
        assert_eq!(arc.explosion, [Some(2), Some(3)]);
        assert_eq!(arc.lightning.as_deref(), Some("coil_bolt"));
        assert!((arc.lightning_scale - 0.62).abs() < 1e-9);
        assert_eq!(arc.cone_length, 23.5);
        assert_eq!(arc.no_target_count, 4);
        assert!((arc.no_target_spread - 0.22).abs() < 1e-9);
        assert_eq!(arc.smolder_timer, 4.75);
        assert_eq!(
            arc.smolders,
            vec![
                (zero_hash::crc("joint_alpha"), 4),
                (zero_hash::crc("joint_beta"), 5),
                (zero_hash::crc("joint_crest"), 4),
            ]
        );
    }

    fn test_spawner(group_count: usize) -> Spawner {
        use bfe_presentation::{Limits, Presentation};
        let color = serde_json::json!({"channels": [[255, 255], [255, 255], [255, 255], [255, 255]]});
        let def = serde_json::json!({
            "startDelay": [0, 0],
            "burstDelay": [0.1, 0.1],
            "burstCount": [1, 1],
            "maxParticles": [-1, -1],
            "size": [1, 1],
            "color": color,
            "registerStep": true,
            "spawner": {
                "shape": "box",
                "box": [[0, 0], [0, 0], [0, 0]],
                "positionScale": [0, 0],
                "velocityScale": [0, 0],
                "offset": [[0, 0], [0, 0], [0, 0]],
                "inheritVelocity": [0, 0],
                "size": [1, 1],
                "color": color,
                "fadeInTime": 0,
                "startRotation": [0, 0],
                "rotationVelocity": [0, 0]
            },
            "transformer": {"lifetime": 0.5},
            "geometry": {"type": "particle", "blend": "additive"},
            "child": 0,
            "gpu": false
        });
        let mut groups = vec![serde_json::json!([])];
        for _ in 0..group_count {
            groups.push(serde_json::json!([0]));
        }
        let descriptor = serde_json::json!({"definitions": [def], "groups": groups});
        let content: bfe_presentation::Content = serde_json::from_value(descriptor).unwrap();
        Spawner::new(Presentation::new(
            content,
            Limits {
                effects: 64,
                emitters: 64,
                particles: 256,
                commands: 256,
                events: 256,
            },
        ))
    }

    #[test]
    fn lightning_cast_queues_target_blocked_and_no_target_bolts_and_smolder_lifecycle() {
        let mut effects = test_spawner(3);
        let arm_l = zero_hash::crc("joint_alpha");
        let arm_r = zero_hash::crc("joint_beta");
        let head = zero_hash::crc("joint_crest");
        let weapon = zero_fnv1a_lower("coil_discharge");
        let mut fx = PowerEffects {
            visuals: vec![PowerVisual {
                weapon,
                explosion: [Some(1), Some(1)],
                cast_effect: None,
                lightning: Some("coil_bolt".into()),
                lightning_scale: 0.62,
                cone_length: 23.5,
                no_target_count: 4,
                no_target_spread: 0.22,
                smolder_timer: 4.75,
                smolders: vec![(arm_l, 2), (arm_r, 3), (head, 2)],
            }],
            blasts: vec![],
            class_cones: vec![],
            water_height: None,
            lights: Vec::with_capacity(8),
            burn_sets: vec![],
            burns_due: Vec::with_capacity(LOG_LEN),
            epoch: 1.0,
            rng: jsrt::rng::ClassicRandom::new(7.0),
            smolders: Vec::with_capacity(MAX_ACTIVE_SMOLDERS),
            pending_bolts: Vec::with_capacity(MAX_PENDING_BOLTS),
            scratch_events: Vec::with_capacity(LOG_LEN),
            bursts: 0,
            bolts: 0,
            smolder_starts: 0,
        };
        // Start and refresh smolder across three bones, verify timer keeps max and stops cleanly.
        fx.start_or_refresh_smolder((2, 1), arm_l, 2, 4.75, [0., 1.1, 6.], 1, true, &mut effects);
        fx.start_or_refresh_smolder((2, 1), arm_r, 3, 4.75, [0., 1.1, 6.], 2, true, &mut effects);
        fx.start_or_refresh_smolder((2, 1), head, 2, 4.75, [0., 1.65, 6.], 3, true, &mut effects);
        assert_eq!(fx.smolders.len(), 3);
        assert_eq!(fx.smolder_starts, 3);
        fx.smolders[0].remaining = 1.25;
        fx.start_or_refresh_smolder((2, 1), arm_l, 2, 4.75, [0., 1.1, 6.], 4, true, &mut effects);
        assert_eq!(fx.smolders.len(), 3);
        assert_eq!(fx.smolders[0].remaining, 4.75);
        assert_eq!(fx.smolder_starts, 3);
        fx.reset(&mut effects);
        assert!(fx.smolders.is_empty());
        assert!(fx.lights.is_empty());
    }

    #[test]
    fn frame_dispatches_push_burst_lightning_target_blocked_no_target_and_ignores_buff() {
        struct FakeBodies {
            alive_target: bool,
            hand: [f64; 3],
            target_feet: [f64; 3],
            blocker_feet: [f64; 3],
        }
        impl PowerBodies for FakeBodies {
            fn is_alive(&self, id: (u32, u32)) -> bool {
                id == (1, 1) || (id == (2, 1) && self.alive_target) || id == (3, 1)
            }
            fn feet_world(&self, id: (u32, u32)) -> Option<[f64; 3]> {
                match id {
                    (1, 1) => Some([0., 0., 0.]),
                    (2, 1) => Some(self.target_feet),
                    (3, 1) => Some(self.blocker_feet),
                    _ => None,
                }
            }
            fn aim_angles(&self, id: (u32, u32)) -> Option<(f64, f64)> {
                (id == (1, 1)).then_some((0., 0.))
            }
            fn hand_world(&mut self, id: (u32, u32)) -> Option<[f64; 3]> {
                (id == (1, 1)).then_some(self.hand)
            }
            fn chest_world(&mut self, id: (u32, u32)) -> Option<[f64; 3]> {
                self.feet_world(id).map(|p| [p[0], p[1] + 1.1, p[2]])
            }
            fn bone_world(&mut self, id: (u32, u32), bone_crc: u32) -> Option<[f64; 3]> {
                let p = self.feet_world(id)?;
                let y = if bone_crc == zero_hash::crc("joint_crest") { 1.65 } else { 1.05 };
                Some([p[0], p[1] + y, p[2]])
            }
        }
        let mut effects = test_spawner(4);
        let arm_l = zero_hash::crc("joint_alpha");
        let arm_r = zero_hash::crc("joint_beta");
        let head = zero_hash::crc("joint_crest");
        let arc_w = zero_fnv1a_lower("coil_discharge");
        let push_w = zero_fnv1a_lower("gust_shove");
        let buff_w = zero_fnv1a_lower("rally_aura");
        let mut fx = PowerEffects {
            visuals: vec![
                PowerVisual {
                    weapon: arc_w,
                    explosion: [Some(1), Some(1)],
                    cast_effect: None,
                    lightning: Some("coil_bolt".into()),
                    lightning_scale: 0.62,
                    cone_length: 23.5,
                    no_target_count: 4,
                    no_target_spread: 0.22,
                    smolder_timer: 4.75,
                    smolders: vec![(arm_l, 2), (arm_r, 3), (head, 2)],
                },
                PowerVisual {
                    weapon: push_w,
                    explosion: [Some(4), Some(4)],
                    cast_effect: None,
                    lightning: None,
                    lightning_scale: 1.0,
                    cone_length: 9.5,
                    no_target_count: 1,
                    no_target_spread: 0.0,
                    smolder_timer: 0.0,
                    smolders: vec![],
                },
            ],
            blasts: vec![],
            class_cones: vec![(7, 23.5)],
            water_height: None,
            lights: Vec::with_capacity(8),
            burn_sets: vec![],
            burns_due: Vec::with_capacity(LOG_LEN),
            epoch: 1.0,
            rng: jsrt::rng::ClassicRandom::new(11.0),
            smolders: Vec::with_capacity(MAX_ACTIVE_SMOLDERS),
            pending_bolts: Vec::with_capacity(MAX_PENDING_BOLTS),
            scratch_events: Vec::with_capacity(LOG_LEN),
            bursts: 0,
            bolts: 0,
            smolder_starts: 0,
        };
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.epoch = 1.0;
        let (caster, victim, blocker) = ((1, 1), (2, 1), (3, 1));
        let mut bodies = FakeBodies {
            alive_target: true,
            hand: [0., 1.35, 0.25],
            target_feet: [0., 0., 7.5],
            blocker_feet: [0., 0., 4.5],
        };
        let mut cast_hit = |_: [f64; 3], _: [f64; 3], _: f64| Some(12.5);
        // Dispatch arc (Cast + Hit + Blocked -> 1 hit bolt + 1 pulled-back blocked bolt + 2 no-target bolts + 1 explosion + 3 smolders),
        // push (Cast + Thrown -> 1 explosion burst, 0 bolts), and buff (ignored).
        fx.scratch_events.extend([
            NetPower { sequence: 1, tick: 10., caster, target: None, weapon: arc_w, kind: PowerEventKind::Cast, position: None },
            NetPower { sequence: 2, tick: 10., caster, target: Some(victim), weapon: arc_w, kind: PowerEventKind::Hit, position: None },
            NetPower { sequence: 3, tick: 10., caster, target: Some(blocker), weapon: arc_w, kind: PowerEventKind::Blocked, position: None },
            NetPower { sequence: 4, tick: 11., caster, target: None, weapon: push_w, kind: PowerEventKind::Cast, position: None },
            NetPower { sequence: 5, tick: 11., caster, target: Some(victim), weapon: push_w, kind: PowerEventKind::Thrown, position: None },
            NetPower { sequence: 6, tick: 12., caster, target: Some(caster), weapon: buff_w, kind: PowerEventKind::Buffed, position: None },
        ]);
        fx.dispatch_scratch(&receiver, &mut bodies, &mut effects, &mut cast_hit);
        assert_eq!(fx.bursts, 2, "1 arc impact explosion + 1 push explosion");
        assert_eq!(fx.bolts, 4, "1 hit + 1 blocked + 2 no-target bolts");
        assert_eq!(fx.pending_bolts.len(), 4);
        assert_eq!(fx.pending_bolts[0].to, [0., 1.1, 7.5]);
        assert!((fx.pending_bolts[0].charge - 0.62).abs() < 1e-9);
        // Blocked bolt stops BLOCKED_PULLBACK (0.75 m) short of the defender's chest.
        let blocked_end = fx.pending_bolts[1].to;
        let blocked_dist_to_chest = ((blocked_end[0]).powi(2)
            + (blocked_end[1] - 1.1).powi(2)
            + (blocked_end[2] - 4.5).powi(2))
        .sqrt();
        assert!((blocked_dist_to_chest - BLOCKED_PULLBACK).abs() < 1e-5);
        // Remaining 2 no-target bolts terminate at raycast hit distance (12.5 m from hand).
        for bolt in &fx.pending_bolts[2..] {
            let span = ((bolt.to[0] - bolt.from[0]).powi(2)
                + (bolt.to[1] - bolt.from[1]).powi(2)
                + (bolt.to[2] - bolt.from[2]).powi(2))
            .sqrt();
            assert!((span - 12.5).abs() < 1e-5);
        }
        assert_eq!(fx.smolders.len(), 3);
        // Verify smolder expiry and victim death cleanup through frame_with_bodies.
        fx.smolders[0].remaining = 0.4;
        fx.smolders[1].remaining = 0.5;
        fx.smolders[2].remaining = 2.8;
        fx.frame_with_bodies(&receiver, &mut bodies, 0.6, &mut effects, &mut cast_hit);
        assert_eq!(fx.smolders.len(), 1, "expired alpha/beta smolders stopped while crest continues");
        assert_eq!(fx.smolders[0].bone_crc, head);
        bodies.alive_target = false;
        fx.frame_with_bodies(&receiver, &mut bodies, 0.1, &mut effects, &mut cast_hit);
        assert!(fx.smolders.is_empty(), "victim death immediately stops remaining smolders");
        assert_eq!(fx.class_lightning_cone(7), Some(23.5));
        assert_eq!(fx.class_lightning_cone(9), None);
        assert!(fx.visual_index(buff_w).is_none());
    }

    #[test]
    fn a_blade_blast_shows_its_explosion_once_at_the_wielders_feet() {
        use bfe_presentation::layout::{command as c, frame as f};
        let names = vec![("dust_disc".to_string(), 1), ("foam_disc".to_string(), 2)];
        let pack = bfe_content::json::parse(
            r#"{"bindings": {"expc/stomp_wave": {"Effect": ["dust_disc"], "WaterEffect": ["foam_disc"]}}}"#,
        )
        .unwrap();
        let blade = bfe_content::json::parse(
            r#"{"class": "quake_blade", "base": "melee", "ExplosionName": "stomp_wave"}"#,
        )
        .unwrap();
        let blast = blade_blast(&blade, &pack, &names).unwrap();
        assert_eq!(blast, BladeBlast { weapon: zero_fnv1a_lower("quake_blade"), groups: [Some(1), Some(2)], light: None });
        let gun = bfe_content::json::parse(r#"{"class": "pop_gun", "base": "cannon", "ExplosionName": "stomp_wave"}"#).unwrap();
        assert!(blade_blast(&gun, &pack, &names).is_none(), "only a blade's combo plays its explosion");
        let unbound = bfe_content::json::parse(r#"{"class": "plain_blade", "base": "melee", "ExplosionName": "quiet_wave"}"#).unwrap();
        assert!(blade_blast(&unbound, &pack, &names).is_none());

        struct Feet;
        impl PowerBodies for Feet {
            fn is_alive(&self, _: (u32, u32)) -> bool {
                true
            }
            fn feet_world(&self, id: (u32, u32)) -> Option<[f64; 3]> {
                // Drawn bodies lag the battle: the published position wins.
                Some(if id == (5, 2) { [70., 4.5, -30.] } else { [9., 1.5, -1.] })
            }
            fn aim_angles(&self, _: (u32, u32)) -> Option<(f64, f64)> {
                Some((0., 0.))
            }
            fn hand_world(&mut self, _: (u32, u32)) -> Option<[f64; 3]> {
                None
            }
            fn chest_world(&mut self, _: (u32, u32)) -> Option<[f64; 3]> {
                None
            }
            fn bone_world(&mut self, _: (u32, u32), _: u32) -> Option<[f64; 3]> {
                None
            }
        }
        let mut effects = test_spawner(2);
        let glow = super::super::blast_lights::light_of(&bfe_content::json::parse(
            r#"{"LightColor": "40 80 200 255", "LightRadius": "6.5", "LightDuration": "0.75"}"#,
        ).unwrap());
        let mut fx = PowerEffects {
            visuals: vec![],
            blasts: vec![BladeBlast { light: glow, ..blast }],
            class_cones: vec![],
            water_height: Some(0.5),
            lights: Vec::with_capacity(8),
            burn_sets: vec![],
            burns_due: Vec::with_capacity(LOG_LEN),
            epoch: 1.0,
            rng: jsrt::rng::ClassicRandom::new(3.0),
            smolders: Vec::with_capacity(MAX_ACTIVE_SMOLDERS),
            pending_bolts: Vec::with_capacity(MAX_PENDING_BOLTS),
            scratch_events: Vec::with_capacity(LOG_LEN),
            bursts: 0,
            bolts: 0,
            smolder_starts: 0,
        };
        let receiver = Receiver::new(serde_json::Value::Null);
        let (wielder, struck) = ((5, 2), (6, 2));
        let mut no_ray = |_: [f64; 3], _: [f64; 3], _: f64| None;
        fx.scratch_events.extend([
            NetPower { sequence: 40, tick: 90., caster: wielder, target: None, weapon: blast.weapon, kind: PowerEventKind::Cast, position: Some([7., 1.5, -3.]) },
            NetPower { sequence: 41, tick: 90., caster: wielder, target: Some(struck), weapon: blast.weapon, kind: PowerEventKind::Thrown, position: None },
        ]);
        fx.dispatch_scratch(&receiver, &mut Feet, &mut effects, &mut no_ray);
        assert_eq!(fx.bursts, 1, "one explosion for the blast, none on the body it threw");
        assert_eq!(effects.owner.input[f::COMMANDS], 1.);
        let row = &effects.owner.input[f::STRIDE..][..c::STRIDE];
        assert_eq!(row[c::CONTENT], 1., "the dry effect above the water line");
        assert_eq!(&row[c::POSITION..c::POSITION + 3], &[7., 1.7, -3.], "0.2 m above where the battle put the wielder");
        assert_eq!(&row[c::QUATERNION..c::QUATERNION + 4], &[0., 0., 0., 1.], "upright");
        let mut flashed = Vec::new();
        fx.take_lights(|at, light| flashed.push((at, light)));
        assert_eq!(flashed, vec![([7., 1.7, -3.], glow.unwrap())], "its explosion's light, at the effect");
        fx.take_lights(|_, _| panic!("flashed twice"));
    }

    #[test]
    fn a_published_burn_sits_on_the_bones_its_weapon_names_and_outlasts_the_body_when_told() {
        use bfe_presentation::network_layout::net_record as wire;
        struct Body {
            alive: bool,
        }
        impl PowerBodies for Body {
            fn is_alive(&self, _: (u32, u32)) -> bool {
                self.alive
            }
            fn feet_world(&self, _: (u32, u32)) -> Option<[f64; 3]> {
                Some([4., 0., 4.])
            }
            fn aim_angles(&self, _: (u32, u32)) -> Option<(f64, f64)> {
                None
            }
            fn hand_world(&mut self, _: (u32, u32)) -> Option<[f64; 3]> {
                None
            }
            fn chest_world(&mut self, _: (u32, u32)) -> Option<[f64; 3]> {
                Some([4., 1.2, 4.])
            }
            fn bone_world(&mut self, id: (u32, u32), bone: u32) -> Option<[f64; 3]> {
                (id == (8, 2)).then(|| [4., if bone == zero_hash::crc("bone_crown") { 1.7 } else { 0.9 }, 4.])
            }
        }
        let (crown, keel) = (zero_hash::crc("bone_crown"), zero_hash::crc("bone_keel"));
        let mut effects = test_spawner(3);
        let mut fx = PowerEffects {
            visuals: vec![],
            blasts: vec![],
            class_cones: vec![],
            water_height: None,
            lights: Vec::with_capacity(8),
            burn_sets: vec![BurnSet { effect: "inv_cinders".into(), pairs: vec![(crown, 2), (keel, 3)], vanish: false }],
            burns_due: Vec::with_capacity(LOG_LEN),
            epoch: 1.0,
            rng: jsrt::rng::ClassicRandom::new(3.0),
            smolders: Vec::with_capacity(MAX_ACTIVE_SMOLDERS),
            pending_bolts: Vec::with_capacity(MAX_PENDING_BOLTS),
            scratch_events: Vec::with_capacity(LOG_LEN),
            bursts: 0,
            bolts: 0,
            smolder_starts: 0,
        };
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.epoch = 1.0;
        let mut row = [0.; wire::STRIDE];
        for (id, bytes) in [
            (40., &br#"{"kind":"power","action":"burn","weaponId":0,"ownerId":3,"ownerGeneration":1,"targetId":8,"targetGeneration":2,"name":"INV_Cinders","duration":2.5}"#[..]),
            (41., &br#"{"kind":"power","action":"burn","weaponId":0,"ownerId":3,"ownerGeneration":1,"targetId":8,"targetGeneration":2,"name":"other_fx","duration":2.5}"#[..]),
        ] {
            row[wire::KIND] = kind::COMBAT as f64;
            row[wire::SCHEMA] = 8.;
            row[wire::ID] = id;
            row[wire::GENERATION] = 1.;
            row[wire::TICK] = 70.;
            receiver.publications.push(&row, bytes);
        }
        let mut body = Body { alive: true };
        let mut no_ray = |_: [f64; 3], _: [f64; 3], _: f64| None;
        fx.frame_with_bodies(&receiver, &mut body, 0., &mut effects, &mut no_ray);
        let lit: Vec<((u32, u32), u32, usize, f64)> = fx.smolders.iter().map(|s| (s.target, s.bone_crc, s.group, s.remaining)).collect();
        assert_eq!(lit, vec![((8, 2), crown, 2, 2.5), ((8, 2), keel, 3, 2.5)], "one copy on each named bone, for its time left");
        assert!(fx.scratch_events.is_empty(), "a burn is no power hit");
        receiver.publications.clear();
        body.alive = false;
        fx.frame_with_bodies(&receiver, &mut body, 1., &mut effects, &mut no_ray);
        assert_eq!(fx.smolders.len(), 2, "this weapon's burn stays on the body after its death");
        fx.frame_with_bodies(&receiver, &mut body, 2., &mut effects, &mut no_ray);
        assert!(fx.smolders.is_empty(), "and ends with its time");
    }

    #[test]
    fn power_events_are_read_from_the_battles_publications() {
        use bfe_presentation::network_layout::net_record as wire;
        let mut receiver = Receiver::new(serde_json::Value::Null);
        let mut row = [0.; wire::STRIDE];
        let mut publish = |receiver: &mut Receiver, id: f64, schema: f64, bytes: &[u8]| {
            row[wire::KIND] = kind::COMBAT as f64;
            row[wire::SCHEMA] = schema;
            row[wire::ID] = id;
            row[wire::GENERATION] = 1.;
            row[wire::TICK] = 300.;
            receiver.publications.push(&row, bytes);
        };
        publish(&mut receiver, 70., 8., br#"{"kind":"power","action":"blocked","weaponId":4000000001,"ownerId":9,"ownerGeneration":3,"targetId":14,"targetGeneration":5,"position":[1,2,3]}"#);
        publish(&mut receiver, 71., 8., br#"{"kind":"effect","name":"puff","position":[0,0,0]}"#);
        publish(&mut receiver, 72., 8., br#"{"kind":"power","action":"glow","weaponId":5,"ownerId":9,"ownerGeneration":3}"#);
        publish(&mut receiver, 73., 8., br#"{"kind":"power","action":"cast","weaponId":5,"ownerId":9,"ownerGeneration":3}"#);
        let mut fx = PowerEffects {
            visuals: vec![],
            blasts: vec![],
            class_cones: vec![],
            water_height: None,
            lights: Vec::with_capacity(8),
            burn_sets: vec![],
            burns_due: Vec::with_capacity(LOG_LEN),
            epoch: f64::NAN,
            rng: jsrt::rng::ClassicRandom::new(3.0),
            smolders: Vec::with_capacity(MAX_ACTIVE_SMOLDERS),
            pending_bolts: Vec::with_capacity(MAX_PENDING_BOLTS),
            scratch_events: Vec::with_capacity(LOG_LEN),
            bursts: 0,
            bolts: 0,
            smolder_starts: 0,
        };
        fx.read(&receiver);
        assert_eq!(fx.scratch_events, vec![
            NetPower { sequence: 70, tick: 300., caster: (9, 3), target: Some((14, 5)), weapon: 4_000_000_001, kind: PowerEventKind::Blocked, position: Some([1., 2., 3.]) },
            NetPower { sequence: 73, tick: 300., caster: (9, 3), target: None, weapon: 5, kind: PowerEventKind::Cast, position: None },
        ], "named effects and unknown actions are not power events");
    }
}
