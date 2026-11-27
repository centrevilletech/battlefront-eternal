//! Mounted guns can name visual rounds outside the replicated combat registry.
use super::{flash, group, hit_groups, values, Burst, Groups, MissionEffects};
use crate::online::{
    combat::{Bolt, Combat, Style, COSMETICS, MERGE},
    particles::Prepared,
};
use bfe_content::Value;
use bfe_content_native::{loader::authored_json, BattleContent};
use bfe_presentation::{
    ordnance::style,
    ordnance_layout::{ordnance_limits as limits, ordnance_style as s},
};
use std::collections::BTreeMap;
use three::{Matrix4, Quaternion, Vector3};

fn definitions(pack: &Value) -> impl Iterator<Item = (&str, &Value)> {
    pack.get("rounds")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|o| o.entries())
}

/// The texture every bolt and beam draws its glow strip with; no class names it.
const GLOW_TEXTURE: &str = "lightglow";

/// The textures the mission's rounds draw with: each round's laser texture once, then the shared glow texture.
fn round_texture_names(pack: &Value) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for (_, def) in definitions(pack) {
        if let Some(name) = def.get("LaserTexture").and_then(Value::as_str) {
            if !names.iter().any(|n| n == name) {
                names.push(name.into());
            }
        }
    }
    if !names.iter().any(|n| n == GLOW_TEXTURE) {
        names.push(GLOW_TEXTURE.into());
    }
    names
}

pub(in crate::online) fn load_round_textures(
    battle: &mut BattleContent,
    prepared: &mut Prepared,
) -> Result<(), String> {
    let pack = battle.visual_effects().clone();
    for name in round_texture_names(&pack) {
        let std::collections::btree_map::Entry::Vacant(slot) = prepared.textures.entry(name.clone()) else {
            continue;
        };
        if let Some(path) = battle.texture_paths.get(&name.to_lowercase()) {
            match battle.textures.load(path) {
                Ok(image) => {
                    slot.insert(image);
                }
                Err(e) => eprintln!("Skipping mission round texture {name}: {e}"),
            }
        }
    }
    Ok(())
}

/// The width of a 2005 beam's second pass: 2.5 times its core width. A class could name its own
/// glow width; none of the beams a battle names does, so the exported definitions do not carry
/// one. 0 for a definition without a core width.
pub(in crate::online) fn beam_glow_width(def: &Value) -> f64 {
    const GLOW_WIDTH_PER_LASER_WIDTH: f64 = 2.5;
    def.get("LaserWidth")
        .and_then(Value::as_f64)
        .filter(|width| width.is_finite() && *width > 0.)
        .map_or(0., |width| width * GLOW_WIDTH_PER_LASER_WIDTH)
}

pub(in crate::online::combat) struct PreparedRound {
    name: String,
    def: Value,
    row: [f64; s::STRIDE],
    batches: [Option<usize>; 2],
}
pub(in crate::online::combat) fn prepare_rounds(
    battle: &BattleContent,
    prepared: &mut Prepared,
) -> Result<Vec<PreparedRound>, String> {
    let mut result = Vec::new();
    for (name, def) in definitions(battle.visual_effects()) {
        let authored: style::Content = match serde_json::from_value(authored_json(def)) {
            Ok(definition) => definition,
            Err(e) => {
                eprintln!("Skipping mission round {name}: {e}");
                continue;
            }
        };
        let mut row = style::row(&authored, true);
        if def.get("base").and_then(Value::as_str) == Some("beam") {
            row[s::BEAM_GLOW_WIDTH] = beam_glow_width(def);
        }
        let texture = def.get("LaserTexture").and_then(Value::as_str);
        let batches = [
            texture.and_then(|s| prepared.ordnance(s)),
            prepared.ordnance("lightglow"),
        ];
        if let Some(image) = style::resources(&authored, 0, true)["hue"]
            .as_str()
            .and_then(|n| prepared.textures.get(n))
        {
            if let Ok(pixels) = battle.textures.cpu_pixels(*image) {
                let hue = style::texture_hue(&pixels);
                if !hue.is_empty() {
                    row[s::LOOK_COLOR..s::LOOK_COLOR + 3].copy_from_slice(&style::vivid(&hue, 1.6));
                }
            }
        }
        result.push(PreparedRound {
            name: name.into(),
            def: def.clone(),
            row,
            batches,
        });
    }
    Ok(result)
}

#[derive(Clone, Copy)]
pub struct RoundClass(usize);

#[derive(Clone, Copy)]
pub enum Surface {
    Terrain,
    Static,
    Soft,
    Rigid,
    Water,
    Shield,
}

