//! src/world.js::geometryFor, models.js::modelPack and actor.js::buildActor.
//! Geometry stays in authored order. Do not weld vertices or sort triangles:
//! those changes alter raster edges, material groups and collision identities.
use bfe_render::{core::MultiDraw, renderer::Renderer};
use glow::HasContext;
use serde_json::Value;

pub const MODEL_STRIDE: usize = 8;
pub const ACTOR_STRIDE: usize = 9;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scalar {
    Float,
    U16,
    U32,
    U8,
}
impl Scalar {
    pub fn gl(self) -> u32 {
        match self {
            Self::Float => glow::FLOAT,
            Self::U16 => glow::UNSIGNED_SHORT,
            Self::U32 => glow::UNSIGNED_INT,
            Self::U8 => glow::UNSIGNED_BYTE,
        }
    }
    pub fn bytes(self) -> usize {
        match self {
            Self::Float | Self::U32 => 4,
            Self::U16 => 2,
            Self::U8 => 1,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Attribute {
    pub name: String,
    pub buffer: usize,
    pub size: u32,
    pub scalar: Scalar,
    pub normalized: bool,
    pub stride: usize,
    pub offset: usize,
}
#[derive(Clone, Debug)]
pub struct Geometry {
    pub buffers: Vec<Vec<u8>>,
    pub attributes: Vec<Attribute>,
    pub indices: Vec<u32>,
    pub vertices: usize,
    pub groups: Vec<Group>,
}
#[derive(Clone, Copy, Debug)]
pub struct Group {
    pub first: usize,
    pub count: usize,
    pub material: usize,
}

// Binary words must not round-trip through JSON numbers. This keeps NaN bits,
// signed zero and authored Float32 positions unchanged at the GPU boundary.
pub fn floats(bytes: &[u8]) -> Result<Vec<f32>, String> {
    if !bytes.len().is_multiple_of(4) {
        return Err("float buffer is not word aligned".into());
    }
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect())
}
pub fn words(bytes: &[u8]) -> Result<Vec<u32>, String> {
    if !bytes.len().is_multiple_of(4) {
        return Err("index buffer is not word aligned".into());
    }
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect())
}
fn span(
    record: &Value,
    key: &str,
    count: usize,
    unit: usize,
    available: usize,
) -> Result<std::ops::Range<usize>, String> {
    let offset = record[key]
        .as_u64()
        .ok_or_else(|| format!("geometry has no {key}"))?;
    let start = usize::try_from(offset).map_err(|_| "geometry offset overflow")?;
    let size = count.checked_mul(unit).ok_or("geometry extent overflow")?;
    let end = start.checked_add(size).ok_or("geometry offset overflow")?;
    if start % 4 != 0 || end > available {
        return Err(format!("geometry {key} exceeds its buffer"));
    }
    Ok(start..end)
}
fn count(record: &Value, name: &str) -> Result<usize, String> {
    usize::try_from(
        record[name]
            .as_u64()
            .ok_or_else(|| format!("invalid geometry {name}"))?,
    )
    .map_err(|_| "geometry count overflow".into())
}
fn size(name: &str) -> Option<u32> {
    match name {
        "position" | "normal" | "tangent" | "binormal" => Some(3),
        "uv" => Some(2),
        "color" => Some(4),
        _ => None,
    }
}
impl Geometry {
    /// geometryFor slices a bucket's vertices but its stored indices are local.
    /// The sum of attribute widths must fit the declared interleaved stride.
    pub fn world(record: &Value, vertices: &[u8], indices: &[u8]) -> Result<Self, String> {
        let count = count(record, "vertexCount")?;
        let stride = record["stride"]
            .as_u64()
            .ok_or("invalid world vertex stride")? as usize;
        if !(3..=64).contains(&stride) {
            return Err("unsupported world vertex stride".into());
        }
        let data = vertices[span(
            record,
            "vertexOffset",
            count,
            stride.checked_mul(4).ok_or("stride overflow")?,
            vertices.len(),
        )?]
        .to_vec();
        let index_count = crate::geometry::count(record, "indexCount")?;
        let index = words(&indices[span(record, "indexOffset", index_count, 4, indices.len())?])?;
        let mut attributes = Vec::new();
        let mut offset = 0;
        for name in record["attributes"]
            .as_array()
            .ok_or("world geometry has no attributes")?
        {
            let name = name.as_str().ok_or("invalid attribute name")?;
            let width = size(name).ok_or_else(|| format!("unsupported world attribute {name}"))?;
            if offset + width as usize > stride {
                return Err("world attributes exceed their stride".into());
            }
            attributes.push(Attribute {
                name: name.into(),
                buffer: 0,
                size: width,
                scalar: Scalar::Float,
                normalized: false,
                stride: stride * 4,
                offset: offset * 4,
            });
            offset += width as usize;
        }
        let geometry = Self {
            buffers: vec![data],
            attributes,
            indices: index,
            vertices: count,
            groups: vec![Group {
                first: 0,
                count: index_count,
                material: 0,
            }],
        };
        geometry.validate()?;
        Ok(geometry)
    }
    /// modelPack rebases indices by the file-global first vertex, while leaving
    /// each part's interleaved position/normal/UV bytes untouched.
    pub fn model(record: &Value, vertices: &[u8], indices: &[u8]) -> Result<Self, String> {
        let vertex_count = count(record, "vertexCount")?;
        let index_count = count(record, "indexCount")?;
        let range = span(
            record,
            "vertexOffset",
            vertex_count,
            MODEL_STRIDE * 4,
            vertices.len(),
        )?;
        if range.start % (MODEL_STRIDE * 4) != 0 {
            return Err("model vertex offset is not a complete vertex".into());
        }
        let base = (range.start / (MODEL_STRIDE * 4)) as u32;
        let mut index =
            words(&indices[span(record, "indexOffset", index_count, 4, indices.len())?])?;
        for item in &mut index {
            *item = item
                .checked_sub(base)
                .ok_or("model index precedes its part")?;
        }
        let geometry = Self {
            buffers: vec![vertices[range].to_vec()],
            attributes: interleaved(MODEL_STRIDE),
            indices: index,
            vertices: vertex_count,
            groups: vec![Group {
                first: 0,
                count: index_count,
                material: 0,
            }],
        };
        geometry.validate()?;
        Ok(geometry)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.indices.iter().any(|&i| i as usize >= self.vertices) {
            return Err("geometry index exceeds vertex storage".into());
        }
        for a in &self.attributes {
            let buffer = self
                .buffers
                .get(a.buffer)
                .ok_or("attribute buffer is missing")?;
            let width = a.size as usize * a.scalar.bytes();
            let end = self
                .vertices
                .saturating_sub(1)
                .checked_mul(a.stride)
                .and_then(|n| n.checked_add(a.offset))
                .and_then(|n| n.checked_add(width))
                .ok_or("attribute extent overflow")?;
            if self.vertices > 0 && end > buffer.len() {
                return Err("attribute exceeds vertex storage".into());
            }
        }
        for group in &self.groups {
            if group
                .first
                .checked_add(group.count)
                .is_none_or(|end| end > self.indices.len())
            {
                return Err("geometry group exceeds index storage".into());
            }
        }
        Ok(())
    }
    /// Resolve named attributes once against the owned shader catalog. Draws
    /// retain the returned geometry id and never inspect these strings again.
    pub fn register<C: HasContext, M: MultiDraw>(
        &self,
        renderer: &mut Renderer<C, M>,
        program: &str,
    ) -> Result<u32, String> {
        self.validate()?;
        let spec = bfe_render::programs::spec(program)?;
        let id = renderer.core.add_geometry();
        for buffer in &self.buffers {
            renderer.core.geometry_buffer(id, buffer);
        }
        for &(name, location) in spec.attributes {
            let Some(a) = self.attributes.iter().find(|a| a.name == name) else {
                continue;
            };
            renderer.core.geometry_pointer(
                id,
                a.buffer as u32,
                location,
                a.size as i32,
                a.scalar.gl(),
                a.normalized,
                a.stride as i32,
                a.offset as i32,
            );
        }
        let index: Vec<u8> = self.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        renderer.core.geometry_index(id, &index, glow::UNSIGNED_INT);
        for group in &self.groups {
            renderer
                .core
                .geometry_group(id, group.first as i32, group.count as i32);
        }
        Ok(id)
    }
}
fn interleaved(stride: usize) -> Vec<Attribute> {
    [("position", 3, 0), ("normal", 3, 3), ("uv", 2, 6)]
        .into_iter()
        .map(|(name, size, offset)| Attribute {
            name: name.into(),
            buffer: 0,
            size,
            scalar: Scalar::Float,
            normalized: false,
            stride: stride * 4,
            offset: offset * 4,
        })
        .collect()
}
