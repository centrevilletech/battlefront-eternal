//! Shell music cycle (`ShellMusic` in `src/shell/audio.js`).
//!
//! This module ports `ShellMusic` from `src/shell/audio.js` on top of
//! `audio_control::shell_music::Music` (`rust/audio-control/src/shell_music.rs`):
//! - `SOUNDTRACK = "shell_soundtrack"` plays its tracks in turn, one at a time,
//!   with `INTERVAL = 10.0` seconds of silence between them
//!   (`SetShellMusicInterval`).
//! - A screen names the music it wants (`set(Some(name))`); `set(None)` keeps
//!   what plays, and `set(Some(STOP))` stops it.
//! - As in the original player, the silence is measured against `interval` as
//!   it stands on each `update(dt)`, so shortening `interval` (the AUDIO
//!   OPTIONS page sets `0.0`) cuts a silence short immediately.

use crate::data::MusicTrack;
use audio_control::{
    shell_music::Music as MusicKernel,
    shell_music_layout::{shell_music_op as op, shell_music_phase as phase, shell_music_row as r},
};
use std::collections::BTreeMap;

/// Default shell soundtrack group name.
/// Mirrors `SOUNDTRACK` in `src/shell/audio.js`.
pub const SOUNDTRACK: &str = "shell_soundtrack";

/// Cue name that halts music playback (`set(Some(STOP))`).
/// Mirrors `STOP` in `src/shell/audio.js`.
pub const STOP: &str = "STOP";

/// Default silence between tracks in seconds (`SetShellMusicInterval`).
/// Mirrors `INTERVAL` in `src/shell/audio.js`.
pub const INTERVAL: f64 = 10.0;

/// Bus gains (`shellmusic = 0.6`, `shellfx = 0.3`).
/// Mirrors `BUSES` in `src/shell/audio.js`.
pub mod buses {
    /// Shell music bus gain (`0.6` of `musicVolume`).
    pub const SHELL_MUSIC: f64 = 0.6;
    /// Shell interface/whoosh sound-effects bus gain (`0.3` of `interfaceVolume`).
    pub const SHELL_FX: f64 = 0.3;
}

/// Playback phase of [`ShellMusic`].
/// Mirrors `PHASES = ["stopped", "playing", "gap"]` in `src/shell/audio.js`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MusicPhase {
    /// No cue is active (`"stopped"`).
    Stopped,
    /// A track is actively playing (`"playing"`).
    Playing,
    /// Silence interval between tracks in a cycle (`"gap"`).
    Gap,
}

impl MusicPhase {
    /// Returns the string label (`"stopped"`, `"playing"`, or `"gap"`).
    /// Mirrors `PHASES[row[M.PHASE]]` in `src/shell/audio.js`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Playing => "playing",
            Self::Gap => "gap",
        }
    }
}

/// Playback handle token identifying a started track instance so a stale
/// `ended(Some(old_handle))` call after track replacement is ignored.
pub type TrackHandle = u64;

/// Start/stop transitions produced by a single [`ShellMusic`] command.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct MusicStep {
    /// Handle of the previous track stopped by this command, if any.
    pub stopped: Option<(TrackHandle, usize)>,
    /// Handle and track index (`0..tracks.len()`) started by this command, if any.
    pub started: Option<(TrackHandle, usize)>,
}

/// Pure shell music timeline controller wrapping `audio_control::shell_music::Music`.
/// Mirrors `ShellMusic` in `src/shell/audio.js`.
pub struct ShellMusic {
    /// Authored shell music tracks loaded from `shell.json`.
    tracks: Vec<MusicTrack>,
    /// Silence duration in seconds between consecutive tracks (`INTERVAL = 10.0`).
    pub interval: f64,
    /// Maps cue/group names (`"shell_soundtrack"`, etc.) to `kernel` group indices.
    groups: BTreeMap<String, usize>,
    /// Out-of-range group index passed to `kernel` when a cue name is unknown.
    missing: usize,
    /// Underlying `audio_control::shell_music::Music` state machine.
    kernel: MusicKernel,
    /// Currently requested music cue name, if any.
    current: Option<String>,
    /// Active playback handle token and track index (`0..tracks.len()`), if playing.
    handle: Option<(TrackHandle, usize)>,
    /// Monotonic counter used to mint unique [`TrackHandle`] tokens.
    next_handle: TrackHandle,
}

