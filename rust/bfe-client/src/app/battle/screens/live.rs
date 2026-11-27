//! Presentation resources acquired once from the selected mission's family root.
use super::ScreenArt;
use crate::{app::shell::ShellHost, resources::Ranges, scene::Scene};
use bfe_battle_ui::battle2::{Screens, Style};
use bfe_render::{
    core::Core,
    hud_model2::catalog::child,
    interface::{Asset, Font},
};
use std::{io::Read, path::Path};

type Gl = Core<glow::Context, Ranges>;

pub(in crate::app::battle) struct Live {
    pub(super) flight: super::flight::FlightMap,
    binding_revision: Option<u64>,
    pub screens: Screens,
    pub art: ScreenArt,
    pub map_name: String,
    pub map_image: String,
    pub mode: bfe_battle_ui::battle2::Mode,
    pub team_labels: Option<[String; 2]>,
    pub(super) names: super::view::Names,
    pub end_camera: Option<crate::scene::View>,
    pub(super) feed: super::launcher::Feed,
    fonts: [Option<[Font; 4]>; 3],
    look: usize,
}

fn register(core: &mut Gl, assets: &mut Vec<Asset>, image: glow::Texture, w: u32, h: u32) -> u32 {
    let held = core.hold_texture(image);
    let handle = core.add_texture(held);
    let id = assets.len() as u32;
    assets.push(Asset {
        texture: handle as i32,
        width: w.into(),
        height: h.into(),
        ready: true,
    });
    id
}

fn upload(core: &mut Gl, assets: &mut Vec<Asset>, path: &Path) -> Result<(u32, u32, u32), String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take((64 << 20) + 1).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    if bytes.len() > 64 << 20 {
        return Err("oversized battle image".into());
    }
    let image = bfe_render::texture_image::decode_png(&bytes, 4096)?;
    let texture = ShellHost::upload_rgba(
        &core.gl,
        image.width as i32,
        image.height as i32,
        &image.rgba,
    )?;
    Ok((
        register(core, assets, texture, image.width, image.height),
        image.width,
        image.height,
    ))
}

fn page_font(core: &mut Gl, assets: &mut Vec<Asset>, data: &[u8]) -> Result<Font, String> {
    let (alpha, mut font) = crate::app::page_font::atlas(data)?;
    let texture = ShellHost::upload_page_font(&core.gl, &alpha)?;
    let id = register(core, assets, texture, 1024, 1024);
    for glyph in font.glyphs.values_mut() {
        glyph.page = id;
    }
    Ok(font)
}

fn look(theme: &str) -> usize {
    match theme {
        "bf1" => 0,
        "bf2" => 1,
        _ => 2,
    }
}

