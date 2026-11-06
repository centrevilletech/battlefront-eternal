//! The rain cover cache (src/weather.js RainCover): a small toroidal grid of
//! the height of the first roof or floor over each cell around the camera,
//! shared by every drop and flake. Only cells newly entered are asked of the
//! page's collision world, a few at a time; the shader clips what falls
//! below the cover.

/// A cell's key before it holds any: no real cell has it.
const NONE: i32 = 2147483647;
/// The height of a cell with no roof or floor under it.
pub const OPEN: f32 = -100000.0;

pub struct Cover {
    pub size: usize,
    pub cell: f64,
    /// The cache's heights: `size` squared of them in use, room for the
    /// largest size the owner will switch to.
    pub heights: Vec<f32>,
    keys_x: Vec<i32>,
    keys_z: Vec<i32>,
    band: Option<f64>,
    /// Cells asked of the collision world in the last update.
    pub queries: usize,
}

impl Cover {
    pub fn new(range: f64, size: usize) -> Cover {
        Cover::with_capacity(range, size, size)
    }

    /// A cache of `size` with room for one of up to `largest`, so a switch
    /// (`reset`) allocates nothing and the page's views stay valid.
    pub fn with_capacity(range: f64, size: usize, largest: usize) -> Cover {
        let mut cover = Cover {
            size,
            cell: 0.0,
            heights: vec![OPEN; largest * largest],
            keys_x: vec![NONE; largest * largest],
            keys_z: vec![NONE; largest * largest],
            band: None,
            queries: 0,
        };
        cover.reset(range, size);
        cover
    }

    /// An empty cache of another range and size (a new RainCover).
    pub fn reset(&mut self, range: f64, size: usize) {
        self.size = size;
        self.cell = (range * 2.0 + 8.0) / size as f64;
        self.heights.fill(OPEN);
        self.keys_x.fill(NONE);
        self.keys_z.fill(NONE);
        self.band = None;
        self.queries = 0;
    }

    /// The cells in use.
    pub fn cells(&self) -> usize {
        self.size * self.size
    }

    /// Asks for the cells now around `at` that the cache does not hold, at
    /// most `budget` of them (RainCover.update). `height(x, y, z)` is the
    /// page's answer for a cell: the first thing hit looking down 512 m from
    /// the point, or the water's level over it if higher. Returns whether
    /// every cell is held, and whether any changed.
    pub fn update(&mut self, at: [f64; 3], budget: usize, height: &mut dyn FnMut(f64, f64, f64) -> f64) -> (bool, bool) {
        let band = (at[1] / 32.0).floor();
        if self.band != Some(band) {
            self.keys_x.fill(NONE);
            self.band = Some(band);
        }
        let size = self.size as i64;
        let x0 = (at[0] / self.cell).floor() as i64 - size / 2;
        let z0 = (at[2] / self.cell).floor() as i64 - size / 2;
        let (mut changed, mut pending) = (false, 0);
        self.queries = 0;
        for z in z0..z0 + size {
            for x in x0..x0 + size {
                let index = (z.rem_euclid(size) * size + x.rem_euclid(size)) as usize;
                if self.keys_x[index] == x as i32 && self.keys_z[index] == z as i32 {
                    continue;
                }
                if self.queries >= budget {
                    pending += 1;
                    continue;
                }
                self.heights[index] = height((x as f64 + 0.5) * self.cell, band * 32.0 + 160.0, (z as f64 + 0.5) * self.cell) as f32;
                self.keys_x[index] = x as i32;
                self.keys_z[index] = z as i32;
                self.queries += 1;
                changed = true;
            }
        }
        (pending == 0, changed)
    }
}
