//! 2005 world ambience: box/cylinder/sphere `soundspace` regions, interior
//! reverb mapping for Classic and Enhanced modes, space-to-space attenuation,
//! and script-controlled `soundtrigger` regions.

use super::{is_stream_sound, LoopRetry};
use crate::acoustics::Room;
use crate::engine::{AMBIENCE, NONE, SILENT};
use crate::layout::frame as f;
use crate::phase::Phase;
use crate::streams::Options;
use crate::{Collision, Point};

/// Provisional blend weight for authored interior spaces in Enhanced mode.
/// Applied from the raw ray-measured room into `rendered_room` (never back
/// into `acoustics.room`), so the blend never compounds across ticks.
pub const INTERIOR_SPACE_BLEND: f64 = 0.65;

/// Region volume primitive as authored in world files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionShape {
    Box,
    Cylinder,
    Sphere,
}

/// Evaluates whether world point `p` lies inside an oriented region primitive.
pub fn region_holds(matrix: &[f64; 16], size: [f64; 3], shape: RegionShape, p: Point) -> bool {
    let d = [p[0] - matrix[12], p[1] - matrix[13], p[2] - matrix[14]];
    let lx = d[0] * matrix[0] + d[1] * matrix[1] + d[2] * matrix[2];
    let ly = d[0] * matrix[4] + d[1] * matrix[5] + d[2] * matrix[6];
    let lz = d[0] * matrix[8] + d[1] * matrix[9] + d[2] * matrix[10];
    match shape {
        RegionShape::Box => lx.abs() <= size[0] && ly.abs() <= size[1] && lz.abs() <= size[2],
        RegionShape::Cylinder => lx * lx + lz * lz <= size[0] * size[0] && ly.abs() <= size[1],
        RegionShape::Sphere => lx * lx + ly * ly + lz * lz <= size[0] * size[0],
    }
}

/// One authored reverb space preset (`I3DL2ReverbPreset` + `RoomGain` send).
#[derive(Clone, Copy)]
pub struct SpacePreset {
    pub hash: u32,
    pub send: f64,
    pub room: Room,
    pub mask: u32,
}

impl Default for SpacePreset {
    fn default() -> Self {
        Self {
            hash: 0,
            send: 0.0,
            room: Room {
                room: 0.0,
                room_hf: 1.0,
                decay: 1.0,
                hf_ratio: 0.5,
                reflections: 0.0,
                refl_delay: 0.02,
                reverb: 0.0,
                reverb_delay: 0.04,
                diffusion: 1.0,
                density: 1.0,
                hf_reference: 5000.0,
            },
            mask: 1,
        }
    }
}

/// One `soundspace` region binding a volume to a `SpacePreset` index.
#[derive(Clone, Copy, Debug)]
pub struct SpaceRegion {
    pub matrix: [f64; 16],
    pub size: [f64; 3],
    pub shape: RegionShape,
    pub space: usize,
}

impl SpaceRegion {
    pub fn holds(&self, p: Point) -> bool {
        region_holds(&self.matrix, self.size, self.shape, p)
    }
}

/// Authored attenuation when the listener is in `from_space` and a 3D source is in `to_space`.
#[derive(Clone, Copy, Debug)]
pub struct SpaceAttenuation {
    pub from_space: usize,
    pub to_space: usize,
    pub gain: f64,
}

/// A `soundtrigger` region toggled by `ScriptCB_TriggerSoundRegionEnable`.
#[derive(Clone, Copy, Debug)]
pub struct TriggerRegion {
    pub hash: u32,
    pub matrix: [f64; 16],
    pub size: [f64; 3],
    pub shape: RegionShape,
    pub sound: usize,
    pub enabled: bool,
    pub was_active: bool,
    pub voice: u64,
    pub is_stream: bool,
    pub retry: LoopRetry,
}

impl TriggerRegion {
    pub fn holds(&self, p: Point) -> bool {
        region_holds(&self.matrix, self.size, self.shape, p)
    }
}

