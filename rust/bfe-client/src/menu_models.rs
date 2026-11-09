//! Common hologram models prepared from the same authored pack as the browser.
//! Geometry is recentered and normalized once; a battle view only places it.
use bfe_content_native::BattleContent;
pub struct Model {
    pub name: String,
    pub vertices: Vec<f32>,
    pub texture: u32,
    pub scroll: [f32; 2],
}
pub fn prepare(content: &mut BattleContent) -> Result<Vec<Model>, String> {
    let vertices =
        bfe_content_native::geometry::floats(&content.textures.read("common/holograms.bin")?)?;
    let indices =
        bfe_content_native::geometry::words(&content.textures.read("common/holograms_index.bin")?)?;
    let pack = bfe_content::models::model_pack(
        content.content.battle.common.get("holograms"),
        &vertices,
        &indices,
    )?;
    let mut out = Vec::new();
    for name in [
        "com_icon_republic",
        "com_icon_cis",
        "com_icon_imperial",
        "com_icon_alliance",
    ] {
        let Some(parts) = pack.get(name) else {
            continue;
        };
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for part in parts.iter() {
            for p in part.data.chunks_exact(8) {
                for k in 0..3 {
                    min[k] = min[k].min(p[k] as f64);
                    max[k] = max[k].max(p[k] as f64);
                }
            }
        }
        let center: [f64; 3] = std::array::from_fn(|k| (min[k] + max[k]) * 0.5);
        let extent = (max[0] - min[0]).max(max[1] - min[1]).max(1e-3);
        for part in parts.iter() {
            let source = part
                .field("texture")
                .and_then(bfe_content::Value::as_str)
                .ok_or("hologram has no texture")?;
            let path = content
                .texture_paths
                .get(&source.to_lowercase())
                .ok_or("hologram texture is absent")?;
            let texture = content.textures.load(path)? as u32;
            let scroll = part
                .field("scroll")
                .and_then(bfe_content::Value::as_array)
                .map_or([0.; 2], |s| {
                    [
                        s.first().and_then(bfe_content::Value::as_f64).unwrap_or(0.) as f32,
                        s.get(1).and_then(bfe_content::Value::as_f64).unwrap_or(0.) as f32,
                    ]
                });
            let mut data = Vec::with_capacity(part.index.len() * 5);
            for index in &part.index {
                let p = &part.data[*index as usize * 8..];
                for k in 0..3 {
                    data.push(((p[k] as f64 - center[k]) / extent) as f32);
                }
                data.extend_from_slice(&p[6..8]);
            }
            out.push(Model {
                name: name.into(),
                vertices: data,
                texture,
                scroll,
            });
        }
    }
    Ok(out)
}
