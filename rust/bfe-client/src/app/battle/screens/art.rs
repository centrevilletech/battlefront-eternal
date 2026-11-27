//! Texture handles and horizontal font roles for the duration of an admitted battle.
use super::Surface;
use bfe_battle_ui::{
    battle2::{BattleView, Phase, Screens},
    HudBatchSurface,
};
use bfe_render::{
    hud_model2::catalog::{child, Catalog},
    interface::{Asset, Batch, Font},
};
use bfe_shell::{page::Page, render::ShellFonts};
use std::{collections::HashMap, fs::File, io::Read, path::Path};

pub struct ScreenArt {
    textures: HashMap<String, u32>,
    assets: HashMap<u32, Asset>,
    pub(super) fonts: [Font; 4],
    /// The game's own HUD fonts, which the battle HUD uses in every look.
    game_fonts: Option<[Font; 4]>,
    solid: u32,
}

impl ScreenArt {
    /// Use the existing page atlases, or load the installed bitmap fonts for the classic look.
    pub fn font_roles(
        theme: &str,
        page: &Page,
        bitmap: &ShellFonts,
        fallback: &ShellFonts,
    ) -> [Font; 4] {
        let roles = match theme {
            "bf1" => [&bitmap.tiny, &bitmap.small, &bitmap.medium, &bitmap.large],
            "bf2" => [&page.medium, &page.bold, &page.title, &page.logo],
            _ => [&page.regular, &page.body, &page.medium, &page.bold],
        };
        let backup = [
            &fallback.tiny,
            &fallback.small,
            &fallback.medium,
            &fallback.large,
        ];
        std::array::from_fn(|i| {
            let f = if roles[i].glyphs.is_empty() {
                backup[i]
            } else {
                roles[i]
            };
            Font {
                height: f.height,
                glyphs: f.glyphs.clone(),
            }
        })
    }

    pub fn bitmap_fonts(
        root: &Path,
        mut upload: impl FnMut(&Path) -> Result<(u32, u32, u32), String>,
    ) -> Result<ShellFonts, String> {
        let path = child(root, "common/common.json")?;
        let mut bytes = Vec::new();
        File::open(path)
            .and_then(|f| f.take((16 << 20) + 1).read_to_end(&mut bytes))
            .map_err(|e| e.to_string())?;
        if bytes.len() > 16 << 20 {
            return Err("oversized battle font catalog".into());
        }
        let common = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let source = bfe_shell::data::read_shell_data(None, Some(&common), None);
        if source.pages.len() > 32 {
            return Err("too many battle font pages".into());
        }
        let mut pages = HashMap::new();
        for url in source.pages.values() {
            let relative = url.strip_prefix('/').unwrap_or(url);
            let path = child(root, relative)?;
            let page = upload(&path).map_err(|err| format!("battle font {relative}: {err}"))?;
            pages.insert(url.as_str(), page);
        }
        let fonts =
            crate::app::shell::ShellHost::load_fonts(&source, &mut |url| pages.get(url).copied());
        for font in [&fonts.tiny, &fonts.small, &fonts.medium, &fonts.large] {
            if font.glyphs.is_empty() || font.glyphs.values().any(|g| g.page <= 1) {
                return Err("missing bitmap glyph atlas in battle font catalog".into());
            }
        }
        Ok(fonts)
    }

    pub fn load(
        catalog: &Catalog,
        fonts: [Font; 4],
        solid: u32,
        mut upload: impl FnMut(&Path) -> Result<(u32, u32, u32), String>,
    ) -> Result<Self, String> {
        let mut art = Self {
            textures: HashMap::with_capacity(catalog.textures.len()),
            assets: HashMap::with_capacity(catalog.textures.len()),
            fonts,
            game_fonts: None,
            solid,
        };
        for (name, path) in &catalog.textures {
            art.insert(name.clone(), upload(path)?);
        }
        Ok(art)
    }

    /// The game's installed bitmap fonts (tiny, small, medium, large) for the battle HUD.
    pub fn set_game_fonts(&mut self, fonts: [Font; 4]) {
        self.game_fonts = Some(fonts);
    }

    /// Register an admitted bitmap or a side model's projected icon.
    pub fn insert(&mut self, name: String, (handle, w, h): (u32, u32, u32)) {
        self.textures.insert(name, handle);
        self.assets.insert(
            handle,
            Asset {
                texture: handle as i32,
                width: w.into(),
                height: h.into(),
                ready: true,
            },
        );
    }

    /// The battle texture admitted under `name`.
    pub fn handle(&self, name: &str) -> Option<u32> {
        self.textures.get(name).copied()
    }

    /// Whether a HUD mesh alias or bitmap name resolves to an uploaded battle texture.
    pub fn has_mesh_art(&self, catalog: &Catalog, name: &str) -> bool {
        if name.is_empty() {
            return false;
        }
        let has = |key: &str| {
            catalog.meshes.get(key).map_or_else(
                || self.textures.contains_key(key),
                |mapped| self.textures.contains_key(mapped),
            )
        };
        if has(name) {
            return true;
        }
        if name.bytes().any(|b| b.is_ascii_uppercase()) {
            let lower = name.to_ascii_lowercase();
            return has(&lower);
        }
        false
    }

    /// Append the overlay to a world frame without allocating or retaining a battle observation.
    pub fn draw(
        &self,
        screens: &mut Screens,
        batch: &mut Batch,
        view: &BattleView<'_>,
        viewport: [f64; 2],
        fraction: f32,
    ) {
        // The HUD over play is the game's; the deployment and score screens follow the look.
        let playing = view.phase == Phase::Playing && !screens.menu.board;
        let fonts = match &self.game_fonts {
            Some(game) if playing => game,
            _ => &self.fonts,
        };
        let mut hud = HudBatchSurface::new(768., batch, fonts, &[]);
        hud.aspect = viewport[0] / viewport[1].max(1.);
        hud.textures = Some(&self.textures);
        screens.draw(
            &mut Surface {
                hud,
                solid: self.solid,
                assets: &self.assets,
            },
            view,
            viewport,
            fraction,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmap_pages_load_from_the_family_root_and_report_upload_errors() {
        let root = std::env::temp_dir().join(format!("bfe-font-pages-{}", std::process::id()));
        let common = root.join("common");
        std::fs::create_dir_all(&common).unwrap();
        let font = serde_json::json!({"pages":["ink"],"glyphs":[[63,0,8,0,6,0,9,0,0,1,1]]});
        let data = serde_json::json!({"textures":{"ink":"ink.png"}, "fonts":{
            "gamefont_tiny":font, "gamefont_small":font, "gamefont_medium":font, "gamefont_large":font}});
        std::fs::write(common.join("common.json"), data.to_string()).unwrap();
        let page = common.join("ink.png");
        std::fs::write(&page, []).unwrap();
        let expected_page = page.canonicalize().unwrap();
        let fonts = ScreenArt::bitmap_fonts(&root, |path| {
            assert_eq!(path, expected_page);
            Ok((73, 8, 8))
        })
        .unwrap();
        for f in [&fonts.tiny, &fonts.small, &fonts.medium, &fonts.large] {
            assert_eq!(f.glyphs[&63].page, 73);
        }
        assert!(ScreenArt::bitmap_fonts(&root, |_| Err("unreadable atlas".into())).is_err());
        std::fs::remove_file(&page).unwrap();
        assert!(ScreenArt::bitmap_fonts(&root, |_| panic!()).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
