//! A streak joins successive particle positions from one emitter. Its width,
//! opacity and texture come from the two samples and the admitted material.
use bfe_presentation::layout::draw as d;

#[derive(Clone, Copy)]
pub(super) struct Point {
    owner: usize,
    definition: usize,
    position: [f64; 3],
    width: f64,
}

impl Point {
    pub fn read(row: &[f64]) -> Self {
        Self {
            owner: row[d::EFFECT] as usize,
            definition: row[d::DEFINITION] as usize,
            position: row[d::POSITION..d::POSITION + 3].try_into().unwrap(),
            width: row[d::SIZE],
        }
    }

    pub fn connect(self, tail: Self, row: &[f64]) -> Option<[f32; 21]> {
        if self.owner != tail.owner || self.definition != tail.definition || self.width <= 0. {
            return None;
        }
        let delta: [f64; 3] = std::array::from_fn(|k| self.position[k] - tail.position[k]);
        let length = delta.iter().map(|n| n * n).sum::<f64>().sqrt();
        if length <= f64::EPSILON {
            return None;
        }
        let mut quad = [0.; 21];
        for i in 0..3 {
            quad[i] = self.position[i] as f32;
            quad[11 + i] = (delta[i] / length) as f32;
        }
        for i in 0..4 {
            quad[3 + i] = row[d::COLOR + i] as f32;
            quad[17 + i] = row[d::FRAME + i] as f32;
        }
        quad[7] = self.width as f32;
        quad[9] = length as f32;
        quad[10] = 1.;
        quad[14] = (tail.width / self.width) as f32;
        Some(quad)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bends_join_samples_from_the_same_emitter_with_zero_particle_velocity() {
        let mut row = [0.; d::STRIDE];
        row[d::SIZE] = 0.4;
        let start = Point::read(&row);
        assert!(start.connect(start, &row).is_none());
        row[d::POSITION] = 3.;
        let bend = Point::read(&row);
        let segment = bend.connect(start, &row).unwrap();
        assert_eq!(&segment[9..14], &[3., 1., 1., 0., 0.]);
        row[d::POSITION + 1] = 5.;
        row[d::SIZE] = 0.8;
        let end = Point::read(&row).connect(bend, &row).unwrap();
        assert_eq!(&end[9..15], &[5., 1., 0., 1., 0., 0.5]);
        row[d::EFFECT] = 8.;
        assert!(Point::read(&row).connect(bend, &row).is_none());
    }
}
