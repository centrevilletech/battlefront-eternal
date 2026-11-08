//! GpuEffects track admission and AnalyticParticleBatch's retained spawn banks.
//! All texture batches publish together. Only identity changes rewrite births;
//! a moving emitter changes its small table without touching particle arrays.
//!
//! Spawn state stays in instance buffers. Authored tracks and the much smaller
//! array of emitter transforms/times are read in the vertex shader. One draw per
//! texture/blend, including effects with different tracks. No simulation readback.
//!
//! Track admission is immutable content policy. Unsupported geometry, blur
//! blending and tracks longer than eight keys remain ordinary native particles.
//! The page does not truncate them or choose a second analytic implementation.
//! The host's definition map retains object identities; this owner returns the
//! numeric track row for every admitted definition in authored visitation order.
//!
//! A track row is sixty-four RGBA texels. Its first three texels describe key
//! counts, shape and flipbook timing. Position keys start at column four, size
//! at twenty and colour at thirty-six. Each key occupies an operation/time
//! texel followed by a value texel. Colour components divide by 255 before the
//! float32 assignment; scalar tracks retain zero in their unused components.
//!
//! Emitter slots are the current native presentation output order. They are
//! not stable effect ids. Particle identity therefore includes both its serial
//! and its current slot, plus the clock epoch used to encode its birth.
//! Removing an earlier effect changes the surviving particle's slot and forces
//! that record to be repacked even if its serial has not changed.
//!
//! Time rebases every 256 seconds. Both emitter time and particle birth subtract
//! the same double epoch before float32 storage, preserving small ages during
//! long sessions. Moving an emitter only updates its twelve-float row. Stable
//! particles keep their birth arrays untouched until one identity component
//! changes. The shader remains responsible for all analytic travel and fading.
//!
//! Every texture batch has a fixed admitted particle prefix. Excess records
//! clip at that prefix; they cannot grow a buffer or alter another batch.
//! Dirty is the earliest changed record in this publication. Unread also retains
//! a pending upload from an earlier publication until the renderer consumes it.
//! A zero-count publication retires identities, so a later reused index cannot
//! appear unchanged merely because its old serial is still in reserved memory.
//!
//! Native arrays are borrowed by retained draw resources. Their addresses may
//! change during cold admission, which must precede the producer's borrowed
//! frame views. Display updates neither admit banks nor allocate transfer views.

//!
//! The host may cache a receipt for a serial/slot/epoch triple to avoid copying
//! unchanged native facts through JavaScript. This cache cannot advance age or
//! choose the epoch; emitter publication supplies the latter before receipt use.
//! The native identity bank remains authoritative for whether packing is dirty.
//!
//! Unused output tails retain their old bytes, just as retained GPU attributes
//! did. They are excluded by the active prefix rather than cleared every frame.
//! Only retired identity entries need clearing when a prefix shrinks.
//!
//! The emitter table admits at most the shader's 1024 slots. Count is clamped
//! before reading the native input, and a particle's slot is bounded again
//! before indexing epochs. A stale or excessive foreign count cannot cross into
//! another bank. Batch capacity is similarly enforced before spawn publication.
//!
//! The reserved banks are sized during resource admission. Long burning effects
//! reuse the same addresses and ordinary frames allocate no vectors or views.