#[derive(Clone, Copy)]
pub struct Contact {
    pub tick: f64,
    pub position: [f64; 3],
    pub normal: Option<[f64; 3]>,
    pub surface: Surface,
}

#[derive(Clone, Copy)]
pub struct Shot {
    pub round: RoundClass,
    pub serial: u64,
    pub tick: f64,
    pub position: [f64; 3],
    pub direction: [f64; 3],
    pub contact: Option<Contact>,
}

#[derive(Clone, Copy)]
struct FlightStyle {
    style: usize,
    speed: f64,
    range: Option<f64>,
    life: f64,
    fade: f64,
    impact: [Option<usize>; 6],
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::online::combat) struct BeamDraw {
    pub key: u32,
    pub style: usize,
    pub position: [f64; 3],
    pub end: [f64; 3],
    pub direction: [f64; 3],
    pub life: f64,
}
#[derive(Clone, Copy)]
struct ActiveBeam {
    key: u32,
    style: usize,
    position: [f64; 3],
    direction: [f64; 3],
    end: [f64; 3],
    length: f64,
    range: f64,
    start: f64,
    until: f64,
    serial: [u32; 2],
}
struct Weapon {
    flight: FlightStyle,
    flash: Vec<usize>,
    muzzle: Muzzle,
}
pub(in crate::online::combat) struct Rounds {
    weapons: BTreeMap<String, Vec<Option<RoundClass>>>,
    secondary: BTreeMap<String, Vec<Option<RoundClass>>>,
    classes: Vec<Weapon>,
    recent: Vec<(u64, f64)>,
    recent_at: usize,
    active: Vec<(Bolt, f64, RoundClass, u64)>,
    active_beams: Vec<ActiveBeam>,
    beam_draws: Vec<BeamDraw>,
}
fn batch(quads: &mut Vec<usize>, batch: Option<usize>) -> f64 {
    let Some(id) = batch else { return -1. };
    if let Some(slot) = quads.iter().position(|q| *q == id) {
        return slot as f64;
    }
    if quads.len() == limits::HANDLES {
        return -1.;
    }
    quads.push(id);
    (quads.len() - 1) as f64
}
/// The weapon can independently override the shared mesh and the light at its muzzle.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub(in crate::online) struct Muzzle {
    pub radius: f64,
    pub ticks: f64,
    pub color: [f64; 4],
    pub light: Option<([f64; 3], f64, f64)>,
}
impl Muzzle {
    pub(super) fn read(pack: &Value, weapon: &str) -> Self {
        let settings = pack.get("flashes").and_then(|table| table.get(weapon));
        if matches!(settings.and_then(|w| w.get("disabled")), Some(Value::Bool(true))) {
            return Self::default();
        }
        // Older libraries preserve the preset spelling in the binding list alone.
        let preset = settings
            .and_then(|w| w.get("MuzzleFlash"))
            .and_then(Value::as_str)
            .or_else(|| values(pack.get("bindings")?.get(weapon)?, "MuzzleFlash").last());
        let (radius, seconds, glow) = match preset {
            Some(p) if p.eq_ignore_ascii_case("small_muzzle_flash") => (0.6, 0.1, [63., 63., 13.]),
            Some(p) if p.eq_ignore_ascii_case("med_muzzle_flash") => (1., 0.15, [127., 127., 25.]),
            Some(p) if p.eq_ignore_ascii_case("large_muzzle_flash") => {
                (1.8, 0.3, [191., 191., 37.])
            }
            _ => (0., 0.1, [127., 127., 25.]),
        };
        let field = |key: &str| settings.and_then(|w| w.get(key));
        let number = |key, default| {
            field(key)
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite())
                .unwrap_or(default)
        };
        let rgba = |key, default: [f64; 4]| match field(key).and_then(Value::as_array) {
            Some(v) => std::array::from_fn(|i| {
                v.get(i)
                    .and_then(Value::as_f64)
                    .unwrap_or(255.)
                    .clamp(0., 255.)
                    / 255.
            }),
            None => default.map(|v| v / 255.),
        };
        let color = rgba("FlashColor", [255., 255., 50., 255.]);
        let light_color = rgba("FlashLightColor", [glow[0], glow[1], glow[2], 255.]);
        let range = number("FlashLightRadius", 0.);
        let duration = number("FlashLightDuration", 0.);
        Self {
            radius: number("FlashRadius", radius),
            ticks: number("FlashLength", seconds).max(0.) * 60.,
            color,
            light: (range > 0. && duration > 0.).then(|| {
                (
                    std::array::from_fn(|i| light_color[i] * light_color[3]),
                    range,
                    duration,
                )
            }),
        }
    }
    pub(in crate::online) fn opacity(self, age: f64) -> Option<f64> {
        (self.radius > 0. && age >= 0. && age < self.ticks)
            .then(|| self.color[3] * (2. * (1. - age / self.ticks)).min(1.))
    }
}
pub(super) struct MeshFlash {
    position: [f64; 3],
    direction: [f64; 3],
    tick: f64,
    muzzle: Muzzle,
}
impl MeshFlash {
    pub(super) fn at_barrel(bolt: &Bolt, muzzle: Muzzle) -> Option<Self> {
        let length = bolt.direction.iter().map(|v| v * v).sum::<f64>().sqrt();
        let position: [f64; 3] = std::array::from_fn(|i| bolt.position[i] + bolt.offset[i]);
        if muzzle.radius <= 0.
            || muzzle.ticks <= 0.
            || !length.is_finite()
            || length <= 0.
            || !position.iter().all(|v| v.is_finite())
        {
            return None;
        }
        Some(Self {
            position,
            direction: bolt.direction.map(|v| v / length),
            tick: bolt.flight,
            muzzle,
        })
    }
    pub(super) fn ends(&self) -> f64 {
        self.tick + self.muzzle.ticks
    }
    fn opacity(&self, tick: f64) -> Option<f64> {
        self.muzzle.opacity(tick - self.tick)
    }
    pub(super) fn draw(
        &self,
        tick: f64,
        particles: &mut crate::online::particles::Particles,
        bridge: &mut crate::render_bridge::Bridge,
    ) -> bool {
        let Some(opacity) = self.opacity(tick) else {
            return false;
        };
        let mut turn = Quaternion::default();
        let [dx, dy, dz] = self.direction;
        turn.set_from_unit_vectors(&Vector3::new(0., 0., 1.), &Vector3::new(dx, dy, dz));
        let mut matrix = Matrix4::default();
        let size = self.muzzle.radius;
        let [x, y, z] = self.position;
        matrix.compose(
            &Vector3::new(x, y, z),
            &turn,
            &Vector3::new(size, size, size),
        );
        particles.flash(bridge, &matrix.elements, &self.muzzle.color[..3], opacity)
    }
}
pub(super) enum ShotAdmission {
    Accepted { direction: [f64; 3] },
    Ignored,
    Refused,
}

