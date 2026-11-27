//! 2005 music and voice-over director.
//! Extends the shared battle music and VO model with:
//! - `MaxPlaybackTime` automatic fade-to-idle on tracks
//! - raw fraction music stage thresholds (`Swbf2MusicSlot`) for points-based and ticket modes
//! - hero (`SoldierMusic`) and vehicle (`RepMusic`/`CisMusic`) priority tracks
//!   gated by `enable_hero_music`
//! - `BroadcastVoiceOver`, hero spawn/defeat/killer VO gated by `enable_hero_vo`,
//!   and `SetLowReinforcementsVoiceOver` (honouring distinct watched and listener teams)
//! - logical stream-group slot and 4-deep priority queue sharing across appended streams

use super::stream_group_of;
use crate::engine::{MUSIC, NONE, SILENT, VOICE};
use crate::layout::{frame as f, post as p};
use crate::phase::{index, Phase};
use crate::streams::Options;
use crate::{length, sub, Point};

pub const MAX_BROADCAST_QUEUE: usize = 32;
pub const MAX_HERO_VO_QUEUE: usize = 16;

/// Hero voice-over event kind gated by `ScriptCB_EnableHeroVO`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeroVoKind {
    Spawned,
    Defeated,
    Killer,
}

/// Raw authored 2005 ambient music stage slot retained without integer truncation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Swbf2MusicSlot {
    pub track: usize,
    /// Raw threshold from `SetAmbientMusic`: a `0.0..=1.0` remaining fraction when
    /// `is_fraction` is true, or a ticket/score count when false.
    pub amount: f64,
    pub is_fraction: bool,
}

/// Low-reinforcements voice-over trigger registered by the mission script.
/// Fires once per battle for `listener_team` when `watched_team`'s reinforcements
/// fall to or below `threshold`.
#[derive(Clone, Copy, Debug)]
pub struct LowReinforcementsRule {
    pub watched_team: usize,
    pub listener_team: usize,
    pub threshold: f64,
    pub sound: usize,
    pub is_fraction: bool,
    pub fired: bool,
}

#[derive(Clone, Copy)]
pub struct BroadcastVo {
    pub sound: usize,
    pub team_filter: u8,
}

#[derive(Clone, Copy)]
pub struct LogicalQueued {
    pub options: Options,
    pub score: f64,
}

/// 2005 music and voice-over director state.
pub struct Swbf2Director {
    /// Parallel to `director.tracks`: optional `MaxPlaybackTime` in seconds (`INFINITY` = unlimited).
    pub track_max_play: Vec<f64>,
    /// Tracks elapsed play time per track for `MaxPlaybackTime` enforcement.
    pub track_played: Vec<f64>,
    /// Authored per-team ambient stage slots retaining exact floating-point fraction thresholds.
    pub stage_slots: [[Option<Swbf2MusicSlot>; 3]; 3],
    pub hero_music_enabled: bool,
    pub hero_vo_enabled: bool,
    /// Optional remaining score fraction `[team1, team2]` in `0.0..=1.0` for points modes.
    pub mode_score_progress: Option<[f64; 2]>,
    /// Current local player mounted vehicle class (for `RepMusic`/`CisMusic`).
    pub own_vehicle_class: Option<usize>,
    /// Current local player hero class (for `SoldierMusic`).
    pub own_hero_class: Option<usize>,
    /// Script-requested music sound (`PlayInGameMusic`) and its live stream voice.
    pub script_music_sound: Option<usize>,
    pub script_music_voice: u64,
    pub script_music_started: bool,
    pub(super) script_input: Option<usize>,
    pub(super) failed_script_input: Option<(usize, usize)>,
    /// Maps each stream id to its logical stream group id (`AudioStreamAppendSegments`).
    pub stream_groups: Vec<usize>,
    /// Four-deep priority queue per logical stream group (shared by appended streams).
    pub logical_queues: Vec<[Option<LogicalQueued>; 4]>,
    pub low_reinforcements: Vec<LowReinforcementsRule>,
    pub initial_tickets: [f64; 3],
    broadcast_queue: [Option<BroadcastVo>; MAX_BROADCAST_QUEUE],
    broadcast_len: usize,
    hero_vo_queue: [Option<(usize, HeroVoKind)>; MAX_HERO_VO_QUEUE],
    hero_vo_len: usize,
    bleeding: [bool; 3],
    bleed_timers: [f64; 3],
    over_elapsed: Option<f64>,
}