impl Live {
    pub fn admit(
        scene: &mut Scene,
        root: &Path,
        map: &str,
        mode: &str,
        theme: &str,
        profile: &str,
    ) -> Result<Self, String> {
        let canonical_root = root.canonicalize().map_err(|e| e.to_string())?;
        let root = canonical_root.as_path();
        let locale = std::env::var("LC_ALL")
            .ok()
            .filter(|v| !v.is_empty())
            .or_else(|| std::env::var("LC_MESSAGES").ok())
            .or_else(|| std::env::var("LANG").ok())
            .unwrap_or_default();
        let mut screens = Screens::open(root, locale.split('.').next().unwrap_or(""), theme)?;
        let classes = scene.online.as_ref().map_or(&[][..], |r| r.hud.class_meta.as_slice());
        let names = super::view::Names::new(profile, classes, &screens.catalog);
        let core = &mut scene.bridge.r.core;
        let assets = &mut scene.bridge.hud_model.assets;
        let bitmap = ScreenArt::bitmap_fonts(root, |path| upload(core, assets, path))?;
        let mut page = bfe_shell::page::Page::new(
            page_font(
                core,
                assets,
                include_bytes!("../../../../assets/fonts/saira-400.ttf"),
            )?,
            page_font(
                core,
                assets,
                include_bytes!("../../../../assets/fonts/saira-900.ttf"),
            )?,
        );
        page.regular = page_font(
            core,
            assets,
            include_bytes!("../../../../assets/fonts/saira-400.ttf"),
        )?;
        page.medium = page_font(
            core,
            assets,
            include_bytes!("../../../../assets/fonts/saira-500.ttf"),
        )?;
        page.bold = page_font(
            core,
            assets,
            include_bytes!("../../../../assets/fonts/saira-700.ttf"),
        )?;
        page.title = page_font(
            core,
            assets,
            include_bytes!("../../../../assets/fonts/saira-800.ttf"),
        )?;
        let mut fonts = ["bf1", "bf2", "eternal"]
            .map(|role| Some(ScreenArt::font_roles(role, &page, &bitmap, &bitmap)));
        let active = look(theme);
        let texture = ShellHost::upload_rgba(&core.gl, 1, 1, &[255; 4])?;
        let solid = register(core, assets, texture, 1, 1);
        let mut art = ScreenArt::load(
            &screens.catalog,
            fonts[active].take().unwrap(),
            solid,
            |path| upload(core, assets, path),
        )?;
        art.set_game_fonts(ScreenArt::font_roles("bf1", &page, &bitmap, &bitmap));
        let catalog = &mut screens.catalog;
        load_map_hud_catalog(root, map, &mut catalog.meshes, &mut catalog.bounds, |key, path| {
            art.insert(key.to_owned(), upload(core, assets, path)?);
            Ok(())
        })?;
        for scope in ["weapon_scope2", "weapon_scope3", "weapon_scope4"] {
            for side in ["all", "cis", "imp", "rep", "tur"] {
                let rel = format!("sides/{side}/textures/{scope}.png");
                if let Ok(path) = child(root, &rel) {
                    if let Ok(uploaded) = upload(core, assets, &path) {
                        art.insert(scope.to_owned(), uploaded);
                        break;
                    }
                }
            }
        }
        let map_name = format!("mapname.name.{map}");
        let mut map_image = String::new();
        let mut map_asset = None;
        let mut flight = super::flight::FlightMap::default();
        if let Some(chart) = &scene.flight_chart {
            let path = child(root, &format!("maps/{map}/{}", chart.path))?;
            map_image.clone_from(&chart.path);
            let uploaded = upload(core, assets, &path)?;
            map_asset = Some(uploaded.0);
            art.insert(map_image.clone(), uploaded);
            flight.chart_extent = Some((chart.center, chart.half));
            flight.symbols = super::flight::radar_symbols(root, map, &chart.symbols, |key, path| {
                art.insert(key.to_owned(), upload(core, assets, path)?);
                Ok(())
            })?;
        } else {
            let map_dir = child(root, &format!("maps/{map}/map.json"))?;
            let record: MapImages = serde_json::from_reader(std::io::BufReader::new(
                std::fs::File::open(&map_dir).map_err(|e| e.to_string())?,
            ))
            .map_err(|e| e.to_string())?;
            let key = format!("{map}_map");
            if let Some(file) = record
                .textures
                .get(&key)
                .and_then(serde_json::Value::as_str)
            {
                let path = child(map_dir.parent().unwrap(), file)?;
                let uploaded = upload(core, assets, &path)?;
                map_asset = Some(uploaded.0);
                art.insert(key.clone(), uploaded);
                map_image = key;
            }
        }
        core.state.reset();
        // The spawn launcher's hologram map draws the HUD's map with the mission's own picture.
        if let (Some(id), Some(replicas)) = (map_asset, scene.online.as_mut()) {
            eprintln!("launcher map: picture {id}, map {:?}, chart {}", replicas.hud.map, scene.flight_chart.is_some());
            match (&mut replicas.hud.map, &scene.flight_chart) {
                (Some(map), _) => map[4] = f64::from(id),
                (None, Some(chart)) => {
                    let half = chart.half[0].max(chart.half[1]);
                    replicas.hud.map = Some([chart.center[0], chart.center[1], half, 1., f64::from(id)]);
                }
                (None, None) => {}
            }
        }
        let mode = match mode {
            "eli" => bfe_battle_ui::battle2::Mode::HeroAssault,
            "ass" => bfe_battle_ui::battle2::Mode::SpaceAssault,
            _ => bfe_battle_ui::battle2::Mode::Other,
        };
        Ok(Self {
            flight,
            binding_revision: None,
            screens,
            names,
            art,
            mode,
            map_name,
            map_image,
            fonts,
            look: active,
            end_camera: None,
            team_labels: None,
            feed: Default::default(),
        })
    }

    pub fn apply_bindings(&mut self, settings: &bfe_interface::Settings) {
        if self.binding_revision != Some(settings.revision) {
            super::apply_screen_bindings(&mut self.screens, settings);
            self.binding_revision = Some(settings.revision);
        }
    }

    pub fn prepare_team_labels(&mut self, observation: &crate::local::mission_view::Observation) {
        self.team_labels.get_or_insert_with(|| {
            std::array::from_fn(|i| {
                let name = &observation.admission.teams[i].name;
                let stem = name.to_ascii_lowercase();
                let keys = [
                    format!("common.sides.{stem}.name"),
                    format!("common.sides.{stem}s.name"),
                    format!("common.sides.{stem}es.name"),
                ];
                keys.iter()
                    .find_map(|key| self.screens.catalog.localized(key))
                    .unwrap_or(name)
                    .to_owned()
            })
        });
    }

