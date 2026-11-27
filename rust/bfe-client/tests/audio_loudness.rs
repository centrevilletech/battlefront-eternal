//! Reproducible BS.1770-4 / EBU Tech 3342 loudness harness for 2004 battles.
//! Runs seeded 60 s active spawned-player battles (walking, turning, firing in bursts
//! among the AI) through the live control runtime, production stream loader, and stereo
//! renderer across Enhanced and Classic modes and all three dynamics profiles (Home
//! theater, Normal, Night).
//!
//! Needs prepared 2004 content:
//! `BFE_CONTENT_DIR=... cargo test --release --locked -p bfe-client --test audio_loudness -- --ignored --nocapture`
//!
//! Optional environment variables:
//! - `BFE_LOUDNESS_MAPS`: comma-separated `map:side` list (default `nab2:rep,geo1:rep,hot1:all`)
//! - `BFE_LOUDNESS_TICKS`: ticks at 60 Hz (default `3600` = 60 s)
//! - `BFE_LOUDNESS_SEED`: simulation seed (default `1`)
//! - `BFE_LOUDNESS_UPSCALED`: set to `1` to also measure Enhanced with upscaled FLAC/Opus assets
use audio::{meter::Meter, output::Output, Block, FRAMES};
use audio_control::{
    engine::NONE,
    layout::*,
    Collision as SoundCollision, Hit, Point,
};
use bfe_client::generated_input::{
    ClientAction, ClientCommand, ClientCommandSource, ClientFlightCommand, ClientHeld, ClientLook,
};
use bfe_client::local::{
    launch::Prepared as BattlePrepared, options::Options, runtime::Runtime as BattleRuntime,
};
use bfe_client::online::{
    enhanced_audio,
    receiver::Receiver,
    replay::Replay,
    sound_data::{Prepared as SoundPrepared, SpaceBox},
    sound_output::{build as build_output, AudioSettings, Rendering},
};
use bfe_content::Value;
use bfe_content_native::setup::admission::{CLOCK_START_MS, CLOCK_UNIX_MS};
use bfe_content_native::Loader;
use bfe_presentation::network_layout::{net_kind as k, net_record as r};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

const OWN: usize = bfe_presentation::network_layout::net_limits::SLOTS + 1;
const FRAMES_PER_TICK: usize = 800; // 48000 Hz / 60 Hz
const PROFILES: [&str; 3] = ["home", "normal", "night"];

struct Queries {
    collision: bfe_client::online::replay::Collision,
    world: u32,
}
impl SoundCollision for Queries {
    fn cast(&mut self, from: Point, d: Point, far: f64) -> Option<Hit> {
        let ray = bfe_sim::collision::Ray {
            ox: from[0],
            oy: from[1],
            oz: from[2],
            dx: d[0],
            dy: d[1],
            dz: d[2],
        };
        self.collision.borrow_mut().ray_hit(self.world, ray, far, -1).map(|(code, h)| Hit {
            distance: h.distance,
            point: [h.px, h.py, h.pz],
            terrain: code == bfe_sim::collision::TERRAIN,
        })
    }
}

fn zhash(s: &str) -> u32 {
    s.encode_utf16()
        .fold(0x811c_9dc5u32, |h, c| (h ^ ((c as u32 & 255) | 32)).wrapping_mul(0x0100_0193))
}

