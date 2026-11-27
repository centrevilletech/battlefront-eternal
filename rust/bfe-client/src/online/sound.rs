//! GameAudio facts feed the shared control runtime. One bounded producer and
//! the device callback exchange native command batches; the callback alone
//! owns Output, voice cursors, mixer timelines and partially consumed quanta.
use super::{receiver::Receiver, replay::Collision, sound_data::Prepared};
use audio::{output::Output, FRAMES};
use audio_control::{layout::*, runtime::Runtime, Collision as SoundCollision, Hit, Point};
use bfe_presentation::network_layout::{net_kind as k, net_record as r};
use std::{
    cell::UnsafeCell,
    sync::{
        atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
};
const QUEUE: usize = 16;
const OWN: usize = bfe_presentation::network_layout::net_limits::SLOTS + 1;
const COMMANDS: usize = 65536;
const PLACEMENTS: usize = 16384;
// One slot carries commands and placements for a complete control boundary.
// PCM completions precede the commands which start those samples. Device
// callbacks alone own voice cursors, so neither the window nor the decoder
// can replace a sample while the callback is borrowing its previous bytes.
struct Batch {
    commands: Vec<f64>,
    placements: Vec<f64>,
    room: [f64; audio::reverb::PARAM_COUNT],
    mask: u32,
    // The master volume (masterVolume), applied at the output's master.
    volume: f32,
    samples: Vec<(usize, Option<super::stream_assets::Pcm>)>,
    // A new mix configuration for the device to install before this batch's
    // commands, and the one it replaced, handed back to be dropped here.
    config: Option<Box<Output>>,
    retired: Option<Box<Output>>,
    // The samples this batch's arrivals replaced (released or swapped for
    // their upscaled versions), handed back to be freed here, not on the
    // device thread.
    freed: Vec<audio::voices::Sample>,
}
// A stream completion writes at most a bind and a start command.
const READY_WORDS: usize = 5 + 8;
impl Batch {
    fn room_for_ready(&self) -> bool {
        self.commands.len() + READY_WORDS <= COMMANDS
    }
    fn new() -> Self {
        Self {
            commands: Vec::with_capacity(COMMANDS),
            placements: Vec::with_capacity(PLACEMENTS),
            room: [0.; audio::reverb::PARAM_COUNT],
            mask: 0,
            volume: 1.,
            samples: Vec::with_capacity(512),
            config: None,
            retired: None,
            freed: Vec::with_capacity(512),
        }
    }
}
struct Mailbox {
    slots: Box<[UnsafeCell<Batch>]>,
    read: AtomicUsize,
    write: AtomicUsize,
    clock: AtomicU64,
    peak: AtomicU32,
    // The longest device quantum (control batches and the render) since the
    // window last read it, in nanoseconds (bench telemetry).
    worst: AtomicU64,
}
// There is exactly one window producer and one device consumer. A release
// store publishes the complete slot; the matching acquire grants exclusive
// access until the consumer publishes its read cursor. Neither wraps into a
// slot whose previous contents are still borrowed by the other thread.
unsafe impl Sync for Mailbox {}
unsafe impl Send for Mailbox {}
impl Mailbox {
    fn new() -> Self {
        Self {
            slots: (0..QUEUE).map(|_| UnsafeCell::new(Batch::new())).collect::<Vec<_>>().into_boxed_slice(),
            read: AtomicUsize::new(0),
            write: AtomicUsize::new(0),
            clock: AtomicU64::new(0),
            peak: AtomicU32::new(0),
            worst: AtomicU64::new(0),
        }
    }
    fn available(&self) -> bool {
        self.write.load(Ordering::Relaxed).wrapping_sub(self.read.load(Ordering::Acquire)) < QUEUE
    }
}
pub struct LiveOutput {
    output: Box<Output>,
    mail: Arc<Mailbox>,
    next: usize,
    volume: f32,
    /// Driven with no sound device behind it: nothing is heard, so nothing
    /// is mixed (Output::muted).
    pub headless: bool,
}
impl LiveOutput {
    // Output.process renders one fixed DSP quantum. Consume every published
    // control batch first, in order, then retain partially consumed samples
    // across callbacks of arbitrary device buffer length. A callback does not
    // advance the gameplay control runtime or ask a window thread for work.
    fn quantum(&mut self) {
        let began = std::time::Instant::now();
        let mut read = self.mail.read.load(Ordering::Relaxed);
        let write = self.mail.write.load(Ordering::Acquire);
        while read != write {
            let batch = unsafe { &mut *self.mail.slots[read % QUEUE].get() };
            if let Some(mut next) = batch.config.take() {
                // Settings changed: the new layout takes the live sources over;
                // the old one goes back to the window thread to be dropped.
                next.take_sources(&mut self.output);
                std::mem::swap(&mut self.output, &mut next);
                batch.retired = Some(next);
                // Coming back from silence the new chain starts at 0 too, so the
                // volume glides up as it does from any other level.
                if self.volume == 0. {
                    let now = self.output.frame as f64 / self.output.rate;
                    self.output.volume(0., 0., now, true);
                } else {
                    self.volume = f32::NAN;
                }
            }
            for (id, pcm) in batch.samples.drain(..) {
                if let Some(old) = self.output.voices.set_sample(id, pcm.map(|pcm| pcm.sample)) {
                    // The window reserved room for every arrival of the batch.
                    batch.freed.push(old);
                }
            }
            if let Some(master) = &mut self.output.master {
                if batch.mask != 0 {
                    master.room(&batch.room, batch.mask, false);
                }
            }
            // The master volume reaches the light mix (Classic stereo) and the
            // master chain alike, gliding over 50 ms as the sliders move.
            if batch.volume != self.volume {
                let first = self.volume.is_nan();
                let now = self.output.frame as f64 / self.output.rate;
                self.output.volume(batch.volume, if first { 0. } else { 0.05 }, now, first);
                self.volume = batch.volume;
            }
            self.output.commands[..batch.commands.len()].copy_from_slice(&batch.commands);
            self.output.apply(batch.commands.len());
            for row in batch.placements.chunks_exact(6) {
                let row: [f32; 6] = std::array::from_fn(|i| row[i] as f32);
                self.output.mixer.take(&row, false);
            }
            read = read.wrapping_add(1);
            self.mail.read.store(read, Ordering::Release);
        }
        self.output.muted = self.headless;
        self.output.process();
        self.mail.clock.store(self.output.frame, Ordering::Release);
        self.mail.worst.fetch_max(began.elapsed().as_nanos() as u64, Ordering::Relaxed);
        self.next = 0;
    }
    /// Fills interleaved device frames of `channels` channels. The output's
    /// own channels go to the device's in its order (crate::audio::order);
    /// device channels the layout lacks stay silent.
    pub fn fill<T: cpal::SizedSample + cpal::FromSample<f32>>(&mut self, data: &mut [T], channels: usize, status: &crate::audio::Status) {
        let mut peak = 0f32;
        let channels = channels.max(1);
        for frame in data.chunks_mut(channels) {
            if self.next == FRAMES {
                self.quantum();
                status.blocks.fetch_add(1, Ordering::Relaxed);
            }
            let have = self.output.channels;
            for (device, sample) in frame.iter_mut().enumerate() {
                let source = crate::audio::order(have, device);
                let value = if source < have { self.output.output[source * FRAMES + self.next] } else { 0. };
                peak = peak.max(value.abs());
                *sample = T::from_sample(value);
            }
            self.next += 1;
        }
        status.peak.fetch_max(peak.to_bits(), Ordering::Relaxed);
        self.mail.peak.fetch_max(peak.to_bits(), Ordering::Relaxed);
    }
}
struct Queries {
    collision: Collision,
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
// GameAudio.update supplies observations to Runtime; it does not implement
// another mixer or sound policy. Class ids resolve while loading. Emitter
// slots retain explicit qualified lives, and the runtime detects changes in
// the resulting facts, including scope, charge, reload and engine transitions.
// One borrowed view covers every audible presentation kind; fields a kind
// doesn't carry keep their defaults, and unknown fields are skipped.
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
    #[serde(default = "positional")]
    positional: bool,
    #[serde(default = "no_building")]
    building: usize,
}
// attachPresentationCapture writes `positional: record.positional !== false`.
fn positional() -> bool {
    true
}
// Only a "serve" record names a building; no other kind may select one.
fn no_building() -> usize {
    usize::MAX
}
// The record kinds that make a sound.
const AUDIBLE: [&str; 7] = ["shot", "vo", "bounce", "pickup", "serve", "towAttach", "towRelease"];
#[derive(Clone, Copy)]
struct Shot {
    owner_id: u32,
    owner_generation: u32,
    weapon_id: usize,
    channel: usize,
    position: Option<[f64; 3]>,
    repair: bool,
}
// A record whose unit this client did not know yet, kept as it arrived:
// the tick it stops being worth playing, and its payload.
#[derive(Clone, Copy)]
struct Held {
    expiry: f64,
    length: usize,
    bytes: [u8; PAYLOAD],
}
// presentationRecord refuses a payload over 900 bytes.
const PAYLOAD: usize = 900;
const HELD: usize = 64;
const MAX_SPACE_HULLS: usize = 256;
const MAX_SPACE_TURRETS: usize = 256;
const MAX_HERO_SABERS: usize = bfe_sim::units::infantry::hero_audio::MAX_HERO_SABERS;

#[derive(Clone, Copy, PartialEq, Eq)]
struct HeroSaberTrack {
    emitter: usize,
    slot: u32,
    generation: u32,
    weapon_hash: u32,
    state_hash: u32,
    blades_off: u8,
    thrown: bool,
}
pub struct Sound {
    /// The weapons whose shot the player's own client plays at the trigger
    /// press, by registry id, and each weapon's registry id by its place in
    /// the battle's weapon list (lightning.rs Guns).
    pub ahead: (Vec<bool>, Vec<usize>),
    /// The player's volumes, 0 to 1: master, effects, ambience, music, voice,
    /// interface (applyVolumes and categoryVolume).
    pub volumes: [f64; 6],
    /// The own body's feet above its origin as drawn (Replicas::own_feet).
    pub own_feet: [f64; 3],
    /// Every turret seat as drawn this frame (Hulls::sound_turrets).
    pub turret_seats: Vec<((u32, u32), usize, bool, f64, f64, f64, [f64; 3])>,
    // The client's replay ran for the player's own body this frame.
    replaying: bool,
    assets: super::stream_assets::Assets,
    pub decoded: u64,
    pub voice_decoded: u64,
    voice_segments: Vec<bool>,
    posts: Vec<(usize,[f64;post::STRIDE])>,
    post_building: Vec<bool>,
    runtime: Runtime,
    pending: Vec<Shot>,
    pending_effects: Vec<[f64; event::STRIDE]>,
    held: Vec<Held>,
    cosmetic: bfe_sim::wire::cosmetics::Compact,
    combat_classes: Vec<usize>,
    stations: Vec<(usize, [f64; 3], bool)>,
    // Whether each live building stands ("built"), as the server last said.
    built: Vec<bool>,
    // The mission voice-overs this battle has played.
    voices_heard: crate::local::mission_view::VoicesHeard,
    pub event_counts: [u64; 28],
    pub foot_counts: [u64; 2],
    saber_state: Option<(u32, u32, u32)>,
    saber_contact: u32,
    /// How many supply stations play their ambient loop now.
    pub station_loops: u64,
    pub bleed: [f64; 2],
    epoch: f64,
    mail: Arc<Mailbox>,
    pub output: Option<LiveOutput>,
    queries: Queries,
    actors: Vec<usize>,
    actor_limits: Vec<[f64; 3]>,
    weapons: Vec<usize>,
    prediction_weapons: Vec<([f64; weapon::STRIDE], usize)>,
    lives: Vec<Option<(f64, f64)>>,
    pilots: super::index::Index,
    own_weapons: [u32; 2],
    pub batches: u64,
    pub events: u64,
    // With the bench's fx key, a line for each explosion heard.
    trace_explosions: bool,
    pub refused: u64,
    last_clock: u64,
    // Game time of the frames that ran no control step (the device had not
    // moved), handed to the next step so delays and fades keep real time.
    carried: f64,
    music: Vec<[f64; music_slot::STRIDE]>,
    starts: [f64; 2],
    ui_paused: bool,
    ui_board: bool,
    // The audio settings in force and what they selected; the control
    // runtime's configuration count, which re-sends every placement.
    audio: super::sound_output::AudioSettings,
    rendering: super::sound_output::Rendering,
    configuration: f64,
    // A configuration built before the device took the output, or waiting
    // for the next batch.
    pending_config: Option<Box<Output>>,
    // The master volume last sent was 0: the device mixes nothing meanwhile.
    silent_sent: bool,
    // The reverb last sent, so a new configuration starts in the same room.
    room: [f64; audio::reverb::PARAM_COUNT],
    room_mask: u32,
    // The upscaled samples: waiting for Enhanced to be chosen, then loading.
    enhanced_items: Vec<super::enhanced_audio::Item>,
    enhanced_base: usize,
    enhanced_loading: Option<std::sync::mpsc::Receiver<super::enhanced_audio::Loaded>>,
    /// How many upscaled samples have arrived.
    pub enhanced_loaded: u64,
    headphones: Option<(std::sync::Arc<Vec<f32>>, usize)>,
    sound_names: Vec<String>,
    // The device generation the output was last resolved against.
    device_seen: u64,
    // The sub-level's spaces, the boxes that pick them, the dry default, and
    // the listener's space now (Classic's reverb).
    spaces: Vec<(f64, [f64; room::STRIDE])>,
    space_boxes: Vec<super::sound_data::SpaceBox>,
    dry: (f64, [f64; room::STRIDE]),
    space: Option<usize>,
    space_set: bool,
    // Chatter records received, by property (bench telemetry).
    vo_asked: Vec<(String, u64)>,
    // 2005 space battle audio tracking (pre-allocated, zero steady-state heap allocations).
    space_tick: Option<u32>,
    space_hull_lives: Box<[Option<u32>; MAX_SPACE_HULLS]>,
    space_hull_tricks: Box<[bfe_sim::space::flight::SpaceTrick; MAX_SPACE_HULLS]>,
    space_ordnance_scratch: Vec<audio_control::swbf2::weapons::SpaceOrdnanceInput>,
    // 2005 hero power and saber audio tracking (player and bots, zero steady-state allocations).
    hero_power_seq: u32,
    hero_saber_tracks: [Option<HeroSaberTrack>; MAX_HERO_SABERS],
    hero_hold_scratch: Vec<audio_control::swbf2::powers::PowerHoldInput>,
    hero_throw_scratch: Vec<audio_control::swbf2::powers::SaberThrowInput>,
}
impl Sound {
    pub fn new(prepared: Prepared, collision: Collision, world: u32) -> Self {
        let mail = Arc::new(Mailbox::new());
        let output = Some(LiveOutput { output: Box::new(prepared.output), mail: mail.clone(), next: FRAMES, volume: f32::NAN, headless: false });
        Self {
            ahead: Default::default(),
            volumes: [1.; 6],
            own_feet: [f64::NAN; 3],
            turret_seats: Vec::new(),
            replaying: false,
            assets: prepared.streams,
            decoded: 0,
            voice_decoded: 0,
            voice_segments: prepared.voice_segments,
            posts: prepared.posts,
            post_building: prepared.post_building,
            runtime: prepared.runtime,
            sound_names: prepared.sound_names,
            vo_asked: Vec::new(),
            pending: Vec::with_capacity(2048),
            pending_effects: Vec::with_capacity(2048),
            held: Vec::with_capacity(HELD),
            cosmetic: Default::default(),
            combat_classes: prepared.combat_classes,
            built: vec![true; prepared.stations.len()],
            stations: prepared.stations,
            voices_heard: Default::default(),
            event_counts: [0;28],
            foot_counts: [0;2],
            saber_state: None, saber_contact: 0,
            station_loops: 0,
            bleed: [0.;2],
            epoch: f64::NAN,
            mail,
            output,
            queries: Queries { collision, world },
            actors: prepared.actor_classes,
            actor_limits: prepared.actor_limits,
            weapons: prepared.weapon_classes,
            prediction_weapons: prepared.prediction_weapons,
            lives: vec![None; OWN + 1],
            pilots: super::index::Index::new(512),
            own_weapons: [0; 2],
            batches: 0,
            events: 0,
            trace_explosions: crate::app::bench::fx().is_some(),
            refused: 0,
            last_clock: 0,
            carried: 0.,
            music: prepared.music,
            starts: [f64::NAN; 2],
            ui_paused: false,
            ui_board: false,
            // No settings yet: the first set_audio always builds and publishes the output in force.
            audio: super::sound_output::AudioSettings { output: String::new(), ..Default::default() },
            rendering: super::sound_output::Rendering {
                layout: "stereo", asked: "stereo", channels: 2, enhanced: false, light: false, hrtf: false, binaural: false, lfe: false,
            },
            device_seen: crate::audio::device_generation(),
            spaces: prepared.spaces,
            space_boxes: prepared.space_boxes,
            dry: (0., { let mut r = [0.; room::STRIDE]; r[room::ROOM_HF] = 1.; r[room::DECAY] = 1.; r[room::HF_RATIO] = 0.5; r[room::REFL_DELAY] = 0.02; r[room::REVERB_DELAY] = 0.04; r[room::DIFFUSION] = 1.; r[room::DENSITY] = 1.; r[room::HF_REFERENCE] = 5000.; r[room::MASK] = 1.; r }),
            space: None,
            space_set: false,
            configuration: 0.,
            pending_config: None,
            silent_sent: false,
            room: [0.; audio::reverb::PARAM_COUNT],
            room_mask: 0,
            enhanced_items: prepared.enhanced,
            enhanced_base: prepared.enhanced_base,
            enhanced_loading: None,
            enhanced_loaded: 0,
            headphones: prepared.headphones,
            space_tick: None,
            space_hull_lives: Box::new([None; MAX_SPACE_HULLS]),
            space_hull_tricks: Box::new([bfe_sim::space::flight::SpaceTrick::None; MAX_SPACE_HULLS]),
            space_ordnance_scratch: Vec::with_capacity(
                audio_control::swbf2::weapons::MAX_SPACE_ORDNANCE_INPUTS,
            ),
            hero_power_seq: 0,
            hero_saber_tracks: [None; MAX_HERO_SABERS],
            hero_hold_scratch: Vec::with_capacity(
                audio_control::swbf2::powers::MAX_POWER_HOLDS,
            ),
            hero_throw_scratch: Vec::with_capacity(
                audio_control::swbf2::powers::MAX_SABER_THROWS,
            ),
        }
    }
    /// The player's audio settings (Settings > Audio). A change builds the new
    /// mix configuration here, off the device thread; the device installs it
    /// before the next batch, keeping every sound that plays.
    pub fn set_audio(&mut self, settings: &super::sound_output::AudioSettings) {
        let (default, widths) = crate::audio::device_channels();
        self.device_seen = crate::audio::device_generation();
        let wanted = super::sound_output::resolve(&settings.output, default, widths).name;
        if *settings == self.audio && wanted == self.rendering.layout {
            return;
        }
        self.configure(settings, default, widths);
    }
    // Builds the mix configuration for `settings` and hands it to the device.
    fn configure(&mut self, settings: &super::sound_output::AudioSettings, default: usize, widths: u32) {
        let room = (self.room_mask != 0).then_some((&self.room, self.room_mask));
        let (mut next, rendering) =
            super::sound_output::build(settings, default, widths, room, self.volumes[0].clamp(0., 1.) as f32,
                self.headphones.as_ref().map(|(filters, frames)| (filters.as_slice(), *frames)));
        crate::audio::want_channels(rendering.channels);
        if rendering.enhanced && self.enhanced_loading.is_none() && !self.enhanced_items.is_empty() {
            self.enhanced_loading =
                Some(super::enhanced_audio::load(std::mem::take(&mut self.enhanced_items), self.enhanced_base));
        }
        self.audio = settings.clone();
        self.rendering = rendering;
        self.configuration += 1.;
        if let Some(live) = &mut self.output {
            // Not yet playing: install it in place.
            next.take_sources(&mut live.output);
            *live.output = next;
        } else {
            self.pending_config = Some(Box::new(next));
        }
    }
    /// The output's layout and rendering in force.
    /// The bench telemetry's sound state: voices per category (playing, and heard of those),
    /// the engine's counters (heard, silent, started, stolen, dropped, culled), stream voices
    /// by sound name and category, the music stage and the track playing.
    pub fn bench(&self) -> serde_json::Value {
        let p = &self.runtime.phase;
        let e = &p.engine;
        let (mut live, mut heard) = ([0u32; 8], [0u32; 8]);
        for v in e.voices.iter().filter(|v| !v.done) {
            live[v.category.min(7)] += 1;
            heard[v.category.min(7)] += v.real as u32;
        }
        let name = |id: usize| self.sound_names.get(id).map_or("", String::as_str);
        let streams: Vec<_> = p.streams.census().map(|(sound, category)| serde_json::json!([name(sound), category])).collect();
        let track = p.director.tracks.iter().find(|t| t.playing()).map(|t| name(t.sound));
        // Heard placed voices past 4 m: how many, their mean occlusion, how many
        // more than half occluded, their mean distance; and the fastest radial
        // speed among heard ones (Doppler), with how many move over 5 m/s.
        let (mut n, mut occluded, mut over, mut far, mut fastest, mut moving, mut fastest_kind) = (0u32, 0., 0u32, 0., 0f64, 0u32, -1.);
        for v in e.voices.iter().filter(|v| !v.done && v.real && v.mode3d) {
            if v.distance > 4. {
                let o = v.occluded.unwrap_or(0.);
                n += 1;
                occluded += o;
                over += (o > 0.5) as u32;
                far += v.distance;
            }
            if v.radial.abs() < 172. {
                if v.radial.abs() > fastest {
                    fastest = v.radial.abs();
                    fastest_kind = v.category as f64;
                }
                moving += (v.radial.abs() > 5.) as u32;
            }
        }
        let occlusion = [n as f64, if n > 0 { occluded / n as f64 } else { 0. }, over as f64, if n > 0 { far / n as f64 } else { 0. }];
        let worst = self.mail.worst.swap(0, Ordering::Relaxed) as f64 / 1000.;
        serde_json::json!({ "live": live, "heard": heard, "stats": e.stats, "streams": streams,
            "stage": p.director.stage, "track": track, "worstUs": worst, "voAsked": self.vo_asked,
            "occlusion": occlusion, "doppler": [fastest, moving as f64, fastest_kind] })
    }
    pub fn rendering(&self) -> super::sound_output::Rendering {
        self.rendering
    }
    pub fn device_frames(&self) -> u64 {
        self.mail.clock.load(Ordering::Acquire)
    }
    pub fn peak(&self) -> f32 {
        f32::from_bits(self.mail.peak.load(Ordering::Relaxed))
    }
    // Battle UI uses the same bounded event path as replicated effects.
    // Core-bank names and bus names resolve in the shared control runtime.
    pub fn interface(&mut self, name: &str) {
        self.ui_event(event_kind::INTERFACE, name, 0., 0.);
    }

    fn fade_bus(&mut self, name: &str, value: f64, seconds: f64) {
        self.ui_event(event_kind::BUS_FADE, name, value, seconds);
    }

    pub fn set_board_open(&mut self, open: bool) {
        if open != self.ui_board {
            self.ui_board = open;
            if self.runtime.phase.family() == bfe_family::GameFamily::Swbf2 {
                self.fade_bus("0xb38617c5", if open { 0.5 } else { 1. }, 0.25);
            } else {
                self.fade_bus("ingame", if open { 0. } else { 1. }, 1.);
            }
        }
    }

    /// Narrow 2005 gameplay and script-host audio entry points. All methods are
    /// gated inside `Phase` on `GameFamily::Swbf2` and are no-ops in 2004 battles.
    pub fn swbf2_push_saber_event(&mut self, event: audio_control::swbf2::saber::SaberEvent) {
        self.runtime.phase.push_saber_event(event);
    }
    pub fn swbf2_push_vehicle_event(&mut self, event: audio_control::swbf2::vehicles::VehicleEvent) {
        self.runtime.phase.push_vehicle_event(event);
    }
    pub fn swbf2_push_door_event(&mut self, event: audio_control::swbf2::vehicles::DoorEvent) {
        self.runtime.phase.push_door_event(event);
    }
    pub fn swbf2_update_chaingun(&mut self, input: audio_control::swbf2::vehicles::ChaingunInput) {
        self.runtime.phase.update_chaingun(input);
    }
    pub fn swbf2_set_own_mount(&mut self, vehicle_class_hash: Option<u32>, first_person: bool) {
        self.runtime.phase.set_own_mount_swbf2(vehicle_class_hash, first_person);
    }
    pub fn swbf2_set_own_hero(&mut self, hero_class_hash: Option<u32>) {
        self.runtime.phase.set_own_hero_swbf2(hero_class_hash);
    }
    pub fn swbf2_set_building_alive(&mut self, instance_name_hash: u32, alive: bool) {
        self.runtime.phase.set_building_alive_swbf2(instance_name_hash, alive);
    }
    pub fn swbf2_set_doppler_factor(&mut self, factor: f64) {
        self.runtime.phase.set_doppler_factor(factor);
    }
    pub fn swbf2_scale_sound_parameter(
        &mut self,
        group_hash: u32,
        param: audio_control::swbf2::SoundParam,
        scale: f64,
    ) {
        self.runtime.phase.scale_sound_parameter(group_hash, param, scale);
    }
    pub fn swbf2_trigger_sound_region_enable(&mut self, region_hash: u32, enabled: bool) {
        self.runtime.phase.trigger_sound_region_enable(region_hash, enabled);
    }
    pub fn swbf2_enable_hero_music(&mut self, enabled: bool) {
        self.runtime.phase.enable_hero_music(enabled);
    }
    pub fn swbf2_enable_hero_vo(&mut self, enabled: bool) {
        self.runtime.phase.enable_hero_vo(enabled);
    }
    pub fn swbf2_set_mode_score_progress(&mut self, team1_remaining: f64, team2_remaining: f64) {
        self.runtime.phase.set_mode_score_progress(team1_remaining, team2_remaining);
    }
    pub fn swbf2_broadcast_voice_over(&mut self, sound_hash: u32, team_filter: u8) {
        self.runtime.phase.broadcast_voice_over(sound_hash, team_filter);
    }
    /// Plays the mission voice-overs of the view the battle's screens read that
    /// have not played yet, for a local world's mission and a server's alike.
    pub fn swbf2_mission_voices(&mut self, view: &crate::local::mission_view::Observation) {
        let mut heard = std::mem::take(&mut self.voices_heard);
        heard.take(view, |sound, team| {
            let filter = team.and_then(|team| u8::try_from(team).ok()).unwrap_or(0);
            self.swbf2_broadcast_voice_over(audio_control::swbf2::zero_hash(sound), filter);
        });
        self.voices_heard = heard;
    }
    pub fn swbf2_play_in_game_music(&mut self, sound_hash: u32) {
        self.runtime.phase.play_in_game_music(sound_hash);
    }
    pub fn swbf2_stop_in_game_music(&mut self, fade_s: f64) {
        self.runtime.phase.stop_in_game_music(fade_s);
    }
    pub fn swbf2_set_low_reinforcements_vo(
        &mut self,
        watched_team: usize,
        threshold: f64,
        sound_hash: u32,
        is_fraction: bool,
        listener_team: usize,
    ) {
        self.runtime
            .phase
            .set_low_reinforcements_vo(watched_team, threshold, sound_hash, is_fraction, listener_team);
    }
    pub fn swbf2_notify_hero_event(
        &mut self,
        hero_class_hash: u32,
        kind: audio_control::swbf2::director::HeroVoKind,
    ) {
        self.runtime.phase.notify_hero_event(hero_class_hash, kind);
    }
    pub fn swbf2_play_vo_tag(
        &mut self,
        class_hash: u32,
        event_hash: u32,
        conditions: &[u32],
        position: Option<Point>,
        positional: bool,
    ) -> bool {
        self.runtime
            .phase
            .play_vo_tag(class_hash, event_hash, conditions, position, positional)
    }

    pub fn set_paused(&mut self, paused: bool) {
        if paused != self.ui_paused {
            self.ui_paused = paused;
            self.ui_event(event_kind::PAUSE, "", paused as u8 as f64, 0.);
        }
    }

    /// GameAudio.explosion: an explosion's SoundProperty at its point, by
    /// its combat definition (one-based).
    pub fn explosion(&mut self, definition: usize, position: [f64; 3]) {
        let class = self.combat_classes.get(definition).copied().filter(|&c| c != usize::MAX && definition != 0);
        if self.trace_explosions && class.is_none() {
            eprintln!("fx-sound explosion definition={definition} has no sound class");
        }
        let Some(class) = class else { return };
        if self.pending_effects.len() == self.pending_effects.capacity() {
            self.pending_effects.remove(0);
            self.refused += 1;
        }
        self.pending_effects.push(super::sound_events::placed(event_kind::EXPLOSION, class, position));
        self.events += 1;
        if self.trace_explosions {
            eprintln!("fx-sound explosion definition={definition} class={class} at={position:.1?}");
        }
    }

    fn ui_event(&mut self, kind: usize, name: &str, value: f64, seconds: f64) {
        if self.pending_effects.len() == self.pending_effects.capacity() {
            self.refused += 1;
            return;
        }
        if std::env::var_os("BFE_MENU_INPUT_TRACE").is_some() {
            let key = super::sound_data::hash(name);
            let resolved = match kind {
                event_kind::INTERFACE => self.runtime.phase.effects.iter()
                    .find(|(k, _)| *k == key).map_or_else(
                        || self.runtime.phase.named(key), |(_, id)| *id) != usize::MAX,
                event_kind::BUS_FADE => self.runtime.phase.bus_names.iter().any(|(k, _)| *k == key),
                _ => true,
            };
            eprintln!("battle audio UI: kind={kind} name={name} resolved={resolved} value={value} seconds={seconds}");
        }
        let mut row = [0.; event::STRIDE];
        row[event::KIND] = kind as f64;
        row[event::SOUND] = if self.runtime.phase.family() == bfe_family::GameFamily::Swbf2 {
            super::sound_data::key_hash(name) as f64
        } else {
            super::sound_data::hash(name) as f64
        };
        row[event::VALUE] = value;
        row[event::SECONDS] = seconds;
        self.pending_effects.push(row);
    }

    // presentEntityEvent "vo" hands Chatter.say the
    // speaker and the property. The line is the speaker's class sound for
    // that property; a hull speaks with its occupant's class. A speaker this
    // client doesn't know, or one whose class has no sounds, says nothing.
    // Only the local player's own line may be 2D; everyone else's is always
    // placed where it was said.
    fn chatter(&self, receiver: &Receiver, source: &EventInput) -> Option<[f64; event::STRIDE]> {
        if source.owner_id == 0 || source.property.is_empty() {
            return None;
        }
        let own = source.owner_id as f64 == receiver.own.id
            && source.owner_generation as f64 == receiver.own.generation;
        let class = if own {
            receiver.own.class
        } else {
            let row = unit(receiver, source.owner_id, source.owner_generation)?;
            let row = if row[r::KIND] == k::VEHICLE as f64 {
                receiver
                    .active()
                    .find(|p| {
                        p[r::SEAT_PRESENT] != 0.
                            && p[r::SEAT_INDEX] == 0.
                            && p[r::SEAT_VEHICLE_ID] == row[r::ID]
                            && p[r::SEAT_VEHICLE_GENERATION] == row[r::GENERATION]
                    })
                    .unwrap_or(row)
            } else {
                row
            };
            row[r::CLASS_ID] as usize
        };
        let class = registered(&self.actors, class).filter(|c| *c != usize::MAX)?;
        let key = super::sound_data::key_hash(&source.property);
        Some(super::sound_events::chatter(class, key, source.position, !own || source.positional))
    }

    // presentEntityEvent "bounce"/"pickup": GameAudio's bounce and pickup
    // handlers play the combat definition's class sound,
    // CollisionOtherSound or PowerupSound, where it happened. An unknown
    // definition or a missing point plays nothing.
    fn ordnance(&self, source: &EventInput) -> Option<[f64; event::STRIDE]> {
        let class = *self.combat_classes.get(source.definition_id).filter(|_| source.definition_id != 0)?;
        let kind = if source.kind == "bounce" {
            event_kind::BOUNCE
        } else {
            event_kind::POWERUP
        };
        Some(super::sound_events::placed(kind, class, source.position?))
    }

    // presentEntityEvent "serve": SceneSounds.served
    // plays the station's class PowerupSound at the station. The record
    // names it by its place among the live buildings.
    fn serve(&self, source: &EventInput) -> Option<[f64; event::STRIDE]> {
        let (class, position, _) = *self.stations.get(source.building)?;
        Some(super::sound_events::placed(event_kind::POWERUP, class, position))
    }

    // presentEntityEvent "towAttach"/"towRelease": GameAudio.towSound
    // plays the cable weapon's attach or release sound
    // at the hull, the snowspeeder's when the weapon's bank has none. The
    // browser finds the weapon among the hull's seats; the record's registry
    // id names the same weapon here.
    fn tow(&self, receiver: &Receiver, source: &EventInput) -> Option<[f64; event::STRIDE]> {
        let hull = unit(receiver, source.owner_id, source.owner_generation)
            .filter(|row| row[r::KIND] == k::VEHICLE as f64)?;
        let class = registered(&self.weapons, source.weapon_id)?;
        let kind = if source.kind == "towAttach" {
            event_kind::TOW_ATTACH
        } else {
            event_kind::TOW_RELEASE
        };
        let mut position = [0.; 3];
        position.copy_from_slice(&hull[r::POSITION..r::POSITION + 3]);
        Some(super::sound_events::placed(kind, class, position))
    }

    // presentEntityEvent: `event.ownerId && (!u || u.generation !==
    // event.ownerGeneration)`. A record naming nobody is never waited for.
    fn known(&self, receiver: &Receiver, source: &EventInput) -> bool {
        source.owner_id == 0
            || (source.owner_id as f64 == receiver.own.id && source.owner_generation as f64 == receiver.own.generation)
            || unit(receiver, source.owner_id, source.owner_generation).is_some()
    }
    // presentEntityEvent's audible branches. "shot" waits for the emitter
    // rows of the next audio step; the others carry every fact they need and
    // join the scalar event queue in arrival order.
    fn audible(&mut self, receiver: &Receiver, source: &EventInput) {
        let event = match source.kind {
            "shot" => {
                // Heard already, at the trigger press (PredictedShots.fire).
                let own = source.owner_id as f64 == receiver.own.id && source.owner_generation as f64 == receiver.own.generation;
                if own && self.replaying && !source.repair && self.ahead.0.get(source.weapon_id) == Some(&true) {
                    return;
                }
                if self.pending.len() == self.pending.capacity() {
                    self.pending.remove(0);
                    self.refused += 1;
                }
                self.pending.push(Shot {
                    owner_id: source.owner_id,
                    owner_generation: source.owner_generation,
                    weapon_id: source.weapon_id,
                    channel: source.channel,
                    position: source.position,
                    repair: source.repair,
                });
                self.events += 1;
                return;
            }
            "vo" => {
                match self.vo_asked.iter_mut().find(|(p, _)| *p == source.property) {
                    Some((_, n)) => *n += 1,
                    None => self.vo_asked.push((source.property.to_string(), 1)),
                }
                self.chatter(receiver, source)
            }
            "bounce" | "pickup" => self.ordnance(source),
            "serve" => self.serve(source),
            "towAttach" | "towRelease" => self.tow(receiver, source),
            _ => None,
        };
        if let Some(event) = event {
            if self.pending_effects.len() == self.pending_effects.capacity() {
                self.pending_effects.remove(0);
                self.refused += 1;
            }
            self.pending_effects.push(event);
        }
    }

    pub fn frame(
        &mut self,
        receiver: &Receiver,
        listener: &[f64; 16],
        dt: f64,
        prediction: &super::replay::Replay,
        battle: ([f64; 4], bool),
        hulls: &super::hulls::Hulls,
    ) {
        if self.output.is_some() {
            return;
        }
        // The device was opened again (another width, or a fallback after a
        // width it would not play): resolve the layout against it again.
        if crate::audio::device_generation() != self.device_seen && !self.audio.output.is_empty() {
            let settings = self.audio.clone();
            self.set_audio(&settings);
        }
        // Device time can stand still between two window frames. Retain
        // authoritative fire facts before that early return; the scene may
        // consume its packet journal while no audio quantum has elapsed.
        // A paused window keeps the newest bounded tail without reserving.
        if self.epoch != receiver.epoch {
            self.epoch = receiver.epoch;
            self.saber_contact = 0;
            self.hero_power_seq = 0;
            self.hero_saber_tracks.fill(None);
            self.space_tick = None;
            self.space_hull_lives.fill(None);
            self.space_hull_tricks.fill(bfe_sim::space::flight::SpaceTrick::None);
            self.pending.clear();
            self.pending_effects.clear();
            self.held.clear();
        }
        if let Some(events) = receiver.contacts.as_deref() {
            use audio_control::engine::NONE;
            use audio_control::swbf2::saber::SaberEvent;
            use bfe_sim::units::infantry::contact::{Cause, Outcome};
            let hero_snap = receiver.hero_audio.as_deref();
            let lookup_blade = |tracks: &[Option<HeroSaberTrack>], slot: u32, generation: u32| -> Option<u32> {
                hero_snap
                    .and_then(|snap| {
                        snap.throws()
                            .find(|t| t.owner_slot == slot && t.owner_generation == generation)
                            .map(|t| t.saber_weapon)
                            .or_else(|| {
                                snap.sabers()
                                    .find(|s| s.owner_slot == slot && s.owner_generation == generation)
                                    .map(|s| s.weapon_hash)
                            })
                    })
                    .or_else(|| {
                        tracks
                            .iter()
                            .flatten()
                            .find(|tr| tr.slot == slot && tr.generation == generation)
                            .map(|tr| tr.weapon_hash)
                    })
                    .or_else(|| {
                        let is_own = receiver.own.alive
                            && slot == receiver.own.id as u32
                            && generation == receiver.own.generation as u32;
                        is_own
                            .then(|| receiver.infantry.and_then(|s| s.saber).map(|s| s.weapon_hash))
                            .flatten()
                    })
            };
            for e in events.since(self.saber_contact) {
                self.saber_contact = e.sequence;
                let position = e.point;
                let cue = match (e.cause, e.outcome) {
                    (Cause::Saber { weapon, .. }, Outcome::Hit) => {
                        let attacker = e
                            .source
                            .and_then(|src| resolve_hero_emitter(receiver, src.slot, src.generation))
                            .unwrap_or(NONE);
                        SaberEvent::Hit { owner: attacker, weapon_class_hash: weapon, position }
                    }
                    (Cause::Throw { .. }, Outcome::Hit) => {
                        let attacker = e
                            .source
                            .and_then(|src| resolve_hero_emitter(receiver, src.slot, src.generation))
                            .unwrap_or(NONE);
                        let weapon = e
                            .source
                            .and_then(|src| lookup_blade(&self.hero_saber_tracks, src.slot, src.generation))
                            .unwrap_or(0);
                        SaberEvent::Hit { owner: attacker, weapon_class_hash: weapon, position }
                    }
                    (_, Outcome::Blocked) => {
                        let defender = resolve_hero_emitter(receiver, e.target.slot, e.target.generation)
                            .unwrap_or(NONE);
                        if let Some(weapon_class_hash) =
                            lookup_blade(&self.hero_saber_tracks, e.target.slot, e.target.generation)
                        {
                            SaberEvent::Deflect { owner: defender, weapon_class_hash, position }
                        } else {
                            SaberEvent::Block { owner: defender, sound_hash: 0, position }
                        }
                    }
                    (_, Outcome::Deflected) => {
                        let Some(weapon_class_hash) =
                            lookup_blade(&self.hero_saber_tracks, e.target.slot, e.target.generation)
                        else {
                            continue;
                        };
                        let defender = resolve_hero_emitter(receiver, e.target.slot, e.target.generation)
                            .unwrap_or(NONE);
                        SaberEvent::Deflect { owner: defender, weapon_class_hash, position }
                    }
                    _ => continue,
                };
                self.swbf2_push_saber_event(cue);
            }
        }
        if receiver.hero_audio.is_none() {
            if let Some(sample) = receiver.infantry.filter(|s| receiver.own.alive && s.generation == receiver.own.generation) {
                if let Some(saber) = sample.saber {
                    use audio_control::swbf2::saber::SaberEvent;
                    let next = (sample.id, sample.generation as u32, saber.state_hash);
                    if self.saber_state != Some(next) {
                        self.swbf2_push_saber_event(SaberEvent::SetBladeActive {
                            owner: OWN, weapon_class_hash: saber.weapon_hash, active: sample.blades_off != u8::MAX,
                        });
                        self.swbf2_push_saber_event(SaberEvent::ComboState {
                            owner: OWN, combo_hash: saber.combo_hash, state_hash: saber.state_hash,
                        });
                        self.saber_state = Some(next);
                    }
                }
            } else if self.saber_state.take().is_some() {
                self.swbf2_push_saber_event(audio_control::swbf2::saber::SaberEvent::Extinguish {
                    owner: OWN, weapon_class_hash: 0,
                });
            }
        }
        // The player's own rounds fired in this frame's replay are heard now.
        self.replaying = prediction.ahead();
        for shot in &prediction.shots {
            let weapon = self.ahead.1.get(shot.weapon).copied().unwrap_or(0);
            if self.ahead.0.get(weapon) == Some(&true) && self.pending.len() < self.pending.capacity() {
                self.pending.push(Shot {
                    owner_id: receiver.own.id as u32,
                    owner_generation: receiver.own.generation as u32,
                    weapon_id: weapon,
                    channel: shot.button as usize,
                    position: None,
                    repair: false,
                });
                self.events += 1;
            }
        }
        for field in receiver.publications.iter() {
            // applyWorldSummary publishes post ownership before the audio
            // phase. A newly received first sample is initialization, not a
            // capture; subsequent changes feed the ordinary post sound/VO.
            if field.row[r::KIND] == k::SUMMARY as f64
                && field.row[r::SCHEMA] == 2.
                && field.length >= 23
            {
                apply_post_summary(&mut self.posts, &self.post_building, &mut self.pending_effects, &field.bytes[..field.length]);
            }
            // applyWorldPresentation, schema 7: a building's state leads its
            // record, after its place among the live buildings. Only "built"
            // is alive (Building.alive).
            if field.row[r::KIND] == k::SUMMARY as f64 && field.row[r::SCHEMA] == 7. {
                use serde::de::IgnoredAny as Rest;
                type State<'a> = (usize, &'a str, Rest, Rest, Rest, Rest, Rest, Rest, Rest, Rest, Rest);
                if let Ok(state) = serde_json::from_slice::<State>(&field.bytes[..field.length]) {
                    if field.row[r::ID] == 200000. + state.0 as f64 {
                        if let Some(built) = self.built.get_mut(state.0) {
                            *built = state.1 == "built";
                        }
                    }
                }
            }
            // NetworkCombatViews dispatches the terminal's sound as well as
            // its visual effect. Retain it if the audio clock has not moved.
            if field.row[r::KIND] == k::COMBAT as f64 && field.row[r::SCHEMA] == 2. {
                if bfe_sim::wire::cosmetics::decode_bolt_into(
                    2.,
                    field.row[r::TICK],
                    &field.bytes[..field.length],
                    &mut self.cosmetic,
                )
                .is_ok()
                {
                    if let Some(&class) = self
                        .combat_classes
                        .get(self.cosmetic.definition as usize)
                        .filter(|&&c| c != usize::MAX)
                    {
                        if self.pending_effects.len() == self.pending_effects.capacity() {
                            self.pending_effects.remove(0);
                            self.refused += 1;
                        }
                        self.pending_effects
                            .push(super::sound_events::impact(&self.cosmetic, class));
                    }
                }
            }
            // Ordnance.explosion's emitSound("explosion"): a blast heard where
            // it went off (schema 5: definition, then its point).
            if field.row[r::KIND] == k::COMBAT as f64 && field.row[r::SCHEMA] == 5. && field.length == 33 {
                let b = &field.bytes[..33];
                let definition = u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
                let position: [f64; 3] = std::array::from_fn(|i| f64::from_le_bytes(b[8 + i * 8..16 + i * 8].try_into().unwrap()));
                self.explosion(definition, position);
            }
            if field.row[r::KIND] != k::COMBAT as f64 || field.row[r::SCHEMA] != 8. {
                continue;
            }
            let bytes = &field.bytes[..field.length];
            let Ok(source) = serde_json::from_slice::<EventInput>(bytes) else {
                continue;
            };
            // presentEntityEvent returns false for a record whose unit is
            // not known here yet, and CombatPresentation asks again until
            // the record's tick + 120. Keep the payload for those asks.
            if !self.known(receiver, &source) {
                if AUDIBLE.contains(&source.kind) && bytes.len() <= PAYLOAD {
                    if self.held.len() == HELD {
                        self.held.remove(0);
                        self.refused += 1;
                    }
                    let mut held = Held { expiry: field.row[r::TICK] + 120., length: bytes.len(), bytes: [0; PAYLOAD] };
                    held.bytes[..bytes.len()].copy_from_slice(bytes);
                    self.held.push(held);
                }
                continue;
            }
            self.audible(receiver, &source);
        }
        // The kept ones after the new ones, oldest first; each leaves once
        // its unit is known or its time is up. Taking the list out lends its
        // payloads while the events they name are queued; it keeps its
        // storage.
        let mut held = std::mem::take(&mut self.held);
        held.retain(|held| {
            let Ok(source) = serde_json::from_slice::<EventInput>(&held.bytes[..held.length]) else {
                return false;
            };
            if self.known(receiver, &source) {
                self.audible(receiver, &source);
                return false;
            }
            held.expiry >= receiver.latest
        });
        self.held = held;
        // Keep every displayed crossing even when the device has not consumed
        // another quantum yet. Do not run a second detector at audio cadence.
        hulls.foot_sounds(|identity, foot, count, point| {
            let Some((slot, _)) = receiver.active_slots().find(|(_, row)|
                row[r::KIND] == k::VEHICLE as f64 &&
                (row[r::ID] as u32, row[r::GENERATION] as u32) == identity) else { return; };
            let mut event = super::sound_events::placed(event_kind::HULL_FOOT, usize::MAX, point);
            event[event::OWNER] = (slot + 1) as f64;
            event[event::VALUE] = foot as f64;
            for _ in 0..count {
                if self.pending_effects.len() < self.pending_effects.capacity() {
                    self.pending_effects.push(event);
                } else { self.refused += 1; }
            }
        });
        // Preserve control state while the device is stopped or its bounded
        // queue is full. Advancing it after dropping START would orphan a loop.
        let clock = self.mail.clock.load(Ordering::Acquire);
        // At most a quarter second carried, so a stall does not jump a fade.
        let dt = (self.carried + dt).min(0.25);
        if clock == self.last_clock {
            self.carried = dt;
            return;
        }
        self.last_clock = clock;
        if !self.mail.available() {
            self.carried = dt;
            self.refused += 1;
            return;
        }
        self.carried = 0.;
        // Device time governs scheduled sample starts and stream parts.
        // Game dt still governs the control phase's frame transitions. Using
        // window wall time for both clocks makes pause/resume schedule audio
        // in a quantum the callback has already consumed.
        let wall = self.mail.clock.load(Ordering::Acquire) as f64 / 48000.;
        // Back from silence (the master volume was 0, so the device mixed
        // nothing): a fresh mix configuration, so no reverb tail or limiter
        // state from before the silence plays; every sound carries on.
        let volume = self.volumes[0].clamp(0., 1.) as f32;
        if self.silent_sent && volume > 0. {
            let (default, widths) = crate::audio::device_channels();
            let settings = self.audio.clone();
            self.configure(&settings, default, widths);
        }
        self.silent_sent = volume == 0.;
        let runtime = &mut self.runtime;
        let starts = [battle.0[2], battle.0[3]];
        if starts != self.starts {
            self.starts = starts;
            runtime.phase.set_starting_tickets(starts[0], starts[1]);
            for row in &mut self.music {
                row[music_slot::START] =
                    starts[(row[music_slot::TEAM] as usize).saturating_sub(1).min(1)];
                runtime.input[..row.len()].copy_from_slice(row);
                runtime.import(import::MUSIC_SLOT, row.len());
            }
        }
        // Upscaled samples that have decoded: the control runtime may pick them
        // from this frame on; their PCM goes to the device in this batch.
        let mut arrived = Vec::new();
        if let Some(loading) = &self.enhanced_loading {
            for loaded in loading.try_iter().take(32) {
                let input = &mut runtime.input[..enhanced_sample::STRIDE];
                input[enhanced_sample::ID] = loaded.index as f64;
                input[enhanced_sample::RENDER_ID] = loaded.render_id as f64;
                input[enhanced_sample::DURATION] = loaded.pcm.frames as f64 / loaded.pcm.rate;
                input[enhanced_sample::RATE] = loaded.pcm.rate;
                input[enhanced_sample::SCALE] = 1.;
                input[enhanced_sample::AVAILABLE] = 1.;
                runtime.import(import::ENHANCED_SAMPLE, enhanced_sample::STRIDE);
                arrived.push((loaded.render_id, Some(loaded.pcm)));
                self.enhanced_loaded += 1;
            }
        }
        runtime.clear_output();
        // GameAudio.frame contributes settings and listener facts verbatim.
        // Native control retains voice ranking, attenuation, occlusion and
        // music selection; it receives no preselected voice or panning result.
        // The stock direct-join host currently uses its default audio settings.
        let mut frame = [0.; frame::STRIDE];
        frame[frame::DT] = dt;
        frame[frame::WALL_NOW] = wall;
        frame[frame::NOW] = wall;
        frame[frame::RATE] = 48000.;
        let rendering = self.rendering;
        frame[frame::ENHANCED] = rendering.enhanced as u8 as f64;
        frame[frame::LIGHT] = rendering.light as u8 as f64;
        frame[frame::HRTF] = rendering.hrtf as u8 as f64;
        frame[frame::BINAURAL] = rendering.binaural as u8 as f64;
        frame[frame::LFE] = rendering.lfe as u8 as f64;
        frame[frame::CONFIGURATION] = self.configuration;
        // The enhanced mode's rooms and occlusion cast rays through the
        // battle's own collision (Queries).
        frame[frame::COLLISION] = 1.;
        frame[frame::RUNNING] = 1.;
        frame[frame::MAX_VOICES] = 32.;
        frame[frame::VIRTUAL_VOICES] = 64.;
        frame[frame::PX..frame::PX + 3].copy_from_slice(&listener[12..15]);
        // The camera phase has already selected the final seat/eye frame.
        // Camera forward is its negative local Z, with its published up;
        // hull bank and vertical aim therefore reach the same listener frame.
        for axis in 0..3 {
            frame[frame::FX + axis] = -listener[8 + axis];
            frame[frame::UX + axis] = listener[4 + axis];
        }
        frame[frame::TEAM] = receiver.own.team as f64;
        frame[frame::HUMAN] = 1.;
        for (at, volume) in [
            frame::EFFECTS_VOLUME,
            frame::AMBIENCE_VOLUME,
            frame::MUSIC_VOLUME,
            frame::VOICE_VOLUME,
            frame::INTERFACE_VOLUME,
        ]
        .into_iter()
        .zip(&self.volumes[1..])
        {
            frame[at] = volume.clamp(0., 1.);
        }
        frame[frame::CHANNELS] = rendering.channels as f64;
        frame[frame::TICKETS1] = battle.0[0];
        frame[frame::TICKETS2] = battle.0[1];
        frame[frame::OVER] = battle.1 as u8 as f64;
        frame[frame::BLEED1] = self.bleed[0];
        frame[frame::BLEED2] = self.bleed[1];
        // Stable receiver storage slots are native emitter ids. Wire ids and
        // generations remain facts, so sparse births cannot shift a listener.
        frame[frame::OWN] = OWN as f64;
        runtime.phase.frame.copy_from_slice(&frame);
        runtime.events.clear();
        for event in self.pending_effects.drain(..) {
            if runtime.events.len() + 1 < runtime.events.capacity() {
                runtime.events.push(event);
            } else {
                self.refused += 1;
            }
        }
        for (slot, (_, row)) in self.posts.iter().enumerate() {
            runtime.phase.posts[slot] = *row;
        }
        runtime.phase.post_count = self.posts.len();
        let mut seen = [false; OWN + 1];
        // Build the current pilot relation once. Looking through every unit
        // for every hull turns this frame into O(vehicles * actors), despite
        // the shared lease already naming both qualified endpoints.
        // Rebuilding this fixed index retains all allocated storage.
        self.pilots.clear();
        for p in receiver.active() {
            if p[r::SEAT_PRESENT] != 0. && p[r::SEAT_INDEX] == 0. {
                self.pilots.insert(
                    (
                        p[r::SEAT_VEHICLE_ID] as u32,
                        p[r::SEAT_VEHICLE_GENERATION] as u32,
                    ),
                    1,
                );
            }
        }
        if let Some((id, generation, 0)) = receiver.own.mount {
            self.pilots.insert((id, generation), 1);
        }

        // Turrets not seen this frame (their hull gone or not drawn) are silent.
        let count = runtime.phase.turret_count;
        for t in &mut runtime.phase.turrets[..count] {
            t[turret::PRESENT] = 0.;
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
            // Receiver slots are reusable storage, not audio identities.
            // Retire a previous generation before overwriting its position so
            // followed one-shots freeze at their old source. Runtime then
            // resets edge histories when this slot becomes present again.
            let life = (row[r::ID], row[r::GENERATION]);
            if self.lives[id] != Some(life) {
                retire(&mut runtime.phase, id);
                self.lives[id] = Some(life);
            }
            seen[id] = true;
            let mut emitter_row = [0.; emitter::STRIDE];
            let e = &mut emitter_row;
            e[emitter::ID] = id as f64;
            e[emitter::KIND] = if kind == k::VEHICLE {
                3.
            } else if kind == k::DROIDEKA {
                2.
            } else {
                1.
            };
            e[emitter::CLASS] = self
                .actors
                .get((row[r::CLASS_ID] as usize).wrapping_sub(1))
                .copied()
                .filter(|i| *i != usize::MAX)
                .map_or(-1., |i| i as f64);
            e[emitter::X..emitter::X + 3].copy_from_slice(&row[r::POSITION..r::POSITION + 3]);
            e[emitter::VX..emitter::VX + 3].copy_from_slice(&row[r::VELOCITY..r::VELOCITY + 3]);
            e[emitter::HEALTH] = row[r::HEALTH];
            let limits = self
                .actor_limits
                .get(row[r::CLASS_ID] as usize)
                .copied()
                .unwrap_or([300., 9., 0.]);
            e[emitter::MAX_HEALTH] = limits[0];
            e[emitter::SPEED_LIMIT] = limits[1];
            e[emitter::FLYING] = limits[2];
            e[emitter::FLIGHT] = row[r::STATE];
            e[emitter::PILOTED] = self
                .pilots
                .get(&(row[r::ID] as u32, row[r::GENERATION] as u32))
                .is_some() as u8 as f64;
            e[emitter::ALIVE] = (row[r::DEAD] == 0.) as u8 as f64;
            e[emitter::GENERATION] = row[r::GENERATION];
            e[emitter::AIR] = row[r::STATE];
            e[emitter::POSTURE] = row[r::POSTURE];
            let unit = (row[r::ID] as u32, row[r::GENERATION] as u32);
            // Its turret seats (VehicleSounds.updateTurrets), the hull's class
            // sounding them; PitchRate is taken as the default 1 rad/s.
            for &(_, turret_id, held, yaw, pitch, top, point) in self.turret_seats.iter().filter(|t| t.0 == unit) {
                if turret_id >= runtime.phase.turrets.len() {
                    continue;
                }
                let t = &mut runtime.phase.turrets[turret_id];
                let fresh = t[turret::PRESENT] == 0. && t[turret::OWNER] != id as f64;
                t[turret::ID] = turret_id as f64;
                t[turret::CLASS] = e[emitter::CLASS];
                t[turret::OWNER] = id as f64;
                t[turret::X..turret::X + 3].copy_from_slice(&point);
                t[turret::HELD] = held as u8 as f64;
                t[turret::ALIVE] = e[emitter::ALIVE];
                t[turret::YAW] = yaw;
                t[turret::PITCH] = pitch;
                t[turret::YAW_TOP] = top;
                t[turret::PITCH_TOP] = 1.;
                t[turret::PRESENT] = 1.;
                if fresh {
                    runtime.phase.turret_state[turret_id] = Default::default();
                }
                runtime.phase.turret_count = runtime.phase.turret_count.max(turret_id + 1);
            }
            if let Some(event) =
                super::sound_events::damage(&runtime.phase.emitters[id], &emitter_row, false)
            {
                if runtime.events.len() + 1 < runtime.events.capacity() {
                    runtime.events.push(event);
                } else {
                    self.refused += 1;
                }
            }
            runtime.phase.observe_emitter(&emitter_row);
            let class = self
                .weapons
                .get((row[r::WEAPON] as usize).wrapping_sub(1))
                .copied()
                .filter(|i| *i != usize::MAX)
                .map_or(-1., |i| i as f64);
            let mut weapon_row = [0.; weapon::STRIDE];
            let w = &mut weapon_row;
            w[weapon::ID] = id as f64;
            w[weapon::OWNER] = id as f64;
            w[weapon::CLASS] = class;
            w[weapon::SELECTED] = 1.;
            runtime.phase.observe_weapon(&weapon_row);
        }
        // Own bodies need not be replicated back to their player. The own
        // checkpoint supplies that emitter; reliable shot records supply fire.
        let life = (receiver.own.id, receiver.own.generation);
        if self.lives[OWN] != Some(life) {
            retire(&mut runtime.phase, OWN);
            self.lives[OWN] = Some(life);
        }
        seen[OWN] = true;
        let mut own_row = [0.; emitter::STRIDE];
        let own = &mut own_row;
        own[emitter::ID] = OWN as f64;
        // A droideka's own sounds (legs, roll, shield) are PlayerSounds.updateDroideka's.
        own[emitter::KIND] = if receiver.own.walker { 2. } else { 1. };
        own[emitter::CLASS] = self
            .actors
            .get(receiver.own.class.wrapping_sub(1))
            .copied()
            .filter(|i| *i != usize::MAX)
            .map_or(-1., |i| i as f64);
        own[emitter::X..emitter::X + 3].copy_from_slice(&prediction.body.as_ref().map_or_else(
            || {
                prediction
                    .fleet
                    .state
                    .as_ref()
                    .map_or(receiver.own.position, |h| h.position)
            },
            |b| b.position,
        ));
        own[emitter::ALIVE] = receiver.own.alive as u8 as f64;
        own[emitter::GENERATION] = receiver.own.generation;
        own[emitter::HEALTH] = receiver.own.health;
        own[emitter::MAX_HEALTH] = self
            .actor_limits
            .get(receiver.own.class)
            .map_or(300., |v| v[0]);
        own[emitter::MOUNTED] = receiver.own.mount.is_some() as u8 as f64;
        // PlayerSounds.updateMoves and updateFeet: the body's state, as
        // predicted when it is. Air as the player sounds know it: 1 jumping,
        // 2 thrown down, 3 on a jetpack, 4 falling.
        let air = |code: u8| [0., 1., 4., 2., 3.].get(code as usize).copied().unwrap_or(0.);
        if let Some(body) = prediction.body.as_ref() {
            own[emitter::VX..emitter::VX + 3].copy_from_slice(&body.velocity);
            own[emitter::AIR] = air(body.air.map_or(0, |a| a as u8 + 1));
            own[emitter::POSTURE] = body.posture as u8 as f64;
            own[emitter::DIVE] = (body.dive != 0.) as u8 as f64;
            own[emitter::GROUNDED] = body.grounded as u8 as f64;
        } else {
            own[emitter::VX..emitter::VX + 3].copy_from_slice(&receiver.own.velocity);
            own[emitter::AIR] = air(receiver.own.air);
            own[emitter::POSTURE] = receiver.own.posture as f64;
            own[emitter::DIVE] = (receiver.own.dive != 0.) as u8 as f64;
            own[emitter::GROUNDED] = (receiver.own.air == 0) as u8 as f64;
        }
        // As world heights over the emitter's own origin (the drawn body's
        // bones measured from the drawn body, so smoothing cannot shift them).
        for (k, foot) in self.own_feet.iter().enumerate() {
            own[emitter::FOOT0 + k] = own[emitter::Y] + foot;
        }
        own[emitter::MODE] = receiver.own.walker_mode as f64;
        // The shield going up plays its weapon's FireSound: the class of the
        // droideka's second-trigger weapon, while the shield is on.
        own[emitter::SHIELD] = if receiver.own.walker && receiver.own.shield_on {
            self.prediction_weapons.get(self.own_weapons[1].saturating_sub(1) as usize).map_or(0., |(row, _)| row[weapon::CLASS])
        } else {
            0.
        };
        own[emitter::ZOOM_STEP] = receiver.own.zoom_step as f64;
        own[emitter::ZOOM] = receiver.own.zoom.max(1.);
        own[emitter::FIRST_PERSON] = 1.;
        if let Some(event) =
            super::sound_events::damage(&runtime.phase.emitters[OWN], &own_row, true)
        {
            if runtime.events.len() + 1 < runtime.events.capacity() {
                runtime.events.push(event);
            } else {
                self.refused += 1;
            }
        }
        runtime.phase.observe_emitter(&own_row);
        if let Some(history) = &receiver.space {
            self.sync_space(history, receiver.clock.shown);
        }
        if receiver.hero_audio.is_some() {
            self.sync_hero_audio(receiver);
        }
        let runtime = &mut self.runtime;
        // SceneSounds.updateStations: every supply station is an emitter the
        // shared scene keeps humming with its class AmbientSound while it
        // stands, and silences when it is destroyed. They sit past the
        // bodies' slots and never change place.
        for (index, (class, position, station)) in self.stations.iter().enumerate() {
            if !station || *class == usize::MAX {
                continue;
            }
            let mut row = [0.; emitter::STRIDE];
            row[emitter::ID] = (OWN + 1 + index) as f64;
            row[emitter::KIND] = audio_control::layout::emitter_kind::STATION as f64;
            row[emitter::CLASS] = *class as f64;
            row[emitter::X..emitter::X + 3].copy_from_slice(position);
            row[emitter::ALIVE] = self.built[index] as u8 as f64;
            runtime.phase.observe_emitter(&row);
        }
        for (id, present) in seen.into_iter().enumerate().skip(1) {
            if !present && self.lives[id].take().is_some() {
                retire(&mut runtime.phase, id);
            }
        }
        // captureWeapon observes the completed prediction result. It must not
        // run weapon ticking again to obtain reload or heat facts. Definition
        // metadata was admitted in the same id order as ClientPrediction;
        // only selected channel state is needed by PlayerSounds here.
        let mut counts = [0usize; 2];
        let mut selected_present = [false; 2];
        let selected = prediction.selected();
        runtime.phase.emitters[OWN][emitter::SELECTED0] = -1.;
        runtime.phase.emitters[OWN][emitter::SELECTED1] = -1.;
        for state in prediction.weapons() {
            let Some((metadata, channel)) = self
                .prediction_weapons
                .get(state.def.0.saturating_sub(1) as usize)
            else {
                continue;
            };
            let channel = (*channel).min(1);
            let ordinal = counts[channel];
            counts[channel] += 1;
            if ordinal as f64 != selected[channel] {
                continue;
            }
            let id = OWN + channel;
            selected_present[channel] = true;
            // These are selected-weapon slots, not permanent weapon objects.
            // Retire the previous binding before a new gun takes its edge
            // history, including when a seat weapon replaces an infantry gun.
            if self.own_weapons[channel] != state.def.0 {
                retire_weapon(&mut runtime.phase, id);
                self.own_weapons[channel] = state.def.0;
            }
            let mut row = *metadata;
            row[weapon::ID] = id as f64;
            row[weapon::OWNER] = OWN as f64;
            row[weapon::SELECTED] = (1 << channel) as f64;
            row[weapon::ORDINAL] = ordinal as f64;
            row[weapon::RELOADING] = (state.reload_left > 0.) as u8 as f64;
            row[weapon::OVERHEATED] = state.overheated as u8 as f64;
            row[weapon::CHARGING] = state.charging as u8 as f64;
            row[weapon::FIRING] = state.firing as u8 as f64;
            row[weapon::HEAT] = if state.overheated { state.heat } else { 0. };
            row[weapon::HELD] = if state.charging { state.held } else { 0. };
            row[weapon::WAIT] = state.wait;
            row[weapon::EMPTY] = (!state.bottomless && state.clip <= 0.) as u8 as f64;
            runtime.phase.observe_weapon(&row);
            // Which weapon the channel holds (the arsenal's selection, as
            // PlayerSounds compares it), so a switch plays WeaponChangeSound.
            runtime.phase.emitters[OWN][[emitter::SELECTED0, emitter::SELECTED1][channel]] =
                ordinal as f64;
        }
        for (channel, present) in selected_present.into_iter().enumerate() {
            if !present && self.own_weapons[channel] != 0 {
                retire_weapon(&mut runtime.phase, OWN + channel);
                self.own_weapons[channel] = 0;
            }
        }
        // Actor fire is an authoritative presentation event, independent of
        // the body's update rate or whether the server replicates its owner
        // back to itself. The journal already suppresses reliable/datagram
        // repeats, so each fire event reaches native sound selection once.
        for source in self.pending.drain(..) {
            let owner = if source.owner_id as f64 == receiver.own.id
                && source.owner_generation as f64 == receiver.own.generation
            {
                Some(OWN)
            } else {
                receiver
                    .active_slots()
                    .find(|(_, row)| {
                        row[r::ID] == source.owner_id as f64
                            && row[r::GENERATION] == source.owner_generation as f64
                    })
                    .map(|(slot, _)| slot + 1)
            };
            let Some(owner) = owner else {
                continue;
            };
            let class = self
                .weapons
                .get(source.weapon_id.wrapping_sub(1))
                .copied()
                .filter(|i| *i != usize::MAX)
                .map_or(-1., |i| i as f64);
            let weapon_id = if owner == OWN {
                owner + source.channel.min(1)
            } else {
                owner
            };
            let weapon = &mut runtime.phase.weapons[weapon_id];
            if owner != OWN {
                weapon.fill(0.);
            }
            weapon[weapon::ID] = weapon_id as f64;
            weapon[weapon::OWNER] = owner as f64;
            weapon[weapon::CLASS] = class;
            weapon[weapon::SELECTED] = 1.;
            runtime.phase.weapon_count = runtime.phase.weapon_count.max(weapon_id + 1);
            if let Some(position) = source.position {
                runtime.phase.emitters[owner][emitter::X..emitter::X + 3]
                    .copy_from_slice(&position);
            }
            let mut event = [0.; event::STRIDE];
            event[event::KIND] = if owner == OWN {
                event_kind::FIRE
            } else {
                event_kind::ACTOR_FIRE
            } as f64;
            event[event::OWNER] = owner as f64;
            event[event::WEAPON] = weapon_id as f64;
            event[event::CLASS] = class;
            event[event::VALUE] = source.repair as u8 as f64;
            if runtime.events.len() + 1 < runtime.events.capacity() {
                runtime.events.push(event);
            }
        }
        if !runtime.phase.started {
            let mut start = [0.; event::STRIDE];
            start[event::KIND] = event_kind::START as f64;
            runtime.events.insert(0, start);
        }
        // Diagnostics count the actual events delivered to the shared runtime,
        // including suppressed/inaudible effects; playback gates stay there.
        for event in &runtime.events {
            if let Some(count) = self.event_counts.get_mut(event[event::KIND] as usize) {
                *count += 1;
            }
            if event[event::KIND] == event_kind::HULL_FOOT as f64 {
                self.foot_counts[(event[event::VALUE] == 6.) as usize] += 1;
            }
        }
        // Classic's reverb: the listener's space (Sound::listener_space).
        // For 2005 battles, `step_swbf2` resolves box/cylinder/sphere spaces directly.
        if runtime.phase.family() != bfe_family::GameFamily::Swbf2 {
            let p = &runtime.phase.frame;
            let ear = [p[frame::PX] + 2. * p[frame::FX], p[frame::PY] + 2. * p[frame::FY], p[frame::PZ] + 2. * p[frame::FZ]];
            let space = listener_space(&self.space_boxes, self.space, ear);
            if space != self.space || !self.space_set {
                self.space = space;
                self.space_set = true;
                let (send, row) = space.map_or(self.dry, |id| self.spaces[id]);
                let phase = &mut runtime.phase;
                phase.space_send = send;
                phase.original_room = audio_control::acoustics::Room {
                    room: row[room::ROOM], room_hf: row[room::ROOM_HF], decay: row[room::DECAY], hf_ratio: row[room::HF_RATIO],
                    reflections: row[room::REFLECTIONS], refl_delay: row[room::REFL_DELAY], reverb: row[room::REVERB],
                    reverb_delay: row[room::REVERB_DELAY], diffusion: row[room::DIFFUSION], density: row[room::DENSITY],
                    hf_reference: row[room::HF_REFERENCE],
                };
                phase.original_room_mask = row[room::MASK] as u32;
                if !phase.engine.enhanced {
                    phase.room_pending |= phase.original_room_mask;
                }
            }
        }
        self.assets.update_durations(&mut runtime.phase.streams);
        runtime.step(&mut self.queries);
        // For a battle's summary: the supply stations humming after this step.
        self.station_loops = self
            .stations
            .iter()
            .enumerate()
            .filter(|(index, (_, _, station))| *station && runtime.phase.playing(runtime.phase.entity_state[OWN + 1 + index].primary))
            .count() as u64;
        // Publish one complete command boundary after control selection.
        // Reservations cover all 512 voices plus bounded stream readiness;
        // placement records keep the shared six-word mixer format.
        // The SPSC release store below is the sole transfer to the device.
        let commands = &runtime.phase.engine.commands;
        let placements = &runtime.phase.engine.placements;
        if commands.len() > COMMANDS || placements.len() > PLACEMENTS {
            self.refused += 1;
            return;
        }
        let write = self.mail.write.load(Ordering::Relaxed);
        let batch = unsafe { &mut *self.mail.slots[write % QUEUE].get() };
        drop(batch.retired.take());
        batch.freed.clear();
        let batch = unsafe { &mut *self.mail.slots[write % QUEUE].get() };
        batch.config = self.pending_config.take();
        batch.samples.clear();
        batch.samples.append(&mut arrived);
        batch.commands.clear();
        batch.commands.extend_from_slice(commands);
        batch.placements.clear();
        batch.placements.extend_from_slice(placements);
        batch.mask = runtime.info[info::ROOM_CHANGED] as u32;
        batch.volume = self.volumes[0].clamp(0., 1.) as f32;
        for (i, v) in batch.room.iter_mut().enumerate() {
            *v = runtime.info[info::ROOM + i];
            if batch.mask & (1 << i) != 0 {
                self.room[i] = *v;
            }
        }
        self.room_mask |= batch.mask;
        // Asset requests carry the runtime's generation. PCM is moved through
        // this same mailbox before its START commands; stale decode results
        // cannot replace a sample belonging to a newer use of the stream slot.
        submit_requests(runtime, &self.assets, batch, &mut self.refused);
        for _ in 0..128 {
            // Completions that no longer fit wait for the next frame.
            if !batch.room_for_ready() {
                break;
            }
            let Some(response) = self.assets.receive() else {
                break;
            };
            let request = response.request;
            let pcm = response.pcm;
            // Stream banks identify their own VO sources. Count successful
            // decode completions, rather than assuming a queued cue played.
            if pcm.is_some()
                && self
                    .voice_segments
                    .get(request.segment)
                    .copied()
                    .unwrap_or(false)
            {
                self.voice_decoded += 1;
            }
            let input = &mut runtime.input[..ready::STRIDE];
            input.fill(0.);
            input[ready::VOICE] = request.voice as f64;
            input[ready::GENERATION] = request.generation as f64;
            input[ready::PART] = request.part as f64;
            input[ready::SAMPLE] = request.sample as f64;
            input[ready::TIME] = wall;
            input[ready::WALL_NOW] = wall;
            if let Some(pcm) = &pcm {
                input[ready::DURATION] = pcm.frames as f64 / pcm.rate;
                input[ready::CHANNELS] = pcm.channels as f64;
            } else {
                input[ready::FAILED] = 1.;
            }
            runtime.ready();
            debug_assert!(runtime.phase.engine.commands.len() <= READY_WORDS && runtime.phase.engine.placements.is_empty());
            if runtime.info[info::ACCEPTED] != 0. {
                if pcm.is_some() {
                    batch.samples.push((request.sample, pcm));
                    self.decoded += 1;
                }
                batch
                    .commands
                    .extend_from_slice(&runtime.phase.engine.commands);
                batch
                    .placements
                    .extend_from_slice(&runtime.phase.engine.placements);
            }
            submit_requests(runtime, &self.assets, batch, &mut self.refused);
        }
        batch.freed.reserve(batch.samples.len());
        self.mail
            .write
            .store(write.wrapping_add(1), Ordering::Release);
        self.batches += 1;
    }

    /// Feeds 2005 space battle facts (all fighters, turrets, weapons, hits,
    /// expiries, destructions, doors, and flying ordnance) into the audio
    /// control runtime with zero heap allocations.
    pub(crate) fn sync_space(&mut self, history: &crate::local::space::History, shown: f64) {
        if self.runtime.phase.family() != bfe_family::GameFamily::Swbf2 {
            return;
        }
        use audio_control::swbf2::vehicles::VehicleEvent;
        use audio_control::swbf2::weapons::{
            SpaceAudioEvent, SpaceOrdnanceInput, SpaceWeaponInput,
        };
        use bfe_sim::space::flight::SpaceTrick;

        let hull_base = OWN + 1 + self.stations.len();
        let ord_base = hull_base + MAX_SPACE_HULLS;
        let space_turret_base = self
            .turret_seats
            .iter()
            .map(|t| t.1 + 1)
            .max()
            .unwrap_or(0);
        let runtime = &mut self.runtime;

        let Some((frame, alpha)) = history.sample(shown) else {
            return;
        };

        let mut hull_fired = [0u8; MAX_SPACE_HULLS];
        let mut hull_changed = [0u8; MAX_SPACE_HULLS];
        let mut turret_fired = [false; MAX_SPACE_TURRETS];

        // Drain every fixed tick elapsed since the previous audio step so no
        // single-tick shot, hit, expiry, trick, weapon switch, death, or door
        // transition is missed or replayed across render cadences.
        for new in history.after(self.space_tick, frame.tick) {
            for hull in &new.hulls {
                if hull.id >= MAX_SPACE_HULLS {
                    continue;
                }
                let count = usize::from(hull.weapon_count).min(5);
                for wi in 0..count {
                    if hull.weapons[wi].fired {
                        hull_fired[hull.id] |= 1u8 << wi;
                    }
                }
                let prev_trick = self.space_hull_tricks[hull.id];
                if prev_trick == SpaceTrick::None && hull.trick != SpaceTrick::None {
                    let owner = hull_base + hull.id;
                    let ev = match hull.trick {
                        SpaceTrick::BarrelLeft | SpaceTrick::BarrelRight => {
                            VehicleEvent::Trick { owner, class_hash: hull.class }
                        }
                        SpaceTrick::Loop | SpaceTrick::Immelmann => {
                            VehicleEvent::Flip { owner, class_hash: hull.class }
                        }
                        SpaceTrick::None => unreachable!(),
                    };
                    runtime.phase.push_vehicle_event(ev);
                }
                self.space_hull_tricks[hull.id] = hull.trick;
            }
            for ch in 0..2 {
                if new.weapon_changed[ch] == Some(new.tick) {
                    if let Some(cockpit) = new.cockpit {
                        if let Some(w) = cockpit.weapons[ch] {
                            if cockpit.slot < MAX_SPACE_HULLS && w.index < 5 {
                                hull_changed[cockpit.slot] |= 1u8 << w.index;
                            }
                        }
                    }
                }
            }
            for (ti, aim) in new.turrets.iter().enumerate().take(MAX_SPACE_TURRETS) {
                if aim.fired {
                    turret_fired[ti] = true;
                }
            }
            for (_, r) in &new.rounds {
                if r.contact.is_some() {
                    runtime.phase.push_space_audio_event(SpaceAudioEvent::RoundHit {
                        weapon_hash: r.weapon,
                        position: r.position,
                    });
                } else if !r.active {
                    runtime.phase.push_space_audio_event(SpaceAudioEvent::RoundExpired {
                        weapon_hash: r.weapon,
                        position: r.position,
                    });
                }
            }
            for death in &new.deaths {
                runtime.phase.push_space_audio_event(SpaceAudioEvent::Destruction {
                    class_hash: death.class,
                    position: death.position,
                });
                if death.name != 0 {
                    runtime.phase.set_building_alive_swbf2(death.name, false);
                }
            }
            for &name_hash in &new.respawns {
                runtime.phase.set_building_alive_swbf2(name_hash, true);
            }
            for &door_ev in &new.door_events {
                runtime.phase.push_door_event(door_ev);
            }
            self.space_tick = Some(new.tick);
        }

        // 1. All live space hulls (player and bots): engines, boost cues, tricks, weapons.
        let mut hull_seen = [false; MAX_SPACE_HULLS];
        for hull in &frame.hulls {
            if hull.id >= MAX_SPACE_HULLS {
                continue;
            }
            let eid = hull_base + hull.id;
            if eid >= audio_control::phase::RECORDS {
                continue;
            }
            hull_seen[hull.id] = true;
            if self.space_hull_lives[hull.id] != Some(hull.generation) {
                if self.space_hull_lives[hull.id].is_some() {
                    retire_space_hull(&mut runtime.phase, eid);
                }
                self.space_hull_lives[hull.id] = Some(hull.generation);
                self.space_hull_tricks[hull.id] = hull.trick;
            }
            let mut row = [0.; emitter::STRIDE];
            row[emitter::ID] = eid as f64;
            row[emitter::KIND] = 3.;
            row[emitter::CLASS] = runtime
                .phase
                .swbf2_class_by_hash(hull.class)
                .map_or(-1., |c| c as f64);
            row[emitter::ALIVE] = 1.;
            row[emitter::GENERATION] = f64::from(hull.generation);
            row[emitter::FLYING] = 1.;
            row[emitter::PILOTED] = hull.piloted as u8 as f64;
            row[emitter::HEALTH] = hull.health;
            row[emitter::MAX_HEALTH] = hull.max_health;
            row[emitter::FLIGHT] = f64::from(hull.state as u8 + 1);
            row[emitter::SPEED_LIMIT] = hull.max_speed;
            for i in 0..3 {
                row[emitter::X + i] = hull.previous.position[i]
                    + alpha * (hull.current.position[i] - hull.previous.position[i]);
                row[emitter::VX + i] = hull.velocity[i];
            }
            runtime.phase.observe_emitter(&row);
        }
        for hid in 0..MAX_SPACE_HULLS {
            if !hull_seen[hid] && self.space_hull_lives[hid].take().is_some() {
                retire_space_hull(&mut runtime.phase, hull_base + hid);
                self.space_hull_tricks[hid] = SpaceTrick::None;
            }
        }

        // 2. All space turrets (remote chair guns, cruiser/frigate/hangar auto-turrets).
        for (ti, aim) in frame.turrets.iter().enumerate().take(MAX_SPACE_TURRETS) {
            let turret_id = space_turret_base + ti;
            if turret_id >= runtime.phase.turrets.len() {
                break;
            }
            let turret_class = runtime
                .phase
                .swbf2_class_by_hash(aim.class)
                .map_or(-1., |c| c as f64);
            let t = &mut runtime.phase.turrets[turret_id];
            t[turret::ID] = turret_id as f64;
            t[turret::CLASS] = turret_class;
            t[turret::OWNER] = -1.;
            t[turret::X..turret::X + 3].copy_from_slice(&aim.position);
            t[turret::HELD] = aim.held as u8 as f64;
            t[turret::ALIVE] = aim.alive as u8 as f64;
            let yaw_step = aim.current[0] - aim.previous[0];
            let pitch_step = aim.current[1] - aim.previous[1];
            t[turret::YAW] = aim.previous[0] + yaw_step.sin().atan2(yaw_step.cos()) * alpha;
            t[turret::PITCH] = aim.previous[1] + pitch_step.sin().atan2(pitch_step.cos()) * alpha;
            t[turret::YAW_TOP] = f64::from(aim.yaw_rate).max(0.1);
            t[turret::PITCH_TOP] = f64::from(aim.pitch_rate).max(0.1);
            t[turret::PRESENT] = 1.;
            runtime.phase.turret_count = runtime.phase.turret_count.max(turret_id + 1);
        }

        // 3. Push space weapons (active first so busy weapons never overflow the bank).
        for active_pass in [true, false] {
            for hull in &frame.hulls {
                if hull.id >= MAX_SPACE_HULLS {
                    continue;
                }
                let eid = hull_base + hull.id;
                if eid >= audio_control::phase::RECORDS {
                    continue;
                }
                let count = usize::from(hull.weapon_count).min(5);
                for wi in 0..count {
                    let w = &hull.weapons[wi];
                    if w.class == 0 {
                        continue;
                    }
                    let fired = (hull_fired[hull.id] & (1u8 << wi)) != 0;
                    let changed = (hull_changed[hull.id] & (1u8 << wi)) != 0;
                    let active =
                        fired || changed || w.firing || w.spinning || w.overheated || w.reloading;
                    if active != active_pass || (!active && !hull.piloted) {
                        continue;
                    }
                    runtime.phase.push_space_weapon(SpaceWeaponInput {
                        key: (hull.id << 3) | wi,
                        owner: eid,
                        class_hash: w.class,
                        fired,
                        firing: w.firing,
                        spinning: w.spinning,
                        overheated: w.overheated,
                        heat: f64::from(w.heat),
                        heat_threshold: f64::from(w.heat_threshold),
                        reloading: w.reloading,
                        changed,
                        own: frame.mount == Some(hull.id),
                    });
                }
            }
            for (ti, aim) in frame.turrets.iter().enumerate().take(MAX_SPACE_TURRETS) {
                let turret_id = space_turret_base + ti;
                if turret_id >= runtime.phase.turrets.len() || !aim.alive || aim.weapon == 0 {
                    continue;
                }
                let fired = turret_fired[ti];
                let active = fired || aim.firing || aim.spinning || aim.overheated;
                if active != active_pass {
                    continue;
                }
                runtime.phase.push_space_weapon(SpaceWeaponInput {
                    key: (1 << 16) | ti,
                    owner: runtime.phase.turret_emitter_base + turret_id,
                    class_hash: aim.weapon,
                    fired,
                    firing: aim.firing,
                    spinning: aim.spinning,
                    overheated: aim.overheated,
                    heat: f64::from(aim.heat),
                    heat_threshold: f64::from(aim.heat_threshold),
                    reloading: false,
                    changed: false,
                    own: frame.chair.is_some_and(|c| c.building == aim.building),
                });
            }
        }

        // 4. Bounded nearest-in-range flying ordnance/rocket loops.
        self.space_ordnance_scratch.clear();
        for (_, r) in &frame.rounds {
            if !r.active || r.serial == 0 {
                continue;
            }
            if self.space_ordnance_scratch.len() == self.space_ordnance_scratch.capacity() {
                break;
            }
            let position =
                std::array::from_fn(|i| r.previous[i] + alpha * (r.position[i] - r.previous[i]));
            self.space_ordnance_scratch.push(SpaceOrdnanceInput {
                serial: r.serial,
                weapon_hash: r.weapon,
                position,
                velocity: r.velocity,
            });
        }
        runtime
            .phase
            .queue_space_ordnance(ord_base, &self.space_ordnance_scratch);
    }

    /// Feeds 2005 hero powers, continuous choke holds, thrown lightsabers, and
    /// player + bot lightsaber states into the audio runtime with zero heap
    /// allocations.
    pub(crate) fn sync_hero_audio(&mut self, receiver: &Receiver) {
        if self.runtime.phase.family() != bfe_family::GameFamily::Swbf2 {
            return;
        }
        let Some(snap) = receiver.hero_audio.as_deref() else {
            return;
        };
        use audio_control::engine::NONE;
        use audio_control::swbf2::powers::{
            PowerAudioEvent, PowerAudioKind, PowerHoldInput, SaberThrowInput, MAX_POWER_HOLDS,
        };
        use audio_control::swbf2::saber::SaberEvent;
        use bfe_sim::combat::powers::PowerEventKind;

        let space_extra = if receiver.space.is_some() {
            MAX_SPACE_HULLS + audio_control::swbf2::weapons::MAX_SPACE_ORDNANCE_LOOPS
        } else {
            0
        };
        let hold_base = OWN + 1 + self.stations.len() + space_extra;
        let throw_base = hold_base + MAX_POWER_HOLDS;

        // 1. Discrete hero power events (Push, Pull, Lightning, Auras, Combo Blasts, Blocks).
        for ev in snap.power_events_since(self.hero_power_seq) {
            self.hero_power_seq = ev.sequence;
            let emitter = resolve_hero_emitter(receiver, ev.caster_slot, ev.caster_generation)
                .unwrap_or(NONE);
            let kind = match ev.kind {
                PowerEventKind::Cast => PowerAudioKind::Cast,
                PowerEventKind::Hit => PowerAudioKind::Hit,
                PowerEventKind::Thrown => PowerAudioKind::Thrown,
                PowerEventKind::Blocked => PowerAudioKind::Blocked,
                PowerEventKind::Held => PowerAudioKind::Held,
                PowerEventKind::Buffed => PowerAudioKind::Buffed,
            };
            let caster_key = (u64::from(ev.caster_slot) << 32) | u64::from(ev.caster_generation);
            self.runtime.phase.push_power_event(PowerAudioEvent {
                caster_key,
                emitter,
                weapon_hash: ev.weapon,
                target_weapon_hash: ev.target_weapon,
                kind,
                caster_pos: ev.caster_pos,
                target_pos: ev.target_pos,
            });
        }

        // 2. Continuous area-power holds (Force Choke).
        self.hero_hold_scratch.clear();
        for hold in snap.holds() {
            if self.hero_hold_scratch.len() == self.hero_hold_scratch.capacity() {
                break;
            }
            let emitter = resolve_hero_emitter(receiver, hold.caster_slot, hold.caster_generation)
                .unwrap_or(NONE);
            self.hero_hold_scratch.push(PowerHoldInput {
                key: (u64::from(hold.caster_slot) << 32) | u64::from(hold.caster_generation),
                emitter,
                weapon_hash: hold.weapon,
                caster_pos: hold.caster_pos,
                target_pos: hold.target_pos,
            });
        }
        self.runtime
            .phase
            .queue_power_holds(hold_base, &self.hero_hold_scratch);

        // 3. In-flight thrown lightsabers (launch + moving twirl loop).
        self.hero_throw_scratch.clear();
        for tr in snap.throws() {
            if self.hero_throw_scratch.len() == self.hero_throw_scratch.capacity() {
                break;
            }
            let owner = resolve_hero_emitter(receiver, tr.owner_slot, tr.owner_generation)
                .unwrap_or(NONE);
            self.hero_throw_scratch.push(SaberThrowInput {
                key: (u64::from(tr.owner_slot) << 32) | u64::from(tr.launched),
                owner,
                weapon_hash: tr.weapon,
                position: tr.position,
                velocity: tr.velocity,
            });
        }
        self.runtime
            .phase
            .queue_saber_throws(throw_base, &self.hero_throw_scratch);

        // 4. Player and bot lightsabers (ignite, hum, combo swings, throw pause, extinguish).
        let mut seen = [false; MAX_HERO_SABERS];
        for s in snap.sabers() {
            let Some(emitter) = resolve_hero_emitter(receiver, s.owner_slot, s.owner_generation)
            else {
                continue;
            };
            let existing = self.hero_saber_tracks.iter().position(|t| {
                t.is_some_and(|tr| tr.slot == s.owner_slot && tr.generation == s.owner_generation)
            });
            let idx = existing.or_else(|| self.hero_saber_tracks.iter().position(Option::is_none));
            let Some(idx) = idx else { continue };
            seen[idx] = true;
            let active = s.blades_off != u8::MAX;
            let next = HeroSaberTrack {
                emitter,
                slot: s.owner_slot,
                generation: s.owner_generation,
                weapon_hash: s.weapon_hash,
                state_hash: s.state_hash,
                blades_off: s.blades_off,
                thrown: s.thrown,
            };
            match self.hero_saber_tracks[idx] {
                None => {
                    if active {
                        self.runtime.phase.push_saber_event(SaberEvent::SetBladeActive {
                            owner: emitter,
                            weapon_class_hash: s.weapon_hash,
                            active: true,
                        });
                    }
                    if s.thrown {
                        self.runtime.phase.push_saber_event(SaberEvent::SetThrown {
                            owner: emitter,
                            thrown: true,
                        });
                    }
                    self.runtime.phase.push_saber_event(SaberEvent::ComboState {
                        owner: emitter,
                        combo_hash: s.combo_hash,
                        state_hash: s.state_hash,
                    });
                    self.hero_saber_tracks[idx] = Some(next);
                }
                Some(prev) => {
                    if prev.blades_off != s.blades_off || prev.weapon_hash != s.weapon_hash {
                        self.runtime.phase.push_saber_event(SaberEvent::SetBladeActive {
                            owner: emitter,
                            weapon_class_hash: s.weapon_hash,
                            active,
                        });
                    }
                    if prev.thrown != s.thrown {
                        self.runtime.phase.push_saber_event(SaberEvent::SetThrown {
                            owner: emitter,
                            thrown: s.thrown,
                        });
                    }
                    if prev.state_hash != s.state_hash {
                        self.runtime.phase.push_saber_event(SaberEvent::ComboState {
                            owner: emitter,
                            combo_hash: s.combo_hash,
                            state_hash: s.state_hash,
                        });
                    }
                    self.hero_saber_tracks[idx] = Some(next);
                }
            }
        }
        for (idx, present) in seen.into_iter().enumerate() {
            if !present {
                if let Some(prev) = self.hero_saber_tracks[idx].take() {
                    self.runtime.phase.push_saber_event(SaberEvent::Extinguish {
                        owner: prev.emitter,
                        weapon_class_hash: prev.weapon_hash,
                    });
                }
            }
        }
    }
}

