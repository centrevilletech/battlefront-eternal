//! Water.prepareSurface's draw values and sampler-frame selection.
//! Real GPU sampler identities enter once after resource registration. Every
//! view supplies camera/light facts; native retained rows resolve all animated
//! slots and quality/far rules without page-side record reconstruction.
use crate::effect_layout::{
    water_reflection as reflection, water_textured as textured, water_tint as tint,
};
use crate::layout::{surface_result as out, water_draw_view as input, water_view_result as policy};
use serde_json::{json, Value};
// Near and far anchors have independent retained rows despite shared images.
// Preparing one camera must not overwrite values kept by an earlier packet.
// Only cold sample registration owns dynamically sized resource-id tables.
pub struct Draws {
    pub input: [f64; input::STRIDE],
    values: [Vec<f64>; 6],
    normal: Vec<f64>,
    bump: Vec<f64>,
    mask: Vec<f64>,
    main: f64,
    normal_fallback: f64,
    mask_fallback: f64,
    pub ocean: bool,
}
// Negative sampler ids denote absent registered resources. The procedural
// normal and specular fallbacks are separate, actual native GPU resources.
impl Default for Draws {
    fn default() -> Self {
        Self {
            input: [0.; input::STRIDE],
            values: std::array::from_fn(|i| {
                vec![0.; [tint::LENGTH, reflection::LENGTH, textured::LENGTH][i % 3]]
            }),
            normal: vec![],
            bump: vec![],
            mask: vec![],
            main: -1.,
            normal_fallback: -1.,
            mask_fallback: -1.,
            ocean: false,
        }
    }
}
impl Draws {
    // Missing image resources remain a host fact. A native table keeps the
    // exact admitted sequence order and falls back only for an absent slot.
    pub fn samples(&mut self, data: &Value) {
        let ids = |key: &str| {
            data[key]
                .as_array()
                .into_iter()
                .flatten()
                .map(|n| n.as_f64().unwrap_or(-1.))
                .collect()
        };
        self.normal = ids("normal");
        self.bump = ids("bump");
        self.mask = ids("mask");
        self.main = data["main"].as_f64().unwrap_or(-1.);
        self.normal_fallback = data["normalFallback"].as_f64().unwrap_or(-1.);
        self.mask_fallback = data["maskFallback"].as_f64().unwrap_or(-1.);
    }
    // Stable slots name tint/reflection/textured for the near and far scenes.
    // Malformed ids clamp at the boundary without allocating another draw row.
    pub fn values(&mut self, slot: usize) -> &mut [f64] {
        &mut self.values[slot.min(5)]
    }
    // All three records begin with the same generated camera fields. These
    // matrices are observed facts; this owner does not update a browser camera
    // or derive another transform while preparing the upload packet.
    pub fn prepare(&mut self, slot: usize, result: &[f64], p: &[f64]) {
        let slot = slot.min(5);
        let kind = slot % 3;
        let far = slot >= 3;
        let f = &self.input;
        let v = &mut self.values[slot];
        v[tint::PROJECTION_MATRIX..tint::PROJECTION_MATRIX + 16]
            .copy_from_slice(&f[input::PROJECTION..input::PROJECTION + 16]);
        v[tint::VIEW_MATRIX..tint::VIEW_MATRIX + 16]
            .copy_from_slice(&f[input::INVERSE..input::INVERSE + 16]);
        v[tint::CAMERA_POSITION..tint::CAMERA_POSITION + 3]
            .copy_from_slice(&f[input::CAMERA + 12..input::CAMERA + 15]);
        // Sequence indices belong to the complete surface phase. Empty image
        // sequences retain the same fallback as a missing browser image did.
        let get = |ids: &[f64], index: f64, fallback: f64| {
            ids.get(index as usize).copied().unwrap_or(fallback)
        };
        // Each resource kind has its own generated record. The page never
        // tests for optional uniform fields or derives a sampler pair.
        // Tint can read opaque colour/depth independently of reflection. Its
        // underwater flag uses the gameplay level while display animation
        // remains the surface phase output shared by all water readers.
        if kind == 0 {
            v[tint::TIME] = result[out::TIME];
            v[tint::QUALITY] = p[policy::ENHANCED];
            v[tint::UNDERWATER] = p[policy::UNDERWATER];
            v[tint::TINT..tint::TINT + 3].copy_from_slice(&f[input::TINT..input::TINT + 3]);
            v[tint::CAMERA_RANGE..tint::CAMERA_RANGE + 2]
                .copy_from_slice(&f[input::CAMERA_RANGE..input::CAMERA_RANGE + 2]);
            v[tint::WADERS..tint::WADERS + 32]
                .copy_from_slice(&result[out::WADERS..out::WADERS + 32]);
            v[tint::SCENE_COLOR] = f[input::SCENE];
            v[tint::SCENE_DEPTH] = f[input::SCENE + 1];
            v[tint::RESOLUTION..tint::RESOLUTION + 2]
                .copy_from_slice(&f[input::RESOLUTION..input::RESOLUTION + 2]);
            if far {
                v[tint::SURFACE_RANGE..tint::SURFACE_RANGE + 2]
                    .copy_from_slice(&p[policy::RANGE..policy::RANGE + 2]);
                v[tint::FOG_COLOR..tint::FOG_COLOR + 3]
                    .copy_from_slice(&f[input::FOG_COLOR..input::FOG_COLOR + 3]);
                v[tint::FOG_RANGE..tint::FOG_RANGE + 2]
                    .copy_from_slice(&f[input::FOG_RANGE..input::FOG_RANGE + 2]);
            }
        // High screen-space and Ultra planar reflection share animation and
        // texture interpolation. Only the native policy bit changes the mode;
        // a second browser material must not introduce another frame clock.
        } else if kind == 1 {
            v[reflection::TIME] = result[out::TIME];
            v[reflection::QUALITY] = p[policy::ENHANCED];
            v[reflection::SCREEN_SPACE] =
                f64::from(p[policy::ENHANCED] != 0. && p[policy::PLANAR] == 0.);
            v[reflection::REFLECTION] = f[input::REFLECTION];
            v[reflection::DISTORT] = p[policy::DISTORT];
            v[reflection::VIEW_YAW] = result[out::YAW];
            v[reflection::SUN_COLOR..reflection::SUN_COLOR + 3]
                .copy_from_slice(&f[input::SUN_COLOR..input::SUN_COLOR + 3]);
            v[reflection::REFLECTION_MATRIX..reflection::REFLECTION_MATRIX + 16]
                .copy_from_slice(&f[input::REFLECTION_MATRIX..input::REFLECTION_MATRIX + 16]);
            // Keep each selected pair and its cross-fade together. Bump uses
            // the normal fallback deliberately, preserving its neutral sample.
            for (at, ids, source, fallback) in [
                (
                    reflection::NORMAL_A,
                    &self.normal,
                    out::NORMAL_A,
                    self.normal_fallback,
                ),
                (
                    reflection::BUMP_A,
                    &self.bump,
                    out::BUMP_A,
                    self.normal_fallback,
                ),
            ] {
                v[at] = get(ids, result[source], fallback);
                v[at + 1] = get(ids, result[source + 1], fallback);
                v[at + 2] = result[source + 2];
            }
            v[reflection::TO_SUN..reflection::TO_SUN + 3]
                .copy_from_slice(&result[out::SUN..out::SUN + 3]);
            v[reflection::CAMERA_RANGE..reflection::CAMERA_RANGE + 2]
                .copy_from_slice(&f[input::CAMERA_RANGE..input::CAMERA_RANGE + 2]);
            v[reflection::WADERS..reflection::WADERS + 32]
                .copy_from_slice(&result[out::WADERS..out::WADERS + 32]);
            v[reflection::SCENE_COLOR] = f[input::SCENE];
            v[reflection::SCENE_DEPTH] = f[input::SCENE + 1];
            v[reflection::RESOLUTION..reflection::RESOLUTION + 2]
                .copy_from_slice(&f[input::RESOLUTION..input::RESOLUTION + 2]);
            if far {
                v[reflection::SURFACE_RANGE..reflection::SURFACE_RANGE + 2]
                    .copy_from_slice(&p[policy::RANGE..policy::RANGE + 2]);
                v[reflection::FOG_COLOR..reflection::FOG_COLOR + 3]
                    .copy_from_slice(&f[input::FOG_COLOR..input::FOG_COLOR + 3]);
                v[reflection::FOG_RANGE..reflection::FOG_RANGE + 2]
                    .copy_from_slice(&f[input::FOG_RANGE..input::FOG_RANGE + 2]);
            }
        // Low water has its independent specular-mask sequence. Resource ids
        // survive quality changes, so toggling does not readmit authored images
        // or change which mask frame follows the retained surface clock.
        } else {
            v[textured::TIME] = result[out::TIME];
            v[textured::MAIN_MAP] = self.main;
            v[textured::MASK_A] = get(&self.mask, result[out::MASK_A], self.mask_fallback);
            v[textured::MASK_B] = get(&self.mask, result[out::MASK_A + 1], self.mask_fallback);
            v[textured::MASK_MIX] = result[out::MASK_A + 2];
            v[textured::TO_SUN..textured::TO_SUN + 3]
                .copy_from_slice(&result[out::SUN..out::SUN + 3]);
            v[textured::CAMERA_RANGE..textured::CAMERA_RANGE + 2]
                .copy_from_slice(&f[input::CAMERA_RANGE..input::CAMERA_RANGE + 2]);
            if far {
                v[textured::SURFACE_RANGE..textured::SURFACE_RANGE + 2]
                    .copy_from_slice(&p[policy::RANGE..policy::RANGE + 2]);
                v[textured::FOG_COLOR..textured::FOG_COLOR + 3]
                    .copy_from_slice(&f[input::FOG_COLOR..input::FOG_COLOR + 3]);
                v[textured::FOG_RANGE..textured::FOG_RANGE + 2]
                    .copy_from_slice(&f[input::FOG_RANGE..input::FOG_RANGE + 2]);
            }
        }
    }
    // Water.applyQuality previously chose both the preset and pass visibility.
    // Keep the original high default for unrecognised settings and the ocean
    // early return, while returning only numeric policy to the resource host.
    // The page supplies setting strings and actual resource availability.
    // Preset numbers and anchor visibility belong to the native water policy.
    // Enhanced ocean selection keeps the original planar-target early return.
    pub fn mode(&mut self, data: &Value) -> Value {
        let quality = data["quality"].as_str().unwrap_or("high");
        let (scale, max_size, planar) = match quality {
            "low" => (0., 512., false),
            "ultra" => (1., 2048., true),
            _ => (0.5, 1024., false),
        };
        let enhanced = scale > 0.;
        let low = data["low"].as_bool().unwrap_or(false);
        self.ocean = data["hasOcean"].as_bool().unwrap_or(false)
            && (data["oceanAlways"].as_bool().unwrap_or(false)
                || data["oceanEffects"].as_str() == Some("enhanced"));
        json!({"quality":{"scale":scale,"maxSize":max_size,"planarNear":planar},"ocean":self.ocean,"low":!enhanced,"surface":enhanced||!low,"enhanced":enhanced,"planar":planar})
    }
}
#[cfg(target_arch = "wasm32")]
// This interface exposes disjoint, retained input/output rows. Preparation
// allocates nothing and sends no serialized descriptor through a frame path.
pub(crate) mod exports {
    use super::*;
    use std::cell::RefCell;
    thread_local! {pub(crate) static DRAWS:RefCell<Draws>=RefCell::new(Draws::default());}
    #[no_mangle]
    pub extern "C" fn water_draw_input() -> *mut f64 {
        DRAWS.with_borrow_mut(|d| d.input.as_mut_ptr())
    }
    #[no_mangle]
    pub extern "C" fn water_draw_values(slot: u32) -> *mut f64 {
        DRAWS.with_borrow_mut(|d| d.values(slot as usize).as_mut_ptr())
    }
    #[no_mangle]
    pub extern "C" fn water_draw_samples() -> u32 {
        let Ok(data) = crate::content::input() else {
            return 1;
        };
        DRAWS.with_borrow_mut(|d| d.samples(&data));
        crate::content::output(&Value::Null);
        0
    }
    // Shader values need scalar range/quality policy, not another box scan.
    // Visibility has already visited the admitted cells at its own boundary;
    // repeating it for each surface pass would multiply work on slow machines.
    pub(crate) fn prepare(slot: usize, result: &[f64]) {
        DRAWS.with_borrow_mut(|d| {
            let policy =
                crate::surface_policy::draw_values(d.input[input::EYE_Y], d.input[input::FAR]);
            d.prepare(slot, result, &policy)
        });
    }
    pub(crate) fn mode(data: &Value) -> Value {
        DRAWS.with_borrow_mut(|d| d.mode(data))
    }
    #[no_mangle]
    pub extern "C" fn water_draw_ocean() -> u32 {
        DRAWS.with_borrow(|d| u32::from(d.ocean))
    }
}
