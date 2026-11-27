//! Draw a fixed simulation tick without starting an offline or online battle.
use bfe_client::app::{settings::AppSettingsStore, shell::ShellHost};
use bfe_content_native::textures::Library;
use bfe_desktop::paths::{resolve_desktop_paths, DesktopPathsOptions};
use bfe_shell::{
    data::read_shell_data,
    loading::LookId,
    loading2::{prepared::PreparedBackdrop, BackdropFrame, MapLoadPhase, MapLoadSequence},
    render::{
        bitmap_tip_lines, draw_loading2, tip_pages, wrap_bitmap_tip, BackdropDraw,
        BackdropTextPlacement, BackdropTextures, TipPanel,
    },
};
use glow::HasContext;
use slint::{ComponentHandle, Rgba8Pixel, SharedPixelBuffer};
use std::{
    cell::RefCell,
    error::Error,
    io::{BufWriter, Read, Write},
    path::{Path, PathBuf},
    rc::Rc,
};

slint::slint! {
    import { MapBackdropText, BackdropPlacement } from "../../bfe-ui/ui/screens/map-backdrop.slint";
    import { Theme } from "../../bfe-ui/ui/theme.slint";
    export { Theme }
    export { BackdropPlacement } from "../../bfe-ui/ui/screens/map-backdrop.slint";
    export component SampleWindow inherits Window {
        preferred-width: 1280px;
        preferred-height: 720px;
        min-width: 320px;
        min-height: 240px;
        no-frame: true;
        title: "Loading screen";
        in property <image> frame;
        in property <BackdropPlacement> placement;
        in property <string> title-text;
        in property <string> tip;
        in property <string> word;
        Image { source: root.frame; image-fit: fill; }
        out property <float> word-width: overlay.word-width;
        in property <length> probe-width;
        in property <string> probe-text;
        out property <length> measured-height: overlay.measured-height;
        out property <length> tip-font-size: overlay.tip-font-size;
        in-out property <int> page;
        in property <int> pages;
        forward-focus: keys;
        keys := FocusScope {
            key-pressed(event) => {
                if (root.pages > 1 && (event.text == Key.RightArrow || event.text == Key.LeftArrow)) {
                    root.page = mod(root.page + root.pages + (event.text == Key.RightArrow ? 1 : -1), root.pages);
                    return accept;
                }
                return reject;
            }
        }
        overlay := MapBackdropText {
            placement: root.placement;
            probe-width: root.probe-width;
            probe-text: root.probe-text;
            title-text: root.title-text;
            tip: root.tip;
            word: root.word;
        }
        if root.pages > 1: pager := Text {
            x: root.placement.tip-x * root.width;
            y: (root.placement.tip-y + root.placement.tip-height) * root.height;
            width: root.placement.tip-width * root.width;
            height: 18px;
            color: white;
            font-family: Theme.font-family;
            font-size: 11px;
            horizontal-alignment: center;
            text: "‹  " + (root.page + 1) + " / " + root.pages + "  ›";
            TouchArea {
                clicked => { root.page = mod(root.page + root.pages + (self.mouse-x < self.width/2 ? -1 : 1), root.pages); }
            }
        }

    }
}

struct Sample {
    root: PathBuf,
    look: LookId,
    frame: BackdropFrame,
    prepared: PreparedBackdrop,
    lines: Vec<Vec<u16>>,
}

impl Sample {
    fn read(
        root: &Path,
        world: &str,
        look: LookId,
        tick: u32,
        language: Option<&str>,
        seed: u32,
    ) -> Result<Option<Self>, Box<dyn Error>> {
        let manifest = Library::new(root, false, 4096, 0)?.path("common/shell/load2.json")?;
        let prepared = PreparedBackdrop::read(std::fs::File::open(manifest)?, world, language)?;
        let mut timeline = MapLoadSequence::new(
            prepared.duration,
            prepared.tip_ticks,
            prepared.pictures.len(),
            prepared.tips.len(),
            seed,
        )?;
        for _ in 0..tick.min(prepared.duration.get()) {
            timeline.tick();
        }
        let MapLoadPhase::Showing(frame) = timeline.phase(false, 0.) else {
            return Ok(None);
        };
        let lines = match frame.tip {
            Some(i) => bitmap_tip_lines(&prepared.tips[i].text, &prepared.tips[i].line_ends)
                .ok_or("invalid tip lines")?,
            None => Vec::new(),
        };
        Ok(Some(Self {
            root: root.into(),
            look,
            frame,
            prepared,
            lines,
        }))
    }

