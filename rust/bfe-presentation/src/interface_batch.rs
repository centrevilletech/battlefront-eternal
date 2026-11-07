//! Numeric interface quads share one phase for shell, spawn and scoreboard
//! surfaces. Resource identities and texture descriptors remain on the host.
use crate::interface_batch_layout::{interface_batch_frame as f,
    interface_batch_limits as lim, interface_quad as q};
pub struct Batch {
    pub frame: [f64; f::STRIDE],
    pub commands: Vec<f64>,
    pub position: Vec<f32>,
    pub uv: Vec<f32>,
    pub mask: Vec<f32>,
    pub tint: Vec<f32>,
    pub dirty: Vec<u8>,
}
// Loading reserves every command, attribute and dirty flag. A long frame
// can fill the bank but cannot request another allocation from this owner.
impl Default for Batch {
    fn default() -> Self {
        Self { frame: [0.0; f::STRIDE], commands: vec![0.0; lim::QUADS * q::STRIDE],
            position: vec![0.0; lim::QUADS * 12], uv: vec![0.0; lim::QUADS * 8],
            mask: vec![0.0; lim::QUADS * 8], tint: vec![0.0; lim::QUADS * 16],
            dirty: vec![0; lim::QUADS] }
    }
}
impl Batch {
    // Ports Batch.corner: retain double arithmetic through the perspective
    // divide, then compare the float32 value the GPU would actually hold.
    fn corner(&mut self, v: usize, a: &[f64], x: f64, y: f64, uv: usize, mu: usize, mv: usize) {
        let e = &a[q::MATRIX..q::MATRIX + 16];
        let z = a[q::DEPTH];
        let d = 1.0 / (e[3] * x + e[7] * y + e[11] * z + e[15]);
        let p = [((e[0] * x + e[4] * y + e[8] * z + e[12]) * d) as f32,
            ((e[1] * x + e[5] * y + e[9] * z + e[13]) * d) as f32,
            ((e[2] * x + e[6] * y + e[10] * z + e[14]) * d) as f32];
        let t = [a[q::TINT] as f32, a[q::TINT + 1] as f32, a[q::TINT + 2] as f32, a[q::TINT + 3] as f32];
        let u = [a[q::UV + uv] as f32, a[q::UV + uv + 1] as f32];
        let m = [a[q::MASK + mu] as f32, a[q::MASK + mv] as f32];
        let p3 = v * 3; let v2 = v * 2; let v4 = v * 4;
        // NaN remains dirty and signed zero compares equal, exactly as the
        // old JavaScript !== comparisons against Math.fround did.
        let changed = self.position[p3..p3 + 3] != p || self.uv[v2..v2 + 2] != u
            || self.mask[v2..v2 + 2] != m || self.tint[v4..v4 + 4] != t;
        if !changed { return; }
        self.position[p3..p3 + 3].copy_from_slice(&p);
        self.uv[v2..v2 + 2].copy_from_slice(&u);
        self.mask[v2..v2 + 2].copy_from_slice(&m);
        self.tint[v4..v4 + 4].copy_from_slice(&t);
        self.dirty[v / 4] = 1;
    }
    // Commands may target noncontiguous quads because the native HUD can
    // insert its already generated geometry between ordinary shell quads.
    pub fn step(&mut self) {
        // Counts are untrusted host facts. Limit them independently before
        // slicing either the command bank or the retained vertex arrays.
        let count = (self.frame[f::COUNT] as usize).min(lim::QUADS);
        let commands = (self.frame[f::COMMANDS] as usize).min(lim::QUADS);
        for i in 0..commands {
            // A stack copy avoids borrowing the input bank while writing
            // output. No command allocates or owns a temporary vertex list.
            let mut a = [0.0; q::STRIDE];
            a.copy_from_slice(&self.commands[i * q::STRIDE..(i + 1) * q::STRIDE]);
            let index = a[q::INDEX] as usize;
            // An excessive target is discarded rather than aliased onto
            // the last quad, which may belong to a different UI surface.
            if index >= count { continue; }
            let x0 = a[q::RECT]; let y0 = a[q::RECT + 1];
            let x1 = a[q::RECT + 2]; let y1 = a[q::RECT + 3];
            self.corner(index * 4, &a, x0, y0, 0, 0, 1);
            self.corner(index * 4 + 1, &a, x0, y1, 2, 0, 3);
            self.corner(index * 4 + 2, &a, x1, y0, 4, 2, 1);
            self.corner(index * 4 + 3, &a, x1, y1, 6, 2, 3);
        }
        // Ports Batch.end's single spanning upload range. Foreign native
        // HUD quads mark their copied output dirty through this same bank.
        let mut first = count; let mut last = 0;
        for i in 0..count {
            if self.dirty[i] != 0 { first = first.min(i); last = i + 1; }
        }
        self.frame[f::FIRST] = first as f64;
        self.frame[f::LAST] = last as f64;
        self.dirty[..count].fill(0);
    }
}
