//! Animation clips as src/anim.js loadClips decodes a bank (anims.json and
//! anims.bin, a droideka's own bank, a vehicle pack's animations, the props'
//! animations): per clip its frames, fps, tracks (remapped by bone name onto
//! the skeleton that plays it), its slice of the bank's floats, and the
//! numbers the Clip constructor derives (duration, root motion, stride,
//! speed). Getting up and diving last as long as their clips (units.js).

use crate::js::{js_hypot, js_max, relative_range, to_int32};
use crate::value::Value;
use std::rc::Rc;

pub const STRIDE: f64 = 7.0;
const ROOT_MOTION: i32 = 1;

#[derive(Clone, Debug)]
pub struct Clip {
    pub name: String,
    pub fps: f64,
    /// `desc.frames` as the record holds it.
    pub frames: Value,
    /// Bone index per track (-1: a bone the skeleton lacks).
    pub tracks: Vec<f64>,
    /// The bank's floats and this clip's `[start, end)` in them.
    pub floats: Rc<[f32]>,
    pub start: usize,
    pub end: usize,
    pub duration: f64,
    pub root_motion: bool,
    pub stride: f64,
    pub speed: f64,
}

impl Clip {
    pub fn data(&self) -> &[f32] {
        &self.floats[self.start..self.end]
    }

    pub fn frames_f64(&self) -> f64 {
        num(Some(&self.frames))
    }
}

/// Clips by name, in the bank's order (a JavaScript Map).
#[derive(Clone, Debug, Default)]
pub struct Clips {
    pub list: Vec<Clip>,
}

impl Clips {
    pub fn get(&self, name: &str) -> Option<&Clip> {
        self.list.iter().find(|c| c.name == name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Clip> {
        self.list.iter_mut().find(|c| c.name == name)
    }
}

fn num(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => *n,
        Some(other) => crate::js::to_number(Some(other)),
        None => f64::NAN,
    }
}

/// An array element by a number index, `array[i]` (undefined unless an
/// integer in range).
fn element(list: &[f64], i: f64) -> Option<f64> {
    if i.is_nan() || i.fract() != 0.0 || i < 0.0 || i >= list.len() as f64 {
        None
    } else {
        Some(list[i as usize])
    }
}

/// src/anim.js loadClips(meta, buffer, boneNames).
pub fn load_clips(meta: &Value, floats: &Rc<[f32]>, bone_names: Option<&[String]>) -> Result<Clips, String> {
    // lookup: lower-case name -> index (a later duplicate wins).
    let lookup = bone_names.map(|names| {
        let mut map: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
        for (i, name) in names.iter().enumerate() {
            map.insert(name.to_lowercase(), i as f64);
        }
        map
    });
    let remap: Option<Vec<f64>> = match (&lookup, meta.get("bones").filter(|b| b.truthy())) {
        (Some(lookup), Some(bones)) => {
            let bones = bones.as_array().ok_or("meta.bones is not an array")?;
            Some(
                bones
                    .iter()
                    .map(|name| name.as_str().and_then(|n| lookup.get(&n.to_lowercase()).copied()).unwrap_or(-1.0))
                    .collect(),
            )
        }
        _ => None,
    };
    let fps = num(meta.get("fps"));
    let clips = meta.get("clips").and_then(Value::as_object).ok_or("meta.clips is not an object")?;
    let mut out = Clips::default();
    for (name, desc) in clips.entries() {
        let raw: Vec<f64> = desc.get("tracks").and_then(Value::as_array).ok_or_else(|| format!("clip {name}: no tracks"))?.iter().map(|t| num(Some(t))).collect();
        let tracks: Vec<f64> = match &remap {
            Some(r) => raw.iter().map(|&i| element(r, i).unwrap_or(-1.0)).collect(),
            None => raw,
        };
        let frames_v = desc.get("frames").cloned().unwrap_or(Value::Null);
        let frames = num(desc.get("frames"));
        let offset = num(desc.get("offset"));
        let (start, end) = relative_range(floats.len(), offset, Some(offset + tracks.len() as f64 * frames * STRIDE));
        let duration = js_max(frames - 1.0, 0.0) / fps;
        let root_motion = (to_int32(num(desc.get("flags"))) & ROOT_MOTION) != 0;
        let travel = desc.get("travel").and_then(Value::as_array).ok_or_else(|| format!("clip {name}: no travel"))?;
        let t0 = travel.first().map(|v| num(Some(v))).unwrap_or(f64::NAN);
        let t2 = travel.get(2).map(|v| num(Some(v))).unwrap_or(f64::NAN);
        let stride = js_hypot(&[t0, t2]);
        let speed = if duration > 0.0 { stride / duration } else { 0.0 };
        let clip = Clip { name: name.to_string(), fps, frames: frames_v, tracks, floats: floats.clone(), start, end, duration, root_motion, stride, speed };
        // Map.set: a repeated name keeps its first place (object keys are
        // unique, so it never happens).
        match out.list.iter_mut().find(|c| c.name == clip.name) {
            Some(slot) => *slot = clip,
            None => out.list.push(clip),
        }
    }
    Ok(out)
}

/// src/vehicles.js 1987-1996: a walker bank holds a reference record and
/// then the animated record for the same joint; only the last drives it.
pub fn keep_last_tracks(clips: &mut Clips) {
    for clip in &mut clips.list {
        let mut seen: Vec<f64> = Vec::new();
        for i in (0..clip.tracks.len()).rev() {
            let joint = clip.tracks[i];
            // Set membership is SameValueZero.
            if seen.iter().any(|&s| s == joint || (s.is_nan() && joint.is_nan())) {
                clip.tracks[i] = -1.0;
            } else {
                seen.push(joint);
            }
        }
    }
}