    fn surface(
        &self,
        get_proc: &dyn Fn(&std::ffi::CStr) -> *const std::ffi::c_void,
        size: [i32; 2],
    ) -> Result<(ShellHost, BackdropTextures), String> {
        let settings = AppSettingsStore::load(resolve_desktop_paths(&DesktopPathsOptions {
            home: Some(self.root.clone()),
            ..Default::default()
        }));
        let mut host = ShellHost::new(size, &settings, None, || unsafe {
            glow::Context::from_loader_function_cstr(get_proc)
        })?;
        eprintln!("GPU: {}", unsafe {
            host.core.gl.get_parameter_string(glow::RENDERER)
        });
        let library = Library::new(&self.root, false, 4096, 0)?;
        let path = "common/common.json";
        let common = if library
            .path(path)?
            .try_exists()
            .map_err(|e| e.to_string())?
        {
            Some(
                serde_json::from_slice(&read_asset(&library, path, 8 << 20)?)
                    .map_err(|e| format!("font catalog: {e}"))?,
            )
        } else {
            None
        };
        let data = read_shell_data(None, common.as_ref(), None);
        if data.pages.len() > 32 {
            return Err("too many font atlas pages".into());
        }
        let core = &mut host.core;
        let mut remaining_pixels = 24 << 20;
        let mut upload = |path: &str| {
            let bytes = read_asset(&library, path, 20 << 20).ok()?;
            let header = bytes.get(16..24)?;
            let width = u32::from_be_bytes(header[..4].try_into().ok()?);
            let height = u32::from_be_bytes(header[4..].try_into().ok()?);
            let pixels = width.checked_mul(height)?;
            if pixels > (2048 * 2048).min(remaining_pixels) {
                return None;
            }
            let image = bfe_render::texture_image::decode_png(&bytes, 4096).ok()?;
            let texture = ShellHost::upload_rgba(
                &core.gl,
                image.width as i32,
                image.height as i32,
                &image.rgba,
            )
            .ok()?;
            remaining_pixels -= pixels;
            core.hold_texture(texture);
            Some((core.add_texture(texture), image.width, image.height))
        };
        let imported = ShellHost::load_fonts(&data, &mut upload);
        for (role, font) in [
            (&mut host.fonts.small, imported.small),
            (&mut host.fonts.tiny, imported.tiny),
            (&mut host.fonts.medium, imported.medium),
        ] {
            if !font.glyphs.is_empty() {
                *role = font;
            }
        }
        let prepared = &self.prepared;
        let selected = self
            .frame
            .picture
            .and_then(|i| prepared.pictures.get(i))
            .map(String::as_str);
        let textures = BackdropTextures::load(
            host.textures.solid,
            selected,
            prepared.fallback.as_deref(),
            prepared.panel.as_deref(),
            prepared.led.as_deref(),
            |path| upload(&format!("common/shell/{path}")),
        );
        Ok((host, textures))
    }