impl Rounds {
    pub(super) fn for_beam_harness(range: f64, life: f64, capacity: usize) -> (Self, RoundClass) {
        let flight = FlightStyle {
            style: 1,
            speed: 0.,
            range: Some(range),
            life,
            fade: 0.,
            impact: [None; 6],
        };
        (
            Self {
                weapons: BTreeMap::new(),
                secondary: BTreeMap::new(),
                classes: vec![Weapon {
                    flight,
                    flash: Vec::new(),
                    muzzle: Muzzle::default(),
                }],
                recent: Vec::with_capacity(capacity),
                recent_at: 0,
                active: Vec::with_capacity(capacity),
                active_beams: Vec::with_capacity(capacity),
                beam_draws: Vec::with_capacity(capacity),
            },
            RoundClass(0),
        )
    }

    pub(super) fn observe_shot(&mut self, next_serial: &mut u32, shot: Shot) -> ShotAdmission {
        let Some(weapon) = self.classes.get(shot.round.0) else {
            return ShotAdmission::Ignored;
        };
        let serial = next_serial.wrapping_add(1).max(1);
        let key = (shot.serial, shot.tick);
        if self.recent.contains(&key) {
            return ShotAdmission::Ignored;
        }
        let direction = if weapon.flight.range.is_some() {
            let Some(beam) = shot.beam(weapon.flight, serial) else {
                return ShotAdmission::Ignored;
            };
            if self.active_beams.len() == self.active_beams.capacity() {
                return ShotAdmission::Refused;
            }
            *next_serial = serial;
            self.active_beams.push(beam);
            beam.direction
        } else {
            let Some(flight) = shot.flight(weapon.flight, serial) else {
                return ShotAdmission::Ignored;
            };
            if self.active.len() == self.active.capacity() {
                return ShotAdmission::Refused;
            }
            *next_serial = serial;
            self.active
                .push((flight.0, flight.1, shot.round, shot.serial));
            flight.0.direction
        };
        if self.recent.len() == self.recent.capacity() {
            if !self.recent.is_empty() {
                self.recent[self.recent_at] = key;
                self.recent_at = (self.recent_at + 1) % self.recent.capacity();
            }
        } else {
            self.recent.push(key);
        }
        ShotAdmission::Accepted { direction }
    }

