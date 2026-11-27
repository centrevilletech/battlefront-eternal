//! Animation clips as src/anim.js loadClips decodes a bank (anims.json and
//! anims.bin, a droideka's own bank, a vehicle pack's animations, the props'
//! animations): per clip its frames, fps, tracks (remapped by bone name onto
//! the skeleton that plays it), its slice of the bank's floats, and the
//! numbers the Clip constructor derives (duration, root motion, stride,
//! speed). Getting up and diving last as long as their clips.

use crate::js::{js_hypot, js_max, relative_range, to_int32};
use crate::value::Value;
use crate::zero_hash;
use std::collections::HashMap;
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

/// Clips by name, in the bank's order.
#[derive(Clone, Debug, Default)]
pub struct Clips {
    pub list: Vec<Clip>,
}

impl Clips {
    /// Resolve a locomotion slot from the ordered per-character banks.
    pub fn swbf2_slot(&self, suffix: &str) -> Option<&Clip> {
        self.list.iter().rev().find(|clip| {
            let name = clip.name.strip_suffix("_full").unwrap_or(&clip.name);
            name.strip_suffix(suffix).is_some_and(|stem| stem.ends_with('_'))
        })
    }

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

/// loadClips(meta, buffer, boneNames).
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

/// 1987-1996: a walker bank holds a reference record and
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

/// Statistics from binding 2005 animation bank tracks onto a skeleton by bone CRC.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CrcBindStats {
    pub banks: Vec<String>,
    pub clips: usize,
    pub matched_tracks: usize,
    pub unmatched_tracks: usize,
}

impl CrcBindStats {
    pub fn total_tracks(&self) -> usize {
        self.matched_tracks + self.unmatched_tracks
    }

    /// Fraction of source tracks that did not match any bone on the target actor (`0.0..=1.0`).
    pub fn unbound_share(&self) -> f64 {
        let total = self.total_tracks();
        if total == 0 {
            0.0
        } else {
            self.unmatched_tracks as f64 / total as f64
        }
    }
}

/// Locomotion, posture, transition, air, and thrown clip suffixes queried by
/// the 2005 soldier animation table (`units::soldier_swbf2`) and `Defs::build`.
///
/// 2005 animation archives identify clips only by Zero engine CRC-32 hashes.
/// This list is limited strictly to the 40 slot suffixes that those table
/// builders actually query; each bank's stem (plus `_sabre` / `_rifle` and
/// optional `_full` suffix) is hashed on demand during bank loading.
pub const SOLDIER_SLOT_SUFFIXES: &[&str] = &[
    // Standing locomotion + alert
    "stand_idle_emote",
    "stand_idle_emote_alert",
    "stand_walkforward",
    "stand_runforward",
    "stand_runforward_alert",
    "stand_walkbackward",
    "stand_runbackward",
    "stand_runbackward_alert",
    "stand_turnleft",
    "stand_turnright",
    // Crouched locomotion
    "crouch_idle_emote",
    "crouch_walkforward",
    "crouch_walkbackward",
    "crouch_turnleft",
    "crouch_turnright",
    // Prone locomotion
    "prone_idle_emote",
    "prone_walkforward",
    "prone_walkbackward",
    "prone_turnleft",
    "prone_turnright",
    "prone_walkleft",
    "prone_walkright",
    // Posture transitions
    "stand_getdown_prone",
    "crouch_getdown_prone",
    "prone_getup_stand",
    "prone_getup_crouch",
    // Air, dive, and jetpack
    "jump",
    "fall",
    "landsoft",
    "landhard",
    "diveforward",
    "jetpack_hover",
    // Thrown reactions
    "thrown_flyingfront",
    "thrown_bouncefrontsoft",
    "thrown_landfrontsoft",
    "thrown_restfrontsoft",
    "thrown_flyingback",
    "thrown_bouncebacksoft",
    "thrown_landbacksoft",
    "thrown_restbacksoft",
];

/// Strips a trailing numeric bank partition suffix (e.g. `"human_0"` -> `"human"`).
pub fn bank_stem(bank_name: &str) -> String {
    let lower = bank_name.to_ascii_lowercase();
    if let Some((head, tail)) = lower.rsplit_once('_') {
        if !head.is_empty() && !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) {
            return head.to_string();
        }
    }
    lower
}