fn resolve_hero_emitter(receiver: &Receiver, slot: u32, generation: u32) -> Option<usize> {
    if receiver.own.alive
        && receiver.own.id == f64::from(slot)
        && receiver.own.generation == f64::from(generation)
    {
        return Some(OWN);
    }
    receiver
        .active_slots()
        .find(|(_, row)| row[r::ID] == f64::from(slot) && row[r::GENERATION] == f64::from(generation))
        .map(|(rslot, _)| rslot + 1)
}

/// The space a listener at `ear` takes (docs/formats.md, "Sound"): the one
/// he has while still inside one of its boxes, else the earliest box (in load
/// order) holding him, else none (the dry default).
fn listener_space(boxes: &[super::sound_data::SpaceBox], current: Option<usize>, ear: [f64; 3]) -> Option<usize> {
    if let Some(space) = current {
        if boxes.iter().any(|b| b.space == space && b.holds(ear)) {
            return current;
        }
    }
    boxes.iter().find(|b| b.holds(ear)).map(|b| b.space)
}

// Disappearance freezes followed one-shots at their last published point,
// detaches layers and rearms the shared owner's generation edge handling.
fn retire(phase: &mut audio_control::phase::Phase, id: usize) {
    let mut emitter = phase.emitters[id];
    emitter[emitter::ID] = id as f64;
    emitter[emitter::KIND] = 0.;
    phase.observe_emitter(&emitter);
    retire_weapon(phase, id);
    if id == OWN {
        retire_weapon(phase, OWN + 1);
    }
}

