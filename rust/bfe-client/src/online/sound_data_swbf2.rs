//! 2005 (`GameFamily::Swbf2`) sound manifest, bank, stream, combo, and region loader.
//! Kept beside `sound_data.rs` so the 2004 preparation path stays completely unchanged.

use super::sound_data::{hash, key_hash, Prepared, SpaceBox};
use audio::output::Output;
use audio_control::acoustics::Room;
use audio_control::engine::NONE;
use audio_control::layout::*;
use audio_control::runtime::Runtime;
use audio_control::swbf2::ambience::{RegionShape, SpaceAttenuation, SpacePreset, SpaceRegion, TriggerRegion};
use audio_control::swbf2::vehicles::{BoostCue, BuildingEmitter, ProximityBinding};
use audio_control::swbf2::{
    zero_hash, ComboSoundSet, ComboSoundTrigger, ComboStateSounds, SoundParam, Swbf2Class, VoRule,
};
use bfe_content::{Object, Value};
use bfe_content_native::BattleContent;
use bfe_family::GameFamily;
use std::collections::BTreeMap;
use std::rc::Rc;

fn field<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.get(key).filter(|v| !v.is_null())
}
fn number(v: Option<&Value>) -> f64 {
    v.map_or(f64::NAN, |v| bfe_content::js::to_number(Some(v)))
}
fn text(v: Option<&Value>) -> &str {
    v.and_then(Value::as_str).unwrap_or("")
}
fn first(v: Option<&Value>) -> Option<&Value> {
    v.and_then(|v| if let Some(a) = v.as_array() { a.first() } else { Some(v) })
}
fn is_prefixed_hex(s: &str) -> bool {
    s.len() == 10
        && s.starts_with("0x")
        && s[2..].bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn key(s: &str) -> String {
    if is_prefixed_hex(s) {
        s.into()
    } else {
        format!("0x{:08x}", hash(s))
    }
}
fn hash_hex(s: &str) -> u32 {
    if is_prefixed_hex(s) {
        u32::from_str_radix(&s[2..], 16).unwrap_or_else(|_| zero_hash(s))
    } else {
        zero_hash(s)
    }
}
fn parse_raw_hex_or_hash(s: &str) -> u32 {
    if is_prefixed_hex(s) {
        u32::from_str_radix(&s[2..], 16).unwrap_or_else(|_| zero_hash(s))
    } else if is_hex_key(s) {
        u32::from_str_radix(s, 16).unwrap_or_else(|_| zero_hash(s))
    } else {
        zero_hash(s)
    }
}

struct SoundObject {
    data: Value,
    own: Value,
}

struct Data {
    manifest: Value,
    objects: Vec<SoundObject>,
    kinds: BTreeMap<String, Vec<(String, usize)>>,
}

impl Data {
    fn new(manifest: Value) -> Result<Self, String> {
        let mut data = Self { manifest, objects: vec![], kinds: BTreeMap::new() };
        for original in data.manifest.get("objects").and_then(Value::as_array).unwrap_or(&[]) {
            let kind = text(original.get("type"));
            let mut resolved = original.clone();
            if kind == "SoundProperties" || kind == "SoundStreamProperties" {
                let parent = data
                    .kinds
                    .get(kind)
                    .and_then(|table| table.iter().find(|(name, _)| name == text(original.get("Inherit"))))
                    .map(|(_, i)| &data.objects[*i]);
                if let Some(parent) = parent {
                    let mut out = Object::new();
                    let fields = original.as_object().ok_or("invalid sound object")?.entries().collect::<Vec<_>>();
                    let at = fields.iter().position(|(k, _)| *k == "Inherit").unwrap_or(fields.len());
                    for (k, v) in &fields[..at] {
                        if *k == "SampleList" && parent.own.get("SampleList").is_none() {
                            continue;
                        }
                        out.set(Rc::from(*k), (*v).clone());
                    }
                    for (k, v) in parent.data.as_object().ok_or("invalid inherited sound")?.entries() {
                        if !["Name", "Group", "type", "SampleList", "SegmentList", "Inherit"].contains(&k) {
                            out.set(Rc::from(k), v.clone());
                        }
                    }
                    for k in ["SampleList", "SegmentList"] {
                        if let Some(v) = parent.own.get(k) {
                            out.set(Rc::from(k), v.clone());
                        }
                    }
                    for (k, v) in &fields[at..] {
                        out.set(Rc::from(*k), (*v).clone());
                    }
                    resolved = Value::Object(Rc::new(out));
                }
            }
            let id = data.objects.len();
            data.objects.push(SoundObject { data: resolved, own: original.clone() });
            let names = original.get("Name");
            let names = names
                .and_then(Value::as_array)
                .map_or_else(|| names.into_iter().collect::<Vec<_>>(), |a| a.iter().collect());
            let table = data.kinds.entry(kind.into()).or_default();
            for name in names {
                let name = text(Some(name));
                if name.is_empty() {
                    continue;
                }
                if let Some((_, old)) = table.iter_mut().find(|(n, _)| n == name) {
                    *old = id;
                } else {
                    table.push((name.into(), id));
                }
            }
        }
        Ok(data)
    }

    fn object(&self, kind: &str, name: &str) -> Option<&Value> {
        let name = key(name);
        self.table(kind).iter().find(|(n, _)| *n == name).map(|(_, i)| &self.objects[*i].data)
    }

    fn table(&self, kind: &str) -> &[(String, usize)] {
        self.kinds.get(kind).map_or(&[], Vec::as_slice)
    }
}

fn import(runtime: &mut Runtime, kind: usize, rows: &[f64]) -> Result<(), String> {
    if rows.len() > runtime.input.len() {
        return Err("sound resource bank exceeds reservation".into());
    }
    runtime.input[..rows.len()].copy_from_slice(rows);
    runtime.import(kind, rows.len());
    Ok(())
}

fn sound_id(names: &BTreeMap<String, usize>, v: Option<&Value>) -> f64 {
    let name = text(first(v)).split_whitespace().next().unwrap_or("");
    if name.is_empty() {
        -1.
    } else {
        names.get(&key(name)).map_or(-1., |id| *id as f64)
    }
}

fn sound_index(names: &BTreeMap<String, usize>, v: Option<&Value>) -> usize {
    let id = sound_id(names, v);
    if id < 0.0 { NONE } else { id as usize }
}

fn value_list(v: Option<&Value>) -> Vec<&Value> {
    match v {
        Some(v) => v
            .as_array()
            .map_or_else(|| vec![v], |a| a.iter().collect()),
        None => Vec::new(),
    }
}

fn lookup_prop<'a>(manifest_cls: Option<&'a Value>, odf_cls: Option<&'a Value>, k: &str) -> Option<&'a Value> {
    let find_in = |obj: &'a Value, key_name: &str| -> Option<&'a Value> {
        if let Some(v) = obj.get(key_name).filter(|v| v.truthy()) {
            return Some(v);
        }
        // Check case-insensitive match for ODF properties stored in canonical or mixed case.
        if let Some(map) = obj.as_object() {
            for (mk, mv) in map.entries() {
                if mk.eq_ignore_ascii_case(key_name) && mv.truthy() {
                    return Some(mv);
                }
            }
        }
        None
    };
    if let Some(v) = manifest_cls.and_then(|o| find_in(o, k)).or_else(|| odf_cls.and_then(|o| find_in(o, k))) {
        return Some(v);
    }
    if !is_hex_key(k) {
        let hex = format!("{:08x}", zero_hash(k));
        return manifest_cls
            .and_then(|o| find_in(o, &hex))
            .or_else(|| odf_cls.and_then(|o| find_in(o, &hex)));
    }
    None
}

/// For repeated multi-value class keys (`BoostSound`, `VOSound`, `AmbientSound`),
/// prefers whichever source preserves more authored entries.
fn lookup_prop_multi<'a>(manifest_cls: Option<&'a Value>, odf_cls: Option<&'a Value>, k: &str) -> Option<&'a Value> {
    let m = manifest_cls.and_then(|obj| lookup_prop(Some(obj), None, k));
    let o = odf_cls.and_then(|obj| lookup_prop(None, Some(obj), k));
    match (m, o) {
        (Some(mv), Some(ov)) => {
            let m_len = mv.as_array().map_or(1, |a| a.len());
            let o_len = ov.as_array().map_or(1, |a| a.len());
            if o_len > m_len { Some(ov) } else { Some(mv) }
        }
        (Some(mv), None) => Some(mv),
        (None, Some(ov)) => Some(ov),
        (None, None) => None,
    }
}

fn lookup_prop_alias<'a>(manifest_cls: Option<&'a Value>, odf_cls: Option<&'a Value>, k: &str) -> Option<&'a Value> {
    if let Some(v) = lookup_prop(manifest_cls, odf_cls, k) {
        return Some(v);
    }
    let aliases: &[&str] = match k {
        "TurningOffSound" => &["7d2c2fa2"],
        "TakeoffSound" => &["WindUpSound", "205b8d59"],
        "LandSound" => &["WindDownSound", "fc64a1f2"],
        "HydraulicSound" => &["069134e5"],
        "FootstepSound0" => &["97157af6"],
        "FootstepSound1" => &["98157c89"],
        "FootstepSound2" => &["951577d0"],
        "FootstepSound3" => &["96157963"],
        "FootstepSound4" => &["9b158142"],
        "FootstepSound5" => &["9c1582d5"],
        _ => &[],
    };
    aliases.iter().find_map(|a| lookup_prop(manifest_cls, odf_cls, a))
}

fn parse_boost_cues(names: &BTreeMap<String, usize>, v: Option<&Value>) -> Vec<BoostCue> {
    let mut cues = Vec::new();
    for item in value_list(v) {
        let mut parts = text(Some(item)).split_whitespace();
        let Some(snd) = parts.next() else { continue };
        let Some(&id) = names.get(&key(snd)) else { continue };
        let threshold = parts.next().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.5);
        let dir = parts.next().and_then(|s| s.parse::<i32>().ok()).unwrap_or(1);
        cues.push(BoostCue { sound: id, threshold, up: dir != 0 });
    }
    cues
}

fn parse_vo_rules(names: &BTreeMap<String, usize>, v: Option<&Value>) -> Vec<VoRule> {
    let mut rules = Vec::new();
    for item in value_list(v) {
        let raw = text(Some(item));
        let mut tokens = raw
            .split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '+'))
            .filter(|s| !s.is_empty());
        let Some(snd) = tokens.next() else { continue };
        let Some(&id) = names.get(&key(snd)) else { continue };
        let Some(event) = tokens.next() else { continue };
        let conds: Vec<u32> = tokens.map(hash).collect();
        match VoRule::try_new(id, hash(event), &conds) {
            Some(rule) => rules.push(rule),
            None => {
                eprintln!(
                    "audio: skipping VOSound rule with more than {} distinct conditions: {raw}",
                    audio_control::swbf2::MAX_VO_CONDITIONS
                );
            }
        }
    }
    rules
}

fn is_hex_key(k: &str) -> bool {
    k.len() == 8 && k.bytes().all(|b| b.is_ascii_hexdigit())
}

fn set_property(properties: &mut Vec<(u32, f64)>, key: u32, sound: f64) {
    match properties.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1 = sound,
        None => properties.push((key, sound)),
    }
}

