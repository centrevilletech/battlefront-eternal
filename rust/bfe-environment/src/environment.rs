//! Which of a map's placed effects (torches, campfires, steam vents, pipes,
//! fountains) are in view (src/environment-effects.js update): each has a
//! sphere around where it plays, seen if it is close enough (a stock effect
//! out to the near scene's end, an Enhanced one to 90 m) and the view
//! frustum touches the sphere. The page plays and stops the stock effects of
//! those in view and hands the Enhanced ones' volumes and fountains to
//! their draws.

use crate::common::{Camera, Frustum};
use crate::layout::{environment_instance as ei, environment_result as er};
use three::Vector3;

pub struct Visibility {
    count: usize,
    spheres: Vec<[f64; 4]>,
    pub positions: Vec<[f64; 3]>,
    classic: Vec<bool>,
    jet: Vec<bool>,
}

impl Visibility {
    /// `config` is an EnvironmentInstance record per instance.
    pub fn new(config: &[f64], count: usize) -> Visibility {
        let mut v = Visibility { count, spheres: Vec::with_capacity(count), positions: Vec::with_capacity(count), classic: Vec::with_capacity(count), jet: Vec::with_capacity(count) };
        for i in 0..count {
            let c = &config[i * ei::STRIDE..(i + 1) * ei::STRIDE];
            v.spheres.push([c[ei::SPHERE], c[ei::SPHERE + 1], c[ei::SPHERE + 2], c[ei::SPHERE + 3]]);
            v.positions.push([c[ei::POSITION], c[ei::POSITION + 1], c[ei::POSITION + 2]]);
            v.classic.push(c[ei::CLASSIC] != 0.0);
            v.jet.push(c[ei::JET] != 0.0);
        }
        v
    }

    /// `result` takes the EnvironmentResult, then, `count` numbers each, a 1
    /// for every instance in view, the Enhanced volumes in view and the
    /// fountains in view (instance indices, the lists as long as the
    /// result's counts say).
    pub fn update(&self, camera: &Camera, reach: f64, enhanced: bool, result: &mut [f64]) {
        let mut frustum = Frustum::default();
        camera.frustum(&mut frustum);
        let eye = camera.position;
        let n = self.count;
        let (mut sources, mut volumes, mut fountains) = (0, 0, 0);
        for i in 0..n {
            let p = self.positions[i];
            let (dx, dy, dz) = (p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]);
            let d2 = dx * dx + dy * dy + dz * dz;
            let limit = if self.classic[i] { reach * reach } else { 90.0 * 90.0 };
            let s = self.spheres[i];
            let visible = d2 < limit && frustum.intersects_sphere(&Vector3::new(s[0], s[1], s[2]), s[3]);
            result[er::STRIDE + i] = f64::from(visible);
            if visible {
                sources += 1;
            }
            if !enhanced || self.classic[i] || !visible {
                continue;
            }
            // Fountains are drawn as water; only steam is a volume.
            if !self.jet[i] {
                result[er::STRIDE + n + volumes] = i as f64;
                volumes += 1;
            } else {
                result[er::STRIDE + 2 * n + fountains] = i as f64;
                fountains += 1;
            }
        }
        result[er::SOURCES] = sources as f64;
        result[er::VOLUMES] = volumes as f64;
        result[er::FOUNTAINS] = fountains as f64;
    }
}
