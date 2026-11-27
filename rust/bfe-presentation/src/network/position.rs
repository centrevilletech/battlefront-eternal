//! Position codes shared by snapshot capture and display.

use jsrt::{js_max, js_min, js_round};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PositionWidth {
    Narrow,
    Wide,
}

impl PositionWidth {
    pub const fn max_code(self) -> f64 {
        match self {
            Self::Narrow => 65_535.,
            Self::Wide => 16_777_215.,
        }
    }
}

/// A battle's immutable axis-aligned box, in XYZ order and world metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PositionBounds {
    min: [f64; 3],
    extent: [f64; 3],
    width: PositionWidth,
}

impl Default for PositionBounds {
    fn default() -> Self {
        Self::LEGACY
    }
}

impl PositionBounds {
    pub const LEGACY: Self = Self {
        min: [-1024., -400., -1024.], extent: [2048., 700., 2048.], width: PositionWidth::Narrow,
    };

    /// The 64 m margin is provisional. Size it from measured excursions past
    /// the flight limits, knockback, and the travel of replicated debris.
    pub fn from_map_extents(min: [f64; 3], max: [f64; 3]) -> Option<Self> {
        for (lo, hi) in min.into_iter().zip(max) {
            if !lo.is_finite() || !hi.is_finite() || lo >= hi {
                return None;
            }
        }
        const MARGIN_METRES: f64 = 64.;
        let min = min.map(|lo| lo.floor() - MARGIN_METRES);
        let max = max.map(|hi| hi.ceil() + MARGIN_METRES);
        let extent = std::array::from_fn(|axis| max[axis] - min[axis]);
        Self::from_padded(min, extent)
    }

    /// Reconstruct the admitted codebook when replaying a captured host call.
    pub fn from_padded(min: [f64; 3], extent: [f64; 3]) -> Option<Self> {
        for (lo, span) in min.into_iter().zip(extent) {
            if lo.fract() != 0. || span.fract() != 0. || !(1. ..=65_536.).contains(&span)
                || lo.abs() > 1_048_576. || (lo + span).abs() > 1_048_576.
            {
                return None;
            }
        }
        let wide = extent.iter().zip(Self::LEGACY.extent).any(|(&span, legacy)| span > legacy);
        let width = if wide { PositionWidth::Wide } else { PositionWidth::Narrow };
        Some(Self { min, extent, width })
    }

    pub fn padded_axes(&self) -> ([f64; 3], [f64; 3]) {
        (self.min, self.extent)
    }

    /// Old capture and inspection rows repeat the already padded box.
    pub fn check_box(&self, min: [f64; 3], extent: [f64; 3]) -> super::Result<()> {
        if self.min != min || self.extent != extent {
            return Err("snapshot bounds do not match admitted map");
        }
        Ok(())
    }

    pub fn contains(&self, position: [f64; 3]) -> bool {
        (0..3).all(|axis| {
            position[axis] >= self.min[axis] && position[axis] <= self.min[axis] + self.extent[axis]
        })
    }

    pub const fn width(&self) -> PositionWidth {
        self.width
    }

    pub fn encode(&self, position: [f64; 3]) -> [f64; 3] {
        let max = self.width.max_code();
        std::array::from_fn(|axis| {
            let code = js_round((position[axis] - self.min[axis]) / self.extent[axis] * max);
            js_max(0., js_min(max, code))
        })
    }

    pub fn decode(&self, codes: [f64; 3]) -> [f64; 3] {
        let max = self.width.max_code();
        std::array::from_fn(|axis| self.min[axis] + codes[axis] / max * self.extent[axis])
    }

    pub fn displacement(&self, codes: [f64; 3], previous: [f64; 3]) -> [f64; 3] {
        let max = self.width.max_code();
        std::array::from_fn(|axis| (codes[axis] - previous[axis]) * self.extent[axis] / max)
    }

    pub fn step(&self) -> [f64; 3] {
        self.extent.map(|span| span / self.width.max_code())
    }
}