impl Default for Swbf2Director {
    fn default() -> Self {
        Self {
            track_max_play: Vec::new(),
            track_played: Vec::new(),
            stage_slots: [[None; 3]; 3],
            hero_music_enabled: true,
            hero_vo_enabled: true,
            mode_score_progress: None,
            own_vehicle_class: None,
            own_hero_class: None,
            script_music_sound: None,
            script_music_voice: SILENT,
            script_music_started: false,
            script_input: None,
            failed_script_input: None,
            stream_groups: Vec::new(),
            logical_queues: Vec::new(),
            low_reinforcements: Vec::new(),
            initial_tickets: [0.0; 3],
            broadcast_queue: [None; MAX_BROADCAST_QUEUE],
            broadcast_len: 0,
            hero_vo_queue: [None; MAX_HERO_VO_QUEUE],
            hero_vo_len: 0,
            bleeding: [false; 3],
            bleed_timers: [0.0; 3],
            over_elapsed: None,
        }
    }
}

impl Swbf2Director {
    /// Stores the raw authored stage threshold without truncating fraction values.
    pub fn set_stage_slot(&mut self, team: usize, stage: usize, track: usize, amount: f64, is_fraction: bool) {
        if team < 3 && stage < 3 && track != NONE && amount.is_finite() {
            self.stage_slots[team][stage] = Some(Swbf2MusicSlot { track, amount, is_fraction });
        }
    }

    /// Registers an appended stream under `target_stream`'s logical slot and queue.
    pub fn bind_stream_group(&mut self, stream_id: usize, target_stream: usize) {
        let needed = stream_id.max(target_stream) + 1;
        if self.stream_groups.len() < needed {
            let start = self.stream_groups.len();
            self.stream_groups.resize(needed, 0);
            for (i, slot) in self.stream_groups.iter_mut().enumerate().skip(start) {
                *slot = i;
            }
        }
        let root = self.stream_groups[target_stream];
        self.stream_groups[stream_id] = root;
        if self.logical_queues.len() < needed {
            self.logical_queues.resize(needed, [None; 4]);
        }
    }

    pub fn ensure_tracks(&mut self, count: usize) {
        if self.track_max_play.len() < count {
            self.track_max_play.resize(count, f64::INFINITY);
        }
        if self.track_played.len() < count {
            self.track_played.resize(count, 0.0);
        }
    }

    pub fn set_track_max_play(&mut self, track_id: usize, max_play: f64) {
        self.ensure_tracks(track_id + 1);
        self.track_max_play[track_id] = if max_play.is_finite() && max_play > 0.0 {
            max_play
        } else {
            f64::INFINITY
        };
    }

    pub fn push_broadcast(&mut self, sound: usize, team_filter: u8) {
        if self.broadcast_len < MAX_BROADCAST_QUEUE {
            self.broadcast_queue[self.broadcast_len] = Some(BroadcastVo { sound, team_filter });
            self.broadcast_len += 1;
        }
    }

    pub fn push_hero_vo(&mut self, hero_class: usize, kind: HeroVoKind) {
        if self.hero_vo_len < MAX_HERO_VO_QUEUE {
            self.hero_vo_queue[self.hero_vo_len] = Some((hero_class, kind));
            self.hero_vo_len += 1;
        }
    }
}

impl Phase {
    /// Returns true if the logical stream group containing `stream` has a free playback slot.
    fn swbf2_logical_free(&self, stream: usize) -> bool {
        let Some(swbf2) = self.swbf2() else { return false };
        let group = stream_group_of(&swbf2.director.stream_groups, stream);
        let Some(def) = self.streams.definitions.get(group) else {
            return false;
        };
        let active = self.streams.active_in_group(|s| {
            stream_group_of(&swbf2.director.stream_groups, s) == group
        });
        active < def.slots
    }

