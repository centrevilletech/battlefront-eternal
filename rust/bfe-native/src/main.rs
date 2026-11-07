// bfe-native CAPTURE_DIR: replays a battle scene the page's Rust renderer drew
// (captured with ?rustRecord) through the same renderer on native OpenGL ES,
// headless (an EGL device, no window), and reports the CPU each frame costs
// there: the renderer's own thread, and the whole process (the GL driver's
// threads too). The same code the page runs in WASM, without the browser.
//
// env: W, H (framebuffer, default 1600x900), SAMPLES (default 2), SECS (20).
// HASH_FRAME optionally hashes a fixed captured frame after the timed run.
use bfe_native::{load_scene, Ranges};
use glow::HasContext;
use glutin::api::egl::{device::Device, display::Display};
use glutin::config::{ConfigSurfaceTypes, ConfigTemplateBuilder};
use glutin::context::{ContextApi, ContextAttributesBuilder, Version};
use glutin::display::GlDisplay;
use std::time::Instant;

mod cpu_time;
use cpu_time::{thread_cpu,process_cpu};

fn env(name: &str, fallback: f64) -> f64 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(fallback)
}

fn main() {
    let dir = std::env::args().nth(1).expect("usage: bfe-native CAPTURE_DIR");
    // A GLES 3.0 context on the first EGL device, current with no surface.
    let device = Device::query_devices().expect("no EGL devices").next().expect("no EGL device");
    eprintln!("EGL device: {:?} / {:?}", device.vendor(), device.name());
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
    let target = bfe_native::target::Target::new(&gl, w, h, samples).expect("target");
    target.bind(&gl);
    let (mut r, frames) = match load_scene(std::path::Path::new(&dir), gl, multi) {
        Ok(scene) => scene,
        Err(error) => { eprintln!("bfe-native: {error}"); std::process::exit(1); }
    };
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
        let frame = &frames[i % frames.len()];
        let (records, input, bones) = (&frame.records, &frame.input, &frame.bones);
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
    if let Ok(index) = std::env::var("HASH_FRAME") {
        let index: usize = index.parse().expect("HASH_FRAME index");
        target.bind(&r.core.gl);
        bfe_native::draw(&mut r, &frames[index]).expect("hash draw");
        println!("frame_hash {} {}", index, target.hash(&r.core.gl).expect("hash"));
    }

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
