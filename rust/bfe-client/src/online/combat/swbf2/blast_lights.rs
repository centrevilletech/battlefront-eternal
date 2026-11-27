//! The light a Battlefront II explosion class names (`LightColor`,
//! `LightRadius`, `LightDuration`), flashed where and when the battle's
//! explosion goes off: a thermal detonator's warm flash, a detpack's green one.
//! The battle's explosion publications are read a second time here; the
//! shared reader that starts their effects is unchanged.
use super::{Receiver, Value};
use bfe_presentation::network_layout::{net_kind as wk, net_record as wire};

/// Explosion publications remembered, so a retransmission flashes nothing new.
const SEEN: usize = 256;
/// Explosions due at once beyond which the oldest pending one is dropped.
const DUE: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Light {
    /// Colour on 0 to 1 with its alpha folded in.
    rgb: [f64; 3],
    radius: f64,
    duration: f64,
}

impl Light {
    /// Hand this light, at `position`, to the scene's flash lights.
    pub(super) fn flash(self, position: [f64; 3], flash: &mut impl FnMut([f64; 3], [f64; 3], f64, f64)) {
        flash(position, self.rgb, self.radius, self.duration);
    }
}

fn number(def: &Value, key: &str) -> Option<f64> {
    let v = def.get(key)?;
    v.as_f64()
        .or_else(|| v.as_str()?.trim().parse().ok())
        .filter(|n: &f64| n.is_finite())
}

/// The light an explosion class names, when it names one that can be seen.
pub(super) fn light_of(def: &Value) -> Option<Light> {
    let radius = number(def, "LightRadius").filter(|r| *r > 0.)?;
    let duration = number(def, "LightDuration").filter(|d| *d > 0.)?;
    let mut c = [255.; 4];
    match def.get("LightColor")? {
        Value::Array(list) => {
            for (i, v) in list.iter().take(4).enumerate() {
                c[i] = v.as_f64()?;
            }
        }
        Value::String(line) => {
            for (i, v) in line.split_whitespace().take(4).enumerate() {
                c[i] = v.parse().ok()?;
            }
        }
        _ => return None,
    }
    let rgb: [f64; 3] = std::array::from_fn(|i| c[i].clamp(0., 255.) / 255. * (c[3].clamp(0., 255.) / 255.));
    rgb.iter().any(|v| *v > 0.).then_some(Light { rgb, radius, duration })
}

/// The light of each named explosion class, from the map's and the sides'
/// class files (`expc`, following `base`, the last value of a key winning), for
/// explosions that are no combat record of their own (a blade's landing blast).
pub(super) fn class_lights(battle: &bfe_content_native::BattleContent, names: &[&str]) -> Vec<(String, Light)> {
    if names.is_empty() {
        return Vec::new();
    }
    let mut tables = Vec::new();
    let map = battle.content.battle.world.get("name").and_then(Value::as_str).unwrap_or("");
    let sides = battle.content.battle.sides.iter().map(|s| s.name.as_str()).chain(["all", "cis", "imp", "rep"]);
    let files = std::iter::once(format!("maps/{map}/classes.json")).filter(|_| !map.is_empty()).chain(sides.map(|s| format!("sides/{s}/classes.json")));
    for file in files {
        if let Some(table) = battle.textures.read(&file).ok().and_then(|raw| bfe_content::json::parse_bytes(&raw).ok()) {
            tables.push(table);
        }
    }
    let entry = |name: &str| {
        tables.iter().find_map(|t| {
            let section = t.get("expc")?.as_object()?;
            section.get(name).or_else(|| section.entries().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v)).cloned()
        })
    };
    let mut out = Vec::new();
    for &name in names {
        let mut chain = Vec::new();
        let mut cursor = Some(name.to_owned());
        while let Some(class) = cursor.take().filter(|_| chain.len() < 16) {
            let Some(e) = entry(&class) else { break };
            cursor = e.get("base").and_then(Value::as_str).filter(|b| !b.eq_ignore_ascii_case(&class)).map(str::to_owned);
            chain.push(e);
        }
        let mut fields = serde_json::Map::new();
        for e in chain.iter().rev() {
            for pair in e.get("properties").and_then(Value::as_array).unwrap_or(&[]) {
                if let (Some(k), Some(v)) = (pair.at(0).and_then(Value::as_str), pair.at(1).and_then(Value::as_str)) {
                    if ["LightColor", "LightRadius", "LightDuration"].iter().any(|f| f.eq_ignore_ascii_case(k)) {
                        fields.insert(k.to_owned(), v.into());
                    }
                }
            }
        }
        let record = bfe_content::json::parse(&serde_json::Value::Object(fields).to_string()).ok();
        if let Some(light) = record.as_ref().and_then(light_of) {
            out.push((name.to_owned(), light));
        }
    }
    out
}

pub(super) struct BlastLights {
    /// By wire definition id (registry index + 1).
    by_definition: Vec<Option<Light>>,
    seen: Vec<(u32, u32)>,
    seen_at: usize,
    due: Vec<(f64, [f64; 3], Light)>,
}

