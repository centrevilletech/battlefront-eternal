//! The battle's music, as the 2004 game runs it (docs/formats.md,
//! "Sound"):
//!
//! - SetAmbientMusic(team, amount, track, stage, isFraction) gives each team
//!   a start, middle, and end track; with isFraction the threshold is that
//!   fraction of the team's reinforcements when the script ran, in whole
//!   tickets (Theed: 200, 198, 20).
//! - The stage: for team 1 and then team 2, the last of its stages whose
//!   threshold its tickets are at or below. Team 2 is looked at last and its
//!   start stage always matches, so in practice team 2's tickets set the
//!   stage for both sides.
//! - Every frame the listener's team's track for the stage is asked for, and
//!   the stage before it stopped (for both teams). One track plays at a time:
//!   the highest priority among those asked for, once it is ready. A track
//!   is ready MinInactiveTime after it last stopped (and at the start); once
//!   playing it can't be stopped for MinPlaybackTime. It fades in over
//!   FadeInTime and out over FadeOutTime, plays one segment (picked by
//!   weight, or in turn with CyclePlayback), and, still asked for, plays
//!   again when it is next ready: Theed's start track plays its 39 s, rests
//!   90 s, and plays again.
//! - When a team is out of reinforcements, 2 s later the game is over: every
//!   stream and voice stops, sound effects fade out over a second, and the
//!   listener's team's victory or defeat track plays.
//!
//! Voice-over during a battle, as the 2004 game triggers it (docs/
//! formats.md, "Sound"). Every line is 2D and queued on its stream (cw_vo on
//! Theed), so lines never overlap.
//!
//! - Command posts: each post's instance properties name a line for each
//!   listener side, actor side, and event: VO_<listener>_<actor>Capture when
//!   the actor's side captures it, VO_<listener>_<actor>Lost when the actor's
//!   side loses it (it was neutralized). Theed sets six per post, for the
//!   Republic and CIS listening: your side capturing, losing, and the enemy
//!   capturing. The capture also plays the post's capture sound at the post,
//!   and the loss its LostSound.
//! - Bleeding (SetBleedingVoiceOver(listener, bleeding team, line, 1)): a
//!   team starting to bleed reinforcements plays the listener's line for it;
//!   while the listener's own team keeps bleeding the line repeats every
//!   30 s. An enemy starting to bleed while your own team bleeds is silent.
//!
//! - Leaving the battlefield (SetOutOfBoundsVoiceOver(team, line)): the
//!   listener's own side's line ("repleaving" on a Clone Wars Bespin), once
//!   each time the player goes out (src/battlefield.js).
//!
//! - Objectives (VO_<listener>_<owner side>Info): while a post stands, the
//!   post's update asks for its owner's Info line every frame,
//!   "defend the shield generator" to the Rebels, "destroy the shield
//!   generator" to the Empire; the line's own PlayInterval (90 s for these)
//!   paces it. Only destructible posts set one. Not while the game is over
//!   or before the player has a body.
//!
//! The AI's chatter shares the stream and its queue (chatter.js). Not in the
//! port yet: planetary bonus lines (Galactic Conquest only).
//!
//! The map's own ambience, as the 2004 game sets it up:
//!
//! - Static emitters: placed objects with a Sound property (Theed's 36
//!   com_snd_amb_static: 31 "canal" and 5 "fountain"), each a looping sound
//!   at its place. MinDistance and MaxDistance (which sets MuteDistance too)
//!   replace the sound's; the class's own are 1 and 10. Out of reach they keep
//!   time silently (virtual voices).
//! - Stream regions, named "soundstream <properties> <v>": an emitter of that
//!   stream at the region's centre, its MaxDistance and MuteDistance the
//!   region's size (|SIZE|, whatever its shape) and its MinDistance that over
//!   v. Theed's "soundstream nab_amb_city 1.0" is 400.5 m: the city's quad
//!   bed at full level anywhere within 400.5 m of the centre, silent beyond.
//!   A multichannel stream isn't panned, only set by distance.
//!
//! Battle music, mission VO and ambience policy from music.js, vo.js and
//! ambience.js. Timers use game time; stream source ends use device time.
use crate::phase::{Phase, index};
use crate::engine::{Curve, NONE, SILENT, AMBIENCE, MUSIC, VOICE, FOLEY};
use crate::streams::Options;
use crate::layout::{frame as f, post as p, class as c, region as r};

