//! Fixed-seed headless audio regression harness for 2004 battles.
//! Drives real 60 Hz battles through `sound_data::Prepared`, `audio_control::Runtime`,
//! deterministic synchronous stream loading, and `audio::output::Output` at 48 kHz stereo.
//! Needs prepared game files:
//! `BFE_CONTENT_DIR=... cargo test --release -p bfe-client --test audio_regression -- --ignored --nocapture`
use bfe_wide::Wide;
use audio::{output::Output, voices::Sample, FRAMES};
use audio_control::{layout::*, Collision as SoundCollision, Hit, Point};
use bfe_client::generated_input::{ClientCommand, ClientFlightCommand, ClientLook};
use bfe_client::local::{launch::Prepared as BattlePrepared, options::Options, runtime::Runtime as BattleRuntime};
use bfe_client::online::{
    receiver::Receiver,
    replay::Replay,
    sound_data::{Prepared as SoundPrepared, SpaceBox},
    sound_output::{build as build_output, AudioSettings},
};
use bfe_content::Value;
use bfe_content_native::setup::admission::{CLOCK_START_MS, CLOCK_UNIX_MS};
use bfe_content_native::Loader;
use bfe_presentation::network_layout::{net_kind as k, net_record as r};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
};

const OWN: usize = bfe_presentation::network_layout::net_limits::SLOTS + 1;
const FRAMES_PER_TICK: usize = 800; // 48000 Hz / 60 Hz

struct Queries {
    collision: bfe_client::online::replay::Collision,
    world: u32,
}
impl SoundCollision for Queries {
    fn cast(&mut self, from: Point, d: Point, far: f64) -> Option<Hit> {
        let ray = bfe_sim::collision::Ray { ox: from[0], oy: from[1], oz: from[2], dx: d[0], dy: d[1], dz: d[2] };
        self.collision.borrow_mut().ray_hit(self.world, ray, far, -1).map(|(code, h)| Hit {
            distance: h.distance,
            point: [h.px, h.py, h.pz],
            terrain: code == bfe_sim::collision::TERRAIN,
        })
    }
}

struct SyncSegment {
    file: PathBuf,
    offset: u64,
    length: usize,
    rate: f64,
    channels: usize,
    substreams: usize,
    interleave: usize,
}

impl SyncSegment {
    fn load(&self) -> Option<(Sample, usize, usize, f64)> {
        let mut file = File::open(&self.file).ok()?;
        file.seek(SeekFrom::Start(self.offset)).ok()?;
        let mut bytes = vec![0u8; self.length];
        file.read_exact(&mut bytes).ok()?;
        let count = self.substreams.max(1);
        let parts = if count == 1 {
            vec![bytes]
        } else {
            if self.interleave == 0 {
                return None;
            }
            let mut parts = vec![Vec::with_capacity(bytes.len() / count + self.interleave); count];
            for (chunk, run) in bytes.chunks_exact(self.interleave).enumerate() {
                parts[chunk % count].extend_from_slice(run);
            }
            parts
        };
        let sample = Sample::adpcm(self.rate, self.channels, parts)?;
        let frames = sample.frames;
        let channels = sample.channels;
        Some((sample, frames, channels, self.rate))
    }
}

fn load_sync_segments(content: &bfe_content_native::BattleContent) -> Result<Vec<SyncSegment>, String> {
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
        .ok_or("battle has no sound bank")?;
    let raw = content.textures.read(&format!("audio/{level}/sound.json"))?;
    let manifest = bfe_content::json::parse(std::str::from_utf8(&raw).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let mut segments = Vec::new();
    for stream_name in manifest.get("streams").and_then(Value::as_array).unwrap_or(&[]) {
        let name = stream_name.as_str().unwrap_or("");
        let Ok(raw) = content.textures.read(&format!("audio/streams/{name}.json")) else {
            continue;
        };
        let ordered = bfe_content::json::parse_bytes(&raw).map_err(|e| e.to_string())?;
        let idx: serde_json::Value = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
        let channels = idx["channels"].as_u64().unwrap_or(1) as usize;
        let substreams = idx["substreams"].as_u64().unwrap_or(1) as usize;
        let interleave = idx["substreamInterleave"].as_u64().unwrap_or(0) as usize;
        let bin_name = idx["name"].as_str().unwrap_or(name);
        let file = content.textures.path(&format!("audio/streams/{bin_name}.bin"))?;
        for (key, _) in ordered.get("segments").and_then(Value::as_object).into_iter().flat_map(|o| o.entries()) {
            let entry = &idx["segments"][key];
            let offset = entry[0].as_u64().ok_or("invalid offset")?;
            let size = entry[1].as_u64().ok_or("invalid length")? as usize;
            let rate = entry[3].as_f64().ok_or("invalid rate")?;
            let chunk = interleave * substreams;
            let length = if substreams > 1 && chunk > 0 { size.div_ceil(chunk) * chunk } else { size };
            segments.push(SyncSegment { file: file.clone(), offset, length, rate, channels, substreams, interleave });
        }
    }
    Ok(segments)
}

fn zhash(s: &str) -> u32 {
    s.encode_utf16().fold(0x811c9dc5u32, |h, c| (h ^ ((c as u32 & 255) | 32)).wrapping_mul(0x1000193))
}

fn key_hash(s: &str) -> u32 {
    if s.len() == 10 && s.starts_with("0x") && s[2..].bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        u32::from_str_radix(&s[2..], 16).unwrap_or_default()
    } else {
        zhash(s)
    }
}

fn listener_space(boxes: &[SpaceBox], current: Option<usize>, ear: [f64; 3]) -> Option<usize> {
    if let Some(space) = current {
        if boxes.iter().any(|b| b.space == space && b.holds(ear)) {
            return current;
        }
    }
    boxes.iter().find(|b| b.holds(ear)).map(|b| b.space)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventInput<'a> {
    kind: &'a str,
    #[serde(default)]
    owner_id: u32,
    #[serde(default)]
    owner_generation: u32,
    #[serde(default)]
    weapon_id: usize,
    #[serde(default)]
    channel: usize,
    #[serde(default)]
    position: Option<[f64; 3]>,
    #[serde(default)]
    repair: bool,
    #[serde(default)]
    definition_id: usize,
    #[serde(default, borrow)]
    property: std::borrow::Cow<'a, str>,
    #[serde(default = "yes")]
    positional: bool,
    #[serde(default = "no_building")]
    building: usize,
}
fn yes() -> bool { true }
fn no_building() -> usize { usize::MAX }

