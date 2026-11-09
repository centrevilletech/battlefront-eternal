//! Short, asset-independent window diagnostic for release platform builds.
//! It uses the ordinary startup and menu hosts, then the renderer smoke's room.
use super::{script::write_png, settings::AppSettingsStore, shell::ShellHost};
use crate::{
    audio::SharedAudioRouter,
    host::World,
    scene::{MapCamera, Scene},
};
use bfe_shell::screens::ScreenId;
use glow::HasContext;
use std::{collections::HashSet, path::PathBuf, time::Instant};

pub(super) struct Smoke {
    directory: PathBuf,
    started: Instant,
    menu_frames: u32,
    pub startup: bool,
    pub done: bool,
    frames: Vec<serde_json::Value>,
}

impl Smoke {
    pub fn new(directory: Option<PathBuf>) -> Self {
        Self {
            directory: directory.unwrap_or_else(|| std::env::temp_dir().join("bfe-platform-smoke")),
            started: Instant::now(),
            menu_frames: 0,
            startup: false,
            done: false,
            frames: Vec::new(),
        }
    }

    pub fn capture(&mut self, name: &str, size: [i32; 2], pixels: &[u8]) -> Result<(), String> {
        let stats = image_stats(pixels)?;
        std::fs::create_dir_all(&self.directory).map_err(|e| e.to_string())?;
        write_png(
            &self.directory.join(format!("{name}.png")),
            size[0] as u32,
            size[1] as u32,
            pixels,
        )?;
        self.frames
            .push(serde_json::json!({"stage": name, "size": size, "pixels": stats}));
        Ok(())
    }

    pub fn record_ui(
        &mut self,
        size: [i32; 2],
        pixels: &[u8],
        input: serde_json::Value,
    ) -> Result<(), String> {
        self.capture("slint", size, pixels)?;
        let path = self.directory.join("summary.json");
        let mut report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        report["frames"] = serde_json::json!(self.frames);
        report["slint"] = input;
        std::fs::write(path, report.to_string()).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn advance(
        &mut self,
        shell: &mut ShellHost,
        settings: &mut AppSettingsStore,
        audio: &SharedAudioRouter,
        load: impl FnMut() -> glow::Context,
    ) -> Result<Option<Box<Scene>>, String> {
        if self.started.elapsed().as_secs() > 90 {
            return Err("platform smoke timed out before the main menu".into());
        }
        match shell.app.current_screen() {
            ScreenId::Gate => {
                shell.handle_key("Enter", settings, audio);
            }
            ScreenId::Main => {
                self.menu_frames += 1;
                // Let the ordinary menu timeline render before reading its target.
                if self.menu_frames >= 12 && !self.done {
                    self.capture("main-menu", shell.size, &shell.pixels()?)?;
                    let root = shell
                        .content_dir
                        .as_ref()
                        .ok_or("smoke content not admitted")?;
                    let (scene, _) = Scene::load(root, "room", shell.size, load)?;
                    let mut scene = Box::new(scene);
                    let mut camera = MapCamera::new([0., 2., 3.], shell.size)?;
                    scene.draw(camera.present(0.))?;
                    self.capture("scene", scene.size, &scene.pixels()?)?;
                    if scene.admitted < 2 || scene.draws == 0 {
                        return Err("platform smoke room admitted no drawable content".into());
                    }
                    let gl = &scene.bridge.r.core.gl;
                    let renderer = unsafe { gl.get_parameter_string(glow::RENDERER) };
                    if !renderer.to_ascii_lowercase().contains("llvmpipe") {
                        return Err(format!("platform smoke requires llvmpipe, got {renderer}"));
                    }
                    let report = serde_json::json!({
                        "ok": true, "renderer": renderer,
                        "version": unsafe { gl.get_parameter_string(glow::VERSION) },
                        "arch": std::env::consts::ARCH, "os": std::env::consts::OS,
                        "admitted": scene.admitted, "draws": scene.draws,
                        "glErrors": 0, "frames": self.frames,
                    });
                    std::fs::write(self.directory.join("summary.json"), report.to_string())
                        .map_err(|e| e.to_string())?;
                    println!("platform smoke: {report}");
                    self.done = true;
                    return Ok(Some(scene));
                }
            }
            _ => {}
        }
        Ok(None)
    }
}

// A clear-colored or black target must fail even when readback itself succeeds.
// Count RGB only: an opaque alpha channel cannot make a black frame pass.
fn image_stats(pixels: &[u8]) -> Result<serde_json::Value, String> {
    if pixels.is_empty() || pixels.len() % 4 != 0 {
        return Err("invalid smoke RGBA readback".into());
    }
    let mut colors = HashSet::new();
    let mut lit = 0;
    for pixel in pixels.chunks_exact(4) {
        colors.insert([pixel[0], pixel[1], pixel[2]]);
        lit += usize::from(pixel[..3].iter().any(|&v| v > 24));
    }
    if colors.len() < 8 || lit < pixels.len() / 4 / 100 {
        return Err(format!(
            "blank smoke frame: {} colors, {lit} lit pixels",
            colors.len()
        ));
    }
    Ok(serde_json::json!({"colors": colors.len(), "lit": lit}))
}

#[cfg(test)]
mod tests {
    use super::image_stats;
    #[test]
    fn rejects_black_and_uniform_frames_even_with_opaque_alpha() {
        assert!(image_stats(&[0, 0, 0, 255].repeat(100)).is_err());
        assert!(image_stats(&[100, 150, 200, 255].repeat(100)).is_err());
        let image: Vec<_> = (0..100u8).flat_map(|v| [v, 100, 200, 255]).collect();
        assert!(image_stats(&image).is_ok());
    }
}