    /// Queues a VO sound respecting 2005 logical stream-group sharing (`AudioStreamAppendSegments`).
    pub(crate) fn swbf2_queue_vo(&mut self, sound: usize, position: Option<Point>, positional: bool) -> bool {
        if !self.streams.has_playable_segment(&self.engine, sound) {
            return false;
        }
        let stream = match self.engine.sounds.get(sound) {
            Some(s) if s.kind == 4 && s.stream != NONE => s.stream,
            _ => return false,
        };
        let (group, has_group) = match self.swbf2() {
            Some(swbf2) => {
                let g = stream_group_of(&swbf2.director.stream_groups, stream);
                (g, g < swbf2.director.logical_queues.len())
            }
            None => (stream, false),
        };
        if !has_group {
            let mut opts = Options::new(sound, VOICE);
            opts.ask.position = position;
            opts.spatial = positional;
            return self.streams.queue(&mut self.engine, opts);
        }
        let s = &mut self.engine.sounds[sound];
        let Some(segment) = s.pick(self.engine.time, &mut self.engine.random) else {
            return false;
        };
        if !s.gate(self.engine.time, &mut self.engine.random) {
            return false;
        }
        let (mode3d, gain, bus, curve, bias, priority) =
            (s.mode3d, s.gain, s.bus, s.curve, s.bias, s.priority);
        let pos_for_score = if mode3d { position } else { None };
        let mut level = (gain * self.engine.bus_gain(bus)).min(1.0);
        if let Some(p) = pos_for_score {
            level *= curve.gain(length(sub(p, self.engine.listener.ear)));
        }
        let score = (1.0 - bias) * level + bias * priority;
        if score <= 0.0 {
            return false;
        }
        let mut options = Options::new(sound, VOICE);
        options.segment = segment;
        options.ask.position = position;
        options.spatial = positional;

        let old = self.swbf2().unwrap().director.logical_queues[group];
        let mut list = [None; 5];
        let mut n = 0;
        for req in old.into_iter().flatten() {
            let rs = &self.engine.sounds[req.options.ask.sound];
            let mut rlvl = (rs.gain * self.engine.bus_gain(rs.bus)).min(1.0);
            if rs.mode3d {
                if let Some(p) = req.options.ask.position {
                    rlvl *= rs.curve.gain(length(sub(p, self.engine.listener.ear)));
                }
            }
            let rscore = (1.0 - rs.bias) * rlvl + rs.bias * rs.priority;
            if rscore > 0.0 {
                list[n] = Some(LogicalQueued { score: rscore, ..req });
                n += 1;
            }
        }
        let at = (0..n).find(|&i| list[i].unwrap().score < score).unwrap_or(n);
        for i in (at..n).rev() {
            list[i + 1] = list[i];
        }
        list[at] = Some(LogicalQueued { options, score });
        n += 1;
        let mut accepted = true;
        if n > 4 && at == 4 {
            if options.ask.position.is_some() && mode3d {
                accepted = false;
            } else {
                list[3] = list[4];
            }
        }
        if let Some(swbf2) = self.swbf2_mut() {
            swbf2.director.logical_queues[group].copy_from_slice(&list[..4]);
        }
        accepted
    }

    pub(crate) fn swbf2_drain_logical_queues(&mut self) {
        let groups = self.swbf2().map_or(0, |s| s.director.logical_queues.len());
        for group in 0..groups {
            while self.swbf2().unwrap().director.logical_queues[group][0].is_some()
                && self.swbf2_logical_free(group)
            {
                let req = self.swbf2().unwrap().director.logical_queues[group][0].unwrap();
                if let Some(swbf2) = self.swbf2_mut() {
                    for i in 0..3 {
                        swbf2.director.logical_queues[group][i] =
                            swbf2.director.logical_queues[group][i + 1];
                    }
                    swbf2.director.logical_queues[group][3] = None;
                }
                self.streams.play(&mut self.engine, req.options);
            }
        }
    }

