//! Native shell audio engine (`ShellAudio` in `src/shell/audio.js` and
//! `LoadingSound` in `src/shell/loading-audio.js`).
//!
//! Decodes prepared `.opus` music tracks, `.flac` transition whooshes, and
//! `core.lvl` interface/loading sounds (`audio/shell/sound.json` + `bank.bin`,
//! falling back to the first battle sub-level in `audio/index.json` when
//! `audio/shell` is absent) through `audio_decode::Decoder` and mixes them on
//! `audio::frontend::Frontend` (`fx` bus 0 at `0.3 * interface_volume`, `music`
//! bus 1 at `0.6 * music_volume`).
//!
//! Key rules ported from `src/shell/audio.js` and `src/shell/loading-audio.js`:
//! - `INTERFACE_SOUNDS` preloads the 8 core menu sound effects (`shell_select_change`,
//!   `shell_menu_accept`, `shell_menu_enter`, `shell_menu_cancel`, `shell_menu_error`,
//!   `shell_menu_exit`, `shell_keyboard_key`, `shell_keyboard_backspace`) on `shellfx`.
//! - `CoreSounds` resolves `SoundProperties` inheritance (`Inherit`) in `sound.json`
//!   and reads 16-bit little-endian PCM slices from `bank.bin` by byte range,
//!   clamping playback sample rates below `8000 Hz` to `8000 Hz` while scaling
//!   `pitch *= rate / max(rate, 8000)` to preserve duration and pitch.
//! - `ShellAudioMixer` drives `Frontend` in `128`-frame stereo quantums at `48 kHz`,
//!   Cycling through `shell_transition1..10` on `whoosh()`, ramping `shellmusic`
//!   via `FRONT_DIP`, and scheduling loading screen `PlannedSound` cues with the
//!   `AV = 1 / 30` s frame-to-speaker offset and `LATE = 0.05` s one-shot window.

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
    /// Normalized mono `f32` PCM samples (`[-1.0, 1.0)`).
    pub pcm: Vec<f32>,
    /// Effective sample rate in Hz (`>= 8000.0` for `core.lvl` bank samples, or `48000.0` for decoded files).
    pub rate: f64,
    /// Linear amplitude gain from `SoundProperties.Gain` or `shell.json` (`1.0` default).
    pub gain: f32,
    /// Playback pitch multiplier from `SoundProperties.Pitch` (adjusted by `rate / max(rate, 8000)`).
    pub pitch: f32,
    /// Minimum cooldown in seconds (`SoundProperties.PlayInterval`) between consecutive triggers.
    pub interval: f64,
    /// Whether the sample loops continuously until stopped (`SoundProperties.Looping != 0`).
    pub looping: bool,
    /// Mixer timestamp (`current_time()`) when this sound was last triggered (`-inf` initially).
    pub last_played: f64,
}

impl DecodedSound {
    /// Duration of the sample in seconds (`pcm.len() / rate`).
    /// Duration of the sample in seconds (`pcm.len() / rate`), or `0.0` if
    /// `rate <= 0.0`. Used when computing loop phase offsets for late-joined
    /// loading pulses (`LoadingSound.play` in `src/shell/loading-audio.js`).
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
    /// Filesystem path to the binary sample bank (`audio/<sublevel>/bank.bin`).
    bank_path: PathBuf,
    /// Resolved `SoundProperties` objects keyed by `0x`-prefixed FNV-1a hex hash or name.
    properties: BTreeMap<String, Value>,
    /// Sample byte ranges `(byte_offset, frame_count, sample_rate_hz)` keyed by sample hash.
    samples: BTreeMap<String, (u64, usize, u32)>,
}

