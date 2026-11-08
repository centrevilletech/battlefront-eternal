//! Interface coordinate space, perspective plane, and 4x4 matrix transforms (`src/interface/space.js`).
//!
//! Every 2004-style screen is laid out in a virtual `1024 x H` plane (`H = 768` at `4:3`,
//! or `1024 / aspect` on wider/taller displays) sitting at `z = -PLANE` in front of a
//! 60-degree horizontal field-of-view perspective camera (`fov = pi / 3`, `tan(pi / 6)`).

pub use bfe_render::interface::{
    alpha, anchor, mix, mul, rotate_y, rotate_z, scale, translate, Batch, Color, Matrix, IDENTITY,
    UV,
};

/// Default solid white RGBA color `[1.0, 1.0, 1.0, 1.0]`.
pub const WHITE_COLOR: Color = [1.0, 1.0, 1.0, 1.0];

/// Virtual width of the 2004 interface plane (`IW` in `src/interface/space.js`).
pub const IW: f64 = 1024.0;

/// Base 4:3 virtual height (`IH = 768` in `src/interface/space.js`).
pub const IH: f64 = 768.0;

/// Horizontal field of view (`pi / 3` radians = 60 degrees, `2 * atan(TAN_X)` in `src/interface/space.js`).
pub const FOV: f64 = std::f64::consts::PI / 3.0;

/// Distance from the origin to the interface plane so `1024` units span the horizontal
/// frustum of a 60-degree horizontal FOV camera (`IW / (2 * tan(pi / 6))` in `space.js`).
pub const PLANE: f64 = 886.810_013_475_265_2;

/// Safe-zone margin fraction on each edge (`SAFE` in `src/interface/space.js`).
pub const SAFE: f64 = 0.05;

/// Scale factor mapping `[0, 1]` into the `[0.05, 0.95]` safe zone (`SAFE_SCALE` in `space.js`).
pub const SAFE_SCALE: f64 = 1.0 - SAFE * 2.0;

/// 2D point `[x, y]` in `1024 x H` interface coordinates.
pub type Point = [f64; 2];

/// Computes the virtual interface height `H` for a physical viewport aspect ratio `width / height`
/// (`interfaceHeight` in `src/interface/space.js`: `768 + (h / w - 0.75) * 768 == 1024 / aspect`).
#[inline]
pub fn interface_height(aspect: f64) -> f64 {
    if aspect.is_finite() && aspect > 0.0 {
        IW / aspect
    } else {
        IH
    }
}

/// Builds the 4x4 column-major placement matrix for a normalized screen anchor `(ax, ay)`
/// (`anchorMatrix(ax, ay, safe, H)` in `src/interface/space.js`).
#[inline]
pub fn anchor_matrix(ax: f64, ay: f64, safe: bool, height: f64) -> Matrix {
    bfe_render::interface::anchor(ax, ay, safe, height)
}

/// Post-multiplies `a` by a rotation of `angle` radians around the local X axis
/// (`THREE.Matrix4.makeRotationX` in `src/interface/space.js` and `rust/bfe-shell/src/render.rs`).
#[inline]
pub fn rotate_x(a: Matrix, angle: f64) -> Matrix {
    let (s, c) = (angle.sin(), angle.cos());
    mul(
        a,
        [
            1.0, 0.0, 0.0, 0.0, 0.0, c, s, 0.0, 0.0, -s, c, 0.0, 0.0, 0.0, 0.0, 1.0,
        ],
    )
}

/// Projects a widget-local 3D point `(x, y, z)` through `matrix` onto the `z = -PLANE`
/// interface plane (`planePoint(m, x, y, z, H)` in `src/interface/space.js`).
#[inline]
pub fn plane_point(matrix: &Matrix, x: f64, y: f64, z: f64, height: f64) -> Point {
    let e = matrix;
    let w = e[3] * x + e[7] * y + e[11] * z + e[15];
    let inv_w = if w != 0.0 { 1.0 / w } else { 1.0 };
    let wx = (e[0] * x + e[4] * y + e[8] * z + e[12]) * inv_w;
    let wy = (e[1] * x + e[5] * y + e[9] * z + e[13]) * inv_w;
    let wz = (e[2] * x + e[6] * y + e[10] * z + e[14]) * inv_w;
    let s = if wz != 0.0 { -PLANE / wz } else { 1.0 };
    [
        wx * s + IW * 0.5,
        -(wy * s) * (height / IH) + height * 0.5,
    ]
}