fn unit_row(receiver: &Receiver, id: u32, generation: u32) -> Option<&[f64; r::STRIDE]> {
    receiver.active().find(|row| row[r::ID] == id as f64 && row[r::GENERATION] == generation as f64)
}

fn retire(phase: &mut audio_control::phase::Phase, id: usize) {
    let mut emitter = phase.emitters[id];
    emitter[emitter::ID] = id as f64;
    emitter[emitter::KIND] = 0.;
    phase.observe_emitter(&emitter);
    let mut w = [0.; weapon::STRIDE];
    w[weapon::ID] = id as f64;
    w[weapon::OWNER] = -1.;
    phase.observe_weapon(&w);
}

fn enqueue_event(kind: usize, class: usize, position: Option<[f64; 3]>) -> [f64; event::STRIDE] {
    let mut out = [0.; event::STRIDE];
    out[event::KIND] = kind as f64;
    out[event::OWNER] = -1.;
    out[event::WEAPON] = -1.;
    out[event::CLASS] = if class == usize::MAX { -1. } else { class as f64 };
    out[event::POSITIONAL] = 1.;
    if let Some(pos) = position {
        out[event::PLACED] = 1.;
        out[event::X..event::X + 3].copy_from_slice(&pos);
    }
    out
}

fn hash_f64_slice(hasher: &mut Sha256, data: &[f64]) {
    hasher.update(Wide::from_u32(u32::try_from(data.len()).unwrap_or(u32::MAX)).to_le_bytes());
    for &v in data {
        hasher.update(v.to_bits().to_le_bytes());
    }
}

fn apply_batch(
    output: &mut Output,
    commands: &[f64],
    placements: &[f64],
    room: &[f64; audio::reverb::PARAM_COUNT],
    room_mask: u32,
) {
    if room_mask != 0 {
        if let Some(master) = &mut output.master {
            master.room(room, room_mask, false);
        }
    }
    output.commands[..commands.len()].copy_from_slice(commands);
    output.apply(commands.len());
    for row in placements.as_chunks::<6>().0 {
        let row: [f32; 6] = std::array::from_fn(|i| row[i] as f32);
        output.mixer.take(&row, false);
    }
}