// Retiring a space hull stops its engine loop cleanly without re-firing `vehicles.rs`
// `DeathSound`, because `Frame::deaths` plays `play_space_destruction` at the death point.
fn retire_space_hull(phase: &mut audio_control::phase::Phase, id: usize) {
    if id >= audio_control::phase::RECORDS {
        return;
    }
    let primary = phase.entity_state[id].primary;
    if primary != audio_control::engine::SILENT {
        phase.stop(primary, 0.08);
        phase.entity_state[id].primary = audio_control::engine::SILENT;
        phase.entity_state[id].on = false;
        phase.entity_state[id].turning_off = false;
    }
    let mut emitter = phase.emitters[id];
    emitter[emitter::ID] = id as f64;
    emitter[emitter::KIND] = 0.;
    phase.observe_emitter(&emitter);
}

// A ready segment can request its successor before another game frame. Drain
// requests at both boundaries, preserving the runtime's prefetch generation.
// Dropping those requests when the next step clears output would stall music
// after its first decoded part even though the device continues to render.
fn submit_requests(runtime: &mut Runtime, assets: &super::stream_assets::Assets, batch: &mut Batch, refused: &mut u64) {
    let mut requests = [None; 128];
    for (slot, request) in requests.iter_mut().zip(&runtime.phase.streams.requests) {
        *slot = Some(*request);
    }
    for request in requests.into_iter().flatten() {
        if request.operation == request_kind::LOAD {
            if !assets.request(request) {
                runtime.refuse_stream(request);
                *refused += 1;
            }
        } else if request.operation == request_kind::RELEASE {
            for id in request.sample..request.sample + 3 {
                if batch.samples.len() < batch.samples.capacity() {
                    batch.samples.push((id, None));
                } else {
                    *refused += 1;
                }
            }
        }
    }
}

