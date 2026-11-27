//! Headless 2005 audio resolution audit and WAV clip renderer.
//! Reads prepared 2005 content from `BFE_SWBF2_CONTENT_DIR` and mission queries
//! from `BFE_SWBF2_MISSION_A` and `BFE_SWBF2_MISSION_B` (`<map>|<query>`).
//! Optional selectors (`BFE_SWBF2_SABER_SPEC`, `BFE_SWBF2_BLASTER_CLASS`,
//! `BFE_SWBF2_FIGHTER_CLASS`, `BFE_SWBF2_VO_LINE`) target specific mission classes,
//! combo states, and voice-over lines without hardcoding game identifiers.
//! When `BFE_SWBF2_CLIPS_DIR` is set, renders six 48 kHz stereo PCM WAV clips
//! through the live `audio-control` and `audio::output::Output` pipeline.

use audio::{meter::Meter, output::Output, voices::Sample, Block, FRAMES};
use audio_control::engine::{NONE, VOICE};
use audio_control::layout::*;
use audio_control::swbf2::saber::SaberEvent;
use audio_control::swbf2::zero_hash;
use audio_control::{Collision as SoundCollision, Hit, Point};
use bfe_client::online::enhanced_audio;
use bfe_client::online::sound_data::Prepared as SoundPrepared;
use bfe_client::online::sound_output::{build as build_output, AudioSettings};
use bfe_content::Value;
use bfe_content_native::{GameFamily, Loader};
use std::collections::BTreeSet;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const FRAMES_PER_TICK: usize = 800; // 48000 Hz / 60 Hz

struct FlatGround;
impl SoundCollision for FlatGround {
    fn cast(&mut self, from: Point, direction: Point, reach: f64) -> Option<Hit> {
        if direction[1] < -1e-6 && from[1] > 0.0 {
            let d = -from[1] / direction[1];
            if d >= 0.0 && d <= reach {
                return Some(Hit {
                    distance: d,
                    point: [from[0] + direction[0] * d, 0.0, from[2] + direction[2] * d],
                    terrain: true,
                });
            }
        }
        None
    }
}

fn decode_stream_chunked(bytes: &[u8], gain_db: f64) -> Result<(Sample, usize, usize, f64), String> {
    use audio_decode::{Decoder, Step};
    let mut decoder = Decoder::new(0, gain_db, false);
    let mut at = 0usize;
    let mut planes: Vec<Vec<f32>> = Vec::new();
    let mut out_rate = 0.0;
    loop {
        match decoder.step(1 << 15)? {
            Step::NeedInput => {
                if at >= bytes.len() {
                    return Err("truncated stream file".into());
                }
                let end = (at + 65536).min(bytes.len());
                decoder.push(&bytes[at..end], end == bytes.len())?;
                at = end;
            }
            Step::Metadata => {
                let mut info = decoder.info.clone().ok_or("missing stream metadata")?;
                if info.frames.is_none() {
                    decoder.tail(&bytes[bytes.len().saturating_sub(65536)..])?;
                    info = decoder.info.clone().ok_or("missing stream metadata")?;
                }
                out_rate = info.rate as f64;
                planes = vec![Vec::with_capacity(info.frames.unwrap_or(0) as usize); info.channels];
            }
            Step::Output(frames) => {
                for (c, plane) in planes.iter_mut().enumerate() {
                    plane.extend_from_slice(&decoder.output[c * frames..(c + 1) * frames]);
                }
            }
            Step::Done => break,
        }
    }
    let channels = planes.len();
    let frames = planes.first().map_or(0, Vec::len);
    if channels == 0 || frames == 0 {
        return Err("empty decoded stream".into());
    }
    let sample = Sample { rate: out_rate, channels, frames, data: planes.concat(), ring: None, feed: None };
    Ok((sample, frames, channels, out_rate))
}

fn resolve_level_name(content: &bfe_content_native::BattleContent) -> Result<String, String> {
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
    index["missions"][mission]
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
        .map(str::to_string)
        .ok_or_else(|| "battle has no sound bank".into())
}

fn write_wav_stereo_16(path: &Path, samples: &[f32], rate: u32) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut f = File::create(path).map_err(|e| e.to_string())?;
    let data_bytes = (samples.len() * 2) as u32;
    let riff_size = 36 + data_bytes;
    let channels: u16 = 2;
    let bits: u16 = 16;
    let byte_rate = rate * u32::from(channels) * u32::from(bits / 8);
    let block_align = channels * (bits / 8);
    f.write_all(b"RIFF").map_err(|e| e.to_string())?;
    f.write_all(&riff_size.to_le_bytes()).map_err(|e| e.to_string())?;
    f.write_all(b"WAVE").map_err(|e| e.to_string())?;
    f.write_all(b"fmt ").map_err(|e| e.to_string())?;
    f.write_all(&16u32.to_le_bytes()).map_err(|e| e.to_string())?;
    f.write_all(&1u16.to_le_bytes()).map_err(|e| e.to_string())?;
    f.write_all(&channels.to_le_bytes()).map_err(|e| e.to_string())?;
    f.write_all(&rate.to_le_bytes()).map_err(|e| e.to_string())?;
    f.write_all(&byte_rate.to_le_bytes()).map_err(|e| e.to_string())?;
    f.write_all(&block_align.to_le_bytes()).map_err(|e| e.to_string())?;
    f.write_all(&bits.to_le_bytes()).map_err(|e| e.to_string())?;
    f.write_all(b"data").map_err(|e| e.to_string())?;
    f.write_all(&data_bytes.to_le_bytes()).map_err(|e| e.to_string())?;
    for &s in samples {
        let clamped = s.clamp(-1.0, 1.0);
        let q = (clamped * 32767.0).round() as i16;
        f.write_all(&q.to_le_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn rms_db_window(pcm_stereo: &[f32], start_s: f64, end_s: f64) -> f64 {
    let i0 = ((start_s * 48000.0) as usize * 2).min(pcm_stereo.len());
    let i1 = ((end_s * 48000.0) as usize * 2).min(pcm_stereo.len());
    if i1 <= i0 {
        return -120.0;
    }
    let sum_sq: f64 = pcm_stereo[i0..i1].iter().map(|&v| (v as f64) * (v as f64)).sum();
    let rms = (sum_sq / (i1 - i0) as f64).sqrt();
    if rms <= 1e-6 { -120.0 } else { 20.0 * rms.log10() }
}

fn channel_rms(pcm_stereo: &[f32], start_s: f64, end_s: f64) -> (f64, f64) {
    let f0 = (start_s * 48000.0) as usize;
    let f1 = ((end_s * 48000.0) as usize).min(pcm_stereo.len() / 2);
    if f1 <= f0 {
        return (0.0, 0.0);
    }
    let mut sl = 0.0f64;
    let mut sr = 0.0f64;
    for frame in f0..f1 {
        let l = pcm_stereo[frame * 2] as f64;
        let r = pcm_stereo[frame * 2 + 1] as f64;
        sl += l * l;
        sr += r * r;
    }
    let n = (f1 - f0) as f64;
    ((sl / n).sqrt(), (sr / n).sqrt())
}

fn first_saber_weapon_hash(
    content: &bfe_content_native::BattleContent,
    phase: &audio_control::phase::Phase,
) -> u32 {
    let is_saber = |h: u32| -> bool {
        let Some(cid) = phase.swbf2_class_by_hash(h) else { return false };
        phase
            .swbf2()
            .and_then(|sw| sw.classes.get(cid))
            .is_some_and(|c| c.saber_turn_on != NONE && c.saber_on_hum != NONE)
    };
    if let Ok(defs) = content.defs() {
        for w in &defs.weapons {
            let wname = w
                .class
                .map_or_else(|| w.record.get("class").and_then(Value::as_str).unwrap_or(""), |n| defs.names.text(n));
            let h = zero_hash(wname);
            if h != 0 && is_saber(h) {
                return h;
            }
        }
    }
    for side in &content.content.battle.sides {
        if let Some(weapons) = side.weapons.as_object() {
            for (name, _) in weapons.entries() {
                let h = zero_hash(name);
                if h != 0 && is_saber(h) {
                    return h;
                }
            }
        }
    }
    0
}

fn parse_hash_or_hex(s: &str) -> u32 {
    let trimmed = s.trim();
    if let Some(hex) = trimmed.strip_prefix("0x") {
        if hex.len() == 8 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            if let Ok(v) = u32::from_str_radix(hex, 16) {
                return v;
            }
        }
    } else if trimmed.len() == 8 && trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
        if let Ok(v) = u32::from_str_radix(trimmed, 16) {
            return v;
        }
    }
    zero_hash(trimmed)
}

struct LoudnessStats {
    integrated_lufs: f64,
    lra_lu: f64,
    true_peak_dbtp: f64,
}

fn measure_bs1770(pcm_stereo: &[f32]) -> LoudnessStats {
    let mut meter = Meter::new(48000.0, Some(vec![1.0, 1.0]));
    let mut buf = [0.0f32; FRAMES * 2];
    let mut integrated = f64::NEG_INFINITY;
    let mut true_peak = f64::NEG_INFINITY;
    let mut short_terms: Vec<f64> = Vec::new();
    let total_frames = pcm_stereo.len() / 2;
    let mut offset = 0usize;
    let mut blocks_seen = 0usize;
    while offset < total_frames {
        let take = FRAMES.min(total_frames - offset);
        for i in 0..take {
            buf[i] = pcm_stereo[(offset + i) * 2];
            buf[take + i] = pcm_stereo[(offset + i) * 2 + 1];
        }
        let block = Block { data: &buf[..take * 2], channels: 2, frames: take };
        if let Some(r) = meter.process(&block) {
            blocks_seen += 1;
            integrated = r.integrated;
            true_peak = true_peak.max(r.true_peak);
            if blocks_seen >= 30 && r.short_term.is_finite() && r.short_term > -70.0 {
                short_terms.push(r.short_term);
            }
        }
        offset += take;
    }
    let lra_lu = if short_terms.len() >= 2 {
        let avg_power = short_terms
            .iter()
            .map(|&l| 10.0f64.powf((l + 0.691) / 10.0))
            .sum::<f64>()
            / short_terms.len() as f64;
        let rel_gate = -0.691 + 10.0 * avg_power.log10() - 20.0;
        let mut gated: Vec<f64> = short_terms.into_iter().filter(|&l| l >= rel_gate).collect();
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
        integrated_lufs: integrated,
        lra_lu,
        true_peak_dbtp: true_peak,
    }
}

struct Renderer {
    sp: SoundPrepared,
    output: Output,
    cursor: usize,
    wall: f64,
    ticks: usize,
    pcm: Vec<f32>,
    enhanced: bool,
    use_upscaled: bool,
    enhanced_samples_loaded: usize,
    upscaled_stream_loads: usize,
    original_stream_loads: usize,
    failed_stream_loads: usize,
    upscaled_bank_starts: usize,
}

impl Renderer {
    fn new(content: &bfe_content_native::BattleContent, enhanced: bool) -> Result<Self, String> {
        Self::new_with_mode(content, enhanced, false, "normal")
    }