/// Builds a CRC-32 hex -> canonical slot clip name lookup for one 2005 bank,
/// combining the bank's stem with [`SOLDIER_SLOT_SUFFIXES`] (plain and `_full`).
fn bank_slot_clip_names(bank_name: &str) -> HashMap<String, String> {
    let stem = bank_stem(bank_name);
    let mut prefixes: Vec<String> = Vec::new();
    let mut push = |p: String| {
        if !p.is_empty() && !prefixes.contains(&p) {
            prefixes.push(p);
        }
    };
    if stem.ends_with("_sabre") || stem.ends_with("_rifle") {
        push(stem.clone());
    } else if stem.contains('_') {
        push(stem.clone());
        if stem.starts_with("human_") {
            push("human_sabre".to_string());
            push("human_rifle".to_string());
        }
    } else {
        push(format!("{stem}_sabre"));
        push(format!("{stem}_rifle"));
        push(stem.clone());
    }
    let mut map = HashMap::with_capacity(prefixes.len() * SOLDIER_SLOT_SUFFIXES.len() * 2);
    for prefix in &prefixes {
        for &suffix in SOLDIER_SLOT_SUFFIXES {
            let plain = format!("{prefix}_{suffix}");
            map.entry(zero_hash::crc_hex(&plain)).or_insert(plain);
            let full = format!("{prefix}_{suffix}_full");
            map.entry(zero_hash::crc_hex(&full)).or_insert(full);
        }
    }
    map
}

