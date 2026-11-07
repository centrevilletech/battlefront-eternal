//! Catalog the native loader's descriptors for capture admission. Matching is
//! by raw-byte digest only at load: a render command then borrows the prepared
//! asset, never a browser-decoded copy. Procedural buffers remain archive data.
use bfe_content_native::BattleContent;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
#[derive(serde::Deserialize)]
pub struct TextureRequest {
    pub path: String,
    pub png: bool,
}

pub fn prepare(content: &mut BattleContent, requests: &[TextureRequest]) -> Result<(), String> {
    for request in requests {
        if !request.png {
            content.textures.load(&request.path)?;
        }
    }
    Ok(())
}

pub struct Assets {
    pub data: BTreeMap<String, Vec<u8>>,
}
impl Assets {
    pub fn new(content: &BattleContent, requests: &[TextureRequest]) -> Result<Self, String> {
        let mut data = BTreeMap::new();
        for (family, meshes) in [("world", &content.world), ("model", &content.models)] {
            for (i, mesh) in meshes.iter().enumerate() {
                for (j, buffer) in mesh.geometry.buffers.iter().enumerate() {
                    data.insert(format!("{family}/{i}/buffer/{j}"), buffer.clone());
                }
                data.insert(
                    format!("{family}/{i}/index"),
                    mesh.geometry
                        .indices
                        .iter()
                        .flat_map(|v| v.to_le_bytes())
                        .collect(),
                );
            }
        }
        for (i, mesh) in content.actors.iter().enumerate() {
            for (j, buffer) in mesh.geometry.buffers.iter().enumerate() {
                data.insert(format!("actor/{i}/buffer/{j}"), buffer.clone());
            }
            data.insert(
                format!("actor/{i}/index"),
                mesh.geometry
                    .indices
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect(),
            );
        }
        for image in &content.textures.images {
            data.insert(
                format!("image/{}/encoded", image.path),
                content.textures.read(&image.path)?,
            );
            data.insert(format!("image/{}/pixels", image.path), image.data.clone());
        }
        for (name, effects) in &content.effects {
            data.insert(
                format!("effects/{name}/tracks"),
                effects
                    .gpu_tracks
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect(),
            );
        }
        for request in requests {
            if request.png {
                data.insert(
                    format!("file/{}", request.path),
                    content.textures.read(&request.path)?,
                );
            }
        }
        Ok(Self { data })
    }
    pub fn hashes(&self) -> BTreeMap<String, &str> {
        self.data
            .iter()
            .map(|(name, data)| (format!("{:x}", Sha256::digest(data)), name.as_str()))
            .collect()
    }
}
