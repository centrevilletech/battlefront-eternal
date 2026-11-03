// bfe-native CAPTURE_DIR: replays a battle scene the page's Rust renderer drew
// (captured with ?rustRecord) through the same renderer on native OpenGL ES,
// headless (an EGL device, no window), and reports the CPU each frame costs
// there: the renderer's own thread, and the whole process (the GL driver's
// threads too). The same code the page runs in WASM, without the browser.
//
// env: W, H (framebuffer, default 1600x900), SAMPLES (default 2), SECS (20).
use bfe_render::core::MultiDraw;
use bfe_render::renderer::Renderer;
use bfe_render::shader::Kind;
use glow::HasContext;
use glutin::api::egl::{device::Device, display::Display};
use glutin::config::{ConfigSurfaceTypes, ConfigTemplateBuilder};
use glutin::context::{ContextApi, ContextAttributesBuilder, Version};
use glutin::display::GlDisplay;
use serde_json::Value;
use std::time::Instant;

// No multi-draw in OpenGL ES 3.0: the ranges one at a time (each a cheap
// call natively).
struct Ranges(glow::Context);

impl MultiDraw for Ranges {
    fn multi_draw(&self, mode: u32, counts: &mut [i32], element_type: u32, offsets: &mut [i32]) {
        for (c, o) in counts.iter().zip(offsets.iter()) {
            unsafe { self.0.draw_elements(mode, *c, element_type, *o) };
        }
    }
}

fn thread_cpu() -> f64 {
    let mut t = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t) };
    t.tv_sec as f64 * 1e3 + t.tv_nsec as f64 / 1e6
}

fn process_cpu() -> f64 {
    let mut u: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut u) };
    let ms = |t: libc::timeval| t.tv_sec as f64 * 1e3 + t.tv_usec as f64 / 1e3;
    ms(u.ru_utime) + ms(u.ru_stime)
}

fn env(name: &str, fallback: f64) -> f64 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(fallback)
}