    pub fn set_theme(&mut self, theme: &str) {
        let next = look(theme);
        if next != self.look {
            let roles = self.fonts[next].take().unwrap();
            self.fonts[self.look] = Some(std::mem::replace(&mut self.art.fonts, roles));
            self.screens.style = Style::for_theme(theme);
            self.look = next;
        }
    }
}

#[derive(serde::Deserialize)]
struct MapImages {
    #[serde(default)]
    textures: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(serde::Deserialize)]
struct MapHudCatalog {
    #[serde(default)]
    textures: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    meshes: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    bounds: std::collections::BTreeMap<String, [f64; 4]>,
}

/// Merge per-map class/weapon HUD icons prepared in `maps/<map>/hud/catalog.json`
/// into the active battle catalog and uploaded screen textures.
fn load_map_hud_catalog(
    root: &Path,
    map_id: &str,
    meshes: &mut std::collections::HashMap<String, String>,
    bounds: &mut std::collections::HashMap<String, [f64; 4]>,
    mut upload: impl FnMut(&str, &Path) -> Result<(), String>,
) -> Result<(), String> {
    let rel = format!("maps/{map_id}/hud/catalog.json");
    if !root.join(&rel).try_exists().map_err(|e| e.to_string())? {
        return Ok(());
    }
    let catalog = child(root, &rel)?;
    let folder = catalog.parent().ok_or("missing map hud catalog directory")?;
    let mut bytes = Vec::new();
    std::fs::File::open(&catalog)
        .and_then(|f| f.take((1 << 20) + 1).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    if bytes.len() > 1 << 20 {
        return Err("oversized map hud catalog".into());
    }
    let index: MapHudCatalog = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if index.textures.len() > 2048
        || index.meshes.len() > 4096
        || index.bounds.len() > 4096
        || index.bounds.values().flatten().any(|v| !v.is_finite() || v.abs() > 1e4)
    {
        return Err("oversized map hud catalog entries".into());
    }
    for (key, file) in &index.textures {
        if Path::new(file).file_name() != Some(std::ffi::OsStr::new(file)) {
            return Err("expected a map hud texture basename".into());
        }
        upload(&key.to_ascii_lowercase(), &child(folder, file)?)?;
    }
    for (alias, target) in index.meshes {
        meshes.insert(alias.to_ascii_lowercase(), target.to_ascii_lowercase());
    }
    for (key, extent) in index.bounds {
        bounds.insert(key.to_ascii_lowercase(), extent);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_map_hud_catalog_merges_map_meshes_and_rejects_traversal() {
        let root = std::env::temp_dir().join(format!("bfe-map-hud-cat-{}", std::process::id()));
        let folder = root.join("maps/tat2/hud");
        std::fs::create_dir_all(&folder).unwrap();
        let root = root.canonicalize().unwrap();
        std::fs::write(folder.join("flame.png"), []).unwrap();
        std::fs::write(
            folder.join("catalog.json"),
            br#"{"textures":{"mesh.hud_imp_flamethrower":"flame.png"},"meshes":{"hud_imp_flamethrower":"mesh.hud_imp_flamethrower"},"bounds":{"mesh.hud_imp_flamethrower":[-1.5,-0.25,2,0.5]}}"#,
        )
        .unwrap();
        let mut meshes = std::collections::HashMap::new();
        let mut bounds = std::collections::HashMap::new();
        let mut uploaded = Vec::new();
        load_map_hud_catalog(&root, "tat2", &mut meshes, &mut bounds, |key, path| {
            uploaded.push((key.to_owned(), path.to_owned()));
            Ok(())
        })
        .unwrap();
        assert_eq!(
            meshes.get("hud_imp_flamethrower").map(String::as_str),
            Some("mesh.hud_imp_flamethrower")
        );
        assert_eq!(bounds["mesh.hud_imp_flamethrower"], [-1.5, -0.25, 2., 0.5]);
        assert_eq!(
            uploaded,
            [(
                "mesh.hud_imp_flamethrower".into(),
                folder.join("flame.png").canonicalize().unwrap()
            )]
        );
        // Missing map catalog is a clean no-op.
        load_map_hud_catalog(&root, "absent", &mut meshes, &mut bounds, |_, _| panic!()).unwrap();
        // Traversal filename is rejected.
        std::fs::write(
            folder.join("catalog.json"),
            br#"{"textures":{"bad":"../flame.png"},"meshes":{}}"#,
        )
        .unwrap();
        assert!(load_map_hud_catalog(&root, "tat2", &mut meshes, &mut bounds, |_, _| Ok(())).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
