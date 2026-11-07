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
    validate_capture(&meta, blob_bytes.len())?;
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
    unsafe {
        renderer.core.gl.clear_color(0.2, 0.2, 0.3, 1.0);
        renderer.core.gl.depth_mask(true);
        renderer.core.gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
    }
    renderer.frame(frame.records.min(bfe_render::instances::MAX_RECORDS)).map(|_| ())
}

/// Validate cold file spans before any GPU work. Offsets belong to the capture
/// file, not native pointers; rejecting a truncated blob avoids partial upload.
pub fn validate_capture(meta: &Value, bytes: usize) -> Result<(), String> {
    // The schema is deliberately small: calls, indexed blobs and frames.
    // It contains no native addresses and never trusts a file offset as a pointer.
    let fail = || "invalid or truncated renderer capture".to_string();
    let blobs = meta["blobs"].as_array().ok_or_else(fail)?;
    for blob in blobs {
        let at = blob[0].as_u64().ok_or_else(fail)?;
        let len = blob[1].as_u64().ok_or_else(fail)?;
        if at.checked_add(len).is_none_or(|end| end > bytes as u64) { return Err(fail()); }
    }
    // Alignment here is element width in the serialized blob, not alignment of
    // the original browser TypedArray. Endian conversion below handles that.
    let span = |id: &Value, align: u64| -> Result<(), String> {
        let index = id.as_u64().ok_or_else(fail)?;
        let blob = blobs.get(index as usize).ok_or_else(fail)?;
        if blob[1].as_u64().ok_or_else(fail)? % align != 0 { return Err(fail()); }
        Ok(())
    };
    // RGBA8 readback has an exact byte extent. A plausible width/height pair
    // with a short blob must fail before tex_image_2d can read its source.
    for texture in meta["textures"].as_array().ok_or_else(fail)? {
        if texture.is_null() { continue; }
        let w = texture["width"].as_u64().ok_or_else(fail)?;
        let h = texture["height"].as_u64().ok_or_else(fail)?;
        span(&texture["blob"], 4)?;
        let index = texture["blob"].as_u64().ok_or_else(fail)? as usize;
        if w == 0 || h == 0 || w > 16384 || h > 16384 || Some(blobs[index][1].as_u64().ok_or_else(fail)?) != w.checked_mul(h).and_then(|n| n.checked_mul(4)) {
            return Err(fail());
        }
    }
    let frames = meta["frames"].as_array().ok_or_else(fail)?;
    if frames.is_empty() { return Err("capture has no frames".into()); }
    for frame in frames {
        frame["records"].as_u64().ok_or_else(fail)?;
        span(&frame["input"], 8)?;
        span(&frame["bones"], 4)?;
    }
    for call in meta["calls"].as_array().ok_or_else(fail)? {
        let name = call[0].as_str().ok_or_else(fail)?;
        let args = call[1].as_array().ok_or_else(fail)?;
        // Check the arguments interpreted by this replay, including typed blob
        // widths. Other recorded registration families are reported as omitted.
        let pattern = match name {
            "add_texture" | "add_geometry" => "",
            "set_far_map" => "n", "add_owned_program" => "s",
            "add_program" => "nssss", "geometry_buffer" => "nx",
            "geometry_pointer" => "nnnnnbnn", "geometry_index" => "nxn",
            "geometry_group" => "nnn", "add_world" => "nnnnd",
            "world_texture" => "nnn", "world_uniform" => "nsfb",
            "world_cells" => "nfxn", "sort" => "", _ => "",
        };
        for (index, kind) in pattern.chars().enumerate() {
            let value = args.get(index).ok_or_else(fail)?;
            match kind {
                'n' => { value.as_f64().filter(|v| v.is_finite()).ok_or_else(fail)?; },
                's' => { value.as_str().ok_or_else(fail)?; },
                'b' => { value.as_bool().ok_or_else(fail)?; },
                'x' | 'f' | 'd' => span(&value["blob"], if kind == 'd' { 8 } else if kind == 'f' { 4 } else { 1 })?,
                _ => return Err(fail()),
            }
        }
        if name == "add_program" {
            for attribute in args[3].as_str().unwrap().split(',').filter(|s| !s.is_empty()) {
                if attribute.split_once(':').is_none_or(|(_, n)| n.parse::<u32>().is_err()) { return Err(fail()); }
            }
        }
    }
    Ok(())
}