impl BlastLights {
    /// The lights of the explosion classes in the battle's combat registry.
    pub(super) fn new(registry: &[(String, Value)]) -> Self {
        let mut by_definition = vec![None; registry.len() + 1];
        for (i, (_, def)) in registry.iter().enumerate() {
            if def.get("base").and_then(Value::as_str) == Some("explosion") {
                by_definition[i + 1] = light_of(def);
            }
        }
        Self { by_definition, seen: vec![(0, 0); SEEN], seen_at: 0, due: Vec::with_capacity(DUE) }
    }
    pub(super) fn is_empty(&self) -> bool {
        self.by_definition.iter().all(Option::is_none)
    }
    pub(super) fn clear(&mut self) {
        self.seen.fill((0, 0));
        self.due.clear();
    }
    /// One published explosion: its id and generation, wire definition, tick and point.
    pub(super) fn observe(&mut self, id: u32, generation: u32, definition: usize, tick: f64, position: [f64; 3]) {
        let Some(light) = self.by_definition.get(definition).copied().flatten() else { return };
        if self.seen.contains(&(id, generation)) {
            return;
        }
        self.seen[self.seen_at] = (id, generation);
        self.seen_at = (self.seen_at + 1) % SEEN;
        if self.due.len() == DUE {
            self.due.remove(0);
        }
        self.due.push((tick, position, light));
    }
    /// This frame's explosion publications (combat schema 5).
    pub(super) fn read(&mut self, receiver: &Receiver) {
        if self.is_empty() {
            return;
        }
        for field in receiver.publications.iter() {
            let row = &field.row;
            if row[wire::KIND] != wk::COMBAT as f64 || row[wire::SCHEMA] as u32 != 5 || field.length != 33 {
                continue;
            }
            let bytes = &field.bytes[..field.length];
            let definition = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
            let position: [f64; 3] = std::array::from_fn(|i| {
                let at = 8 + i * 8;
                f64::from_le_bytes(bytes[at..at + 8].try_into().unwrap_or([0; 8]))
            });
            if position.iter().all(|v| v.is_finite()) {
                self.observe(row[wire::ID] as u32, row[wire::GENERATION] as u32, definition, row[wire::TICK], position);
            }
        }
    }
    /// Flash every light whose explosion is due on the display clock `tick`.
    pub(super) fn flash_due(&mut self, tick: f64, flash: &mut impl FnMut([f64; 3], [f64; 3], f64, f64)) {
        self.due.retain(|(at, position, light)| {
            if *at > tick {
                return true;
            }
            flash(*position, light.rgb, light.radius, light.duration);
            false
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(text: &str) -> Value {
        bfe_content::json::parse(text).unwrap()
    }

    #[test]
    fn an_explosion_flashes_its_light_once_when_it_is_due() {
        let registry = vec![
            ("a".to_string(), def(r#"{"base": "explosion", "LightColor": "200 100 50 128", "LightRadius": 6, "LightDuration": "0.5"}"#)),
            ("b".to_string(), def(r#"{"base": "explosion", "LightColor": "255 255 255", "LightRadius": "0.0", "LightDuration": "0.0"}"#)),
            ("c".to_string(), def(r#"{"base": "bolt", "LightColor": "12 34 56 78", "LightRadius": 3, "LightDuration": 1}"#)),
        ];
        let mut lights = BlastLights::new(&registry);
        assert!(!lights.is_empty());
        assert_eq!(lights.by_definition[2], None, "a dark explosion lights nothing");
        assert_eq!(lights.by_definition[3], None, "only explosion classes");
        lights.observe(11, 1, 1, 40., [1., 2., 3.]);
        lights.observe(11, 1, 1, 40., [1., 2., 3.]);
        lights.observe(12, 1, 2, 40., [0.; 3]);
        let mut flashed = Vec::new();
        lights.flash_due(39., &mut |p, c, r, d| flashed.push((p, c, r, d)));
        assert!(flashed.is_empty(), "not before its tick");
        lights.flash_due(40.5, &mut |p, c, r, d| flashed.push((p, c, r, d)));
        assert_eq!(flashed.len(), 1, "once, however often it was published");
        let (p, c, r, d) = flashed[0];
        assert_eq!((p, r, d), ([1., 2., 3.], 6., 0.5));
        let a = 128. / 255.;
        assert!((c[0] - 200. / 255. * a).abs() < 1e-12 && (c[2] - 50. / 255. * a).abs() < 1e-12);
        assert!(BlastLights::new(&registry[2..]).is_empty());
    }

    #[test]
    fn explosions_are_read_from_the_battles_publications() {
        let registry = vec![
            ("a".to_string(), def(r#"{"base": "explosion", "LightColor": "250 120 30 255", "LightRadius": 5, "LightDuration": 1.5}"#)),
        ];
        let mut lights = BlastLights::new(&registry);
        let mut receiver = Receiver::new(serde_json::Value::Null);
        let explosion = |definition: u32, at: [f64; 3]| {
            let mut bytes = vec![0u8; 33];
            bytes[4..8].copy_from_slice(&definition.to_le_bytes());
            for (i, v) in at.iter().enumerate() {
                bytes[8 + i * 8..16 + i * 8].copy_from_slice(&v.to_le_bytes());
            }
            bytes
        };
        let mut publish = |receiver: &mut Receiver, id: f64, schema: f64, bytes: &[u8]| {
            let mut row = [0.; wire::STRIDE];
            row[wire::KIND] = wk::COMBAT as f64;
            row[wire::SCHEMA] = schema;
            row[wire::ID] = id;
            row[wire::GENERATION] = 1.;
            row[wire::TICK] = 50.;
            receiver.publications.push(&row, bytes);
        };
        publish(&mut receiver, 5., 5., &explosion(1, [2., 3., 4.]));
        publish(&mut receiver, 6., 6., &explosion(1, [9., 9., 9.]));
        publish(&mut receiver, 7., 5., &explosion(1, [9., 9., 9.])[..32]);
        publish(&mut receiver, 8., 5., &explosion(7, [9., 9., 9.]));
        lights.read(&receiver);
        let mut flashed = Vec::new();
        lights.flash_due(50., &mut |p, _, r, d| flashed.push((p, r, d)));
        assert_eq!(flashed, vec![([2., 3., 4.], 5., 1.5)], "only a whole explosion record of a lit class");
    }
}
