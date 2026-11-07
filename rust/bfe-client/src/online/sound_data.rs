//! SoundData.load and ControlResources.prepare resolve authored resource names
//! before a join opens its socket. Runtime owns every mutable audio decision.
use audio::output::Output;
use audio_control::{layout::*, runtime::Runtime};
use bfe_content::{Object, Value};
use bfe_content_native::BattleContent;
use std::{collections::BTreeMap, rc::Rc};
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
fn hash(s: &str) -> u32 {
    s.encode_utf16().fold(0x811c9dc5u32, |h, c| (h ^ ((c as u32 & 255) | 32)).wrapping_mul(0x1000193))
}
fn key(s: &str) -> String {
    if s.len() == 10 && s.starts_with("0x") && s[2..].bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        s.into()
    } else {
        format!("0x{:08x}", hash(s))
    }
}
// SoundData objects retain their authored property order and identity.
// Inherit is not a final object spread: fields before and after that key
// participate at different positions. Alias names must resolve to the same
// resource, while two separately authored equal objects remain distinct.
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
            let names = names.and_then(Value::as_array).map_or_else(|| names.into_iter().collect::<Vec<_>>(), |a| a.iter().collect());
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
    // SoundData.resolve follows the table for this authored resource kind.
    // Hash aliases and original names resolve before mutable playback starts.
    // A missing optional property keeps its missing id instead of selecting
    // a different sound whose name merely resembles the requested one.
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
fn class_value<'a>(o: &'a Value, key: &str) -> Option<&'a Value> {
    if let Some(v) = o.get(key).filter(|v| v.truthy()) {
        return Some(v);
    }
    let aliases: &[&str] = match key {
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
    aliases.iter().find_map(|k| o.get(k).filter(|v| v.truthy()))
}
fn sound_id(names: &BTreeMap<String, usize>, v: Option<&Value>) -> f64 {
    let name = text(first(v)).split_whitespace().next().unwrap_or("");
    if name.is_empty() {
        -1.
    } else {
        names.get(&key(name)).map_or(-1., |id| *id as f64)
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
    if let Some(id) = tracks.get(name) {
        return Ok(*id as f64);
    }
    let id = tracks.len();
    tracks.insert(name.into(), id);
    let object = data.object("Music", name);
    let mut row = [f64::NAN; track::STRIDE];
    row[track::STREAM] = if let Some(value) = object.and_then(|o| o.get("SoundStream").or_else(|| o.get("soundstream"))) {
        sound_id(names, Some(value))
    } else {
        sound_id(names, Some(&Value::str(name)))
    };
    for (at, key, alias, default) in [
        (track::MIN_PLAY, "MinPlaybackTime", "6813be83", 0.),
        (track::MIN_IDLE, "MinInactiveTime", "35724887", 0.),
        (track::PRIORITY, "Priority", "", 0.),
        (track::FADE_IN, "FadeInTime", "", 1.),
        (track::FADE_OUT, "FadeOutTime", "", 1.),
    ] {
        row[at] = object.and_then(|o| field(o, key).or_else(|| field(o, alias))).map_or(default, |v| number(Some(v)));
    }
    import(runtime, import::TRACK, &row)?;
    Ok(id as f64)
}
// ControlResources.prepare turns authored names into immutable numeric rows.
// All string maps, bank reads, stream indices and sample allocations are
// admission work. Runtime receives class ids and metadata; Output receives
// PCM and rates. Neither performs authored-table lookups during playback.
pub struct Prepared {
    pub runtime: Runtime,
    pub output: Output,
    pub actor_classes: Vec<usize>,
    pub weapon_classes: Vec<usize>,
    pub prediction_weapons: Vec<([f64; weapon::STRIDE], usize)>,
    pub actor_limits: Vec<[f64; 3]>,
    pub streams: super::stream_assets::Assets,
    pub music: Vec<[f64; music_slot::STRIDE]>,
}
impl Prepared {
    pub fn new(content: &BattleContent) -> Result<Self, String> {
        // Select the mission bank through the prepared audio index, just as
        // SoundData.load does. A server supplies a mission query, never an
        // audio path or code. Every byte below comes from installed content.
        // Missing mandatory bank data fails before a live socket is opened.
        let index: serde_json::Value = serde_json::from_slice(&content.textures.read("audio/index.json")?).map_err(|e| e.to_string())?;
        let mission = content.content.battle.world.get("mission").and_then(|m| m.get("name")).and_then(Value::as_str).unwrap_or("");
        let map = content.content.battle.world.get("name").and_then(Value::as_str).unwrap_or("");
        let level = index["missions"][mission].as_str().or_else(|| index["maps"][map].as_str()).ok_or("battle has no sound bank")?;
        // Ordered JSON retains the position of Inherit inside each object.
        // Sorting its properties would change which authored values survive.
        let manifest = content.textures.read(&format!("audio/{level}/sound.json"))?;
        let manifest = bfe_content::json::parse(std::str::from_utf8(&manifest).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let data = Data::new(manifest)?;
        let pcm = content.textures.read(&format!("audio/{level}/bank.bin"))?;
        let mut runtime = Runtime::new(1);
        let mut output = Output::new(48000., 2, false, false, false, false, cfg!(target_arch = "x86_64"));
        output.reserve_controls(65536);
        for slot in 0..audio::output::VOICES {
            output.voice(slot, true);
        }
        output.layout(0, None, &[Some((-std::f64::consts::FRAC_PI_4, 0.)), Some((std::f64::consts::FRAC_PI_4, 0.))], -1, -1, 1.);
        output.volume(1., 0., 0., true);
        let mut buses = BTreeMap::new();
        for (name, _) in data.table("Bus") {
            let id = buses.len();
            buses.insert(name.clone(), id);
        }
        for (name, id) in data.table("Bus") {
            let o = &data.objects[*id].data;
            let mut row = [f64::NAN; bus::STRIDE];
            row[bus::GAIN] = field(o, "Gain").map_or(1., |v| number(Some(v)));
            row[bus::PARENT] = buses.get(text(o.get("Parent"))).filter(|_| text(o.get("Parent")) != name).map_or(-1., |i| *i as f64);
            row[bus::TARGET] = buses.get(text(o.get("AttenuateBus"))).map_or(-1., |i| *i as f64);
            for (at, k, default) in
                [(bus::AMOUNT, "Attenuation", 0.5), (bus::LEAD_IN, "AttenuationLeadIn", 0.5), (bus::LEAD_OUT, "AttenuationLeadOut", 0.5)]
            {
                row[at] = field(o, k).map_or(default, |v| number(Some(v)));
            }
            import(&mut runtime, import::BUS, &row)?;
        }
        let mut samples = BTreeMap::new();
        if let Some(table) = data.manifest.get("samples").and_then(Value::as_object) {
            for (name, v) in table.entries() {
                let id = samples.len();
                samples.insert(name.to_string(), id);
                let at = number(v.at(0)) as usize;
                let frames = number(v.at(1)) as usize;
                let rate = number(v.at(2));
                let bytes = pcm
                    .get(at..at.checked_add(frames.checked_mul(2).ok_or("sample length overflow")?).ok_or("sample length overflow")?)
                    .ok_or("sample exceeds PCM bank")?;
                let target = output.voices.sample_storage(id, frames, rate.max(8000.));
                for (to, b) in target.iter_mut().zip(bytes.chunks_exact(2)) {
                    *to = i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.;
                }
                import(&mut runtime, import::SAMPLE, &[id as f64, frames as f64 / rate, rate.max(8000.), rate / rate.max(8000.), 1.])?;
            }
        }
        let mut stream_ids = BTreeMap::new();
        let mut segment_ids = BTreeMap::new();
        let mut segments = Vec::new();
        for stream_name in data.manifest.get("streams").and_then(Value::as_array).unwrap_or(&[]) {
            let name = text(Some(stream_name));
            let Ok(raw) = content.textures.read(&format!("audio/streams/{name}.json")) else {
                continue;
            };
            let ordered = bfe_content::json::parse_bytes(&raw).map_err(|e| e.to_string())?;
            let index: serde_json::Value = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
            let id = stream_ids.len();
            stream_ids.insert(key(name), id);
            let channels = index["channels"].as_u64().unwrap_or(1) as usize;
            let substreams = index["substreams"].as_u64().unwrap_or(1) as usize;
            let interleave = index["substreamInterleave"].as_u64().unwrap_or(0) as usize;
            if channels == 0 || substreams == 0 || channels * substreams > 12 || substreams > 1 && interleave == 0 {
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
            for (key, _) in ordered.get("segments").and_then(Value::as_object).into_iter().flat_map(|o| o.entries()) {
                let entry = &index["segments"][key];
                let offset = entry[0].as_u64().ok_or("invalid stream offset")?;
                let size = entry[1].as_u64().ok_or("invalid stream length")? as usize;
                let rate = entry[3].as_f64().ok_or("invalid stream rate")?;
                if !(8000. ..=192000.).contains(&rate) || size > 256 << 20 {
                    return Err("stream segment exceeds bounds".into());
                }
                let chunk = interleave * substreams;
                let length = if substreams > 1 && chunk > 0 { size.div_ceil(chunk) * chunk } else { size };
                let file = content.textures.path(&format!("audio/streams/{}.bin", index["name"].as_str().unwrap_or(name)))?;
                let segment = segments.len();
                segment_ids.insert((id, key.to_string()), segment);
                let mut row = [-1.; segment::STRIDE];
                row[segment::STREAM] = id as f64;
                row[segment::SAMPLE] = segment as f64;
                row[segment::DURATION] = entry[2].as_f64().unwrap_or(0.) / substreams as f64 / rate;
                row[segment::CHANNELS] = (channels * substreams) as f64;
                import(&mut runtime, import::SEGMENT, &row)?;
                segments.push(super::stream_assets::Segment { file, offset, length, rate, channels, substreams, interleave });
            }
        }
        output.voices.reserve_samples(samples.len() + 96);
        let mut names = BTreeMap::new();
        let mut ids = BTreeMap::new();
        let mut sounds = Vec::new();
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
                import(
                    &mut runtime,
                    import::NAME,
                    &[u32::from_str_radix(name.trim_start_matches("0x"), 16).unwrap_or_else(|_| hash(name)) as f64, id as f64],
                )?;
            }
        }
        let mut parameterized = BTreeMap::new();
        for (name, object) in data.table("SoundParameterized") {
            if data.object("SoundLayered", text(data.objects[*object].data.get("SoundLayered"))).is_none() {
                continue;
            }
            let id = *ids.entry(*object).or_insert_with(|| {
                let id = sounds.len();
                sounds.push(*object);
                id
            });
            parameterized.insert(name.clone(), id);
        }
        let resolve = |v: Option<&Value>| -> f64 {
            let v = first(v);
            let name = text(v).split_whitespace().next().unwrap_or("");
            if name.is_empty() {
                -1.
            } else {
                names.get(&key(name)).map_or(-1., |n| *n as f64)
            }
        };
        let mut entry_count = 0;
        let mut binding_count = 0;
        let mut point_count = 0;
        for (sound_id, object) in sounds.into_iter().enumerate() {
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
            if let Some(entries) = o.get(if row[sound::KIND] == 4. { "SegmentList" } else { "SampleList" }).and_then(Value::as_array) {
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
            // Layer.children supplies immutable parameter graphs. Runtime
            // performs their interpolation, delays and mutable cycle gates.
            let layered = if kind == "SoundParameterized" {
                data.object("SoundLayered", text(o.get("SoundLayered")))
            } else if kind == "SoundLayered" {
                Some(o)
            } else {
                None
            };
            for source in layered.and_then(|o| o.get("Layer")).and_then(Value::as_array).unwrap_or(&[]) {
                let mut row = [f64::NAN; layer::STRIDE];
                row[layer::PARENT] = sound_id as f64;
                row[layer::SOUND] = resolve(source.at(1));
                row[layer::DELAY] = source.at(2).map_or(0., |v| number(Some(v)));
                row[layer::DEVIATION] = source.at(3).map_or(0., |v| number(Some(v)));
                row[layer::BINDING_FIRST] = binding_count as f64;
                if kind == "SoundParameterized" {
                    for child in o.get("Layer.children").and_then(Value::as_array).unwrap_or(&[]) {
                        if text(first(child.get("Name"))) != text(source.at(0)) {
                            continue;
                        }
                        let graph = child.get("ParameterGraph");
                        let input = text(graph.and_then(|g| g.at(0)));
                        let mut b = [f64::NAN; binding::STRIDE];
                        b[binding::INPUT] = ["Speed", "Acceleration", "Health", "TurnOnTime", "TurnOffTime", "WindUpTime", "WindDownTime"]
                            .iter()
                            .position(|n| key(n) == input)
                            .map_or(-1., |i| i as f64);
                        let output = text(graph.and_then(|g| g.at(2)));
                        b[binding::OUTPUT] = if output == key("Gain") {
                            0.
                        } else if output == key("Pitch") {
                            1.
                        } else {
                            -1.
                        };
                        b[binding::FIRST] = point_count as f64;
                        for point in data
                            .object("ParameterGraph", text(graph.and_then(|g| g.at(1))))
                            .and_then(|g| g.get("ControlPoint"))
                            .and_then(Value::as_array)
                            .unwrap_or(&[])
                        {
                            import(&mut runtime, import::POINT, &[number(point.at(0)), number(point.at(1))])?;
                            point_count += 1;
                        }
                        b[binding::COUNT] = point_count as f64 - b[binding::FIRST];
                        import(&mut runtime, import::BINDING, &b)?;
                        binding_count += 1;
                    }
                }
                row[layer::BINDING_COUNT] = binding_count as f64 - row[layer::BINDING_FIRST];
                import(&mut runtime, import::LAYER, &row)?;
            }
        }
        // Audio class ids belong to the sound manifest, not the network's
        // class or weapon registries. Keep those namespaces separate until
        // the cold definition joins below. Sharing their numeric values by
        // accident works for some maps and plays unrelated sounds on others.
        let mut classes = BTreeMap::<String, usize>::new();
        if let Some(table) = data.manifest.get("classes").and_then(Value::as_object) {
            for (name, o) in table.entries() {
                let id = classes.len();
                classes.insert(name.into(), id);
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
                    row[at] = if at == class::ENGINE {
                        let name = text(first(class_value(o, k))).split_whitespace().next().unwrap_or("");
                        parameterized.get(&key(name)).map_or_else(|| resolve(class_value(o, k)), |id| *id as f64)
                    } else {
                        resolve(class_value(o, k))
                    };
                }
                for (side, at) in
                    [("Rep", class::AMBIENT_REP), ("Cis", class::AMBIENT_CIS), ("All", class::AMBIENT_ALL), ("Imp", class::AMBIENT_IMP)]
                {
                    let values = o.get("AmbientSound");
                    let list =
                        values.and_then(Value::as_array).map_or_else(|| values.into_iter().collect::<Vec<_>>(), |a| a.iter().collect());
                    row[at] = list
                        .iter()
                        .find_map(|v| {
                            let mut parts = text(Some(v)).split_whitespace();
                            parts
                                .next()
                                .filter(|n| n.eq_ignore_ascii_case(side))
                                .map(|_| parts.next().and_then(|name| names.get(&key(name))).map_or(-1., |id| *id as f64))
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
                    row[at] = number(if [class::TURN_OFF_TIME, class::FOOTSTEP_STRIDE, class::FIRE_STOP].contains(&at) {
                        o.get(k)
                    } else {
                        first(o.get(k))
                    });
                }
                import(&mut runtime, import::CLASS, &row)?;
            }
        }
        // Mission rows describe alternatives. The shared director alone
        // selects their priority, ticket stage, repeat interval and fades.
        let mission = data.manifest.get("mission");
        let mut tracks = BTreeMap::new();
        let mut music = Vec::new();
        for (team, list) in mission.and_then(|m| m.get("music")).and_then(Value::as_object).into_iter().flat_map(|o| o.entries()) {
            let Ok(team) = team.parse::<usize>() else {
                continue;
            };
            if team > 2 {
                continue;
            }
            for source in list.as_array().unwrap_or(&[]) {
                let id = track(&data, &names, &mut tracks, &mut runtime, source.at(1))?;
                let mut row = [0.; music_slot::STRIDE];
                row[music_slot::TEAM] = team as f64;
                row[music_slot::STAGE] = number(source.at(2));
                row[music_slot::TRACK] = id;
                row[music_slot::AMOUNT] = number(source.at(0));
                row[music_slot::FRACTION] = source.at(3).map_or(0., |v| number(Some(v)));
                music.push(row);
            }
        }
        for (name, value) in mission.and_then(|m| m.get("effects")).and_then(Value::as_object).into_iter().flat_map(|o| o.entries()) {
            import(&mut runtime, import::EFFECT, &[hash(name) as f64, resolve(Some(value))])?;
        }
        // World emitters and sound regions carry authored bounds. Their
        // distance, gates and current voices remain native director state.
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
        for source in world.and_then(|w| w.get("regions")).and_then(Value::as_array).unwrap_or(&[]) {
            let mut parts = text(source.get("name")).split_whitespace();
            if !parts.next().is_some_and(|s| s.eq_ignore_ascii_case("soundstream")) {
                continue;
            }
            let name = parts.next().unwrap_or("");
            let mut row = [f64::NAN; region::STRIDE];
            row[region::SOUND] = sound_id(&names, Some(&Value::str(name)));
            for k in 0..3 {
                row[region::X + k] = number(source.get("matrix").and_then(|m| m.at(12 + k)));
                row[region::SIZE_X + k] = number(source.get("size").and_then(|m| m.at(k)));
            }
            row[region::SCALE] = parts.next().map_or(0., |v| v.parse::<f64>().unwrap_or(f64::NAN));
            import(&mut runtime, import::REGION, &row)?;
        }
        let mut config = [-1.; config::STRIDE];
        config[config::MASTER_BUS] = buses.get("0xeceb308a").map_or(-1., |n| *n as f64);
        config[config::PAUSE_BUS] = buses.get(&key("ingame")).map_or(-1., |n| *n as f64);
        config[config::DEVICE_MEMORY] = 4.;
        config[config::SAMPLE_BASE] = samples.len() as f64;
        config[config::SPACE_SEND] = 0.;
        config[config::OWN_SCOPE_IN] = resolve(mission.and_then(|m| m.get("effects")).and_then(|e| e.get("ScopeDisplayZoomIn")));
        config[config::OWN_SCOPE_OUT] = resolve(mission.and_then(|m| m.get("effects")).and_then(|e| e.get("ScopeDisplayZoomOut")));
        for team in 1..=2 {
            let key = team.to_string();
            config[if team == 1 { config::VICTORY1 } else { config::VICTORY2 }] =
                track(&data, &names, &mut tracks, &mut runtime, mission.and_then(|m| m.get("victory")).and_then(|m| m.get(&key)))?;
            config[if team == 1 { config::DEFEAT1 } else { config::DEFEAT2 }] =
                track(&data, &names, &mut tracks, &mut runtime, mission.and_then(|m| m.get("defeat")).and_then(|m| m.get(&key)))?;
            config[if team == 1 { config::BOUNDS1 } else { config::BOUNDS2 }] =
                resolve(mission.and_then(|m| m.get("outOfBounds")).and_then(|m| m.get(&key)));
        }
        let space = data.table("space").last().map(|(_, id)| &data.objects[*id].data);
        let preset =
            space.and_then(|s| s.get("I3DL2ReverbPreset")).and_then(Value::as_str).and_then(|name| data.object("I3DL2ReverbPreset", name));
        config[config::SPACE_SEND] = space.and_then(|s| field(s, "RoomGain")).map_or(0., |v| number(Some(v)));
        let mut room = [f64::NAN; room::STRIDE];
        for (at, key, default) in [
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
            room[at] = preset.and_then(|p| field(p, key)).map_or(default, |v| number(Some(v)));
        }
        room[room::ROOM_HF] = room[room::ROOM_HF].max(0.001);
        room[room::DIFFUSION] = room[room::DIFFUSION].clamp(0., 1.);
        room[room::DENSITY] = 1.;
        room[room::MASK] = if preset.is_some() { 2047. } else { 1. };
        import(&mut runtime, import::ROOM, &room)?;

        import(&mut runtime, import::CONFIG, &config)?;
        let defs = bfe_content::defs::Defs::build(&content.content)?;
        // Prediction ids follow the same infantry, vehicle-seat and turret
        // definition order as ClientPrediction admission. Mutable weapon
        // timers are later borrowed from that owner's generated publication.
        let mut prediction_weapons = Vec::new();
        let mut weapon_meta = |record: &Value| {
            let mut row = [0.; weapon::STRIDE];
            row[weapon::CLASS] = classes.get(text(record.get("class"))).map_or(-1., |id| *id as f64);
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
        // Hull speed and actor health are observation prerequisites used by
        // the native sound owner. Resolve class defaults here, beside the
        // other immutable class metadata; displayed frames must not parse
        // settings or recreate a definition to detect an engine transition.
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
        // The wire registries are one-based and the audio manifest is not.
        // Missing sound classes retain an invalid sentinel, preserving silent
        // optional resources. The frame adapter performs only checked numeric
        // lookup into these tables, including sparse class and weapon ids.
        let actor_classes = defs.registry.classes.iter().map(|name| classes.get(name).copied().unwrap_or(usize::MAX)).collect();
        let weapon_classes = defs.registry.weapons.iter().map(|name| classes.get(name).copied().unwrap_or(usize::MAX)).collect();
        Ok(Self {
            runtime,
            output,
            actor_classes,
            weapon_classes,
            prediction_weapons,
            actor_limits,
            music,
            streams: super::stream_assets::Assets::new(segments),
        })
    }
}
