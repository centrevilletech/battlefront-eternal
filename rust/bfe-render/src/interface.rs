//! Bitmap interface geometry shared by battle HUD and other Rust screens.
//! Ports interface/batch.js, interface/fonts.js and interface/space.js. All
//! frame writes use retained storage; font pages and glyphs are cold assets.
use std::collections::HashMap;
use std::fmt::{self, Write};

pub const QUADS: usize = 2048;
pub type Matrix = [f64; 16];
pub type Color = [f64; 4];
pub const UV: [f64; 4] = [0., 0., 1., 1.];
pub const IDENTITY: Matrix = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];

// anchorMatrix: preserve the interface's fixed perspective and its y-down
// plane rather than rescaling groups with the physical canvas aspect.
pub fn anchor(ax: f64, ay: f64, safe: bool, height: f64) -> Matrix {
    let (ax, ay) = if safe { (0.05 + 0.9 * ax, 0.05 + 0.9 * ay) } else { (ax, ay) };
    [
        1.,
        0.,
        0.,
        0.,
        0.,
        -768. / height,
        0.,
        0.,
        0.,
        0.,
        1.,
        0.,
        (ax - 0.5) * 1024.,
        -(ay - 0.5) * 768.,
        -1024. / (2. * (std::f64::consts::PI / 6.).tan()),
        1.,
    ]
}

// Matrix4.multiplyMatrices, in the original operation order. Keeping f64
// until the final corner store matches Vector3.applyMatrix4's rounding.
pub fn mul(a: Matrix, b: Matrix) -> Matrix {
    let mut out = [0.; 16];
    for j in 0..4 {
        for i in 0..4 {
            out[j * 4 + i] =
                a[i] * b[j * 4] + a[4 + i] * b[j * 4 + 1] + a[8 + i] * b[j * 4 + 2] + a[12 + i] * b[j * 4 + 3];
        }
    }
    out
}
// translate/rotate_y/rotate_z/scale port the Matrix4 operations used by
// BattleHud. They multiply a full local matrix rather than editing a world
// translation in place: a panel's depth and tilt affect its final perspective.
// All matrices remain f64 until Batch writes the final vertex floats.
pub fn translate(a: Matrix, x: f64, y: f64, z: f64) -> Matrix {
    let mut b = IDENTITY;
    b[12] = x;
    b[13] = y;
    b[14] = z;
    mul(a, b)
}
pub fn rotate_y(a: Matrix, angle: f64) -> Matrix {
    let (s, c) = (angle.sin(), angle.cos());
    mul(a, [c, 0., -s, 0., 0., 1., 0., 0., s, 0., c, 0., 0., 0., 0., 1.])
}
pub fn rotate_z(a: Matrix, angle: f64) -> Matrix {
    let (s, c) = (angle.sin(), angle.cos());
    mul(a, [c, s, 0., 0., -s, c, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.])
}
pub fn scale(a: Matrix, x: f64) -> Matrix {
    mul(a, [x, 0., 0., 0., 0., x, 0., 0., 0., 0., x, 0., 0., 0., 0., 1.])
}
pub fn alpha(mut color: Color, a: f64) -> Color {
    color[3] *= a;
    color
}
pub fn mix(a: Color, b: Color, t: f64) -> Color {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3] + (b[3] - a[3]) * t]
}

#[derive(Clone, Copy, Default)]
pub struct Asset {
    pub texture: i32,
    pub width: f64,
    pub height: f64,
    pub ready: bool,
}
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Draw {
    pub first: usize,
    pub count: usize,
    pub texture: u32,
    pub mask: u32,
}