    fn capture_swbf2_initial_tickets(&mut self) {
        let t1 = self.frame[f::TICKETS1];
        let t2 = self.frame[f::TICKETS2];
        if let Some(swbf2) = self.swbf2_mut() {
            if swbf2.director.initial_tickets[1] <= 0.0 && t1 > 0.0 {
                swbf2.director.initial_tickets[1] = t1;
            }
            if swbf2.director.initial_tickets[2] <= 0.0 && t2 > 0.0 {
                swbf2.director.initial_tickets[2] = t2;
            }
        }
    }

    /// 2005 music director supporting `MaxPlaybackTime`, raw fraction stage thresholds
    /// for points and ticket modes, hero/vehicle priority tracks, script music override,
    /// and victory/defeat stings.
    pub(crate) fn swbf2_music(&mut self, dt: f64) {
        let n_tracks = self.director.tracks.len();
        if let Some(swbf2) = self.swbf2_mut() {
            swbf2.director.ensure_tracks(n_tracks);
            let input = swbf2.director.script_input.take();
            swbf2.director.failed_script_input = swbf2.director.failed_script_input.filter(|(sound, _)| Some(*sound) == input);
        }
        self.capture_swbf2_initial_tickets();

        // Advance track playback timers and enforce `MaxPlaybackTime`.
        for id in 0..n_tracks {
            if self.director.tracks[id].playing() {
                let exceeded = if let Some(swbf2) = self.swbf2_mut() {
                    swbf2.director.track_played[id] += dt;
                    swbf2.director.track_played[id] >= swbf2.director.track_max_play[id]
                } else {
                    false
                };
                if exceeded {
                    self.force_stop_track(id);
                }
            } else if let Some(swbf2) = self.swbf2_mut() {
                swbf2.director.track_played[id] = 0.0;
            }
        }

        // Handle explicit script music override (`PlayInGameMusic`).
        let (script_sound, script_voice, script_started) = match self.swbf2() {
            Some(s) => (
                s.director.script_music_sound,
                s.director.script_music_voice,
                s.director.script_music_started,
            ),
            None => return,
        };
        if let Some(script_sound) = script_sound {
            if self.streams.sounding(script_voice) {
                if !script_started {
                    for id in 0..n_tracks { self.force_stop_track(id); }
                    self.swbf2_mut().unwrap().director.script_music_started = true;
                }
                return;
            }
            if script_voice == SILENT {
                let stream = self.engine.sounds[script_sound].stream;
                let retiring = self.director.tracks.iter().find(|t| {
                    t.playing() && self.engine.sounds.get(t.sound).is_some_and(|s| s.stream == stream)
                }).map_or(SILENT, |t| t.voice());
                let mut options = Options::new(script_sound, MUSIC);
                options.fade = 0.5;
                let voice = self.streams.play_replacing(&mut self.engine, options, retiring);
                if voice == SILENT {
                    self.stop_in_game_music(0.0);
                } else {
                    self.swbf2_mut().unwrap().director.script_music_voice = voice;
                    if !self.director.tracks.iter().any(|t| t.playing()) { return; }
                }
            } else if !self.streams.playing(script_voice) {
                self.swbf2_end_script();
            } else if !self.director.tracks.iter().any(|t| t.playing()) {
                return;
            }
        }

        // End of battle: victory/defeat stings after 2 s.
        if self.frame[f::OVER] != 0.0 {
            let before = self.swbf2().and_then(|s| s.director.over_elapsed).unwrap_or(0.0);
            let now = before + dt;
            if let Some(swbf2) = self.swbf2_mut() {
                swbf2.director.over_elapsed = Some(now);
            }
            if before < 2.0 && now >= 2.0 {
                for id in 0..n_tracks {
                    self.force_stop_track(id);
                }
                self.end_battle();
                self.stop_swbf2_sabers();
                self.stop_swbf2_vehicles();
                self.stop_swbf2_weapons();
                self.stop_swbf2_ambience();
                let team = (self.frame[f::TEAM] as usize).clamp(1, 2);
                let lost = self.frame[if team == 1 { f::TICKETS1 } else { f::TICKETS2 }] <= 0.0;
                let track = if lost {
                    self.director.defeat[team]
                } else {
                    self.director.victory[team]
                };
                self.start_track_swbf2(track);
            }
            return;
        }

        // Determine current ambient stage from raw authored fractions (points mode or ticket mode).
        // Follows the shared director rule: team 2 is visited last so its matched stage wins.
        let (stage_slots, mode_progress, initial_tickets) = {
            let swbf2 = self.swbf2().unwrap();
            (
                swbf2.director.stage_slots,
                swbf2.director.mode_score_progress,
                swbf2.director.initial_tickets,
            )
        };
        let has_raw_slots = stage_slots.iter().flatten().any(Option::is_some);
        let mut stage = self.director.stage;
        if let Some([p1, p2]) = mode_progress {
            for (team, progress) in [(1usize, p1), (2usize, p2)] {
                if has_raw_slots {
                    for s in (0..3).rev() {
                        if stage_slots[team][s].is_some_and(|slot| slot.amount >= progress) {
                            stage = s;
                            break;
                        }
                    }
                } else {
                    for s in (0..3).rev() {
                        if self.director.slots[team][s].is_some_and(|slot| slot.threshold >= progress) {
                            stage = s;
                            break;
                        }
                    }
                }
            }
        } else {
            for team in 1..=2 {
                let left = self.frame[if team == 1 { f::TICKETS1 } else { f::TICKETS2 }];
                if has_raw_slots {
                    let start = initial_tickets[team];
                    for s in (0..3).rev() {
                        if let Some(slot) = stage_slots[team][s] {
                            let thresh = if slot.is_fraction {
                                if start > 0.0 {
                                    start * slot.amount
                                } else if let Some(dslot) = self.director.slots[team][s] {
                                    dslot.threshold
                                } else {
                                    slot.amount
                                }
                            } else {
                                slot.amount
                            };
                            if thresh >= left {
                                stage = s;
                                break;
                            }
                        }
                    }
                } else {
                    for s in (0..3).rev() {
                        if self.director.slots[team][s].is_some_and(|slot| slot.threshold >= left) {
                            stage = s;
                            break;
                        }
                    }
                }
            }
        }

        // Single-stage maps (such as Hero Assault) stay on stage 0 even when tickets are 0.
        let team = (self.frame[f::TEAM] as usize).min(2);
        let stage_track = |st: usize| -> usize {
            if has_raw_slots {
                stage_slots[team][st].map_or(NONE, |s| s.track)
            } else {
                self.director.slots[team][st].map_or(NONE, |s| s.track)
            }
        };
        if stage_track(stage) == NONE && stage_track(0) != NONE {
            stage = 0;
        }
        self.director.stage = stage;

        // Collect desired tracks: ambient stage track + optional hero or vehicle priority track.
        let ambient_track = stage_track(stage);
        let (vehicle_track, hero_track) = {
            let swbf2 = self.swbf2().unwrap();
            let v_track = swbf2.director.own_vehicle_class.map_or(NONE, |cls| {
                swbf2.classes.get(cls).map_or(NONE, |c| {
                    if team == 2 {
                        if c.cis_music != NONE { c.cis_music } else { c.rep_music }
                    } else if c.rep_music != NONE {
                        c.rep_music
                    } else {
                        c.cis_music
                    }
                })
            });
            let h_track = if swbf2.director.hero_music_enabled {
                swbf2
                    .director
                    .own_hero_class
                    .and_then(|cls| swbf2.classes.get(cls))
                    .map_or(NONE, |c| c.soldier_music)
            } else {
                NONE
            };
            (v_track, h_track)
        };

        // Pick highest-priority wanted/active track (unbounded track count, no fixed cap).
        let wanted = [ambient_track, vehicle_track, hero_track];

        // Advance director track clocks using the standard Track fields via helper methods.
        self.director.advance_swbf2_tracks(dt, &wanted);

        // Stop earlier stage track when a new stage is active and `min_play` allows.
        if stage > 0 {
            for (t, raw_team) in stage_slots.iter().enumerate().skip(1) {
                let prev = if has_raw_slots {
                    raw_team[stage - 1].map_or(NONE, |s| s.track)
                } else {
                    self.director.slots[t][stage - 1].map_or(NONE, |s| s.track)
                };
                if prev != NONE && prev != ambient_track && prev < n_tracks && self.director.track_can_stop(prev) {
                    self.force_stop_track(prev);
                }
            }
        }

        let mut head = NONE;
        for id in 0..n_tracks {
            let is_wanted = wanted.contains(&id);
            let playing = self.director.tracks[id].playing();
            if !(is_wanted || playing) {
                continue;
            }
            let rank = self.director.track_rank(id);
            if head == NONE || rank > self.director.track_rank(head) {
                head = id;
            }
        }

        // Stop lower-priority tracks when `min_play` allows.
        for id in 0..n_tracks {
            if id != head && self.director.tracks[id].playing() && self.director.track_can_stop(id) {
                self.force_stop_track(id);
            }
        }

        let any_playing = self.director.tracks.iter().any(|t| t.playing());
        if head != NONE && wanted.contains(&head) && !self.director.tracks[head].playing() && !any_playing && self.director.track_ready(head) {
            self.start_track_swbf2(head);
        }
    }