fn run_map(root: &str, spec: &str, ticks: usize, enhanced: bool) {
    let parts: Vec<&str> = spec.split(':').collect();
    let map = parts[0];
    let side = parts.get(1).copied().unwrap_or("rep");
    let heroes = parts.get(2).is_some_and(|s| *s == "heroes");
    let seed = std::env::var("BFE_AUDIO_SEED").unwrap_or_else(|_| "1".into());
    let mut args: Vec<String> = ["--local", map, root, "--side", side, "--seed", &seed, "--ai", "classic"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    if heroes {
        args.push("--heroes".into());
    }
    let options = Options::parse(&args, None).unwrap().unwrap();
    let battle_prep = BattlePrepared::new(&options).unwrap();
    let query = battle_prep.admission.query.clone();
    let mut battle_rt = BattleRuntime::new(*battle_prep.engine, *battle_prep.admission, CLOCK_START_MS, CLOCK_UNIX_MS).unwrap();

    let content = Loader::detect(root).load(map, &query).unwrap();
    let replay = Replay::new(&content).unwrap();
    let mut queries = Queries { collision: replay.collision.clone(), world: replay.world };
    let segments = load_sync_segments(&content).unwrap();
    let mut sp = SoundPrepared::new(&content).unwrap();
    let settings = AudioSettings { enhanced, output: "stereo".into(), dynamics: "normal".into(), mono: false, clarity: true };
    let (mut output, rendering) = build_output(&settings, 2, 1 << 2, None, 1.0, None);
    output.take_sources(&mut sp.output);

    let mut posts = sp.posts;
    let post_building = sp.post_building;
    let mut built = vec![true; sp.stations.len()];
    let mut lives: Vec<Option<(f64, f64)>> = vec![None; OWN + 1];
    let mut pilots = std::collections::BTreeSet::<(u32, u32)>::new();
    let mut cosmetic = bfe_sim::wire::cosmetics::Compact::default();
    let dry = (0., {
        let mut r = [0.; room::STRIDE];
        r[room::ROOM_HF] = 1.;
        r[room::DECAY] = 1.;
        r[room::HF_RATIO] = 0.5;
        r[room::REFL_DELAY] = 0.02;
        r[room::REVERB_DELAY] = 0.04;
        r[room::DIFFUSION] = 1.;
        r[room::DENSITY] = 1.;
        r[room::HF_REFERENCE] = 5000.;
        r[room::MASK] = 1.;
        r
    });
    let mut cur_space: Option<usize> = None;
    let mut space_set = false;
    let mut starts_sent = false;

    let mut cmd_hasher = Sha256::new();
    let mut pcm_hasher = Sha256::new();
    let mut total_commands: u32 = 0;
    let mut total_streams: u32 = 0;
    let mut peak: f32 = 0.0;
    let mut sum_sq: f64 = 0.0;
    let mut total_samples: u32 = 0;
    let mut nonzero_samples: u32 = 0;
    let mut sample_cursor = FRAMES;

    let dt = 1.0 / 60.0;
    let mut shots = Vec::<(u32, u32, usize, usize, Option<[f64; 3]>, bool)>::with_capacity(256);

    for tick in 1..=ticks {
        battle_rt
            .step(&ClientCommand::default(), &ClientLook::default(), &ClientFlightCommand::default())
            .unwrap();
        let receiver = battle_rt.receiver();

        if !starts_sent {
            starts_sent = true;
            for row in &mut sp.music {
                row[music_slot::START] = 150.0;
                sp.runtime.input[..row.len()].copy_from_slice(row);
                sp.runtime.import(import::MUSIC_SLOT, row.len());
            }
        }

        let wall = output.frame as f64 / 48000.0;
        sp.runtime.clear_output();
        sp.runtime.events.clear();
        shots.clear();

        for field in receiver.publications.iter() {
            if field.row[r::KIND] == k::SUMMARY as f64 && field.row[r::SCHEMA] == 2. && field.length >= 23 {
                let b = &field.bytes[..field.length];
                let id = u16::from_le_bytes([b[0], b[1]]) as usize;
                if let Some((slot, (_, p))) = posts.iter_mut().enumerate().find(|(_, p)| p.0 == id) {
                    let old = p[post::TEAM];
                    let team = b[2] as f64;
                    let active = b[21] != 0;
                    let lost = p[post::ACTIVE] != 0. && !active && post_building.get(slot).copied().unwrap_or(false);
                    if p[post::PRESENT] != 0. && (team != old || lost) && sp.runtime.events.len() + 1 < sp.runtime.events.capacity() {
                        let mut e = [0.; event::STRIDE];
                        e[event::KIND] = event_kind::POST as f64;
                        e[event::OWNER] = slot as f64;
                        e[event::VALUE] = if team != old && team != 0. { team } else { old };
                        e[event::SECONDS] = if team != old && team != 0. { 0. } else { 2. };
                        sp.runtime.events.push(e);
                    }
                    p[post::PRESENT] = 1.;
                    p[post::TEAM] = team;
                    p[post::ACTIVE] = b[21] as f64;
                    p[post::CONTESTED] = b[20] as f64;
                    p[post::PROGRESS] = f64::from_le_bytes(b[4..12].try_into().unwrap());
                    p[post::NEUTRALIZE] = f64::from_le_bytes(b[12..20].try_into().unwrap());
                }
            }
            if field.row[r::KIND] == k::SUMMARY as f64 && field.row[r::SCHEMA] == 7. {
                use serde::de::IgnoredAny as Rest;
                type State<'a> = (usize, &'a str, Rest, Rest, Rest, Rest, Rest, Rest, Rest, Rest, Rest);
                if let Ok(state) = serde_json::from_slice::<State>(&field.bytes[..field.length]) {
                    if field.row[r::ID] == 200000. + state.0 as f64 {
                        if let Some(b) = built.get_mut(state.0) {
                            *b = state.1 == "built";
                        }
                    }
                }
            }
            if field.row[r::KIND] == k::COMBAT as f64 && field.row[r::SCHEMA] == 2.
                && bfe_sim::wire::cosmetics::decode_bolt_into(2., field.row[r::TICK], &field.bytes[..field.length], &mut cosmetic).is_ok()
            {
                if let Some(&class) = sp.combat_classes.get(cosmetic.definition as usize).filter(|&&c| c != usize::MAX) {
                    let kind = if cosmetic.surface == 4. { event_kind::SPLASH } else { event_kind::IMPACT };
                    if sp.runtime.events.len() + 1 < sp.runtime.events.capacity() {
                        sp.runtime.events.push(enqueue_event(kind, class, Some(cosmetic.view.position.unwrap_or_default())));
                    }
                }
            }
            if field.row[r::KIND] == k::COMBAT as f64 && field.row[r::SCHEMA] == 5. && field.length == 33 {
                let b = &field.bytes[..33];
                let definition = u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
                let pos: [f64; 3] = std::array::from_fn(|i| f64::from_le_bytes(b[8 + i * 8..16 + i * 8].try_into().unwrap()));
                if let Some(&class) = sp.combat_classes.get(definition).filter(|&&c| c != usize::MAX && definition != 0) {
                    if sp.runtime.events.len() + 1 < sp.runtime.events.capacity() {
                        sp.runtime.events.push(enqueue_event(event_kind::EXPLOSION, class, Some(pos)));
                    }
                }
            }
            if field.row[r::KIND] == k::COMBAT as f64 && field.row[r::SCHEMA] == 8. {
                if let Ok(src) = serde_json::from_slice::<EventInput>(&field.bytes[..field.length]) {
                    match src.kind {
                        "shot" => shots.push((src.owner_id, src.owner_generation, src.weapon_id, src.channel, src.position, src.repair)),
                        "vo" if src.owner_id != 0 && !src.property.is_empty() => {
                            let own = src.owner_id as f64 == receiver.own.id && src.owner_generation as f64 == receiver.own.generation;
                            let cls = if own {
                                Some(receiver.own.class)
                            } else {
                                unit_row(receiver, src.owner_id, src.owner_generation).map(|row| row[r::CLASS_ID] as usize)
                            };
                            if let Some(c) = cls.and_then(|c| c.checked_sub(1)).and_then(|i| sp.actor_classes.get(i).copied()).filter(|&c| c != usize::MAX) {
                                let mut ev = enqueue_event(event_kind::CHATTER, c, src.position);
                                ev[event::SOUND] = key_hash(&src.property) as f64;
                                ev[event::POSITIONAL] = (!own || src.positional) as u8 as f64;
                                if sp.runtime.events.len() + 1 < sp.runtime.events.capacity() {
                                    sp.runtime.events.push(ev);
                                }
                            }
                        }
                        "bounce" | "pickup" => {
                            if let (Some(&c), Some(pos)) = (sp.combat_classes.get(src.definition_id).filter(|_| src.definition_id != 0), src.position) {
                                let kind = if src.kind == "bounce" { event_kind::BOUNCE } else { event_kind::POWERUP };
                                if sp.runtime.events.len() + 1 < sp.runtime.events.capacity() {
                                    sp.runtime.events.push(enqueue_event(kind, c, Some(pos)));
                                }
                            }
                        }
                        "serve" => {
                            if let Some(&(c, pos, _)) = sp.stations.get(src.building) {
                                if sp.runtime.events.len() + 1 < sp.runtime.events.capacity() {
                                    sp.runtime.events.push(enqueue_event(event_kind::POWERUP, c, Some(pos)));
                                }
                            }
                        }
                        "towAttach" | "towRelease" => {
                            if let Some(hull) = unit_row(receiver, src.owner_id, src.owner_generation).filter(|row| row[r::KIND] == k::VEHICLE as f64) {
                                if let Some(&c) = src.weapon_id.checked_sub(1).and_then(|i| sp.weapon_classes.get(i)) {
                                    let kind = if src.kind == "towAttach" { event_kind::TOW_ATTACH } else { event_kind::TOW_RELEASE };
                                    let pos = [hull[r::POSITION], hull[r::POSITION + 1], hull[r::POSITION + 2]];
                                    if sp.runtime.events.len() + 1 < sp.runtime.events.capacity() {
                                        sp.runtime.events.push(enqueue_event(kind, c, Some(pos)));
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        let mut frame = [0.; frame::STRIDE];
        frame[frame::DT] = dt;
        frame[frame::WALL_NOW] = wall;
        frame[frame::NOW] = wall;
        frame[frame::RATE] = 48000.;
        frame[frame::ENHANCED] = rendering.enhanced as u8 as f64;
        frame[frame::LIGHT] = rendering.light as u8 as f64;
        frame[frame::HRTF] = rendering.hrtf as u8 as f64;
        frame[frame::BINAURAL] = rendering.binaural as u8 as f64;
        frame[frame::LFE] = rendering.lfe as u8 as f64;
        frame[frame::CONFIGURATION] = 1.0;
        frame[frame::COLLISION] = 1.;
        frame[frame::RUNNING] = 1.;
        frame[frame::MAX_VOICES] = 32.;
        frame[frame::VIRTUAL_VOICES] = 64.;
        let pos = receiver.own.position;
        frame[frame::PX] = pos[0];
        frame[frame::PY] = pos[1] + 1.7;
        frame[frame::PZ] = pos[2];
        frame[frame::FX] = 0.;
        frame[frame::FY] = 0.;
        frame[frame::FZ] = -1.;
        frame[frame::UX] = 0.;
        frame[frame::UY] = 1.;
        frame[frame::UZ] = 0.;
        frame[frame::TEAM] = receiver.own.team as f64;
        frame[frame::HUMAN] = 1.;
        frame[frame::EFFECTS_VOLUME] = 1.;
        frame[frame::AMBIENCE_VOLUME] = 1.;
        frame[frame::MUSIC_VOLUME] = 1.;
        frame[frame::VOICE_VOLUME] = 1.;
        frame[frame::INTERFACE_VOLUME] = 1.;
        frame[frame::CHANNELS] = rendering.channels as f64;
        frame[frame::TICKETS1] = (150.0 - (tick as f64 * 0.02)).max(10.0);
        frame[frame::TICKETS2] = (150.0 - (tick as f64 * 0.02)).max(10.0);
        frame[frame::OWN] = OWN as f64;
        sp.runtime.phase.frame.copy_from_slice(&frame);

        for (slot, (_, row)) in posts.iter().enumerate() {
            sp.runtime.phase.posts[slot] = *row;
        }
        sp.runtime.phase.post_count = posts.len();

        let mut seen = [false; OWN + 1];
        pilots.clear();
        for p in receiver.active() {
            if p[r::SEAT_PRESENT] != 0. && p[r::SEAT_INDEX] == 0. {
                pilots.insert((p[r::SEAT_VEHICLE_ID] as u32, p[r::SEAT_VEHICLE_GENERATION] as u32));
            }
        }
        if let Some((id, gen, 0)) = receiver.own.mount {
            pilots.insert((id, gen));
        }

        for (slot, row) in receiver.active_slots() {
            if row[r::ID] == receiver.own.id && row[r::GENERATION] == receiver.own.generation {
                continue;
            }
            let id = slot + 1;
            if id >= lives.len() {
                continue;
            }
            let kind = row[r::KIND] as u32;
            if kind != k::SOLDIER && kind != k::DROIDEKA && kind != k::VEHICLE {
                continue;
            }
            let life = (row[r::ID], row[r::GENERATION]);
            if lives[id] != Some(life) {
                retire(&mut sp.runtime.phase, id);
                lives[id] = Some(life);
            }
            seen[id] = true;
            let mut e = [0.; emitter::STRIDE];
            e[emitter::ID] = id as f64;
            e[emitter::KIND] = if kind == k::VEHICLE { 3. } else if kind == k::DROIDEKA { 2. } else { 1. };
            e[emitter::CLASS] = sp
                .actor_classes
                .get((row[r::CLASS_ID] as usize).wrapping_sub(1))
                .copied()
                .filter(|&i| i != usize::MAX)
                .map_or(-1., |i| i as f64);
            e[emitter::X..emitter::X + 3].copy_from_slice(&row[r::POSITION..r::POSITION + 3]);
            e[emitter::VX..emitter::VX + 3].copy_from_slice(&row[r::VELOCITY..r::VELOCITY + 3]);
            e[emitter::HEALTH] = row[r::HEALTH];
            let limits = sp.actor_limits.get(row[r::CLASS_ID] as usize).copied().unwrap_or([300., 9., 0.]);
            e[emitter::MAX_HEALTH] = limits[0];
            e[emitter::SPEED_LIMIT] = limits[1];
            e[emitter::FLYING] = limits[2];
            e[emitter::FLIGHT] = row[r::STATE];
            e[emitter::PILOTED] = pilots.contains(&(row[r::ID] as u32, row[r::GENERATION] as u32)) as u8 as f64;
            e[emitter::ALIVE] = (row[r::DEAD] == 0.) as u8 as f64;
            e[emitter::GENERATION] = row[r::GENERATION];
            e[emitter::AIR] = row[r::STATE];
            e[emitter::POSTURE] = row[r::POSTURE];
            let before = sp.runtime.phase.emitters[id];
            if before[emitter::KIND] != 0. && before[emitter::GENERATION] == e[emitter::GENERATION] && before[emitter::ALIVE] != 0. {
                let amount = before[emitter::HEALTH] - e[emitter::HEALTH];
                let dead = e[emitter::ALIVE] == 0.;
                if (amount > 0. || dead) && sp.runtime.events.len() + 1 < sp.runtime.events.capacity() {
                    let mut ev = [0.; event::STRIDE];
                    ev[event::KIND] = if dead { event_kind::ACTOR_DEATH } else { event_kind::ACTOR_HURT } as f64;
                    ev[event::OWNER] = e[emitter::ID];
                    ev[event::CLASS] = e[emitter::CLASS];
                    ev[event::AMOUNT] = amount.max(0.);
                    ev[event::HEALTH] = e[emitter::HEALTH];
                    sp.runtime.events.push(ev);
                }
            }
            sp.runtime.phase.observe_emitter(&e);
            let wcls = sp
                .weapon_classes
                .get((row[r::WEAPON] as usize).wrapping_sub(1))
                .copied()
                .filter(|&i| i != usize::MAX)
                .map_or(-1., |i| i as f64);
            let mut w = [0.; weapon::STRIDE];
            w[weapon::ID] = id as f64;
            w[weapon::OWNER] = id as f64;
            w[weapon::CLASS] = wcls;
            w[weapon::SELECTED] = 1.;
            sp.runtime.phase.observe_weapon(&w);
        }

        let own_life = (receiver.own.id, receiver.own.generation);
        if lives[OWN] != Some(own_life) {
            retire(&mut sp.runtime.phase, OWN);
            lives[OWN] = Some(own_life);
        }
        seen[OWN] = true;
        let mut own = [0.; emitter::STRIDE];
        own[emitter::ID] = OWN as f64;
        own[emitter::KIND] = if receiver.own.walker { 2. } else { 1. };
        own[emitter::CLASS] = sp
            .actor_classes
            .get(receiver.own.class.wrapping_sub(1))
            .copied()
            .filter(|&i| i != usize::MAX)
            .map_or(-1., |i| i as f64);
        own[emitter::X..emitter::X + 3].copy_from_slice(&receiver.own.position);
        own[emitter::VX..emitter::VX + 3].copy_from_slice(&receiver.own.velocity);
        own[emitter::ALIVE] = receiver.own.alive as u8 as f64;
        own[emitter::GENERATION] = receiver.own.generation;
        own[emitter::HEALTH] = receiver.own.health;
        own[emitter::MAX_HEALTH] = sp.actor_limits.get(receiver.own.class).map_or(300., |v| v[0]);
        own[emitter::MOUNTED] = receiver.own.mount.is_some() as u8 as f64;
        own[emitter::GROUNDED] = (receiver.own.air == 0) as u8 as f64;
        own[emitter::FIRST_PERSON] = 1.;
        sp.runtime.phase.observe_emitter(&own);

        for (index, &(class, position, station)) in sp.stations.iter().enumerate() {
            if !station || class == usize::MAX {
                continue;
            }
            let mut row = [0.; emitter::STRIDE];
            row[emitter::ID] = (OWN + 1 + index) as f64;
            row[emitter::KIND] = emitter_kind::STATION as f64;
            row[emitter::CLASS] = class as f64;
            row[emitter::X..emitter::X + 3].copy_from_slice(&position);
            row[emitter::ALIVE] = built[index] as u8 as f64;
            sp.runtime.phase.observe_emitter(&row);
        }
        for (id, present) in seen.into_iter().enumerate().skip(1) {
            if !present && lives[id].take().is_some() {
                retire(&mut sp.runtime.phase, id);
            }
        }
        for &(owner_id, owner_gen, weapon_id, channel, position, repair) in &shots {
            let owner = if owner_id as f64 == receiver.own.id && owner_gen as f64 == receiver.own.generation {
                Some(OWN)
            } else {
                receiver
                    .active_slots()
                    .find(|(_, row)| row[r::ID] == owner_id as f64 && row[r::GENERATION] == owner_gen as f64)
                    .map(|(slot, _)| slot + 1)
            };
            let Some(owner) = owner else { continue };
            let class = sp
                .weapon_classes
                .get(weapon_id.wrapping_sub(1))
                .copied()
                .filter(|&i| i != usize::MAX)
                .map_or(-1., |i| i as f64);
            let wid = if owner == OWN { owner + channel.min(1) } else { owner };
            let w = &mut sp.runtime.phase.weapons[wid];
            if owner != OWN {
                w.fill(0.);
            }
            w[weapon::ID] = wid as f64;
            w[weapon::OWNER] = owner as f64;
            w[weapon::CLASS] = class;
            w[weapon::SELECTED] = 1.;
            sp.runtime.phase.weapon_count = sp.runtime.phase.weapon_count.max(wid + 1);
            if let Some(pos) = position {
                sp.runtime.phase.emitters[owner][emitter::X..emitter::X + 3].copy_from_slice(&pos);
            }
            let mut ev = [0.; event::STRIDE];
            ev[event::KIND] = if owner == OWN { event_kind::FIRE } else { event_kind::ACTOR_FIRE } as f64;
            ev[event::OWNER] = owner as f64;
            ev[event::WEAPON] = wid as f64;
            ev[event::CLASS] = class;
            ev[event::VALUE] = repair as u8 as f64;
            if sp.runtime.events.len() + 1 < sp.runtime.events.capacity() {
                sp.runtime.events.push(ev);
            }
        }

        if !sp.runtime.phase.started {
            let mut start = [0.; event::STRIDE];
            start[event::KIND] = event_kind::START as f64;
            sp.runtime.events.insert(0, start);
        }
        battle_rt.receiver_mut().publications.clear();

        let p = &sp.runtime.phase.frame;
        let ear = [p[frame::PX] + 2. * p[frame::FX], p[frame::PY] + 2. * p[frame::FY], p[frame::PZ] + 2. * p[frame::FZ]];
        let space = listener_space(&sp.space_boxes, cur_space, ear);
        if space != cur_space || !space_set {
            cur_space = space;
            space_set = true;
            let (send, row) = space.map_or(dry, |id| sp.spaces[id]);
            sp.runtime.phase.space_send = send;
            sp.runtime.phase.original_room = audio_control::acoustics::Room {
                room: row[room::ROOM],
                room_hf: row[room::ROOM_HF],
                decay: row[room::DECAY],
                hf_ratio: row[room::HF_RATIO],
                reflections: row[room::REFLECTIONS],
                refl_delay: row[room::REFL_DELAY],
                reverb: row[room::REVERB],
                reverb_delay: row[room::REVERB_DELAY],
                diffusion: row[room::DIFFUSION],
                density: row[room::DENSITY],
                hf_reference: row[room::HF_REFERENCE],
            };
            sp.runtime.phase.original_room_mask = row[room::MASK] as u32;
            if !sp.runtime.phase.engine.enhanced {
                sp.runtime.phase.room_pending |= sp.runtime.phase.original_room_mask;
            }
        }

        sp.runtime.step(&mut queries);

        // Hash control command stream for this tick.
        hash_f64_slice(&mut cmd_hasher, &sp.runtime.phase.engine.commands);
        hash_f64_slice(&mut cmd_hasher, &sp.runtime.phase.engine.placements);
        let room_mask = sp.runtime.info[info::ROOM_CHANGED] as u32;
        cmd_hasher.update(room_mask.to_le_bytes());
        let room: [f64; audio::reverb::PARAM_COUNT] = std::array::from_fn(|i| sp.runtime.info[info::ROOM + i]);
        if room_mask != 0 {
            hash_f64_slice(&mut cmd_hasher, &room);
        }
        total_commands = total_commands.saturating_add(sp.runtime.phase.engine.commands.len() as u32);

        apply_batch(&mut output, &sp.runtime.phase.engine.commands, &sp.runtime.phase.engine.placements, &room, room_mask);

        // Handle stream requests synchronously and deterministically on the same tick.
        let mut reqs = sp.runtime.phase.streams.requests.clone();
        while !reqs.is_empty() {
            let mut next_reqs = Vec::new();
            for req in reqs {
                cmd_hasher.update(Wide::from_u32(req.voice as u32).to_le_bytes());
                cmd_hasher.update(req.generation.to_le_bytes());
                cmd_hasher.update(Wide::from_u32(req.segment as u32).to_le_bytes());
                cmd_hasher.update(Wide::from_u32(req.part as u32).to_le_bytes());
                cmd_hasher.update(Wide::from_u32(req.sample as u32).to_le_bytes());
                cmd_hasher.update(Wide::from_u32(req.operation as u32).to_le_bytes());
                if req.operation == request_kind::LOAD {
                    let loaded = segments.get(req.segment).and_then(SyncSegment::load);
                    let input = &mut sp.runtime.input[..ready::STRIDE];
                    input.fill(0.);
                    input[ready::VOICE] = req.voice as f64;
                    input[ready::GENERATION] = req.generation as f64;
                    input[ready::PART] = req.part as f64;
                    input[ready::SAMPLE] = req.sample as f64;
                    input[ready::TIME] = wall;
                    input[ready::WALL_NOW] = wall;
                    if let Some((_, frames, channels, rate)) = &loaded {
                        input[ready::DURATION] = *frames as f64 / *rate;
                        input[ready::CHANNELS] = *channels as f64;
                    } else {
                        input[ready::FAILED] = 1.;
                    }
                    sp.runtime.ready();
                    if sp.runtime.info[info::ACCEPTED] != 0. {
                        if let Some((sample, _, _, _)) = loaded {
                            output.voices.set_sample(req.sample, Some(sample));
                            total_streams = total_streams.saturating_add(1);
                        }
                        hash_f64_slice(&mut cmd_hasher, &sp.runtime.phase.engine.commands);
                        hash_f64_slice(&mut cmd_hasher, &sp.runtime.phase.engine.placements);
                        total_commands = total_commands.saturating_add(sp.runtime.phase.engine.commands.len() as u32);
                        apply_batch(&mut output, &sp.runtime.phase.engine.commands, &sp.runtime.phase.engine.placements, &[0.; audio::reverb::PARAM_COUNT], 0);
                    }
                    next_reqs.extend_from_slice(&sp.runtime.phase.streams.requests);
                } else if req.operation == request_kind::RELEASE {
                    for id in req.sample..req.sample + 3 {
                        output.voices.set_sample(id, None);
                    }
                }
            }
            reqs = next_reqs;
        }

        // Render exactly 800 stereo frames (1/60 s at 48 kHz) in lockstep.
        for _ in 0..FRAMES_PER_TICK {
            if sample_cursor == FRAMES {
                output.process();
                sample_cursor = 0;
            }
            let l = output.output[sample_cursor];
            let r = output.output[FRAMES + sample_cursor];
            sample_cursor += 1;
            pcm_hasher.update(l.to_le_bytes());
            pcm_hasher.update(r.to_le_bytes());
            peak = peak.max(l.abs()).max(r.abs());
            sum_sq += (l as f64) * (l as f64) + (r as f64) * (r as f64);
            total_samples = total_samples.saturating_add(2);
            if l != 0.0 || r != 0.0 {
                nonzero_samples = nonzero_samples.saturating_add(2);
            }
        }
    }

    let cmd_hex = format!("{:x}", cmd_hasher.finalize());
    let pcm_hex = format!("{:x}", pcm_hasher.finalize());
    let rms = (sum_sq / total_samples.max(1) as f64).sqrt();
    let mode = if enhanced { "enhanced" } else { "classic" };
    println!(
        "swbf1_audio {spec} mode={mode} ticks={ticks} cmds={total_commands} started={} streams={total_streams} nonzero={nonzero_samples}/{total_samples} peak={peak:.5} rms={rms:.5} cmd_sha256={cmd_hex} pcm_sha256={pcm_hex}",
        sp.runtime.phase.engine.stats[2],
    );
    assert!(total_commands > 0, "{spec} produced no audio commands");
    assert!(nonzero_samples > 0, "{spec} rendered silence");
    battle_rt.close();
}

#[test]
#[ignore = "requires prepared 2004 content in BFE_CONTENT_DIR"]
fn swbf1_six_map_audio_unchanged() {
    let root = std::env::var("BFE_CONTENT_DIR").expect("BFE_CONTENT_DIR");
    let maps = std::env::var("BFE_AUDIO_MAPS")
        .unwrap_or_else(|_| "nab2:rep,hot1:all,kam1:rep,tat3:all,geo1:rep,tat2:imp:heroes".into());
    let ticks = std::env::var("BFE_AUDIO_TICKS").ok().and_then(|s| s.parse().ok()).unwrap_or(3600);
    for spec in maps.split(',').filter(|s| !s.is_empty()) {
        run_map(&root, spec, ticks, true);
    }
    // Also exercise Classic mode (I3DL2 space reverb + OpenAL Soft stereo decoder) on interior map.
    run_map(&root, "tat3:all", ticks, false);
}

#[test]
#[ignore = "requires prepared 2004 content in BFE_CONTENT_DIR"]
fn swbf1_low_reinforcements_threshold_crossing() {
    use audio_control::engine::{NONE, VOICE};
    use std::collections::BTreeSet;
    use std::time::{Duration, Instant};

    let root = std::env::var("BFE_CONTENT_DIR").expect("BFE_CONTENT_DIR");
    let mut seen_sounds = BTreeSet::new();


    let cases = [
        ("all", 1usize, 1usize),
        ("all", 1, 2),
        ("imp", 2, 2),
        ("imp", 2, 1),
    ];

    for (side, listener, watched) in cases {
        let args: Vec<String> = ["--local", "bes1", &root, "--side", side, "--seed", "1", "--ai", "classic"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let options = Options::parse(&args, None).unwrap().unwrap();
        let battle_prep = BattlePrepared::new(&options).unwrap();
        let query = battle_prep.admission.query.clone();
        let content = Loader::detect(&root).load("bes1", &query).unwrap();
        let replay = Replay::new(&content).unwrap();
        let mut queries = Queries { collision: replay.collision.clone(), world: replay.world };
        let mut sp = SoundPrepared::new(&content).unwrap();
        let n_rules = sp.runtime.phase.director.low_reinforcements.len();
        let mission = content.content.battle.world.get("mission").unwrap().get("name").unwrap().as_str().unwrap();
        let index: serde_json::Value = serde_json::from_slice(&content.textures.read("audio/index.json").unwrap()).unwrap();
        let level = index["missions"][mission].as_str().unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(&content.textures.read(&format!("audio/{level}/sound.json")).unwrap()).unwrap();
        let config = &manifest["mission"];
        let authored = config["lowReinforcementsVoByMission"].get(mission).or_else(|| config.get("lowReinforcementsVo"));
        if let Ok(mode) = std::env::var("BFE_EXPECT_LOW_REINFORCEMENTS") {
            assert_eq!(authored.is_some(), mode.parse::<bool>().unwrap(), "wrong Bespin manifest for this crossing run");
        }
        let expected = authored.map(|rows| {
            let rows = rows.as_array().unwrap();
            assert_eq!(rows.len(), 4, "the Bespin crossing cases cover both teams and listeners");
            let row = rows.iter().find(|r| r[0].as_f64() == Some(watched as f64) && r[4].as_f64() == Some(listener as f64)).unwrap();
            key_hash(row[2].as_str().unwrap())
        });
        assert_eq!(n_rules, authored.map_or(0, |rows| rows.as_array().unwrap().len()));

        let settings = AudioSettings { enhanced: true, output: "stereo".into(), dynamics: "normal".into(), mono: false, clarity: true };
        let (mut output, rendering) = build_output(&settings, 2, 1 << 2, None, 1.0, None);
        output.take_sources(&mut sp.output);
        sp.runtime.phase.set_starting_tickets(150.0, 150.0);

        let mut cursor = FRAMES;
        let mut pre_vo_fires = 0usize;
        let mut cross_vo_ids: Vec<usize> = Vec::new();
        let mut post_vo_fires = 0usize;
        let mut pre_peak = 0.0f32;
        let mut cross_peak = 0.0f32;
        let mut cross_sum_sq = 0.0f64;
        let mut cross_samples = 0usize;
        let mut cross_nonzero = 0usize;

        let dt = 1.0 / 60.0;
        for tick in 1..=210usize {
            let wall = output.frame as f64 / 48000.0;
            sp.runtime.clear_output();
            sp.runtime.events.clear();
            let mut frame = [0.; frame::STRIDE];
            frame[frame::DT] = dt;
            frame[frame::WALL_NOW] = wall;
            frame[frame::NOW] = wall;
            frame[frame::RATE] = 48000.;
            frame[frame::ENHANCED] = rendering.enhanced as u8 as f64;
            frame[frame::LIGHT] = rendering.light as u8 as f64;
            frame[frame::HRTF] = rendering.hrtf as u8 as f64;
            frame[frame::BINAURAL] = rendering.binaural as u8 as f64;
            frame[frame::LFE] = rendering.lfe as u8 as f64;
            frame[frame::CONFIGURATION] = 1.0;
            frame[frame::COLLISION] = 1.;
            frame[frame::RUNNING] = 1.;
            frame[frame::MAX_VOICES] = 32.;
            frame[frame::VIRTUAL_VOICES] = 64.;
            frame[frame::PY] = 1.7;
            frame[frame::FZ] = -1.;
            frame[frame::UY] = 1.;
            frame[frame::TEAM] = listener as f64;
            frame[frame::HUMAN] = 1.;
            frame[frame::EFFECTS_VOLUME] = 0.;
            frame[frame::AMBIENCE_VOLUME] = 0.;
            frame[frame::MUSIC_VOLUME] = 0.;
            frame[frame::VOICE_VOLUME] = 1.;
            frame[frame::INTERFACE_VOLUME] = 0.;
            frame[frame::CHANNELS] = rendering.channels as f64;
            let (t1, t2) = if tick <= 30 {
                (150.0, 150.0)
            } else if tick <= 60 {
                (20.0, 20.0)
            } else if tick <= 180 {
                if watched == 1 { (15.0, 80.0) } else { (80.0, 15.0) }
            } else if watched == 1 {
                (10.0, 80.0)
            } else {
                (80.0, 10.0)
            };
            frame[frame::TICKETS1] = t1;
            frame[frame::TICKETS2] = t2;
            frame[frame::OWN] = OWN as f64;
            sp.runtime.phase.frame.copy_from_slice(&frame);

            if !sp.runtime.phase.started {
                let mut start = [0.; event::STRIDE];
                start[event::KIND] = event_kind::START as f64;
                sp.runtime.events.insert(0, start);
            }
            sp.runtime.step(&mut queries);
            let room_mask = sp.runtime.info[info::ROOM_CHANGED] as u32;
            let room: [f64; audio::reverb::PARAM_COUNT] =
                std::array::from_fn(|i| sp.runtime.info[info::ROOM + i]);
            apply_batch(&mut output, &sp.runtime.phase.engine.commands, &sp.runtime.phase.engine.placements, &room, room_mask);

            let mut reqs = sp.runtime.phase.streams.requests.clone();
            while !reqs.is_empty() {
                let mut next_reqs = Vec::new();
                for mut req in reqs {
                    if req.operation == request_kind::LOAD {
                        req.variant = NONE;
                        let before_vo: Vec<usize> = sp
                            .runtime
                            .phase
                            .streams
                            .census()
                            .filter(|&(_, cat)| cat == VOICE)
                            .map(|(snd, _)| snd)
                            .collect();
                        assert!(sp.streams.request(req), "stream request queue full");
                        let started = Instant::now();
                        let resp = loop {
                            if let Some(r) = sp.streams.receive() {
                                break r;
                            }
                            assert!(started.elapsed() < Duration::from_secs(20), "stream asset timeout");
                            std::thread::sleep(Duration::from_micros(200));
                        };
                        let input = &mut sp.runtime.input[..ready::STRIDE];
                        input.fill(0.);
                        input[ready::VOICE] = resp.request.voice as f64;
                        input[ready::GENERATION] = resp.request.generation as f64;
                        input[ready::PART] = resp.request.part as f64;
                        input[ready::SAMPLE] = resp.request.sample as f64;
                        input[ready::TIME] = wall;
                        input[ready::WALL_NOW] = wall;
                        if let Some(pcm) = &resp.pcm {
                            input[ready::DURATION] = pcm.frames as f64 / pcm.rate;
                            input[ready::CHANNELS] = pcm.channels as f64;
                        } else {
                            input[ready::FAILED] = 1.;
                        }
                        sp.runtime.ready();
                        if sp.runtime.info[info::ACCEPTED] != 0. {
                            if let Some(pcm) = resp.pcm {
                                output.voices.set_sample(resp.request.sample, Some(pcm.sample));
                            }
                            apply_batch(&mut output, &sp.runtime.phase.engine.commands, &sp.runtime.phase.engine.placements, &[0.; audio::reverb::PARAM_COUNT], 0);
                            let after_vo: Vec<usize> = sp
                                .runtime
                                .phase
                                .streams
                                .census()
                                .filter(|&(_, cat)| cat == VOICE)
                                .map(|(snd, _)| snd)
                                .collect();
                            if after_vo.len() > before_vo.len() {
                                let new_snd = *after_vo.last().unwrap();
                                if tick <= 60 {
                                    pre_vo_fires += 1;
                                } else if tick <= 180 {
                                    cross_vo_ids.push(new_snd);
                                } else {
                                    post_vo_fires += 1;
                                }
                            }
                        }
                        next_reqs.extend_from_slice(&sp.runtime.phase.streams.requests);
                    } else if req.operation == request_kind::RELEASE {
                        for id in req.sample..req.sample + 3 {
                            output.voices.set_sample(id, None);
                        }
                    }
                }
                reqs = next_reqs;
            }

            for _ in 0..FRAMES_PER_TICK {
                if cursor == FRAMES {
                    output.process();
                    cursor = 0;
                }
                let l = output.output[cursor];
                let r = output.output[FRAMES + cursor];
                cursor += 1;
                if tick <= 60 {
                    pre_peak = pre_peak.max(l.abs()).max(r.abs());
                } else if tick <= 180 {
                    cross_peak = cross_peak.max(l.abs()).max(r.abs());
                    cross_sum_sq += (l as f64) * (l as f64) + (r as f64) * (r as f64);
                    cross_samples += 2;
                    if l.abs() > 1e-6 || r.abs() > 1e-6 {
                        cross_nonzero += 2;
                    }
                }
            }
        }

        let cross_rms = (cross_sum_sq / cross_samples.max(1) as f64).sqrt();
        let sid_str = cross_vo_ids
            .first()
            .map(|&id| format!("{id}({})", sp.sound_names.get(id).map_or("", String::as_str)))
            .unwrap_or_else(|| "none".into());
        println!(
            "swbf1_low_reinf bes1:{side} listener={listener} watched={watched} rules={n_rules} pre={pre_vo_fires} cross={} post={post_vo_fires} sound={sid_str} pre_peak={pre_peak:.5} peak={cross_peak:.5} rms={cross_rms:.5} nz={cross_nonzero}",
            cross_vo_ids.len(),
        );
        assert_eq!(pre_vo_fires, 0, "bes1:{side} l={listener} w={watched}: must not fire above 10%");
        assert_eq!(post_vo_fires, 0, "bes1:{side} l={listener} w={watched}: must not refire after initial trigger");
        assert!(pre_peak < 1e-5, "bes1:{side}: must stay silent above 10% (pre_peak={pre_peak})");

        if let Some(name) = expected {
            assert_eq!(
                cross_vo_ids.len(),
                1,
                "bes1:{side} l={listener} w={watched}: must fire once on 10% crossing, got {cross_vo_ids:?}"
            );
            assert!(
                cross_peak > 0.05 && cross_nonzero > 4000,
                "bes1:{side} l={listener} w={watched}: VO line must render audible PCM (peak={cross_peak}, nz={cross_nonzero})"
            );
            assert_eq!(key_hash(&sp.sound_names[cross_vo_ids[0]]), name, "wrong line for team {watched}, listener {listener}");
            assert!(seen_sounds.insert(name), "each of the four Bespin crossings has a distinct line");
        } else {
            assert!(cross_vo_ids.is_empty(), "old manifest without lowReinforcementsVo must not fire");
            assert!(cross_peak < 1e-5, "old manifest without lowReinforcementsVo must stay silent");
        }
    }

}