fn track(
    data: &Data,
    names: &BTreeMap<String, usize>,
    tracks: &mut BTreeMap<String, usize>,
    runtime: &mut Runtime,
    name: Option<&Value>,
) -> Result<f64, String> {
    let name = text(name);
    if name.is_empty() {
        return Ok(-1.);
    }
    let key_name = name.to_ascii_lowercase();
    if let Some(id) = tracks.get(&key_name) {
        return Ok(*id as f64);
    }
    let id = tracks.len();
    tracks.insert(key_name, id);
    let object = data.object("Music", name);
    let mut row = [f64::NAN; track::STRIDE];
    row[track::STREAM] = if let Some(value) = object.and_then(|o| o.get("SoundStream").or_else(|| o.get("soundstream"))) {
        sound_id(names, Some(value))
    } else {
        sound_id(names, Some(&Value::str(name)))
    };
    let max_play = object
        .and_then(|o| field(o, "MaxPlaybackTime").or_else(|| field(o, "319a9661")))
        .map_or(f64::INFINITY, |v| number(Some(v)));
    for (at, k, alias, default) in [
        (track::MIN_PLAY, "MinPlaybackTime", "6813be83", 0.),
        (track::MIN_IDLE, "MinInactiveTime", "35724887", 0.),
        (track::PRIORITY, "Priority", "", 0.),
        (track::FADE_IN, "FadeInTime", "", 1.),
        (track::FADE_OUT, "FadeOutTime", "", 1.),
    ] {
        row[at] = object
            .and_then(|o| field(o, k).or_else(|| field(o, alias)))
            .map_or(default, |v| number(Some(v)));
    }
    if max_play.is_finite() && row[track::MIN_PLAY] > max_play {
        row[track::MIN_PLAY] = max_play;
    }
    import(runtime, import::TRACK, &row)?;
    if max_play.is_finite() {
        if let Some(swbf2) = runtime.phase.swbf2_mut() {
            swbf2.director.set_track_max_play(id, max_play);
        }
    }
    Ok(id as f64)
}

fn parse_shape(s: &str) -> RegionShape {
    if s.eq_ignore_ascii_case("cylinder") {
        RegionShape::Cylinder
    } else if s.eq_ignore_ascii_case("sphere") {
        RegionShape::Sphere
    } else {
        RegionShape::Box
    }
}

fn parse_sound_param(s: &str) -> Option<SoundParam> {
    if s.eq_ignore_ascii_case("Gain") {
        Some(SoundParam::Gain)
    } else if s.eq_ignore_ascii_case("Pitch") {
        Some(SoundParam::Pitch)
    } else if s.eq_ignore_ascii_case("MinDistance") {
        Some(SoundParam::MinDistance)
    } else if s.eq_ignore_ascii_case("MaxDistance") {
        Some(SoundParam::MaxDistance)
    } else if s.eq_ignore_ascii_case("MuteDistance") {
        Some(SoundParam::MuteDistance)
    } else {
        None
    }
}

fn append_repeated_pair(pairs: &mut BTreeMap<String, Vec<Value>>, k: &str, v: Value) {
    if k.is_empty() || v.is_null() {
        return;
    }
    let entry = pairs.entry(k.to_ascii_lowercase()).or_default();
    if let Some(arr) = v.as_array() {
        for item in arr {
            if !item.is_null() {
                entry.push(item.clone());
            }
        }
    } else {
        entry.push(v);
    }
}

/// Builds a merged property object from an ODF class record, preserving repeated
/// `[key, value]` entries from ordered `properties` arrays as JSON arrays.
fn odf_record_to_object(val: &Value) -> Option<Value> {
    let mut pairs: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut has_any = false;
    if let Some(props_arr) = val.get("properties").and_then(Value::as_array) {
        for pair in props_arr {
            let k = text(pair.at(0));
            if let Some(v) = pair.at(1) {
                if !k.is_empty() && !v.is_null() {
                    append_repeated_pair(&mut pairs, k, v.clone());
                    has_any = true;
                }
            }
        }
    }
    if let Some(props_obj) = val.get("properties").and_then(Value::as_object) {
        for (k, v) in props_obj.entries() {
            if !pairs.contains_key(&k.to_ascii_lowercase()) {
                append_repeated_pair(&mut pairs, k, v.clone());
                has_any = true;
            }
        }
    }
    if let Some(obj) = val.as_object() {
        for (k, v) in obj.entries() {
            if k == "properties" || k == "settings" {
                continue;
            }
            if !pairs.contains_key(&k.to_ascii_lowercase()) {
                append_repeated_pair(&mut pairs, k, v.clone());
                has_any = true;
            }
        }
    }
    if !has_any {
        return None;
    }
    let mut out = Object::new();
    for (k, mut vals) in pairs {
        let field_val = if vals.len() == 1 {
            vals.pop().unwrap()
        } else {
            Value::Array(Rc::new(vals))
        };
        out.set(Rc::from(k), field_val);
    }
    Some(Value::Object(Rc::new(out)))
}

fn insert_odf_class(map: &mut BTreeMap<String, Value>, name: &str, val: &Value) {
    if name.is_empty() {
        return;
    }
    let Some(built) = odf_record_to_object(val) else {
        return;
    };
    let key_lower = name.to_ascii_lowercase();
    if let Some(existing) = map.get(&key_lower) {
        // Merge without losing multi-valued arrays from either source.
        let mut merged = Object::new();
        if let Some(obj) = existing.as_object() {
            for (k, v) in obj.entries() {
                merged.set(Rc::from(k), v.clone());
            }
        }
        if let Some(obj) = built.as_object() {
            for (k, v) in obj.entries() {
                let replace = match merged.get(k) {
                    None => true,
                    Some(prev) => {
                        let new_len = v.as_array().map_or(1, |a| a.len());
                        let old_len = prev.as_array().map_or(1, |a| a.len());
                        new_len > old_len
                    }
                };
                if replace {
                    merged.set(Rc::from(k), v.clone());
                }
            }
        }
        map.insert(key_lower, Value::Object(Rc::new(merged)));
    } else {
        map.insert(key_lower, built);
    }
}

fn strip_class_section_prefix(raw: &str) -> &str {
    let token = raw.split_whitespace().next().unwrap_or("");
    token
        .strip_prefix("wpnc/")
        .or_else(|| token.strip_prefix("ordc/"))
        .or_else(|| token.strip_prefix("entc/"))
        .or_else(|| token.strip_prefix("expc/"))
        .unwrap_or(token)
}

fn is_multi_prop(lower_key: &str) -> bool {
    matches!(lower_key, "boostsound" | "vosound" | "ambientsound")
}

/// Flattens a class and its `base` inheritance chain across `classes.json` tables
/// (root first so child properties override parent properties, preserving repeated
/// multi-valued keys such as `BoostSound`, `VOSound`, and `AmbientSound`).
fn flatten_section_class(
    tables: &[Value],
    section: &str,
    name: &str,
) -> Option<Value> {
    let find_entry = |n: &str| -> Option<&Value> {
        for table in tables {
            let Some(sec) = table.get(section).and_then(Value::as_object) else {
                continue;
            };
            if let Some(exact) = sec.get(n) {
                return Some(exact);
            }
            for (k, v) in sec.entries() {
                if k.eq_ignore_ascii_case(n) {
                    return Some(v);
                }
            }
        }
        None
    };

    let mut chain: Vec<&Value> = Vec::new();
    let mut visited: Vec<String> = Vec::new();
    let mut cursor = Some(name.to_string());
    while let Some(cur) = cursor.take() {
        if chain.len() >= 16 {
            break;
        }
        let low = cur.to_ascii_lowercase();
        if visited.iter().any(|v| v == &low) {
            break;
        }
        visited.push(low);
        let Some(entry) = find_entry(&cur) else {
            break;
        };
        cursor = entry
            .get("base")
            .and_then(Value::as_str)
            .filter(|b| !b.is_empty())
            .map(str::to_string);
        chain.push(entry);
    }
    if chain.is_empty() {
        return None;
    }

    let mut pairs: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for entry in chain.into_iter().rev() {
        let mut child_seen: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        if let Some(props_arr) = entry.get("properties").and_then(Value::as_array) {
            for pair in props_arr {
                let k = text(pair.at(0));
                if let Some(v) = pair.at(1) {
                    if !k.is_empty() && !v.is_null() {
                        append_repeated_pair(&mut child_seen, k, v.clone());
                    }
                }
            }
        }
        if let Some(props_obj) = entry.get("properties").and_then(Value::as_object) {
            for (k, v) in props_obj.entries() {
                if !k.is_empty() && !v.is_null() {
                    append_repeated_pair(&mut child_seen, k, v.clone());
                }
            }
        }
        for (k, vals) in child_seen {
            if is_multi_prop(&k) {
                pairs.entry(k).or_default().extend(vals);
            } else {
                pairs.insert(k, vals);
            }
        }
    }
    if pairs.is_empty() {
        return None;
    }
    let mut out = Object::new();
    for (k, mut vals) in pairs {
        let field_val = if vals.len() == 1 {
            vals.pop().unwrap()
        } else {
            Value::Array(Rc::new(vals))
        };
        out.set(Rc::from(k), field_val);
    }
    Some(Value::Object(Rc::new(out)))
}

/// Collects all resolved ODF class property maps available on a 2005 `BattleContent`.
fn collect_odf_classes(content: &BattleContent) -> BTreeMap<String, Value> {
    let mut map = BTreeMap::new();
    if let Ok(defs) = content.defs() {
        for c in &defs.classes {
            let cname = defs.names.text(c.name);
            insert_odf_class(&mut map, cname, &c.record);
            if let Some(settings) = c.record.get("settings") {
                insert_odf_class(&mut map, cname, settings);
            }
        }
        for w in &defs.weapons {
            let wname = w.class.map_or_else(|| text(w.record.get("class")), |n| defs.names.text(n));
            insert_odf_class(&mut map, wname, &w.record);
        }
        for v in &defs.vehicles {
            let vname = defs.names.text(v.name);
            insert_odf_class(&mut map, vname, &v.record);
            if let Some(settings) = v.record.get("settings") {
                insert_odf_class(&mut map, vname, settings);
            }
            for seat in &v.seats {
                for w in &seat.weapons {
                    if let Some(def) = w.get("def") {
                        let wname = text(def.get("class"));
                        insert_odf_class(&mut map, wname, def);
                    }
                }
            }
        }
        for p in &defs.props {
            let pname = p.class.map_or_else(|| text(p.record.get("class")), |n| defs.names.text(n));
            insert_odf_class(&mut map, pname, &p.record);
        }
    }
    // Also read raw side units/weapons tables and map classes for any non-simulable classes
    // (such as starfighters, turrets, or ship props) that carry sound properties.
    for side in &content.content.battle.sides {
        for obj in [side.units.as_object(), side.weapons.as_object(), side.items.as_object()]
            .into_iter()
            .flatten()
        {
            for (name, val) in obj.entries() {
                insert_odf_class(&mut map, name, val);
                if let Some(weapons) = val.get("weapons").and_then(Value::as_array) {
                    for w in weapons {
                        let wname = text(w.get("class"));
                        if !wname.is_empty() {
                            insert_odf_class(&mut map, wname, w);
                        }
                    }
                }
            }
        }
    }

    // Load sectioned `maps/<map>/classes.json` and `sides/<side>/classes.json`
    // (`entc`, `wpnc`, `ordc`, `expc`) with `base` inheritance flattened so space
    // weapons, turrets, ordnance, explosions, and hero powers resolve every
    // inherited sound property and class link.
    let mut class_tables: Vec<Value> = Vec::new();
    let map_name = content.content.battle.world.get("name").and_then(Value::as_str).unwrap_or("");
    if !map_name.is_empty() {
        if let Ok(raw) = content.textures.read(&format!("maps/{map_name}/classes.json")) {
            if let Ok(parsed) = bfe_content::json::parse_bytes(&raw) {
                class_tables.push(parsed);
            }
        }
    }
    let mut loaded_sides: Vec<String> = Vec::new();
    for side_name in content
        .content
        .battle
        .sides
        .iter()
        .map(|s| s.name.as_str())
        .chain(["all", "cis", "imp", "rep"])
    {
        if side_name.is_empty() || loaded_sides.iter().any(|s| s.eq_ignore_ascii_case(side_name)) {
            continue;
        }
        loaded_sides.push(side_name.to_string());
        if let Ok(raw) = content.textures.read(&format!("sides/{side_name}/classes.json")) {
            if let Ok(parsed) = bfe_content::json::parse_bytes(&raw) {
                class_tables.push(parsed);
            }
        }
    }
    for section in ["entc", "wpnc", "ordc", "expc"] {
        let mut names_in_section: Vec<String> = Vec::new();
        for table in &class_tables {
            if let Some(sec) = table.get(section).and_then(Value::as_object) {
                for (cname, _) in sec.entries() {
                    if !names_in_section.iter().any(|n| n.eq_ignore_ascii_case(cname)) {
                        names_in_section.push(cname.to_string());
                    }
                }
            }
        }
        for cname in names_in_section {
            if let Some(flat) = flatten_section_class(&class_tables, section, &cname) {
                insert_odf_class(&mut map, &cname, &flat);
            }
        }
    }

    if let Some(classes) = content.content.battle.map_data.get("classes").and_then(Value::as_object) {
        for (name, val) in classes.entries() {
            if !matches!(name, "entc" | "wpnc" | "ordc" | "expc") {
                insert_odf_class(&mut map, name, val);
            }
        }
    }
    map
}