    fn start_track_swbf2(&mut self, id: usize) {
        if id == NONE || id >= self.director.tracks.len() {
            return;
        }
        let sound = self.director.tracks[id].sound;
        let fade_in = self.director.tracks[id].fade_in;
        let mut opts = Options::new(sound, MUSIC);
        opts.fade = fade_in;
        let voice = self.streams.play(&mut self.engine, opts);
        if voice != SILENT {
            self.director.mark_track_started(id, voice);
            if let Some(swbf2) = self.swbf2_mut() {
                swbf2.director.ensure_tracks(id + 1);
                swbf2.director.track_played[id] = 0.0;
            }
        }
    }

    fn force_stop_track(&mut self, id: usize) {
        if id == NONE || id >= self.director.tracks.len() {
            return;
        }
        if let Some((voice, fade_out)) = self.director.take_track_voice(id) {
            self.streams.stop(&mut self.engine, voice, fade_out);
            if let Some(swbf2) = self.swbf2_mut() {
                if id < swbf2.director.track_played.len() {
                    swbf2.director.track_played[id] = 0.0;
                }
            }
        }
    }

    fn swbf2_end_script(&mut self) {
        let d = &self.swbf2().unwrap().director;
        let failed = d.script_music_sound.filter(|_| !d.script_music_started);
        let layout = if self.engine.enhanced { self.engine.channels } else { 0 };
        self.stop_in_game_music(0.0);
        self.swbf2_mut().unwrap().director.failed_script_input = failed.map(|sound| (sound, layout));
    }
    pub(crate) fn swbf2_streams_ended(&mut self) {
        self.streams_ended();
        let Some(d) = self.swbf2().map(|s| &s.director) else { return };
        if d.script_music_voice != SILENT && !self.streams.playing(d.script_music_voice) {
            self.swbf2_end_script();
        }
    }

