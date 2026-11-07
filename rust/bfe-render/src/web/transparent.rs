// The transparent pass's bindings (transparent.rs): where the page writes
// the keys of three's own remaining transparent items and reads the
// segments, the later segments' draws, and the map's blended buckets.
use super::BfeRenderer;
use crate::transparent::{KEY_STRIDE, MAX_THREE, OBJ_STRIDE};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl BfeRenderer {
    // three's items' keys (f64: group order, render order, z, id each), in
    // three's order, before transparent_frame.
    pub fn transparent_keys_ptr(&mut self) -> u32 { self.r.transparent.keys.as_mut_ptr() as u32 }
    pub fn transparent_keys_len(&self) -> u32 { (MAX_THREE * KEY_STRIDE) as u32 }
    // Native sorting publishes segment boundaries for diagnostics.
    pub fn transparent_segments_ptr(&mut self) -> u32 { self.r.transparent.list.segments.as_mut_ptr() as u32 }
    pub fn transparent_segments_len(&self) -> u32 { self.r.transparent.list.segments.len() as u32 }

    // A blended bucket: blend as three's setBlending (enabled, equations,
    // factors), depth func, flags (1 depth test, 2 depth write, 4 colour
    // write), bounds (its box, min then max).
    pub fn transparent_bucket(&mut self, geometry: u32, blend: &[u32], depth_func: u32, flags: u32, point_lit: bool, bounds: &[f64]) -> u32 {
        self.remember_transparent_bucket(geometry, blend, depth_func, flags, point_lit, bounds); // recovery
 self.r.transparent.add_bucket(geometry, blend, depth_func, flags, point_lit, bounds)
    }
    pub fn transparent_bucket_pass(&mut self, b: u32, program: u32, side: u32) -> u32 {
        self.remember_transparent_bucket_pass(b, program, side); // recovery
 self.r.transparent.bucket_pass(b, program, side) }
    pub fn transparent_bucket_texture(&mut self, b: u32, unit: u32, texture: u32) {
        self.remember_transparent_bucket_texture(b, unit, texture); // recovery
 self.r.transparent.bucket_texture(&self.r.core, b, unit, texture) }
    pub fn transparent_bucket_uniform(&mut self, b: u32, pass: u32, name: &str, values: &[f32], int: bool) -> bool {
        self.remember_transparent_bucket_uniform(b, pass, name, values, int); // recovery
 self.r.transparent.bucket_uniform(&self.r.core, b, pass, name, values, int)
    }

    // The records the page writes each frame (f64: the count, then
    // OBJ_STRIDE each: a bucket's or an object's), and objects drawn with
    // three's basic or sprite material (kind 0 or 1): registration, a pass
    // and the map.
    pub fn transparent_objects_ptr(&mut self) -> u32 { self.r.transparent.objects.as_mut_ptr() as u32 }
    pub fn transparent_objects_len(&self) -> u32 { self.r.transparent.objects.len() as u32 }
    pub fn transparent_object_stride(&self) -> u32 { OBJ_STRIDE as u32 }
    #[allow(clippy::too_many_arguments)]
    pub fn transparent_object(&mut self, kind: u32, geometry: u32, start: i32, count: i32, blend: &[u32], depth_func: u32, flags: u32) -> u32 {
        self.remember_transparent_object(kind, geometry, start, count, blend, depth_func, flags); // recovery
 self.r.transparent.add_object(kind, geometry, start, count, blend, depth_func, flags)
    }
    pub fn transparent_object_pass(&mut self, o: u32, program: u32, side: u32) {
        self.remember_transparent_object_pass(o, program, side); // recovery
 self.r.transparent.object_pass(&self.r.core, o, program, side) }
    pub fn transparent_object_texture(&mut self, o: u32, texture: u32) {
        self.remember_transparent_object_texture(o, texture); // recovery
 self.r.transparent.object_texture(&self.r.core, o, texture) }
}