/// Extracts all `ParameterGraph` entries from a `SoundParameterized` `Layer.children` node.
/// Prefers the plural `ParameterGraphs` list (including an empty list) when present so the
/// legacy singular `ParameterGraph` key is never imported twice.
fn extract_param_graphs(child: &Value) -> Vec<&Value> {
    if let Some(arr) = child.get("ParameterGraphs").and_then(Value::as_array) {
        return arr.iter().collect();
    }
    let mut out = Vec::new();
    if let Some(val) = child.get("ParameterGraph") {
        if let Some(arr) = val.as_array() {
            if arr.first().is_some_and(|v| v.as_array().is_some()) {
                for entry in arr {
                    out.push(entry);
                }
            } else if !arr.is_empty() {
                out.push(val);
            }
        }
    }
    out
}

/// Loads all prepared `sides/<side>/combos.json` sound triggers into `runtime.phase.swbf2`.
fn load_combos(
    content: &BattleContent,
    names: &BTreeMap<String, usize>,
    runtime: &mut Runtime,
) {
    let mut loaded_sides = Vec::<String>::new();
    for side_name in content
        .content
        .battle
        .sides
        .iter()
        .map(|s| s.name.as_str())
        .chain(["all", "cis", "imp", "rep"])
    {
        if side_name.is_empty() || loaded_sides.iter().any(|s| s.eq_ignore_ascii_case(side_name)) {
            continue;
        }
        loaded_sides.push(side_name.to_string());
        let Ok(raw) = content.textures.read(&format!("sides/{side_name}/combos.json")) else {
            continue;
        };
        let Ok(doc) = serde_json::from_slice::<serde_json::Value>(&raw) else {
            continue;
        };
        let Some(combos_obj) = doc.as_object() else { continue };
        for (combo_key, combo_val) in combos_obj {
            let combo_hash = combo_val
                .get("nameHash")
                .and_then(|v| v.as_str())
                .map(parse_raw_hex_or_hash)
                .unwrap_or_else(|| parse_raw_hex_or_hash(combo_key));
            let mut set = ComboSoundSet { combo_hash, states: Vec::new() };
            for state in combo_val.get("states").and_then(|v| v.as_array()).into_iter().flatten() {
                let state_name = state.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let state_hash = if !state_name.is_empty() {
                    zero_hash(state_name)
                } else {
                    state.get("hash").and_then(|v| v.as_str()).map(parse_raw_hex_or_hash).unwrap_or(0)
                };
                let mut triggers = Vec::new();
                for child in state.get("children").and_then(|v| v.as_array()).into_iter().flatten() {
                    let is_sound = child
                        .get("key")
                        .and_then(|v| v.as_str())
                        .is_some_and(|k| k.eq_ignore_ascii_case("Sound"));
                    if !is_sound {
                        continue;
                    }
                    let Some(vals) = child.get("values").and_then(|v| v.as_array()) else { continue };
                    let Some(snd_name) = vals.first().and_then(|v| v.as_str()) else { continue };
                    let Some(&snd_id) = names.get(&key(snd_name)) else { continue };
                    // Combo timings authored in 30 fps frames are converted to seconds.
                    let delay_s = match (vals.get(1).and_then(|v| v.as_f64()), vals.get(2).and_then(|v| v.as_str())) {
                        (Some(n), Some(unit)) if unit.eq_ignore_ascii_case("Frames") => (n / 30.0).max(0.0),
                        (Some(n), _) => n.max(0.0),
                        _ => 0.0,
                    };
                    triggers.push(ComboSoundTrigger { sound: snd_id, delay_s });
                }
                if !triggers.is_empty() {
                    set.states.push(ComboStateSounds { state_hash, triggers });
                }
            }
            if let Some(swbf2) = runtime.phase.swbf2_mut() {
                let idx = swbf2.combos.len();
                swbf2.combos.push(set);
                swbf2.register_combo_hash(combo_hash, idx);
            }
        }
    }
}

