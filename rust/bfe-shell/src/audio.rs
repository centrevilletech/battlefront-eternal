//! Native shell audio engine (`ShellAudio` in `src/shell/audio.js` and
//! `LoadingSound` in `src/shell/loading-audio.js`).
//!
//! Decodes prepared `.opus` music tracks, `.flac` transition whooshes, and
//! `core.lvl` interface/loading sounds (`audio/shell/sound.json` + `bank.bin`,
//! falling back to the first battle sub-level in `audio/index.json` when
//! `audio/shell` is absent) through `audio_decode::Decoder` and mixes them on
//! `audio::frontend::Frontend` (`fx` bus 0 at `0.3 * interface_volume`, `music`
//! bus 1 at `0.6 * music_volume`).

use crate::{
    data::{ShellData, ShellSoundDef},
    loading::{audio_constants, LoadingCue, PlannedSound},
    media::MediaEvent,
    music::{buses, MusicStep, TrackHandle},
};
use audio::{
    commands::{
        FRONT_DIP, FRONT_END_GENERATION, FRONT_END_SLOT, FRONT_END_STRIDE, FRONT_FADE,
        FRONT_START, FRONT_STOP, FRONT_VOLUME,
    },
    frontend::{self, Frontend},
    FRAMES,
};
use audio_decode::{Decoder, Step};
use jsrt::hash::fnv1a_engine;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

/// Output sample rate (`48_000 Hz`) for the native shell mixer.
pub const TARGET_RATE: u32 = 48_000;

/// Interface sounds preloaded by the shell (`core.lvl`, on `shellfx`).
/// Mirrors `INTERFACE_SOUNDS` in `src/shell/audio.js`.
pub const INTERFACE_SOUNDS: [&str; 8] = [
    "shell_select_change",
    "shell_menu_accept",
    "shell_menu_enter",
    "shell_menu_cancel",
    "shell_menu_error",
    "shell_menu_exit",
    "shell_keyboard_key",
    "shell_keyboard_backspace",
];

/// Decoded mono PCM buffer and playback metadata for an interface, whoosh, or
/// loading sound.
#[derive(Clone, Debug)]
pub struct DecodedSound {
    pub pcm: Vec<f32>,
    pub rate: f64,
    pub gain: f32,
    pub pitch: f32,
    pub interval: f64,
    pub looping: bool,
    pub last_played: f64,
}

impl DecodedSound {
    /// Duration of the sample in seconds (`pcm.len() / rate`).
    pub fn duration(&self) -> f64 {
        if self.rate > 0.0 {
            (self.pcm.len() as f64) / self.rate
        } else {
            0.0
        }
    }
}

/// Resolves a sound name or `"0x1234abcd"` hash key to the `0x`-prefixed 8-digit
/// lowercase hex FNV-1a engine hash (`src/audio/hash.js` `keyOf`).
pub fn key_of(name_or_key: &str) -> Option<String> {
    if name_or_key.is_empty() {
        return None;
    }
    if let Some(hex) = name_or_key.strip_prefix("0x") {
        if hex.len() == 8
            && hex
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Some(name_or_key.to_string());
        }
    }
    Some(format!("0x{:08x}", fnv1a_engine(name_or_key)))
}

/// Normalizes signed 16-bit PCM samples into `[-1.0, 1.0)` `f32` samples
/// (`sample / 32768.0`), matching `pcm16ToFloat32` in `src/audio/pcm.js`.
pub fn normalize_pcm16(src: &[i16], dst: &mut [f32]) {
    for (d, &s) in dst.iter_mut().zip(src.iter()) {
        *d = (s as f32) * (1.0 / 32768.0);
    }
}

/// Core sound bank reader (`sound.json` + `bank.bin`), resolving `SoundProperties`
/// inheritance (`Inherit`) and extracting 16-bit PCM byte ranges on demand.
///
/// Ports `SoundData` (`src/audio/data.js`) and `coreSounds` (`src/shell/audio.js`).
pub struct CoreSounds {
    bank_path: PathBuf,
    properties: BTreeMap<String, Value>,
    samples: BTreeMap<String, (u64, usize, u32)>,
}