pub struct Track {
    pub sound: usize, pub min_play: f64, pub min_idle: f64, pub priority: f64, pub fade_in: f64, pub fade_out: f64,
    state: bool, played: f64, idle: f64, voice: u64, wanted: bool,
}
impl Track {
    pub fn new(sound: usize, min_play: f64, min_idle: f64, priority: f64, fade_in: f64, fade_out: f64) -> Self { Self { sound, min_play, min_idle, priority, fade_in, fade_out, state: false, played: 0.0, idle: min_idle, voice: SILENT, wanted: false } }
    fn rank(&self) -> f64 { if self.idle >= self.min_idle { self.priority + 0.0001 } else { 0.0 } }
    fn ended(&mut self) { self.state = false; self.voice = SILENT; self.idle = 0.0; }
}
#[derive(Clone, Copy)]
pub struct Slot { pub track: usize, pub threshold: f64 }
pub struct Director {
    pub tracks: Vec<Track>, pub slots: [[Option<Slot>; 3]; 3], pub victory: [usize; 3], pub defeat: [usize; 3],
    pub bounds: [usize; 3], pub bleed: [[usize; 3]; 3], pub stage: usize, over: Option<f64>, bleeding: [bool; 3], timers: [f64; 3],
    pub regions: Vec<[f64; r::STRIDE]>, pub region_voices: Vec<u64>, pub static_emitters: Vec<[f64; r::STRIDE]>, pub static_voices: Vec<u64>,
}
impl Default for Director {
    fn default() -> Self { Self { tracks: Vec::new(), slots: [[None; 3]; 3], victory: [NONE; 3], defeat: [NONE; 3], bounds: [NONE; 3], bleed: [[NONE; 3]; 3], stage: 0, over: None, bleeding: [false; 3], timers: [0.0; 3], regions: Vec::new(), region_voices: Vec::new(), static_emitters: Vec::new(), static_voices: Vec::new() } }
}
impl Phase {
    pub fn line(&mut self, sound: usize) { if sound != NONE { self.streams.queue(&mut self.engine, Options::new(sound, VOICE)); } }
    fn track_start(&mut self, id: usize) {
        if id == NONE { return; } let t = &mut self.director.tracks[id]; t.state = true; t.played = 0.0;
        let mut options = Options::new(t.sound, MUSIC); options.fade = t.fade_in; t.voice = self.streams.play(&mut self.engine, options);
        if t.voice == SILENT { t.ended(); }
    }
    fn track_stop(&mut self, id: usize) {
        let t = &mut self.director.tracks[id]; if !t.state || t.played < t.min_play { return; }
        let voice = t.voice; t.voice = SILENT; self.streams.stop(&mut self.engine, voice, t.fade_out); t.ended();
    }
    /// ports BattleMusic.update/stageNow and Track's state machine. Team two
    /// is deliberately visited last, so its start threshold can choose the
    /// stage for both teams. Stable rank ties keep track creation order.
    pub fn music(&mut self, dt: f64) {
        for t in &mut self.director.tracks { if t.state { t.played += dt; } else { t.idle += dt; } }
        if self.frame[f::OVER] != 0.0 {
            let before = self.director.over.unwrap_or(0.0); let now = before + dt; self.director.over = Some(now);
            if before < 2.0 && now >= 2.0 {
                for t in &mut self.director.tracks { self.streams.stop(&mut self.engine, t.voice, 0.05); t.ended(); }
                self.end_battle(); let team = self.frame[f::TEAM] as usize;
                let lost = self.frame[if team == 1 { f::TICKETS1 } else { f::TICKETS2 }] <= 0.0;
                self.track_start(if lost { self.director.defeat[team] } else { self.director.victory[team] });
            } return;
        }
        let mut stage = self.director.stage;
        for team in 1..=2 { let left = self.frame[if team == 1 { f::TICKETS1 } else { f::TICKETS2 }]; for s in (0..3).rev() { if self.director.slots[team][s].is_some_and(|slot| slot.threshold >= left) { stage = s; break; } } }
        self.director.stage = stage; for t in &mut self.director.tracks { t.wanted = false; }
        let team = self.frame[f::TEAM] as usize; let want = self.director.slots[team][stage].map_or(NONE, |slot| slot.track);
        if want != NONE { self.director.tracks[want].wanted = true; }
        if stage > 0 { for team in 1..=2 { if let Some(slot) = self.director.slots[team][stage - 1] { if slot.track != want { self.track_stop(slot.track); } } } }
        let mut head = NONE;
        for (id, t) in self.director.tracks.iter().enumerate() { if (t.wanted || t.state) && (head == NONE || t.rank() > self.director.tracks[head].rank()) { head = id; } }
        for id in 0..self.director.tracks.len() { if id != head && self.director.tracks[id].state { self.track_stop(id); } }
        let playing = self.director.tracks.iter().any(|t| t.state);
        if head != NONE { let t = &self.director.tracks[head]; if t.wanted && !t.state && !playing && t.idle >= t.min_idle { self.track_start(head); } }
    }
    /// ports VoiceOver.update/objectives. The own-team bleed test observes
    /// the same sequential team updates as the original controller.
    pub fn voice_over(&mut self, dt: f64) {
        if self.frame[f::OVER] != 0.0 { return; } let own = self.frame[f::TEAM] as usize;
        if self.frame[f::HUMAN] != 0.0 {
            for id in 0..self.post_count { let row = self.posts[id]; if row[p::PRESENT] == 0.0 || row[p::ACTIVE] == 0.0 || row[p::BUILDING_ALIVE] == 0.0 { continue; } let owner = row[p::TEAM] as usize;
                if (1..=2).contains(&owner) { self.line(index(row[p::INFO11 + (own - 1) * 2 + owner - 1])); }
            }
        }
        for team in 1..=2 {
            let now = self.frame[if team == 1 { f::BLEED1 } else { f::BLEED2 }] > 0.0; let was = self.director.bleeding[team]; self.director.bleeding[team] = now; if !now { continue; }
            if !was { self.director.timers[team] = 0.0; if team != own && self.director.bleeding[own] { continue; } self.line(self.director.bleed[own][team]); }
            else { self.director.timers[team] += dt; if self.director.timers[team] > 30.0 { self.director.timers[team] = 0.0; if team == own { self.line(self.director.bleed[own][team]); } } }
        }
    }
    /// ports VoiceOver.post and GameAudio.EVENTS.post. Sound and VO use the
    /// same event identity but retain their independent gates and queues.
    pub fn post_event(&mut self, id: usize, actor: usize, captured: bool) {
        if id >= self.post_count { return; } let row = self.posts[id];
        self.play_class(index(row[p::CLASS]), if captured { c::CAPTURE } else { c::LOST }, FOLEY, NONE, Some([row[p::X], row[p::Y], row[p::Z]]), None);
        let listener = self.frame[f::TEAM] as usize; if !(1..=2).contains(&actor) || !(1..=2).contains(&listener) { return; }
        self.line(index(row[if captured { p::CAPTURE11 } else { p::LOST11 } + (listener - 1) * 2 + actor - 1]));
    }
    fn ambience_options(row: &[f64; r::STRIDE]) -> Options {
        let mut options = Options::new(index(row[r::SOUND]), AMBIENCE); options.ask.position = Some([row[r::X], row[r::Y], row[r::Z]]); options.ask.looping = Some(true);
        options.ask.curve = Some(Curve { roll_in: 0.0, min: row[r::MIN_DISTANCE], max: row[r::MAX_DISTANCE], mute: row[r::MAX_DISTANCE], roll: 0.0 }); options.fade = 0.5; options
    }
    /// ports Ambience.constructor/update. Static voices start once, whereas
    /// stream regions retry only after the preceding stream handle has ended.
    pub fn start_ambience(&mut self) {
        for i in 0..self.director.static_emitters.len() {
            let row = self.director.static_emitters[i]; let mut options = Self::ambience_options(&row); let sound = options.ask.sound;
            if let Some(s) = self.engine.sounds.get(sound) { let mut curve = s.curve; curve.min = row[r::MIN_DISTANCE]; curve.max = row[r::MAX_DISTANCE]; curve.mute = curve.max; options.ask.curve = Some(curve); }
            let voice = self.play(options.ask); self.director.static_voices[i] = voice;
        }
    }
    pub fn ambience(&mut self) {
        for i in 0..self.director.regions.len() {
            if self.streams.playing(self.director.region_voices[i]) { continue; }
            let row = self.director.regions[i]; let mut options = Self::ambience_options(&row);
            if let Some(s) = self.engine.sounds.get(options.ask.sound) { let mut curve = s.curve; curve.min = row[r::MIN_DISTANCE]; curve.max = row[r::MAX_DISTANCE]; curve.mute = curve.max; options.ask.curve = Some(curve); }
            self.director.region_voices[i] = self.streams.play(&mut self.engine, options);
        }
    }
    pub fn streams_ended(&mut self) { for t in &mut self.director.tracks { if t.voice != SILENT && !self.streams.playing(t.voice) { t.ended(); } } }
    pub fn end_battle(&mut self) {
        self.streams.stop_all(&mut self.engine, 0.05);
        for id in 0..self.engine.voices.len() { let v = &self.engine.voices[id]; if !v.done { self.stop(v.serial * crate::engine::CAPACITY as u64 + id as u64, 0.05); } }
        for id in 0..crate::layers::GROUPS { let handle = self.layers.handle(id); if self.layers.is_active(handle) { self.stop(handle, 0.05); } }
        self.stop_scene(); self.stop_vehicles(); if let Some(bus) = self.engine.buses.get_mut(self.master_bus) { bus.fade(0.0, 1.0); }
    }
}