/// Per-battle 2005 interior space and trigger ambience state.
#[derive(Default)]
pub struct AmbienceState {
    pub spaces: Vec<SpacePreset>,
    pub space_regions: Vec<SpaceRegion>,
    pub attenuations: Vec<SpaceAttenuation>,
    pub triggers: Vec<TriggerRegion>,
    pub current_space: Option<usize>,
    pub space_set: bool,
    pub dry: SpacePreset,
    /// Non-compounding Enhanced-mode room blended from raw `acoustics.room` and the active preset.
    pub rendered_room: Room,
}

impl AmbienceState {
    /// Finds the active space for `ear`, staying in the current space while still inside any of its regions.
    pub fn resolve_space(&self, current: Option<usize>, ear: Point) -> Option<usize> {
        if let Some(space) = current {
            if self.space_regions.iter().any(|r| r.space == space && r.holds(ear)) {
                return current;
            }
        }
        self.space_regions.iter().find(|r| r.holds(ear)).map(|r| r.space)
    }

    /// Returns the authored attenuation gain for a 3D voice at `pos` relative to `listener_space`.
    pub fn voice_space_gain(&self, listener_space: Option<usize>, pos: Point) -> f64 {
        if self.attenuations.is_empty() {
            return 1.0;
        }
        let Some(from) = listener_space else {
            return 1.0;
        };
        let Some(to) = self.space_regions.iter().find(|r| r.holds(pos)).map(|r| r.space) else {
            return 1.0;
        };
        if to == from {
            return 1.0;
        }
        self.attenuations
            .iter()
            .find(|a| a.from_space == from && a.to_space == to)
            .map_or(1.0, |a| a.gain.clamp(0.0, 1.0))
    }

    /// Toggles all trigger regions matching `region_hash`.
    pub fn set_trigger_enabled(&mut self, region_hash: u32, enabled: bool) {
        for tr in &mut self.triggers {
            if tr.hash == region_hash {
                tr.enabled = enabled;
            }
        }
    }
}

/// Pure one-step blend from a raw measured Enhanced room toward an authored interior preset.
pub fn blend_interior_room(measured: Room, preset: SpacePreset) -> Room {
    let mut out = measured;
    let s = &preset.room;
    out.room = measured.room + (s.room * preset.send - measured.room) * INTERIOR_SPACE_BLEND;
    out.decay = measured.decay + (s.decay - measured.decay) * INTERIOR_SPACE_BLEND;
    out.hf_ratio = measured.hf_ratio + (s.hf_ratio - measured.hf_ratio) * INTERIOR_SPACE_BLEND;
    out.reflections = measured.reflections + (s.reflections - measured.reflections) * INTERIOR_SPACE_BLEND;
    out.reverb = measured.reverb + (s.reverb - measured.reverb) * INTERIOR_SPACE_BLEND;
    out.diffusion = measured.diffusion + (s.diffusion - measured.diffusion) * INTERIOR_SPACE_BLEND;
    out
}

impl Phase {
    /// Updates 2005 interior sound spaces and enhanced acoustics.
    /// In Classic mode, space presets directly drive `original_room` and `space_send`.
    /// In Enhanced mode, raycast acoustics keep their raw measured room untouched in
    /// `self.acoustics.room`, and `rendered_room` blends the active space preset once
    /// into the published output without compounding across ticks.
    pub(crate) fn swbf2_acoustics(&mut self, game_dt: f64, collision: &mut impl Collision) {
        let ear = self.engine.listener.ear;
        let (next_space, space_changed, preset) = {
            let Some(swbf2) = self.swbf2_mut() else { return };
            let next_space = swbf2.ambience.resolve_space(swbf2.ambience.current_space, ear);
            let space_changed = next_space != swbf2.ambience.current_space || !swbf2.ambience.space_set;
            if space_changed {
                swbf2.ambience.current_space = next_space;
                swbf2.ambience.space_set = true;
            }
            let preset = next_space
                .and_then(|id| swbf2.ambience.spaces.get(id).copied())
                .unwrap_or(swbf2.ambience.dry);
            (next_space, space_changed, preset)
        };
        if space_changed {
            self.space_send = preset.send;
            self.original_room = preset.room;
            self.original_room_mask = preset.mask;
            if !self.engine.enhanced {
                self.room_pending |= preset.mask;
            }
        }

        if self.engine.enhanced {
            if self.frame[f::COLLISION] != 0.0 {
                self.acoustics.update(game_dt, &mut self.engine, collision);
                if self.acoustics.room_changed {
                    self.room_pending = 2047;
                }
            }
            let measured = self.acoustics.room;
            if let Some(swbf2) = self.swbf2_mut() {
                if next_space.is_some() {
                    swbf2.ambience.rendered_room = blend_interior_room(measured, preset);
                } else {
                    swbf2.ambience.rendered_room = measured;
                }
            }
            if space_changed {
                self.room_pending = 2047;
            }
            self.acoustics.loudness(game_dt, &self.engine);
        }
    }