    /// 2005 voice-over director: broadcast VO, hero VO, low reinforcements, bleeding, post info.
    pub(crate) fn swbf2_voice_over(&mut self, dt: f64) {
        if self.frame[f::OVER] != 0.0 {
            return;
        }
        let own = (self.frame[f::TEAM] as usize).clamp(1, 2);

        // 1. Drain BroadcastVoiceOver requests.
        let mut broadcasts = [None; MAX_BROADCAST_QUEUE];
        let b_count = {
            let Some(swbf2) = self.swbf2_mut() else { return };
            let n = swbf2.director.broadcast_len;
            for (dst, src) in broadcasts[..n]
                .iter_mut()
                .zip(&mut swbf2.director.broadcast_queue[..n])
            {
                *dst = src.take();
            }
            swbf2.director.broadcast_len = 0;
            n
        };
        for b in broadcasts[..b_count].iter().flatten().copied() {
            if b.team_filter == 0 || b.team_filter as usize == own {
                self.swbf2_queue_vo(b.sound, None, false);
            }
        }

        // 2. Drain hero VO requests (gated by `ScriptCB_EnableHeroVO`).
        let mut heroes = [None; MAX_HERO_VO_QUEUE];
        let (h_count, hero_vo_enabled) = {
            let Some(swbf2) = self.swbf2_mut() else { return };
            let n = swbf2.director.hero_vo_len;
            for (dst, src) in heroes[..n]
                .iter_mut()
                .zip(&mut swbf2.director.hero_vo_queue[..n])
            {
                *dst = src.take();
            }
            swbf2.director.hero_vo_len = 0;
            (n, swbf2.director.hero_vo_enabled)
        };
        if hero_vo_enabled {
            for (hero_class, kind) in heroes[..h_count].iter().flatten().copied() {
                let sound = self
                    .swbf2()
                    .and_then(|s| s.classes.get(hero_class))
                    .map_or(NONE, |c| match kind {
                        HeroVoKind::Spawned => c.hero_spawned,
                        HeroVoKind::Defeated => c.hero_defeated,
                        HeroVoKind::Killer => c.hero_killer,
                    });
                if sound != NONE {
                    self.swbf2_queue_vo(sound, None, false);
                }
            }
        }

        // 3. Low-reinforcements VO rules (distinct listener_team and watched_team).
        self.capture_swbf2_initial_tickets();
        let rule_count = self.swbf2().map_or(0, |s| s.director.low_reinforcements.len());
        for i in 0..rule_count {
            let (rule, start) = {
                let swbf2 = self.swbf2().unwrap();
                let rule = swbf2.director.low_reinforcements[i];
                (rule, swbf2.director.initial_tickets[rule.watched_team])
            };
            if rule.fired || rule.listener_team != own {
                continue;
            }
            let current = self.frame[if rule.watched_team == 1 { f::TICKETS1 } else { f::TICKETS2 }];
            let triggered = if rule.is_fraction {
                start > 0.0 && current > 0.0 && (current / start) <= rule.threshold
            } else {
                current > 0.0 && current <= rule.threshold
            };
            if triggered {
                if let Some(swbf2) = self.swbf2_mut() {
                    swbf2.director.low_reinforcements[i].fired = true;
                }
                self.swbf2_queue_vo(rule.sound, None, false);
            }
        }

        // 4. Command post info VO and bleeding VO through the logical queue.
        if self.frame[f::HUMAN] != 0.0 {
            for id in 0..self.post_count {
                let row = self.posts[id];
                if row[p::PRESENT] == 0.0 || row[p::ACTIVE] == 0.0 || row[p::BUILDING_ALIVE] == 0.0 {
                    continue;
                }
                let owner = row[p::TEAM] as usize;
                if (1..=2).contains(&owner) {
                    let sound = index(row[p::INFO11 + (own - 1) * 2 + owner - 1]);
                    if sound != NONE {
                        self.swbf2_queue_vo(sound, None, false);
                    }
                }
            }
        }
        for team in 1..=2 {
            let now = self.frame[if team == 1 { f::BLEED1 } else { f::BLEED2 }] > 0.0;
            let (was, own_bleeding) = {
                let swbf2 = self.swbf2_mut().unwrap();
                let was = swbf2.director.bleeding[team];
                swbf2.director.bleeding[team] = now;
                (was, swbf2.director.bleeding[own])
            };
            if !now {
                continue;
            }
            if !was {
                if let Some(swbf2) = self.swbf2_mut() {
                    swbf2.director.bleed_timers[team] = 0.0;
                }
                if team != own && own_bleeding {
                    continue;
                }
                let sound = self.director.bleed[own][team];
                if sound != NONE {
                    self.swbf2_queue_vo(sound, None, false);
                }
            } else {
                let repeat = if let Some(swbf2) = self.swbf2_mut() {
                    swbf2.director.bleed_timers[team] += dt;
                    if swbf2.director.bleed_timers[team] > 30.0 {
                        swbf2.director.bleed_timers[team] = 0.0;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };
                if repeat && team == own {
                    let sound = self.director.bleed[own][team];
                    if sound != NONE {
                        self.swbf2_queue_vo(sound, None, false);
                    }
                }
            }
        }
    }
}