fn retire_weapon(phase: &mut audio_control::phase::Phase, id: usize) {
    let mut row = [0.; weapon::STRIDE];
    row[weapon::ID] = id as f64;
    row[weapon::OWNER] = -1.;
    phase.observe_weapon(&row);
}

// presentEntityEvent's `game.units.byId(id)` with its generation check.
fn unit(receiver: &Receiver, id: u32, generation: u32) -> Option<&[f64; r::STRIDE]> {
    receiver
        .active()
        .find(|row| row[r::ID] == id as f64 && row[r::GENERATION] == generation as f64)
}

// Wire registries are one-based; zero and ids past the table name nothing.
// A present entry may still be the sound manifest's missing-class sentinel.
fn registered(table: &[usize], id: usize) -> Option<usize> {
    table.get(id.checked_sub(1)?).copied()
}

/// applyWorldSummary's post sounds: a post that changed hands is captured by
/// its new team or lost by its old one; a destructible post whose building
/// died goes inactive with its owner kept and is lost by that owner, as the
/// local battle's CommandPost.destroy event is. The
/// first sample of a post only initializes it.
fn apply_post_summary(posts: &mut [(usize, [f64; post::STRIDE])], building: &[bool], effects: &mut Vec<[f64; event::STRIDE]>, b: &[u8]) {
    let id = u16::from_le_bytes([b[0], b[1]]) as usize;
    let Some((slot, (_, p))) = posts.iter_mut().enumerate().find(|(_, p)| p.0 == id) else { return };
    let old = p[post::TEAM];
    let team = b[2] as f64;
    let active = b[21] != 0;
    let lost = p[post::ACTIVE] != 0. && !active && building.get(slot).copied().unwrap_or(false);
    if p[post::PRESENT] != 0. && (team != old || lost) && effects.len() < effects.capacity() {
        let mut e = [0.; event::STRIDE];
        e[event::KIND] = event_kind::POST as f64;
        e[event::OWNER] = slot as f64;
        e[event::VALUE] = if team != old && team != 0. { team } else { old };
        e[event::SECONDS] = if team != old && team != 0. { 0. } else { 2. };
        effects.push(e);
    }
    p[post::PRESENT] = 1.;
    p[post::TEAM] = team;
    p[post::ACTIVE] = b[21] as f64;
    p[post::CONTESTED] = b[20] as f64;
    p[post::PROGRESS] = f64::from_le_bytes(b[4..12].try_into().unwrap());
    p[post::NEUTRALIZE] = f64::from_le_bytes(b[12..20].try_into().unwrap());
}