impl CoreSounds {
    /// Opens `audio/shell/sound.json` (or falls back to the first battle in
    /// `audio/index.json`) under `content_dir`, returning an error naming the
    /// file or field if reading or parsing fails.
    pub fn try_load_from_dir(content_dir: &Path) -> Result<Self, String> {
        let mut sublevel = "shell".to_string();
        let shell_sound_path = content_dir.join("audio/shell/sound.json");
        // Prefer the dedicated front-end sound sub-level (`audio/shell/sound.json`)
        // written by `tools/zero_shell.py`; if absent, fall back to the first battle
        // sub-level listed in `audio/index.json` (`maps` or `missions`), matching
        // `coreSounds()` in `src/shell/audio.js`.
        let manifest_bytes = match std::fs::read(&shell_sound_path) {
            Ok(bytes) => bytes,
            Err(_) => {
                let index_path = content_dir.join("audio/index.json");
                let index_bytes = std::fs::read(&index_path)
                    .map_err(|e| format!("{}: {e}", index_path.display()))?;
                let index: Value = serde_json::from_slice(&index_bytes)
                    .map_err(|e| format!("{}: {e}", index_path.display()))?;
                // Read the first sub-level name from `index.maps` or `index.missions`
                // (`Object.values(index?.maps || index?.missions || {})[0]` in `src/shell/audio.js`).
                sublevel = index
                    .get("maps")
                    .or_else(|| index.get("missions"))
                    .and_then(Value::as_object)
                    .and_then(|m| m.values().next())
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        format!(
                            "{} (`audio/index.json`): missing or invalid `maps`/`missions` field",
                            index_path.display()
                        )
                    })?
                    .to_string();
                let sub_path = content_dir.join(format!("audio/{sublevel}/sound.json"));
                std::fs::read(&sub_path)
                    .map_err(|e| format!("{}: {e}", sub_path.display()))?
            }
        };
        let manifest_path = content_dir.join(format!("audio/{sublevel}/sound.json"));
        let manifest: Value = serde_json::from_slice(&manifest_bytes)
            .map_err(|e| format!("{}: {e}", manifest_path.display()))?;
        let bank_path = content_dir.join(format!("audio/{sublevel}/bank.bin"));
        Ok(Self::from_manifest(&manifest, bank_path))
    }

    /// Opens `audio/shell/sound.json` (or falls back to the first battle in
    /// `audio/index.json`) under `content_dir`.
    ///
    /// Mirrors `coreSounds()` in `src/shell/audio.js`.
    pub fn load_from_dir(content_dir: &Path) -> Option<Self> {
        let mut sublevel = "shell".to_string();
        let mut manifest_bytes =
            std::fs::read(content_dir.join(format!("audio/{sublevel}/sound.json"))).ok();
        // Fall back to `audio/index.json` when `audio/shell/sound.json` is not present.
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

        // Walk `manifest.objects` in declaration order, resolving `SoundProperties`
        // inheritance (`Inherit`) and indexing each entry under its `Name` (which
        // may be a single key string or an array of alias keys).
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

        // Index `manifest.samples` entries `sample_id -> [byte_offset, frame_count, sample_rate]`
        // so individual `core.lvl` sound effects can be read by byte range from `bank.bin`
        // without loading the entire sound bank into memory.
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
        // Extract the primary sample hash from `sound.SampleList[0][0]` and look up
        // its `[offset, frames, rate]` tuple in `self.samples`.
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
        // Read only the `frames * 2` byte slice for this sample from `bank.bin`
        // (matching the HTTP `Range: bytes=offset-(offset + frames * 2 - 1)` fetch
        // in `coreSounds().load` in `src/shell/audio.js`).
        let mut file = File::open(&self.bank_path).ok()?;
        file.seek(SeekFrom::Start(offset)).ok()?;
        let mut bytes = vec![0u8; frames * 2];
        file.read_exact(&mut bytes).ok()?;

        // Decode 16-bit little-endian signed PCM frames and normalize into `[-1.0, 1.0)`
        // `f32` samples (`sample / 32768.0`), matching `normalizePcm16` in `src/audio/pcm.js`.
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
        // Web Audio `AudioContext.createBuffer` enforces an 8 kHz floor; clamping
        // `effective_rate` to `rate.max(8000)` and scaling `pitch` by `rate / effective_rate`
        // matches `coreSounds().load` in `src/shell/audio.js`.
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

/// Resolves `SoundProperties` inheritance (`Inherit`) in declaration order,
/// copying inherited scalar properties from `table[parent_key]` and only copying
/// `SampleList` / `SegmentList` when declared directly on `own_table[parent_key]`,
/// while preserving the child's own `type`, `Name`, and `Group`.
///
/// Mirrors `inherit(object, table, ownTable)` in `src/audio/data.js`.
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
    // When `Inherit` is encountered, splice in inherited scalar fields from the
    // resolved parent (`table[parent_key]`) while taking `SampleList` and
    // `SegmentList` only from the parent's own declaration (`own_table[parent_key]`).
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
    // Restore the child object's identity fields (`type`, `Name`, and optional
    // `Group`) so parent metadata never overwrites the derived sound's name or group.
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
/// mono `f32` PCM using `audio_decode::Decoder`, returning an error naming
/// the file if reading or decoding fails.
pub fn try_decode_audio_file(path: &Path) -> Result<Vec<f32>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut decoder = Decoder::new(TARGET_RATE, 0.0, false);
    decoder
        .push(&bytes, true)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = Vec::new();
    let mut channels = 1usize;
    loop {
        let step = decoder
            .step(4096)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        match step {
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
        Err(format!("{}: decoded 0 audio frames", path.display()))
    } else {
        Ok(out)
    }
}

/// Decodes an audio file (`.opus`, `.flac`, `.wav`) from disk into 48kHz
/// mono `f32` PCM using `audio_decode::Decoder`, returning `None` if the file
/// cannot be read, fails to decode, or produces zero output frames.
///
/// Mirrors `loadAudioPcm(bytes, { rate: ctx.sampleRate })` in `src/shell/audio.js`.
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
    /// Monotonic generation tag paired with the `Frontend` voice slot to ignore stale endings.
    generation: u32,
    /// Optional [`TrackHandle`] when this slot is playing a `ShellMusic` track on bus 1.
    music_handle: Option<TrackHandle>,
}

