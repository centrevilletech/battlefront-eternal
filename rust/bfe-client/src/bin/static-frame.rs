//! One admitted component in a fixed offscreen target, for browser comparison.
//! The regular client exercises the same recipes in its window and frame loop.
use glutin::{
    config::{ConfigSurfaceTypes, ConfigTemplateBuilder},
    context::{ContextApi, ContextAttributesBuilder, Version},
    prelude::*,
};
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let battle = args.get(5).is_some_and(|arg| arg == "--battle");
    if args.len() != 5 && !(args.len() == 6 && battle) {
        return Err("usage: static-frame CONTENT MAP FRAME_JSON OUTPUT_RGBA COMPONENT [--battle]".into());
    }
    let input: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&args[2]).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let size: [i32; 2] =
        serde_json::from_value(input["size"].clone()).map_err(|e| e.to_string())?;
    if size.iter().any(|n| !(1..=8192).contains(n)) {
        return Err("invalid frame viewport".into());
    }
    let values: Vec<f64> =
        serde_json::from_value(input["values"].clone()).map_err(|e| e.to_string())?;
    if values.len() != bfe_render::shader::FRAME_FLOATS || values.iter().any(|v| !v.is_finite()) {
        return Err("invalid static frame bank".into());
    }
    let device = glutin::api::egl::device::Device::query_devices()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("no EGL device")?;
    let display = unsafe { glutin::api::egl::display::Display::with_device(&device, None) }
        .map_err(|e| e.to_string())?;
    let template = ConfigTemplateBuilder::new()
        .with_surface_type(ConfigSurfaceTypes::empty())
        .build();
    let config = unsafe { display.find_configs(template) }
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("no EGL config")?;
    let attrs = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::Gles(Some(Version::new(3, 0))))
        .build(None);
    let context = unsafe { display.create_context(&config, &attrs) }.map_err(|e| e.to_string())?;
    let _context = context
        .make_current_surfaceless()
        .map_err(|e| e.to_string())?;
    let mut load = || unsafe {
        glow::Context::from_loader_function(|name| {
            display
                .get_proc_address(&std::ffi::CString::new(name).unwrap())
                .cast()
        })
    };
    let random: Option<Vec<f64>> =
        serde_json::from_value(input["foliageRandom"].clone()).map_err(|e| e.to_string())?;
    // Battle admission retains the regular client's shadows and material
    // variants. The default inspector still accepts recorded foliage draws.
    let (mut scene, _) = if battle {
        bfe_client::scene::Scene::load_online(
            std::path::Path::new(&args[0]),
            &format!("map={}", args[1]),
            size,
            &mut load,
        )?
    } else {
        bfe_client::scene::Scene::load_with_random(
            std::path::Path::new(&args[0]),
            &args[1],
            size,
            random.as_deref(),
            &mut load,
        )?
    };
    let mut view = bfe_client::scene::View {
        values: [0.; bfe_render::shader::FRAME_FLOATS],
    };
    view.values.copy_from_slice(&values);
    scene.draw_component(&view, &args[4])?;
    std::fs::write(&args[3], scene.pixels()?).map_err(|e| e.to_string())?;
    println!(
        "{}",
        serde_json::json!({"hash":scene.hash()?,"admitted":scene.admitted,"deferred":scene.deferred,"draws":scene.draws,"bindings":scene.bindings(),"schedule":scene.frame_trace()})
    );
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
