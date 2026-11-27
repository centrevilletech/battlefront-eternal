//! Draw a sequence of fixed-tick battle observations into GPU framebuffers.
#[path = "battle_screens/recording.rs"]
mod recording;
use bfe_battle_ui::battle2::Menu;
use bfe_client::{
    app::{
        battle::screens::{device_event, Presentation, ScreenArt},
        settings::{current_desktop_paths, AppSettingsStore},
        shell::ShellHost,
    },
    input::{BattleInput, InputTrigger},
};
use bfe_family::GameFamily;
use bfe_render::{hud_model2::catalog::child, interface::anchor};
use glow::HasContext;
use serde::Deserialize;
use std::{
    ffi::CStr,
    fs::File,
    io::{BufWriter, Read},
    path::{Path, PathBuf},
    time::Instant,
};

#[global_allocator]
static ALLOCATOR: bfe_test_alloc::Counting = bfe_test_alloc::Counting::new();

#[cfg(target_os = "linux")]
fn context() -> Result<impl Fn(&CStr) -> *const std::ffi::c_void, String> {
    use glutin::{
        api::egl::{device::Device, display::Display},
        config::{ConfigSurfaceTypes, ConfigTemplateBuilder},
        context::{ContextApi, ContextAttributesBuilder, Version},
        prelude::*,
    };
    for device in Device::query_devices().map_err(|e| e.to_string())? {
        let Ok(display) = (unsafe { Display::with_device(&device, None) }) else {
            continue;
        };
        let template = ConfigTemplateBuilder::new()
            .with_surface_type(ConfigSurfaceTypes::empty())
            .build();
        let Some(config) = (unsafe { display.find_configs(template) })
            .ok()
            .and_then(|mut c| c.next())
        else {
            continue;
        };
        for api in [
            ContextApi::Gles(Some(Version::new(3, 0))),
            ContextApi::OpenGl(Some(Version::new(3, 3))),
        ] {
            let attrs = ContextAttributesBuilder::new()
                .with_context_api(api)
                .build(None);
            if let Some(ctx) = (unsafe { display.create_context(&config, &attrs) })
                .ok()
                .and_then(|c| c.make_current_surfaceless().ok())
            {
                return Ok(move |name: &CStr| {
                    let _ = &ctx;
                    display.get_proc_address(name)
                });
            }
        }
    }
    Err("no surfaceless GPU context".into())
}
#[cfg(not(target_os = "linux"))]
fn context() -> Result<fn(&CStr) -> *const std::ffi::c_void, String> {
    Err("this framebuffer example requires EGL device enumeration on Linux".into())
}

fn read(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    File::open(path)
        .and_then(|f| f.take(limit as u64 + 1).read_to_end(&mut out))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if out.len() > limit {
        return Err("sample input exceeds its size limit".into());
    }
    Ok(out)
}

fn upload(host: &mut ShellHost, path: &Path, budget: &mut u32) -> Result<(u32, u32, u32), String> {
    let bytes = read(path, 32 << 20)?;
    let header = bytes.get(16..24).ok_or("missing PNG dimensions")?;
    let width = u32::from_be_bytes(header[..4].try_into().unwrap());
    let height = u32::from_be_bytes(header[4..].try_into().unwrap());
    let count = width.checked_mul(height).unwrap_or(u32::MAX);
    if width == 0 || height == 0 || width > 4096 || height > 4096 || count > *budget {
        return Err("sample texture exceeds its pixel budget".into());
    }
    let image = bfe_render::texture_image::decode_png(&bytes, 4096)?;
    let texture = ShellHost::upload_rgba(&host.core.gl, width as i32, height as i32, &image.rgba)?;
    host.core.hold_texture(texture);
    *budget -= count;
    Ok((host.core.add_texture(texture), width, height))
}

fn cover_uv(image: f64, viewport: f64) -> [f64; 4] {
    let width = (viewport / image).min(1.);
    let height = (image / viewport).min(1.);
    [
        (1. - width) * 0.5,
        (1. - height) * 0.5,
        (1. + width) * 0.5,
        (1. + height) * 0.5,
    ]
}

fn size(s: &str) -> Result<[i32; 2], String> {
    let (w, h) = s.split_once('x').ok_or("expected WIDTHxHEIGHT")?;
    let dims = [
        w.parse::<i32>().map_err(|e| e.to_string())?,
        h.parse::<i32>().map_err(|e| e.to_string())?,
    ];
    if dims.iter().any(|&n| !(1..=8192).contains(&n))
        || dims[0] * dims[1] > 16 << 20
    {
        return Err("framebuffer exceeds size limit".into());
    }
    Ok(dims)
}