    fn new_with_mode(
        content: &bfe_content_native::BattleContent,
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
        let (mut output, _) = build_output(&settings, 2, 1 << 2, None, 1.0, None);
        output.take_sources(&mut sp.output);
        let mut enhanced_samples_loaded = 0usize;
        if enhanced && use_upscaled && !sp.enhanced.is_empty() {
            // Use the client's production loading thread (`enhanced_audio::load`).
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
                enhanced_samples_loaded += 1;
            }
        }
        Ok(Self {
            sp,
            output,
            cursor: FRAMES,
            wall: 0.0,
            ticks: 0,
            pcm: Vec::new(),
            enhanced,
            use_upscaled,
            enhanced_samples_loaded,
            upscaled_stream_loads: 0,
            original_stream_loads: 0,
            failed_stream_loads: 0,
            upscaled_bank_starts: 0,
        })
    }

    fn init_frame(&mut self, listener_pos: Point, music_vol: f64, ambience_vol: f64, fx_vol: f64, vo_vol: f64) {
        let f = &mut self.sp.runtime.phase.frame;
        f.fill(0.0);
        f[frame::DT] = 1.0 / 60.0;
        f[frame::RATE] = 48000.0;
        f[frame::RUNNING] = 1.0;
        f[frame::ENHANCED] = if self.enhanced { 1.0 } else { 0.0 };
        f[frame::COLLISION] = 1.0;
        f[frame::CHANNELS] = 2.0;
        f[frame::MAX_VOICES] = 32.0;
        f[frame::VIRTUAL_VOICES] = 64.0;
        f[frame::CONFIGURATION] = 1.0;
        f[frame::PX..frame::PX + 3].copy_from_slice(&listener_pos);
        f[frame::FZ] = -1.0;
        f[frame::UY] = 1.0;
        f[frame::TEAM] = 1.0;
        f[frame::HUMAN] = 1.0;
        f[frame::OWN] = 1.0;
        f[frame::EFFECTS_VOLUME] = fx_vol;
        f[frame::AMBIENCE_VOLUME] = ambience_vol;
        f[frame::MUSIC_VOLUME] = music_vol;
        f[frame::VOICE_VOLUME] = vo_vol;
        f[frame::INTERFACE_VOLUME] = 1.0;
        f[frame::TICKETS1] = 100.0;
        f[frame::TICKETS2] = 100.0;
    }

    fn step_tick(&mut self) {
        let wall = self.output.frame as f64 / 48000.0;
        self.wall = wall;
        self.sp.runtime.clear_output();
        self.sp.runtime.phase.frame[frame::NOW] = wall;
        self.sp.runtime.phase.frame[frame::WALL_NOW] = wall;
        if !self.sp.runtime.phase.started {
            let mut start = [0.; event::STRIDE];
            start[event::KIND] = event_kind::START as f64;
            self.sp.runtime.events.insert(0, start);
        }
        let mut ground = FlatGround;
        self.sp.runtime.step(&mut ground);
        self.sp.runtime.events.clear();

        let room_mask = self.sp.runtime.info[info::ROOM_CHANGED] as u32;
        let room: [f64; audio::reverb::PARAM_COUNT] =
            std::array::from_fn(|i| self.sp.runtime.info[info::ROOM + i]);
        if room_mask != 0 {
            if let Some(master) = &mut self.output.master {
                master.room(&room, room_mask, false);
            }
        }
        let cmds = &self.sp.runtime.phase.engine.commands;
        // Count upscaled bank sample starts (`START` sample id at +2 >= enhanced_base).
        let mut ci = 0usize;
        while ci < cmds.len() {
            let op = cmds[ci] as u32;
            if op == audio::voices::START && ci + 2 < cmds.len() {
                let sample_id = cmds[ci + 2] as usize;
                if sample_id >= self.sp.enhanced_base {
                    self.upscaled_bank_starts += 1;
                }
            }
            let arity = audio::commands::arity(op);
            if arity == 0 {
                break;
            }
            ci += 1 + arity;
        }
        self.output.commands[..cmds.len()].copy_from_slice(cmds);
        self.output.apply(cmds.len());
        for row in self.sp.runtime.phase.engine.placements.as_chunks::<6>().0 {
            let row: [f32; 6] = std::array::from_fn(|i| row[i] as f32);
            self.output.mixer.take(&row, false);
        }

        // Route every stream request through the client's production `sp.streams` (`Assets`),
        // with no harness-side fallback so production failures are observed directly.
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
                    } else {
                        self.original_stream_loads += 1;
                    }
                    assert!(self.sp.streams.request(req), "stream asset request queue full");
                    let started = Instant::now();
                    let resp = loop {
                        if let Some(r) = self.sp.streams.receive() {
                            break r;
                        }
                        assert!(started.elapsed() < Duration::from_secs(20), "stream asset timeout");
                        std::thread::sleep(Duration::from_micros(200));
                    };
                    let input = &mut self.sp.runtime.input[..ready::STRIDE];
                    input.fill(0.0);
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
                        input[ready::FAILED] = 1.0;
                        self.failed_stream_loads += 1;
                    }
                    self.sp.runtime.ready();
                    if self.sp.runtime.info[info::ACCEPTED] != 0.0 {
                        if let Some(pcm) = resp.pcm {
                            self.output.voices.set_sample(resp.request.sample, Some(pcm.sample));
                        }
                        let cmds = &self.sp.runtime.phase.engine.commands;
                        self.output.commands[..cmds.len()].copy_from_slice(cmds);
                        self.output.apply(cmds.len());
                        for row in self.sp.runtime.phase.engine.placements.as_chunks::<6>().0 {
                            let row: [f32; 6] = std::array::from_fn(|i| row[i] as f32);
                            self.output.mixer.take(&row, false);
                        }
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

        // Give the background decode-ahead thread time to refill its ring when Opus streams play.
        self.ticks += 1;
        if self.upscaled_stream_loads > 0 && self.ticks.is_multiple_of(8) {
            std::thread::sleep(Duration::from_millis(3));
        }

        for _ in 0..FRAMES_PER_TICK {
            if self.cursor == FRAMES {
                self.output.process();
                self.cursor = 0;
            }
            self.pcm.push(self.output.output[self.cursor]);
            self.pcm.push(self.output.output[FRAMES + self.cursor]);
            self.cursor += 1;
        }
    }

    fn summary(&self) -> (f32, f64, usize) {
        let peak = self.pcm.iter().fold(0.0f32, |m, &v| m.max(v.abs()));
        let sum_sq: f64 = self.pcm.iter().map(|&v| (v as f64) * (v as f64)).sum();
        let rms = (sum_sq / self.pcm.len().max(1) as f64).sqrt();
        let nz = self.pcm.iter().filter(|&&v| v.abs() > 1e-6).count();
        (peak, rms, nz)
    }
}

fn audit_mission(root: &str, label: &str, map: &str, query: &str) -> Result<bfe_content_native::BattleContent, String> {
    let content = Loader::detect(root).load_for_family(GameFamily::Swbf2, map, query)?;
    let level = resolve_level_name(&content)?;
    let sp = SoundPrepared::new(&content)?;
    let segments = &sp.runtime.phase.streams.segments;
    let p = &sp.runtime.phase;
    let sw = p.swbf2().expect("Swbf2 phase must carry 2005 state");

    let total_sounds = p.engine.sounds.len();
    let total_samples = p.engine.samples.len();
    let total_streams = p.streams.definitions.len();
    let total_classes = p.classes.len();
    let saber_classes = sw
        .classes
        .iter()
        .filter(|c| c.saber_turn_on != NONE && c.saber_on_hum != NONE)
        .count();
    let boost_classes = sw.classes.iter().filter(|c| !c.boost_cues.is_empty()).count();
    let boost_cues: usize = sw.classes.iter().map(|c| c.boost_cues.len()).sum();
    let trick_classes = sw.classes.iter().filter(|c| c.trick_sound != NONE).count();
    let warmup_classes = sw.classes.iter().filter(|c| c.warm_up != NONE).count();
    let barrel_classes = sw.classes.iter().filter(|c| c.barrel != NONE).count();
    let building_loops = sw.vehicles.buildings.iter().filter(|b| {
        sw.classes.get(b.class).is_some_and(|c| c.building_sound != NONE)
    }).count();
    let building_total = sw.vehicles.buildings.len();
    let combo_triggers: usize = sw
        .combos
        .iter()
        .flat_map(|c| c.states.iter())
        .map(|s| s.triggers.len())
        .sum();
    let param_bindings: usize = p
        .layers
        .definitions
        .iter()
        .flat_map(|d| d.layers.iter())
        .map(|l| l.bindings.len())
        .sum();
    let prox_bindings = sw.vehicles.proximity_bindings.len();
    let resolved_regions = p
        .director
        .regions
        .iter()
        .filter(|r| r[region::SOUND] >= 0.0)
        .count();
    let total_regions = p.director.regions.len();
    let spaces = sw.ambience.spaces.len();
    let space_regions = sw.ambience.space_regions.len();
    let space_atts = sw.ambience.attenuations.len();
    let resolved_triggers = sw.ambience.triggers.iter().filter(|t| t.sound != NONE).count();
    let total_triggers = sw.ambience.triggers.len();
    let low_reinf = sw.director.low_reinforcements.len();
    let tracks_resolved = p.director.tracks.iter().filter(|t| t.sound != NONE).count();
    let tracks_total = p.director.tracks.len();
    let resolved_stream_sounds = p
        .engine
        .sounds
        .iter()
        .filter(|s| s.kind == 4 && s.stream != NONE && !s.entries.is_empty())
        .count();
    let total_stream_sounds = p.engine.sounds.iter().filter(|s| s.kind == 4).count();

    println!(
        "audit {label} level={level} sounds={total_sounds} samples={total_samples} streams={total_streams} segments={} classes={total_classes} sabers={saber_classes} combo_triggers={combo_triggers} boost={boost_classes}({boost_cues}_cues) tricks={trick_classes} warmup={warmup_classes} barrel={barrel_classes} bldg={building_loops}/{building_total} param_bindings={param_bindings}(prox={prox_bindings}) regions={resolved_regions}/{total_regions} spaces={spaces}({space_regions}_vols,{space_atts}_atts) triggers={resolved_triggers}/{total_triggers} doppler={:.2} low_reinf={low_reinf} tracks={tracks_resolved}/{tracks_total} stream_sounds={resolved_stream_sounds}/{total_stream_sounds}",
        segments.len(),
        p.engine.doppler_factor,
    );
    assert!(total_sounds > 0 && total_samples > 0 && total_streams > 0);
    assert!(tracks_resolved > 0);
    Ok(content)
}