impl CoreSounds {
    /// Opens `audio/shell/sound.json` (or falls back to the first battle in
    /// `audio/index.json`) under `content_dir`.
    ///
    /// Mirrors `coreSounds()` in `src/shell/audio.js`.
    pub fn load_from_dir(content_dir: &Path) -> Option<Self> {
        let mut sublevel = "shell".to_string();
        let mut manifest_bytes =
            std::fs::read(content_dir.join(format!("audio/{sublevel}/sound.json"))).ok();
        if manifest_bytes.is_none() {
            let index_bytes = std::fs::read(content_dir.join("audio/index.json")).ok()?;
            let index: Value = serde_json::from_slice(&index_bytes).ok()?;
            sublevel = index
                .get("maps")
                .or_else(|| index.get("missions"))
                .and_then(Value::as_object)
                .and_then(|m| m.values().next())
                .and_then(Value::as_str)?
                .to_string();
            manifest_bytes =
                std::fs::read(content_dir.join(format!("audio/{sublevel}/sound.json"))).ok();
        }
        let manifest: Value = serde_json::from_slice(&manifest_bytes?).ok()?;
        let bank_path = content_dir.join(format!("audio/{sublevel}/bank.bin"));
        Some(Self::from_manifest(&manifest, bank_path))
    }

    /// Parses a `sound.json` manifest and resolves `SoundProperties` inheritance
    /// in declaration order (`inherit` in `src/audio/data.js`).
    pub fn from_manifest(manifest: &Value, bank_path: PathBuf) -> Self {
        let mut properties: BTreeMap<String, Value> = BTreeMap::new();
        let mut own_table: BTreeMap<String, Value> = BTreeMap::new();

        if let Some(objects) = manifest.get("objects").and_then(Value::as_array) {
            for obj in objects {
                let Some(kind) = obj.get("type").and_then(Value::as_str) else {
                    continue;
                };
                if kind != "SoundProperties" {
                    continue;
                }
                let resolved = inherit_sound_properties(obj, &properties, &own_table);
                if let Some(name_val) = obj.get("Name") {
                    if let Some(s) = name_val.as_str() {
                        properties.insert(s.to_string(), resolved.clone());
                        own_table.insert(s.to_string(), obj.clone());
                    } else if let Some(arr) = name_val.as_array() {
                        for item in arr {
                            if let Some(s) = item.as_str() {
                                properties.insert(s.to_string(), resolved.clone());
                                own_table.insert(s.to_string(), obj.clone());
                            }
                        }
                    }
                }
            }
        }

        let mut samples = BTreeMap::new();
        if let Some(s_obj) = manifest.get("samples").and_then(Value::as_object) {
            for (id, arr_val) in s_obj {
                if let Some(arr) = arr_val.as_array().filter(|a| a.len() >= 3) {
                    let offset = arr[0].as_u64().unwrap_or(0);
                    let frames = arr[1].as_u64().unwrap_or(0) as usize;
                    let rate = arr[2].as_u64().unwrap_or(22050) as u32;
                    samples.insert(id.clone(), (offset, frames, rate));
                }
            }
        }

        Self {
            bank_path,
            properties,
            samples,
        }
    }