fn key_hash(s: &str) -> u32 {
    if s.len() == 10
        && s.starts_with("0x")
        && s[2..].bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
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
    receiver
        .active()
        .find(|row| row[r::ID] == id as f64 && row[r::GENERATION] == generation as f64)
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

#[derive(Clone, Copy, Debug)]
struct LoudnessStats {
    integrated_lufs: f64,
    lra_lu: f64,
    true_peak_dbtp: f64,
}

struct MeterAccumulator {
    meter: Meter,
    integrated: f64,
    true_peak: f64,
    short_terms: Vec<f64>,
    blocks_seen: usize,
    buf: [f32; FRAMES * 2],
    fill: usize,
}

impl MeterAccumulator {
    fn new() -> Self {
        Self {
            meter: Meter::new(48000.0, Some(vec![1.0, 1.0])),
            integrated: f64::NEG_INFINITY,
            true_peak: f64::NEG_INFINITY,
            short_terms: Vec::with_capacity(640),
            blocks_seen: 0,
            buf: [0.0; FRAMES * 2],
            fill: 0,
        }
    }

    fn push_sample(&mut self, l: f32, r: f32) {
        self.buf[self.fill] = l;
        self.buf[FRAMES + self.fill] = r;
        self.fill += 1;
        if self.fill == FRAMES {
            let block = Block { data: &self.buf, channels: 2, frames: FRAMES };
            if let Some(reading) = self.meter.process(&block) {
                self.blocks_seen += 1;
                self.integrated = reading.integrated;
                self.true_peak = self.true_peak.max(reading.true_peak);
                if self.blocks_seen >= 30 && reading.short_term.is_finite() && reading.short_term > -70.0 {
                    self.short_terms.push(reading.short_term);
                }
            }
            self.fill = 0;
        }
    }

    fn finish(&self) -> LoudnessStats {
        let lra_lu = if self.short_terms.len() >= 2 {
            let avg_power = self
                .short_terms
                .iter()
                .map(|&l| 10.0f64.powf((l + 0.691) / 10.0))
                .sum::<f64>()
                / self.short_terms.len() as f64;
            let rel_gate = -0.691 + 10.0 * avg_power.log10() - 20.0;
            let mut gated: Vec<f64> = self
                .short_terms
                .iter()
                .copied()
                .filter(|&l| l >= rel_gate)
                .collect();
            if gated.len() >= 2 {
                gated.sort_by(f64::total_cmp);
                let p10 = gated[((gated.len() - 1) as f64 * 0.10).round() as usize];
                let p95 = gated[((gated.len() - 1) as f64 * 0.95).round() as usize];
                (p95 - p10).max(0.0)
            } else {
                0.0
            }
        } else {
            0.0
        };
        LoudnessStats {
            integrated_lufs: self.integrated,
            lra_lu,
            true_peak_dbtp: self.true_peak,
        }
    }
}

type ShotRecord = (u32, u32, usize, usize, Option<[f64; 3]>, bool);

struct AudioRunner {
    label: String,
    sp: SoundPrepared,
    output: Output,
    rendering: Rendering,
    use_upscaled: bool,
    upscaled_stream_loads: usize,
    posts: Vec<(usize, [f64; post::STRIDE])>,
    post_building: Vec<bool>,
    built: Vec<bool>,
    lives: Vec<Option<(f64, f64)>>,
    pilots: BTreeSet<(u32, u32)>,
    cosmetic: bfe_sim::wire::cosmetics::Compact,
    dry: (f64, [f64; room::STRIDE]),
    cur_space: Option<usize>,
    space_set: bool,
    starts_sent: bool,
    sample_cursor: usize,
    shots: Vec<ShotRecord>,
    meter: MeterAccumulator,
}

impl AudioRunner {
    fn new(
        content: &bfe_content_native::BattleContent,
        label: String,
        enhanced: bool,
        use_upscaled: bool,
        dynamics: &str,
    ) -> Result<Self, String> {
        let mut sp = SoundPrepared::new(content)?;
        let settings = AudioSettings {
            enhanced,
            output: "stereo".into(),
            dynamics: dynamics.into(),
            mono: false,
            clarity: true,
        };
        let (mut output, rendering) = build_output(&settings, 2, 1 << 2, None, 1.0, None);
        output.take_sources(&mut sp.output);
        if enhanced && use_upscaled && !sp.enhanced.is_empty() {
            let rx = enhanced_audio::load(std::mem::take(&mut sp.enhanced), sp.enhanced_base);
            for loaded in rx.iter() {
                let input = &mut sp.runtime.input[..enhanced_sample::STRIDE];
                input[enhanced_sample::ID] = loaded.index as f64;
                input[enhanced_sample::RENDER_ID] = loaded.render_id as f64;
                input[enhanced_sample::DURATION] = loaded.pcm.frames as f64 / loaded.pcm.rate;
                input[enhanced_sample::RATE] = loaded.pcm.rate;
                input[enhanced_sample::SCALE] = 1.0;
                input[enhanced_sample::AVAILABLE] = 1.0;
                sp.runtime.import(import::ENHANCED_SAMPLE, enhanced_sample::STRIDE);
                output.voices.set_sample(loaded.render_id, Some(loaded.pcm.sample));
            }
        }
        let posts = sp.posts.clone();
        let post_building = sp.post_building.clone();
        let built = vec![true; sp.stations.len()];
        let mut dry_row = [0.; room::STRIDE];
        dry_row[room::ROOM_HF] = 1.;
        dry_row[room::DECAY] = 1.;
        dry_row[room::HF_RATIO] = 0.5;
        dry_row[room::REFL_DELAY] = 0.02;
        dry_row[room::REVERB_DELAY] = 0.04;
        dry_row[room::DIFFUSION] = 1.;
        dry_row[room::DENSITY] = 1.;
        dry_row[room::HF_REFERENCE] = 5000.;
        dry_row[room::MASK] = 1.;
        Ok(Self {
            label,
            sp,
            output,
            rendering,
            use_upscaled,
            upscaled_stream_loads: 0,
            posts,
            post_building,
            built,
            lives: vec![None; OWN + 1],
            pilots: BTreeSet::new(),
            cosmetic: bfe_sim::wire::cosmetics::Compact::default(),
            dry: (0., dry_row),
            cur_space: None,
            space_set: false,
            starts_sent: false,
            sample_cursor: FRAMES,
            shots: Vec::with_capacity(256),
            meter: MeterAccumulator::new(),
        })
    }

    fn step_tick(&mut self, receiver: &Receiver, queries: &mut Queries, tick: usize) {
        let dt = 1.0 / 60.0;
        if !self.starts_sent {
            self.starts_sent = true;
            for row in &mut self.sp.music {
                row[music_slot::START] = 150.0;
                self.sp.runtime.input[..row.len()].copy_from_slice(row);
                self.sp.runtime.import(import::MUSIC_SLOT, row.len());
            }
            self.sp.runtime.phase.set_starting_tickets(150.0, 150.0);
        }

        let wall = self.output.frame as f64 / 48000.0;
        self.sp.runtime.clear_output();
        self.sp.runtime.events.clear();
        self.shots.clear();

        for field in receiver.publications.iter() {
            if field.row[r::KIND] == k::SUMMARY as f64 && field.row[r::SCHEMA] == 2. && field.length >= 23 {
                let b = &field.bytes[..field.length];
                let id = u16::from_le_bytes([b[0], b[1]]) as usize;
                if let Some((slot, (_, p))) = self.posts.iter_mut().enumerate().find(|(_, p)| p.0 == id) {
                    let old = p[post::TEAM];
                    let team = b[2] as f64;
                    let active = b[21] != 0;
                    let lost = p[post::ACTIVE] != 0.
                        && !active
                        && self.post_building.get(slot).copied().unwrap_or(false);
                    if p[post::PRESENT] != 0.
                        && (team != old || lost)
                        && self.sp.runtime.events.len() + 1 < self.sp.runtime.events.capacity()
                    {
                        let mut e = [0.; event::STRIDE];
                        e[event::KIND] = event_kind::POST as f64;
                        e[event::OWNER] = slot as f64;
                        e[event::VALUE] = if team != old && team != 0. { team } else { old };
                        e[event::SECONDS] = if team != old && team != 0. { 0. } else { 2. };
                        self.sp.runtime.events.push(e);
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
                        if let Some(b) = self.built.get_mut(state.0) {
                            *b = state.1 == "built";
                        }
                    }
                }
            }
            if field.row[r::KIND] == k::COMBAT as f64 && field.row[r::SCHEMA] == 2.
                && bfe_sim::wire::cosmetics::decode_bolt_into(
                    2.,
                    field.row[r::TICK],
                    &field.bytes[..field.length],
                    &mut self.cosmetic,
                )
                .is_ok()
            {
                if let Some(&class) = self
                    .sp
                    .combat_classes
                    .get(self.cosmetic.definition as usize)
                    .filter(|&&c| c != usize::MAX)
                {
                    let kind = if self.cosmetic.surface == 4. {
                        event_kind::SPLASH
                    } else {
                        event_kind::IMPACT
                    };
                    if self.sp.runtime.events.len() + 1 < self.sp.runtime.events.capacity() {
                        self.sp.runtime.events.push(enqueue_event(
                            kind,
                            class,
                            Some(self.cosmetic.view.position.unwrap_or_default()),
                        ));
                    }
                }
            }
            if field.row[r::KIND] == k::COMBAT as f64 && field.row[r::SCHEMA] == 5. && field.length == 33 {
                let b = &field.bytes[..33];
                let definition = u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
                let pos: [f64; 3] =
                    std::array::from_fn(|i| f64::from_le_bytes(b[8 + i * 8..16 + i * 8].try_into().unwrap()));
                if let Some(&class) = self
                    .sp
                    .combat_classes
                    .get(definition)
                    .filter(|&&c| c != usize::MAX && definition != 0)
                {
                    if self.sp.runtime.events.len() + 1 < self.sp.runtime.events.capacity() {
                        self.sp
                            .runtime
                            .events
                            .push(enqueue_event(event_kind::EXPLOSION, class, Some(pos)));
                    }
                }
            }
            if field.row[r::KIND] == k::COMBAT as f64 && field.row[r::SCHEMA] == 8. {
                if let Ok(src) = serde_json::from_slice::<EventInput>(&field.bytes[..field.length]) {
                    match src.kind {
                        "shot" => self.shots.push((
                            src.owner_id,
                            src.owner_generation,
                            src.weapon_id,
                            src.channel,
                            src.position,
                            src.repair,
                        )),
                        "vo" if src.owner_id != 0 && !src.property.is_empty() => {
                            let own = src.owner_id as f64 == receiver.own.id
                                && src.owner_generation as f64 == receiver.own.generation;
                            let cls = if own {
                                Some(receiver.own.class)
                            } else {
                                unit_row(receiver, src.owner_id, src.owner_generation)
                                    .map(|row| row[r::CLASS_ID] as usize)
                            };
                            if let Some(c) = cls
                                .and_then(|c| c.checked_sub(1))
                                .and_then(|i| self.sp.actor_classes.get(i).copied())
                                .filter(|&c| c != usize::MAX)
                            {
                                let mut ev = enqueue_event(event_kind::CHATTER, c, src.position);
                                ev[event::SOUND] = key_hash(&src.property) as f64;
                                ev[event::POSITIONAL] = (!own || src.positional) as u8 as f64;
                                if self.sp.runtime.events.len() + 1 < self.sp.runtime.events.capacity() {
                                    self.sp.runtime.events.push(ev);
                                }
                            }
                        }
                        "bounce" | "pickup" => {
                            if let (Some(&c), Some(pos)) = (
                                self.sp.combat_classes.get(src.definition_id).filter(|_| src.definition_id != 0),
                                src.position,
                            ) {
                                let kind = if src.kind == "bounce" {
                                    event_kind::BOUNCE
                                } else {
                                    event_kind::POWERUP
                                };
                                if self.sp.runtime.events.len() + 1 < self.sp.runtime.events.capacity() {
                                    self.sp.runtime.events.push(enqueue_event(kind, c, Some(pos)));
                                }
                            }
                        }
                        "serve" => {
                            if let Some(&(c, pos, _)) = self.sp.stations.get(src.building) {
                                if self.sp.runtime.events.len() + 1 < self.sp.runtime.events.capacity() {
                                    self.sp
                                        .runtime
                                        .events
                                        .push(enqueue_event(event_kind::POWERUP, c, Some(pos)));
                                }
                            }
                        }
                        "towAttach" | "towRelease" => {
                            if let Some(hull) = unit_row(receiver, src.owner_id, src.owner_generation)
                                .filter(|row| row[r::KIND] == k::VEHICLE as f64)
                            {
                                if let Some(&c) =
                                    src.weapon_id.checked_sub(1).and_then(|i| self.sp.weapon_classes.get(i))
                                {
                                    let kind = if src.kind == "towAttach" {
                                        event_kind::TOW_ATTACH
                                    } else {
                                        event_kind::TOW_RELEASE
                                    };
                                    let pos = [hull[r::POSITION], hull[r::POSITION + 1], hull[r::POSITION + 2]];
                                    if self.sp.runtime.events.len() + 1 < self.sp.runtime.events.capacity() {
                                        self.sp.runtime.events.push(enqueue_event(kind, c, Some(pos)));
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        let yaw = receiver.own.yaw;
        let mut frame = [0.; frame::STRIDE];
        frame[frame::DT] = dt;
        frame[frame::WALL_NOW] = wall;
        frame[frame::NOW] = wall;
        frame[frame::RATE] = 48000.;
        frame[frame::ENHANCED] = self.rendering.enhanced as u8 as f64;
        frame[frame::LIGHT] = self.rendering.light as u8 as f64;
        frame[frame::HRTF] = self.rendering.hrtf as u8 as f64;
        frame[frame::BINAURAL] = self.rendering.binaural as u8 as f64;
        frame[frame::LFE] = self.rendering.lfe as u8 as f64;
        frame[frame::CONFIGURATION] = 1.0;
        frame[frame::COLLISION] = 1.;
        frame[frame::RUNNING] = 1.;
        frame[frame::MAX_VOICES] = 32.;
        frame[frame::VIRTUAL_VOICES] = 64.;
        let pos = receiver.own.position;
        frame[frame::PX] = pos[0];
        frame[frame::PY] = pos[1] + 1.7;
        frame[frame::PZ] = pos[2];
        frame[frame::FX] = -yaw.sin();
        frame[frame::FY] = 0.;
        frame[frame::FZ] = -yaw.cos();
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
        frame[frame::CHANNELS] = self.rendering.channels as f64;
        frame[frame::TICKETS1] = (150.0 - (tick as f64 * 0.02)).max(10.0);
        frame[frame::TICKETS2] = (150.0 - (tick as f64 * 0.02)).max(10.0);
        frame[frame::OWN] = OWN as f64;
        self.sp.runtime.phase.frame.copy_from_slice(&frame);

        for (slot, (_, row)) in self.posts.iter().enumerate() {
            self.sp.runtime.phase.posts[slot] = *row;
        }
        self.sp.runtime.phase.post_count = self.posts.len();

        let mut seen = [false; OWN + 1];
        self.pilots.clear();
        for p in receiver.active() {
            if p[r::SEAT_PRESENT] != 0. && p[r::SEAT_INDEX] == 0. {
                self.pilots
                    .insert((p[r::SEAT_VEHICLE_ID] as u32, p[r::SEAT_VEHICLE_GENERATION] as u32));
            }
        }
        if let Some((id, gen, 0)) = receiver.own.mount {
            self.pilots.insert((id, gen));
        }

        for (slot, row) in receiver.active_slots() {
            if row[r::ID] == receiver.own.id && row[r::GENERATION] == receiver.own.generation {
                continue;
            }
            let id = slot + 1;
            if id >= self.lives.len() {
                continue;
            }
            let kind = row[r::KIND] as u32;
            if kind != k::SOLDIER && kind != k::DROIDEKA && kind != k::VEHICLE {
                continue;
            }
            let life = (row[r::ID], row[r::GENERATION]);
            if self.lives[id] != Some(life) {
                retire(&mut self.sp.runtime.phase, id);
                self.lives[id] = Some(life);
            }
            seen[id] = true;
            let mut e = [0.; emitter::STRIDE];
            e[emitter::ID] = id as f64;
            e[emitter::KIND] = if kind == k::VEHICLE {
                3.
            } else if kind == k::DROIDEKA {
                2.
            } else {
                1.
            };
            e[emitter::CLASS] = self
                .sp
                .actor_classes
                .get((row[r::CLASS_ID] as usize).wrapping_sub(1))
                .copied()
                .filter(|&i| i != usize::MAX)
                .map_or(-1., |i| i as f64);
            e[emitter::X..emitter::X + 3].copy_from_slice(&row[r::POSITION..r::POSITION + 3]);
            e[emitter::VX..emitter::VX + 3].copy_from_slice(&row[r::VELOCITY..r::VELOCITY + 3]);
            e[emitter::HEALTH] = row[r::HEALTH];
            let limits = self
                .sp
                .actor_limits
                .get(row[r::CLASS_ID] as usize)
                .copied()
                .unwrap_or([300., 9., 0.]);
            e[emitter::MAX_HEALTH] = limits[0];
            e[emitter::SPEED_LIMIT] = limits[1];
            e[emitter::FLYING] = limits[2];
            e[emitter::FLIGHT] = row[r::STATE];
            e[emitter::PILOTED] =
                self.pilots.contains(&(row[r::ID] as u32, row[r::GENERATION] as u32)) as u8 as f64;
            e[emitter::ALIVE] = (row[r::DEAD] == 0.) as u8 as f64;
            e[emitter::GENERATION] = row[r::GENERATION];
            e[emitter::AIR] = row[r::STATE];
            e[emitter::POSTURE] = row[r::POSTURE];
            let before = self.sp.runtime.phase.emitters[id];
            if before[emitter::KIND] != 0.
                && before[emitter::GENERATION] == e[emitter::GENERATION]
                && before[emitter::ALIVE] != 0.
            {
                let amount = before[emitter::HEALTH] - e[emitter::HEALTH];
                let dead = e[emitter::ALIVE] == 0.;
                if (amount > 0. || dead)
                    && self.sp.runtime.events.len() + 1 < self.sp.runtime.events.capacity()
                {
                    let mut ev = [0.; event::STRIDE];
                    ev[event::KIND] = if dead {
                        event_kind::ACTOR_DEATH
                    } else {
                        event_kind::ACTOR_HURT
                    } as f64;
                    ev[event::OWNER] = e[emitter::ID];
                    ev[event::CLASS] = e[emitter::CLASS];
                    ev[event::AMOUNT] = amount.max(0.);
                    ev[event::HEALTH] = e[emitter::HEALTH];
                    self.sp.runtime.events.push(ev);
                }
            }
            self.sp.runtime.phase.observe_emitter(&e);
            let wcls = self
                .sp
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
            self.sp.runtime.phase.observe_weapon(&w);
        }

        let own_life = (receiver.own.id, receiver.own.generation);
        if self.lives[OWN] != Some(own_life) {
            retire(&mut self.sp.runtime.phase, OWN);
            self.lives[OWN] = Some(own_life);
        }
        seen[OWN] = true;
        let mut own = [0.; emitter::STRIDE];
        own[emitter::ID] = OWN as f64;
        own[emitter::KIND] = if receiver.own.walker { 2. } else { 1. };
        own[emitter::CLASS] = self
            .sp
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
        own[emitter::MAX_HEALTH] = self
            .sp
            .actor_limits
            .get(receiver.own.class)
            .map_or(300., |v| v[0]);
        own[emitter::MOUNTED] = receiver.own.mount.is_some() as u8 as f64;
        own[emitter::GROUNDED] = (receiver.own.air == 0) as u8 as f64;
        own[emitter::FIRST_PERSON] = 1.;
        self.sp.runtime.phase.observe_emitter(&own);

        for (index, &(class, position, station)) in self.sp.stations.iter().enumerate() {
            if !station || class == usize::MAX {
                continue;
            }
            let mut row = [0.; emitter::STRIDE];
            row[emitter::ID] = (OWN + 1 + index) as f64;
            row[emitter::KIND] = emitter_kind::STATION as f64;
            row[emitter::CLASS] = class as f64;
            row[emitter::X..emitter::X + 3].copy_from_slice(&position);
            row[emitter::ALIVE] = self.built[index] as u8 as f64;
            self.sp.runtime.phase.observe_emitter(&row);
        }
        for (id, present) in seen.into_iter().enumerate().skip(1) {
            if !present && self.lives[id].take().is_some() {
                retire(&mut self.sp.runtime.phase, id);
            }
        }
        for &(owner_id, owner_gen, weapon_id, channel, position, repair) in &self.shots {
            let owner = if owner_id as f64 == receiver.own.id && owner_gen as f64 == receiver.own.generation {
                Some(OWN)
            } else {
                receiver
                    .active_slots()
                    .find(|(_, row)| row[r::ID] == owner_id as f64 && row[r::GENERATION] == owner_gen as f64)
                    .map(|(slot, _)| slot + 1)
            };
            let Some(owner) = owner else { continue };
            let class = self
                .sp
                .weapon_classes
                .get(weapon_id.wrapping_sub(1))
                .copied()
                .filter(|&i| i != usize::MAX)
                .map_or(-1., |i| i as f64);
            let wid = if owner == OWN { owner + channel.min(1) } else { owner };
            let w = &mut self.sp.runtime.phase.weapons[wid];
            if owner != OWN {
                w.fill(0.);
            }
            w[weapon::ID] = wid as f64;
            w[weapon::OWNER] = owner as f64;
            w[weapon::CLASS] = class;
            w[weapon::SELECTED] = 1.;
            self.sp.runtime.phase.weapon_count = self.sp.runtime.phase.weapon_count.max(wid + 1);
            if let Some(pos) = position {
                self.sp.runtime.phase.emitters[owner][emitter::X..emitter::X + 3].copy_from_slice(&pos);
            }
            let mut ev = [0.; event::STRIDE];
            ev[event::KIND] = if owner == OWN {
                event_kind::FIRE
            } else {
                event_kind::ACTOR_FIRE
            } as f64;
            ev[event::OWNER] = owner as f64;
            ev[event::WEAPON] = wid as f64;
            ev[event::CLASS] = class;
            ev[event::VALUE] = repair as u8 as f64;
            if self.sp.runtime.events.len() + 1 < self.sp.runtime.events.capacity() {
                self.sp.runtime.events.push(ev);
            }
        }

        if !self.sp.runtime.phase.started {
            let mut start = [0.; event::STRIDE];
            start[event::KIND] = event_kind::START as f64;
            self.sp.runtime.events.insert(0, start);
        }

        let p = &self.sp.runtime.phase.frame;
        let ear = [
            p[frame::PX] + 2. * p[frame::FX],
            p[frame::PY] + 2. * p[frame::FY],
            p[frame::PZ] + 2. * p[frame::FZ],
        ];
        let space = listener_space(&self.sp.space_boxes, self.cur_space, ear);
        if space != self.cur_space || !self.space_set {
            self.cur_space = space;
            self.space_set = true;
            let (send, row) = space.map_or(self.dry, |id| self.sp.spaces[id]);
            self.sp.runtime.phase.space_send = send;
            self.sp.runtime.phase.original_room = audio_control::acoustics::Room {
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
            self.sp.runtime.phase.original_room_mask = row[room::MASK] as u32;
            if !self.sp.runtime.phase.engine.enhanced {
                self.sp.runtime.phase.room_pending |= self.sp.runtime.phase.original_room_mask;
            }
        }

        self.sp.runtime.step(queries);

        let room_mask = self.sp.runtime.info[info::ROOM_CHANGED] as u32;
        let room: [f64; audio::reverb::PARAM_COUNT] =
            std::array::from_fn(|i| self.sp.runtime.info[info::ROOM + i]);
        apply_batch(
            &mut self.output,
            &self.sp.runtime.phase.engine.commands,
            &self.sp.runtime.phase.engine.placements,
            &room,
            room_mask,
        );

        let mut reqs = self.sp.runtime.phase.streams.requests.clone();
        while !reqs.is_empty() {
            let mut next = Vec::new();
            for mut req in reqs {
                if req.operation == request_kind::LOAD {
                    if !self.use_upscaled {
                        req.variant = NONE;
                    }
                    if req.variant != NONE {
                        self.upscaled_stream_loads += 1;
                    }
                    assert!(self.sp.streams.request(req), "stream request queue full");
                    let started = Instant::now();
                    let resp = loop {
                        if let Some(r) = self.sp.streams.receive() {
                            break r;
                        }
                        assert!(started.elapsed() < Duration::from_secs(20), "stream asset timeout");
                        std::thread::sleep(Duration::from_micros(200));
                    };
                    let input = &mut self.sp.runtime.input[..ready::STRIDE];
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
                    self.sp.runtime.ready();
                    if self.sp.runtime.info[info::ACCEPTED] != 0. {
                        if let Some(pcm) = resp.pcm {
                            self.output.voices.set_sample(resp.request.sample, Some(pcm.sample));
                        }
                        apply_batch(
                            &mut self.output,
                            &self.sp.runtime.phase.engine.commands,
                            &self.sp.runtime.phase.engine.placements,
                            &[0.; audio::reverb::PARAM_COUNT],
                            0,
                        );
                    }
                    next.extend_from_slice(&self.sp.runtime.phase.streams.requests);
                } else if req.operation == request_kind::RELEASE {
                    for id in req.sample..req.sample + 3 {
                        self.output.voices.set_sample(id, None);
                    }
                }
            }
            reqs = next;
        }

        if self.upscaled_stream_loads > 0 && tick.is_multiple_of(8) {
            std::thread::sleep(Duration::from_millis(2));
        }

        for _ in 0..FRAMES_PER_TICK {
            if self.sample_cursor == FRAMES {
                self.output.process();
                self.sample_cursor = 0;
            }
            let l = self.output.output[self.sample_cursor];
            let r = self.output.output[FRAMES + self.sample_cursor];
            self.sample_cursor += 1;
            self.meter.push_sample(l, r);
        }
    }
}

fn default_spawn_class(content: &bfe_content_native::BattleContent, team: u8) -> Option<String> {
    content
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
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn default_spawn_post(content: &bfe_content_native::BattleContent, team: u8) -> Option<String> {
    let defs = content.defs().ok()?;
    let world_posts = content.content.battle.world.get("posts");
    for name in &defs.registry.posts {
        let post_team = world_posts
            .and_then(|p| p.get(name))
            .and_then(|m| m.get("team"))
            .and_then(Value::as_f64)
            .map(|t| t as u8);
        if post_team == Some(team) {
            return Some(name.clone());
        }
    }
    defs.registry.posts.first().cloned()
}

struct MapLoudnessResult {
    enhanced: [LoudnessStats; 3],
    classic: [LoudnessStats; 3],
    upscaled: Option<[LoudnessStats; 3]>,
    alive_ticks: usize,
    own_shots: usize,
}

fn measure_map(root: &str, spec: &str, ticks: usize, seed: &str, measure_upscaled: bool) -> MapLoudnessResult {
    let parts: Vec<&str> = spec.split(':').collect();
    let map = parts[0];
    let side = parts.get(1).copied().unwrap_or("rep");
    let args: Vec<String> = ["--local", map, root, "--side", side, "--seed", seed, "--ai", "classic"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let options = Options::parse(&args, None).unwrap().unwrap();
    let battle_prep = BattlePrepared::new(&options).unwrap();
    let query = battle_prep.admission.query.clone();
    let mut battle_rt =
        BattleRuntime::new(*battle_prep.engine, *battle_prep.admission, CLOCK_START_MS, CLOCK_UNIX_MS).unwrap();

    let content = Loader::detect(root).load(map, &query).unwrap();
    let replay = Replay::new(&content).unwrap();
    let mut queries = Queries { collision: replay.collision.clone(), world: replay.world };

    let team = battle_rt.receiver().own.team.clamp(1, 2);
    let spawn_class = default_spawn_class(&content, team);
    let spawn_post = default_spawn_post(&content, team);

    let has_upscaled = measure_upscaled
        && SoundPrepared::new(&content)
            .map(|sp| !sp.enhanced.is_empty())
            .unwrap_or(false);

    let mut runners = Vec::new();
    for profile in PROFILES {
        runners.push(AudioRunner::new(&content, format!("enh_{profile}"), true, false, profile).unwrap());
        runners.push(AudioRunner::new(&content, format!("cls_{profile}"), false, false, profile).unwrap());
        if has_upscaled {
            runners.push(AudioRunner::new(&content, format!("up_{profile}"), true, true, profile).unwrap());
        }
    }

    let mut alive_ticks = 0usize;
    let mut own_shots = 0usize;
    let mut respawn_cooldown = 0usize;

    for tick in 1..=ticks {
        let phase = tick % 540;
        let mut held = 0u32;
        if phase < 180 || (270..450).contains(&phase) {
            held |= 1 << (ClientHeld::Forward as u8);
        } else if phase < 270 {
            held |= 1 << (ClientHeld::Right as u8);
        } else {
            held |= 1 << (ClientHeld::Left as u8);
        }
        let mut actions = 0u32;
        let burst = tick % 180;
        if burst < 36 {
            held |= 1 << (ClientHeld::Primary as u8);
            if burst == 0 {
                actions |= 1 << (ClientAction::FirePrimary as u8);
            }
        }
        let cmd = ClientCommand {
            sequence: tick as f64,
            held,
            actions,
            source: if actions != 0 {
                ClientCommandSource::Action
            } else {
                ClientCommandSource::Movement
            },
            look_count: 1,
            mouse_scale: 1.0,
            has_mouse_scale: true,
            ..Default::default()
        };
        let look = ClientLook { x: 4.0, y: 0.0 };
        battle_rt.step(&cmd, &look, &ClientFlightCommand::default()).unwrap();

        if battle_rt.receiver().own.alive {
            alive_ticks += 1;
            respawn_cooldown = 0;
        } else if respawn_cooldown == 0 {
            if let (Some(cls), Some(post)) = (&spawn_class, &spawn_post) {
                let _ = battle_rt.spawn_human(team, cls, post);
                respawn_cooldown = 30;
            }
        } else {
            respawn_cooldown -= 1;
        }

        let receiver = battle_rt.receiver();
        for field in receiver.publications.iter() {
            if field.row[r::KIND] == k::COMBAT as f64 && field.row[r::SCHEMA] == 8. {
                if let Ok(src) = serde_json::from_slice::<EventInput>(&field.bytes[..field.length]) {
                    if src.kind == "shot"
                        && src.owner_id as f64 == receiver.own.id
                        && src.owner_generation as f64 == receiver.own.generation
                    {
                        own_shots += 1;
                    }
                }
            }
        }

        for runner in &mut runners {
            runner.step_tick(receiver, &mut queries, tick);
        }
        battle_rt.receiver_mut().publications.clear();
    }
    battle_rt.close();

    let stride = if has_upscaled { 3 } else { 2 };
    let mut enhanced = [LoudnessStats { integrated_lufs: 0.0, lra_lu: 0.0, true_peak_dbtp: 0.0 }; 3];
    let mut classic = [LoudnessStats { integrated_lufs: 0.0, lra_lu: 0.0, true_peak_dbtp: 0.0 }; 3];
    let mut upscaled = has_upscaled.then_some([LoudnessStats { integrated_lufs: 0.0, lra_lu: 0.0, true_peak_dbtp: 0.0 }; 3]);

    for (i, profile) in PROFILES.iter().enumerate() {
        let e = runners[i * stride].meter.finish();
        let c = runners[i * stride + 1].meter.finish();
        enhanced[i] = e;
        classic[i] = c;
        let gap = e.integrated_lufs - c.integrated_lufs;
        assert!(e.integrated_lufs.is_finite() && e.integrated_lufs > -60.0, "{spec} {} silent", runners[i * stride].label);
        assert!(c.integrated_lufs.is_finite() && c.integrated_lufs > -60.0, "{spec} {} silent", runners[i * stride + 1].label);
        assert!(e.true_peak_dbtp <= -0.95, "{spec} enh {profile} true peak exceeded -1 dBTP: {:.2}", e.true_peak_dbtp);
        assert!(c.true_peak_dbtp <= -0.95, "{spec} cls {profile} true peak exceeded -1 dBTP: {:.2}", c.true_peak_dbtp);

        if let Some(ref mut up_arr) = upscaled {
            let u = runners[i * stride + 2].meter.finish();
            up_arr[i] = u;
            assert!(u.true_peak_dbtp <= -0.95, "{spec} up {profile} true peak exceeded -1 dBTP: {:.2}", u.true_peak_dbtp);
            println!(
                "loudness {spec} profile={profile} enh={:.2}LUFS(lra={:.1},tp={:.2}) cls={:.2}LUFS(lra={:.1},tp={:.2}) gap={:+.2}LU up={:.2}LUFS(lra={:.1},tp={:.2},delta_up={:+.2}LU) alive={alive_ticks}/{ticks} shots={own_shots}",
                e.integrated_lufs, e.lra_lu, e.true_peak_dbtp,
                c.integrated_lufs, c.lra_lu, c.true_peak_dbtp,
                gap,
                u.integrated_lufs, u.lra_lu, u.true_peak_dbtp, u.integrated_lufs - e.integrated_lufs,
            );
        } else {
            println!(
                "loudness {spec} profile={profile} enh={:.2}LUFS(lra={:.1},tp={:.2}) cls={:.2}LUFS(lra={:.1},tp={:.2}) gap={:+.2}LU alive={alive_ticks}/{ticks} shots={own_shots}",
                e.integrated_lufs, e.lra_lu, e.true_peak_dbtp,
                c.integrated_lufs, c.lra_lu, c.true_peak_dbtp,
                gap,
            );
        }
    }

    MapLoudnessResult { enhanced, classic, upscaled, alive_ticks, own_shots }
}

#[test]
#[ignore = "requires prepared 2004 content in BFE_CONTENT_DIR"]
fn swbf1_loudness_matrix() {
    let root = std::env::var("BFE_CONTENT_DIR").expect("BFE_CONTENT_DIR");
    let maps = std::env::var("BFE_LOUDNESS_MAPS").unwrap_or_else(|_| "nab2:rep,geo1:rep,hot1:all".into());
    let ticks = std::env::var("BFE_LOUDNESS_TICKS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3600);
    let seed = std::env::var("BFE_LOUDNESS_SEED").unwrap_or_else(|_| "1".into());
    let measure_upscaled = std::env::var("BFE_LOUDNESS_UPSCALED").is_ok_and(|v| v == "1");

    let mut results = Vec::new();
    for spec in maps.split(',').filter(|s| !s.is_empty()) {
        let res = measure_map(&root, spec, ticks, &seed, measure_upscaled);
        assert!(res.alive_ticks > ticks / 2, "{spec}: player must stay active during loudness run");
        assert!(res.own_shots > 0, "{spec}: player must fire bursts during loudness run");
        results.push((spec.to_string(), res));
    }
    assert!(!results.is_empty());

    let n = results.len() as f64;
    for (i, profile) in PROFILES.iter().enumerate() {
        let mean_gap: f64 = results
            .iter()
            .map(|(_, r)| r.enhanced[i].integrated_lufs - r.classic[i].integrated_lufs)
            .sum::<f64>()
            / n;
        let min_gap = results
            .iter()
            .map(|(_, r)| r.enhanced[i].integrated_lufs - r.classic[i].integrated_lufs)
            .fold(f64::INFINITY, f64::min);
        let max_gap = results
            .iter()
            .map(|(_, r)| r.enhanced[i].integrated_lufs - r.classic[i].integrated_lufs)
            .fold(f64::NEG_INFINITY, f64::max);
        let up_note = if results.iter().all(|(_, r)| r.upscaled.is_some()) {
            let max_shift = results
                .iter()
                .map(|(_, r)| (r.upscaled.unwrap()[i].integrated_lufs - r.enhanced[i].integrated_lufs).abs())
                .fold(0.0f64, f64::max);
            format!(" max_upscaled_shift={max_shift:.2}LU")
        } else {
            String::new()
        };
        println!(
            "loudness_mean maps={} profile={profile} mean_gap={mean_gap:+.2}LU range=[{min_gap:+.2}, {max_gap:+.2}]LU{up_note}",
            results.len(),
        );
    }
}
