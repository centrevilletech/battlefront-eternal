//! Admit Saira and page surfaces into the battle HUD's own texture namespace.
use super::{page_font, page_surfaces, settings::AppSettingsStore, shell::ShellHost};
use crate::scene::Scene;
use bfe_shell::{page::Page, render::ShellTextures};
pub(super) struct PagePause {
    pub page: Page,
    pub textures: ShellTextures,
    pub theme: String,
    pub blur: super::page_blur::PageBlur,
    pending: bool,
}
impl PagePause {
    /// Clip the already published pause batch around a modal compositor pass.
    pub fn draw_range(scene: &mut Scene, first: usize, last: usize) {
        use bfe_render::hud::*;
        let bridge = &mut scene.bridge;
        let batch = &bridge.hud_model.batch;
        let mut count = 0;
        for draw in &batch.draws {
            let lo = draw.first.max(first * 6);
            let hi = (draw.first + draw.count).min(last * 6);
            if hi <= lo || count == MAX_GROUPS {
                continue;
            }
            let at = IN_FIRST_GROUP + count * GROUP;
            count += 1;
            bridge.r.hud.input[at] = lo as f64;
            bridge.r.hud.input[at + 1] = (hi - lo) as f64;
            bridge.r.hud.input[at + 2] = bridge.hud_model.asset(draw.texture).texture as f64;
            bridge.r.hud.input[at + 3] = bridge
                .hud_model
                .asset(if draw.mask == 0 {
                    draw.texture
                } else {
                    draw.mask
                })
                .texture as f64;
            bridge.r.hud.input[at + 4] = u8::from(draw.mask != 0) as f64;
        }
        bridge.r.hud.input[IN_GROUPS] = count as f64;
        bridge.r.core.state.reset();
        bridge.r.hud.draw(&mut bridge.r.core);
    }
    pub fn pointer(&mut self, x: f64, y: f64) -> bool {
        match self.page.hit(x, y) {
            Some(bfe_shell::page::Hit::Theme) => {
                self.page.dropdown = !self.page.dropdown;
                self.page.dropdown_cursor = match self.theme.as_str() {
                    "bf1" => 0,
                    "bf2" => 1,
                    _ => 2,
                };
                true
            }
            Some(bfe_shell::page::Hit::Look(index)) => {
                self.theme = ["bf1", "bf2", "eternal"][index].into();
                self.pending = true;
                self.page.dropdown = false;
                true
            }
            _ => {
                self.page.dropdown = false;
                false
            }
        }
    }
    pub fn key(&mut self, key: &str) -> bool {
        if let Some(selection) = self
            .page
            .dropdown_key(if key == "Space" { " " } else { key })
        {
            if let Some(index) = selection {
                self.theme = ["bf1", "bf2", "eternal"][index].into();
                self.pending = true;
            }
            true
        } else {
            false
        }
    }
    pub fn save(&mut self, settings: &mut AppSettingsStore) {
        if !self.pending {
            return;
        }
        self.pending = false;
        settings.player.shell.menu_theme = self.theme.clone();
        settings.sync_interface_from_player();
        if let Err(error) = settings.save_player() {
            eprintln!("cannot save menu look: {error}");
        }
    }
    pub fn admit(
        scene: &mut Scene,
        size: [i32; 2],
        settings: &AppSettingsStore,
    ) -> Result<Self, String> {
        fn upload(scene: &mut Scene, w: usize, h: usize, rgba: &[u8]) -> Result<u32, String> {
            let texture =
                ShellHost::upload_rgba(&scene.bridge.r.core.gl, w as i32, h as i32, rgba)?;
            let handle = scene.bridge.r.core.add_texture(texture);
            let id = scene.bridge.hud_model.assets.len() as u32;
            scene
                .bridge
                .hud_model
                .assets
                .push(bfe_render::interface::Asset {
                    texture: handle as i32,
                    width: w as f64,
                    height: h as f64,
                    ready: true,
                });
            Ok(id)
        }
        let mut fonts = Vec::with_capacity(6);
        for bytes in [
            include_bytes!("../../assets/fonts/saira-700.ttf").as_slice(),
            include_bytes!("../../assets/fonts/saira-900.ttf").as_slice(),
            include_bytes!("../../assets/fonts/saira-800.ttf").as_slice(),
            include_bytes!("../../assets/fonts/saira-800-normal.ttf").as_slice(),
            include_bytes!("../../assets/fonts/saira-400.ttf").as_slice(),
            include_bytes!("../../assets/fonts/saira-600.ttf").as_slice(),
        ] {
            let (rgba, mut font) = page_font::atlas(bytes)?;
            let texture = ShellHost::upload_page_font(&scene.bridge.r.core.gl, &rgba)?;
            let handle = scene.bridge.r.core.add_texture(texture);
            let id = scene.bridge.hud_model.assets.len() as u32;
            scene
                .bridge
                .hud_model
                .assets
                .push(bfe_render::interface::Asset {
                    texture: handle as i32,
                    width: 1024.,
                    height: 1024.,
                    ready: true,
                });
            let handle = id;
            for glyph in font.glyphs.values_mut() {
                glyph.page = handle;
            }
            fonts.push(font);
        }
        let medium = fonts.pop().unwrap();
        let regular = fonts.pop().unwrap();
        let bold = fonts.pop().unwrap();
        let title = fonts.pop().unwrap();
        let logo = fonts.pop().unwrap();
        let mut page = Page::new(fonts.pop().unwrap(), logo);
        page.title = title;
        page.bold = bold;
        page.regular = regular;
        page.medium = medium;
        let bw = ((size[0] as f64 * 0.30).min(400.) - 16.).round().max(28.) as usize;
        let bh = (size[1] as f64 * 0.056).clamp(38., 56.).round() as usize;
        page.button_source = [bw as f64, bh as f64];
        for look in 0..3 {
            for selected in [false, true] {
                let rgba = page_surfaces::button(bw, bh, look, selected);
                page.buttons[look * 2 + usize::from(selected)] = upload(scene, bw, bh, &rgba)?;
            }
        }
        for look in 0..3 {
            let rgba = page_surfaces::pause_backdrop(size[0] as usize, size[1] as usize, look);
            page.pause_backdrops[look] = upload(scene, size[0] as usize, size[1] as usize, &rgba)?;
        }
        let solid = upload(scene, 1, 1, &[255, 255, 255, 255])?;
        let blur = super::page_blur::PageBlur::new(&scene.bridge.r.core.gl, size)?;
        scene.bridge.r.core.state.reset();
        Ok(Self {
            blur,
            page,
            textures: ShellTextures {
                solid,
                ..ShellTextures::default()
            },
            theme: settings.player.shell.menu_theme.clone(),
            pending: false,
        })
    }
}