    fn draw(
        &self,
        ui: &SampleWindow,
        host: &mut ShellHost,
        textures: BackdropTextures,
        viewport: [f64; 2],
    ) -> Result<(slint::Image, BackdropTextPlacement), String> {
        let prepared = &self.prepared;
        let text = self
            .frame
            .tip
            .map(|i| prepared.tips[i].text.as_str())
            .unwrap_or("");
        let mut panel = TipPanel::new(viewport, prepared.border, prepared.bar.height, false);
        let mut wide = false;
        let mut vector_height = 0.;
        let mut wrapped;
        let (rows, pages, selected);
        loop {
            let unit = panel.unit;
            let height = |s: &str| {
                ui.set_probe_text(s.into());
                f64::from(ui.get_measured_height()) / unit
            };
            let page = ui.get_page().max(0) as usize;
            if self.look == LookId::Bf1 {
                let font = &host.fonts.small;
                let scale = panel.bitmap_scale(font);
                let width = panel.text_width() / scale;
                let line_height = (font.height + 2.) * scale;
                let lines = if !wide
                    && scale == 1.
                    && panel.width == 400.
                    && self
                        .lines
                        .iter()
                        .all(|row| font.width(row, 1., 0.) <= width)
                {
                    &self.lines
                } else {
                    wrapped = wrap_bitmap_tip(text, font, width);
                    &wrapped
                };
                if lines.len() as f64 * line_height > panel.max_text_height && panel.width < 720. {
                    wide = true;
                    panel = TipPanel::new(viewport, prepared.border, prepared.bar.height, wide);
                    continue;
                }
                let available = if lines.len() as f64 * line_height > panel.max_text_height {
                    panel.max_text_height - panel.controls_height()
                } else {
                    panel.max_text_height
                };
                let count = (available / line_height).floor().max(1.) as usize;
                pages = lines.len().div_ceil(count).max(1);
                selected = page.min(pages - 1);
                rows = &lines[(selected * count)..((selected + 1) * count).min(lines.len())];
            } else {
                ui.set_probe_width((panel.text_width() * unit) as f32);
                vector_height = height(text);
                if vector_height > panel.max_text_height && panel.width < 720. {
                    wide = true;
                    panel = TipPanel::new(viewport, prepared.border, prepared.bar.height, wide);
                    continue;
                }
                if vector_height > panel.max_text_height {
                    let ranges = tip_pages(text, |s| {
                        height(s.trim()) <= panel.max_text_height - panel.controls_height()
                    });
                    pages = ranges.len();
                    selected = page.min(pages - 1);
                    let part = text[ranges[selected].clone()].trim();
                    ui.set_tip(part.into());
                    vector_height = height(part);
                } else {
                    (pages, selected) = (1, 0);
                    ui.set_tip(text.into());
                }
                rows = &[];
            }
            break;
        }
        ui.set_pages(pages as i32);
        ui.set_page(selected as i32);
        let show = BackdropDraw {
            textures,
            state: self.frame,
            tip: rows,
            border: prepared.border,
            wide_tip: wide,
            page_controls: pages > 1,
            tip_height: vector_height,
            word_width: f64::from(ui.get_word_width()),
            max_tip_lines: prepared.max_lines,
            leds: prepared.bar,
            blink_rate: prepared.blink_rate,
        };
        let placement = draw_loading2(
            &mut host.batch,
            &host.fonts,
            &show,
            &prepared.title,
            &prepared.label,
            self.look,
            viewport[0],
            viewport[1],
        );
        let [w, h] = host.size;
        let tip_size = if self.look == LookId::Bf1 {
            host.fonts.small.height * panel.bitmap_scale(&host.fonts.small) * panel.unit
        } else {
            f64::from(ui.get_tip_font_size())
        };
        let ink_height = if self.look == LookId::Bf1 {
            rows.len() as f64
                * (host.fonts.small.height + 2.)
                * panel.bitmap_scale(&host.fonts.small)
        } else {
            vector_height
        };
        let [x, y, tw, th] = placement.tip;
        let note = [
            f64::from(x) * viewport[0],
            f64::from(y) * viewport[1],
            f64::from(tw) * viewport[0],
            f64::from(th) * viewport[1],
        ];
        println!("viewport {w}x{h} tip {tip_size:.2}px height {:.2}/{:.2}px pages {pages} rectangle {:.2} {:.2} {:.2} {:.2}",
            ink_height * panel.unit, note[3], note[0], note[1], note[2], note[3]);
        host.upload_and_draw_batch();
        let pixels = host.pixels()?;
        let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(w as u32, h as u32);
        let stride = w as usize * 4;
        for (dst, src) in buffer
            .make_mut_bytes()
            .chunks_mut(stride)
            .zip(pixels.rchunks_exact(stride))
        {
            dst.copy_from_slice(src);
        }
        Ok((slint::Image::from_rgba8(buffer), placement))
    }
}

fn read_asset(library: &Library, path: &str, limit: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let file = std::fs::File::open(library.path(path)?).map_err(|e| format!("{path}: {e}"))?;
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err(format!("oversized asset: {path}"));
    }
    Ok(bytes)
}

fn save_frame(window: &SampleWindow, path: &Path) -> Result<(), Box<dyn Error>> {
    let pixels = window.window().take_snapshot()?;
    let mut file = BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(&mut file, pixels.width(), pixels.height());
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_color(png::ColorType::Rgba);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(pixels.as_bytes())?;
    writer.finish()?;
    file.flush()?;
    Ok(())
}

fn framebuffer_size(w: u32, h: u32) -> Result<[i32; 2], &'static str> {
    if !(1..=8192).contains(&w)
        || !(1..=8192).contains(&h)
        || w * h > (32 << 20)
    {
        return Err("sample framebuffer exceeds its size limits");
    }
    Ok([w as i32, h as i32])
}