use crate::{
    layout::effect as e,
    visual_effects_layout::{
        analytic_arrays as a, analytic_batch as b, analytic_emitter as o, analytic_frame as f,
        analytic_spawn as s, visual_limits as l,
    },
    Key, Transformer, Value,
};
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Geometry {
    r#type: String,
    blend: String,
    texture: Option<String>,
    spark_length: Option<f64>,
    time_per_frame: Option<f64>,
    frame_size: Option<f64>,
    total_frames: Option<f64>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Definition {
    #[serde(default)]
    register_step: bool,
    geometry: Option<Geometry>,
    transformer: Option<Transformer>,
}
#[derive(Clone, Copy)]
struct Identity {
    serial: f64,
    slot: f64,
    epoch: f64,
}
impl Default for Identity {
    fn default() -> Self {
        Self {
            serial: f64::NAN,
            slot: f64::NAN,
            epoch: f64::NAN,
        }
    }
}
pub struct Analytic {
    pub input: Vec<f64>,
    pub emitters: Vec<f64>,
    pub data: Vec<f32>,
    pub epochs: Vec<f64>,
    pub definitions: Vec<i32>,
    pub tracks: Vec<f32>,
    pub batches: Vec<f64>,
    pub spawns: Vec<f64>,
    pub arrays: Vec<f32>,
    pub track_rows: usize,
    identity: Vec<Identity>,
    previous_counts: Vec<usize>,
    emitters_ready: bool,
}
impl Analytic {
    // GpuEffects.constructor's eligibility and track encoding happen once.
    // A long or unsupported authored track stays in the existing native CPU
    // particle path; no truncated analytic definition is admitted.
    pub fn new(defs: Vec<Definition>) -> Self {
        let mut definitions = vec![-1; defs.len()];
        let mut admitted = 0;
        for (k, def) in defs.iter().enumerate() {
            if admitted == l::ANALYTIC_DEFINITIONS {
                break;
            }
            let (Some(g), Some(t)) = (&def.geometry, &def.transformer) else {
                continue;
            };
            if g.blend == "blur"
                || g.texture.as_ref().is_none_or(|s| s.is_empty())
                || !["particle", "animated", "spark", "streak", "billboard"]
                    .contains(&g.r#type.as_str())
                || t.position.len() > l::ANALYTIC_KEYS
                || t.size.len() > l::ANALYTIC_KEYS
                || t.color.len() > l::ANALYTIC_KEYS
            {
                continue;
            }
            definitions[k] = admitted as i32;
            admitted += 1;
        }
        let track_rows = admitted.max(1).next_power_of_two();
        let mut tracks = vec![0.; l::ANALYTIC_WIDTH * track_rows * 4];
        for (k, def) in defs.iter().enumerate() {
            let row = definitions[k];
            if row < 0 {
                continue;
            }
            let base = row as usize * l::ANALYTIC_WIDTH * 4;
            let g = def.geometry.as_ref().unwrap();
            let t = def.transformer.as_ref().unwrap();
            tracks[base] = t.position.len() as f32;
            tracks[base + 1] = t.size.len() as f32;
            tracks[base + 2] = t.color.len() as f32;
            tracks[base + 4] = if g.r#type == "billboard" {
                2.
            } else if ["spark", "streak"].contains(&g.r#type.as_str()) {
                1.
            } else {
                0.
            };
            tracks[base + 5] = if def.register_step { 0. } else { 1. };
            tracks[base + 6] = g.spark_length.filter(|v| *v != 0.).unwrap_or(0.1) as f32;
            tracks[base + 8] = if g.r#type == "animated" {
                g.time_per_frame.unwrap_or(0.) as f32
            } else {
                0.
            };
            tracks[base + 9] =
                v8math::round(g.frame_size.filter(|v| *v != 0.).unwrap_or(1.)).max(1.) as f32;
            tracks[base + 10] =
                v8math::round(g.total_frames.filter(|v| *v != 0.).unwrap_or(1.)).max(1.) as f32;
            encode(&mut tracks, base + 4 * 4, &t.position, 0);
            encode(&mut tracks, base + 20 * 4, &t.size, 1);
            encode(&mut tracks, base + 36 * 4, &t.color, 2);
        }
        Self {
            input: vec![0.; f::STRIDE],
            emitters: vec![0.; l::ANALYTIC_EFFECTS * e::STRIDE],
            data: vec![0.; l::ANALYTIC_EFFECTS * o::STRIDE],
            epochs: vec![0.; l::ANALYTIC_EFFECTS],
            definitions,
            tracks,
            batches: Vec::new(),
            spawns: Vec::new(),
            arrays: Vec::new(),
            track_rows,
            identity: Vec::new(),
            previous_counts: Vec::new(),
            emitters_ready: false,
        }
    }
    pub fn add_batch(&mut self) -> usize {
        let id = self.batches.len() / b::STRIDE;
        if id >= l::ANALYTIC_BATCHES {
            return usize::MAX;
        }
        self.batches.extend_from_slice(&[0., f64::INFINITY, 0.]);
        self.previous_counts.push(0);
        self.spawns
            .resize((id + 1) * l::ANALYTIC_PARTICLES * s::STRIDE, 0.);
        self.arrays.resize((id + 1) * a::STRIDE, 0.);
        self.identity
            .resize((id + 1) * l::ANALYTIC_PARTICLES, Identity::default());
        id
    }
    pub fn emitters(&mut self) {
        let count = (self.input[f::EMITTERS] as usize).min(l::ANALYTIC_EFFECTS);
        for slot in 0..count {
            let src = &self.emitters[slot * e::STRIDE..][..e::STRIDE];
            let dst = &mut self.data[slot * o::STRIDE..][..o::STRIDE];
            for k in 0..3 {
                dst[o::POSITION + k] = src[e::POSITION + k] as f32;
            }
            dst[o::TINT] = src[e::TINT] as f32;
            let time = src[e::TIME];
            let epoch = (time / 256.).floor() * 256.;
            self.epochs[slot] = epoch;
            for k in 0..4 {
                dst[o::QUATERNION + k] = src[e::QUATERNION + k] as f32;
            }
            dst[o::TIME] = (time - epoch) as f32;
            dst[o::SCALE] = src[e::SCALE] as f32;
        }
        self.emitters_ready = true;
    }
    // The emitter publication precedes borrowed spawn facts. Hosts can skip
    // an unchanged resource record using this already-native epoch, while
    // this owner remains responsible for packing and dirty/upload spans.
    pub fn frame(&mut self) {
        if !self.emitters_ready {
            self.emitters();
        }
        self.emitters_ready = false;
        for id in 0..self.batches.len() / b::STRIDE {
            let header = &mut self.batches[id * b::STRIDE..][..b::STRIDE];
            let count = (header[b::COUNT] as usize).min(l::ANALYTIC_PARTICLES);
            let mut dirty = f64::INFINITY;
            for index in 0..count {
                let src =
                    &self.spawns[(id * l::ANALYTIC_PARTICLES + index) * s::STRIDE..][..s::STRIDE];
                let slot = (src[s::SLOT] as usize).min(l::ANALYTIC_EFFECTS - 1);
                let epoch = self.epochs[slot];
                let serial = src[s::SERIAL];
                let previous = &mut self.identity[id * l::ANALYTIC_PARTICLES + index];
                if previous.serial == serial
                    && previous.slot == src[s::SLOT]
                    && previous.epoch == epoch
                {
                    continue;
                }
                *previous = Identity {
                    serial,
                    slot: src[s::SLOT],
                    epoch,
                };
                dirty = dirty.min(index as f64);
                let dst = &mut self.arrays[id * a::STRIDE..][..a::STRIDE];
                for k in 0..3 {
                    dst[a::POSITION + index * 3 + k] = src[s::POSITION + k] as f32;
                    dst[a::VELOCITY + index * 3 + k] = src[s::VELOCITY + k] as f32;
                }
                for k in 0..4 {
                    dst[a::COLOR + index * 4 + k] = src[s::COLOR + k] as f32;
                }
                let at = index * 4;
                dst[a::SHAPE + at] = src[s::SIZE] as f32;
                dst[a::SHAPE + at + 1] = src[s::ROTATION] as f32;
                dst[a::SHAPE + at + 2] = src[s::SPIN] as f32;
                dst[a::SHAPE + at + 3] = 0.;
                dst[a::META + at] = (src[s::BIRTH] - epoch) as f32;
                dst[a::META + at + 1] = src[s::LIFE] as f32;
                dst[a::META + at + 2] = src[s::SLOT] as f32;
                dst[a::META + at + 3] = src[s::DEFINITION] as f32;
            }
            // Dead references were trimmed in AnalyticParticleBatch.end. A reused
            // index must not see a matching id from a particle no longer retained.
            if count < self.previous_counts[id] {
                for identity in &mut self.identity[id * l::ANALYTIC_PARTICLES + count
                    ..id * l::ANALYTIC_PARTICLES + self.previous_counts[id]]
                {
                    *identity = Identity::default();
                }
            }
            self.previous_counts[id] = count;
            header[b::COUNT] = count as f64;
            header[b::DIRTY] = dirty;
            header[b::UNREAD] = header[b::UNREAD].min(dirty);
        }
    }
}
// GpuEffects.put's operation ordinals and colour normalization are preserved.
// Missing scalar/vector components remain zero in the admitted track texels.
fn encode(out: &mut [f32], base: usize, keys: &[Key], kind: usize) {
    for (k, key) in keys.iter().enumerate() {
        let (op, value) = if kind == 0 {
            if let Some(v) = key.accelerate {
                (1, v)
            } else if let Some(v) = key.reach {
                (2, v)
            } else if let Some(v) = key.scale {
                (3, Value::Scalar(v))
            } else {
                (0, Value::Scalar(0.))
            }
        } else if kind == 1 {
            if let Some(v) = key.scale {
                (1, Value::Scalar(v))
            } else if let Some(v) = key.r#move {
                (2, v)
            } else if let Some(v) = key.reach {
                (3, v)
            } else {
                (0, Value::Scalar(0.))
            }
        } else if let Some(v) = key.reach {
            (1, v)
        } else if let Some(v) = key.r#move {
            (2, v)
        } else {
            (0, Value::Scalar(0.))
        };
        let at = base + k * 8;
        out[at] = key.lifetime as f32;
        out[at + 1] = op as f32;
        let (v, count) = match value {
            Value::Scalar(v) => ([v, 0., 0., 0.], 1),
            Value::Vector(v) => ([v[0], v[1], v[2], 0.], 3),
            Value::Color(v) => (v, 4),
        };
        for j in 0..count {
            out[at + 4 + j] = (if kind == 2 { v[j] / 255. } else { v[j] }) as f32;
        }
    }
}