fn verify_points_mode_music_stages(content: &bfe_content_native::BattleContent) {
    let mut sp = SoundPrepared::new(content).unwrap();
    let f = &mut sp.runtime.phase.frame;
    f.fill(0.0);
    f[frame::DT] = 1.0;
    f[frame::RATE] = 48000.0;
    f[frame::RUNNING] = 1.0;
    f[frame::ENHANCED] = 1.0;
    f[frame::CHANNELS] = 2.0;
    f[frame::MAX_VOICES] = 32.0;
    f[frame::VIRTUAL_VOICES] = 64.0;
    f[frame::CONFIGURATION] = 1.0;
    f[frame::FZ] = -1.0;
    f[frame::UY] = 1.0;
    f[frame::TEAM] = 1.0;
    f[frame::HUMAN] = 1.0;
    f[frame::OWN] = 1.0;
    f[frame::MUSIC_VOLUME] = 1.0;
    f[frame::TICKETS1] = 100.0;
    f[frame::TICKETS2] = 100.0;

    // Simulate Sound::frame re-importing MUSIC_SLOT with START = 100.0 (which truncates 0.99/0.10 in shared slots).
    let music_rows = sp.music.clone();
    for mut row in music_rows {
        row[music_slot::START] = 100.0;
        sp.runtime.input[..row.len()].copy_from_slice(&row);
        sp.runtime.import(import::MUSIC_SLOT, row.len());
    }
    let has_three_stages = {
        let sw = sp.runtime.phase.swbf2().unwrap();
        sw.director.stage_slots[1][0].is_some()
            && sw.director.stage_slots[1][1].is_some()
            && sw.director.stage_slots[1][2].is_some()
    };
    if !has_three_stages {
        return;
    }

    let mut now = 0.0;
    let mut step_sec = |sp: &mut SoundPrepared, secs: usize| {
        let mut ground = FlatGround;
        for _ in 0..secs {
            sp.runtime.clear_output();
            sp.runtime.phase.frame[frame::NOW] = now;
            sp.runtime.phase.frame[frame::WALL_NOW] = now;
            if !sp.runtime.phase.started {
                let mut start = [0.; event::STRIDE];
                start[event::KIND] = event_kind::START as f64;
                sp.runtime.events.insert(0, start);
            }
            sp.runtime.step(&mut ground);
            sp.runtime.events.clear();
            now += 1.0;
        }
    };

    sp.runtime.phase.set_mode_score_progress(1.0, 1.0);
    step_sec(&mut sp, 2);
    assert_eq!(sp.runtime.phase.director.stage, 0, "stage 0 at 1.0 remaining");
    let t0 = sp.runtime.phase.director.tracks.iter().position(|t| t.playing()).expect("stage 0 track playing");

    // Sweep below 0.99 threshold (0.95) and step past MinPlaybackTime (20 s).
    sp.runtime.phase.set_mode_score_progress(0.95, 0.95);
    step_sec(&mut sp, 25);
    assert_eq!(sp.runtime.phase.director.stage, 1, "stage 1 must activate below 0.99 after START=100 re-import");
    let t1 = sp.runtime.phase.director.tracks.iter().position(|t| t.playing()).expect("stage 1 track playing");
    assert_ne!(t1, t0, "stage 1 must switch to a new track after MinPlaybackTime");

    // Sweep below 0.10 threshold (0.05) and step past MinPlaybackTime (20 s).
    sp.runtime.phase.set_mode_score_progress(0.05, 0.05);
    step_sec(&mut sp, 25);
    assert_eq!(sp.runtime.phase.director.stage, 2, "stage 2 must activate below 0.10 after START=100 re-import");
    let t2 = sp.runtime.phase.director.tracks.iter().position(|t| t.playing()).expect("stage 2 track playing");
    assert_ne!(t2, t1, "stage 2 must switch to a new track after MinPlaybackTime");
    println!("proof points_mode_music_stages: ok (0 -> 1 -> 2, tracks {t0} -> {t1} -> {t2})");
}

fn verify_low_reinforcements_vo_teams(content: &bfe_content_native::BattleContent) {
    let check_rules = SoundPrepared::new(content).unwrap();
    let n_rules = check_rules.runtime.phase.swbf2().unwrap().director.low_reinforcements.len();
    if n_rules < 4 {
        return;
    }
    let mut seen_sounds = BTreeSet::new();
    for (label, listener, t1, t2) in [
        ("listener1_t1_low", 1usize, 5.0, 80.0),
        ("listener1_t2_low", 1, 80.0, 5.0),
        ("listener2_t2_low", 2, 80.0, 5.0),
        ("listener2_t1_low", 2, 5.0, 80.0),
    ] {
        let mut r = Renderer::new(content, true).unwrap();
        r.init_frame([0.0, 1.7, 0.0], 0.0, 0.0, 0.0, 1.0);
        r.sp.runtime.phase.frame[frame::TEAM] = listener as f64;
        // Tick 0 captures initial tickets (100, 100).
        r.step_tick();
        r.sp.runtime.phase.frame[frame::TICKETS1] = t1;
        r.sp.runtime.phase.frame[frame::TICKETS2] = t2;
        r.step_tick();
        r.step_tick();
        let active_vo: Vec<usize> = r
            .sp
            .runtime
            .phase
            .streams
            .census()
            .filter(|&(_, cat)| cat == VOICE)
            .map(|(snd, _)| snd)
            .collect();
        assert_eq!(
            active_vo.len(),
            1,
            "{label} must fire exactly 1 low-reinforcements VO line, got {active_vo:?}"
        );
        let sid = active_vo[0];
        let sname = r.sp.sound_names.get(sid).map_or("", String::as_str);
        println!("proof low_reinforcements_vo {label}: sound_id={sid} name={sname}");
        seen_sounds.insert(sid);
    }
    assert_eq!(
        seen_sounds.len(),
        4,
        "all 4 (listener, watched) low-reinforcements VO cases must resolve to 4 distinct lines"
    );
}

