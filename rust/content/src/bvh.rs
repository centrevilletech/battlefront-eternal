//! The indirect CENTER builder used by collision.js: three-mesh-bvh 0.8.3,
//! full indexed geometry, depth 40, ten triangles per leaf. Intermediate
//! bounds are Float32 stores; partitioning and node packing retain their
//! original order so gathers visit exactly the same triangles.
//
// Portions translated from three-mesh-bvh, Copyright (c) 2018 Garrett Johnson.
// MIT License
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

use crate::install::BvhTree;

pub enum Positions<'a> {
    F32(&'a [f32]),
    F64(&'a [f64]),
}
impl Positions<'_> {
    fn at(&self, index: usize) -> f64 {
        match self {
            Self::F32(values) => values.get(index).map_or(f64::NAN, |n| *n as f64),
            Self::F64(values) => values.get(index).copied().unwrap_or(f64::NAN),
        }
    }
}

const EPSILON: f64 = 1. / 16777216.;
const MAX_DEPTH: usize = 40;
const LEAF_TRIANGLES: usize = 10;

fn triangle_bounds(positions: Positions<'_>, indices: &[u32]) -> Vec<[f32; 6]> {
    indices
        .chunks_exact(3)
        .map(|tri| {
            let mut bounds = [0.; 6];
            for axis in 0..3 {
                let a = positions.at(tri[0] as usize * 3 + axis);
                let b = positions.at(tri[1] as usize * 3 + axis);
                let c = positions.at(tri[2] as usize * 3 + axis);
                let mut min = a;
                if b < min {
                    min = b;
                }
                if c < min {
                    min = c;
                }
                let mut max = a;
                if b > max {
                    max = b;
                }
                if c > max {
                    max = c;
                }
                let half = (max - min) / 2.;
                bounds[axis * 2] = (min + half) as f32;
                bounds[axis * 2 + 1] = (half + (min.abs() + half) * EPSILON) as f32;
            }
            bounds
        })
        .collect()
}

fn bounds(triangles: &[[f32; 6]]) -> ([f32; 6], [f32; 6]) {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    let mut center_low = low;
    let mut center_high = high;
    for triangle in triangles {
        for axis in 0..3 {
            let center = triangle[axis * 2] as f64;
            let half = triangle[axis * 2 + 1] as f64;
            let left = center - half;
            let right = center + half;
            if left < low[axis] {
                low[axis] = left;
            }
            if right > high[axis] {
                high[axis] = right;
            }
            if center < center_low[axis] {
                center_low[axis] = center;
            }
            if center > center_high[axis] {
                center_high[axis] = center;
            }
        }
    }
    let mut node = [0.; 6];
    let mut centers = [0.; 6];
    for axis in 0..3 {
        node[axis] = low[axis] as f32;
        node[axis + 3] = high[axis] as f32;
        centers[axis] = center_low[axis] as f32;
        centers[axis + 3] = center_high[axis] as f32;
    }
    (node, centers)
}

fn partition(
    triangles: &mut [[f32; 6]],
    indirect: &mut [u32],
    offset: usize,
    count: usize,
    axis: usize,
    position: f64,
) -> usize {
    let mut left = offset as isize;
    let mut right = (offset + count) as isize - 1;
    loop {
        while left <= right && (triangles[left as usize][axis * 2] as f64) < position {
            left += 1;
        }
        while left <= right && (triangles[right as usize][axis * 2] as f64) >= position {
            right -= 1;
        }
        if left >= right {
            return left as usize;
        }
        indirect.swap(left as usize, right as usize);
        triangles.swap(left as usize, right as usize);
        left += 1;
        right -= 1;
    }
}

fn node(
    triangles: &mut [[f32; 6]],
    indirect: &mut [u32],
    output: &mut Vec<u32>,
    offset: usize,
    count: usize,
    depth: usize,
) {
    let (box_bounds, centers) = bounds(&triangles[offset..offset + count]);
    let at = output.len();
    output.extend(box_bounds.map(f32::to_bits));
    output.extend([0, 0]);
    let mut split = None;
    if count > LEAF_TRIANGLES && depth < MAX_DEPTH {
        let mut distance = f64::NEG_INFINITY;
        for axis in 0..3 {
            let d = centers[axis + 3] as f64 - centers[axis] as f64;
            if d > distance {
                distance = d;
                split = Some(axis);
            }
        }
    }
    if let Some(axis) = split {
        let position = (centers[axis] as f64 + centers[axis + 3] as f64) / 2.;
        let middle = partition(triangles, indirect, offset, count, axis, position);
        if middle != offset && middle != offset + count {
            node(
                triangles,
                indirect,
                output,
                offset,
                middle - offset,
                depth + 1,
            );
            output[at + 6] = output.len() as u32;
            node(
                triangles,
                indirect,
                output,
                middle,
                offset + count - middle,
                depth + 1,
            );
            output[at + 7] = axis as u32;
            return;
        }
    }
    output[at + 6] = offset as u32;
    // The packed count is Uint16 even for an unsplit degenerate leaf.
    output[at + 7] = 0xffff0000 | (count as u16 as u32);
}

/// Indexed BufferGeometry with its full draw range and indirect=true.
/// No index is reordered; only the indirect map and its parallel bounds are.
pub fn build(positions: Positions<'_>, indices: &[u32]) -> BvhTree {
    let mut triangles = triangle_bounds(positions, indices);
    let count = triangles.len();
    let mut indirect = (0..count as u32).collect::<Vec<_>>();
    let mut root = Vec::new();
    node(&mut triangles, &mut indirect, &mut root, 0, count, 0);
    BvhTree {
        roots: vec![root],
        indirect,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_and_degenerate_leaf_buffers_keep_the_wire_layout() {
        let empty = build(Positions::F32(&[]), &[]);
        assert_eq!(
            empty.roots,
            vec![vec![
                f32::INFINITY.to_bits(),
                f32::INFINITY.to_bits(),
                f32::INFINITY.to_bits(),
                f32::NEG_INFINITY.to_bits(),
                f32::NEG_INFINITY.to_bits(),
                f32::NEG_INFINITY.to_bits(),
                0,
                0xffff0000
            ]]
        );
        for count in [65535, 65536, 65537] {
            let tree = build(Positions::F64(&[-0., 0., -0.]), &vec![0; count * 3]);
            assert_eq!(tree.roots[0].len(), 8);
            assert_eq!(tree.roots[0][7], 0xffff0000 | (count as u16 as u32));
            assert_eq!(tree.indirect, (0..count as u32).collect::<Vec<_>>());
        }
    }
}