impl ShellMusic {
    /// Constructs a `ShellMusic` controller from `tracks` and `interval`
    /// (default [`INTERVAL`]).
    ///
    /// Mirrors `new ShellMusic({ tracks, interval })` in `src/shell/audio.js`.
    pub fn new(tracks: Vec<MusicTrack>, interval: f64) -> Self {
        let mut groups: BTreeMap<String, usize> = BTreeMap::new();
        let mut members: Vec<Vec<usize>> = Vec::new();
        let mut add = |name: &str, index: usize| {
            let id = if let Some(&id) = groups.get(name) {
                id
            } else {
                let id = members.len();
                groups.insert(name.to_string(), id);
                members.push(Vec::new());
                id
            };
            members[id].push(index);
        };

        // Resolve authored string membership while loading so runtime frames
        // operate on stable group and track indices (`src/shell/audio.js`).
        for (i, t) in tracks.iter().enumerate() {
            if let Some(props) = &t.properties {
                let mut seen = Vec::new();
                for name in props {
                    if !seen.contains(name) {
                        seen.push(name.clone());
                        add(name, i);
                    }
                }
            } else {
                // Legacy fallback when `properties` is absent: `soundtrack` tracks
                // join `SOUNDTRACK`, and every track is also addressable by `name`.
                if t.soundtrack {
                    add(SOUNDTRACK, i);
                }
                if t.name != SOUNDTRACK {
                    add(&t.name, i);
                }
            }
        }
        let missing = members.len();
        let kernel = MusicKernel::new(members);
        Self {
            tracks,
            interval,
            groups,
            missing,
            kernel,
            current: None,
            handle: None,
            next_handle: 1,
        }
    }

    /// Returns the track list.
    pub fn tracks(&self) -> &[MusicTrack] {
        &self.tracks
    }

    /// Returns the current playback phase (`Stopped`, `Playing`, or `Gap`).
    /// Mirrors `get phase()` in `src/shell/audio.js`.
    pub fn phase(&self) -> MusicPhase {
        match self.kernel.row[r::PHASE] as u32 {
            phase::PLAYING => MusicPhase::Playing,
            phase::GAP => MusicPhase::Gap,
            _ => MusicPhase::Stopped,
        }
    }

    /// Returns the shared cycle index (`-1.0` before the first track starts).
    /// Mirrors `get cycle()` in `src/shell/audio.js`.
    pub fn cycle(&self) -> f64 {
        self.kernel.row[r::CYCLE]
    }

    /// Returns the elapsed silence in seconds during a `Gap`.
    /// Mirrors `get silent()` in `src/shell/audio.js`.
    pub fn silent(&self) -> f64 {
        self.kernel.row[r::SILENT]
    }

    /// Returns the currently active handle and track index `(handle, track_idx)`,
    /// if any.
    pub fn handle(&self) -> Option<(TrackHandle, usize)> {
        self.handle
    }

    /// Returns the currently playing or active [`MusicTrack`] if `handle` is
    /// `Some`.
    pub fn current_track(&self) -> Option<&MusicTrack> {
        self.handle.and_then(|(_, idx)| self.tracks.get(idx))
    }

    /// Returns whether `name` is the currently active music cue (even during the
    /// silence gap between tracks).
    /// Mirrors `ShellMusic.playing(name)` in `src/shell/audio.js`.
    pub fn playing(&self, name: Option<&str>) -> bool {
        let Some(name) = name.filter(|s| !s.is_empty()) else {
            return false;
        };
        self.current.as_deref() == Some(name)
    }

    /// Writes a command into the [`MusicKernel`] row, steps `kernel.frame()`,
    /// and translates `STOP` / `START` outputs into a [`MusicStep`].
    /// Mirrors `ShellMusic.command(op, dt, group, name)` in `src/shell/audio.js`.
    fn command(&mut self, op_code: u32, dt: f64, group: usize, name: Option<String>) -> MusicStep {
        self.kernel.row[r::ENABLED] = if name.as_ref().is_some_and(|s| !s.is_empty()) {
            1.0
        } else {
            0.0
        };
        self.kernel.row[r::OP] = op_code as f64;
        self.kernel.row[r::DT] = dt;
        self.kernel.row[r::INTERVAL] = self.interval;
        self.kernel.row[r::GROUP] = group as f64;
        self.kernel.frame();

        // Read kernel output decisions (`r::STOP` and 1-based `r::START` track index).
        let stop = self.kernel.row[r::STOP] != 0.0;
        let track = self.kernel.row[r::START] as usize;
        let mut step = MusicStep::default();
        if stop {
            let old = self.handle.take();
            self.current = None;
            step.stopped = old;
        }
        self.current = name;
        // A non-zero `r::START` index (`1..=tracks.len()`) starts a new track
        // and assigns it a fresh `TrackHandle`.
        if track > 0 {
            let idx = track - 1;
            let h = self.next_handle;
            self.next_handle += 1;
            self.handle = Some((h, idx));
            step.started = Some((h, idx));
        }
        step
    }