/// Binds 2005 animation banks (`anim_banks.json` + `anim_banks.bin`) onto an
/// actor's skeleton by Zero engine bone CRC-32.
///
/// Tracks bind strictly by bone CRC regardless of the actor's bone order,
/// duplicate tracks are kept in source order, and bones missing from a clip
/// stay unmapped so the pose sampler retains their bind pose.
pub fn load_clips_by_crc(
    meta: &Value,
    floats: &Rc<[f32]>,
    bank_names: &[String],
    bone_names: &[String],
    extra_clip_names: &HashMap<String, Vec<String>>,
) -> Result<(Clips, CrcBindStats), String> {
    let mut bone_by_crc: HashMap<String, f64> = HashMap::with_capacity(bone_names.len());
    for (i, name) in bone_names.iter().enumerate() {
        // Later duplicate wins, matching load_clips.
        bone_by_crc.insert(zero_hash::crc_hex(name), i as f64);
    }
    let root_bone = bone_names
        .iter()
        .position(|n| n.eq_ignore_ascii_case("dummyroot"))
        .map(|i| i as f64);
    let meta_obj = meta
        .as_object()
        .ok_or_else(|| "anim_banks.json: expected JSON object".to_string())?;
    let fps = match meta_obj.get("fps").filter(|v| !v.is_null()) {
        Some(v) => {
            let f = num(Some(v));
            if !f.is_finite() || f <= 0.0 {
                return Err(format!("anim_banks.json: invalid fps value {f}"));
            }
            f
        }
        None => 30.0,
    };
    let banks_obj = match meta_obj.get("banks") {
        None | Some(Value::Null) => return Ok((Clips::default(), CrcBindStats::default())),
        Some(v) => v
            .as_object()
            .ok_or_else(|| "anim_banks.json: banks must be a JSON object".to_string())?,
    };

    let mut out = Clips::default();
    let mut stats = CrcBindStats::default();
    for wanted in bank_names {
        let bank_entry = banks_obj.get(wanted).map(|v| (wanted.as_str(), v)).or_else(|| {
            banks_obj
                .entries()
                .find(|(k, _)| k.eq_ignore_ascii_case(wanted))
        });
        let Some((bank_name, bank)) = bank_entry else { continue };
        let bank_obj = bank
            .as_object()
            .ok_or_else(|| format!("bank {bank_name}: expected JSON object"))?;
        stats.banks.push(bank_name.to_string());
        let known = bank_slot_clip_names(bank_name);
        let clips = bank_obj
            .get("clips")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("bank {bank_name}: clips must be a JSON array"))?;
        for (clip_idx, desc) in clips.iter().enumerate() {
            let crc = desc
                .get("crc")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| format!("bank {bank_name} clip[{clip_idx}]: missing or non-string crc"))?
                .to_ascii_lowercase();
            let bank_crc_key = format!("{}:{crc}", bank_name.to_ascii_lowercase());
            let mut alias_names: Vec<String> = extra_clip_names
                .get(&bank_crc_key)
                .or_else(|| extra_clip_names.get(&crc))
                .cloned()
                .unwrap_or_default();
            if let Some(slot_name) = known.get(&crc) {
                if !alias_names.iter().any(|n| n == slot_name) {
                    alias_names.push(slot_name.clone());
                }
            }
            if alias_names.is_empty() {
                alias_names.push(crc.clone());
            }
            let raw_tracks = desc
                .get("tracks")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("bank {bank_name} clip {crc}: no tracks"))?;
            let mut tracks = Vec::with_capacity(raw_tracks.len());
            for t in raw_tracks {
                let mapped = match t {
                    Value::String(s) => bone_by_crc
                        .get(&s.to_ascii_lowercase())
                        .copied()
                        .unwrap_or(-1.0),
                    other => {
                        let idx = num(Some(other));
                        if idx.is_finite() && idx >= 0.0 && idx.fract() == 0.0 && (idx as usize) < bone_names.len() {
                            idx
                        } else {
                            -1.0
                        }
                    }
                };
                if mapped >= 0.0 {
                    stats.matched_tracks += 1;
                } else {
                    stats.unmatched_tracks += 1;
                }
                tracks.push(mapped);
            }
            let frames_v = desc.get("frames").cloned().unwrap_or(Value::Null);
            let frames = num(desc.get("frames"));
            if !frames.is_finite()
                || frames < 0.0
                || frames.fract() != 0.0
                || frames > (usize::MAX as f64)
            {
                return Err(format!(
                    "bank {bank_name} clip {crc}: invalid frames value {frames}"
                ));
            }
            let offset = num(desc.get("offset"));
            if !offset.is_finite()
                || offset < 0.0
                || offset.fract() != 0.0
                || offset > (usize::MAX as f64)
            {
                return Err(format!(
                    "bank {bank_name} clip {crc}: invalid offset value {offset}"
                ));
            }
            let fcount = frames as usize;
            let start = offset as usize;
            let sample_count = tracks
                .len()
                .checked_mul(fcount)
                .and_then(|n| n.checked_mul(7))
                .ok_or_else(|| {
                    format!(
                        "bank {bank_name} clip {crc}: sample count overflow (tracks={}, frames={fcount})",
                        tracks.len()
                    )
                })?;
            let end = start.checked_add(sample_count).ok_or_else(|| {
                format!(
                    "bank {bank_name} clip {crc}: buffer range overflow (offset={start}, samples={sample_count})"
                )
            })?;
            if end > floats.len() {
                return Err(format!(
                    "bank {bank_name} clip {crc}: clip range [{start}, {end}) exceeds anim_banks.bin ({} floats)",
                    floats.len()
                ));
            }
            if let Some(bad) = floats[start..end].iter().position(|v| !v.is_finite()) {
                return Err(format!(
                    "bank {bank_name} clip {crc}: non-finite sample float at index {bad}"
                ));
            }
            let duration = js_max(frames - 1.0, 0.0) / fps;
            let root_motion = (to_int32(num(desc.get("flags"))) & ROOT_MOTION) != 0;
            let stride = if let Some(travel) = desc.get("travel").and_then(Value::as_array) {
                let t0 = travel.first().map(|v| num(Some(v))).unwrap_or(0.0);
                let t2 = travel.get(2).map(|v| num(Some(v))).unwrap_or(0.0);
                if !(t0.is_finite() && t2.is_finite()) {
                    return Err(format!("bank {bank_name} clip {crc}: non-finite travel vector"));
                }
                js_hypot(&[t0, t2])
            } else if root_motion && fcount >= 2 && !tracks.is_empty() {
                let root_track = root_bone
                    .and_then(|rb| tracks.iter().position(|&t| t == rb))
                    .unwrap_or(0);
                let data = &floats[start..end];
                let stride_opt = root_track
                    .checked_mul(fcount)
                    .and_then(|n| n.checked_mul(7))
                    .and_then(|t_base| {
                        fcount
                            .checked_sub(1)
                            .and_then(|k| k.checked_mul(7))
                            .and_then(|k| t_base.checked_add(k))
                            .and_then(|last_base| {
                                last_base
                                    .checked_add(6)
                                    .filter(|&hi| hi < data.len())
                                    .map(|_| (t_base, last_base))
                            })
                    });
                if let Some((t_base, last_base)) = stride_opt {
                    let dx = (data[last_base + 4] - data[t_base + 4]) as f64;
                    let dz = (data[last_base + 6] - data[t_base + 6]) as f64;
                    js_hypot(&[dx, dz])
                } else {
                    0.0
                }
            } else {
                0.0
            };
            let speed = if duration > 0.0 { stride / duration } else { 0.0 };
            stats.clips += 1;
            for clip_name in alias_names {
                let clip = Clip {
                    name: clip_name,
                    fps,
                    frames: frames_v.clone(),
                    tracks: tracks.clone(),
                    floats: floats.clone(),
                    start,
                    end,
                    duration,
                    root_motion,
                    stride,
                    speed,
                };
                match out.list.iter_mut().find(|c| c.name == clip.name) {
                    Some(slot) => *slot = clip,
                    None => out.list.push(clip),
                }
            }
        }
    }
    Ok((out, stats))
}