#[cfg(test)]
mod post_tests {
    use super::*;

    fn summary(id: u16, team: u8, active: u8) -> [u8; 23] {
        let mut b = [0u8; 23];
        b[0..2].copy_from_slice(&id.to_le_bytes());
        b[2] = team;
        b[21] = active;
        b
    }

    #[test]
    fn a_destroyed_building_post_is_announced_lost_by_its_owner() {
        let mut posts = vec![(1, [0.; post::STRIDE]), (2, [0.; post::STRIDE])];
        let building = [true, false];
        let mut events = Vec::with_capacity(8);
        for id in [1, 2] { apply_post_summary(&mut posts, &building, &mut events, &summary(id, 2, 1)); }
        assert!(events.is_empty(), "the first sample only initializes");
        apply_post_summary(&mut posts, &building, &mut events, &summary(2, 2, 0));
        assert!(events.is_empty(), "a post without a building going inactive is not a loss");
        apply_post_summary(&mut posts, &building, &mut events, &summary(1, 2, 0));
        assert_eq!(events.len(), 1);
        assert_eq!((events[0][event::OWNER], events[0][event::VALUE], events[0][event::SECONDS]), (0., 2., 2.));
        apply_post_summary(&mut posts, &building, &mut events, &summary(1, 2, 0));
        assert_eq!(events.len(), 1, "announced once");
    }
}

