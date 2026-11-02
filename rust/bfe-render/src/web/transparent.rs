// The transparent pass's bindings (transparent.rs): where the page writes
// the keys of three's own remaining transparent items and reads the
// segments, the later segments' draws, and the map's blended buckets.
use super::BfeRenderer;
use crate::transparent::{KEY_STRIDE, MAX_THREE};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl BfeRenderer {
    // three's items' keys (f64: group order, render order, z, id each), in
    // three's order, before transparent_frame.
    pub fn transparent_keys_ptr(&mut self) -> u32 { self.r.transparent.keys.as_mut_ptr() as u32 }
    pub fn transparent_keys_len(&self) -> u32 { (MAX_THREE * KEY_STRIDE) as u32 }
    // After transparent_frame: where each segment starts (u32, three's count + 2).
    pub fn transparent_segments_ptr(&mut self) -> u32 { self.r.transparent.list.segments.as_mut_ptr() as u32 }
    pub fn transparent_segments_len(&self) -> u32 { self.r.transparent.list.segments.len() as u32 }
    // Segment k, right after three drew its item k - 1: its stats.
    pub fn transparent_segment(&mut self, k: u32) -> Result<Vec<u32>, JsValue> {
        Ok(self.r.transparent_segment(k as usize).map_err(|e| JsValue::from_str(&e))?.values())
    }

    // A blended bucket: blend as three's setBlending (enabled, equations,
    // factors), depth func, flags (1 depth test, 2 depth write, 4 colour
    // write), ids (mesh id, renderOrder), bounds (sphere, then box).
    #[allow(clippy::too_many_arguments)]
    pub fn transparent_bucket(&mut self, geometry: u32, blend: &[u32], depth_func: u32, flags: u32, point_lit: bool, ids: &[f64], bounds: &[f64]) -> u32 {
        self.r.transparent.add_bucket(geometry, blend, depth_func, flags, point_lit, ids, bounds)
    }
    pub fn transparent_bucket_pass(&mut self, b: u32, program: u32, side: u32) -> u32 { self.r.transparent.bucket_pass(b, program, side) }
    pub fn transparent_bucket_texture(&mut self, b: u32, unit: u32, texture: u32) { self.r.transparent.bucket_texture(&self.r.core, b, unit, texture) }
    pub fn transparent_bucket_uniform(&mut self, b: u32, pass: u32, name: &str, values: &[f32], int: bool) -> bool {
        self.r.transparent.bucket_uniform(&self.r.core, b, pass, name, values, int)
    }
    pub fn transparent_bucket_shown(&mut self, b: u32, shown: bool) { self.r.transparent.bucket_shown(b, shown) }
}
