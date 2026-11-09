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
    samples: Vec<(usize, Option<super::stream_assets::Pcm>)>,
}
impl Batch {
    fn new() -> Self {
        Self {
            commands: Vec::with_capacity(COMMANDS),
            placements: Vec::with_capacity(PLACEMENTS),
            room: [0.; audio::reverb::PARAM_COUNT],
            mask: 0,
            samples: Vec::with_capacity(512),
        }
    }
}
struct Mailbox {
    slots: Box<[UnsafeCell<Batch>]>,
    read: AtomicUsize,
    write: AtomicUsize,
    clock: AtomicU64,
    peak: AtomicU32,
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
        }
    }
    fn available(&self) -> bool {
        self.write.load(Ordering::Relaxed).wrapping_sub(self.read.load(Ordering::Acquire)) < QUEUE
    }
}
pub struct LiveOutput {
    output: Output,
    mail: Arc<Mailbox>,
    next: usize,
}
impl LiveOutput {
    // Output.process renders one fixed DSP quantum. Consume every published
    // control batch first, in order, then retain partially consumed samples
    // across callbacks of arbitrary device buffer length. A callback does not
    // advance the gameplay control runtime or ask a window thread for work.
    fn quantum(&mut self) {
        let mut read = self.mail.read.load(Ordering::Relaxed);
        let write = self.mail.write.load(Ordering::Acquire);
        while read != write {
            let batch = unsafe { &mut *self.mail.slots[read % QUEUE].get() };
            for (id, pcm) in batch.samples.drain(..) {
                if let Some(pcm) = pcm {
                    self.output.voices.decoded_sample(id, pcm.rate, pcm.channels, pcm.frames, pcm.data);
                } else {
                    self.output.voices.decoded_sample(id, 48000., 1, 0, Vec::new());
                }
            }
            if batch.mask != 0 {
                if let Some(master) = &mut self.output.master {
                    master.room(&batch.room, batch.mask, false);
                }
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
        self.output.process();
        self.mail.clock.store(self.output.frame, Ordering::Release);
        self.next = 0;
    }
    pub fn fill<T: cpal::SizedSample + cpal::FromSample<f32>>(&mut self, data: &mut [T], status: &crate::audio::Status) {
        let mut peak = 0f32;
        for sample in data {
            if self.next == FRAMES * 2 {
                self.quantum();
                status.blocks.fetch_add(1, Ordering::Relaxed);
            }
            let value = self.output.output[(self.next % 2) * FRAMES + self.next / 2];
            peak = peak.max(value.abs());
            *sample = T::from_sample(value);
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
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShotInput<'a> {
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
}
#[derive(Clone, Copy)]
struct Shot {
    owner_id: u32,
    owner_generation: u32,
    weapon_id: usize,
    channel: usize,
    position: Option<[f64; 3]>,
    repair: bool,
}
pub struct Sound {
    assets: super::stream_assets::Assets,
    pub decoded: u64,
    pub voice_decoded: u64,
    voice_segments: Vec<bool>,
    posts: Vec<(usize,[f64;post::STRIDE])>,
    runtime: Runtime,
    pending: Vec<Shot>,
    pending_effects: Vec<[f64; event::STRIDE]>,
    cosmetic: bfe_sim::wire::cosmetics::Compact,
    combat_classes: Vec<usize>,
    pub event_counts: [u64; 27],
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
    pub refused: u64,
    last_clock: u64,
    music: Vec<[f64; music_slot::STRIDE]>,
    starts: [f64; 2],
    ui_paused: bool,
    ui_board: bool,
}
impl Sound {
    pub fn new(prepared: Prepared, collision: Collision, world: u32) -> Self {
        let mail = Arc::new(Mailbox::new());
        let output = Some(LiveOutput { output: prepared.output, mail: mail.clone(), next: FRAMES * 2 });
        Self {
            assets: prepared.streams,
            decoded: 0,
            voice_decoded: 0,
            voice_segments: prepared.voice_segments,
            posts: prepared.posts,
            runtime: prepared.runtime,
            pending: Vec::with_capacity(2048),
            pending_effects: Vec::with_capacity(2048),
            cosmetic: Default::default(),
            combat_classes: prepared.combat_classes,
            event_counts: [0;27],
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
            refused: 0,
            last_clock: 0,
            music: prepared.music,
            starts: [f64::NAN; 2],
            ui_paused: false,
            ui_board: false,
        }
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
            self.fade_bus("ingame", if open { 0. } else { 1. }, 1.);
        }
    }

    pub fn set_paused(&mut self, paused: bool) {
        if paused != self.ui_paused {
            self.ui_paused = paused;
            self.ui_event(event_kind::PAUSE, "", paused as u8 as f64, 0.);
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
        row[event::SOUND] = super::sound_data::hash(name) as f64;
        row[event::VALUE] = value;
        row[event::SECONDS] = seconds;
        self.pending_effects.push(row);
    }

    pub fn frame(
        &mut self,
        receiver: &Receiver,
        listener: &[f64; 16],
        dt: f64,
        prediction: &super::replay::Replay,
        battle: ([f64; 4], bool),
    ) {
        if self.output.is_some() {
            return;
        }
        // Device time can stand still between two window frames. Retain
        // authoritative fire facts before that early return; the scene may
        // consume its packet journal while no audio quantum has elapsed.
        // A paused window keeps the newest bounded tail without reserving.
        if self.epoch != receiver.epoch {
            self.epoch = receiver.epoch;
            self.pending.clear();
            self.pending_effects.clear();
        }
        for field in receiver.publications.iter() {
            // applyWorldSummary publishes post ownership before the audio
            // phase. A newly received first sample is initialization, not a
            // capture; subsequent changes feed the ordinary post sound/VO.
            if field.row[r::KIND] == k::SUMMARY as f64
                && field.row[r::SCHEMA] == 2.
                && field.length >= 23
            {
                let b = &field.bytes[..field.length];
                let id = u16::from_le_bytes([b[0], b[1]]) as usize;
                if let Some((slot, (_, p))) =
                    self.posts.iter_mut().enumerate().find(|(_, p)| p.0 == id)
                {
                    let old = p[post::TEAM];
                    let team = b[2] as f64;
                    if p[post::PRESENT] != 0.
                        && team != old
                        && self.pending_effects.len() < self.pending_effects.capacity()
                    {
                        let mut e = [0.; event::STRIDE];
                        e[event::KIND] = event_kind::POST as f64;
                        e[event::OWNER] = slot as f64;
                        e[event::VALUE] = if team != 0. { team } else { old };
                        e[event::SECONDS] = if team != 0. { 0. } else { 2. };
                        self.pending_effects.push(e);
                    }
                    p[post::PRESENT] = 1.;
                    p[post::TEAM] = team;
                    p[post::ACTIVE] = b[21] as f64;
                    p[post::CONTESTED] = b[20] as f64;
                    p[post::PROGRESS] = f64::from_le_bytes(b[4..12].try_into().unwrap());
                    p[post::NEUTRALIZE] = f64::from_le_bytes(b[12..20].try_into().unwrap());
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
            if field.row[r::KIND] != k::COMBAT as f64 || field.row[r::SCHEMA] != 8. {
                continue;
            }
            let Ok(source) = serde_json::from_slice::<ShotInput>(&field.bytes[..field.length])
            else {
                continue;
            };
            if source.kind != "shot" {
                continue;
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
        }
        // Preserve control state while the device is stopped or its bounded
        // queue is full. Advancing it after dropping START would orphan a loop.
        let clock = self.mail.clock.load(Ordering::Acquire);
        if clock == self.last_clock {
            return;
        }
        self.last_clock = clock;
        if !self.mail.available() {
            self.refused += 1;
            return;
        }
        // Device time governs scheduled sample starts and stream parts.
        // Game dt still governs the control phase's frame transitions. Using
        // window wall time for both clocks makes pause/resume schedule audio
        // in a quantum the callback has already consumed.
        let wall = self.mail.clock.load(Ordering::Acquire) as f64 / 48000.;
        let runtime = &mut self.runtime;
        let starts = [battle.0[2], battle.0[3]];
        if starts != self.starts {
            self.starts = starts;
            for row in &mut self.music {
                row[music_slot::START] =
                    starts[(row[music_slot::TEAM] as usize).saturating_sub(1).min(1)];
                runtime.input[..row.len()].copy_from_slice(row);
                runtime.import(import::MUSIC_SLOT, row.len());
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
        frame[frame::LIGHT] = 1.;
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
        for at in [
            frame::EFFECTS_VOLUME,
            frame::AMBIENCE_VOLUME,
            frame::MUSIC_VOLUME,
            frame::VOICE_VOLUME,
            frame::INTERFACE_VOLUME,
        ] {
            frame[at] = 1.;
        }
        frame[frame::CHANNELS] = 2.;
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
        own[emitter::KIND] = 1.;
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
            runtime.phase.emitters[OWN][[emitter::SELECTED0, emitter::SELECTED1][channel]] =
                id as f64;
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
        }
        runtime.step(&mut self.queries);
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
        batch.samples.clear();
        batch.commands.clear();
        batch.commands.extend_from_slice(commands);
        batch.placements.clear();
        batch.placements.extend_from_slice(placements);
        batch.mask = runtime.info[info::ROOM_CHANGED] as u32;
        for (i, v) in batch.room.iter_mut().enumerate() {
            *v = runtime.info[info::ROOM + i];
        }
        // Asset requests carry the runtime's generation. PCM is moved through
        // this same mailbox before its START commands; stale decode results
        // cannot replace a sample belonging to a newer use of the stream slot.
        submit_requests(runtime, &self.assets, batch, &mut self.refused);
        for _ in 0..128 {
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
        self.mail
            .write
            .store(write.wrapping_add(1), Ordering::Release);
        self.batches += 1;
    }
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
                runtime.phase.streams.failed(&runtime.phase.engine, request.voice, request.generation);
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
