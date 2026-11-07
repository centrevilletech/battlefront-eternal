//! TextureLibrary.prepare/load without fetch, DOM images or a worker heap.
//! PNG, lossless WebP and KTX2 use the same Rust decoders and GPU format policy
//! as the page. A declared compressed twin that fails is a loading error.
use bfe_render::texture_format::{Capabilities, Format};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct Mip {
    pub width: u32,
    pub height: u32,
    pub level: u32,
    pub layer: u32,
    pub face: u32,
    pub offset: usize,
    pub length: usize,
}
pub struct Image {
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub format: Format,
    pub premultiplied: bool,
    pub data: Vec<u8>,
    pub mips: Vec<Mip>,
}
// Samplers are separate descriptors: a repeated or clamped clone shares the
// decoded pixels, just as RenderResources separates Source and Texture.
#[derive(Clone, Copy, Debug)]
pub struct Sampler {
    pub wrap_s: u32,
    pub wrap_t: u32,
    pub min_filter: u32,
    pub mag_filter: u32,
    pub anisotropy: f32,
    pub generate_mipmaps: bool,
}
impl Default for Sampler {
    fn default() -> Self {
        Self {
            wrap_s: glow::REPEAT,
            wrap_t: glow::REPEAT,
            min_filter: glow::LINEAR_MIPMAP_LINEAR,
            mag_filter: glow::LINEAR,
            anisotropy: 1.0,
            generate_mipmaps: true,
        }
    }
}
pub struct Library {
    root: PathBuf,
    compressed: bool,
    max_size: u32,
    capabilities: u32,
    folders: BTreeMap<String, BTreeMap<String, String>>,
    ids: BTreeMap<String, usize>,
    pub images: Vec<Image>,
}
impl Library {
    pub fn new(
        root: impl Into<PathBuf>,
        compressed: bool,
        max_size: u32,
        capabilities: u32,
    ) -> Result<Self, String> {
        let root = root
            .into()
            .canonicalize()
            .map_err(|e| format!("content directory: {e}"))?;
        if max_size == 0 {
            return Err("texture device limit is zero".into());
        }
        Ok(Self {
            root,
            compressed,
            max_size,
            capabilities,
            folders: BTreeMap::new(),
            ids: BTreeMap::new(),
            images: Vec::new(),
        })
    }
    /// Prepared paths are content-relative names. Query strings are a browser
    /// cache detail; traversal and external schemes are never asset identifiers.
    pub fn path(&self, name: &str) -> Result<PathBuf, String> {
        let name = name
            .split('?')
            .next()
            .unwrap_or(name)
            .trim_start_matches('/');
        if name.contains(':')
            || name.contains('\\')
            || Path::new(name)
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err("invalid content-relative path".into());
        }
        let path = self.root.join(name);
        if let Ok(resolved) = path.canonicalize() {
            if !resolved.starts_with(&self.root) {
                return Err("content path escapes its directory".into());
            }
        }
        Ok(path)
    }
    pub fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        std::fs::read(self.path(name)?).map_err(|e| format!("asset {name}: {e}"))
    }
    fn twins(&mut self, folder: &str) -> Result<(), String> {
        if self.folders.contains_key(folder) {
            return Ok(());
        }
        let mut entries = BTreeMap::new();
        if self.compressed {
            let path = self.path(&format!("{folder}/ktx2.json"))?;
            match std::fs::read(path) {
                Ok(bytes) => {
                    let manifest: Value =
                        serde_json::from_slice(&bytes).map_err(|e| format!("KTX2 index: {e}"))?;
                    let files = manifest["files"]
                        .as_object()
                        .ok_or("KTX2 index has no files object")?;
                    for (png, record) in files {
                        let twin = record["ktx2"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .ok_or("invalid KTX2 index entry")?;
                        if !png.to_ascii_lowercase().ends_with("_bump.png") {
                            entries.insert(png.clone(), twin.into());
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(format!("KTX2 index: {error}")),
            }
        }
        self.folders.insert(folder.into(), entries);
        Ok(())
    }
    pub fn load(&mut self, requested: &str) -> Result<usize, String> {
        let requested = requested.trim_start_matches('/');
        if let Some(&id) = self.ids.get(requested) {
            return Ok(id);
        }
        let (folder, file) = requested
            .rsplit_once('/')
            .ok_or("texture has no content folder")?;
        self.twins(folder)?;
        let path = self.folders[folder]
            .get(file)
            .map(|t| format!("{folder}/{t}"))
            .unwrap_or_else(|| requested.into());
        let bytes = self.read(&path)?;
        let image = if path.to_ascii_lowercase().ends_with(".ktx2") {
            let mut decoder = basis_transcode::Decoder::new(bytes, self.max_size)?;
            let info = decoder.info();
            if info.faces != 1 || info.layers > 1 {
                return Err(
                    "KTX2 texture arrays and cubes are not supported by TextureLibrary".into(),
                );
            }
            let format = bfe_render::texture_format::choose(
                Capabilities::from_bits(self.capabilities),
                info.uastc,
                info.alpha,
                info.width,
                info.height,
                self.capabilities & 64 != 0,
            );
            let decoded = decoder.transcode(format.transcode())?;
            Image {
                path,
                width: info.width,
                height: info.height,
                format,
                premultiplied: info.premultiplied,
                mips: decoded
                    .mips
                    .iter()
                    .map(|m| Mip {
                        width: m.width,
                        height: m.height,
                        level: m.level,
                        layer: m.layer,
                        face: m.face,
                        offset: m.bytes.start,
                        length: m.bytes.len(),
                    })
                    .collect(),
                data: decoded.data,
            }
        } else {
            // Straight authored bytes are used on the GPU. Canvas alpha/gamma
            // conversion belongs only to explicit CPU colour queries.
            let decoded = bfe_render::texture_image::decode_image(&bytes, self.max_size)?;
            let length = decoded.rgba.len();
            Image {
                path,
                width: decoded.width,
                height: decoded.height,
                format: Format::Rgba,
                premultiplied: false,
                data: decoded.rgba,
                mips: vec![Mip {
                    width: decoded.width,
                    height: decoded.height,
                    level: 0,
                    layer: 0,
                    face: 0,
                    offset: 0,
                    length,
                }],
            }
        };
        let id = self.images.len();
        self.images.push(image);
        self.ids.insert(requested.into(), id);
        Ok(id)
    }
}