fn requested_size(input: &str) -> Result<slint::LogicalSize, Box<dyn Error>> {
    let (w, h) = input.split_once('x').ok_or("expected WIDTHxHEIGHT")?;
    let [w, h] = framebuffer_size(w.parse()?, h.parse()?)?;
    Ok(slint::LogicalSize::new(w as f32, h as f32))
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("expected prepared 2005 root")?;
    let world = args.next().ok_or("expected world")?;
    let look = LookId::from_theme(args.next().as_deref());
    let tick = args.next().map(|s| s.parse()).transpose()?.unwrap_or(60);
    let language = args.next();
    let seed = args
        .next()
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos()
        });
    let Some(sample) = Sample::read(
        Path::new(&root),
        &world,
        look,
        tick,
        language.as_deref(),
        seed,
    )?
    else {
        println!("Waiting for readiness");
        return Ok(());
    };
    let ui = SampleWindow::new()?;
    ui.global::<Theme>().set_menu_theme(look.as_str().into());
    ui.set_title_text(sample.prepared.title.as_str().into());
    ui.set_word(sample.prepared.label.as_str().into());
    if let Some(i) = sample.frame.tip {
        ui.set_tip(sample.prepared.tips[i].text.as_str().into());
    }
    if let Some(size) = args.next() {
        ui.window().set_size(requested_size(&size)?);
    }
    let mut output = args.next().map(PathBuf::from);
    let weak = ui.as_weak();
    let error = Rc::new(RefCell::new(None));
    let outcome = error.clone();
    let mut surface = None;
    let mut last_layout = None;
    ui.window().set_rendering_notifier(move |state, api| {
        if matches!(state, slint::RenderingState::RenderingTeardown) {
            surface.take();
            last_layout = None;
            return;
        }
        if !matches!(state, slint::RenderingState::BeforeRendering) {
            return;
        }
        let window = weak.unwrap();
        let physical = window.window().size();
        if physical.width == 0 || physical.height == 0 {
            return;
        }
        let size = match framebuffer_size(physical.width, physical.height) {
            Ok(size) => size,
            Err(message) => {
                *outcome.borrow_mut() = Some(message.to_owned());
                let _ = slint::quit_event_loop();
                return;
            }
        };
        let factor = window.window().scale_factor();
        let key = (size, factor, window.get_page());
        if last_layout == Some(key) {
            return;
        }
        last_layout = Some(key);
        let viewport = size.map(|v| f64::from(v) / f64::from(factor));
        window
            .global::<Theme>()
            .set_viewport_width(viewport[0] as f32);
        window
            .global::<Theme>()
            .set_viewport_height(viewport[1] as f32);
        let result = (|| {
            let slint::GraphicsAPI::NativeOpenGL { get_proc_address } = api else {
                return Err("the loading screen sample requires OpenGL".into());
            };
            let gl = unsafe { glow::Context::from_loader_function_cstr(get_proc_address) };
            let (read, draw) = unsafe {
                (
                    gl.get_parameter_framebuffer(glow::READ_FRAMEBUFFER_BINDING),
                    gl.get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING),
                )
            };
            let result = (|| {
                if surface.is_none() {
                    surface = Some(sample.surface(get_proc_address, size)?);
                }
                let (host, textures) = surface.as_mut().unwrap();
                host.resize(size, f64::from(factor), || unsafe {
                    glow::Context::from_loader_function_cstr(get_proc_address)
                })?;
                host.core.state.reset();
                sample.draw(&window, host, *textures, viewport)
            })();
            unsafe {
                gl.bind_framebuffer(glow::READ_FRAMEBUFFER, read);
                gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, draw);
                gl.viewport(0, 0, size[0], size[1]);
            }
            result
        })();
        match result {
            Ok((image, pos)) => {
                window.set_frame(image);
                window.set_placement(BackdropPlacement {
                    unit_width: pos.unit_width,
                    title_x: pos.title[0],
                    title_y: pos.title[1],
                    tip_x: pos.tip[0],
                    tip_y: pos.tip[1],
                    tip_width: pos.tip[2],
                    tip_height: pos.tip[3],
                    word_left: pos.word[0],
                    word_bottom: pos.word[1],
                    word_opacity: pos.word_opacity,
                });
                println!("{world} {look:?} seed {seed}: {:?}", sample.frame);
                if let Some(path) = output.take() {
                    let weak = window.as_weak();
                    let result = outcome.clone();
                    slint::Timer::single_shot(std::time::Duration::ZERO, move || {
                        if let Err(error) = save_frame(&weak.unwrap(), &path) {
                            *result.borrow_mut() = Some(error.to_string());
                        }
                        let _ = slint::quit_event_loop();
                    });
                }
            }
            Err(e) => {
                *outcome.borrow_mut() = Some(e);
                let _ = slint::quit_event_loop();
            }
        }
    })?;
    ui.run()?;
    if let Some(error) = error.take() {
        return Err(error.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_dimensions_bound_the_framebuffer_before_allocation() {
        for size in ["NaNx480", "320x-5", "80000x512", "8192x8192", "0x600"] {
            assert!(requested_size(size).is_err(), "{size}");
        }
        for size in ["320x240", "2520x1080", "7680x4320"] {
            assert!(requested_size(size).is_ok(), "{size}");
        }
    }
}