    /// Sets the active music cue (`Some("shell_soundtrack")`, `Some("STOP")`,
    /// `None` to keep current).
    ///
    /// Mirrors `ShellMusic.set(name)` in `src/shell/audio.js`.
    pub fn set(&mut self, name: Option<&str>) -> MusicStep {
        let Some(name) = name else {
            return MusicStep::default();
        };
        if self.current.as_deref() == Some(name) {
            return MusicStep::default();
        }
        if name == STOP {
            return self.halt();
        }
        let group = self.groups.get(name).copied().unwrap_or(self.missing);
        self.command(op::SET, 0.0, group, Some(name.to_string()))
    }

    /// Stops music playback immediately without rewinding the shared track cycle.
    /// Mirrors `ShellMusic.halt()` in `src/shell/audio.js`.
    pub fn halt(&mut self) -> MusicStep {
        self.current = None;
        self.command(op::HALT, 0.0, 0, None)
    }

    /// Advances immediately to the next track in the active group.
    /// Mirrors `ShellMusic.next()` in `src/shell/audio.js`.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> MusicStep {
        let name = self.current.clone();
        self.command(op::NEXT, 0.0, 0, name)
    }

    /// Marks the active track (or `handle` if supplied) as ended, entering the
    /// silence `Gap` until `interval` elapses. If `handle` does not match the
    /// current active handle, the call is ignored.
    ///
    /// Mirrors `ShellMusic.ended(handle)` in `src/shell/audio.js`.
    pub fn ended(&mut self, handle: Option<TrackHandle>) -> MusicStep {
        if let Some(h) = handle {
            if self.handle.map(|(cur, _)| cur) != Some(h) {
                return MusicStep::default();
            }
        } else if self.handle.is_none() {
            return MusicStep::default();
        }
        let name = self.current.clone();
        let step = self.command(op::END, 0.0, 0, name);
        // Clear the active handle once the kernel transitions into the `Gap` phase.
        if self.phase() == MusicPhase::Gap {
            self.handle = None;
        }
        step
    }

    /// Advances the silence timer by `dt` seconds; starts the next track in the
    /// cycle once `silent >= interval`.
    ///
    /// Mirrors `ShellMusic.update(dt)` in `src/shell/audio.js`.
    pub fn update(&mut self, dt: f64) -> MusicStep {
        let name = self.current.clone();
        self.command(op::TICK, dt, 0, name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Sample track list with three `shell_soundtrack` tracks (`a`, `b`, `c`)
    // and one standalone screen cue (`shell_galacticconquest`).
    fn sample_tracks() -> Vec<MusicTrack> {
        vec![
            MusicTrack {
                name: "a".to_string(),
                url: "/audio/shell/a.ogg".to_string(),
                properties: None,
                soundtrack: true,
            },
            MusicTrack {
                name: "b".to_string(),
                url: "/audio/shell/b.ogg".to_string(),
                properties: None,
                soundtrack: true,
            },
            MusicTrack {
                name: "shell_galacticconquest".to_string(),
                url: "/audio/shell/gc.ogg".to_string(),
                properties: None,
                soundtrack: false,
            },
            MusicTrack {
                name: "c".to_string(),
                url: "/audio/shell/c.ogg".to_string(),
                properties: None,
                soundtrack: true,
            },
        ]
    }

    // Verifies that `SOUNDTRACK` cycles through `a -> b -> c -> a` with `INTERVAL`
    // seconds of `Gap` between tracks while skipping non-soundtrack cues.
    #[test]
    fn soundtrack_plays_tracks_in_turn_with_10s_silence_and_skips_non_soundtrack()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut m = ShellMusic::new(sample_tracks(), INTERVAL);
        let s0 = m.set(Some(SOUNDTRACK));
        assert_eq!(m.tracks()[s0.started.ok_or("not started")?.1].name, "a");
        assert!(m.playing(Some(SOUNDTRACK)));

        m.ended(None);
        assert_eq!(m.phase(), MusicPhase::Gap);

        let s1 = m.update(INTERVAL - 0.01);
        assert!(s1.started.is_none());
        assert!(m.playing(Some(SOUNDTRACK)));

        let s2 = m.update(0.02);
        assert_eq!(m.tracks()[s2.started.ok_or("not started")?.1].name, "b");

        for expected in ["c", "a"] {
            m.ended(None);
            let step = m.update(INTERVAL);
            assert_eq!(
                m.tracks()[step.started.ok_or("not started")?.1].name,
                expected
            );
        }
        Ok(())
    }

    // Verifies that `set(None)` and `set(same)` keep the current track,
    // `set(Some(STOP))` halts without resetting the cycle, and resuming
    // `SOUNDTRACK` advances to the next track (`b`).
    #[test]
    fn screen_music_keeps_same_ignores_none_stops_on_stop_and_resumes_cycle()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut m = ShellMusic::new(sample_tracks(), INTERVAL);
        let s0 = m.set(Some(SOUNDTRACK));
        assert_eq!(m.tracks()[s0.started.ok_or("not started")?.1].name, "a");
        assert_eq!(m.set(Some(SOUNDTRACK)), MusicStep::default());
        assert_eq!(m.set(None), MusicStep::default());

        let s_gc = m.set(Some("shell_galacticconquest"));
        assert_eq!(m.tracks()[s_gc.stopped.ok_or("not stopped")?.1].name, "a");
        assert_eq!(
            m.tracks()[s_gc.started.ok_or("not started")?.1].name,
            "shell_galacticconquest"
        );

        let s_stop = m.set(Some(STOP));
        assert_eq!(
            m.tracks()[s_stop.stopped.ok_or("not stopped")?.1].name,
            "shell_galacticconquest"
        );
        assert!(!m.playing(Some(SOUNDTRACK)));
        assert_eq!(m.phase(), MusicPhase::Stopped);
        assert_eq!(m.update(100.0), MusicStep::default());

        let s_resume = m.set(Some(SOUNDTRACK));
        assert_eq!(
            m.tracks()[s_resume.started.ok_or("not started")?.1].name,
            "b"
        );
        Ok(())
    }

    // Verifies that lowering `interval` during a silence `Gap` starts the next
    // track immediately, and that a stale `TrackHandle` is ignored by `ended`.
    #[test]
    fn shorter_interval_cuts_silence_short_and_stale_handle_cannot_end_replacement()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut m = ShellMusic::new(sample_tracks(), INTERVAL);
        let s0 = m.set(Some(SOUNDTRACK));
        let old_handle = s0.started.ok_or("not started")?.0;
        m.ended(None);
        assert_eq!(m.update(4.0), MusicStep::default());
        m.interval = 0.0;
        let s1 = m.update(0.0);
        assert_eq!(m.tracks()[s1.started.ok_or("not started")?.1].name, "b");

        // Replacing the track invalidates the old handle so a late ended(old_handle) is a no-op.
        let s_gc = m.set(Some("shell_galacticconquest"));
        let new_handle = s_gc.started.ok_or("not started")?.0;
        assert_ne!(old_handle, new_handle);
        m.ended(Some(old_handle));
        assert_eq!(m.phase(), MusicPhase::Playing);
        assert_eq!(m.handle().ok_or("missing handle")?.0, new_handle);
        Ok(())
    }

    // Verifies that tracks with explicit `properties` group lists are indexed
    // under each named group in declaration order.
    #[test]
    fn tracks_with_properties_play_under_each_named_group_in_order()
    -> Result<(), Box<dyn std::error::Error>> {
        let tracks = vec![
            MusicTrack {
                name: "shell_soundtrack_1".to_string(),
                url: "/a.opus".to_string(),
                properties: Some(vec!["shell_soundtrack".to_string()]),
                soundtrack: true,
            },
            MusicTrack {
                name: "shell_clonewars".to_string(),
                url: "/b.opus".to_string(),
                properties: Some(vec![
                    "shell_soundtrack".to_string(),
                    "shell_clonewars".to_string(),
                ]),
                soundtrack: true,
            },
            MusicTrack {
                name: "shell_droidinvasion".to_string(),
                url: "/c.opus".to_string(),
                properties: Some(vec![
                    "shell_galacticconquest".to_string(),
                    "shell_droidinvasion".to_string(),
                ]),
                soundtrack: true,
            },
            MusicTrack {
                name: "shell_soundtrack_5".to_string(),
                url: "/d.opus".to_string(),
                properties: Some(vec!["shell_soundtrack".to_string()]),
                soundtrack: true,
            },
        ];
        let mut m = ShellMusic::new(tracks, INTERVAL);
        let mut played = Vec::new();
        let s0 = m.set(Some(SOUNDTRACK));
        played.push(m.tracks()[s0.started.ok_or("not started")?.1].name.clone());
        for _ in 0..3 {
            m.ended(None);
            let step = m.update(INTERVAL);
            played.push(
                m.tracks()[step.started.ok_or("not started")?.1]
                    .name
                    .clone(),
            );
        }
        assert_eq!(
            played,
            vec![
                "shell_soundtrack_1",
                "shell_clonewars",
                "shell_soundtrack_5",
                "shell_soundtrack_1",
            ]
        );
        let gc = m.set(Some("shell_galacticconquest"));
        assert_eq!(
            m.tracks()[gc.started.ok_or("not started")?.1].name,
            "shell_droidinvasion"
        );
        Ok(())
    }
}