    /// Updates world ambient streams and script-gated `soundtrigger` regions.
    pub(crate) fn swbf2_ambience(&mut self, dt: f64) {
        self.ambience();
        let ear = self.engine.listener.ear;
        let n = self.swbf2().map_or(0, |s| s.ambience.triggers.len());
        for i in 0..n {
            let mut tr = self.swbf2().unwrap().ambience.triggers[i];
            let active = tr.enabled && tr.sound != NONE && tr.holds(ear);
            if active {
                let live = if tr.is_stream {
                    self.streams.playing(tr.voice)
                } else {
                    self.playing(tr.voice)
                };
                if live {
                    tr.was_active = true;
                    tr.retry.note_live();
                } else {
                    tr.voice = SILENT;
                    let pos = [tr.matrix[12], tr.matrix[13], tr.matrix[14]];
                    let want_start = if !tr.was_active {
                        true
                    } else {
                        self.swbf2_loop_should_retry(&mut tr.retry, dt, tr.sound, NONE, Some(pos), false)
                    };
                    if want_start {
                        let had_room = self.swbf2_loop_has_room(tr.sound, false);
                        let (voice, is_stream) = if is_stream_sound(&self.engine, tr.sound) {
                            let mut opts = Options::new(tr.sound, AMBIENCE);
                            opts.ask.position = Some(pos);
                            opts.ask.looping = Some(true);
                            opts.fade = 0.3;
                            (self.streams.play(&mut self.engine, opts), true)
                        } else {
                            (self.play_sound_at(tr.sound, AMBIENCE, NONE, Some(pos), Some(true)), false)
                        };
                        let now_live = if is_stream {
                            self.streams.playing(voice)
                        } else {
                            self.playing(voice)
                        };
                        tr.voice = if now_live { voice } else { SILENT };
                        tr.is_stream = is_stream;
                        tr.was_active = true;
                        tr.retry.record(now_live, had_room);
                    }
                }
                if let Some(swbf2) = self.swbf2_mut() {
                    swbf2.ambience.triggers[i].voice = tr.voice;
                    swbf2.ambience.triggers[i].is_stream = tr.is_stream;
                    swbf2.ambience.triggers[i].was_active = tr.was_active;
                    swbf2.ambience.triggers[i].retry = tr.retry;
                }
            } else if tr.was_active || tr.voice != SILENT {
                if tr.voice != SILENT {
                    if tr.is_stream {
                        self.streams.stop(&mut self.engine, tr.voice, 0.25);
                    } else {
                        self.stop(tr.voice, 0.25);
                    }
                }
                if let Some(swbf2) = self.swbf2_mut() {
                    swbf2.ambience.triggers[i].voice = SILENT;
                    swbf2.ambience.triggers[i].was_active = false;
                    swbf2.ambience.triggers[i].retry.reset();
                }
            }
        }
    }

    pub(crate) fn stop_swbf2_ambience(&mut self) {
        let n = self.swbf2().map_or(0, |s| s.ambience.triggers.len());
        for i in 0..n {
            let tr = self.swbf2().unwrap().ambience.triggers[i];
            if tr.voice != SILENT {
                if tr.is_stream {
                    self.streams.stop(&mut self.engine, tr.voice, 0.05);
                } else {
                    self.stop(tr.voice, 0.05);
                }
            }
            if let Some(swbf2) = self.swbf2_mut() {
                swbf2.ambience.triggers[i].voice = SILENT;
                swbf2.ambience.triggers[i].was_active = false;
                swbf2.ambience.triggers[i].retry.reset();
            }
        }
    }
}