#[cfg(test)]
mod batch_tests {
    use super::*;

    // A frame may publish up to COMMANDS words of its own; stream completions
    // received after that must not take the batch past the device's reserve.
    #[test]
    fn the_listener_keeps_his_space_while_inside_it_else_takes_the_earliest_box() {
        use super::super::sound_data::SpaceBox;
        let at = |x: f64, space: usize| SpaceBox { matrix: [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., x, 0., 0., 1.], size: [5., 5., 5.], space };
        let boxes = [at(0., 1), at(4., 2)];
        assert_eq!(listener_space(&boxes, None, [100., 0., 0.]), None);
        assert_eq!(listener_space(&boxes, None, [3., 0., 0.]), Some(1));
        // Inside both: the space he has stays; entering fresh, the earliest box wins.
        assert_eq!(listener_space(&boxes, Some(2), [3., 0., 0.]), Some(2));
        assert_eq!(listener_space(&boxes, None, [8., 0., 0.]), Some(2));
        assert_eq!(listener_space(&boxes, Some(1), [8., 0., 0.]), Some(2));
        assert_eq!(listener_space(&boxes, Some(1), [20., 0., 0.]), None);
    }
    #[test]
    fn a_full_frame_batch_reaches_the_device_without_overrunning_it() {
        let mut output = Output::new(48000., 2, false, false, false, false, Default::default());
        output.reserve_controls(COMMANDS);
        let mail = Arc::new(Mailbox::new());
        let batch = unsafe { &mut *mail.slots[0].get() };
        batch.commands.resize(COMMANDS - READY_WORDS - 1, 0.);
        // Accepted completions, each a stream bind and its start, while there is room.
        while batch.room_for_ready() {
            batch.commands.extend_from_slice(&[0.; READY_WORDS]);
        }
        assert!(batch.commands.len() <= COMMANDS);
        mail.write.store(1, Ordering::Release);
        let mut live = LiveOutput { output: Box::new(output), mail, next: 0, volume: 1., headless: false };
        live.quantum();
    }