// Batch retains the prior frame's bytes even after its active count resets.
// A smaller new batch may leave old vertices beyond its draw range, but those
// vertices never acquire a draw record. A later larger batch compares and
// updates them normally. This mirrors Batch.begin/end without clearing arrays.
pub struct Batch {
    pub position: Vec<f32>,
    pub uv: Vec<f32>,
    pub mask_uv: Vec<f32>,
    pub tint: Vec<f32>,
    pub draws: Vec<Draw>,
    pub count: usize,
    pub start: usize,
    pub end: usize,
}
impl Default for Batch {
    fn default() -> Self {
        Self::new()
    }
}
impl Batch {
    pub fn new() -> Self {
        Self {
            position: vec![0.; QUADS * 12],
            uv: vec![0.; QUADS * 8],
            mask_uv: vec![0.; QUADS * 8],
            tint: vec![0.; QUADS * 16],
            draws: Vec::with_capacity(QUADS),
            count: 0,
            start: QUADS * 4,
            end: 0,
        }
    }
    // Batch.begin/end retain previous bytes so unchanged quads need no upload.
    pub fn begin(&mut self) {
        self.count = 0;
        self.draws.clear();
        self.start = QUADS * 4;
        self.end = 0;
    }
    // Retains the ported JS/GL call-site signature without an extra wrapper struct.
    #[allow(clippy::too_many_arguments)]
    pub fn quad(&mut self, m: Matrix, rect: [f64; 4], texture: u32, uv: [f64; 4], tint: Color, z: f64, mask: u32) {
        self.corners(m, rect, texture, [[uv[0], uv[1]], [uv[0], uv[3]], [uv[2], uv[1]], [uv[2], uv[3]]], tint, z, mask);
    }
    // Batch.quadCorners: the minimap's rotated art retains a fixed mask UV.
    #[allow(clippy::too_many_arguments)]
    pub fn corners(&mut self, m: Matrix, r: [f64; 4], texture: u32, uv: [[f64; 2]; 4], tint: Color, z: f64, mask: u32) {
        if texture == 0 || self.count >= QUADS {
            return;
        }
        let i = self.count;
        self.count += 1;
        let xy = [[r[0], r[1]], [r[0], r[3]], [r[2], r[1]], [r[2], r[3]]];
        // Batch.corner uses the original strip corner order: left/top, left/bottom,
        // right/top, right/bottom. The two index triangles can therefore remain the
        // same for every frame. Mask coordinates stay unrotated when minimap art UVs
        // rotate; ordinary quads use the same mask coordinates even when unmasked.
        for k in 0..4 {
            let (x, y) = (xy[k][0], xy[k][1]);
            let d = 1. / (m[3] * x + m[7] * y + m[11] * z + m[15]);
            let p = [
                ((m[0] * x + m[4] * y + m[8] * z + m[12]) * d) as f32,
                ((m[1] * x + m[5] * y + m[9] * z + m[13]) * d) as f32,
                ((m[2] * x + m[6] * y + m[10] * z + m[14]) * d) as f32,
            ];
            let u = [uv[k][0] as f32, uv[k][1] as f32];
            let mu = [(k / 2) as f32, (k % 2) as f32];
            let t = tint.map(|v| v as f32);
            let v = i * 4 + k;
            // Compare the values after their Float32 rounding. Comparing f64 corner
            // results to previous f32 bytes would mark almost every static vertex dirty.
            // One enclosing changed span is enough for all four GPU attribute buffers;
            // their differing strides are applied only when those mirrors are copied.
            if self.position[v * 3..v * 3 + 3] != p
                || self.uv[v * 2..v * 2 + 2] != u
                || self.mask_uv[v * 2..v * 2 + 2] != mu
                || self.tint[v * 4..v * 4 + 4] != t
            {
                self.position[v * 3..v * 3 + 3].copy_from_slice(&p);
                self.uv[v * 2..v * 2 + 2].copy_from_slice(&u);
                self.mask_uv[v * 2..v * 2 + 2].copy_from_slice(&mu);
                self.tint[v * 4..v * 4 + 4].copy_from_slice(&t);
                self.start = self.start.min(v);
                self.end = self.end.max(v + 1);
            }
        }
        // Batch.quad merges only adjacent runs with equal texture and mask handles.
        // Sorting all quads by texture would change translucent overlaps, especially
        // labels over panels and flashing flags over objective and unit markers.
        if let Some(last) = self.draws.last_mut() {
            if last.texture == texture && last.mask == mask {
                last.count += 6;
                return;
            }
        }
        self.draws.push(Draw { first: i * 6, count: 6, texture, mask });
    }
}

#[derive(Clone, Copy, Default)]
// Fonts.constructor resolves each page to a numeric asset handle once.
// Bounds are in pen coordinates with y down, while UVs stay in the imported
// atlas's coordinates. They are not re-normalized against upgraded atlas
// pixel dimensions, since those atlases preserve the source UV rectangles.
pub struct Glyph {
    pub page: u32,
    pub advance: f64,
    pub bounds: [f64; 4],
    pub uv: [f64; 4],
}
pub struct Font {
    pub height: f64,
    pub glyphs: HashMap<u16, Glyph>,
}
impl Default for Font {
    fn default() -> Self {
        Self { height: 18., glyphs: HashMap::new() }
    }
}
impl Font {
    // Fonts.width deliberately includes the trailing spacing and rounds each
    // advance separately, including scaled countdown and stat text.
    pub fn width(&self, text: &[u16], scale: f64, spacing: f64) -> f64 {
        let mut width = 0.;
        for ch in text {
            if let Some(g) = self.glyphs.get(ch) {
                width += (g.advance * scale + 0.5).floor() + spacing;
            }
        }
        width
    }
    // Fonts.capsMiddle uses the capital A rather than half a line height when
    // available. Flight prompts are capital labels; centring their visible box
    // keeps the prompt in the same vertical band for fonts with descenders.
    pub fn caps_middle(&self) -> f64 {
        self.glyphs.get(&65).map_or(self.height / 2., |g| (g.bounds[1] + g.bounds[3]) / 2.)
    }
    // Fonts.draw: missing glyphs have neither a quad nor an advance. The
    // bitmap's box is rounded after scaling, while its baseline stays f64.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        batch: &mut Batch,
        m: Matrix,
        x: f64,
        y: f64,
        text: &[u16],
        tint: Color,
        align: u32,
        scale: f64,
        spacing: f64,
        z: f64,
    ) {
        // Fonts.draw measures before moving the pen. Right/centre alignment includes
        // the final spacing unit, as in the original interface. Glyph bounds and
        // advances each round after scaling; rounding the complete string width or
        // transform instead would shift countdown digits and scaled stat labels.
        let width = self.width(text, scale, spacing);
        let mut pen = match align {
            1 => x - width,
            2 => x - width * 0.5,
            _ => x,
        };
        for ch in text {
            if let Some(g) = self.glyphs.get(ch) {
                let r = g.bounds.map(|v| (v * scale + 0.5).floor());
                if g.bounds[0] < g.bounds[2] {
                    batch.quad(m, [pen + r[0], y + r[1], pen + r[2], y + r[3]], g.page, g.uv, tint, z, 0);
                }
                pen += (g.advance * scale + 0.5).floor() + spacing;
            }
        }
    }
}

// Number labels use stack storage, including the reserve slash and clock.
// This replaces String/format! in frame paths without a small-string heap.
pub struct Label {
    pub chars: [u16; 64],
    pub len: usize,
}
impl Default for Label {
    fn default() -> Self {
        Self { chars: [0; 64], len: 0 }
    }
}
impl Write for Label {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.encode_utf16() {
            if self.len == 64 {
                return Err(fmt::Error);
            }
            self.chars[self.len] = c;
            self.len += 1;
        }
        Ok(())
    }
}
impl Label {
    pub fn text(&self) -> &[u16] {
        &self.chars[..self.len]
    }
}
