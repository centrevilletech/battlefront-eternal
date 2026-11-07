//! Water.surface's authored grid and clipped patches. Cold geometry preserves
//! the browser's vertex order, UVs and final Float32 conversion.
use serde_json::Value;
pub struct Surface {
    pub vertices: Vec<f32>,
    pub index: Vec<u32>,
}

/// OceanSurface.oceanGeometry: PlaneGeometry's exact grid winding and rotated
/// Float32 positions, followed by four flat outer-skirt quads.
pub fn ocean_geometry() -> Surface {
    let mut result = Surface {
        vertices: Vec::with_capacity((65 * 65 + 16) * 5),
        index: Vec::with_capacity(64 * 64 * 6 + 24),
    };
    let mut turn = three::Matrix4::default();
    turn.make_rotation_x(-std::f64::consts::FRAC_PI_2);
    for y in 0..=64 {
        for x in 0..=64 {
            let mut p = three::Vector3::new((x * 8) as f64 - 256., -((y * 8) as f64 - 256.), 0.);
            p.apply_matrix4(&turn);
            result
                .vertices
                .extend([p.x as f32, p.y as f32, p.z as f32, 0., 0.]);
        }
    }
    for y in 0..64 {
        for x in 0..64 {
            let a = x + 65 * y;
            let b = x + 65 * (y + 1);
            let c = x + 1 + 65 * (y + 1);
            let d = x + 1 + 65 * y;
            result.index.extend([a, b, d, b, c, d]);
        }
    }
    let inner = [[-256., -256.], [256., -256.], [256., 256.], [-256., 256.]];
    let outer = [
        [-4096., -4096.],
        [4096., -4096.],
        [4096., 4096.],
        [-4096., 4096.],
    ];
    for i in 0..4 {
        let j = (i + 1) % 4;
        let base = result.vertices.len() as u32 / 5;
        for p in [inner[i], inner[j], outer[j], outer[i]] {
            result.vertices.extend([p[0], 0., p[1], 0., 0.]);
        }
        result
            .index
            .extend([base, base + 2, base + 1, base, base + 3, base + 2]);
    }
    result
}
// Water.surface first emits regular cells, then appends imported clipped
// patches. A patched cell contributes no regular quad, avoiding overlapping
// geometry and doubled alpha where a canal meets its bank.
pub fn surface(data: &Value, divisions: usize, rows: usize) -> Result<Surface, String> {
    if divisions == 0 || rows == 0 || divisions > 128 || rows > 128 {
        return Err("invalid water subdivisions".into());
    }
    let cell = data["cell"].as_f64().ok_or("missing water cell")?;
    let height = data["height"].as_f64().ok_or("missing water height")?;
    let offsets: [f64; 2] =
        serde_json::from_value(data["offset"].clone()).map_err(|e| e.to_string())?;
    if !cell.is_finite()
        || cell <= 0.
        || !height.is_finite()
        || offsets.iter().any(|v| !v.is_finite())
    {
        return Err("invalid water dimensions".into());
    }
    // Check dimensions before multiplying the allocation budget. Subdivision
    // inputs come from authored look values, so rejecting an invalid grid is
    // preferable to allocating a partial surface with broken index offsets.
    let cells = data["cells"].as_array().ok_or("missing water cells")?;
    if cells
        .len()
        .checked_mul((divisions + 1) * (rows + 1))
        .is_none_or(|n| n > 8_388_608)
    {
        return Err("water vertex capacity".into());
    }
    let mut result = Surface {
        vertices: vec![],
        index: vec![],
    };
    for c in cells {
        if c[2].as_f64().unwrap_or(0.) != 0. {
            continue;
        }
        let x = c[0].as_f64().ok_or("invalid water column")? * cell - offsets[0];
        let z = c[1].as_f64().ok_or("invalid water row")? * cell - offsets[1];
        if !x.is_finite() || !z.is_finite() {
            return Err("nonfinite water cell".into());
        }
        // UVs restart at each cell, as the water shader's authored tile value
        // expects. Positions retain Float64 arithmetic through interpolation
        // and round only when assigned to the final typed vertex stream.
        let base = result.vertices.len() as u32 / 5;
        for b in 0..=rows {
            for a in 0..=divisions {
                result.vertices.extend(
                    [
                        x + a as f64 * cell / divisions as f64,
                        height,
                        z + b as f64 * cell / rows as f64,
                        a as f64 / divisions as f64,
                        b as f64 / rows as f64,
                    ]
                    .map(|v| v as f32),
                );
            }
        }
        let width = divisions as u32 + 1;
        for b in 0..rows {
            for a in 0..divisions {
                let k = base + b as u32 * width + a as u32;
                result
                    .index
                    .extend([k, k + width, k + 1, k + 1, k + width, k + width + 1]);
            }
        }
    }
    // Shore clips have their own UVs and local triangle indices. Rebase only
    // those indices; welding to the regular grid would alter the imported
    // shoreline and its interpolation even when positions nearly coincide.
    if let Some(patches) = data["patches"].as_array() {
        for patch in patches {
            let p: Vec<[f64; 3]> =
                serde_json::from_value(patch["positions"].clone()).map_err(|e| e.to_string())?;
            let uv: Vec<[f64; 2]> =
                serde_json::from_value(patch["uvs"].clone()).map_err(|e| e.to_string())?;
            let ix: Vec<u32> =
                serde_json::from_value(patch["indices"].clone()).map_err(|e| e.to_string())?;
            if p.len() != uv.len()
                || ix.len() % 3 != 0
                || ix.iter().any(|i| *i as usize >= p.len())
                || p.iter()
                    .flatten()
                    .chain(uv.iter().flatten())
                    .any(|v| !v.is_finite())
                || result.vertices.len() / 5 + p.len() > 8_388_608
            {
                return Err("invalid clipped water patch".into());
            }
            let base = result.vertices.len() as u32 / 5;
            for (p, uv) in p.iter().zip(uv) {
                result
                    .vertices
                    .extend([p[0], p[1], p[2], uv[0], uv[1]].map(|v| v as f32));
            }
            result.index.extend(ix.iter().map(|i| base + i));
        }
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grid_winding_and_clipped_patch_indices() {
        let mut data = serde_json::json!({"cell":32.,"height":-4.,"offset":[16.,16.],"cells":[[0,0,0],[1,0,1]],"patches":[{"positions":[[0.,-4.,0.],[1.,-4.,0.],[0.,-4.,1.]],"uvs":[[0.,0.],[1.,0.],[0.,1.]],"indices":[0,1,2]}]});
        let s = surface(&data, 1, 1).unwrap();
        assert_eq!(&s.vertices[..5], &[-16., -4., -16., 0., 0.]);
        assert_eq!(s.index, [0, 2, 1, 1, 2, 3, 4, 5, 6]);
        data["patches"][0]["indices"][2] = serde_json::json!(9);
        assert!(surface(&data, 1, 1).is_err());
        assert!(surface(&data, usize::MAX, 1).is_err());
    }
}