/// Overlays `overlay` clips on top of `base` clips; matching names overwrite
/// earlier entries while preserving order.
pub fn merge_clips(base: &Clips, overlay: &Clips) -> Clips {
    let mut out = base.clone();
    for clip in &overlay.list {
        match out.list.iter_mut().find(|c| c.name == clip.name) {
            Some(slot) => *slot = clip.clone(),
            None => out.list.push(clip.clone()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    #[test]
    fn crc_binding_is_order_independent_and_preserves_duplicates() {
        let root_crc = zero_hash::crc_hex("DummyRoot");
        let spine_crc = zero_hash::crc_hex("bone_a_spine");
        let missing_crc = zero_hash::crc_hex("bone_absent");
        let jump_crc = zero_hash::crc_hex("human_rifle_jump");

        let meta = parse(&format!(
            r#"{{
                "fps": 30,
                "banks": {{
                    "human_1": {{
                        "clipCount": 1,
                        "clips": [{{
                            "crc": "{jump_crc}",
                            "flags": 1,
                            "frames": 2,
                            "bones": 4,
                            "offset": 0,
                            "tracks": ["{root_crc}", "{spine_crc}", "{spine_crc}", "{missing_crc}"]
                        }}]
                    }}
                }}
            }}"#
        ))
        .unwrap();

        // 4 tracks * 2 frames * 7 floats = 56 floats; root track moves +2m on Z.
        let mut samples = vec![0.0f32; 56];
        samples[3] = 1.0; // frame 0 quat w
        samples[10] = 1.0; // frame 1 quat w
        samples[13] = 2.0; // frame 1 pos z
        let floats: Rc<[f32]> = Rc::from(samples);

        let bones_a = vec!["DummyRoot".into(), "bone_a_spine".into(), "bone_head".into()];
        let bones_b = vec!["bone_head".into(), "bone_a_spine".into(), "DummyRoot".into()];

        let (clips_a, stats_a) = load_clips_by_crc(
            &meta,
            &floats,
            &["human_1".into()],
            &bones_a,
            &HashMap::new(),
        )
        .unwrap();
        let (clips_b, stats_b) = load_clips_by_crc(
            &meta,
            &floats,
            &["human_1".into()],
            &bones_b,
            &HashMap::new(),
        )
        .unwrap();

        let ca = clips_a.get("human_rifle_jump").expect("canonical clip name");
        let cb = clips_b.get("human_rifle_jump").expect("canonical clip name");
        assert_eq!(ca.tracks, vec![0.0, 1.0, 1.0, -1.0]);
        assert_eq!(cb.tracks, vec![2.0, 1.0, 1.0, -1.0]);
        assert_eq!(stats_a.matched_tracks, 3);
        assert_eq!(stats_a.unmatched_tracks, 1);
        assert_eq!(stats_b.matched_tracks, 3);
        assert_eq!(stats_b.unmatched_tracks, 1);
        assert!((ca.stride - 2.0).abs() < 1e-6);
        assert!((cb.stride - 2.0).abs() < 1e-6);
        assert!((stats_a.unbound_share() - 0.25).abs() < 1e-9);
    }

    #[test]
    fn crc_binding_rejects_huge_and_malformed_frames_without_panicking() {
        let root_crc = zero_hash::crc_hex("DummyRoot");
        let spine_crc = zero_hash::crc_hex("bone_a_spine");
        let floats: Rc<[f32]> = Rc::from(vec![0.0f32; 56]);
        let bones = vec!["DummyRoot".into(), "bone_a_spine".into()];

        for bad_frames in ["1e30", "4e18", "2.7e18", "1e400", "-2", "1.5", "100"] {
            let meta = parse(&format!(
                r#"{{
                    "fps": 30,
                    "banks": {{
                        "b1": {{
                            "clips": [{{
                                "crc": "01020304",
                                "flags": 1,
                                "frames": {bad_frames},
                                "offset": 0,
                                "tracks": ["{spine_crc}", "{root_crc}"]
                            }}]
                        }}
                    }}
                }}"#
            ))
            .unwrap();
            let err = load_clips_by_crc(&meta, &floats, &["b1".into()], &bones, &HashMap::new())
                .err()
                .unwrap_or_else(|| panic!("expected error for frames={bad_frames}"));
            assert!(
                err.contains("b1") && err.contains("01020304"),
                "error should name bank and clip for frames={bad_frames}: {err}"
            );
        }

        // Null clips array, numeric crc, and NaN sample floats must error cleanly.
        let null_clips = parse(r#"{"fps":30,"banks":{"b1":{"clips":null}}}"#).unwrap();
        assert!(load_clips_by_crc(&null_clips, &floats, &["b1".into()], &bones, &HashMap::new()).is_err());

        let num_crc = parse(&format!(
            r#"{{"fps":30,"banks":{{"b1":{{"clips":[{{"crc":5,"flags":0,"frames":1,"offset":0,"tracks":["{root_crc}"]}}]}}}}}}"#
        ))
        .unwrap();
        assert!(load_clips_by_crc(&num_crc, &floats, &["b1".into()], &bones, &HashMap::new()).is_err());

        let nan_floats: Rc<[f32]> = Rc::from(vec![f32::NAN; 14]);
        let valid_meta = parse(&format!(
            r#"{{"fps":30,"banks":{{"b1":{{"clips":[{{"crc":"01020304","flags":0,"frames":2,"offset":0,"tracks":["{root_crc}"]}}]}}}}}}"#
        ))
        .unwrap();
        let err = load_clips_by_crc(&valid_meta, &nan_floats, &["b1".into()], &bones, &HashMap::new())
            .err()
            .unwrap();
        assert!(err.contains("non-finite"), "expected non-finite sample error: {err}");
    }

    #[test]
    fn crc_binding_resolves_custom_bank_sabre_slots_and_preserves_extra_aliases() {
        let root_crc = zero_hash::crc_hex("DummyRoot");
        let idle_crc = zero_hash::crc_hex("quad_sabre_stand_idle_emote_full");
        let run_crc = zero_hash::crc_hex("quad_sabre_stand_runforward_full");
        let jump_crc = zero_hash::crc_hex("quad_sabre_jump_full");
        let meta = parse(&format!(
            r#"{{
                "fps": 30,
                "banks": {{
                    "quad": {{
                        "clips": [
                            {{"crc": "{idle_crc}", "flags": 0, "frames": 2, "offset": 0, "tracks": ["{root_crc}"]}},
                            {{"crc": "{run_crc}", "flags": 1, "frames": 2, "offset": 14, "tracks": ["{root_crc}"]}},
                            {{"crc": "{jump_crc}", "flags": 0, "frames": 2, "offset": 28, "tracks": ["{root_crc}"]}}
                        ]
                    }}
                }}
            }}"#
        ))
        .unwrap();
        let floats: Rc<[f32]> = Rc::from(vec![0.0f32; 42]);
        let bones = vec!["DummyRoot".into()];
        let mut extra = HashMap::new();
        extra.insert(format!("quad:{jump_crc}"), vec!["combo_leap".to_string()]);
        let (clips, _) = load_clips_by_crc(&meta, &floats, &["quad".into()], &bones, &extra).unwrap();
        assert!(clips.get("quad_sabre_stand_idle_emote_full").is_some());
        assert!(clips.get("quad_sabre_stand_runforward_full").is_some());
        // Both the combo alias and the canonical slot name are retained for the same CRC.
        assert!(clips.get("combo_leap").is_some());
        assert!(clips.get("quad_sabre_jump_full").is_some());
    }
}