/// Prepares a 2005 battle's audio runtime, bank samples, streams, classes, combos, and regions.
pub fn prepare(content: &BattleContent) -> Result<Prepared, String> {
    let index: serde_json::Value =
        serde_json::from_slice(&content.textures.read("audio/index.json")?).map_err(|e| e.to_string())?;
    let mission = content
        .content
        .battle
        .world
        .get("mission")
        .and_then(|m| m.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let map = content.content.battle.world.get("name").and_then(Value::as_str).unwrap_or("");
    let level = index["missions"][mission]
        .as_str()
        .or_else(|| index["maps"][map].as_str())
        .or_else(|| {
            content
                .content
                .battle
                .world
                .get("mission")
                .and_then(|m| m.get("sound"))
                .and_then(|s| s.at(0))
                .and_then(Value::as_str)
        })
        .ok_or("battle has no sound bank")?;

    let manifest = content.textures.read(&format!("audio/{level}/sound.json"))?;
    let manifest =
        bfe_content::json::parse(std::str::from_utf8(&manifest).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let data = Data::new(manifest)?;
    let pcm = content.textures.read(&format!("audio/{level}/bank.bin"))?;

    let mut runtime = Runtime::new_for_family(1, GameFamily::Swbf2);
    let mut output = Output::new(48000., 2, false, false, false, false, cfg!(target_arch = "x86_64"));
    output.reserve_controls(65536);
    for slot in 0..audio::output::VOICES {
        output.voice(slot, true);
    }
    output.layout(
        0,
        None,
        &[Some((-std::f64::consts::FRAC_PI_4, 0.)), Some((std::f64::consts::FRAC_PI_4, 0.))],
        -1,
        -1,
        1.,
    );
    output.volume(1., 0., 0., true);

    // 1. Buses (13 buses in 2005 core hierarchy).
    let mut buses = BTreeMap::new();
    for (name, _) in data.table("Bus") {
        let id = buses.len();
        buses.insert(name.clone(), id);
    }
    for (name, id) in &buses {
        let k = hash_hex(name);
        import(&mut runtime, import::BUS_NAME, &[k as f64, *id as f64])?;
    }
    for (name, id) in data.table("Bus") {
        let o = &data.objects[*id].data;
        let mut row = [f64::NAN; bus::STRIDE];
        row[bus::GAIN] = field(o, "Gain").map_or(1., |v| number(Some(v)));
        row[bus::PARENT] = buses
            .get(text(o.get("Parent")))
            .filter(|_| text(o.get("Parent")) != name)
            .map_or(-1., |i| *i as f64);
        row[bus::TARGET] = buses.get(text(o.get("AttenuateBus"))).map_or(-1., |i| *i as f64);
        for (at, k, default) in [
            (bus::AMOUNT, "Attenuation", 0.5),
            (bus::LEAD_IN, "AttenuationLeadIn", 0.5),
            (bus::LEAD_OUT, "AttenuationLeadOut", 0.5),
        ] {
            row[at] = field(o, k).map_or(default, |v| number(Some(v)));
        }
        import(&mut runtime, import::BUS, &row)?;
    }

    // 2. Bank PCM samples.
    let mut samples = BTreeMap::new();
    if let Some(table) = data.manifest.get("samples").and_then(Value::as_object) {
        for (name, v) in table.entries() {
            let id = samples.len();
            samples.insert(name.to_string(), id);
            let at = number(v.at(0)) as usize;
            let frames = number(v.at(1)) as usize;
            let rate = number(v.at(2));
            let bytes = pcm
                .get(
                    at..at
                        .checked_add(frames.checked_mul(2).ok_or("sample length overflow")?)
                        .ok_or("sample length overflow")?,
                )
                .ok_or("sample exceeds PCM bank")?;
            let target = output.voices.sample_storage(id, frames, rate.max(8000.));
            let (pairs, _) = bytes.as_chunks::<2>();
            for (to, b) in target.iter_mut().zip(pairs) {
                *to = i16::from_le_bytes(*b) as f32 / 32768.;
            }
            import(
                &mut runtime,
                import::SAMPLE,
                &[id as f64, frames as f64 / rate, rate.max(8000.), rate / rate.max(8000.), 1.],
            )?;
        }
    }

    // 3. Streams (including any streams referenced by `mission.appendedStreams`)
    // and optional upscaled enhanced audio manifest (`audio/enhanced/<level>/manifest.json`).
    let enhanced_manifest: Option<serde_json::Value> = content
        .textures
        .read(&format!("audio/enhanced/{level}/manifest.json"))
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok());
    let mut variants = Vec::new();
    let mut stream_list: Vec<String> = data
        .manifest
        .get("streams")
        .and_then(Value::as_array)
        .unwrap_or(&[])
        .iter()
        .map(|v| text(Some(v)).to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if let Some(appended) = data
        .manifest
        .get("mission")
        .and_then(|m| m.get("appendedStreams"))
        .and_then(Value::as_array)
    {
        for pair in appended {
            let appended_name = text(pair.at(1));
            if !appended_name.is_empty() && !stream_list.iter().any(|s| s == appended_name) {
                stream_list.push(appended_name.to_string());
            }
        }
    }

    let mut stream_ids = BTreeMap::new();
    let mut segment_ids = BTreeMap::new();
    let mut segments = Vec::new();
    let mut voice_segments = Vec::new();
    for name in &stream_list {
        let Ok(raw) = content.textures.read(&format!("audio/streams/{name}.json")) else {
            continue;
        };
        let ordered = bfe_content::json::parse_bytes(&raw).map_err(|e| e.to_string())?;
        let idx: serde_json::Value = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
        let id = stream_ids.len();
        stream_ids.insert(key(name), id);
        let channels = idx["channels"].as_u64().unwrap_or(1) as usize;
        let substreams = idx["substreams"].as_u64().unwrap_or(1) as usize;
        let interleave = idx["substreamInterleave"].as_u64().unwrap_or(0) as usize;
        if channels == 0 || substreams == 0 || channels * substreams > 12 || (substreams > 1 && interleave == 0) {
            return Err("invalid stream channel layout".into());
        }
        let slots = data
            .manifest
            .get("mission")
            .and_then(|m| m.get("streams"))
            .and_then(Value::as_array)
            .map_or(0, |calls| calls.iter().filter(|call| text(call.at(1)) == name).count())
            .max(1);
        import(&mut runtime, import::STREAM, &[slots as f64, (channels * substreams) as f64])?;
        runtime.phase.swbf2_mut().unwrap().director.bind_stream_group(id, id);

        let is_vo = name.ends_with("_vo") || name.contains("_vo_");
        for (seg_key, _) in ordered
            .get("segments")
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(|o| o.entries())
        {
            let entry = &idx["segments"][seg_key];
            let offset = entry[0].as_u64().ok_or("invalid stream offset")?;
            let size = entry[1].as_u64().ok_or("invalid stream length")? as usize;
            let frames = entry[2].as_f64().unwrap_or(0.0);
            let rate = entry[3].as_f64().ok_or("invalid stream rate")?;
            // Skip zero-length placeholder segments present in some 2005 streams.
            if size == 0 || !frames.is_finite() || frames <= 0.0 {
                continue;
            }
            if !(8000. ..=192000.).contains(&rate) || size > 256 << 20 {
                return Err("stream segment exceeds bounds".into());
            }
            let chunk = interleave * substreams;
            let length = if substreams > 1 && chunk > 0 { size.div_ceil(chunk) * chunk } else { size };
            let file = content
                .textures
                .path(&format!("audio/streams/{}.bin", idx["name"].as_str().unwrap_or(name)))?;
            let segment = segments.len();
            segment_ids.insert((id, seg_key.to_string()), segment);
            let mut row = [-1.; segment::STRIDE];
            row[segment::STREAM] = id as f64;
            row[segment::SAMPLE] = segment as f64;
            row[segment::DURATION] = frames / substreams as f64 / rate;
            row[segment::CHANNELS] = (channels * substreams) as f64;
            let enh_entry = enhanced_manifest.as_ref().map(|m| &m["streams"][name][seg_key]);
            if let Some(enh_entry) = enh_entry.filter(|e| !e["fallback"].as_bool().unwrap_or(true)) {
                let gain_db = enh_entry["playback_db"].as_f64().unwrap_or(0.);
                let mut first = -1.;
                for (layout, file) in enh_entry["files"].as_object().into_iter().flatten() {
                    let Some(file) = file.as_str() else { continue };
                    let Ok(path) = content.textures.path(&format!("audio/enhanced/{level}/{file}")) else {
                        continue;
                    };
                    let at = variants.len() as f64;
                    variants.push(super::stream_assets::Variant::new(path, gain_db, layout, enh_entry["kind"].as_str()));
                    if first < 0. {
                        first = at;
                    }
                    match layout.as_str() {
                        "stereo" => row[segment::STEREO] = at,
                        "5.1" => row[segment::SURROUND6] = at,
                        "7.1" => row[segment::SURROUND8] = at,
                        _ => {}
                    }
                }
                row[segment::FIRST] = first;
            }
            import(&mut runtime, import::SEGMENT, &row)?;
            voice_segments.push(is_vo);
            segments.push(super::stream_assets::Segment {
                file,
                offset,
                length,
                rate,
                channels,
                substreams,
                interleave,
            });
        }
    }

    // Bind logical stream-group sharing for `AudioStreamAppendSegments`.
    if let Some(appended) = data
        .manifest
        .get("mission")
        .and_then(|m| m.get("appendedStreams"))
        .and_then(Value::as_array)
    {
        for pair in appended {
            let target = text(pair.at(0));
            let source = text(pair.at(1));
            if let (Some(&target_id), Some(&source_id)) = (stream_ids.get(&key(target)), stream_ids.get(&key(source))) {
                runtime.phase.swbf2_mut().unwrap().director.bind_stream_group(source_id, target_id);
            }
        }
    }

    // Stream segments take the 96 render ids after the bank; non-fallback upscaled
    // bank samples (manifest order) follow them.
    let enhanced_base = samples.len() + 96;
    let mut enhanced = Vec::new();
    if let Some(manifest) = &enhanced_manifest {
        for (name, entry) in manifest["samples"].as_object().into_iter().flatten() {
            let (Some(&index), Some(file)) = (samples.get(name.as_str()), entry["file"].as_str()) else {
                continue;
            };
            if entry["fallback"].as_bool().unwrap_or(false) {
                continue;
            }
            let Ok(path) = content.textures.path(&format!("audio/enhanced/{level}/{file}")) else {
                continue;
            };
            enhanced.push(super::enhanced_audio::Item {
                index,
                file: path,
                gain_db: entry["playback_db"].as_f64().unwrap_or(0.),
            });
        }
    }
    output.voices.reserve_samples(enhanced_base + enhanced.len());
    let headphones = match (
        content.textures.read("audio/hrtf/sadie2_d1_3oa_left.wav"),
        content.textures.read("audio/hrtf/sadie2_d1_3oa_right.wav"),
    ) {
        (Ok(left), Ok(right)) => match super::enhanced_audio::headphone_filters(&left, &right) {
            Ok((filters, frames)) => Some((std::sync::Arc::new(filters), frames)),
            Err(err) => {
                eprintln!("audio: headphone decoder not used ({err})");
                None
            }
        },
        _ => None,
    };

    // 4. Sound objects (Properties, Layered, Parameterized, StreamProperties).
    let mut names = BTreeMap::new();
    let mut ids = BTreeMap::new();
    let mut sounds = Vec::new();
    let mut sound_names: Vec<String> = Vec::new();
    for kind in ["SoundProperties", "SoundLayered", "SoundParameterized", "SoundStreamProperties", "SoundStream"] {
        for (name, object) in data.table(kind) {
            if names.contains_key(name) {
                continue;
            }
            let id = *ids.entry(*object).or_insert_with(|| {
                let id = sounds.len();
                sounds.push(*object);
                id
            });
            names.insert(name.clone(), id);
            if sound_names.len() <= id {
                sound_names.resize(id + 1, String::new());
            }
            if sound_names[id].is_empty() {
                sound_names[id] = name.clone();
            }
            import(&mut runtime, import::NAME, &[hash_hex(name) as f64, id as f64])?;
        }
    }
    let mut parameterized = BTreeMap::new();
    for (name, object) in data.table("SoundParameterized") {
        if data
            .object("SoundLayered", text(data.objects[*object].data.get("SoundLayered")))
            .is_none()
        {
            continue;
        }
        let id = *ids.entry(*object).or_insert_with(|| {
            let id = sounds.len();
            sounds.push(*object);
            id
        });
        parameterized.insert(name.clone(), id);
    }

    let resolve = |v: Option<&Value>| -> f64 { sound_id(&names, v) };
    let mut entry_count = 0;
    let mut binding_count = 0;
    let mut point_count = 0;
    let proximity_key = key("Proximity");

    for (sid, object) in sounds.into_iter().enumerate() {
        let o = &data.objects[object].data;
        let kind = text(o.get("type"));
        let mut row = [f64::NAN; sound::STRIDE];
        row[sound::KIND] = if kind == "SoundProperties" {
            1.
        } else if kind == "SoundLayered" {
            2.
        } else if kind == "SoundParameterized" {
            3.
        } else {
            4.
        };
        row[sound::BUS] = buses
            .get(text(o.get("Bus")))
            .or_else(|| buses.get(if row[sound::KIND] == 4. { "0x9f9c4fd4" } else { "0xeceb308a" }))
            .map_or(-1., |n| *n as f64);
        row[sound::STREAM] = stream_ids.get(&key(text(o.get("Stream")))).map_or(-1., |id| *id as f64);
        for (at, k) in [
            (sound::PITCH, "Pitch"),
            (sound::PITCH_DEV, "PitchDev"),
            (sound::GAIN, "Gain"),
            (sound::GAIN_DEV, "GainDev"),
            (sound::REVERB, "ReverbGain"),
            (sound::LOOP, "Looping"),
            (sound::PAN, "Pan"),
            (sound::MODE3D, "Mode3D"),
            (sound::DOPPLER, "ModeDoppler"),
            (sound::BIAS, "Bias"),
            (sound::PRIORITY, "Priority"),
            (sound::MIN_DISTANCE, "MinDistance"),
            (sound::MUTE_DISTANCE, "MuteDistance"),
            (sound::MAX_DISTANCE, "MaxDistance"),
            (sound::ROLL_OFF, "RollOff"),
            (sound::ROLL_IN_DISTANCE, "RollInDistance"),
            (sound::PROBABILITY, "PlayProbability"),
            (sound::INTERVAL, "PlayInterval"),
            (sound::INTERVAL_DEV, "PlayIntervalDev"),
            (sound::RANDOM_POSITION, "RandomPlayPos"),
            (sound::CYCLE, "CyclePlayback"),
        ] {
            row[at] = number(field(o, k).or_else(|| {
                field(
                    o,
                    if at == sound::RANDOM_POSITION {
                        "3af79d76"
                    } else if at == sound::CYCLE {
                        "4d5af670"
                    } else {
                        ""
                    },
                )
            }));
        }
        row[sound::FIRST] = entry_count as f64;
        if let Some(entries) = o
            .get(if row[sound::KIND] == 4. { "SegmentList" } else { "SampleList" })
            .and_then(Value::as_array)
        {
            for e in entries {
                let Some(id) = (if row[sound::KIND] == 4. {
                    segment_ids.get(&(row[sound::STREAM] as usize, text(e.at(0)).to_string()))
                } else {
                    samples.get(text(e.at(0)))
                }) else {
                    continue;
                };
                import(
                    &mut runtime,
                    import::ENTRY,
                    &[
                        *id as f64,
                        field(e, "1").map_or_else(|| e.at(1).map_or(1., |v| number(Some(v))), |v| number(Some(v))),
                        e.at(2).map_or(0., |v| number(Some(v))),
                        e.at(3).map_or(0., |v| number(Some(v))),
                    ],
                )?;
                entry_count += 1;
            }
        }
        row[sound::COUNT] = entry_count as f64 - row[sound::FIRST];
        import(&mut runtime, import::SOUND, &row)?;
        let group_hash = {
            let g = text(o.get("Group"));
            if g.is_empty() { 0 } else { hash_hex(g) }
        };
        runtime.phase.swbf2_mut().unwrap().sound_groups.push(group_hash);

        let layered = if kind == "SoundParameterized" {
            data.object("SoundLayered", text(o.get("SoundLayered")))
        } else if kind == "SoundLayered" {
            Some(o)
        } else {
            None
        };
        for (layer_idx, source) in layered
            .and_then(|o| o.get("Layer"))
            .and_then(Value::as_array)
            .unwrap_or(&[])
            .iter()
            .enumerate()
        {
            let mut lrow = [f64::NAN; layer::STRIDE];
            lrow[layer::PARENT] = sid as f64;
            lrow[layer::SOUND] = resolve(source.at(1));
            lrow[layer::DELAY] = source.at(2).map_or(0., |v| number(Some(v)));
            lrow[layer::DEVIATION] = source.at(3).map_or(0., |v| number(Some(v)));
            lrow[layer::BINDING_FIRST] = binding_count as f64;
            if kind == "SoundParameterized" {
                for child in o.get("Layer.children").and_then(Value::as_array).unwrap_or(&[]) {
                    if text(first(child.get("Name"))) != text(source.at(0)) {
                        continue;
                    }
                    for graph in extract_param_graphs(child) {
                        let input = text(graph.at(0));
                        let output_key = text(graph.at(2));
                        let points_arr = data
                            .object("ParameterGraph", text(graph.at(1)))
                            .and_then(|g| g.get("ControlPoint"))
                            .and_then(Value::as_array)
                            .unwrap_or(&[]);
                        if input == proximity_key {
                            let is_pitch = output_key == key("Pitch");
                            let pts: Vec<[f64; 2]> = points_arr
                                .iter()
                                .map(|pt| [number(pt.at(0)), number(pt.at(1))])
                                .collect();
                            let min_dist = graph.at(3).map_or(5.0, |v| number(Some(v)));
                            let max_dist = graph.at(4).map_or(150.0, |v| number(Some(v)));
                            runtime.phase.swbf2_mut().unwrap().vehicles.proximity_bindings.push(ProximityBinding {
                                sound: sid,
                                layer: layer_idx,
                                pitch: is_pitch,
                                min_dist: if min_dist.is_finite() { min_dist } else { 5.0 },
                                max_dist: if max_dist.is_finite() { max_dist } else { 150.0 },
                                points: pts,
                            });
                            continue;
                        }
                        let mut b = [f64::NAN; binding::STRIDE];
                        b[binding::INPUT] = [
                            "Speed",
                            "Acceleration",
                            "Health",
                            "TurnOnTime",
                            "TurnOffTime",
                            "WindUpTime",
                            "WindDownTime",
                        ]
                        .iter()
                        .position(|n| key(n) == input)
                        .map_or(-1., |i| i as f64);
                        b[binding::OUTPUT] = if output_key == key("Gain") {
                            0.
                        } else if output_key == key("Pitch") {
                            1.
                        } else {
                            -1.
                        };
                        b[binding::FIRST] = point_count as f64;
                        for point in points_arr {
                            import(&mut runtime, import::POINT, &[number(point.at(0)), number(point.at(1))])?;
                            point_count += 1;
                        }
                        b[binding::COUNT] = point_count as f64 - b[binding::FIRST];
                        import(&mut runtime, import::BINDING, &b)?;
                        binding_count += 1;
                    }
                }
            }
            lrow[layer::BINDING_COUNT] = binding_count as f64 - lrow[layer::BINDING_FIRST];
            import(&mut runtime, import::LAYER, &lrow)?;
        }
    }

    // 5. Load melee combos from `sides/<side>/combos.json`.
    load_combos(content, &names, &mut runtime);

    // 6. Merge `sound.json.classes` with resolved ODF classes from `BattleContent`
    // so inherited properties (lightsabers, starfighters, turrets, doors, heroes) resolve cleanly.
    let odf_classes = collect_odf_classes(content);
    let manifest_classes = data.manifest.get("classes").and_then(Value::as_object);
    let mut all_class_names: Vec<String> = Vec::new();
    let mut seen_lower = BTreeMap::<String, usize>::new();
    if let Some(table) = manifest_classes {
        for (name, _) in table.entries() {
            let lower = name.to_ascii_lowercase();
            seen_lower.entry(lower).or_insert_with(|| {
                let idx = all_class_names.len();
                all_class_names.push(name.to_string());
                idx
            });
        }
    }
    for lower in odf_classes.keys() {
        seen_lower.entry(lower.clone()).or_insert_with(|| {
            let idx = all_class_names.len();
            all_class_names.push(lower.clone());
            idx
        });
    }

    let mut tracks = BTreeMap::new();
    let mut classes = BTreeMap::<String, usize>::new();
    let mut class_rows = Vec::<[f64; class::STRIDE]>::new();

    for (id, name) in all_class_names.iter().enumerate() {
        classes.insert(name.to_ascii_lowercase(), id);
        let m_cls = manifest_classes.and_then(|t| t.get(name));
        let o_cls = odf_classes.get(&name.to_ascii_lowercase());

        let mut row = [-1.; class::STRIDE];
        for (at, k) in [
            (class::FIRE, "FireSound"),
            (class::RELOAD, "ReloadSound"),
            (class::OVERHEAT, "OverheatSound"),
            (class::CHARGE, "ChargeSound"),
            (class::FIRE_LOOP, "FireLoopSound"),
            (class::EMPTY, "FireEmptySound"),
            (class::CHANGE, "WeaponChangeSound"),
            (class::JUMP, "JumpSound"),
            (class::LAND, "LandSound"),
            (class::ROLL, "RollSound"),
            (class::SQUAT, "SquatSound"),
            (class::PRONE, "ProneSound"),
            (class::STAND, "StandSound"),
            (class::RUSTLE, "ClothingRustleSound"),
            (class::HURT, "HurtSound"),
            (class::DEATH, "DeathSound"),
            (class::ENGINE, "EngineSound"),
            (class::TURN_ON, "TurnOnSound"),
            (class::TURNING_OFF, "TurningOffSound"),
            (class::TURN_OFF, "TurnOffSound"),
            (class::TAKEOFF, "TakeoffSound"),
            (class::ACTIVATE, "TurretActivateSound"),
            (class::DEACTIVATE, "TurretDeactivateSound"),
            (class::YAW, "TurretYawSound"),
            (class::PITCH, "TurretPitchSound"),
            (class::IMPACT, "CollisionSound"),
            (class::BOUNCE, "CollisionOtherSound"),
            (class::SPLASH, "CollisionWaterSound"),
            (class::EXPLOSION, "SoundProperty"),
            (class::POWERUP, "PowerupSound"),
            (class::ORDNANCE, "OrdnanceSound"),
            (class::AMBIENT, "AmbientSound"),
            (class::AMBIENT2, "Ambient2Sound"),
            (class::TICK, "TickSound"),
            (class::HYDRAULIC, "HydraulicSound"),
            (class::STEP0, "FootstepSound0"),
            (class::STEP1, "FootstepSound1"),
            (class::STEP2, "FootstepSound2"),
            (class::STEP3, "FootstepSound3"),
            (class::STEP4, "FootstepSound4"),
            (class::STEP5, "FootstepSound5"),
            (class::DK_STEP0, "Footstep0Sound"),
            (class::DK_STEP1, "Footstep1Sound"),
            (class::DK_STEP2, "Footstep2Sound"),
            (class::WEAPON_LEFT, "0x1064c0a7"),
            (class::WEAPON_RIGHT, "0x10a991e4"),
            (class::COLLISION, "VehicleCollisionSound"),
            (class::DISCHARGE, "DischargeSound"),
            (class::DISPUTE, "DisputeSound"),
            (class::CAPTURE, "CapturedSound"),
            (class::LOST, "LostSound"),
        ] {
            let val = lookup_prop_alias(m_cls, o_cls, k);
            row[at] = if at == class::ENGINE {
                let sname = text(first(val)).split_whitespace().next().unwrap_or("");
                parameterized.get(&key(sname)).map_or_else(|| resolve(val), |pid| *pid as f64)
            } else {
                resolve(val)
            };
        }
        for (side, at) in [
            ("Rep", class::AMBIENT_REP),
            ("Cis", class::AMBIENT_CIS),
            ("All", class::AMBIENT_ALL),
            ("Imp", class::AMBIENT_IMP),
        ] {
            let vals = lookup_prop_multi(m_cls, o_cls, "AmbientSound");
            row[at] = value_list(vals)
                .into_iter()
                .find_map(|v| {
                    let mut parts = text(Some(v)).split_whitespace();
                    parts
                        .next()
                        .filter(|n| n.eq_ignore_ascii_case(side))
                        .map(|_| parts.next().and_then(|n| names.get(&key(n))).map_or(-1., |i| *i as f64))
                })
                .unwrap_or(-1.);
        }
        for (at, k) in [
            (class::OVERHEAT_PITCH, "OverheatSoundPitch"),
            (class::CHARGE_PITCH, "ChargeSoundPitch"),
            (class::FIRE_STOP, "FireSoundStop"),
            (class::TICK_PITCH, "TickSoundPitch"),
            (class::TURN_OFF_TIME, "TurnOffTime"),
            (class::FOOTSTEP_STRIDE, "FootstepStride"),
            (class::YAW_PITCH, "TurretYawSoundPitch"),
            (class::PITCH_PITCH, "TurretPitchSoundPitch"),
        ] {
            let val = lookup_prop(m_cls, o_cls, k);
            row[at] = number(if [class::TURN_OFF_TIME, class::FOOTSTEP_STRIDE, class::FIRE_STOP].contains(&at) {
                val
            } else {
                first(val)
            });
        }
        import(&mut runtime, import::CLASS, &row)?;
        class_rows.push(row);

        // Populate 2005 per-class extensions.
        let num_or = |k: &str, default: f64| -> f64 {
            let v = number(first(lookup_prop(m_cls, o_cls, k)));
            if v.is_finite() { v } else { default }
        };
        let soldier_music = {
            let tid = track(&data, &names, &mut tracks, &mut runtime, first(lookup_prop(m_cls, o_cls, "SoldierMusic")))?;
            if tid < 0.0 { NONE } else { tid as usize }
        };
        let rep_music = {
            let tid = track(&data, &names, &mut tracks, &mut runtime, first(lookup_prop(m_cls, o_cls, "RepMusic")))?;
            if tid < 0.0 { NONE } else { tid as usize }
        };
        let cis_music = {
            let tid = track(&data, &names, &mut tracks, &mut runtime, first(lookup_prop(m_cls, o_cls, "CisMusic")))?;
            if tid < 0.0 { NONE } else { tid as usize }
        };
        let swbf2_cls = Swbf2Class {
            saber_turn_on: sound_index(&names, lookup_prop(m_cls, o_cls, "TurnOnSound")),
            saber_turn_off: sound_index(&names, lookup_prop(m_cls, o_cls, "TurnOffSound")),
            saber_on_hum: sound_index(&names, lookup_prop(m_cls, o_cls, "OnSound")),
            saber_hit: sound_index(&names, lookup_prop(m_cls, o_cls, "HitSound")),
            saber_deflect: sound_index(&names, lookup_prop(m_cls, o_cls, "DeflectSound")),
            boost_cues: parse_boost_cues(&names, lookup_prop_multi(m_cls, o_cls, "BoostSound")),
            trick_sound: sound_index(&names, lookup_prop(m_cls, o_cls, "TrickSound")),
            flip_sound: sound_index(&names, lookup_prop(m_cls, o_cls, "FlipSound")),
            cockpit_1p: sound_index(&names, lookup_prop(m_cls, o_cls, "Cockpit1stPersonSound")),
            cockpit_3p: sound_index(&names, lookup_prop(m_cls, o_cls, "Cockpit3rdPersonSound")),
            warm_up: sound_index(&names, lookup_prop(m_cls, o_cls, "WarmUpSound")),
            warm_up_time: num_or("WarmUpTime", 0.5),
            warm_up_start_pitch: num_or("WarmUpSoundStartPitch", 0.5),
            warm_up_fade_in: num_or("WarmUpSoundFadeInTime", 0.1),
            warm_up_fade_out: num_or("WarmUpSoundFadeOutTime", 0.1),
            barrel: sound_index(&names, lookup_prop(m_cls, o_cls, "BarrelSound")),
            barrel_fade_in: num_or("BarrelSoundFadeInTime", 0.05),
            barrel_fade_out: num_or("BarrelSoundFadeOutTime", 0.1),
            building_sound: sound_index(&names, lookup_prop(m_cls, o_cls, "BuildingSound")),
            damage_effect_sound: sound_index(&names, lookup_prop(m_cls, o_cls, "DamageEffectSound")),
            door_open: sound_index(&names, lookup_prop(m_cls, o_cls, "OpenSound")),
            door_close: sound_index(&names, lookup_prop(m_cls, o_cls, "CloseSound")),
            door_locked: sound_index(&names, lookup_prop(m_cls, o_cls, "LockedSound")),
            soldier_music,
            rep_music,
            cis_music,
            hero_spawned: sound_index(&names, lookup_prop(m_cls, o_cls, "SndHeroSpawned")),
            hero_defeated: sound_index(&names, lookup_prop(m_cls, o_cls, "SndHeroDefeated")),
            hero_killer: sound_index(&names, lookup_prop(m_cls, o_cls, "SndHeroKiller")),
            vo_unit_type: num_or("VOUnitType", 0.0).max(0.0) as u32,
            vo_rules: parse_vo_rules(&names, lookup_prop_multi(m_cls, o_cls, "VOSound")),
            ordnance_class: NONE,
            explosion_class: NONE,
            expire_explosion_class: NONE,
            destruct_explosion_class: NONE,
        };
        let swbf2 = runtime.phase.swbf2_mut().unwrap();
        swbf2.classes.push(swbf2_cls);
        swbf2.register_class_hash(zero_hash(name), id);

        // Register raw property-hash chatter table for any class properties present.
        let mut properties: Vec<(u32, f64)> = Vec::new();
        for src in [o_cls, m_cls].into_iter().flatten() {
            let Some(props) = src.as_object() else { continue };
            for (k, v) in props.entries() {
                if !is_hex_key(k) {
                    set_property(&mut properties, key_hash(k), resolve(Some(v)));
                }
            }
            for (k, v) in props.entries() {
                if is_hex_key(k) {
                    let hk = u32::from_str_radix(k, 16).unwrap_or_default();
                    if properties.iter().find(|(h, _)| *h == hk).map_or(-1., |(_, s)| *s) < 0. {
                        set_property(&mut properties, hk, resolve(Some(v)));
                    }
                }
            }
        }
        for (pk, ps) in properties {
            import(&mut runtime, import::PROPERTY, &[id as f64, pk as f64, ps])?;
        }
    }

    // Resolve class-to-class links (weapon -> ordnance -> explosion, entity -> destruction explosion)
    // from `visual_effects()["links"]` and flattened ODF properties once all class indices exist.
    let fx_links = content.visual_effects().get("links").and_then(Value::as_object);
    let resolve_class_ref = |raw: &str| -> usize {
        let stripped = strip_class_section_prefix(raw);
        if stripped.is_empty() {
            NONE
        } else {
            classes.get(&stripped.to_ascii_lowercase()).copied().unwrap_or(NONE)
        }
    };
    let link_field = |prefix: &str, lower: &str, field_name: &str| -> usize {
        let Some(links) = fx_links else { return NONE };
        let key_str = format!("{prefix}/{lower}");
        let entry = links.get(&key_str).or_else(|| {
            links
                .entries()
                .find(|(k, _)| k.eq_ignore_ascii_case(&key_str))
                .map(|(_, v)| v)
        });
        entry
            .and_then(|e| e.get(field_name))
            .and_then(|v| first(Some(v)))
            .and_then(Value::as_str)
            .map(&resolve_class_ref)
            .unwrap_or(NONE)
    };
    for (id, name) in all_class_names.iter().enumerate() {
        let lower = name.to_ascii_lowercase();
        let m_cls = manifest_classes.and_then(|t| t.get(name));
        let o_cls = odf_classes.get(&lower);
        let prop_ref = |prop: &str| -> usize {
            let s = text(first(lookup_prop(m_cls, o_cls, prop)));
            if s.is_empty() { NONE } else { resolve_class_ref(s) }
        };
        let first_valid = |candidates: &[usize]| -> usize {
            candidates.iter().copied().find(|&c| c != NONE).unwrap_or(NONE)
        };
        let ordnance_class = first_valid(&[
            link_field("wpnc", &lower, "OrdnanceName"),
            prop_ref("OrdnanceName"),
        ]);
        let explosion_class = first_valid(&[
            link_field("ordc", &lower, "ExplosionName"),
            link_field("ordc", &lower, "ExplosionDestruct"),
            link_field("ordc", &lower, "Explosion"),
            link_field("wpnc", &lower, "ExplosionName"),
            link_field("wpnc", &lower, "Explosion"),
            link_field("entc", &lower, "ExplosionName"),
            prop_ref("ExplosionName"),
            prop_ref("ExplosionDestruct"),
            prop_ref("Explosion"),
        ]);
        let expire_explosion_class = first_valid(&[
            link_field("ordc", &lower, "ExplosionExpire"),
            link_field("wpnc", &lower, "ExplosionExpire"),
            link_field("entc", &lower, "ExplosionExpire"),
            prop_ref("ExplosionExpire"),
        ]);
        let destruct_explosion_class = first_valid(&[
            link_field("entc", &lower, "ExplosionDestruct"),
            link_field("entc", &lower, "ExplosionName"),
            prop_ref("ExplosionDestruct"),
            prop_ref("ExplosionName"),
        ]);
        if let Some(cls) = runtime.phase.swbf2_mut().and_then(|s| s.classes.get_mut(id)) {
            cls.ordnance_class = ordnance_class;
            cls.explosion_class = explosion_class;
            cls.expire_explosion_class = expire_explosion_class;
            cls.destruct_explosion_class = destruct_explosion_class;
        }
    }

    // 7. Footstep material boxes and FoleyFX tables (with ODF inheritance fallback).
    let mut materials: Vec<String> = vec!["terrain_foley".into(), "water_foley".into()];
    for b in data
        .manifest
        .get("world")
        .and_then(|w| w.get("materials"))
        .and_then(Value::as_array)
        .unwrap_or(&[])
    {
        let group = text(b.get("group"));
        let id = match materials.iter().position(|m| m == group) {
            Some(id) => id,
            None => {
                materials.push(group.into());
                materials.len() - 1
            }
        };
        let mut row = [0.; material::STRIDE];
        for k in 0..3 {
            row[material::MIN_X + k] = number(b.get("min").and_then(|v| v.at(k)));
            row[material::MAX_X + k] = number(b.get("max").and_then(|v| v.at(k)));
        }
        row[material::GROUP] = id as f64;
        import(&mut runtime, import::MATERIAL, &row)?;
    }
    for (material_id, group_name) in materials.iter().enumerate() {
        let Some(group) = data
            .object("FoleyFXGroup", group_name)
            .or_else(|| data.object("a1c9cbc5", group_name))
        else {
            continue;
        };
        for (class_id, name) in all_class_names.iter().enumerate() {
            let m_cls = manifest_classes.and_then(|t| t.get(name));
            let o_cls = odf_classes.get(&name.to_ascii_lowercase());
            let fx_class = text(first(lookup_prop(m_cls, o_cls, "FoleyFXClass")));
            if fx_class.is_empty() {
                continue;
            }
            let fx_class_key = key(fx_class);
            let Some(entry) = group
                .get("FoleyFX")
                .and_then(Value::as_array)
                .unwrap_or(&[])
                .iter()
                .filter_map(|n| data.object("FoleyFXSoldier", text(n.at(0))))
                .find(|e| text(e.get("Class")) == fx_class_key)
            else {
                continue;
            };
            let mut row = [-1.; foley::STRIDE];
            row[foley::CLASS] = class_id as f64;
            row[foley::MATERIAL] = material_id as f64;
            for (at, k) in [
                (foley::LEFT, "FootstepLeftRun"),
                (foley::RIGHT, "FootstepRightRun"),
                (foley::JUMP, "jump"),
                (foley::BODYFALL, "bodyfall"),
                (foley::LAND, "Land"),
                (foley::ROLL, "roll"),
                (foley::SQUAT, "squat"),
                (foley::PRONE, "prone"),
                (foley::STAND, "stand"),
            ] {
                row[at] = resolve(entry.get(k));
            }
            import(&mut runtime, import::FOLEY, &row)?;
        }
    }

    // 8. Mission music, effects, doppler, parameter scaling, hero flags, low reinforcements.
    let mission_cfg = data.manifest.get("mission");
    let mut music = Vec::new();
    for (team, list) in mission_cfg
        .and_then(|m| m.get("music"))
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|o| o.entries())
    {
        let Ok(team) = team.parse::<usize>() else { continue };
        if team > 2 {
            continue;
        }
        for source in list.as_array().unwrap_or(&[]) {
            let id = track(&data, &names, &mut tracks, &mut runtime, source.at(1))?;
            let stage = number(source.at(2));
            let amount = number(source.at(0));
            let is_frac = source.at(3).map_or(0., |v| number(Some(v)));
            let mut row = [0.; music_slot::STRIDE];
            row[music_slot::TEAM] = team as f64;
            row[music_slot::STAGE] = stage;
            row[music_slot::TRACK] = id;
            row[music_slot::AMOUNT] = amount;
            row[music_slot::FRACTION] = is_frac;
            row[music_slot::START] = 1.0;
            music.push(row);
            import(&mut runtime, import::MUSIC_SLOT, &row)?;
            if id >= 0.0 && stage.is_finite() && stage >= 0.0 && amount.is_finite() {
                runtime.phase.swbf2_mut().unwrap().director.set_stage_slot(
                    team,
                    stage as usize,
                    id as usize,
                    amount,
                    is_frac != 0.0,
                );
            }
        }
    }
    for (name, value) in mission_cfg
        .and_then(|m| m.get("effects"))
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|o| o.entries())
    {
        import(&mut runtime, import::EFFECT, &[hash(name) as f64, resolve(Some(value))])?;
    }
    if let Some(factor) = mission_cfg.and_then(|m| m.get("dopplerFactor")).and_then(Value::as_f64) {
        runtime.phase.set_doppler_factor(factor);
    }
    if let Some(scaled) = mission_cfg.and_then(|m| m.get("scaledParameters")).and_then(Value::as_array) {
        for entry in scaled {
            let grp = text(entry.at(0));
            let param_name = text(entry.at(1));
            let scale = number(entry.at(2));
            if !grp.is_empty() && scale.is_finite() {
                if let Some(param) = parse_sound_param(param_name) {
                    runtime.phase.scale_sound_parameter(hash_hex(grp), param, scale);
                }
            }
        }
    }
    if let Some(val) = mission_cfg.and_then(|m| m.get("enableHeroMusic")).filter(|v| !v.is_null()) {
        runtime.phase.enable_hero_music(val.truthy());
    }
    if let Some(val) = mission_cfg.and_then(|m| m.get("enableHeroVo")).filter(|v| !v.is_null()) {
        runtime.phase.enable_hero_vo(val.truthy());
    }
    let low_reinf_rules = mission_cfg
        .and_then(|m| m.get("lowReinforcementsVoByMission"))
        .and_then(|by_m| by_m.get(mission))
        .and_then(Value::as_array)
        .or_else(|| mission_cfg.and_then(|m| m.get("lowReinforcementsVo")).and_then(Value::as_array));
    if let Some(rules) = low_reinf_rules {
        for r in rules {
            let watched_team = number(r.at(0)) as usize;
            let threshold = number(r.at(1));
            let snd = text(r.at(2));
            let is_frac = r.at(3).is_none_or(|v| number(Some(v)) != 0.0);
            let listener_team = r.at(4).map_or(watched_team, |v| number(Some(v)) as usize);
            if (1..=2).contains(&watched_team)
                && (1..=2).contains(&listener_team)
                && !snd.is_empty()
                && threshold.is_finite()
            {
                runtime.phase.set_low_reinforcements_vo(
                    watched_team,
                    threshold,
                    hash_hex(snd),
                    is_frac,
                    listener_team,
                );
            }
        }
    }

    // 9. World static emitters, `soundstream`, `soundspace`, and `soundtrigger` regions.
    let world = data.manifest.get("world");
    for source in world.and_then(|w| w.get("emitters")).and_then(Value::as_array).unwrap_or(&[]) {
        let props = source.get("props");
        let mut row = [f64::NAN; region::STRIDE];
        row[region::SOUND] = resolve(props.and_then(|p| p.get("Sound").or_else(|| p.get("SoundStream"))));
        for k in 0..3 {
            row[region::X + k] = number(source.get("position").and_then(|p| p.at(k)));
        }
        row[region::MIN_DISTANCE] = props.and_then(|p| field(p, "MinDistance")).map_or(1., |v| number(Some(v)));
        row[region::MAX_DISTANCE] = props.and_then(|p| field(p, "MaxDistance")).map_or(10., |v| number(Some(v)));
        import(&mut runtime, import::STATIC_EMITTER, &row)?;
    }

    // Prefer `world.regions` from sound.json, falling back to `map_data.regions` when richer.
    let world_regions = world.and_then(|w| w.get("regions")).and_then(Value::as_array);
    let map_regions = content.content.battle.map_data.get("regions").and_then(Value::as_array);
    let regions_slice: &[Value] = match (world_regions, map_regions) {
        (Some(wr), Some(mr)) if mr.len() > wr.len() => mr,
        (Some(wr), _) => wr,
        (None, Some(mr)) => mr,
        (None, None) => &[],
    };

    for source in regions_slice {
        let mut parts = text(source.get("name")).split_whitespace();
        let Some(tag) = parts.next() else { continue };
        if tag.eq_ignore_ascii_case("soundstream") {
            let name = parts.next().unwrap_or("");
            let mut row = [f64::NAN; region::STRIDE];
            row[region::SOUND] = sound_id(&names, Some(&Value::str(name)));
            for k in 0..3 {
                row[region::X + k] = number(source.get("matrix").and_then(|m| m.at(12 + k)));
                row[region::SIZE_X + k] = number(source.get("size").and_then(|m| m.at(k)));
            }
            // For sphere regions, `size[0]` is the sphere radius; zero out editor defaults on y/z.
            if text(source.get("type")).eq_ignore_ascii_case("sphere") {
                row[region::SIZE_Y] = 0.0;
                row[region::SIZE_Z] = 0.0;
            }
            row[region::SCALE] = parts.next().map_or(1., |v| v.parse::<f64>().unwrap_or(1.));
            import(&mut runtime, import::REGION, &row)?;
        } else if tag.eq_ignore_ascii_case("soundtrigger") {
            let name = parts.next().unwrap_or("");
            if name.is_empty() {
                continue;
            }
            let m: [f64; 16] = std::array::from_fn(|k| number(source.get("matrix").and_then(|m| m.at(k))));
            let size: [f64; 3] = std::array::from_fn(|k| number(source.get("size").and_then(|m| m.at(k))));
            if m.iter().chain(&size).all(|v| v.is_finite()) {
                let shape = parse_shape(text(source.get("type")));
                let snd = sound_index(&names, Some(&Value::str(name)));
                runtime.phase.swbf2_mut().unwrap().ambience.triggers.push(TriggerRegion {
                    hash: zero_hash(name),
                    matrix: m,
                    size,
                    shape,
                    sound: snd,
                    enabled: false,
                    was_active: false,
                    voice: audio_control::engine::SILENT,
                    is_stream: false,
                    retry: Default::default(),
                });
            }
        }
    }

    // 10. Interior sound spaces (`space` I3DL2 presets, box/cylinder/sphere regions, neighbor attenuation).
    let room_of = |space: Option<&Value>| -> (f64, [f64; room::STRIDE], Room, u32) {
        let preset = space
            .and_then(|s| s.get("I3DL2ReverbPreset"))
            .and_then(Value::as_str)
            .and_then(|name| data.object("I3DL2ReverbPreset", name));
        let send = space.and_then(|s| field(s, "RoomGain")).map_or(0., |v| number(Some(v)));
        let mut r_row = [f64::NAN; room::STRIDE];
        for (at, k, default) in [
            (room::ROOM, "RoomGain", 0.),
            (room::ROOM_HF, "RoomHFGain", 1.),
            (room::DECAY, "DecayTime", 1.),
            (room::HF_RATIO, "DecayHFRatio", 0.5),
            (room::REFLECTIONS, "ERGain", 0.),
            (room::REFL_DELAY, "ERDelay", 0.02),
            (room::REVERB, "ReverbGain", 0.),
            (room::REVERB_DELAY, "ReverbDelay", 0.04),
            (room::DIFFUSION, "Diffusion", 1.),
            (room::HF_REFERENCE, "HFReference", 5000.),
        ] {
            r_row[at] = preset.and_then(|p| field(p, k)).map_or(default, |v| number(Some(v)));
        }
        r_row[room::ROOM_HF] = r_row[room::ROOM_HF].max(0.001);
        r_row[room::DIFFUSION] = r_row[room::DIFFUSION].clamp(0., 1.);
        r_row[room::DENSITY] = 1.;
        let mask = if preset.is_some() { 2047 } else { 1 };
        r_row[room::MASK] = mask as f64;
        let room = Room {
            room: r_row[room::ROOM],
            room_hf: r_row[room::ROOM_HF],
            decay: r_row[room::DECAY],
            hf_ratio: r_row[room::HF_RATIO],
            reflections: r_row[room::REFLECTIONS],
            refl_delay: r_row[room::REFL_DELAY],
            reverb: r_row[room::REVERB],
            reverb_delay: r_row[room::REVERB_DELAY],
            diffusion: r_row[room::DIFFUSION],
            density: r_row[room::DENSITY],
            hf_reference: r_row[room::HF_REFERENCE],
        };
        (send, r_row, room, mask)
    };

    let mut spaces = Vec::new();
    let mut space_ids = BTreeMap::new();
    let mut space_objects: Vec<(usize, &Value)> = Vec::new();
    for (name, id) in data.table("space") {
        let sid = *space_ids.entry(key(name)).or_insert(spaces.len());
        let obj = &data.objects[*id].data;
        let (send, row, room, mask) = room_of(Some(obj));
        if sid == spaces.len() {
            spaces.push((send, row));
            runtime.phase.swbf2_mut().unwrap().ambience.spaces.push(SpacePreset {
                hash: hash_hex(name),
                send,
                room,
                mask,
            });
            space_objects.push((sid, obj));
        }
    }
    // Parse neighbor space attenuation entries (`148f0d58` -> `Space` / `3553e285`) when exported.
    for (from_sid, obj) in space_objects {
        for att_key in ["SpaceAttenuations", "148f0d58"] {
            if let Some(list) = obj.get(att_key).and_then(Value::as_array) {
                for item in list {
                    let target_name = text(item.at(0));
                    let gain = number(item.at(1));
                    if !target_name.is_empty() && gain.is_finite() {
                        if let Some(&to_sid) = space_ids.get(&key(target_name)) {
                            runtime.phase.swbf2_mut().unwrap().ambience.attenuations.push(SpaceAttenuation {
                                from_space: from_sid,
                                to_space: to_sid,
                                gain,
                            });
                        }
                    }
                }
            }
        }
    }

    let mut space_boxes = Vec::new();
    for source in regions_slice {
        let mut parts = text(source.get("name")).split_whitespace();
        if !parts.next().is_some_and(|s| s.eq_ignore_ascii_case("soundspace")) {
            continue;
        }
        let Some(&space) = space_ids.get(&key(parts.next().unwrap_or(""))) else { continue };
        let m: [f64; 16] = std::array::from_fn(|k| number(source.get("matrix").and_then(|m| m.at(k))));
        let size: [f64; 3] = std::array::from_fn(|k| number(source.get("size").and_then(|m| m.at(k))));
        if m.iter().chain(&size).all(|v| v.is_finite()) {
            let shape = parse_shape(text(source.get("type")));
            space_boxes.push(SpaceBox { matrix: m, size, space });
            runtime.phase.swbf2_mut().unwrap().ambience.space_regions.push(SpaceRegion {
                matrix: m,
                size,
                shape,
                space,
            });
        }
    }
    let (_, dry_row, dry_room, dry_mask) = room_of(None);
    runtime.phase.swbf2_mut().unwrap().ambience.dry = SpacePreset {
        hash: 0,
        send: 0.0,
        room: dry_room,
        mask: dry_mask,
    };
    import(&mut runtime, import::ROOM, &dry_row)?;

    // 11. Global runtime config (2005 pause & scoreboard buses).
    let mut config = [-1.; config::STRIDE];
    config[config::MASTER_BUS] = buses.get("0xeceb308a").map_or(-1., |n| *n as f64);
    config[config::PAUSE_BUS] = buses
        .get("0xfa930d1c")
        .or_else(|| buses.get(&key("ingame")))
        .map_or(-1., |n| *n as f64);
    runtime.phase.swbf2_mut().unwrap().scoreboard_bus = buses
        .get("0xb38617c5")
        .or_else(|| buses.get(&key("ingame")))
        .copied()
        .unwrap_or(NONE);
    config[config::DEVICE_MEMORY] = 4.;
    config[config::SAMPLE_BASE] = samples.len() as f64;
    config[config::SPACE_SEND] = 0.;
    config[config::OWN_SCOPE_IN] = resolve(mission_cfg.and_then(|m| m.get("effects")).and_then(|e| e.get("ScopeDisplayZoomIn")));
    config[config::OWN_SCOPE_OUT] = resolve(mission_cfg.and_then(|m| m.get("effects")).and_then(|e| e.get("ScopeDisplayZoomOut")));
    for team in 1..=2 {
        let tk = team.to_string();
        config[if team == 1 { config::VICTORY1 } else { config::VICTORY2 }] =
            track(&data, &names, &mut tracks, &mut runtime, mission_cfg.and_then(|m| m.get("victory")).and_then(|m| m.get(&tk)))?;
        config[if team == 1 { config::DEFEAT1 } else { config::DEFEAT2 }] =
            track(&data, &names, &mut tracks, &mut runtime, mission_cfg.and_then(|m| m.get("defeat")).and_then(|m| m.get(&tk)))?;
        config[if team == 1 { config::BOUNDS1 } else { config::BOUNDS2 }] =
            resolve(mission_cfg.and_then(|m| m.get("outOfBounds")).and_then(|m| m.get(&tk)));
    }
    for listener in 1..=2 {
        for actor in 1..=2 {
            let found = mission_cfg
                .and_then(|m| m.get("bleeding"))
                .and_then(Value::as_array)
                .and_then(|rows| {
                    rows.iter().find(|r| {
                        number(r.at(0)) == listener as f64
                            && number(r.at(1)) == actor as f64
                            && r.at(3).map_or(1., |v| number(Some(v))) != 0.
                    })
                });
            config[config::BLEED11 + (listener - 1) * 2 + actor - 1] = resolve(found.and_then(|r| r.at(2)));
        }
    }
    import(&mut runtime, import::CONFIG, &config)?;

    // 12. Bind definitions, registries, posts, stations, and placed ambient buildings.
    let defs = content.defs()?;
    let lookup_class = |name: &str| -> usize {
        classes.get(&name.to_ascii_lowercase()).copied().unwrap_or(usize::MAX)
    };
    let mut prediction_weapons = Vec::new();
    let mut weapon_meta = |record: &Value| {
        let mut row = [0.; weapon::STRIDE];
        let cid = lookup_class(text(record.get("class")));
        row[weapon::CLASS] = if cid == usize::MAX { -1. } else { cid as f64 };
        row[weapon::THRESHOLD] = record.get("HeatThreshold").map_or(1., |v| number(Some(v)));
        row[weapon::MAX_PRESSED] = bfe_content::odf::num(record.get("MaxPressedTime"), 1.);
        row[weapon::SHOT_DELAY] = bfe_content::odf::num(record.get("ShotDelay"), 0.);
        row[weapon::LAUNCHER] = (text(record.get("base")) == "launcher") as u8 as f64;
        row[weapon::STRIKE] = record.get("strike").is_some_and(Value::truthy) as u8 as f64;
        row[weapon::ZOOM_FIRST_PERSON] = record.get("ZoomFirstPerson").is_some_and(Value::truthy) as u8 as f64;
        prediction_weapons.push((row, bfe_content::odf::num(record.get("channel"), 0.) as usize));
    };
    for def in &defs.weapons {
        weapon_meta(&def.record);
    }
    for vehicle in &defs.vehicles {
        if vehicle.registry_id.is_some() {
            for seat in &vehicle.seats {
                for weapon in &seat.weapons {
                    if let Some(def) = weapon.get("def") {
                        weapon_meta(def);
                    }
                }
            }
        }
    }
    for prop in &defs.props {
        if text(prop.record.get("base")) == "armedbuilding" {
            if let Some(def) = prop.record.get("weapon") {
                weapon_meta(def);
            }
        }
    }

    let mut actor_limits = vec![[300., 9., 0.]; defs.registry.classes.len() + 1];
    for (id, record, vehicle) in defs
        .classes
        .iter()
        .filter_map(|c| c.registry_id.map(|id| (id, &c.record, false)))
        .chain(defs.vehicles.iter().filter_map(|c| c.registry_id.map(|id| (id, &c.record, true))))
    {
        let flying = record.get("base").and_then(Value::as_str) == Some("flyer");
        let record = if vehicle { record.get("settings").unwrap_or(record) } else { record };
        actor_limits[id as usize] = [
            bfe_content::odf::num(record.get("MaxHealth"), if vehicle { 1000. } else { 300. }),
            bfe_content::odf::num(record.get("MaxSpeed"), 9.),
            flying as u8 as f64,
        ];
    }

    let actor_classes = defs.registry.classes.iter().map(|n| lookup_class(n)).collect();
    let weapon_classes = defs.registry.weapons.iter().map(|n| lookup_class(n)).collect();
    let combat_classes = std::iter::once(usize::MAX)
        .chain(defs.registry.combat.iter().map(|(name, def)| {
            let c = lookup_class(text(def.get("class")));
            if c != usize::MAX { c } else { lookup_class(name) }
        }))
        .collect();

    let mut sides = ["", "", ""];
    for (team, side) in sides.iter_mut().enumerate().skip(1) {
        let cls = content
            .content
            .battle
            .world
            .get("mission")
            .and_then(|m| m.get("teams"))
            .and_then(|t| t.get(&team.to_string()))
            .and_then(|t| t.get("classes"))
            .and_then(|c| c.at(0))
            .and_then(|c| c.at(0))
            .and_then(Value::as_str)
            .unwrap_or("");
        *side = match cls.split('_').next().unwrap_or("") {
            "rep" => "Rep",
            "cis" => "Cis",
            "all" => "All",
            "imp" => "Imp",
            _ => "",
        };
    }

    let instances = content
        .content
        .battle
        .props
        .as_ref()
        .and_then(|p| p.get("instances"))
        .and_then(Value::as_array)
        .unwrap_or(&[]);
    let objectives: Vec<&str> = instances
        .iter()
        .filter_map(|inst| inst.get("post").and_then(Value::as_str))
        .filter(|post| !post.is_empty())
        .collect();
    let mut posts = Vec::new();
    let mut post_building = Vec::new();
    for (slot, name) in defs.registry.posts.iter().enumerate() {
        let meta = world.and_then(|w| w.get("posts")).and_then(|p| p.get(name));
        let mut row = [-1.; post::STRIDE];
        row[post::ID] = slot as f64;
        let cid = lookup_class(
            meta.and_then(|m| m.get("class"))
                .and_then(Value::as_str)
                .unwrap_or("com_bldg_controlzone"),
        );
        row[post::CLASS] = if cid == usize::MAX { -1. } else { cid as f64 };
        for (team, at) in [(1, post::AMBIENT1), (2, post::AMBIENT2)] {
            let ambient = match sides[team] {
                "Rep" => class::AMBIENT_REP,
                "Cis" => class::AMBIENT_CIS,
                "All" => class::AMBIENT_ALL,
                "Imp" => class::AMBIENT_IMP,
                _ => continue,
            };
            row[at] = if cid != usize::MAX {
                class_rows.get(cid).map_or(-1., |r| r[ambient])
            } else {
                -1.
            };
        }
        row[post::PRESENT] = 0.;
        row[post::ACTIVE] = 0.;
        row[post::BUILDING_ALIVE] = 1.;
        for k in 0..3 {
            row[post::X + k] = meta
                .and_then(|m| m.get("position"))
                .and_then(|p| p.at(k))
                .map_or(0., |v| number(Some(v)));
        }
        for listener in 1..=2 {
            for owner in 1..=2 {
                for (base, suffix) in [(post::INFO11, "Info"), (post::CAPTURE11, "Capture"), (post::LOST11, "Lost")] {
                    let key_name = format!("VO_{}_{}{}", sides[listener], sides[owner], suffix);
                    row[base + (listener - 1) * 2 + owner - 1] =
                        resolve(meta.and_then(|m| m.get("vo")).and_then(|v| v.get(&key_name)));
                }
            }
        }
        posts.push((slot + 1, row));
        post_building.push(!name.starts_with('#') && objectives.contains(&name.as_str()));
    }

    let stations = bfe_sim::world::admit::rows::props::live_sources(&content.content.battle)
        .into_iter()
        .map(|source| {
            let desc = &instances[source];
            let cls = lookup_class(text(desc.get("class")));
            (
                cls,
                std::array::from_fn(|axis| number(desc.get("matrix").and_then(|m| m.at(12 + axis)))),
                text(desc.get("base")) == "powerupstation",
            )
        })
        .collect();

    // Register placed map objects that carry a `BuildingSound` persistent ambient loop
    // or a `DamageEffectSound` destruction cue.
    let placed_list = content
        .content
        .battle
        .map_data
        .get("placedObjects")
        .and_then(Value::as_array)
        .or_else(|| content.content.battle.map_data.get("instances").and_then(Value::as_array))
        .or_else(|| (!instances.is_empty()).then_some(instances));
    if let Some(placed) = placed_list {
        for obj in placed {
            let cls_name = text(obj.get("class").or_else(|| obj.get("type")));
            let cid = lookup_class(cls_name);
            if cid == usize::MAX {
                continue;
            }
            let has_building_audio = runtime
                .phase
                .swbf2()
                .and_then(|s| s.classes.get(cid))
                .is_some_and(|c| c.building_sound != NONE || c.damage_effect_sound != NONE);
            if has_building_audio {
                let pos: [f64; 3] = if let Some(m) = obj.get("matrix") {
                    std::array::from_fn(|axis| number(m.at(12 + axis)))
                } else if let Some(p) = obj.get("position") {
                    std::array::from_fn(|axis| number(p.at(axis)))
                } else {
                    continue;
                };
                if pos.iter().all(|v| v.is_finite()) {
                    let name_hash = zero_hash(text(obj.get("name")));
                    let swbf2 = runtime.phase.swbf2_mut().unwrap();
                    let idx = swbf2.vehicles.buildings.len();
                    swbf2.vehicles.buildings.push(BuildingEmitter {
                        name_hash,
                        class: cid,
                        position: pos,
                        alive: true,
                        damaged: false,
                        in_range: false,
                        voice: audio_control::engine::SILENT,
                        retry: Default::default(),
                    });
                    swbf2.register_building_hash(name_hash, idx);
                }
            }
        }
    }
    runtime.phase.swbf2_mut().unwrap().finalize_lookups();

    let streams = super::stream_assets::Assets::prepare(&mut runtime.phase.streams, segments, variants);
    Ok(Prepared {
        runtime,
        output,
        actor_classes,
        weapon_classes,
        combat_classes,
        stations,
        posts,
        post_building,
        voice_segments,
        prediction_weapons,
        actor_limits,
        music,
        streams,
        enhanced,
        enhanced_base,
        headphones,
        sound_names,
        spaces,
        space_boxes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_section_class_inherits_overrides_and_keeps_multi_props() {
        let side_table = bfe_content::json::parse(
            r#"{
                "wpnc": {
                    "base_blaster": {
                        "properties": [
                            ["FireSound", "snd_base_fire"],
                            ["OverheatSound", "snd_base_heat"],
                            ["OrdnanceName", "base_bolt"]
                        ]
                    },
                    "child_blaster": {
                        "base": "base_blaster",
                        "properties": [
                            ["FireSound", "snd_child_fire"]
                        ]
                    }
                },
                "entc": {
                    "base_craft": {
                        "properties": [
                            ["BoostSound", "snd_boost_hi 0.8 1"],
                            ["DeathSound", "snd_craft_die"]
                        ]
                    },
                    "child_craft": {
                        "base": "base_craft",
                        "properties": [
                            ["BoostSound", "snd_boost_lo 0.2 0"]
                        ]
                    },
                    "cycle_a": { "base": "cycle_b", "properties": [["DeathSound", "snd_a"]] },
                    "cycle_b": { "base": "cycle_a", "properties": [["HurtSound", "snd_b"]] }
                }
            }"#,
        )
        .unwrap();

        let flat_wpn = flatten_section_class(&[side_table.clone()], "wpnc", "child_blaster").unwrap();
        assert_eq!(text(flat_wpn.get("firesound")), "snd_child_fire");
        assert_eq!(text(flat_wpn.get("overheatsound")), "snd_base_heat");
        assert_eq!(text(flat_wpn.get("ordnancename")), "base_bolt");

        let flat_craft = flatten_section_class(&[side_table.clone()], "entc", "child_craft").unwrap();
        assert_eq!(text(flat_craft.get("deathsound")), "snd_craft_die");
        let boosts = value_list(flat_craft.get("boostsound"));
        assert_eq!(boosts.len(), 2);
        assert_eq!(text(Some(boosts[0])), "snd_boost_hi 0.8 1");
        assert_eq!(text(Some(boosts[1])), "snd_boost_lo 0.2 0");

        let flat_cycle = flatten_section_class(&[side_table], "entc", "cycle_a").unwrap();
        assert_eq!(text(flat_cycle.get("deathsound")), "snd_a");
        assert_eq!(text(flat_cycle.get("hurtsound")), "snd_b");

        assert_eq!(strip_class_section_prefix("ordc/test_bolt extra"), "test_bolt");
        assert_eq!(strip_class_section_prefix("expc/test_boom"), "test_boom");
        assert_eq!(strip_class_section_prefix("plain_class"), "plain_class");
    }

    #[test]
    fn parse_vo_rules_retains_all_authored_conditions() {
        let mut names = BTreeMap::new();
        names.insert(key("synth_vo_base"), 1);
        names.insert(key("synth_vo_pair"), 2);
        names.insert(key("synth_vo_triple"), 3);

        let val = Value::Array(Rc::new(vec![
            Value::str("synth_vo_base SynthEvent"),
            Value::str("synth_vo_pair SynthEvent +TagOne +TagTwo"),
            Value::str("synth_vo_triple SynthEvent +TagOne +TagTwo +TagThree"),
        ]));
        let rules = parse_vo_rules(&names, Some(&val));
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].sound, 1);
        assert!(rules[0].conditions().is_empty());
        assert_eq!(rules[1].sound, 2);
        assert_eq!(rules[1].conditions(), &[hash("TagOne"), hash("TagTwo")]);
        assert_eq!(rules[2].sound, 3);
        assert_eq!(rules[2].conditions(), &[hash("TagOne"), hash("TagTwo"), hash("TagThree")]);
        assert!(!rules[2].matches(hash("SynthEvent"), &[hash("TagOne"), hash("TagTwo")]));
        assert!(rules[2].matches(hash("SynthEvent"), &[hash("TagThree"), hash("TagOne"), hash("TagTwo")]));
    }

    #[test]
    fn parse_vo_rules_rejects_rules_exceeding_condition_limit() {
        let mut names = BTreeMap::new();
        names.insert(key("synth_vo_eight"), 1);
        names.insert(key("synth_vo_nine"), 2);

        let val = Value::Array(Rc::new(vec![
            Value::str("synth_vo_eight SynthEvent +T1 +T2 +T3 +T4 +T5 +T6 +T7 +T8 +T1"),
            Value::str("synth_vo_nine SynthEvent +T1 +T2 +T3 +T4 +T5 +T6 +T7 +T8 +T9"),
        ]));
        let rules = parse_vo_rules(&names, Some(&val));
        assert_eq!(rules.len(), 1, "oversized 9-condition rule must be skipped rather than truncated");
        assert_eq!(rules[0].sound, 1);
        assert_eq!(rules[0].conditions().len(), audio_control::swbf2::MAX_VO_CONDITIONS);
    }
}