fn main() {
    let dir = std::env::args().nth(1).expect("usage: bfe-native CAPTURE_DIR");
    let meta: Value = serde_json::from_slice(&std::fs::read(format!("{dir}/calls.json")).unwrap()).unwrap();
    let blob_bytes = std::fs::read(format!("{dir}/blobs.bin")).unwrap();
    let blobs = meta["blobs"].as_array().unwrap();
    let blob = |i: usize| -> &[u8] {
        let b = &blobs[i];
        let (at, len) = (b[0].as_u64().unwrap() as usize, b[1].as_u64().unwrap() as usize);
        &blob_bytes[at..at + len]
    };
    let cast = |bytes: &[u8], size: usize| -> Vec<u8> { assert!(bytes.len() % size == 0); bytes.to_vec() };
    let f32s = |bytes: &[u8]| -> Vec<f32> { bytes.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect() };
    let u32s = |bytes: &[u8]| -> Vec<u32> { bytes.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect() };
    let f64s = |bytes: &[u8]| -> Vec<f64> { bytes.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect() };

    // A GLES 3.0 context on the first EGL device, current with no surface.
    let device = Device::query_devices().expect("no EGL devices").next().expect("no EGL device");
    eprintln!("EGL device: {:?}", device.drm_render_device_node_path());
    let display = unsafe { Display::with_device(&device, None) }.expect("EGL display");
    let template = ConfigTemplateBuilder::new().with_surface_type(ConfigSurfaceTypes::empty()).build();
    let config = unsafe { display.find_configs(template) }.expect("configs").next().expect("no config");
    let attributes = ContextAttributesBuilder::new().with_context_api(ContextApi::Gles(Some(Version::new(3, 0)))).build(None);
    let context = unsafe { display.create_context(&config, &attributes) }.expect("context");
    let _current = context.make_current_surfaceless().expect("make current");
    let load = |name: &std::ffi::CStr| display.get_proc_address(name) as *const _;
    let gl = unsafe { glow::Context::from_loader_function_cstr(load) };
    let multi = Ranges(unsafe { glow::Context::from_loader_function_cstr(load) });
    unsafe {
        eprintln!("GL: {} / {}", gl.get_parameter_string(glow::VERSION), gl.get_parameter_string(glow::RENDERER));
    }

    // The frame's target, as the page's near scene draws into.
    let (w, h, samples) = (env("W", 1600.0) as i32, env("H", 900.0) as i32, env("SAMPLES", 2.0) as i32);
    unsafe {
        let fb = gl.create_framebuffer().unwrap();
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fb));
        let color = gl.create_renderbuffer().unwrap();
        gl.bind_renderbuffer(glow::RENDERBUFFER, Some(color));
        gl.renderbuffer_storage_multisample(glow::RENDERBUFFER, samples, glow::RGBA8, w, h);
        gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::RENDERBUFFER, Some(color));
        let depth = gl.create_renderbuffer().unwrap();
        gl.bind_renderbuffer(glow::RENDERBUFFER, Some(depth));
        gl.renderbuffer_storage_multisample(glow::RENDERBUFFER, samples, glow::DEPTH_COMPONENT24, w, h);
        gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, glow::DEPTH_ATTACHMENT, glow::RENDERBUFFER, Some(depth));
        assert_eq!(gl.check_framebuffer_status(glow::FRAMEBUFFER), glow::FRAMEBUFFER_COMPLETE);
        gl.viewport(0, 0, w, h);
    }

    // The textures, as read back from the page (level 0, RGBA8), mipmapped here.
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
    let mut texture_next = 0usize;
    let start = Instant::now();
    for call in meta["calls"].as_array().unwrap() {
        let name = call[0].as_str().unwrap();
        let a = call[1].as_array().unwrap();
        let n = |i: usize| a[i].as_f64().unwrap();
        let b = |i: usize| blob(a[i]["blob"].as_u64().unwrap() as usize);
        match name {
            "add_texture" => {
                let (id, t) = created.iter().find(|(id, _)| *id == texture_next).copied().expect("texture");
                assert_eq!(r.core.add_texture(t) as usize, id);
                texture_next += 1;
            }
            "set_far_map" => r.core.set_far_map(n(0) as u32),
            "add_program" => {
                let kind = match n(0) as u32 { 1 => Kind::Skinned, 2 => Kind::Rigid, 3 => Kind::Plain, _ => Kind::World };
                let attrs: Vec<(String, u32)> = a[3].as_str().unwrap().split(',').filter(|s| !s.is_empty())
                    .map(|s| { let (k, v) = s.split_once(':').unwrap(); (k.to_string(), v.parse().unwrap()) }).collect();
                let uniforms: Vec<String> = a[4].as_str().unwrap().split(',').filter(|s| !s.is_empty()).map(String::from).collect();
                let id = r.core.add_program(kind, a[1].as_str().unwrap(), a[2].as_str().unwrap(), &attrs, &uniforms).unwrap_or_else(|e| panic!("program: {e}"));
                assert_eq!(id as f64, call[2].as_f64().unwrap());
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
            "sort" => r.world.sort(a[0].as_bool().unwrap()),
            "world_runs" => r.world.world_runs(n(0) as u32, &u32s(b(1)), &f64s(b(2))),
            "world_shown" => r.world.world_shown(n(0) as u32, a[1].as_bool().unwrap()),
            other => eprintln!("skipped call {other}"),
        }
    }
    eprintln!("scene registered in {:.0} ms", start.elapsed().as_secs_f64() * 1e3);

    // The captured frames, over and over, two frames in flight at most.
    let frames: Vec<(usize, Vec<f64>, Vec<f32>)> = meta["frames"].as_array().unwrap().iter()
        .map(|f| (f["records"].as_u64().unwrap() as usize, f64s(blob(f["input"].as_u64().unwrap() as usize)), f32s(blob(f["bones"].as_u64().unwrap() as usize))))
        .collect();
    let secs = env("SECS", 20.0);
    // The driver compiles shader variants as new state first shows up:
    // measure only after a warm-up.
    let warm = env("WARM", 0.0);
    let (mut cpu, mut wall) = (vec![], vec![]);
    let mut fences = std::collections::VecDeque::new();
    let run_start = Instant::now();
    let mut process_start = process_cpu();
    let mut measured_start = Instant::now();
    let mut calls = 0u64;
    let mut i = 0usize;
    let mut warming = warm > 0.0;
    while run_start.elapsed().as_secs_f64() < warm + secs {
        if warming && run_start.elapsed().as_secs_f64() >= warm {
            warming = false;
            cpu.clear();
            wall.clear();
            calls = 0;
            i = 0;
            process_start = process_cpu();
            measured_start = Instant::now();
        }
        let (records, input, bones) = &frames[i % frames.len()];
        r.core.input[..input.len()].copy_from_slice(input);
        r.core.bones[..bones.len()].copy_from_slice(bones);
        let (t0, w0) = (thread_cpu(), Instant::now());
        unsafe {
            r.core.gl.clear_color(0.2, 0.2, 0.3, 1.0);
            r.core.gl.depth_mask(true);
            r.core.gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        }
        let stats = r.frame(*records).unwrap();
        unsafe { r.core.gl.flush() };
        cpu.push(thread_cpu() - t0);
        calls += stats.gl_calls as u64;
        if std::env::var("FINISH").is_ok() {
            unsafe { r.core.gl.finish() };
        } else { unsafe {
            fences.push_back(r.core.gl.fence_sync(glow::SYNC_GPU_COMMANDS_COMPLETE, 0).unwrap());
            if fences.len() > 2 {
                let f = fences.pop_front().unwrap();
                r.core.gl.client_wait_sync(f, glow::SYNC_FLUSH_COMMANDS_BIT, 1_000_000_000);
                r.core.gl.delete_sync(f);
            }
        } }
        wall.push(w0.elapsed().as_secs_f64() * 1e3);
        i += 1;
    }
    let process = process_cpu() - process_start;
    let elapsed = measured_start.elapsed().as_secs_f64() * 1e3;
    let slow = wall.iter().filter(|&&w| w > 50.0).count();
    let stat = |v: &mut Vec<f64>| { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); (v[v.len() / 2], v[v.len() * 9 / 10], v.iter().sum::<f64>() / v.len() as f64) };
    let (c50, c90, cmean) = stat(&mut cpu);
    let (w50, w90, wmean) = stat(&mut wall);
    let last = r.core.stats;
    println!("frames {} in {:.1} s ({:.0} fps), {}x{} x{} samples", i, elapsed / 1e3, i as f64 / elapsed * 1e3, w, h, samples);
    println!("last frame: world draws {}, instanced draws {}, instances {}, program switches {}, GL calls {} (mean {:.0})",
        last.world_draws, last.instance_draws, last.instances, last.program_switches, last.gl_calls, calls as f64 / i as f64);
    println!("renderer thread CPU a frame: p50 {c50:.3} p90 {c90:.3} mean {cmean:.3} ms");
    println!("wall a frame (incl. waiting on the GPU): p50 {w50:.3} p90 {w90:.3} mean {wmean:.3} ms, {slow} frames over 50 ms");
    println!("process CPU a frame (all threads, driver included): {:.3} ms", process / i as f64);
}