    /// Loads a named sound (`"shell_select_change"`, `"load_ytracking"`, etc.)
    /// from `bank.bin` by byte range, normalizing 16-bit PCM to `f32` and
    /// applying the 8kHz minimum rate pitch adjustment (`rate / max(rate, 8000)`).
    ///
    /// Mirrors `coreSounds().load(name, ctx, into)` in `src/shell/audio.js`.
    pub fn load_sound(&self, name: &str) -> Option<DecodedSound> {
        let key = key_of(name)?;
        let sound = self.properties.get(&key)?;
        let sample_id = sound
            .get("SampleList")?
            .as_array()?
            .first()?
            .as_array()?
            .first()?
            .as_str()?;
        let &(offset, frames, rate) = self.samples.get(sample_id)?;
        if frames == 0 {
            return None;
        }
        let mut file = File::open(&self.bank_path).ok()?;
        file.seek(SeekFrom::Start(offset)).ok()?;
        let mut bytes = vec![0u8; frames * 2];
        file.read_exact(&mut bytes).ok()?;

        let pcm_i16: Vec<i16> = bytes
            .as_chunks::<2>().0.iter()
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        let mut pcm = vec![0.0f32; pcm_i16.len().max(1)];
        normalize_pcm16(&pcm_i16, &mut pcm);

        let gain = sound.get("Gain").and_then(Value::as_f64).unwrap_or(1.0) as f32;
        let base_pitch = sound.get("Pitch").and_then(Value::as_f64).unwrap_or(1.0) as f32;
        let interval = sound
            .get("PlayInterval")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let looping = sound.get("Looping").and_then(Value::as_f64).unwrap_or(0.0) != 0.0;
        let effective_rate = rate.max(8000) as f64;
        let pitch = base_pitch * ((rate as f32) / (effective_rate as f32));

        Some(DecodedSound {
            pcm,
            rate: effective_rate,
            gain,
            pitch,
            interval,
            looping,
            last_played: f64::NEG_INFINITY,
        })
    }
}

fn inherit_sound_properties(
    object: &Value,
    table: &BTreeMap<String, Value>,
    own_table: &BTreeMap<String, Value>,
) -> Value {
    let Some(obj_map) = object.as_object() else {
        return object.clone();
    };
    let Some(parent_key) = obj_map.get("Inherit").and_then(Value::as_str) else {
        return object.clone();
    };
    let Some(parent) = table.get(parent_key).and_then(Value::as_object) else {
        return object.clone();
    };
    let parent_own = own_table
        .get(parent_key)
        .and_then(Value::as_object)
        .unwrap_or(parent);

    let mut out = serde_json::Map::new();
    for (k, v) in obj_map {
        if k == "Inherit" {
            for (pk, pv) in parent {
                if !matches!(
                    pk.as_str(),
                    "Name" | "Group" | "type" | "SampleList" | "SegmentList" | "Inherit"
                ) {
                    out.insert(pk.clone(), pv.clone());
                }
            }
            if let Some(sl) = parent_own.get("SampleList") {
                out.insert("SampleList".to_string(), sl.clone());
            } else {
                out.remove("SampleList");
            }
            if let Some(seg) = parent_own.get("SegmentList") {
                out.insert("SegmentList".to_string(), seg.clone());
            }
            continue;
        }
        out.insert(k.clone(), v.clone());
    }
    if let Some(t) = obj_map.get("type") {
        out.insert("type".to_string(), t.clone());
    }
    if let Some(n) = obj_map.get("Name") {
        out.insert("Name".to_string(), n.clone());
    }
    if let Some(g) = obj_map.get("Group") {
        out.insert("Group".to_string(), g.clone());
    } else {
        out.remove("Group");
    }
    Value::Object(out)
}