/// Native shell audio mixer driving `audio::frontend::Frontend` at `48 kHz`
/// stereo (`128`-frame quantums).
///
/// Ports `ShellAudio` (`src/shell/audio.js`) and `FrontendAudio` (`src/audio/frontend.js`).
pub struct ShellAudioMixer {
    /// Low-level 2-bus (`shellfx` = 0, `shellmusic` = 1) DSP voice mixer from the `audio` crate.
    frontend: Frontend,
    /// Metadata for each of the `frontend::SLOTS` voice slots (`None` when free).
    slots: [Option<ActiveSlot>; frontend::SLOTS],
    /// Next non-zero generation counter assigned by [`Self::alloc_slot`].
    next_generation: u32,
    /// Total output sample frames rendered so far (`current_time() = current_frame / 48000`).
    current_frame: u64,
    /// Root directory of extracted game content on disk (`None` in unit tests without files).
    content_dir: Option<PathBuf>,
    /// Reader for `core.lvl` sound properties and `bank.bin` PCM slices.
    core: Option<CoreSounds>,
    /// Transition whoosh URLs (`shell_transition1..10`) paired with lazily decoded mono PCM.
    whooshes: Vec<(String, Option<Vec<f32>>)>,
    /// Round-robin index into `whooshes` advanced on each [`Self::whoosh`] call.
    whoosh_index: usize,
    /// Cache of preloaded interface and loading sounds keyed by sound name.
    sounds: BTreeMap<String, Option<DecodedSound>>,
    /// Sound overrides from `shell.json`'s `"sounds"` table (`url`, `gain`, `pitch`).
    sound_defs: BTreeMap<String, ShellSoundDef>,
    /// Decoded mono `f32` PCM cache for `.opus` shell music tracks keyed by URL.
    music_cache: BTreeMap<String, Vec<f32>>,
    /// Currently playing music `(slot_index, generation, track_handle)` on bus 1, if any.
    active_music_slot: Option<(usize, u32, TrackHandle)>,
    /// Pre-allocated buffer of music handles that finished during [`Self::render_interleaved`].
    ended_music_handles: Vec<TrackHandle>,
    /// Master volume setting (`0.0..=1.0`, default `1.0`).
    pub master_volume: f32,
    /// Music volume setting (`0.0..=1.0`, scaled by `buses::SHELL_MUSIC = 0.6`).
    pub music_volume: f32,
    /// Interface volume setting (`0.0..=1.0`, scaled by `buses::SHELL_FX = 0.3`).
    pub interface_volume: f32,
}