#[test]
#[ignore = "requires prepared 2005 content in BFE_SWBF2_CONTENT_DIR"]
fn swbf2_resolution_and_clips() {
    let root = std::env::var("BFE_SWBF2_CONTENT_DIR").expect("BFE_SWBF2_CONTENT_DIR");
    let spec_a = std::env::var("BFE_SWBF2_MISSION_A").expect("BFE_SWBF2_MISSION_A (map|query)");
    let spec_b = std::env::var("BFE_SWBF2_MISSION_B").expect("BFE_SWBF2_MISSION_B (map|query)");
    let (map_a, query_a) = spec_a.split_once('|').expect("MISSION_A must be map|query");
    let (map_b, query_b) = spec_b.split_once('|').expect("MISSION_B must be map|query");

    let content_a = audit_mission(&root, "mission_a", map_a, query_a).unwrap();
    let content_b = audit_mission(&root, "mission_b", map_b, query_b).unwrap();

    // Real-content functional proofs for points-mode music stages and low-reinforcements VO.
    verify_points_mode_music_stages(&content_b);
    verify_low_reinforcements_vo_teams(&content_a);
    verify_low_reinforcements_vo_teams(&content_b);

    let Ok(clips_dir) = std::env::var("BFE_SWBF2_CLIPS_DIR") else {
        return;
    };
    let clips_path = PathBuf::from(clips_dir);

    // Clip 1: Saber ignite + hum + real production ComboState swings + hit + deflect clash + bolt deflect + extinguish
    {
        let mut r = Renderer::new(&content_a, true).unwrap();
        r.init_frame([0.0, 1.7, 0.0], 0.0, 0.0, 1.0, 0.0);
        // Optional format: "weapon_class|combo_hex|STATE1@t1,STATE2@t2,STATE3@t3|DEFLECT@td"
        let (weapon_hash, combo_hash, swing_schedule, block_event) =
            if let Ok(spec) = std::env::var("BFE_SWBF2_SABER_SPEC") {
                let parts: Vec<&str> = spec.split('|').collect();
                assert!(parts.len() >= 4, "BFE_SWBF2_SABER_SPEC needs 4 |-separated fields");
                let wh = zero_hash(parts[0]);
                let ch = parse_hash_or_hex(parts[1]);
                let swings: Vec<(usize, u32)> = parts[2]
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(|item| {
                        let (st, ts) = item.split_once('@').expect("STATE@sec");
                        ((ts.parse::<f64>().unwrap() * 60.0).round() as usize, zero_hash(st))
                    })
                    .collect();
                let (bst, bts) = parts[3].split_once('@').expect("DEFLECT@sec");
                let blk = ((bts.parse::<f64>().unwrap() * 60.0).round() as usize, zero_hash(bst));
                (wh, ch, swings, Some(blk))
            } else {
                // Fallback: use first loaded combo machine with states and first saber class.
                let sw = r.sp.runtime.phase.swbf2().unwrap();
                let ch = sw.combos.iter().find(|c| c.states.len() >= 3).map(|c| c.combo_hash).unwrap_or(0);
                let states: Vec<u32> = sw
                    .combos
                    .iter()
                    .find(|c| c.combo_hash == ch)
                    .map(|c| c.states.iter().take(4).map(|s| s.state_hash).collect())
                    .unwrap_or_default();
                let swings = states
                    .iter()
                    .take(3)
                    .enumerate()
                    .map(|(i, &sh)| (40 + i * 42, sh))
                    .collect();
                let blk = states.get(3).map(|&sh| (180, sh));
                let wh = first_saber_weapon_hash(&content_a, &r.sp.runtime.phase);
                (wh, ch, swings, blk)
            };
        assert!(
            r.sp.runtime.phase.swbf2_class_by_hash(weapon_hash).is_some(),
            "saber weapon class must resolve by hash"
        );
        assert!(
            r.sp.runtime.phase.swbf2_combo_by_hash(combo_hash).is_some(),
            "saber combo machine must resolve by hash"
        );

        let p = &mut r.sp.runtime.phase;
        p.emitters[1][emitter::ID] = 1.0;
        p.emitters[1][emitter::KIND] = 1.0;
        p.emitters[1][emitter::ALIVE] = 1.0;
        p.emitters[1][emitter::GENERATION] = 1.0;
        p.emitters[1][emitter::Z] = -2.0;
        p.entity_count = 2;
        p.push_saber_event(SaberEvent::Ignite { owner: 1, weapon_class_hash: weapon_hash });

        let mut combo_fired_voices = 0usize;
        for tick in 0..300 {
            for &(st_tick, sh) in &swing_schedule {
                if tick == st_tick {
                    let before = r.sp.runtime.phase.engine.stats[2];
                    r.sp.runtime.phase.push_saber_event(SaberEvent::ComboState {
                        owner: 1,
                        combo_hash,
                        state_hash: sh,
                    });
                    r.step_tick();
                    if r.sp.runtime.phase.engine.stats[2] > before
                        || r.sp.runtime.phase.swbf2().unwrap().sabers.pending_delayed_count() > 0
                    {
                        combo_fired_voices += 1;
                    }
                    continue;
                }
            }
            if swing_schedule.iter().any(|&(st_tick, _)| st_tick == tick) {
                continue;
            }
            if tick == 138 {
                r.sp.runtime.phase.push_saber_event(SaberEvent::Hit {
                    owner: 1,
                    weapon_class_hash: weapon_hash,
                    position: [0.0, 1.5, -2.5],
                });
            }
            if let Some((btick, bsh)) = block_event {
                if tick == btick {
                    r.sp.runtime.phase.push_saber_event(SaberEvent::ComboState {
                        owner: 1,
                        combo_hash,
                        state_hash: bsh,
                    });
                }
            }
            if tick == 215 {
                r.sp.runtime.phase.push_saber_event(SaberEvent::Deflect {
                    owner: 1,
                    weapon_class_hash: weapon_hash,
                    position: [-0.4, 1.5, -2.0],
                });
            }
            if tick == 250 {
                r.sp.runtime.phase.push_saber_event(SaberEvent::Extinguish {
                    owner: 1,
                    weapon_class_hash: weapon_hash,
                });
            }
            r.step_tick();
        }
        let (peak, rms, nz) = r.summary();
        write_wav_stereo_16(&clips_path.join("01_saber_sequence.wav"), &r.pcm, 48000).unwrap();
        println!(
            "clip 01_saber_sequence.wav frames={} peak={peak:.5} rms={rms:.5} nonzero={nz} combo_triggers_hit={combo_fired_voices}",
            r.pcm.len() / 2
        );
        assert!(peak > 0.01 && nz > 10000 && combo_fired_voices >= 2);
    }

    // Clip 2: Hero blaster fire burst placed off-center left then right for audible stereo panning
    {
        let mut r = Renderer::new(&content_a, true).unwrap();
        r.init_frame([0.0, 1.7, 0.0], 0.0, 0.0, 1.0, 0.0);
        let blaster_cls = if let Ok(cls_name) = std::env::var("BFE_SWBF2_BLASTER_CLASS") {
            r.sp.runtime
                .phase
                .swbf2_class_by_hash(zero_hash(&cls_name))
                .expect("BFE_SWBF2_BLASTER_CLASS must resolve")
        } else {
            r.sp.runtime
                .phase
                .classes
                .iter()
                .enumerate()
                .position(|(i, row)| {
                    row[class::FIRE] >= 0.0
                        && r.sp
                            .runtime
                            .phase
                            .swbf2()
                            .unwrap()
                            .classes
                            .get(i)
                            .is_none_or(|c| c.saber_on_hum == NONE)
                })
                .expect("mission A has blaster class")
        };
        let p = &mut r.sp.runtime.phase;
        p.emitters[1][emitter::ID] = 1.0;
        p.emitters[1][emitter::KIND] = 1.0;
        p.emitters[1][emitter::ALIVE] = 1.0;
        p.emitters[1][emitter::GENERATION] = 1.0;
        p.emitters[1][emitter::X] = -3.0;
        p.emitters[1][emitter::Y] = 1.6;
        p.emitters[1][emitter::Z] = -3.0;
        p.entity_count = 2;
        p.weapons[1][weapon::ID] = 1.0;
        p.weapons[1][weapon::OWNER] = 1.0;
        p.weapons[1][weapon::CLASS] = blaster_cls as f64;
        p.weapons[1][weapon::SELECTED] = 1.0;
        p.weapon_count = 2;
        for tick in 0..210 {
            if tick == 90 {
                // Move shooter to the listener's right for the second burst.
                r.sp.runtime.phase.emitters[1][emitter::X] = 3.0;
            }
            if [15, 35, 55, 100, 120, 140].contains(&tick) {
                let mut ev = [0.; event::STRIDE];
                ev[event::KIND] = event_kind::ACTOR_FIRE as f64;
                ev[event::OWNER] = 1.0;
                ev[event::WEAPON] = 1.0;
                ev[event::CLASS] = blaster_cls as f64;
                r.sp.runtime.events.push(ev);
            }
            r.step_tick();
        }
        let (peak, rms, nz) = r.summary();
        let (l1, r1) = channel_rms(&r.pcm, 0.25, 1.3);
        let (l2, r2) = channel_rms(&r.pcm, 1.65, 2.7);
        write_wav_stereo_16(&clips_path.join("02_blaster_fire.wav"), &r.pcm, 48000).unwrap();
        println!(
            "clip 02_blaster_fire.wav frames={} peak={peak:.5} rms={rms:.5} nonzero={nz} burst1_LR=({l1:.4},{r1:.4}) burst2_LR=({l2:.4},{r2:.4})",
            r.pcm.len() / 2
        );
        assert!(peak > 0.01 && nz > 10000);
        assert!(l1 > r1 && r2 > l2, "off-center bursts must pan left then right");
    }

    // Clip 3: Starfighter accelerating kinematically consistent flyby with BoostSound cues and Doppler
    {
        let mut r = Renderer::new(&content_b, true).unwrap();
        r.init_frame([0.0, 2.0, 0.0], 0.0, 0.0, 1.0, 0.0);
        // Silence placed ship hull loops so the starfighter pass is clean and isolated.
        r.sp.runtime.phase.swbf2_mut().unwrap().vehicles.buildings.clear();
        let fighter_cls = if let Ok(cls_name) = std::env::var("BFE_SWBF2_FIGHTER_CLASS") {
            r.sp.runtime
                .phase
                .swbf2_class_by_hash(zero_hash(&cls_name))
                .expect("BFE_SWBF2_FIGHTER_CLASS must resolve")
        } else {
            r.sp.runtime
                .phase
                .classes
                .iter()
                .enumerate()
                .position(|(i, row)| {
                    row[class::ENGINE] >= 0.0
                        && r.sp
                            .runtime
                            .phase
                            .swbf2()
                            .unwrap()
                            .classes
                            .get(i)
                            .is_some_and(|c| c.boost_cues.len() >= 2)
                })
                .expect("mission B has starfighter with engine and multi boost cues")
        };
        let boost_count = r.sp.runtime.phase.swbf2().unwrap().classes[fighter_cls].boost_cues.len();
        let boost_sounds: BTreeSet<usize> = r
            .sp
            .runtime
            .phase
            .swbf2()
            .unwrap()
            .classes[fighter_cls]
            .boost_cues
            .iter()
            .map(|c| c.sound)
            .collect();

        let p = &mut r.sp.runtime.phase;
        p.emitters[2][emitter::ID] = 2.0;
        p.emitters[2][emitter::KIND] = 3.0;
        p.emitters[2][emitter::ALIVE] = 1.0;
        p.emitters[2][emitter::GENERATION] = 1.0;
        p.emitters[2][emitter::CLASS] = fighter_cls as f64;
        p.emitters[2][emitter::PILOTED] = 1.0;
        p.emitters[2][emitter::FLYING] = 1.0;
        p.emitters[2][emitter::FLIGHT] = 3.0;
        p.emitters[2][emitter::HEALTH] = 1000.0;
        p.emitters[2][emitter::MAX_HEALTH] = 1000.0;
        p.emitters[2][emitter::SPEED_LIMIT] = 100.0;
        p.entity_count = 3;

        let mut x = -140.0;
        let mut boost_starts = 0usize;
        let mut seen_boost_serials = BTreeSet::new();
        let mut pitch_samples = Vec::new();
        for tick in 0..360 {
            let frac = tick as f64 / 359.0;
            let vx = 10.0 + 85.0 * frac; // 10 -> 95 m/s across 6.0 s
            let e = &mut r.sp.runtime.phase.emitters[2];
            e[emitter::X] = x;
            e[emitter::Y] = 4.0;
            e[emitter::Z] = -14.0;
            e[emitter::VX] = vx;
            e[emitter::VY] = 0.0;
            e[emitter::VZ] = 0.0;
            r.step_tick();
            x += vx / 60.0;
            for v in &r.sp.runtime.phase.engine.voices {
                if !v.done && boost_sounds.contains(&v.sound) && seen_boost_serials.insert(v.serial) {
                    boost_starts += 1;
                }
            }
            if [30, 120, 200, 300].contains(&tick) {
                let engine_sample = r
                    .sp
                    .runtime
                    .phase
                    .engine
                    .voices
                    .iter()
                    .filter(|v| !v.done && v.real && v.looping)
                    .map(|v| (v.pitch_scale, v.gain_scale, v.radial))
                    .max_by(|a, b| a.0.total_cmp(&b.0))
                    .unwrap_or((0.0, 0.0, 0.0));
                pitch_samples.push((tick, x.round() as i32, vx.round() as i32, engine_sample));
            }
        }
        let (peak, rms, nz) = r.summary();
        write_wav_stereo_16(&clips_path.join("03_starfighter_flyby.wav"), &r.pcm, 48000).unwrap();
        println!(
            "clip 03_starfighter_flyby.wav frames={} peak={peak:.5} rms={rms:.5} nonzero={nz} boost_cues_authored={boost_count} boost_fired={boost_starts} samples={pitch_samples:?}",
            r.pcm.len() / 2
        );
        assert!(peak > 0.01 && nz > 10000);
        assert!(boost_starts >= 2, "accelerating pass must trigger multiple real BoostSound cues");
    }

    // Clip 4: Interior hangar ambience + space reverb (Classic space I3DL2 + quad ambient stream, buildings silenced)
    {
        let mut r = Renderer::new(&content_b, false).unwrap();
        r.sp.runtime.phase.swbf2_mut().unwrap().vehicles.buildings.clear();
        let best_space = r
            .sp
            .runtime
            .phase
            .swbf2()
            .unwrap()
            .ambience
            .spaces
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.room.decay.total_cmp(&b.room.decay))
            .map(|(i, _)| i)
            .expect("mission B has interior space");
        let space_pos = r
            .sp
            .runtime
            .phase
            .swbf2()
            .unwrap()
            .ambience
            .space_regions
            .iter()
            .find(|reg| reg.space == best_space)
            .map(|reg| [reg.matrix[12], reg.matrix[13], reg.matrix[14]])
            .unwrap_or([0.0, 0.0, 0.0]);
        let listener_pos = r
            .sp
            .runtime
            .phase
            .director
            .regions
            .iter()
            .filter(|reg| reg[region::SOUND] >= 0.0)
            .min_by(|a, b| {
                let da = (a[region::X] - space_pos[0]).hypot(a[region::Z] - space_pos[2]);
                let db = (b[region::X] - space_pos[0]).hypot(b[region::Z] - space_pos[2]);
                da.total_cmp(&db)
            })
            .map(|reg| [reg[region::X], reg[region::Y], reg[region::Z]])
            .unwrap_or(space_pos);
        r.init_frame(listener_pos, 0.0, 1.0, 0.85, 0.0);
        let shot_cls = if let Ok(cls_name) = std::env::var("BFE_SWBF2_HANGAR_SHOT_CLASS") {
            r.sp.runtime
                .phase
                .swbf2_class_by_hash(zero_hash(&cls_name))
                .expect("BFE_SWBF2_HANGAR_SHOT_CLASS must resolve")
        } else {
            // Pick the shortest crisp single-shot weapon fire sound (<= 1.0 s) so the reverb tail is isolated cleanly.
            r.sp.runtime
                .phase
                .classes
                .iter()
                .enumerate()
                .filter_map(|(i, row)| {
                    if row[class::FIRE] < 0.0 {
                        return None;
                    }
                    let sid = row[class::FIRE] as usize;
                    let snd = r.sp.runtime.phase.engine.sounds.get(sid)?;
                    let entry = snd.entries.first()?;
                    let dur = r.sp.runtime.phase.engine.samples.get(entry.id)?.duration;
                    (dur > 0.05 && dur <= 1.0).then_some((i, dur))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
                .or_else(|| r.sp.runtime.phase.classes.iter().position(|row| row[class::FIRE] >= 0.0))
                .unwrap_or(0)
        };
        let shot_dur = {
            let sid = r.sp.runtime.phase.classes[shot_cls][class::FIRE] as usize;
            r.sp.runtime
                .phase
                .engine
                .sounds
                .get(sid)
                .and_then(|s| s.entries.first())
                .and_then(|e| r.sp.runtime.phase.engine.samples.get(e.id))
                .map_or(0.9, |smp| smp.duration)
        };
        for tick in 0..420 {
            if tick == 90 || tick == 240 {
                let mut ev = [0.; event::STRIDE];
                ev[event::KIND] = event_kind::TURRET_FIRE as f64;
                ev[event::CLASS] = shot_cls as f64;
                ev[event::PLACED] = 1.0;
                ev[event::X] = listener_pos[0] + 4.0;
                ev[event::Y] = listener_pos[1];
                ev[event::Z] = listener_pos[2] - 4.0;
                r.sp.runtime.events.push(ev);
            }
            r.step_tick();
        }
        let (peak, rms, nz) = r.summary();
        let active_space = r.sp.runtime.phase.swbf2().unwrap().ambience.current_space;
        write_wav_stereo_16(&clips_path.join("04_hangar_ambience.wav"), &r.pcm, 48000).unwrap();

        // Measure dry vs space reverb tail AFTER the dry impulse ends (ambience muted in measurement passes).
        let tail_start = 0.5 + shot_dur + 0.15;
        let tail_mid = tail_start + 1.0;
        let tail_end = tail_start + 2.5;
        let measure_ticks = ((tail_end + 0.2) * 60.0).ceil() as usize;
        let measure_impulse = |pos: Point| -> (f64, f64, Option<usize>) {
            let mut m = Renderer::new(&content_b, false).unwrap();
            m.sp.runtime.phase.swbf2_mut().unwrap().vehicles.buildings.clear();
            m.init_frame(pos, 0.0, 0.0, 0.85, 0.0);
            for tick in 0..measure_ticks {
                if tick == 30 {
                    let mut ev = [0.; event::STRIDE];
                    ev[event::KIND] = event_kind::TURRET_FIRE as f64;
                    ev[event::CLASS] = shot_cls as f64;
                    ev[event::PLACED] = 1.0;
                    ev[event::X] = pos[0] + 4.0;
                    ev[event::Y] = pos[1];
                    ev[event::Z] = pos[2] - 4.0;
                    m.sp.runtime.events.push(ev);
                }
                m.step_tick();
            }
            (
                rms_db_window(&m.pcm, tail_start, tail_mid),
                rms_db_window(&m.pcm, tail_mid, tail_end),
                m.sp.runtime.phase.swbf2().unwrap().ambience.current_space,
            )
        };
        let (wet_early, wet_late, wet_space) = measure_impulse(listener_pos);
        let (dry_early, dry_late, dry_space) = measure_impulse([5000.0, 5000.0, 5000.0]);
        println!(
            "clip 04_hangar_ambience.wav frames={} peak={peak:.5} rms={rms:.5} nonzero={nz} space={active_space:?} shot_dur={shot_dur:.3}s tail_win=[{tail_start:.2}..{tail_mid:.2}..{tail_end:.2}] reverb_tail_dB wet=({wet_early:.1},{wet_late:.1},space={wet_space:?}) dry=({dry_early:.1},{dry_late:.1},space={dry_space:?})",
            r.pcm.len() / 2
        );
        assert!(peak > 0.005 && nz > 10000 && active_space.is_some());
        assert!(
            wet_early > dry_early + 12.0,
            "hangar space must produce a measurable reverb tail above dry ({wet_early:.1} dB vs {dry_early:.1} dB)"
        );
    }

    // Clip 5: Mission A ambient music (12 s)
    {
        let mut r = Renderer::new(&content_a, true).unwrap();
        r.init_frame([0.0, 1.7, 0.0], 1.0, 0.0, 0.0, 0.0);
        r.sp.runtime.phase.set_mode_score_progress(1.0, 1.0);
        for _ in 0..720 {
            r.step_tick();
        }
        let (peak, rms, nz) = r.summary();
        write_wav_stereo_16(&clips_path.join("05_mission_a_music.wav"), &r.pcm, 48000).unwrap();
        println!("clip 05_mission_a_music.wav frames={} peak={peak:.5} rms={rms:.5} nonzero={nz}", r.pcm.len() / 2);
        assert!(peak > 0.01 && nz > 10000);
    }

    // Clip 6: Mission B ambient music + genuine Mission B objective VO ducking (12 s)
    {
        let mut r = Renderer::new(&content_b, true).unwrap();
        r.init_frame([0.0, 1.7, 0.0], 1.0, 0.0, 0.0, 1.0);
        r.sp.runtime.phase.set_mode_score_progress(1.0, 1.0);
        let vo_hash = if let Ok(vo_name) = std::env::var("BFE_SWBF2_VO_LINE") {
            let h = parse_hash_or_hex(&vo_name);
            assert!(
                r.sp.runtime.phase.swbf2_sound_by_hash(h).is_some(),
                "BFE_SWBF2_VO_LINE must resolve in mission B bank"
            );
            Some(h)
        } else {
            r.sp.runtime
                .phase
                .sound_names
                .iter()
                .find(|&&(_, id)| {
                    r.sp.runtime.phase.engine.sounds.get(id).is_some_and(|s| {
                        s.kind == 4
                            && !s.entries.is_empty()
                            && r.sp.voice_segments.get(s.entries[0].id).copied().unwrap_or(false)
                    })
                })
                .map(|&(h, _)| h)
        };
        let mut vo_duck_observed = false;
        for tick in 0..720 {
            if tick == 180 {
                if let Some(h) = vo_hash {
                    r.sp.runtime.phase.broadcast_voice_over(h, 1);
                }
            }
            r.step_tick();
            if tick > 185 && tick < 360 {
                // Check whether any bus is ducked below its base gain while VO speaks.
                if r.sp.runtime.phase.engine.buses.iter().any(|b| b.duck < 0.95) {
                    vo_duck_observed = true;
                }
            }
        }
        let (peak, rms, nz) = r.summary();
        write_wav_stereo_16(&clips_path.join("06_mission_b_music.wav"), &r.pcm, 48000).unwrap();
        println!(
            "clip 06_mission_b_music.wav frames={} peak={peak:.5} rms={rms:.5} nonzero={nz} vo_triggered={} vo_ducked={vo_duck_observed}",
            r.pcm.len() / 2,
            vo_hash.is_some()
        );
        assert!(peak > 0.01 && nz > 10000 && vo_hash.is_some() && vo_duck_observed);
    }
}

fn measure_channel_loop_wrap(data: &[f32], channels: usize, frames: usize) -> (f32, f32, f32) {
    let mut worst_wrap = 0.0f32;
    let mut max_interior_step = 0.0f32;
    let mut peak = 0.0f32;
    for ch in 0..channels {
        let plane = &data[ch * frames..(ch + 1) * frames];
        if frames < 2 {
            continue;
        }
        let wrap = (plane[0] - plane[frames - 1]).abs();
        worst_wrap = worst_wrap.max(wrap);
        for i in 1..frames {
            peak = peak.max(plane[i].abs());
            let step = (plane[i] - plane[i - 1]).abs();
            max_interior_step = max_interior_step.max(step);
        }
        peak = peak.max(plane[0].abs());
    }
    (worst_wrap, max_interior_step, peak)
}

fn render_saber_sequence(
    content: &bfe_content_native::BattleContent,
    enhanced: bool,
    use_upscaled: bool,
    dynamics: &str,
) -> Renderer {
    let mut r = Renderer::new_with_mode(content, enhanced, use_upscaled, dynamics).unwrap();
    r.init_frame([0.0, 1.7, 0.0], 0.0, 0.0, 1.0, 0.0);
    let (weapon_hash, combo_hash, swing_schedule, block_event) =
        if let Ok(spec) = std::env::var("BFE_SWBF2_SABER_SPEC") {
            let parts: Vec<&str> = spec.split('|').collect();
            assert!(parts.len() >= 4, "BFE_SWBF2_SABER_SPEC needs 4 |-separated fields");
            let wh = zero_hash(parts[0]);
            let ch = parse_hash_or_hex(parts[1]);
            let swings: Vec<(usize, u32)> = parts[2]
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|item| {
                    let (st, ts) = item.split_once('@').expect("STATE@sec");
                    ((ts.parse::<f64>().unwrap() * 60.0).round() as usize, zero_hash(st))
                })
                .collect();
            let (bst, bts) = parts[3].split_once('@').expect("DEFLECT@sec");
            let blk = ((bts.parse::<f64>().unwrap() * 60.0).round() as usize, zero_hash(bst));
            (wh, ch, swings, Some(blk))
        } else {
            let sw = r.sp.runtime.phase.swbf2().unwrap();
            let ch = sw.combos.iter().find(|c| c.states.len() >= 3).map(|c| c.combo_hash).unwrap_or(0);
            let states: Vec<u32> = sw
                .combos
                .iter()
                .find(|c| c.combo_hash == ch)
                .map(|c| c.states.iter().take(4).map(|s| s.state_hash).collect())
                .unwrap_or_default();
            let swings = states.iter().take(3).enumerate().map(|(i, &sh)| (40 + i * 42, sh)).collect();
            let blk = states.get(3).map(|&sh| (180, sh));
            let wh = first_saber_weapon_hash(content, &r.sp.runtime.phase);
            (wh, ch, swings, blk)
        };
    let p = &mut r.sp.runtime.phase;
    p.emitters[1][emitter::ID] = 1.0;
    p.emitters[1][emitter::KIND] = 1.0;
    p.emitters[1][emitter::ALIVE] = 1.0;
    p.emitters[1][emitter::GENERATION] = 1.0;
    p.emitters[1][emitter::Z] = -2.0;
    p.entity_count = 2;
    p.push_saber_event(SaberEvent::Ignite { owner: 1, weapon_class_hash: weapon_hash });

    for tick in 0..300 {
        for &(st_tick, sh) in &swing_schedule {
            if tick == st_tick {
                r.sp.runtime.phase.push_saber_event(SaberEvent::ComboState {
                    owner: 1,
                    combo_hash,
                    state_hash: sh,
                });
            }
        }
        if tick == 138 {
            r.sp.runtime.phase.push_saber_event(SaberEvent::Hit {
                owner: 1,
                weapon_class_hash: weapon_hash,
                position: [0.0, 1.5, -2.5],
            });
        }
        if let Some((btick, bsh)) = block_event {
            if tick == btick {
                r.sp.runtime.phase.push_saber_event(SaberEvent::ComboState {
                    owner: 1,
                    combo_hash,
                    state_hash: bsh,
                });
            }
        }
        if tick == 215 {
            r.sp.runtime.phase.push_saber_event(SaberEvent::Deflect {
                owner: 1,
                weapon_class_hash: weapon_hash,
                position: [-0.4, 1.5, -2.0],
            });
        }
        if tick == 250 {
            r.sp.runtime.phase.push_saber_event(SaberEvent::Extinguish {
                owner: 1,
                weapon_class_hash: weapon_hash,
            });
        }
        r.step_tick();
    }
    r
}

