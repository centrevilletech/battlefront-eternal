//! Cold foliage descriptors, formerly FoliageGpuBatch's Three geometry/material.
//! Authored cards arrive before any draw; Rust owns packing, atlas policy and
//! raster constants. The recovery journal retains those authored inputs, so a
//! lost context rebuilds the same bytes without constructing browser resources.
use serde::Deserialize;

pub const CARD: usize = crate::static_layout::CARD_STRIDE;
pub const VERTEX: usize = 16;
pub const MAX_CARDS: usize = crate::static_layout::CARD_MAX;
const GRASS_TURNS: [f64; 10] = [0., -0.05, 0.1, -0.15, 0.2, 0., 0.05, -0.1, 0.15, -0.2];
const LEAF_TURNS: [f64; 10] = [0., -1., 2., -3., 4., 0., 1., -2., 3., -4.];
const ATLAS: [[f64; 4]; 4] = [[0., 0.5, 0.5, 1.], [0.5, 0.5, 1., 1.], [0., 0., 0.5, 0.5], [0.5, 0., 1., 0.5]];

// Only authored class values cross the cold boundary; the page cannot choose
// vertex types, shader locations or alpha policy through a descriptor facade.
#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub struct Definition {
    pub height_scale: Option<f64>,
    pub num_parts: Option<u32>,
    pub alpha: Option<f64>,
    pub wiggle_speed: Option<f64>,
    pub wiggle_amount: Option<f64>,
}
pub struct Cards {
    pub vertices: Vec<f32>,
    pub index: Vec<u32>,
    pub wiggle: f32,
}
impl Cards {
    // FoliageGpuBatch.constructor: preserve each f64 operation until the old
    // Float32Array assignment. In particular the sunk grass UV and scaled leaf
    // height must not be rounded before their final arithmetic operation.
    pub fn new(grass: bool, cards: &[f64], def: &Definition) -> Result<Self, &'static str> {
        if !cards.len().is_multiple_of(CARD) || cards.len() / CARD > MAX_CARDS { return Err("foliage card capacity or stride"); }
        let count = cards.len() / CARD;
        let mut vertices = vec![0.; count * 4 * VERTEX];
        let mut index = Vec::with_capacity(count * 6);
        let scale = def.height_scale.unwrap_or(1.);
        // FoliageGpuBatch treated grass and leaves differently when NumParts
        // was absent: grass uses its atlas unless explicitly set to one.
        let atlas = if grass { def.num_parts.unwrap_or(4) != 1 } else { def.num_parts.unwrap_or(4) == 4 };
        for (i, c) in cards.as_chunks::<CARD>().0.iter().enumerate() {
            if c.iter().any(|v| !v.is_finite()) || c[8] < 0. || c[8] > 3. || c[8].fract() != 0. { return Err("invalid authored foliage card"); }
            let [u0, v0, u1, v1] = if atlas { ATLAS[c[8] as usize] } else { [0., 0., 1., 1.] };
            // A single-image class mirrors odd authored parts. Atlas classes
            // instead select the stock quadrant without changing winding.
            let mirror = !atlas && c[8] as u32 & 1 != 0;
            let (left, right) = if mirror { (u1, u0) } else { (u0, u1) };
            for j in 0..4 {
                // Sunk grass extends its bottom UV rather than shrinking the
                // card. Keep this calculation before the final f32 conversion.
                let bottom = if grass && c[1] < 0. { v1 + (v0 - v1) * (-c[1] / c[4]).min(1.) } else { v1 };
                let row = [c[0], if grass { c[1] } else { c[1] * scale }, c[2], c[3], c[3], c[3], def.alpha.unwrap_or(0.5),
                    if j % 2 != 0 { right } else { left }, if j < 2 { bottom } else { v0 },
                    if j % 2 != 0 { 0.5 } else { -0.5 }, if j < 2 { -0.5 } else { 0.5 },
                    c[4], if grass { c[5] } else { c[4] * scale }, if grass { GRASS_TURNS[i % 10] } else { LEAF_TURNS[i % 10] }, c[6],
                    if grass { c[7] } else { def.wiggle_speed.unwrap_or(1.2) }];
                for (k, v) in row.into_iter().enumerate() { vertices[(i * 4 + j) * VERTEX + k] = v as f32; }
            }
            // The old constructor used this fixed quad winding; changing it
            // changes cutout coverage where two cards overlap.
            let b = (i * 4) as u32;
            index.extend_from_slice(&[b, b + 2, b + 1, b + 1, b + 2, b + 3]);
        }
        Ok(Self { vertices, index, wiggle: def.wiggle_amount.unwrap_or(0.11) as f32 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atlas_mirror_sunk_grass_and_leaf_scale() {
        let c = [1., -0.25, 3., 0.7, 0.5, 0.9, 17., 1.3, 1.];
        let grass = Cards::new(true, &c, &Definition::default()).unwrap();
        assert_eq!(&grass.vertices[..16], &[1., -0.25, 3., 0.7, 0.7, 0.7, 0.5, 0.5, 0.75, -0.5, -0.5, 0.5, 0.9, 0., 17., 1.3]);
        assert_eq!(grass.index, [0, 2, 1, 1, 2, 3]);
        let def = Definition { num_parts: Some(1), height_scale: Some(1.7), ..Definition::default() };
        let leaf = Cards::new(false, &c, &def).unwrap();
        assert_eq!(leaf.vertices[1].to_bits(), (-0.425f32).to_bits());
        assert_eq!(&leaf.vertices[7..9], &[1., 1.]);
        assert_eq!(leaf.vertices[12].to_bits(), (0.85f32).to_bits());
        assert_eq!(leaf.vertices[15], 1.2);
    }
    // Cold admission fails clearly; no imported part or count can index past
    // the fixed atlas. Maximum cardinality also covers the largest class.
    #[test]
    fn capacities_and_bad_imports() {
        let c = [0., 0., 0., 1., 1., 1., 0., 1., 3.];
        let max = c.repeat(MAX_CARDS);
        let packed = Cards::new(true, &max, &Definition::default()).unwrap();
        assert_eq!(packed.index.last(), Some(&((MAX_CARDS * 4 - 1) as u32)));
        assert!(Cards::new(true, &c.repeat(MAX_CARDS + 1), &Definition::default()).is_err());
        assert!(Cards::new(true, &c[..8], &Definition::default()).is_err());
        let mut bad = c; bad[8] = 4.;
        assert!(Cards::new(true, &bad, &Definition::default()).is_err());
    }
}