    pub(super) fn new(
        pack: &Value,
        wire: &[(String, Value)],
        combat: &mut Combat,
        prepared: Vec<PreparedRound>,
        groups: &Groups,
    ) -> Self {
        let mut styles = BTreeMap::new();
        for PreparedRound {
            name,
            def,
            mut row,
            batches,
        } in prepared
        {
            let base = def.get("base").and_then(Value::as_str).unwrap_or("");
            let is_beam = base == "beam";
            let (speed, range) = if is_beam {
                let Some(range) = def
                    .get("Range")
                    .and_then(Value::as_f64)
                    .filter(|v| *v > 0.)
                else {
                    continue;
                };
                (0., Some(range))
            } else {
                let Some(speed) = def
                    .get("Velocity")
                    .and_then(Value::as_f64)
                    .filter(|v| *v > 0.)
                else {
                    continue;
                };
                (speed, None)
            };
            let Some(life) = def
                .get("LifeSpan")
                .and_then(Value::as_f64)
                .filter(|v| *v > 0.)
            else {
                continue;
            };
            let mut impact = hit_groups(&def, &combat.effect_names);
            let shield = def
                .get("ImpactEffectShield")
                .or_else(|| def.get("ImpactEffect"))
                .and_then(Value::as_str)
                .and_then(|n| group(&combat.effect_names, n));
            let existing = wire
                .iter()
                .enumerate()
                .find(|(_, (_, d))| {
                    d.get("class")
                        .and_then(Value::as_str)
                        .is_some_and(|c| name.strip_prefix("ordc/") == Some(c))
                })
                .and_then(|(i, _)| combat.definitions[i + 1]);
            let style = if let Some(i) = existing {
                i
            } else {
                if combat.styles.len() == limits::STYLES {
                    continue;
                }
                row[s::CORE_BATCH] = batch(&mut combat.quads, batches[0]);
                row[s::GLOW_BATCH] = batch(&mut combat.quads, batches[1]);
                row[s::BEAM_BATCH] = if is_beam {
                    batch(&mut combat.quads, batches[0].or(batches[1]))
                } else {
                    -1.
                };
                let i = combat.styles.len();
                combat.ordnance.style_input[..s::STRIDE].copy_from_slice(&row);
                combat.ordnance.set_style(i);
                let rail = (!is_beam && row[s::FADE_OUT_TIME] > 0.).then(|| {
                    let rgb = ["LaserGlowColor", "LightColor"]
                        .into_iter()
                        .find_map(|key| def.get(key).and_then(Value::as_array));
                    (
                        std::array::from_fn(|i| {
                            rgb.and_then(|c| c.get(i))
                                .and_then(Value::as_f64)
                                .unwrap_or([120., 160., 255.][i])
                        }),
                        speed * life,
                    )
                });
                combat.styles.push(Style {
                    model: None,
                    impact,
                    explosion: [None; 2],
                    trail: None,
                    rail,
                    fade_out_ticks: 0.,
                });
                i
            };
            // The wire uses the sixth slot for a miss. An explicit shield contact has its own group here.
            impact[Surface::Shield as usize] = shield;
            styles.insert(
                name,
                FlightStyle {
                    style,
                    speed,
                    range,
                    life,
                    fade: row[s::FADE_OUT_TIME],
                    impact,
                },
            );
        }
        let mut weapons = BTreeMap::new();
        let mut secondary = BTreeMap::new();
        let mut classes = Vec::new();
        if let Some(links) = pack.get("links").and_then(Value::as_object) {
            for (name, fields) in links
                .entries()
                .filter(|(name, _)| name.starts_with("wpnc/"))
            {
                let slots = values(fields, "OrdnanceName")
                    .map(|target| {
                        let flight = *styles.get(target)?;
                        let index = classes.len();
                        classes.push(Weapon {
                            flight,
                            flash: flash(groups, name),
                            muzzle: Muzzle::read(pack, name),
                        });
                        Some(RoundClass(index))
                    })
                    .collect::<Vec<_>>();
                if slots.iter().any(Option::is_some) {
                    weapons.insert(name.into(), slots);
                }
                let secondary_slots = values(fields, "SecondaryOrdnanceName")
                    .map(|target| {
                        let flight = *styles.get(target)?;
                        let index = classes.len();
                        classes.push(Weapon {
                            flight,
                            flash: Vec::new(),
                            muzzle: Muzzle::default(),
                        });
                        Some(RoundClass(index))
                    })
                    .collect::<Vec<_>>();
                if secondary_slots.iter().any(Option::is_some) {
                    secondary.insert(name.into(), secondary_slots);
                }
            }
        }
        Self {
            weapons,
            secondary,
            classes,
            recent: Vec::with_capacity(limits::RECORDS),
            recent_at: 0,
            active: Vec::with_capacity(COSMETICS),
            active_beams: Vec::with_capacity(COSMETICS),
            beam_draws: Vec::with_capacity(COSMETICS),
        }
    }
    pub(super) fn reserve_live(&mut self, count: usize) {
        self.active.reserve(count);
        self.active_beams.reserve(count);
        self.beam_draws.reserve(count);
    }

