//! Capture replay shared by the headless renderer and windowed client.
//! Registration is cold work; the retained input/bone rows allocate nothing on draw.
use bfe_render::{core::MultiDraw, renderer::Renderer, shader::Kind};
use glow::HasContext;
use serde_json::Value;
use std::path::Path;
pub mod target;

// No multi-draw in OpenGL ES 3.0: the ranges one at a time (each a cheap
// call natively).
pub struct Ranges(pub glow::Context);

impl MultiDraw for Ranges {
    fn multi_draw(&self, mode: u32, counts: &mut [i32], element_type: u32, offsets: &mut [i32]) {
        for (c, o) in counts.iter().zip(offsets.iter()) {
            unsafe { self.0.draw_elements(mode, *c, element_type, *o) };
        }
    }
}


/// Retained snapshots of the page renderer input. These are admitted once;
/// neither native host clones them as the capture repeats.
/// Records are bounded by the same renderer capacity used by the page.
pub struct Frame {
    pub records: usize,
    pub input: Vec<f64>,
    pub bones: Vec<f32>,
}

// Registration order is part of the capture: geometry, texture and program
// numbers in a frame refer to earlier allocations in this exact sequence.
// This is the original bfe-native near replay, shared rather than reimplemented
// by the window host. Other page phases remain outside this capture contract.
pub fn load_scene(dir: &Path, gl: glow::Context, multi: Ranges) -> Result<(Renderer<glow::Context, Ranges>, Vec<Frame>), String> {
    let meta: Value = serde_json::from_slice(&std::fs::read(dir.join("calls.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let blob_bytes = std::fs::read(dir.join("blobs.bin")).map_err(|e| e.to_string())?;
    validate_capture(&meta, &blob_bytes)?;
    let blobs = meta["blobs"].as_array().unwrap();
    // JSON keeps only offsets into one binary file. Checking all spans before
    // constructing slices prevents a truncated transfer reaching GPU upload.
    let blob = |i: usize| -> &[u8] {
        let b = &blobs[i];
        let (at, len) = (b[0].as_u64().unwrap() as usize, b[1].as_u64().unwrap() as usize);
        &blob_bytes[at..at + len]
    };
    // Explicit little-endian decoding makes the capture independent of native
    // alignment. These temporary vectors exist only during cold registration.
    let cast = |bytes: &[u8], size: usize| -> Vec<u8> { assert!(bytes.len() % size == 0); bytes.to_vec() };
    let f32s = |bytes: &[u8]| -> Vec<f32> { bytes.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect() };
    let u32s = |bytes: &[u8]| -> Vec<u32> { bytes.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect() };
    let f64s = |bytes: &[u8]| -> Vec<f64> { bytes.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect() };

    // The textures, as read back from the page (level 0, RGBA8), mipmapped here.
    // The original replay stores level-zero RGBA8, then regenerates mipmaps.
    // Preserve that behavior for both hosts rather than adopting the window
    // context's resource defaults or a separate native image-loading policy.
    let textures = meta["textures"].as_array().unwrap();
    let make_texture = |id: usize| unsafe {
        let t = gl.create_texture().unwrap();
        gl.bind_texture(glow::TEXTURE_2D, Some(t));
        let info = &textures[id];
        let (tw, th) = (info["width"].as_i64().unwrap() as i32, info["height"].as_i64().unwrap() as i32);
        let px = blob(info["blob"].as_u64().unwrap() as usize);
        gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, tw, th, 0, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(Some(px)));
        gl.generate_mipmap(glow::TEXTURE_2D);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR_MIPMAP_LINEAR as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::REPEAT as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::REPEAT as i32);
        t
    };
    let mut created = vec![];
    for id in 0..textures.len() {
        if !textures[id].is_null() {
            created.push((id, make_texture(id)));
        }
    }
    let mut r = Renderer::new(gl, multi);

    // The page's registration calls, in order.
    // The recorder also observes registration for passes this near-only tool
    // never executes. Report that scope once, rather than implying whole-frame
    // capture support or flooding output with one message per omitted object.
    let mut ignored = std::collections::BTreeSet::new();
    let mut texture_next = 0usize;

    for call in meta["calls"].as_array().unwrap() {
        let name = call[0].as_str().unwrap();
        let a = call[1].as_array().unwrap();
        let n = |i: usize| a[i].as_f64().unwrap();
        let b = |i: usize| blob(a[i]["blob"].as_u64().unwrap() as usize);
        match name {
            "add_texture" => {
                let (id, t) = created.iter().find(|(id, _)| *id == texture_next).copied().ok_or("capture texture id is missing")?;
                if r.core.add_texture(t) as usize != id { return Err("capture texture id differs".into()); }
                texture_next += 1;
            }
            "set_far_map" => r.core.set_far_map(n(0) as u32),
            // Owned keys resolve through the same Rust catalog on both hosts.
            // A shader rejection is fatal; there is no browser compiler path.
            "add_owned_program" => {
                let id = r.core.add_owned_program(a[0].as_str().unwrap())?;
                if Some(id as f64) != call[2].as_f64() { return Err("capture program id differs".into()); }
            }
            "add_program" => {
                let kind = match n(0) as u32 { 1 => Kind::Skinned, 2 => Kind::Rigid, 3 => Kind::Plain, 4 => Kind::Effect, _ => Kind::World };
                let attrs: Vec<(String, u32)> = a[3].as_str().unwrap().split(',').filter(|s| !s.is_empty())
                    .map(|s| { let (k, v) = s.split_once(':').unwrap(); (k.to_string(), v.parse().unwrap()) }).collect();
                let uniforms: Vec<String> = a[4].as_str().unwrap().split(',').filter(|s| !s.is_empty()).map(String::from).collect();
                let id = r.core.add_program(kind, a[1].as_str().unwrap(), a[2].as_str().unwrap(), &attrs, &uniforms)?;
                if Some(id as f64) != call[2].as_f64() { return Err("capture program id differs".into()); }
            }
            "add_geometry" => { r.core.add_geometry(); }
            "geometry_buffer" => { r.core.geometry_buffer(n(0) as u32, &cast(b(1), 1)); }
            "geometry_pointer" => r.core.geometry_pointer(n(0) as u32, n(1) as u32, n(2) as u32, n(3) as i32, n(4) as u32, a[5].as_bool().unwrap(), n(6) as i32, n(7) as i32),
            "geometry_index" => r.core.geometry_index(n(0) as u32, b(1), n(2) as u32),
            "geometry_group" => r.core.geometry_group(n(0) as u32, n(1) as i32, n(2) as i32),
            "add_world" => { r.world.add_world(n(0) as u32, n(1) as u32, n(2) as u32, n(3) as u32, &f64s(b(4))); }
            "world_texture" => r.world.world_texture(&r.core, n(0) as u32, n(1) as u32, n(2) as u32),
            "world_uniform" => { r.world.world_uniform(&r.core, n(0) as u32, a[1].as_str().unwrap(), &f32s(b(2)), a[3].as_bool().unwrap()); }
            "world_cells" => r.world.world_cells(n(0) as u32, &f32s(b(1)), &u32s(b(2)), n(3)),
            // wasm-bindgen coerces an omitted sort argument to false. JSON
            // records undefined as null, so replay must retain that coercion.
            "sort" => r.world.sort(a[0].as_bool().unwrap_or(false)),
            other => { ignored.insert(other); },
        }
    }


    if !ignored.is_empty() { eprintln!("near replay omits registrations: {ignored:?}"); }
    // Frame rows and bone arrays retain their capacity for the entire run.
    // Reject oversized storage before the first frame, then clamp the record
    // count exactly where the renderer itself bounds instance traversal.
    let frames = meta["frames"].as_array().unwrap().iter().map(|f| Frame {
        records: (f["records"].as_u64().unwrap() as usize).min(bfe_render::instances::MAX_RECORDS),
        input: f64s(blob(f["input"].as_u64().unwrap() as usize)),
        bones: f32s(blob(f["bones"].as_u64().unwrap() as usize)),
    }).collect::<Vec<_>>();
    if frames.is_empty() { return Err("capture has no frames".into()); }
    // Empty captures cannot produce a timed run. Validate each frame
    // independently so a later truncated frame cannot hide behind frame zero.
    for frame in &frames {
        if frame.input.len() > r.core.input.len() || frame.bones.len() > r.core.bones.len() {
            return Err("capture exceeds renderer storage".into());
        }
    }
    Ok((r, frames))
}

/// Apply the page's captured near-frame input, keeping the original clear colour.
// The frame loop borrows these slices; copies go directly into renderer-owned
// banks. No per-frame serialization, file read or temporary record list exists.
// Both hosts use the same clear depth and the captured input arithmetic.
pub fn draw(renderer: &mut Renderer<glow::Context, Ranges>, frame: &Frame) -> Result<(), String> {
    if frame.input.len() > renderer.core.input.len() || frame.bones.len() > renderer.core.bones.len() {
        return Err("presentation frame exceeds renderer storage".into());
    }
    renderer.core.input[..frame.input.len()].copy_from_slice(&frame.input);
    renderer.core.bones[..frame.bones.len()].copy_from_slice(&frame.bones);
    use bfe_render::frame_plan::layout::{facts as F,meta as M,binding as B,role as R,kind as K};
    use bfe_render::post_chain::layout::{facts as P,program as Programs};
    // The capture contains a near-view packet, not another native draw API.
    // The ordinary planner skips absent views and owns its clear/draw order.
    renderer.frame_reserve();renderer.frame_begin();renderer.frames.plan.facts.fill(0.);renderer.frames.plan.facts[F::MAIN]=1.;
    renderer.post_chain.input[P::PROGRAMS..P::PROGRAMS+Programs::COUNT].fill(u32::MAX as f64);
    let mut viewport=[0;4];unsafe{renderer.core.gl.get_parameter_i32_slice(glow::VIEWPORT,&mut viewport);}
    let at=R::NEAR*M::LEN;renderer.frames.meta[at+M::KIND]=K::SCENE as f64;
    renderer.frames.meta[at+M::RECORDS]=frame.records.min(bfe_render::instances::MAX_RECORDS) as f64;
    renderer.frames.meta[at+M::BONES]=(frame.bones.len()/16) as f64;
    let b=&mut renderer.frames.bindings[R::NEAR*B::LEN..(R::NEAR+1)*B::LEN];b[B::TARGET]=-1.;
    b[B::X]=viewport[0] as f64;b[B::Y]=viewport[1] as f64;b[B::WIDTH]=viewport[2] as f64;b[B::HEIGHT]=viewport[3] as f64;
    b[B::CLEAR..B::CLEAR+4].copy_from_slice(&[0.2,0.2,0.3,1.]);
    if !renderer.frame_publish(R::NEAR){return Err("capture packet exceeds renderer storage".into());}
    renderer.battle_frame(&mut BoundTarget)

}

// The EGL/window caller already owns the bound framebuffer. This diagnostic
// imports no page texture targets, copies, probes or multisample resolves.
struct BoundTarget;
impl bfe_render::frame::FrameTargets for BoundTarget {
    fn bind(&mut self,_binding:&[f64])->Result<(),String>{Ok(())}
    fn resolve(&mut self,_id:i32)->Result<(),String>{Ok(())}
    fn copy(&mut self,_from:i32,_to:i32,_mask:u32)->Result<(),String>{Ok(())}
    fn mipmap(&mut self,_sample:i32)->Result<(),String>{Ok(())}
}

mod capture;
pub use capture::validate_capture;