fn save(host: &ShellHost, size: [i32; 2], path: &Path) -> Result<(), String> {
    let [w, h] = size;
    let mut px = host.pixels()?;
    let stride = w as usize * 4;
    for j in 0..h as usize / 2 {
        let (before, after) = px.split_at_mut((h as usize - 1 - j) * stride);
        before[j * stride..(j + 1) * stride].swap_with_slice(&mut after[..stride]);
    }
    let out = BufWriter::new(File::create(path).map_err(|e| e.to_string())?);
    let mut png = png::Encoder::new(out, w as u32, h as u32);
    png.set_color(png::ColorType::Rgba);
    png.set_depth(png::BitDepth::Eight);
    png.set_compression(png::Compression::Fast);
    png.write_header()
        .and_then(|mut w| w.write_image_data(&px))
        .map_err(|e| e.to_string())
}

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [root, file, theme, locale, resolution, out] = args.as_slice() else {
        return Err("expected prepared root, observation file, look, locale, WIDTHxHEIGHT and output directory".into());
    };
    if !matches!(theme.as_str(), "bf1" | "bf2" | "eternal") {
        return Err("unknown look".into());
    }
    let root = PathBuf::from(root)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let size = size(resolution)?;
    let viewport = size.map(f64::from);
    let source: serde_json::Value =
        serde_json::from_slice(&read(Path::new(file), 8 << 20)?).map_err(|e| e.to_string())?;
    let sequence = recording::Recording::deserialize(&source).map_err(|e| e.to_string())?;
    if sequence.images.len() > 128 || sequence.stages.len() > 256 {
        return Err("oversized sequence".into());
    }
    let get = context()?;
    let settings = AppSettingsStore::load(current_desktop_paths());
    let mut host = ShellHost::new(size, &settings, None, || unsafe {
        glow::Context::from_loader_function_cstr(&get)
    })?;
    let renderer = unsafe { host.core.gl.get_parameter_string(glow::RENDERER) };
    println!("GPU {renderer}");
    let Presentation::BattlefrontII(mut screens) = Presentation::admit(
        GameFamily::Swbf2,
        || Err::<(), _>("expected second game".into()),
        &root,
        locale,
        theme,
        &settings.interface,
    )?
    else {
        unreachable!()
    };
    let mut budget = 128 << 20;
    let installed = ScreenArt::bitmap_fonts(&root, |path| upload(&mut host, path, &mut budget))?;
    let roles = ScreenArt::font_roles(theme, &host.page, &installed, &host.fonts);
    let game = ScreenArt::font_roles("bf1", &host.page, &installed, &host.fonts);
    let mut art = ScreenArt::load(&screens.catalog, roles, host.textures.solid, |path| {
        upload(&mut host, path, &mut budget)
    })?;
    art.set_game_fonts(game);
    let mut backgrounds = std::collections::HashMap::new();
    for (name, path) in sequence.images {
        let (handle, w, h) = upload(&mut host, &child(&root, path)?, &mut budget)?;
        art.insert(name.to_owned(), (handle, w, h));
        backgrounds.insert(name, (handle, f64::from(w) / f64::from(h)));
    }
    let mut input = BattleInput::default();
    input.set_profile(GameFamily::Swbf2, &settings.interface);
    let out = Path::new(out);
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    for stage in &sequence.stages {
        if stage.name.is_empty()
            || !stage
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err("invalid stage name".into());
        }
        let view = stage.view();
        screens.menu = Menu::default();
        let mut held_roster = None;
        for code in &stage.keys {
            let trigger = InputTrigger::from_dom_code(code).ok_or("invalid recorded input")?;
            let was_open = screens.menu.board;
            device_event(
                &mut screens,
                &mut input,
                &view,
                trigger,
                true,
                false,
                [0.; 2],
            );
            if !was_open && screens.menu.board {
                held_roster = Some(trigger);
            } else {
                device_event(
                    &mut screens,
                    &mut input,
                    &view,
                    trigger,
                    false,
                    false,
                    [0.; 2],
                );
            }
        }
        let mut draw = || {
            host.batch.begin();
            if let Some(&(back, aspect)) = backgrounds.get(stage.backdrop) {
                host.batch.quad(
                    anchor(0., 0., false, 768.),
                    [0., 0., 1024., 768.],
                    back,
                    cover_uv(aspect, viewport[0] / viewport[1]),
                    [1.; 4],
                    0.,
                    0,
                );
            }
            art.draw(&mut screens, &mut host.batch, &view, viewport, 0.5);
        };
        draw();
        let now = Instant::now();
        let before = bfe_test_alloc::thread_allocations();
        for _ in 0..20 {
            draw();
        }
        let allocations = bfe_test_alloc::thread_allocations().wrapping_sub(before);
        let nanos = now.elapsed().as_nanos() / 20;
        if allocations != 0 {
            return Err(format!(
                "{} made {allocations} draw allocations",
                stage.name
            ));
        }
        if host.batch.count >= bfe_render::interface::QUADS {
            return Err(format!("{} exceeded the HUD batch", stage.name));
        }
        host.upload_and_draw_batch();
        let path = out.join(format!("{}-{theme}-{locale}-{resolution}.png", stage.name));
        save(&host, size, &path)?;
        println!(
            "{} {} quads {} ns/frame {} allocations {} omitted quads",
            path.display(),
            host.batch.count,
            nanos,
            allocations,
            screens.omitted_quads
        );
        if let Some(trigger) = held_roster {
            device_event(
                &mut screens,
                &mut input,
                &view,
                trigger,
                false,
                false,
                [0.; 2],
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_world_backplate_keeps_its_aspect_at_each_viewport() {
        assert_eq!(cover_uv(2., 2.), [0., 0., 1., 1.]);
        assert_eq!(cover_uv(2., 4.), [0., 0.25, 1., 0.75]);
        assert_eq!(cover_uv(2., 1.), [0.25, 0., 0.75, 1.]);
    }
}