    #[test]
    fn space_frame_wires_fighters_turrets_hits_doors_without_144hz_double_fire() {
        use audio_control::swbf2::{vehicles::DoorEvent, zero_hash, Swbf2Class};
        use bfe_sim::space::{
            flight::{SpaceFlightState, SpaceTrick},
            observations::{Contact, Round},
        };
        use crate::local::space::{Aim, Counts, Destruction, Frame, History, Hull, HullWeapon, Pose};

        let mut runtime = Runtime::new_for_family(7, bfe_family::GameFamily::Swbf2);
        let mut output = Output::new(48000., 2, false, false, false, false, false);
        output.reserve_controls(COMMANDS);
        for slot in 0..audio::output::VOICES {
            output.voice(slot, true);
        }
        output.voices.reserve_samples(16);
        let imp = |rt: &mut Runtime, kind: usize, row: &[f64]| {
            rt.input[..row.len()].copy_from_slice(row);
            rt.import(kind, row.len());
        };
        let mut bus = [f64::NAN; bus::STRIDE];
        bus[bus::GAIN] = 1.0;
        imp(&mut runtime, import::BUS, &bus);
        for s in 0..10 {
            let target = output.voices.sample_storage(s, 4800, 48000.);
            target.fill(0.25);
            imp(&mut runtime, import::SAMPLE, &[s as f64, 0.1, 48000., 1.0, 1.0]);
            imp(&mut runtime, import::ENTRY, &[s as f64, 1.0, 0.0, 0.0]);
            let mut snd = [f64::NAN; sound::STRIDE];
            snd[sound::KIND] = 1.0;
            snd[sound::BUS] = 0.0;
            snd[sound::GAIN] = 0.9;
            snd[sound::PITCH] = 1.0;
            snd[sound::MODE3D] = 1.0;
            snd[sound::MIN_DISTANCE] = 10.0;
            snd[sound::MAX_DISTANCE] = 200.0;
            snd[sound::MUTE_DISTANCE] = 400.0;
            snd[sound::ROLL_OFF] = 1.0;
            snd[sound::PRIORITY] = 0.8;
            snd[sound::LOOP] = if matches!(s, 2 | 3 | 6) { 1.0 } else { 0.0 };
            snd[sound::FIRST] = s as f64;
            snd[sound::COUNT] = 1.0;
            imp(&mut runtime, import::SOUND, &snd);
            runtime.phase.swbf2_mut().unwrap().sound_groups.push(0);
        }

        // Classes: 0=craft, 1=cannon, 2=turret, 3=chaingun, 4=bolt_ord, 5=boom_exp, 6=door
        let register = |rt: &mut Runtime, name: &str, row: [f64; class::STRIDE], ext: Swbf2Class| -> usize {
            let cid = rt.phase.classes.len();
            rt.input[..row.len()].copy_from_slice(&row);
            rt.import(import::CLASS, row.len());
            let sw = rt.phase.swbf2_mut().unwrap();
            sw.classes.push(ext);
            sw.register_class_hash(zero_hash(name), cid);
            cid
        };
        let mut r0 = [-1.; class::STRIDE];
        r0[class::DEATH] = 8.;
        let mut e0 = Swbf2Class::default();
        e0.trick_sound = 1;
        register(&mut runtime, "craft_alpha", r0, e0);

        let mut r1 = [-1.; class::STRIDE];
        r1[class::FIRE] = 0.;
        let mut e1 = Swbf2Class::default();
        e1.ordnance_class = 4;
        register(&mut runtime, "wpn_alpha", r1, e1);

        let r2 = [-1.; class::STRIDE];
        register(&mut runtime, "tur_alpha", r2, Swbf2Class::default());

        let r3 = [-1.; class::STRIDE];
        let mut e3 = Swbf2Class::default();
        e3.warm_up = 2;
        e3.barrel = 3;
        register(&mut runtime, "wpn_spin", r3, e3);

        let mut r4 = [-1.; class::STRIDE];
        r4[class::IMPACT] = 4.;
        r4[class::ORDNANCE] = 6.;
        let mut e4 = Swbf2Class::default();
        e4.explosion_class = 5;
        register(&mut runtime, "ord_alpha", r4, e4);

        let mut r5 = [-1.; class::STRIDE];
        r5[class::EXPLOSION] = 5.;
        register(&mut runtime, "exp_alpha", r5, Swbf2Class::default());

        let r6 = [-1.; class::STRIDE];
        let mut e6 = Swbf2Class::default();
        e6.door_open = 7;
        register(&mut runtime, "door_alpha", r6, e6);

        runtime.phase.swbf2_mut().unwrap().finalize_lookups();

        let prepared = Prepared {
            runtime,
            output,
            actor_classes: Vec::new(),
            weapon_classes: Vec::new(),
            combat_classes: Vec::new(),
            stations: Vec::new(),
            posts: Vec::new(),
            post_building: Vec::new(),
            voice_segments: Vec::new(),
            prediction_weapons: Vec::new(),
            actor_limits: Vec::new(),
            music: Vec::new(),
            streams: super::super::stream_assets::Assets::new(Vec::new(), Vec::new()),
            enhanced: Vec::new(),
            enhanced_base: 16,
            headphones: None,
            sound_names: (0..10).map(|i| format!("s{i}")).collect(),
            spaces: Vec::new(),
            space_boxes: Vec::new(),
        };
        let collision = std::rc::Rc::new(std::cell::RefCell::new(Box::new(bfe_sim::collision::CollisionOwner::new())));
        let mut sound = Sound::new(prepared, collision, 0);
        let mut history = History::new(Counts { fighters: 4, turrets: 2, rounds: 4, ..Default::default() });

        let mut frame = Frame::reserved(Counts { fighters: 4, turrets: 2, rounds: 4, ..Default::default() });
        frame.tick = 1;
        let mut weapons = [HullWeapon::default(); 5];
        weapons[0] = HullWeapon {
            class: zero_hash("wpn_alpha"),
            fired: true,
            firing: false,
            ..Default::default()
        };
        frame.hulls.push(Hull {
            id: 0,
            generation: 1,
            class: zero_hash("craft_alpha"),
            team: 1,
            previous: Pose { position: [10., 0., 0.], ..Default::default() },
            current: Pose { position: [12., 0., 0.], ..Default::default() },
            health: 100.,
            max_health: 100.,
            max_speed: 80.,
            piloted: true,
            trick: SpaceTrick::BarrelLeft,
            state: SpaceFlightState::Flying,
            weapon_count: 1,
            weapons,
            ..Default::default()
        });
        frame.turrets.push(Aim {
            building: 0,
            class: zero_hash("tur_alpha"),
            weapon: zero_hash("wpn_spin"),
            position: [15., 0., 0.],
            previous: [0., 0.],
            current: [0.2, 0.],
            yaw_rate: 1.5,
            pitch_rate: 1.5,
            alive: true,
            held: true,
            fired: true,
            firing: true,
            spinning: true,
            overheated: false,
            heat: 0.1,
            heat_threshold: 0.5,
        });
        frame.rounds.push((
            0,
            Round {
                serial: 10,
                weapon: zero_hash("wpn_alpha"),
                tick: 1,
                position: [20., 0., 0.],
                previous: [18., 0., 0.],
                active: false,
                contact: Some(Contact::default()),
                ..Default::default()
            },
        ));
        frame.rounds.push((
            1,
            Round {
                serial: 11,
                weapon: zero_hash("wpn_alpha"),
                tick: 1,
                position: [25., 0., 0.],
                previous: [22., 0., 0.],
                velocity: [40., 0., 0.],
                active: true,
                ..Default::default()
            },
        ));
        frame.deaths.push(Destruction {
            class: zero_hash("craft_alpha"),
            name: 0,
            position: [30., 0., 0.],
        });
        frame.door_events.push(DoorEvent::Open {
            class_hash: zero_hash("door_alpha"),
            position: [8., 0., 0.],
        });
        history.publish(&mut frame);

        let step_once = |sound: &mut Sound, wall: f64, dt: f64, shown: f64| {
            sound.runtime.clear_output();
            let f = &mut sound.runtime.phase.frame;
            f.fill(0.0);
            f[frame::DT] = dt;
            f[frame::WALL_NOW] = wall;
            f[frame::NOW] = wall;
            f[frame::RATE] = 48000.0;
            f[frame::RUNNING] = 1.0;
            f[frame::MAX_VOICES] = 32.0;
            f[frame::VIRTUAL_VOICES] = 64.0;
            f[frame::FZ] = 1.0;
            f[frame::UY] = 1.0;
            f[frame::TEAM] = 1.0;
            f[frame::HUMAN] = 1.0;
            f[frame::EFFECTS_VOLUME] = 1.0;
            f[frame::AMBIENCE_VOLUME] = 1.0;
            f[frame::MUSIC_VOLUME] = 1.0;
            f[frame::VOICE_VOLUME] = 1.0;
            f[frame::INTERFACE_VOLUME] = 1.0;
            f[frame::CHANNELS] = 2.0;
            f[frame::OWN] = OWN as f64;
            sound.runtime.events.clear();
            if !sound.runtime.phase.started {
                let mut start = [0.; event::STRIDE];
                start[event::KIND] = event_kind::START as f64;
                sound.runtime.events.push(start);
            }
            sound.sync_space(&history, shown);
            sound.runtime.step(&mut sound.queries);
        };

        step_once(&mut sound, 0.02, 1.0 / 60.0, 1.0);
        let active_sounds: Vec<usize> = sound
            .runtime
            .phase
            .engine
            .voices
            .iter()
            .filter(|v| !v.done)
            .map(|v| v.sound)
            .collect();
        for expected in [0usize, 1, 2, 3, 4, 5, 6, 7, 8] {
            assert!(
                active_sounds.contains(&expected),
                "expected sound {expected} active in {active_sounds:?}"
            );
        }
        let started_after_tick1 = sound.runtime.phase.engine.stats[2];

        // A second render frame on the exact same sim tick (144 Hz vs 60 Hz) must not re-fire one-shots.
        step_once(&mut sound, 0.03, 1.0 / 144.0, 1.0);
        assert_eq!(
            sound.runtime.phase.engine.stats[2],
            started_after_tick1,
            "same sim tick must not double-fire space one-shots"
        );
    }