/// Intersects a camera ray at normalized device coordinates `(ndc_x, ndc_y)` (`-1..1`)
/// with the local `z = 0` plane of `matrix`, returning `[local_x, local_y]` for 3D hit-testing
/// of rotated menu widgets (`rot_y = 35` degrees on the main options menu).
pub fn local_point(matrix: &Matrix, ndc_x: f64, ndc_y: f64) -> Option<Point> {
    // Camera frustum tangents matching `TAN_X = tan(pi / 6)` and `TAN_Y = 0.75 * TAN_X` in `space.js`.
    let tan_half_fov = (FOV * 0.5).tan();
    let dir_x = ndc_x * tan_half_fov;
    let dir_y = ndc_y * 0.75 * tan_half_fov;
    let dir_z = -1.0;

    // Column-major matrix columns:
    // P(u, v) = col0 * u + col1 * v + col3
    // We solve: col0 * u + col1 * v - t * dir = -col3
    let a00 = matrix[0];
    let a10 = matrix[1];
    let a20 = matrix[2];

    let a01 = matrix[4];
    let a11 = matrix[5];
    let a21 = matrix[6];

    let a02 = -dir_x;
    let a12 = -dir_y;
    let a22 = -dir_z;

    let b0 = -matrix[12];
    let b1 = -matrix[13];
    let b2 = -matrix[14];

    // Solve the 3x3 linear system via Cramer's rule cofactors; reject grazing rays (`det ~ 0`)
    // and intersections behind the camera (`t <= 0`).
    let c00 = a11 * a22 - a12 * a21;
    let c01 = -(a10 * a22 - a12 * a20);
    let c02 = a10 * a21 - a11 * a20;
    let det = a00 * c00 + a01 * c01 + a02 * c02;
    if det.abs() < 1e-12 {
        return None;
    }
    let inv_det = 1.0 / det;

    let c10 = -(a01 * a22 - a02 * a21);
    let c11 = a00 * a22 - a02 * a20;
    let c12 = -(a00 * a21 - a01 * a20);

    let u = (b0 * c00 + b1 * c10 + b2 * (a01 * a12 - a02 * a11)) * inv_det;
    let v = (b0 * c01 + b1 * c11 + b2 * -(a00 * a12 - a02 * a10)) * inv_det;
    let t = (b0 * c02 + b1 * c12 + b2 * (a00 * a11 - a01 * a10)) * inv_det;

    if t <= 0.0 {
        return None;
    }
    Some([u, v])
}

/// Converts a client point `(px, py)` inside a `(vw, vh)` viewport to `1024 x height` coordinates
/// (`InterfaceSurface.toInterface` in `src/interface/surface.js`).
#[inline]
pub fn to_interface(px: f64, py: f64, vw: f64, vh: f64, height: f64) -> Point {
    let w = vw.max(1.0);
    let h = vh.max(1.0);
    [(px / w) * IW, (py / h) * height]
}

/// Computes `(IW, interface_height(vw / vh))` for a `(vw, vh)` viewport
/// (`IW` and `interfaceHeight` in `src/interface/space.js`).
#[inline]
pub fn to_interface_size(vw: f64, vh: f64) -> Point {
    let aspect = if vh > 0.0 { vw / vh } else { 4.0 / 3.0 };
    [IW, interface_height(aspect)]
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verifies `PLANE`, `interface_height`, `plane_point`, and `local_point` against `space.js`.
    #[test]
    fn space_projection_matches_js_plane_geometry() -> Result<(), Box<dyn std::error::Error>> {
        let exact_plane = IW / (2.0 * (std::f64::consts::PI / 6.0).tan());
        assert!((PLANE - exact_plane).abs() < 1e-12);
        assert!((interface_height(4.0 / 3.0) - 768.0).abs() < 1e-9);

        let m_unpadded = anchor_matrix(0.5, 0.5, false, 768.0);
        let center = plane_point(&m_unpadded, 0.0, 0.0, 0.0, 768.0);
        assert!((center[0] - 512.0).abs() < 1e-6);
        assert!((center[1] - 384.0).abs() < 1e-6);

        let pt = local_point(&m_unpadded, 0.0, 0.0).ok_or("ray must hit plane center")?;
        assert!(pt[0].abs() < 1e-6);
        assert!(pt[1].abs() < 1e-6);
        Ok(())
    }
}
