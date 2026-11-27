//! Share vertex streams across a body's material programs. A skinned record
//! carries up to four diffuse ranges; transparent parts keep their own order.
use super::{draw, material_row, Bridge, Row};
use bfe_content_native::{
    geometry::{Geometry, Group},
    loader::{Blend, Material},
};

struct Batch {
    row: Row,
    key: String,
    ranges: Vec<(Group, f64)>,
}

pub(super) fn register(
    bridge: &mut Bridge,
    textures: &[u32],
    mesh: &Geometry,
    materials: Vec<(Group, Material)>,
) -> Result<Vec<Row>, String> {
    mesh.validate()?;
    let mut batches: Vec<Batch> = Vec::new();
    for (group, material) in materials {
        let opaque = material.blend == Blend::Opaque;
        let (mut row, key) = material_row(bridge, textures, material, true)?;
        let diffuse = row[draw::REC_TEXTURES];
        row[draw::REC_TEXTURES] = -1.;
        if let Some(batch) = batches
            .iter_mut()
            .find(|b| opaque && b.row == row && b.ranges.len() < 4)
        {
            batch.ranges.push((group, diffuse));
        } else {
            batches.push(Batch {
                row,
                key,
                ranges: vec![(group, diffuse)],
            });
        }
    }
    let core = &mut bridge.r.core;
    let mut source = None;
    let mut rows = Vec::with_capacity(batches.len());
    for mut batch in batches {
        let id = core.add_geometry();
        let owner = *source.get_or_insert_with(|| {
            for bytes in &mesh.buffers {
                core.geometry_buffer(id, bytes);
            }
            // A body without authored tint draws with a multiplier of one.
            core.geometry_buffer(id, &bfe_content_native::geometry::neutral_tint(mesh.vertices));
            id
        });
        for &(name, location) in bfe_render::programs::spec(&batch.key)?.attributes {
            if name == bfe_content_native::geometry::NEUTRAL_TINT && !mesh.attributes.iter().any(|a| a.name == name) {
                core.geometry_pointer_from(id, owner, mesh.buffers.len(), location, 4,
                    bfe_content_native::geometry::Scalar::U8.gl(), true, 0, 0);
                continue;
            }
            if let Some(a) = mesh.attributes.iter().find(|a| a.name == name) {
                core.geometry_pointer_from(
                    id,
                    owner,
                    a.buffer,
                    location,
                    a.size as i32,
                    a.scalar.gl(),
                    a.normalized,
                    a.stride as i32,
                    a.offset as i32,
                );
            }
        }
        let mut indices = Vec::new();
        for (n, (group, diffuse)) in batch.ranges.iter().enumerate() {
            core.geometry_group(id, (indices.len() / 4) as i32, group.count as i32);
            let span = &mesh.indices[group.first..group.first + group.count];
            indices.extend(span.iter().flat_map(|i| i.to_le_bytes()));
            batch.row[draw::REC_TEXTURES + n] = *diffuse;
        }
        core.geometry_index(id, &indices, glow::UNSIGNED_INT);
        batch.row[draw::REC_GEOMETRY] = f64::from(id);
        batch.row[draw::REC_GROUPS] = batch.ranges.len() as f64;
        rows.push(batch.row);
    }
    Ok(rows)
}