    /// The bolt's flight ends on `tick`. Where the end point is known it ends there: the
    /// fastest rounds cover hundreds of metres in a tick and would be drawn far past it.
    pub(super) fn finish_live(&mut self, serial: u64, tick: f64, at: Option<[f64; 3]>) {
        self.finish_live_words([serial as u32, (serial >> 32) as u32], tick, at);
    }

    pub(super) fn finish_live_words(&mut self, serial: [u32; 2], tick: f64, at: Option<[f64; 3]>) {
        if let Some((bolt, end, class, _)) = self
            .active
            .iter_mut()
            .find(|r| [r.3 as u32, (r.3 >> 32) as u32] == serial)
        {
            let mut seconds = (tick - bolt.flight) / 60.;
            if let Some(at) = at {
                let flown: f64 = (0..3).map(|i| (at[i] - bolt.position[i]) * bolt.direction[i]).sum();
                seconds = seconds.min(flown / bolt.speed);
            }
            bolt.life = seconds.max(0.).min(bolt.life);
            *end = tick + self.classes[class.0].flight.fade * 60.;
        }
        if let Some(at) = at {
            if let Some(beam) = self.active_beams.iter_mut().find(|r| r.serial == serial) {
                let flown: f64 = (0..3)
                    .map(|i| (at[i] - beam.position[i]) * beam.direction[i])
                    .sum();
                beam.length = flown.clamp(0., beam.range);
                beam.end =
                    std::array::from_fn(|i| beam.position[i] + beam.direction[i] * beam.length);
            }
        }
    }

    pub(super) fn has_beam(&self, serial: [u32; 2]) -> bool {
        self.active_beams.iter().any(|r| r.serial == serial)
    }

    pub(super) fn move_beam(
        &mut self,
        serial: [u32; 2],
        position: [f64; 3],
        direction: [f64; 3],
    ) -> bool {
        let Some(beam) = self.active_beams.iter_mut().find(|r| r.serial == serial) else {
            return false;
        };
        let mag = direction.iter().map(|n| n * n).sum::<f64>().sqrt();
        if !mag.is_finite() || mag <= 0. || !position.iter().all(|n| n.is_finite()) {
            return true;
        }
        beam.position = position;
        beam.direction = direction.map(|v| v / mag);
        beam.end = std::array::from_fn(|i| beam.position[i] + beam.direction[i] * beam.length);
        true
    }

    pub(super) fn clear(&mut self) {
        self.recent.clear();
        self.recent_at = 0;
        self.active.clear();
        self.active_beams.clear();
        self.beam_draws.clear();
    }
    pub(in crate::online::combat) fn draw_flashes(
        &self,
        tick: f64,
        particles: &mut crate::online::particles::Particles,
        bridge: &mut crate::render_bridge::Bridge,
    ) {
        for (bolt, _, class, _) in &self.active {
            let weapon = &self.classes[class.0];
            if let Some(flash) = MeshFlash::at_barrel(bolt, weapon.muzzle) {
                flash.draw(tick, particles, bridge);
            }
        }
    }
    pub(in crate::online::combat) fn display(&mut self, tick: f64, slots: &mut [Bolt]) -> usize {
        self.active.retain(|(_, until, _, _)| tick <= *until);
        let mut count = 0;
        for ((bolt, _, _, _), slot) in self
            .active
            .iter()
            .filter(|(bolt, _, _, _)| bolt.flight <= tick)
            .zip(slots)
        {
            *slot = *bolt;
            count += 1;
        }
        count
    }
    pub(in crate::online::combat) fn take_beams(&mut self, tick: f64) -> Vec<BeamDraw> {
        self.active_beams.retain(|beam| tick <= beam.until);
        let mut draws = std::mem::take(&mut self.beam_draws);
        draws.clear();
        for beam in self.active_beams.iter().filter(|beam| beam.start <= tick) {
            draws.push(BeamDraw {
                key: beam.key,
                style: beam.style,
                position: beam.position,
                end: beam.end,
                direction: beam.direction,
                life: ((beam.until - tick) / 60.).max(0.),
            });
        }
        draws
    }
    pub(in crate::online::combat) fn return_beams(&mut self, mut draws: Vec<BeamDraw>) {
        draws.clear();
        self.beam_draws = draws;
    }
}