impl ShellAudioMixer {
    /// Creates a new `ShellAudioMixer` for `data` and optional `content_dir`.
    pub fn new(data: &ShellData, content_dir: Option<&Path>) -> Self {
        // Initialize the 2-bus `Frontend` DSP mixer at 48 kHz with default unit
        // volumes (`master = 1.0`, `music = 0.6`, `fx = 0.3`), matching `BUSES`
        // in `src/shell/audio.js`.
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

        // Eagerly preload all 8 menu interface sounds (`shell_select_change`,
        // `shell_menu_accept`, etc.) so the first menu click or highlight move
        // plays with zero disk I/O latency.
        for name in INTERFACE_SOUNDS {
            mixer.preload_sound(name);
        }
        mixer
    }

    /// Applies any queued `FRONT_*` commands in `self.frontend.commands` to the
    /// underlying DSP state and clears the command buffer.
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

    /// Maps a content-root-relative URL (such as `"/common/shell/audio/whoosh1.flac"`)
    /// to a filesystem path inside `self.content_dir`.
    fn resolve_content_path(&self, url: &str) -> Option<PathBuf> {
        let dir = self.content_dir.as_ref()?;
        Some(dir.join(url.trim_start_matches('/')))
    }

    /// Allocates the first free voice slot in `self.slots` (or steals slot `0`
    /// if all `frontend::SLOTS` voices are active) and assigns a fresh generation tag.
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

    /// Copies mono `pcm` samples into a `Frontend` voice slot and queues a
    /// `FRONT_START` command (`bus = 0` for `shellfx`, `bus = 1` for `shellmusic`).
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
        // SAFETY: `self.frontend.sample(slot, pcm.len(), 1, rate)` allocates and returns a
        // valid, aligned `*mut f32` pointing to `pcm.len()` contiguous elements inside
        // `self.frontend.samples[slot]`, which is disjoint from the caller-borrowed `pcm` slice.
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