    #[test]
    fn hero_audio_wires_player_and_bot_powers_and_sabers_without_double_play() {
        use audio_control::swbf2::{zero_hash, Swbf2Class};
        use bfe_sim::combat::powers::PowerEventKind;
        use bfe_sim::units::infantry::hero_audio::{
            HeroAudioSnapshot, HeroPowerEvent, HeroPowerHold, HeroSaberState, HeroSaberThrow,
        };

        let mut runtime = Runtime::new_for_family(13, bfe_family::GameFamily::Swbf2);
        let mut output = Output::new(48000., 2, false, false, false, false, false);
        output.reserve_controls(COMMANDS);
        for slot in 0..audio::output::VOICES {
            output.voice(slot, true);
        }
        output.voices.reserve_samples(1);
        runtime.phase.engine.samples.push(audio_control::engine::Sample {
            render_id: 0,
            duration: 1.0,
            rate: 48000.,
            scale: 1.,
            available: true,
        });
        let make_snd = |looping: bool| audio_control::engine::Sound {
            kind: 1,
            bus: usize::MAX,
            pitch: 1.,
            pitch_dev: 0.,
            gain: 0.85,
            gain_dev: 0.,
            reverb: 0.,
            looping,
            pan: 0.,
            mode3d: true,
            doppler: true,
            bias: 0.0001,
            priority: 0.5,
            curve: audio_control::engine::Curve { roll_in: 0., min: 15., mute: 80., max: 80., roll: 1. },
            probability: 1.,
            interval: 0.,
            interval_dev: 0.,
            random_position: false,
            cycle: -2,
            next: f64::NEG_INFINITY,
            stream: usize::MAX,
            entries: vec![audio_control::engine::Entry {
                id: 0,
                weight: 1.,
                interval: 0.,
                dev: 0.,
                next: f64::NEG_INFINITY,
            }],
        };
        // 0=push, 1=choke, 2=throw_launch, 3=throw_twirl, 4=saber_on, 5=saber_hum, 6=blast
        for (i, lp) in [(0, false), (1, false), (2, false), (3, true), (4, false), (5, true), (6, false)] {
            runtime.phase.engine.sounds.push(make_snd(lp));
            runtime.phase.swbf2_mut().unwrap().sound_groups.push(i as u32);
        }
        runtime.phase.classes = vec![[-1.; class::STRIDE]; 6];
        runtime.phase.swbf2_mut().unwrap().classes = vec![Swbf2Class::default(); 6];
        // 0=push, 1=choke, 2=throw, 3=throw_ord, 4=blade, 5=blast_exp
        runtime.phase.classes[0][class::FIRE] = 0.0;
        runtime.phase.classes[1][class::FIRE] = 1.0;
        runtime.phase.classes[2][class::FIRE] = 2.0;
        runtime.phase.classes[3][class::ORDNANCE] = 3.0;
        runtime.phase.classes[5][class::EXPLOSION] = 6.0;
        {
            let sw = runtime.phase.swbf2_mut().unwrap();
            sw.classes[2].ordnance_class = 3;
            sw.classes[4].explosion_class = 5;
            sw.classes[4].saber_turn_on = 4;
            sw.classes[4].saber_on_hum = 5;
            for (name, id) in [
                ("inv_push", 0),
                ("inv_choke", 1),
                ("inv_throw", 2),
                ("inv_throw_ord", 3),
                ("inv_blade", 4),
                ("inv_blast_exp", 5),
            ] {
                sw.register_class_hash(zero_hash(name), id);
            }
            sw.finalize_lookups();
        }

        let prepared = Prepared {
            runtime,
            output,
            streams: super::super::stream_assets::Assets::new(Vec::new(), Vec::new()),
            voice_segments: Vec::new(),
            actor_classes: Vec::new(),
            actor_limits: Vec::new(),
            weapon_classes: Vec::new(),
            prediction_weapons: Vec::new(),
            combat_classes: Vec::new(),
            stations: Vec::new(),
            posts: Vec::new(),
            post_building: Vec::new(),
            music: Vec::new(),
            spaces: Vec::new(),
            space_boxes: Vec::new(),
            enhanced: Vec::new(),
            enhanced_base: 0,
            headphones: None,
            sound_names: Vec::new(),
        };
        let collision = std::rc::Rc::new(std::cell::RefCell::new(Box::new(bfe_sim::collision::CollisionOwner::new())));
        let mut sound = Sound::new(prepared, collision, 0);
        sound.output = None;

        // Build a Receiver with own player (slot 10, gen 1) and a bot hero (slot 20, gen 1).
        let mut receiver = Receiver::new(serde_json::json!({}));
        receiver.own.id = 10.0;
        receiver.own.generation = 1.0;
        receiver.own.alive = true;
        // Observe emitters OWN (player) and 1 (bot) directly on phase for unit test.
        sound.runtime.phase.emitters[OWN][emitter::ID] = OWN as f64;
        sound.runtime.phase.emitters[OWN][emitter::KIND] = 1.0;
        sound.runtime.phase.emitters[OWN][emitter::ALIVE] = 1.0;
        sound.runtime.phase.emitters[OWN][emitter::GENERATION] = 1.0;
        sound.runtime.phase.entity_count = OWN + 1;

        let mut snap = HeroAudioSnapshot::default();
        snap.power_latest = 2;
        snap.power_count = 2;
        snap.power_events[0] = Some(HeroPowerEvent {
            sequence: 1,
            tick: 1,
            caster_slot: 10,
            caster_generation: 1,
            caster_pos: [0.0, 0.0, 0.0],
            target_pos: None,
            weapon: zero_hash("inv_push"),
            target_weapon: 0,
            kind: PowerEventKind::Cast,
            bot: false,
        });
        snap.power_events[1] = Some(HeroPowerEvent {
            sequence: 2,
            tick: 1,
            caster_slot: 20,
            caster_generation: 1,
            caster_pos: [0.0, 0.0, -8.0],
            target_pos: None,
            weapon: zero_hash("inv_blade"),
            target_weapon: 0,
            kind: PowerEventKind::Cast,
            bot: true,
        });
        snap.hold_count = 1;
        snap.holds[0] = Some(HeroPowerHold {
            caster_slot: 20,
            caster_generation: 1,
            weapon: zero_hash("inv_choke"),
            caster_pos: [0.0, 0.0, -8.0],
            target_pos: [0.0, 0.0, -2.0],
            bot: true,
        });
        snap.throw_count = 1;
        snap.throws[0] = Some(HeroSaberThrow {
            owner_slot: 10,
            owner_generation: 1,
            launched: 1,
            weapon: zero_hash("inv_throw"),
            saber_weapon: zero_hash("inv_blade"),
            position: [0.0, 1.0, -5.0],
            velocity: [0.0, 0.0, -20.0],
            bot: false,
        });
        snap.saber_count = 1;
        snap.sabers[0] = Some(HeroSaberState {
            owner_slot: 10,
            owner_generation: 1,
            weapon_hash: zero_hash("inv_blade"),
            combo_hash: 0,
            state_hash: 1,
            blades_off: 0,
            thrown: false,
            position: [0.0, 0.0, 0.0],
            human: true,
        });
        receiver.hero_audio = Some(Box::new(snap));

        let step = |sound: &mut Sound, receiver: &Receiver, wall: f64, dt: f64| {
            sound.runtime.clear_output();
            let f = &mut sound.runtime.phase.frame;
            f.fill(0.0);
            f[frame::DT] = dt;
            f[frame::WALL_NOW] = wall;
            f[frame::NOW] = wall;
            f[frame::RATE] = 48000.0;
            f[frame::RUNNING] = 1.0;
            f[frame::MAX_VOICES] = 32.0;
            f[frame::VIRTUAL_VOICES] = 64.0;
            f[frame::FZ] = -1.0;
            f[frame::UY] = 1.0;
            f[frame::EFFECTS_VOLUME] = 1.0;
            f[frame::AMBIENCE_VOLUME] = 1.0;
            f[frame::MUSIC_VOLUME] = 1.0;
            f[frame::VOICE_VOLUME] = 1.0;
            f[frame::INTERFACE_VOLUME] = 1.0;
            f[frame::CHANNELS] = 2.0;
            f[frame::OWN] = OWN as f64;
            sound.runtime.events.clear();
            if !sound.runtime.phase.started {
                let mut start = [0.; event::STRIDE];
                start[event::KIND] = event_kind::START as f64;
                sound.runtime.events.push(start);
            }
            sound.sync_hero_audio(receiver);
            sound.runtime.step(&mut sound.queries);
        };

        step(&mut sound, &receiver, 0.02, 1.0 / 60.0);
        let live: Vec<usize> = sound
            .runtime
            .phase
            .engine
            .voices
            .iter()
            .filter(|v| !v.done)
            .map(|v| v.sound)
            .collect();
        for expected in [0usize, 1, 2, 3, 4, 5, 6] {
            assert!(live.contains(&expected), "expected hero sound {expected} active in {live:?}");
        }
        let started_tick1 = sound.runtime.phase.engine.stats[2];

        // Second render frame on the same snapshot must not replay one-shots.
        step(&mut sound, &receiver, 0.03, 1.0 / 144.0);
        assert_eq!(
            sound.runtime.phase.engine.stats[2],
            started_tick1,
            "same snapshot must not double-play hero power one-shots"
        );
    }
}

#[cfg(test)]
mod pressure;