impl MissionEffects {
    /// Resolve the ordered OrdnanceName of a mounted or handheld weapon during admission.
    pub fn round(&self, weapon: &str, variant: usize) -> Option<RoundClass> {
        self.rounds
            .weapons
            .get(weapon)?
            .get(variant)
            .copied()
            .flatten()
    }
    /// Resolve the ordered SecondaryOrdnanceName of a mounted or handheld weapon during admission.
    pub fn secondary_round(&self, weapon: &str, variant: usize) -> Option<RoundClass> {
        self.rounds
            .secondary
            .get(weapon)?
            .get(variant)
            .copied()
            .flatten()
    }
}
impl Shot {
    fn beam(&self, style: FlightStyle, key: u32) -> Option<ActiveBeam> {
        let range = style.range?;
        if !self.tick.is_finite()
            || !self
                .position
                .iter()
                .chain(&self.direction)
                .all(|n| n.is_finite())
        {
            return None;
        }
        let mag = self.direction.iter().map(|n| n * n).sum::<f64>().sqrt();
        if !mag.is_finite() || mag <= 0. {
            return None;
        }
        let direction = self.direction.map(|v| v / mag);
        let mut length = range;
        if let Some(contact) = self.contact {
            if !contact.tick.is_finite()
                || contact.tick < self.tick
                || !contact.position.iter().all(|n| n.is_finite())
                || contact
                    .normal
                    .is_some_and(|normal| !normal.iter().all(|n| n.is_finite()))
            {
                return None;
            }
            let flown: f64 = (0..3)
                .map(|i| (contact.position[i] - self.position[i]) * direction[i])
                .sum();
            length = flown.clamp(0., range);
        }
        let end = std::array::from_fn(|i| self.position[i] + direction[i] * length);
        Some(ActiveBeam {
            key,
            style: style.style,
            position: self.position,
            direction,
            end,
            length,
            range,
            start: self.tick,
            until: self.tick + style.life * 60.,
            serial: [self.serial as u32, (self.serial >> 32) as u32],
        })
    }
    fn flight(&self, style: FlightStyle, key: u32) -> Option<(Bolt, f64)> {
        if !self.tick.is_finite()
            || !self
                .position
                .iter()
                .chain(&self.direction)
                .all(|n| n.is_finite())
        {
            return None;
        }
        let length = self.direction.iter().map(|n| n * n).sum::<f64>().sqrt();
        if !length.is_finite() || length <= 0. {
            return None;
        }
        let mut life = style.life;
        if let Some(contact) = self.contact {
            if !contact.tick.is_finite()
                || contact.tick < self.tick
                || !contact.position.iter().all(|n| n.is_finite())
                || contact
                    .normal
                    .is_some_and(|normal| !normal.iter().all(|n| n.is_finite()))
            {
                return None;
            }
            life = life.min((contact.tick - self.tick) / 60.);
        }
        Some((
            Bolt {
                active: true,
                started: true,
                aimed: true,
                key,
                style: style.style,
                position: self.position,
                direction: self.direction.map(|v| v / length),
                speed: style.speed,
                life,
                merge: MERGE.min(life * style.speed).max(1e-3),
                flight: self.tick,
                ..Bolt::default()
            },
            self.tick + (life + style.fade.max(0.)) * 60.,
        ))
    }
}
impl Combat {
    /// Present an observed shot whose source has no replicated launch. Its style and motion belong to its authored class.
    pub fn observe_shot(&mut self, shot: Shot) -> bool {
        let Some(mission) = &mut self.mission_effects else {
            return false;
        };
        let direction = match mission.rounds.observe_shot(&mut self.serial, shot) {
            ShotAdmission::Accepted { direction } => direction,
            ShotAdmission::Ignored => return false,
            ShotAdmission::Refused => {
                mission.refused = mission.refused.saturating_add(1);
                return false;
            }
        };
        let weapon = &mission.rounds.classes[shot.round.0];
        let base = Burst {
            serial: shot.serial,
            tick: shot.tick,
            position: shot.position,
            normal: Some(direction),
            group: 0,
        };
        let release = shot.tick + weapon.muzzle.ticks;
        if let Some(contact) = shot.contact {
            if let Some(group) = weapon.flight.impact[contact.surface as usize] {
                mission.observe(Burst {
                    group,
                    tick: contact.tick,
                    position: contact.position,
                    normal: contact.normal,
                    ..base
                });
            }
        }
        for i in 0..mission.rounds.classes[shot.round.0].flash.len() {
            let group = mission.rounds.classes[shot.round.0].flash[i];
            mission.observe_until(Burst { group, ..base }, release);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_missions_round_textures_are_each_laser_texture_once_and_the_shared_glow() {
        let pack = bfe_content::json::parse(r#"{"rounds":{
            "ordc/a":{"base":"beam","LaserTexture":"amber"},
            "ordc/b":{"base":"bolt","LaserTexture":"amber"},
            "ordc/c":{"base":"bolt","LaserTexture":"violet"},
            "ordc/d":{"base":"bolt"}}}"#).unwrap();
        assert_eq!(round_texture_names(&pack), ["amber", "violet", "lightglow"]);
        // A pack without rounds still asks for the glow texture; one that names it asks once.
        assert_eq!(round_texture_names(&bfe_content::json::parse("{}").unwrap()), ["lightglow"]);
        let named = bfe_content::json::parse(r#"{"rounds":{"ordc/a":{"LaserTexture":"lightglow"}}}"#).unwrap();
        assert_eq!(round_texture_names(&named), ["lightglow"]);
    }

    #[test]
    fn a_beams_glow_strip_is_two_and_a_half_times_its_core_width() {
        let width = |text: &str| beam_glow_width(&bfe_content::json::parse(text).unwrap());
        assert_eq!(width(r#"{"LaserWidth":0.4}"#), 1.);
        assert_eq!(width(r#"{"LaserWidth":3}"#), 7.5);
        // No core width, or one that cannot be drawn, leaves the beam without a second pass.
        assert_eq!(width(r#"{"Range":90}"#), 0.);
        assert_eq!(width(r#"{"LaserWidth":0}"#), 0.);
        assert_eq!(width(r#"{"LaserWidth":-1}"#), 0.);
    }

    #[test]
    fn muzzle_settings_follow_the_weapon_and_work_with_older_packs() {
        let pack = bfe_content::json::parse(r#"{
            "bindings":{"wand":{"MuzzleFlash":["SMALL_MUZZLE_FLASH"]},"rod":{"MuzzleFlash":["med_muzzle_flash"]}},
            "flashes":{"coil":{"MuzzleFlash":"large_muzzle_flash","FlashLength":0.47,
                "FlashRadius":2.75,"FlashColor":[16,36,8],"FlashLightRadius":9,"FlashLightDuration":0.21}}
        }"#).unwrap();
        let small = Muzzle::read(&pack, "wand");
        let medium = Muzzle::read(&pack, "rod");
        let explicit = Muzzle::read(&pack, "coil");
        assert!(0. < small.radius && small.radius < medium.radius);
        assert!(small.ticks < medium.ticks);
        assert_eq!(explicit.radius, 2.75);
        assert_eq!(explicit.ticks, 0.47 * 60.);
        assert_eq!(explicit.color, [16. / 255., 36. / 255., 8. / 255., 1.]);
        assert!(explicit.light.unwrap().0[0] > explicit.light.unwrap().0[2]);
        assert!(Muzzle::read(&pack, "absent").opacity(0.).is_none());
    }

    #[test]
    fn barrel_flash_uses_launch_offset_and_fades_between_ticks() {
        let bolt = Bolt {
            flight: 17.,
            position: [1., 2., 3.],
            offset: [0., -1., 5.],
            direction: [0., 3., 4.],
            ..Bolt::default()
        };
        let muzzle = Muzzle {
            radius: 2.5,
            ticks: 2.,
            color: [0.2, 0.4, 1., 0.5],
            light: None,
        };
        let flash = MeshFlash::at_barrel(&bolt, muzzle).unwrap();
        assert_eq!(flash.position, [1., 1., 8.]);
        assert_eq!(flash.direction, [0., 0.6, 0.8]);
        assert_eq!(flash.opacity(16.75), None);
        assert_eq!(flash.opacity(17.5), Some(0.5));
        assert_eq!(flash.opacity(18.5), Some(0.25));
        assert_eq!(flash.opacity(19.), None);
        assert!(MeshFlash::at_barrel(&Bolt::default(), muzzle).is_none());
    }
    #[test]
    fn a_fast_bolt_ends_where_it_struck() {
        let flight = FlightStyle {
            style: 2,
            speed: 9000.,
            range: None,
            life: 0.5,
            fade: 0.25,
            impact: [None; 6],
        };
        let shot = Shot {
            round: RoundClass(0),
            serial: 5,
            tick: 40.,
            position: [10., 4., -6.],
            direction: [0., 0., 3.],
            contact: None,
        };
        let mut rounds = Rounds {
            weapons: BTreeMap::new(),
            secondary: BTreeMap::new(),
            classes: vec![Weapon { flight, flash: Vec::new(), muzzle: Muzzle::default() }],
            recent: Vec::new(),
            recent_at: 0,
            active: Vec::new(),
            active_beams: Vec::new(),
            beam_draws: Vec::new(),
        };
        let launch = |rounds: &mut Rounds, serial| {
            let (bolt, end) = shot.flight(flight, 9).unwrap();
            rounds.active.push((bolt, end, RoundClass(0), serial));
        };
        for serial in 5..8 {
            launch(&mut rounds, serial);
        }
        // 225 m of path is a tick and a half: the flight ends inside its second tick.
        rounds.finish_live(5, 42., Some([10., 4., 219.]));
        // With no end point the whole second tick is flown, 75 m too far.
        rounds.finish_live(6, 42., None);
        // A point behind the muzzle leaves nothing to fly.
        rounds.finish_live(7, 42., Some([10., 4., -9.]));
        let flown = |at: usize| rounds.active[at].0.life * 9000.;
        assert_eq!([flown(0), flown(1), flown(2)], [225., 300., 0.]);
        // Each fades from the tick it ended on.
        assert!(rounds.active.iter().all(|r| r.1 == 57.));
    }

    #[test]
    fn recorded_contact_limits_visible_flight_on_the_display_clock() {
        let style = FlightStyle {
            style: 3,
            speed: 11.,
            range: None,
            life: 4.,
            fade: 0.25,
            impact: [None; 6],
        };
        let mut shot = Shot {
            round: RoundClass(0),
            serial: 7,
            tick: 29.,
            position: [8., 3., 1.],
            direction: [0., 0., -4.],
            contact: Some(Contact {
                tick: 74.,
                position: [8., 3., -7.25],
                normal: None,
                surface: Surface::Static,
            }),
        };
        let (bolt, end) = shot.flight(style, 17).unwrap();
        assert_eq!(
            (bolt.flight, bolt.life, bolt.direction, end),
            (29., 0.75, [0., 0., -1.], 89.)
        );
        shot.direction = [0.; 3];
        assert!(shot.flight(style, 17).is_none());
    }

    #[test]
    fn beam_shot_spans_authored_range_clips_on_contact_and_expires() {
        let style = FlightStyle {
            style: 4,
            speed: 0.,
            range: Some(175.),
            life: 0.1,
            fade: 0.15,
            impact: [None; 6],
        };
        let shot = Shot {
            round: RoundClass(0),
            serial: 19,
            tick: 100.,
            position: [2., 5., -3.],
            direction: [0., 0., 5.],
            contact: None,
        };
        let mut rounds = Rounds {
            weapons: BTreeMap::new(),
            secondary: BTreeMap::new(),
            classes: vec![Weapon { flight: style, flash: Vec::new(), muzzle: Muzzle::default() }],
            recent: Vec::new(),
            recent_at: 0,
            active: Vec::new(),
            active_beams: vec![shot.beam(style, 23).unwrap()],
            beam_draws: Vec::new(),
        };
        let draws = rounds.take_beams(100.);
        assert_eq!(draws.len(), 1);
        assert_eq!(draws[0].end, [2., 5., 172.]);
        assert!((draws[0].life - 0.1).abs() < 1e-9);
        rounds.return_beams(draws);
        // Ending without a contact leaves an open-space beam at its authored range.
        rounds.finish_live(19, 101., None);
        let draws = rounds.take_beams(101.);
        assert_eq!(draws[0].end, [2., 5., 172.]);
        rounds.return_beams(draws);
        // A hit past the beam's own range keeps the authored span; a closer hit trims it.
        rounds.finish_live(19, 102., Some([2., 5., 300.]));
        let draws = rounds.take_beams(102.);
        assert_eq!(draws[0].end, [2., 5., 172.]);
        rounds.return_beams(draws);
        rounds.finish_live(19, 103., Some([2., 5., 57.]));
        let draws = rounds.take_beams(103.);
        assert_eq!(draws[0].end, [2., 5., 57.]);
        assert!((draws[0].life - 0.05).abs() < 1e-9);
        rounds.return_beams(draws);
        // Moving an active beam preserves its current clipped span along the new pose.
        assert!(rounds.move_beam([19, 0], [10., 5., 10.], [1., 0., 0.]));
        let draws = rounds.take_beams(104.);
        assert_eq!(draws[0].position, [10., 5., 10.]);
        assert_eq!(draws[0].end, [70., 5., 10.]);
        rounds.return_beams(draws);
        // Once past LifeSpan the beam drops out and anchors stop tracking it.
        let draws = rounds.take_beams(107.);
        assert!(draws.is_empty());
        assert!(!rounds.move_beam([19, 0], [10., 5., 10.], [1., 0., 0.]));
    }
}