fn render_hangar_ambience(
    content: &bfe_content_native::BattleContent,
    enhanced: bool,
    use_upscaled: bool,
    dynamics: &str,
) -> Renderer {
    let mut r = Renderer::new_with_mode(content, enhanced, use_upscaled, dynamics).unwrap();
    r.sp.runtime.phase.swbf2_mut().unwrap().vehicles.buildings.clear();
    let best_space = r
        .sp
        .runtime
        .phase
        .swbf2()
        .unwrap()
        .ambience
        .spaces
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.room.decay.total_cmp(&b.room.decay))
        .map(|(i, _)| i)
        .expect("mission B has interior space");
    let space_pos = r
        .sp
        .runtime
        .phase
        .swbf2()
        .unwrap()
        .ambience
        .space_regions
        .iter()
        .find(|reg| reg.space == best_space)
        .map(|reg| [reg.matrix[12], reg.matrix[13], reg.matrix[14]])
        .unwrap_or([0.0, 0.0, 0.0]);
    let listener_pos = r
        .sp
        .runtime
        .phase
        .director
        .regions
        .iter()
        .filter(|reg| reg[region::SOUND] >= 0.0)
        .min_by(|a, b| {
            let da = (a[region::X] - space_pos[0]).hypot(a[region::Z] - space_pos[2]);
            let db = (b[region::X] - space_pos[0]).hypot(b[region::Z] - space_pos[2]);
            da.total_cmp(&db)
        })
        .map(|reg| [reg[region::X], reg[region::Y], reg[region::Z]])
        .unwrap_or(space_pos);
    r.init_frame(listener_pos, 0.0, 1.0, 0.85, 0.0);
    let shot_cls = if let Ok(cls_name) = std::env::var("BFE_SWBF2_HANGAR_SHOT_CLASS") {
        r.sp.runtime
            .phase
            .swbf2_class_by_hash(zero_hash(&cls_name))
            .expect("BFE_SWBF2_HANGAR_SHOT_CLASS must resolve")
    } else {
        r.sp.runtime
            .phase
            .classes
            .iter()
            .enumerate()
            .filter_map(|(i, row)| {
                if row[class::FIRE] < 0.0 {
                    return None;
                }
                let sid = row[class::FIRE] as usize;
                let snd = r.sp.runtime.phase.engine.sounds.get(sid)?;
                let entry = snd.entries.first()?;
                let dur = r.sp.runtime.phase.engine.samples.get(entry.id)?.duration;
                (dur > 0.05 && dur <= 1.0).then_some((i, dur))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
            .or_else(|| r.sp.runtime.phase.classes.iter().position(|row| row[class::FIRE] >= 0.0))
            .unwrap_or(0)
    };
    for tick in 0..420 {
        if tick == 90 || tick == 240 {
            let mut ev = [0.; event::STRIDE];
            ev[event::KIND] = event_kind::TURRET_FIRE as f64;
            ev[event::CLASS] = shot_cls as f64;
            ev[event::PLACED] = 1.0;
            ev[event::X] = listener_pos[0] + 4.0;
            ev[event::Y] = listener_pos[1];
            ev[event::Z] = listener_pos[2] - 4.0;
            r.sp.runtime.events.push(ev);
        }
        r.step_tick();
    }
    r
}

fn render_music_clip(
    content: &bfe_content_native::BattleContent,
    enhanced: bool,
    use_upscaled: bool,
    dynamics: &str,
    ticks: usize,
) -> Renderer {
    let mut r = Renderer::new_with_mode(content, enhanced, use_upscaled, dynamics).unwrap();
    r.init_frame([0.0, 1.7, 0.0], 1.0, 0.0, 0.0, 0.0);
    r.sp.runtime.phase.set_mode_score_progress(1.0, 1.0);
    for _ in 0..ticks {
        r.step_tick();
    }
    r
}

#[test]
#[ignore = "requires prepared 2005 content with audio/enhanced in BFE_SWBF2_CONTENT_DIR"]
fn swbf2_enhanced_upscaled_proof() {
    let root = std::env::var("BFE_SWBF2_CONTENT_DIR").expect("BFE_SWBF2_CONTENT_DIR");
    let spec_a = std::env::var("BFE_SWBF2_MISSION_A").expect("BFE_SWBF2_MISSION_A (map|query)");
    let spec_b = std::env::var("BFE_SWBF2_MISSION_B").expect("BFE_SWBF2_MISSION_B (map|query)");
    let (map_a, query_a) = spec_a.split_once('|').expect("MISSION_A must be map|query");
    let (map_b, query_b) = spec_b.split_once('|').expect("MISSION_B must be map|query");

    let content_a = Loader::detect(&root).load_for_family(GameFamily::Swbf2, map_a, query_a).unwrap();
    let content_b = Loader::detect(&root).load_for_family(GameFamily::Swbf2, map_b, query_b).unwrap();

    // 1. Audit upscaled sample/variant coverage and loop-wrap continuity across all looped FLACs and 4ch Opus beds.
    for (label, content) in [("mission_a", &content_a), ("mission_b", &content_b)] {
        let level = resolve_level_name(content).unwrap();
        let sp = SoundPrepared::new(content).unwrap();
        let total_segments = sp.runtime.phase.streams.segments.len();
        let segs_with_variant = sp
            .runtime
            .phase
            .streams
            .segments
            .iter()
            .filter(|s| s.variants[3] != NONE)
            .count();
        println!(
            "enhanced_audit {label} level={level} enhanced_samples={} total_samples={} stream_segments={} segs_with_variant={}",
            sp.enhanced.len(),
            sp.runtime.phase.engine.samples.len(),
            total_segments,
            segs_with_variant,
        );
        assert!(!sp.enhanced.is_empty(), "{label}: must load non-fallback upscaled FLAC bank samples");
        assert!(segs_with_variant > 0, "{label}: must load upscaled Opus stream variants");

        let manifest: serde_json::Value = serde_json::from_slice(
            &content.textures.read(&format!("audio/enhanced/{level}/manifest.json")).unwrap(),
        )
        .unwrap();
        let mut loop_checked = 0usize;
        let mut max_wrap_ratio = 0.0f32;
        for (name, entry) in manifest["samples"].as_object().unwrap() {
            if entry["fallback"].as_bool().unwrap_or(false) || !entry["loop"].as_bool().unwrap_or(false) {
                continue;
            }
            let rel = entry["file"].as_str().unwrap();
            let path = content.textures.path(&format!("audio/enhanced/{level}/{rel}")).unwrap();
            let bytes = std::fs::read(&path).unwrap();
            let gain_db = entry["playback_db"].as_f64().unwrap_or(0.0);
            let pcm = enhanced_audio::decode(&bytes, gain_db, 0).unwrap();
            let expected_frames = entry["loop_frames"][1].as_u64().unwrap() as usize;
            assert_eq!(
                pcm.frames, expected_frames,
                "{label} sample {name}: decoded FLAC frames must match loop_frames"
            );
            let (wrap, max_step, peak) = measure_channel_loop_wrap(&pcm.sample.data, pcm.channels, pcm.frames);
            if peak > 1e-4 && max_step > 1e-5 {
                max_wrap_ratio = max_wrap_ratio.max(wrap / max_step);
            }
            loop_checked += 1;
        }
        println!(
            "loop_audit {label} level={level} flac_loops_checked={loop_checked} worst_wrap_to_max_step_ratio={max_wrap_ratio:.3}"
        );
        assert!(loop_checked > 0, "{label}: must verify looped FLAC samples");

        // Verify 4ch quad ambience Opus loop segment(s).
        for (sname, segs) in manifest["streams"].as_object().unwrap() {
            for (skey, entry) in segs.as_object().unwrap() {
                if entry["fallback"].as_bool().unwrap_or(true) || entry["channels"].as_u64() != Some(4) {
                    continue;
                }
                if let Some(rel) = entry["files"]["4ch"].as_str() {
                    let path = content.textures.path(&format!("audio/enhanced/{level}/{rel}")).unwrap();
                    let bytes = std::fs::read(&path).unwrap();
                    let (sample, frames, channels, _) =
                        decode_stream_chunked(&bytes, entry["playback_db"].as_f64().unwrap_or(0.0)).unwrap();
                    let expected_frames = entry["loop_frames"][1].as_u64().unwrap() as usize;
                    assert_eq!(channels, 4, "{label} {sname}/{skey}: quad Opus must decode 4 channels");
                    assert_eq!(
                        frames, expected_frames,
                        "{label} {sname}/{skey}: decoded Opus frames must match loop_frames"
                    );
                    let (wrap, max_step, peak) = measure_channel_loop_wrap(&sample.data, channels, frames);
                    println!(
                        "quad_loop_audit {label} {sname}/{skey} ch={channels} frames={frames} wrap={wrap:.5} max_step={max_step:.5} peak={peak:.5}"
                    );
                    assert!(peak > 0.001);
                }
            }
        }
    }

    // 2. Verify live rendered loop-wrap continuity on lightsaber hum (Mission A) and starfighter engine (Mission B).
    {
        let mut r = Renderer::new_with_mode(&content_a, true, true, "normal").unwrap();
        r.init_frame([0.0, 1.7, 0.0], 0.0, 0.0, 1.0, 0.0);
        let weapon_hash = std::env::var("BFE_SWBF2_SABER_SPEC")
            .ok()
            .and_then(|s| s.split('|').next().map(zero_hash))
            .unwrap_or_else(|| first_saber_weapon_hash(&content_a, &r.sp.runtime.phase));
        let hum_dur = {
            let sw = r.sp.runtime.phase.swbf2().unwrap();
            let cid = sw.class_by_hash(weapon_hash).unwrap();
            let sid = sw.classes[cid].saber_on_hum;
            let sample_idx = r.sp.runtime.phase.engine.sounds[sid].entries[0].id;
            r.sp.runtime.phase.engine.enhanced_samples[sample_idx]
                .map(|s| s.duration)
                .unwrap_or(1.0)
        };
        let p = &mut r.sp.runtime.phase;
        p.emitters[1][emitter::ID] = 1.0;
        p.emitters[1][emitter::KIND] = 1.0;
        p.emitters[1][emitter::ALIVE] = 1.0;
        p.emitters[1][emitter::GENERATION] = 1.0;
        p.emitters[1][emitter::Z] = -2.0;
        p.entity_count = 2;
        p.push_saber_event(SaberEvent::Ignite { owner: 1, weapon_class_hash: weapon_hash });
        let ticks = ((hum_dur * 4.5 + 1.0) * 60.0).ceil() as usize;
        for _ in 0..ticks {
            r.step_tick();
        }
        // Inspect steady-state region (after ignite settles at 1.0 s) around loop boundaries vs interior.
        let steady_start = 48000usize;
        let total_frames = r.pcm.len() / 2;
        let mut max_steady_step = 0.0f32;
        let mut steady_peak = 0.0f32;
        for f in (steady_start + 1)..total_frames {
            for ch in 0..2 {
                let a = r.pcm[(f - 1) * 2 + ch];
                let b = r.pcm[f * 2 + ch];
                steady_peak = steady_peak.max(b.abs());
                max_steady_step = max_steady_step.max((b - a).abs());
            }
        }
        println!(
            "live_saber_hum_loop dur={hum_dur:.4}s wraps>=4 upscaled_starts={} steady_peak={steady_peak:.5} max_step={max_steady_step:.5}",
            r.upscaled_bank_starts
        );
        assert!(r.upscaled_bank_starts >= 2, "saber ignite+hum must use upscaled FLAC bank samples");
        assert!(steady_peak > 0.01 && max_steady_step < steady_peak * 0.5, "hum loop wraps must stay click-free");
    }

    // 2b. Verify starfighter engine loop wraps cleanly with upscaled FLAC and appended VO streams load Opus variants.
    {
        let mut r = Renderer::new_with_mode(&content_b, true, true, "normal").unwrap();
        r.sp.runtime.phase.swbf2_mut().unwrap().vehicles.buildings.clear();
        r.init_frame([0.0, 2.0, 0.0], 0.0, 0.0, 1.0, 1.0);
        let fighter_cls = std::env::var("BFE_SWBF2_FIGHTER_CLASS")
            .ok()
            .and_then(|n| r.sp.runtime.phase.swbf2_class_by_hash(zero_hash(&n)))
            .unwrap_or_else(|| {
                r.sp.runtime
                    .phase
                    .classes
                    .iter()
                    .position(|row| row[class::ENGINE] >= 0.0)
                    .expect("mission B has engine class")
            });
        let p = &mut r.sp.runtime.phase;
        p.emitters[2][emitter::ID] = 2.0;
        p.emitters[2][emitter::KIND] = 3.0;
        p.emitters[2][emitter::ALIVE] = 1.0;
        p.emitters[2][emitter::GENERATION] = 1.0;
        p.emitters[2][emitter::CLASS] = fighter_cls as f64;
        p.emitters[2][emitter::PILOTED] = 1.0;
        p.emitters[2][emitter::FLYING] = 1.0;
        p.emitters[2][emitter::FLIGHT] = 3.0;
        p.emitters[2][emitter::HEALTH] = 1000.0;
        p.emitters[2][emitter::MAX_HEALTH] = 1000.0;
        p.emitters[2][emitter::SPEED_LIMIT] = 100.0;
        p.emitters[2][emitter::Z] = -12.0;
        p.emitters[2][emitter::VX] = 25.0;
        p.entity_count = 3;
        let vo_hash = std::env::var("BFE_SWBF2_VO_LINE").ok().map(|s| parse_hash_or_hex(&s));
        for tick in 0..240 {
            if tick == 60 {
                if let Some(h) = vo_hash {
                    r.sp.runtime.phase.broadcast_voice_over(h, 1);
                }
            }
            r.step_tick();
        }
        let steady_start = 48000usize;
        let total_frames = r.pcm.len() / 2;
        let mut max_step = 0.0f32;
        let mut peak = 0.0f32;
        for f in (steady_start + 1)..total_frames {
            for ch in 0..2 {
                let a = r.pcm[(f - 1) * 2 + ch];
                let b = r.pcm[f * 2 + ch];
                peak = peak.max(b.abs());
                max_step = max_step.max((b - a).abs());
            }
        }
        println!(
            "live_fighter_and_appended_vo upscaled_bank_starts={} upscaled_stream_loads={} peak={peak:.5} max_step={max_step:.5}",
            r.upscaled_bank_starts,
            r.upscaled_stream_loads
        );
        assert!(r.upscaled_bank_starts >= 1, "starfighter engine must use upscaled FLAC samples");
        if vo_hash.is_some() {
            assert!(r.upscaled_stream_loads >= 1, "appended objective VO must stream upscaled Opus variant");
        }
        assert!(peak > 0.005 && max_step < peak * 0.75, "engine loop wraps must stay click-free");
    }

    // 3. Render saber sequence & hangar ambience across Enhanced (upscaled), Enhanced (original), and Classic (original).
    let r3_dir = std::env::var("BFE_SWBF2_R3_CLIPS_DIR").ok().map(PathBuf::from);
    let emit_clip = |name: &str, r: &Renderer| -> LoudnessStats {
        let (peak, rms, nz) = r.summary();
        let stats = measure_bs1770(&r.pcm);
        if let Some(dir) = &r3_dir {
            write_wav_stereo_16(&dir.join(name), &r.pcm, 48000).unwrap();
        }
        println!(
            "r3_clip {name} frames={} peak={peak:.5} rms={rms:.5} nz={nz} lufs={:.2} lra={:.2} tp={:.2}dBTP enh_loaded={} enh_bank_starts={} enh_stream_loads={} orig_stream_loads={}",
            r.pcm.len() / 2,
            stats.integrated_lufs,
            stats.lra_lu,
            stats.true_peak_dbtp,
            r.enhanced_samples_loaded,
            r.upscaled_bank_starts,
            r.upscaled_stream_loads,
            r.original_stream_loads,
        );
        assert!(peak > 0.005 && nz > 10000, "{name} must render audible audio");
        assert!(stats.true_peak_dbtp <= -0.95, "{name} true peak must stay <= -1 dBTP (got {:.2})", stats.true_peak_dbtp);
        stats
    };

    let saber_enh = render_saber_sequence(&content_a, true, true, "normal");
    let saber_orig = render_saber_sequence(&content_a, true, false, "normal");
    let saber_cls = render_saber_sequence(&content_a, false, false, "normal");
    let s_enh = emit_clip("01_saber_enhanced.wav", &saber_enh);
    let s_orig = emit_clip("01_saber_original.wav", &saber_orig);
    let s_cls = emit_clip("01_saber_classic.wav", &saber_cls);
    assert!(saber_enh.upscaled_bank_starts >= 4, "saber sequence must play upscaled FLAC samples");
    assert_eq!(saber_orig.upscaled_bank_starts, 0);
    assert!(
        (s_enh.integrated_lufs - s_orig.integrated_lufs).abs() <= 0.5,
        "saber Enhanced upscaled vs original loudness delta exceeded 0.5 LU: {:.2} vs {:.2}",
        s_enh.integrated_lufs,
        s_orig.integrated_lufs
    );

    let hangar_enh = render_hangar_ambience(&content_b, true, true, "normal");
    let hangar_orig = render_hangar_ambience(&content_b, true, false, "normal");
    let hangar_cls = render_hangar_ambience(&content_b, false, false, "normal");
    let h_enh = emit_clip("04_hangar_enhanced.wav", &hangar_enh);
    let h_orig = emit_clip("04_hangar_original.wav", &hangar_orig);
    let h_cls = emit_clip("04_hangar_classic.wav", &hangar_cls);
    assert!(hangar_enh.upscaled_stream_loads >= 1, "hangar ambience must stream upscaled quad Opus bed");
    assert_eq!(hangar_orig.upscaled_stream_loads, 0);
    assert!(
        (h_enh.integrated_lufs - h_orig.integrated_lufs).abs() <= 0.5,
        "hangar Enhanced upscaled vs original loudness delta exceeded 0.5 LU: {:.2} vs {:.2}",
        h_enh.integrated_lufs,
        h_orig.integrated_lufs
    );

    // Also measure BS.1770 loudness on 12 s ambient music (stereo Opus vs ADPCM) on both missions.
    for (label, content) in [("music_a", &content_a), ("music_b", &content_b)] {
        let m_enh = render_music_clip(content, true, true, "normal", 720);
        let m_orig = render_music_clip(content, true, false, "normal", 720);
        let m_cls = render_music_clip(content, false, false, "normal", 720);
        let l_enh = measure_bs1770(&m_enh.pcm);
        let l_orig = measure_bs1770(&m_orig.pcm);
        let l_cls = measure_bs1770(&m_cls.pcm);
        println!(
            "r3_music {label} enh_up={:.2}LUFS(tp={:.2},st_up={}) enh_orig={:.2}LUFS(tp={:.2}) classic={:.2}LUFS(tp={:.2}) delta_up_vs_orig={:+.2}LU delta_up_vs_cls={:+.2}LU",
            l_enh.integrated_lufs,
            l_enh.true_peak_dbtp,
            m_enh.upscaled_stream_loads,
            l_orig.integrated_lufs,
            l_orig.true_peak_dbtp,
            l_cls.integrated_lufs,
            l_cls.true_peak_dbtp,
            l_enh.integrated_lufs - l_orig.integrated_lufs,
            l_enh.integrated_lufs - l_cls.integrated_lufs,
        );
        assert!(m_enh.upscaled_stream_loads >= 1, "{label}: must load upscaled music Opus variant");
        assert!(
            (l_enh.integrated_lufs - l_orig.integrated_lufs).abs() <= 0.5,
            "{label}: upscaled vs original music loudness delta exceeded 0.5 LU"
        );
    }

    // 4. Verify missing audio/enhanced/ folder falls back cleanly to original 2005 samples & streams in Enhanced mode.
    if let Ok(fallback_root) = std::env::var("BFE_SWBF2_FALLBACK_CONTENT_DIR") {
        let fb_a = Loader::detect(&fallback_root).load_for_family(GameFamily::Swbf2, map_a, query_a).unwrap();
        let fb_b = Loader::detect(&fallback_root).load_for_family(GameFamily::Swbf2, map_b, query_b).unwrap();
        let sp_a = SoundPrepared::new(&fb_a).unwrap();
        let sp_b = SoundPrepared::new(&fb_b).unwrap();
        assert!(sp_a.enhanced.is_empty() && sp_b.enhanced.is_empty(), "without enhanced folder, enhanced list must be empty");
        assert!(
            sp_a.runtime.phase.streams.segments.iter().all(|s| s.variants == [NONE; 4]),
            "without enhanced folder, segments must have no variants"
        );
        let r_saber_fb = render_saber_sequence(&fb_a, true, true, "normal");
        let r_hangar_fb = render_hangar_ambience(&fb_b, true, true, "normal");
        let (sp_peak, _, sp_nz) = r_saber_fb.summary();
        let (hg_peak, _, hg_nz) = r_hangar_fb.summary();
        println!(
            "fallback_no_enhanced saber(peak={sp_peak:.5},nz={sp_nz},up_starts={}) hangar(peak={hg_peak:.5},nz={hg_nz},orig_st={})",
            r_saber_fb.upscaled_bank_starts,
            r_hangar_fb.original_stream_loads,
        );
        assert!(sp_peak > 0.01 && sp_nz > 10000 && r_saber_fb.upscaled_bank_starts == 0);
        assert!(hg_peak > 0.005 && hg_nz > 10000 && r_hangar_fb.original_stream_loads >= 1);
    }
    println!(
        "r3_summary saber_delta_up_vs_orig={:+.2}LU saber_delta_up_vs_cls={:+.2}LU hangar_delta_up_vs_orig={:+.2}LU hangar_delta_up_vs_cls={:+.2}LU",
        s_enh.integrated_lufs - s_orig.integrated_lufs,
        s_enh.integrated_lufs - s_cls.integrated_lufs,
        h_enh.integrated_lufs - h_orig.integrated_lufs,
        h_enh.integrated_lufs - h_cls.integrated_lufs,
    );
}

#[derive(Clone, Copy)]
enum DamageKind {
    Missing,
    CutHalf,
}

#[derive(Clone, Copy)]
enum StreamTarget {
    Bed,
    Music,
}

fn clone_tree_hardlink(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            clone_tree_hardlink(&from, &to)?;
        } else if ty.is_file() && std::fs::hard_link(&from, &to).is_err() {
            #[cfg(unix)]
            if std::os::unix::fs::symlink(&from, &to).is_ok() {
                continue;
            }
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Discovers the Enhanced stream `.opus` files belonging to active ambient beds or music tracks
/// dynamically from the level's prepared manifests (no hardcoded game stream names).
fn target_enhanced_stream_files(
    root: &Path,
    content: &bfe_content_native::BattleContent,
    target: StreamTarget,
) -> Vec<PathBuf> {
    let Ok(level) = resolve_level_name(content) else { return Vec::new() };
    let Ok(sp) = SoundPrepared::new(content) else { return Vec::new() };
    let Ok(raw_manifest) = content.textures.read(&format!("audio/{level}/sound.json")) else {
        return Vec::new();
    };
    let Ok(manifest_val) = bfe_content::json::parse(std::str::from_utf8(&raw_manifest).unwrap_or("")) else {
        return Vec::new();
    };
    let Ok(raw_enh) = content.textures.read(&format!("audio/enhanced/{level}/manifest.json")) else {
        return Vec::new();
    };
    let Ok(enh): Result<serde_json::Value, _> = serde_json::from_slice(&raw_enh) else {
        return Vec::new();
    };
    let mut stream_names: Vec<String> = manifest_val
        .get("streams")
        .and_then(Value::as_array)
        .unwrap_or(&[])
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    if let Some(appended) = manifest_val
        .get("mission")
        .and_then(|m| m.get("appendedStreams"))
        .and_then(Value::as_array)
    {
        for pair in appended {
            if let Some(name) = pair.at(1).and_then(Value::as_str) {
                if !stream_names.iter().any(|s| s == name) {
                    stream_names.push(name.to_string());
                }
            }
        }
    }
    // Keep only stream indices that opened cleanly (matching `streams.definitions` order).
    let opened_names: Vec<String> = stream_names
        .into_iter()
        .filter(|name| content.textures.read(&format!("audio/streams/{name}.json")).is_ok())
        .collect();
    let mut stream_indices = BTreeSet::new();
    match target {
        StreamTarget::Bed => {
            for row in &sp.runtime.phase.director.regions {
                let sid = row[region::SOUND] as usize;
                if let Some(snd) = sp.runtime.phase.engine.sounds.get(sid) {
                    if snd.stream != NONE {
                        stream_indices.insert(snd.stream);
                    }
                }
            }
        }
        StreamTarget::Music => {
            for tr in &sp.runtime.phase.director.tracks {
                if let Some(snd) = sp.runtime.phase.engine.sounds.get(tr.sound) {
                    if snd.stream != NONE {
                        stream_indices.insert(snd.stream);
                    }
                }
            }
        }
    }
    let mut paths = BTreeSet::new();
    for idx in stream_indices {
        let Some(sname) = opened_names.get(idx) else { continue };
        let Some(segs) = enh["streams"][sname].as_object() else { continue };
        for entry in segs.values() {
            if entry["fallback"].as_bool().unwrap_or(true) {
                continue;
            }
            for rel in entry["files"].as_object().into_iter().flatten().filter_map(|(_, v)| v.as_str()) {
                let p = root.join(format!("audio/enhanced/{level}/{rel}"));
                if p.exists() {
                    paths.insert(p);
                }
            }
        }
    }
    paths.into_iter().collect()
}

struct DamagedSpec<'a> {
    label: &'a str,
    src_root: &'a Path,
    family: GameFamily,
    map: &'a str,
    query: &'a str,
    target: StreamTarget,
    damage: DamageKind,
    seconds: usize,
}

struct DamagedOutcome {
    panicked: bool,
    failed: usize,
    var_loads: usize,
    orig_loads: usize,
    peak: f32,
    silent_s: usize,
    total_s: usize,
}

fn run_damaged_scenario(spec: &DamagedSpec<'_>) -> Result<DamagedOutcome, String> {
    let DamagedSpec { label, src_root, family, map, query, target, damage, seconds } = *spec;
    let unique = format!(
        "bfe_dmg_{label}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let dst = std::env::temp_dir().join(unique);
    clone_tree_hardlink(src_root, &dst).map_err(|e| e.to_string())?;
    let probe = Loader::detect(&dst).load_for_family(family, map, query)?;
    let files = target_enhanced_stream_files(&dst, &probe, target);
    assert!(!files.is_empty(), "{label}: expected at least one Enhanced stream file to damage");
    for path in &files {
        match damage {
            DamageKind::Missing => {
                std::fs::remove_file(path).map_err(|e| format!("remove {}: {e}", path.display()))?;
            }
            DamageKind::CutHalf => {
                let data = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
                // Unlink hard link before writing so the source tree is never modified.
                std::fs::remove_file(path).map_err(|e| format!("unlink {}: {e}", path.display()))?;
                std::fs::write(path, &data[..data.len() / 2])
                    .map_err(|e| format!("truncate {}: {e}", path.display()))?;
            }
        }
    }

    let dst_run = dst.clone();
    let map_s = map.to_string();
    let query_s = query.to_string();
    let outcome = std::panic::catch_unwind(move || {
        let content = Loader::detect(&dst_run).load_for_family(family, &map_s, &query_s).unwrap();
        let mut r = Renderer::new_with_mode(&content, true, true, "normal").unwrap();
        let ear = r
            .sp
            .runtime
            .phase
            .director
            .regions
            .iter()
            .find(|reg| reg[region::SOUND] >= 0.0)
            .map(|reg| [reg[region::X], reg[region::Y], reg[region::Z]])
            .unwrap_or([0.0, 1.7, 0.0]);
        let (m_vol, a_vol) = match target {
            StreamTarget::Bed => (0.0, 1.0),
            StreamTarget::Music => (1.0, 0.0),
        };
        r.init_frame(ear, m_vol, a_vol, 0.0, 0.0);
        r.sp.runtime.phase.frame[frame::TICKETS1] = 150.0;
        r.sp.runtime.phase.frame[frame::TICKETS2] = 150.0;
        for row in &mut r.sp.music {
            row[music_slot::START] = 150.0;
            r.sp.runtime.input[..row.len()].copy_from_slice(row);
            r.sp.runtime.import(import::MUSIC_SLOT, row.len());
        }
        r.sp.runtime.phase.set_starting_tickets(150.0, 150.0);
        let mut silent_s = 0usize;
        for _ in 0..seconds {
            let start_len = r.pcm.len();
            for _ in 0..60 {
                r.step_tick();
            }
            let slice = &r.pcm[start_len..];
            let sq: f64 = slice.iter().map(|&v| (v as f64) * (v as f64)).sum();
            let db = 10.0 * (sq / slice.len().max(1) as f64 + 1e-20).log10();
            if db < -80.0 {
                silent_s += 1;
            }
        }
        let (peak, _, _) = r.summary();
        (r.failed_stream_loads, r.upscaled_stream_loads, r.original_stream_loads, peak, silent_s, seconds)
    });
    let _ = std::fs::remove_dir_all(&dst);
    match outcome {
        Ok((failed, var_loads, orig_loads, peak, silent_s, total_s)) => Ok(DamagedOutcome {
            panicked: false,
            failed,
            var_loads,
            orig_loads,
            peak,
            silent_s,
            total_s,
        }),
        Err(_) => Ok(DamagedOutcome {
            panicked: true,
            failed: 0,
            var_loads: 0,
            orig_loads: 0,
            peak: 0.0,
            silent_s: seconds,
            total_s: seconds,
        }),
    }
}

#[test]
#[ignore = "requires prepared 2004 (BFE_CONTENT_DIR) and/or 2005 (BFE_SWBF2_CONTENT_DIR) Enhanced trees"]
fn damaged_enhanced_streams_fall_back_without_crash_or_silence() {
    let report_only = std::env::var("BFE_DAMAGED_REPORT_ONLY").is_ok_and(|v| v == "1");
    let mut ran = 0usize;

    let mut check = |spec: DamagedSpec<'_>| {
        let label = spec.label;
        let DamagedOutcome { panicked, failed, var_loads, orig_loads, peak, silent_s, total_s } =
            run_damaged_scenario(&spec).unwrap();
        println!(
            "damaged_case {label} panicked={panicked} failed={failed} var_loads={var_loads} orig_loads={orig_loads} peak={peak:.5} silent_s={silent_s}/{total_s}"
        );
        ran += 1;
        if !report_only {
            assert!(!panicked, "{label}: damaged Enhanced stream panicked");
            assert_eq!(failed, 0, "{label}: damaged Enhanced stream must fall back to original without failing");
            assert!(peak > 0.005, "{label}: fallback audio must be audible (peak={peak})");
            assert!(
                silent_s <= 2,
                "{label}: fallback audio must play continuously (silent_s={silent_s}/{total_s})"
            );
        }
    };

    if let Ok(root_2004) = std::env::var("BFE_CONTENT_DIR") {
        let root_2004 = PathBuf::from(root_2004);
        let map = "nab2";
        let args: Vec<String> = ["--local", map, root_2004.to_str().unwrap(), "--side", "rep", "--seed", "1", "--ai", "classic"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let options = bfe_client::local::options::Options::parse(&args, None).unwrap().unwrap();
        let prep = bfe_client::local::launch::Prepared::new(&options).unwrap();
        let q = prep.admission.query.clone();
        for (label, target, damage, seconds) in [
            ("2004_bed_missing", StreamTarget::Bed, DamageKind::Missing, 15),
            ("2004_bed_cut", StreamTarget::Bed, DamageKind::CutHalf, 15),
            ("2004_music_missing", StreamTarget::Music, DamageKind::Missing, 20),
            ("2004_music_cut", StreamTarget::Music, DamageKind::CutHalf, 20),
        ] {
            check(DamagedSpec { label, src_root: &root_2004, family: GameFamily::Swbf1, map, query: &q, target, damage, seconds });
        }
    }

    if let (Ok(root_2005), Ok(spec_a), Ok(spec_b)) = (
        std::env::var("BFE_SWBF2_CONTENT_DIR"),
        std::env::var("BFE_SWBF2_MISSION_A"),
        std::env::var("BFE_SWBF2_MISSION_B"),
    ) {
        let root_2005 = PathBuf::from(root_2005);
        let (map_a, query_a) = spec_a.split_once('|').unwrap();
        let (map_b, query_b) = spec_b.split_once('|').unwrap();
        for (label, map, query, target, damage, seconds) in [
            ("2005_a_bed_missing", map_a, query_a, StreamTarget::Bed, DamageKind::Missing, 15),
            ("2005_a_bed_cut", map_a, query_a, StreamTarget::Bed, DamageKind::CutHalf, 15),
            ("2005_a_music_missing", map_a, query_a, StreamTarget::Music, DamageKind::Missing, 20),
            ("2005_a_music_cut", map_a, query_a, StreamTarget::Music, DamageKind::CutHalf, 20),
            ("2005_b_bed_missing", map_b, query_b, StreamTarget::Bed, DamageKind::Missing, 15),
            ("2005_b_bed_cut", map_b, query_b, StreamTarget::Bed, DamageKind::CutHalf, 15),
        ] {
            check(DamagedSpec { label, src_root: &root_2005, family: GameFamily::Swbf2, map, query, target, damage, seconds });
        }
    }

    assert!(ran > 0, "set BFE_CONTENT_DIR and/or BFE_SWBF2_CONTENT_DIR to run damaged stream checks");
}