/// Decodes an audio file (`.opus`, `.flac`, `.wav`) from disk into 48kHz
/// mono `f32` PCM using `audio_decode::Decoder`.
pub fn decode_audio_file(path: &Path) -> Option<Vec<f32>> {
    let bytes = std::fs::read(path).ok()?;
    let mut decoder = Decoder::new(TARGET_RATE, 0.0, false);
    decoder.push(&bytes, true).ok()?;
    let mut out = Vec::new();
    let mut channels = 1usize;
    loop {
        match decoder.step(4096).ok()? {
            Step::Metadata => {
                if let Some(info) = &decoder.info {
                    channels = info.channels.max(1);
                }
            }
            Step::Output(frames) => {
                // `decoder.output` is channel-major (`channels * frames`); take
                // channel 0 (or average stereo into mono).
                if channels == 1 {
                    out.extend_from_slice(&decoder.output[..frames]);
                } else {
                    for i in 0..frames {
                        let l = decoder.output[i];
                        let r = decoder.output[frames + i];
                        out.push(0.5 * (l + r));
                    }
                }
            }
            Step::Done | Step::NeedInput => break,
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Active voice slot allocation in [`ShellAudioMixer`].
#[derive(Clone, Copy, Debug)]
struct ActiveSlot {
    generation: u32,
    music_handle: Option<TrackHandle>,
}

/// Native shell audio mixer driving `audio::frontend::Frontend` at `48 kHz`
/// stereo (`128`-frame quantums).
///
/// Ports `ShellAudio` (`src/shell/audio.js`) and `FrontendAudio` (`src/audio/frontend.js`).
pub struct ShellAudioMixer {
    frontend: Frontend,
    slots: [Option<ActiveSlot>; frontend::SLOTS],
    next_generation: u32,
    current_frame: u64,
    content_dir: Option<PathBuf>,
    core: Option<CoreSounds>,
    whooshes: Vec<(String, Option<Vec<f32>>)>,
    whoosh_index: usize,
    sounds: BTreeMap<String, Option<DecodedSound>>,
    sound_defs: BTreeMap<String, ShellSoundDef>,
    music_cache: BTreeMap<String, Vec<f32>>,
    active_music_slot: Option<(usize, u32, TrackHandle)>,
    ended_music_handles: Vec<TrackHandle>,
    pub master_volume: f32,
    pub music_volume: f32,
    pub interface_volume: f32,
}

impl ShellAudioMixer {
    /// Creates a new `ShellAudioMixer` for `data` and optional `content_dir`.
    pub fn new(data: &ShellData, content_dir: Option<&Path>) -> Self {
        let mut frontend = Frontend::new(TARGET_RATE as f64);
        let master = 1.0f64;
        let music = buses::SHELL_MUSIC * 1.0;
        let fx = buses::SHELL_FX * 1.0;
        frontend.commands.extend_from_slice(&[
            FRONT_VOLUME as f64,
            master,
            music,
            fx,
        ]);
        frontend.apply();
        frontend.commands.clear();

        let core = content_dir.and_then(CoreSounds::load_from_dir);
        let whooshes = data
            .whooshes
            .iter()
            .map(|url| (url.clone(), None))
            .collect();

        let mut mixer = Self {
            frontend,
            slots: [None; frontend::SLOTS],
            next_generation: 1,
            current_frame: 0,
            content_dir: content_dir.map(Path::to_path_buf),
            core,
            whooshes,
            whoosh_index: 0,
            sounds: BTreeMap::new(),
            sound_defs: data.sounds.clone(),
            music_cache: BTreeMap::new(),
            active_music_slot: None,
            ended_music_handles: Vec::with_capacity(16),
            master_volume: 1.0,
            music_volume: 1.0,
            interface_volume: 1.0,
        };

        for name in INTERFACE_SOUNDS {
            mixer.preload_sound(name);
        }
        mixer
    }

    fn flush_commands(&mut self) {
        if !self.frontend.commands.is_empty() {
            self.frontend.apply();
            self.frontend.commands.clear();
        }
    }

    /// Updates master, music, and interface volume settings (`0.0..=1.0`).
    /// Mirrors `ShellAudio.applyVolumes()` in `src/shell/audio.js`.
    pub fn set_volumes(&mut self, master: f32, music: f32, interface: f32) {
        self.master_volume = master;
        self.music_volume = music;
        self.interface_volume = interface;
        self.frontend.commands.extend_from_slice(&[
            FRONT_VOLUME as f64,
            master as f64,
            buses::SHELL_MUSIC * (music as f64),
            buses::SHELL_FX * (interface as f64),
        ]);
        self.flush_commands();
    }

    /// Ramps the `shellmusic` bus level to `level` over `seconds`.
    /// Mirrors `ShellAudio.fadeBus(bus, level, seconds)` in `src/shell/audio.js`.
    pub fn fade_bus(&mut self, bus: &str, level: f64, seconds: f64) {
        if bus != "shellmusic" {
            return;
        }
        let now = self.current_time();
        self.frontend.commands.extend_from_slice(&[
            FRONT_DIP as f64,
            level,
            seconds.max(0.0),
            now,
        ]);
        self.flush_commands();
    }

    fn resolve_content_path(&self, url: &str) -> Option<PathBuf> {
        let dir = self.content_dir.as_ref()?;
        Some(dir.join(url.trim_start_matches('/')))
    }

    fn alloc_slot(&mut self, music_handle: Option<TrackHandle>) -> (usize, u32) {
        let slot = self
            .slots
            .iter()
            .position(Option::is_none)
            .unwrap_or(0);
        let gen = self.next_generation;
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        self.slots[slot] = Some(ActiveSlot {
            generation: gen,
            music_handle,
        });
        (slot, gen)
    }

    #[allow(clippy::too_many_arguments)]
    fn start_pcm(
        &mut self,
        pcm: &[f32],
        rate: f64,
        bus: u32,
        looping: bool,
        gain: f32,
        pitch: f32,
        when: f64,
        offset: f64,
        music_handle: Option<TrackHandle>,
    ) -> (usize, u32) {
        let (slot, gen) = self.alloc_slot(music_handle);
        let ptr = self.frontend.sample(slot, pcm.len(), 1, rate);
        // SAFETY: `sample()` just allocated `pcm.len()` floats in `self.frontend.samples[slot]`.
        unsafe {
            std::ptr::copy_nonoverlapping(pcm.as_ptr(), ptr, pcm.len());
        }
        let now = self.current_time();
        // `FRONT_START` parameters: `slot generation when offset pitch loop gain bus`.
        self.frontend.commands.extend_from_slice(&[
            FRONT_START as f64,
            slot as f64,
            gen as f64,
            when.max(now),
            offset.max(0.0),
            pitch as f64,
            if looping { 1.0 } else { 0.0 },
            gain as f64,
            bus as f64,
        ]);
        self.flush_commands();
        (slot, gen)
    }

    /// Preloads a named sound into `self.sounds`.
    pub fn preload_sound(&mut self, name: &str) {
        if self.sounds.contains_key(name) {
            return;
        }
        let loaded = if let Some(own) = self.sound_defs.get(name) {
            self.resolve_content_path(&own.url)
                .and_then(|p| decode_audio_file(&p))
                .map(|pcm| DecodedSound {
                    pcm,
                    rate: TARGET_RATE as f64,
                    gain: own.gain as f32,
                    pitch: own.pitch as f32,
                    interval: 0.0,
                    looping: false,
                    last_played: f64::NEG_INFINITY,
                })
        } else {
            self.core.as_ref().and_then(|c| c.load_sound(name))
        };
        self.sounds.insert(name.to_string(), loaded);
    }

    /// Plays a menu or interface sound by name (at most once per `PlayInterval`).
    /// Mirrors `ShellAudio.play(name)` in `src/shell/audio.js`.
    pub fn play(&mut self, name: &str) -> Option<(usize, u32)> {
        if name.is_empty() {
            return None;
        }
        self.preload_sound(name);
        let now = self.current_time();
        let sound = self.sounds.get_mut(name)?.as_mut()?;
        if now - sound.last_played < sound.interval {
            return None;
        }
        sound.last_played = now;
        let pcm = sound.pcm.clone();
        let rate = sound.rate;
        let gain = sound.gain;
        let pitch = sound.pitch;
        let looping = sound.looping;
        Some(self.start_pcm(&pcm, rate, 0, looping, gain, pitch, now, 0.0, None))
    }

    /// Plays the next transition whoosh sound in turn (`shell_transition1..10`).
    /// Mirrors `ShellAudio.whoosh()` in `src/shell/audio.js`.
    pub fn whoosh(&mut self) -> Option<(usize, u32)> {
        if self.whooshes.is_empty() {
            return None;
        }
        let idx = self.whoosh_index;
        self.whoosh_index = (self.whoosh_index + 1) % self.whooshes.len();
        if self.whooshes[idx].1.is_none() {
            let url = self.whooshes[idx].0.clone();
            if let Some(path) = self.resolve_content_path(&url) {
                self.whooshes[idx].1 = decode_audio_file(&path);
            }
        }
        let pcm = self.whooshes[idx].1.as_ref()?.clone();
        let now = self.current_time();
        Some(self.start_pcm(
            &pcm,
            TARGET_RATE as f64,
            0,
            false,
            1.0,
            1.0,
            now,
            0.0,
            None,
        ))
    }

    /// Applies a [`MusicStep`] (stopping any previous track and starting any
    /// newly selected track on `shellmusic` bus 1).
    pub fn apply_music_step(&mut self, step: &MusicStep, data: &ShellData) {
        if let Some((stopped_handle, _)) = step.stopped {
            if let Some((slot, _gen, active_handle)) = self.active_music_slot {
                if active_handle == stopped_handle {
                    let now = self.current_time();
                    self.frontend
                        .commands
                        .extend_from_slice(&[FRONT_STOP as f64, slot as f64, now]);
                    self.flush_commands();
                    self.slots[slot] = None;
                    self.active_music_slot = None;
                }
            }
        }
        if let Some((handle, track_idx)) = step.started {
            if let Some(track) = data.music.get(track_idx) {
                if !self.music_cache.contains_key(&track.url) {
                    if let Some(path) = self.resolve_content_path(&track.url) {
                        if let Some(pcm) = decode_audio_file(&path) {
                            self.music_cache.insert(track.url.clone(), pcm);
                        }
                    }
                }
                if let Some(pcm) = self.music_cache.get(&track.url).cloned() {
                    let now = self.current_time();
                    let (slot, gen) = self.start_pcm(
                        &pcm,
                        TARGET_RATE as f64,
                        1,
                        false,
                        1.0,
                        1.0,
                        now,
                        0.0,
                        Some(handle),
                    );
                    self.active_music_slot = Some((slot, gen, handle));
                }
            }
        }
    }

    /// Applies a slice of [`MediaEvent`]s emitted by [`crate::media::ShellMedia`].
    pub fn apply_media_events(&mut self, events: &[MediaEvent], data: &ShellData) {
        for ev in events {
            match ev {
                MediaEvent::FadeBus {
                    bus,
                    level,
                    seconds,
                } => self.fade_bus(bus, *level, *seconds),
                MediaEvent::Whoosh => {
                    self.whoosh();
                }
                MediaEvent::Music(step) => self.apply_music_step(step, data),
                MediaEvent::PlayMovie { .. } | MediaEvent::PreviewMovie { .. } => {}
            }
        }
    }

    /// Schedules a loading screen sound plan (`LoadingTimeline::sounds()`) onto
    /// the `Frontend` DSP clock starting at `start_sec` on the mixer clock.
    ///
    /// Mirrors `LoadingSound.schedule` and `LoadingSound.apply` in
    /// `src/shell/loading-audio.js`.
    pub fn schedule_loading_plan(&mut self, plan: &[PlannedSound], start_sec: f64) -> usize {
        for cue in [
            LoadingCue::YTrack,
            LoadingCue::XTrack,
            LoadingCue::Zoom,
            LoadingCue::Transition,
        ] {
            self.preload_sound(audio_constants::sound_name(cue));
        }
        let now = self.current_time();
        let mut scheduled = 0;
        for ev in plan {
            let name = audio_constants::sound_name(ev.cue);
            let Some(Some(sound)) = self.sounds.get(name).cloned() else {
                continue;
            };
            let at = start_sec + ev.at + audio_constants::AV;
            let until = ev.until.map(|u| start_sec + u + audio_constants::AV);
            if let Some(end) = until {
                if end <= now {
                    continue;
                }
                let begin = at.max(now);
                let dur = sound.duration().max(1e-3);
                let offset = ((begin - at) * (sound.pitch as f64)) % dur;
                let (slot, _gen) = self.start_pcm(
                    &sound.pcm,
                    sound.rate,
                    0,
                    true,
                    sound.gain,
                    sound.pitch,
                    begin,
                    offset,
                    None,
                );
                let fade_start = (end - audio_constants::STOP).max(begin);
                // `FRONT_FADE` parameters: `slot start end`.
                self.frontend.commands.extend_from_slice(&[
                    FRONT_FADE as f64,
                    slot as f64,
                    fade_start,
                    end,
                ]);
                self.flush_commands();
                scheduled += 1;
            } else {
                if now - at > audio_constants::LATE {
                    continue;
                }
                let begin = at.max(now);
                self.start_pcm(
                    &sound.pcm,
                    sound.rate,
                    0,
                    false,
                    sound.gain,
                    sound.pitch,
                    begin,
                    0.0,
                    None,
                );
                scheduled += 1;
            }
        }
        scheduled
    }

    /// Renders `frames` stereo samples (`frames * 2` `f32` values) in 128-frame
    /// quantums without allocating on the audio thread. Any music track handles
    /// that finished during these quantums can be drained with
    /// [`Self::take_ended_music`].
    pub fn render_interleaved(&mut self, out_stereo: &mut [f32]) {
        let total_frames = out_stereo.len() / 2;
        let mut offset = 0;

        while offset < total_frames {
            let chunk = (total_frames - offset).min(FRAMES);
            let ended_count = self.frontend.process(self.current_frame);
            self.current_frame += FRAMES as u64;

            for i in 0..chunk {
                let dst = (offset + i) * 2;
                out_stereo[dst] = self.frontend.output[i];
                out_stereo[dst + 1] = self.frontend.output[FRAMES + i];
            }

            if ended_count > 0 {
                for k in 0..ended_count {
                    let slot = self.frontend.ended[k * FRONT_END_STRIDE + FRONT_END_SLOT] as usize;
                    let gen = self.frontend.ended[k * FRONT_END_STRIDE + FRONT_END_GENERATION];
                    if let Some(active) = self.slots.get(slot).and_then(|s| *s) {
                        if active.generation == gen {
                            self.slots[slot] = None;
                            if let Some(h) = active.music_handle {
                                if self.active_music_slot.map(|(_, _, ah)| ah) == Some(h) {
                                    self.active_music_slot = None;
                                }
                                if self.ended_music_handles.len()
                                    < self.ended_music_handles.capacity()
                                {
                                    self.ended_music_handles.push(h);
                                }
                            }
                        }
                    }
                }
            }

            offset += chunk;
        }
    }

    /// Drains any [`TrackHandle`]s that finished playing during recent
    /// [`Self::render_interleaved`] calls so the caller can pass them to
    /// [`crate::music::ShellMusic::ended`].
    pub fn take_ended_music(&mut self) -> Vec<TrackHandle> {
        let out = self.ended_music_handles.clone();
        self.ended_music_handles.clear();
        out
    }

    pub fn current_time(&self) -> f64 {
        (self.current_frame as f64) / (TARGET_RATE as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::read_shell_data;

    #[test]
    fn key_of_hashes_sound_names_with_engine_fnv1a() {
        assert_eq!(key_of(""), None);
        assert_eq!(key_of("0x586e70e8").as_deref(), Some("0x586e70e8"));
        assert_eq!(key_of("shell_soundtrack").as_deref(), Some("0x586e70e8"));
        assert_eq!(
            key_of("shell_galacticconquest").as_deref(),
            Some("0x48ae1970")
        );
    }

    #[test]
    fn mixer_renders_quantums_and_reports_ended_music_handles() {
        let data = read_shell_data(None, None, None);
        let mut mixer = ShellAudioMixer::new(&data, None);
        // Inject a synthetic 64-sample sound and start it as a music handle.
        let pcm = vec![0.25f32; 64];
        mixer.start_pcm(
            &pcm,
            TARGET_RATE as f64,
            1,
            false,
            1.0,
            1.0,
            0.0,
            0.0,
            Some(77),
        );

        let mut stereo = vec![0.0f32; 128 * 2];
        mixer.render_interleaved(&mut stereo);
        assert!(stereo.iter().any(|&s| s > 0.0));
        assert_eq!(mixer.take_ended_music(), vec![77]);
    }
}