    /// Preloads a named sound into `self.sounds`, checking `shell.json`'s own
    /// `"sounds"` overrides first and falling back to `core.lvl` (`CoreSounds::load_sound`),
    /// matching `ShellAudio.sound(name)` in `src/shell/audio.js`.
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
        // Enforce `SoundProperties.PlayInterval` cooldown so rapid hover or key
        // events do not re-trigger the sound before its minimum interval elapses.
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
        // Advance round-robin through `shell_transition1..10`, lazily decoding
        // each `.flac` whoosh clip on first use and caching its 48 kHz PCM buffer.
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
    /// Applies a [`MusicStep`] returned by [`crate::music::ShellMusic`] (stopping
    /// any previous track via `FRONT_STOP` and starting any newly selected track
    /// on `shellmusic` bus 1, caching decoded `.opus` PCM by track URL).
    /// Mirrors `startTrack` and `stopTrack` in `src/shell/audio.js`.
    pub fn apply_music_step(&mut self, step: &MusicStep, data: &ShellData) {
        // Stop the currently active music voice slot if its handle matches `step.stopped`.
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
        // Decode (if not yet cached) and start the newly selected music track on bus 1.
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

    /// Applies a slice of [`MediaEvent`]s emitted by [`crate::media::ShellMedia`]
    /// during screen transitions (`FadeBus` -> `fade_bus`, `Whoosh` -> `whoosh`,
    /// `Music` -> `apply_music_step`; movie events are handled by the video/backdrop player).
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
        // Ensure all four loading screen cues (`load_ytracking`, `load_xtracking`,
        // `load_zoom`, `load_transition`) have been resolved from `CoreSounds`
        // before scheduling their start/stop times on the mixer clock.
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
            // Looping pulse (`load_ytracking` or `load_xtracking`): if joining
            // late (`now > at`), compute the phase offset within the loop cycle
            // `((begin - at) * pitch) % duration` and schedule a `STOP` (`0.01s`)
            // fade-out ending at `end`, matching `LoadingSound.play` in
            // `src/shell/loading-audio.js`.
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
                // One-shot cue (`load_zoom` or `load_transition`): drop if more
                // than `LATE` (`0.05s`) past its scheduled moment.
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

        // Step the `Frontend` DSP in fixed 128-frame (`FRAMES`) quantums and
        // interleave its planar left (`output[0..FRAMES]`) and right
        // (`output[FRAMES..2*FRAMES]`) buses into `out_stereo`.
        while offset < total_frames {
            let chunk = (total_frames - offset).min(FRAMES);
            let ended_count = self.frontend.process(self.current_frame);
            self.current_frame += FRAMES as u64;

            // Copy planar left (`output[0..chunk]`) and right (`output[FRAMES..FRAMES+chunk]`)
            // samples into interleaved `[L, R, L, R, ...]` order in `out_stereo`.
            for i in 0..chunk {
                let dst = (offset + i) * 2;
                out_stereo[dst] = self.frontend.output[i];
                out_stereo[dst + 1] = self.frontend.output[FRAMES + i];
            }

            // Reclaim any voice slots that finished during this 128-frame quantum
            // and record completed music track handles so `ShellMusic::ended`
            // can start the 10-second inter-track silence gap.
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
    /// Returns and clears the list of music [`TrackHandle`]s that reached end-of-stream
    /// during [`Self::render_interleaved`] quantums since the last call, preserving
    /// completion order so the caller can notify [`crate::music::ShellMusic::ended`].
    /// Mirrors the `source.onended = () => this.music.ended(handle)` callback in
    /// `ShellAudio.retry()` (`src/shell/audio.js`).
    pub fn take_ended_music(&mut self) -> Vec<TrackHandle> {
        let out = self.ended_music_handles.clone();
        self.ended_music_handles.clear();
        out
    }

    /// Returns the mixer's current audio clock time in seconds
    /// (`current_frame / 48_000.0`).
    pub fn current_time(&self) -> f64 {
        (self.current_frame as f64) / (TARGET_RATE as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::read_shell_data;

    // Verifies that `key_of` passes through pre-hashed `0x`-prefixed 8-digit
    // lowercase hex keys and hashes ASCII sound names with the 2004 engine's
    // lowercase FNV-1a (`src/audio/hash.js`).
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

    // Verifies that `ShellAudioMixer::render_interleaved` mixes a voice on
    // `shellmusic` bus 1 into interleaved stereo output and reports its
    // `TrackHandle` via `take_ended_music()` once the 64-frame buffer ends.
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

        // Render one 128-frame quantum (`FRAMES = 128`): the 64-sample buffer
        // finishes mid-quantum and queues handle `77` in `ended_music_handles`.
        let mut stereo = vec![0.0f32; 128 * 2];
        mixer.render_interleaved(&mut stereo);
        assert!(stereo.iter().any(|&s| s > 0.0));
        assert_eq!(mixer.take_ended_music(), vec![77]);
    }
}
