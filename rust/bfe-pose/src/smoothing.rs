//! CorrectionSmoother keeps visual offsets separate from collided positions.
//! Both page presentation and simulation diagnostics share these exact rules.
// Retain Math.hypot and Math.exp semantics across WASM and native replay.
// Summing squared components or using a different exponential changes exact
// reference offsets after repeated small corrections, even when pixels agree.
use jsrt::{js_hypot, js_max};

#[derive(Clone, Copy, Debug, Default)]
pub struct SmootherState {
    pub offset: [f64; 3],
    pub count: f64,
    pub worst: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct SmootherNote {
    pub size: f64,
    pub changed: bool,
    pub reset: bool,
}

impl SmootherState {
    pub fn reset(&mut self) {
        self.offset = [0.0; 3];
    }

    /// Returns before any reset callback. Publish offset/count/worst first,
    /// then call the current reset method and increment the current snaps.
    pub fn note(&mut self, old: [f64; 3], new: [f64; 3], snap: f64) -> SmootherNote {
        let delta = [old[0] - new[0], old[1] - new[1], old[2] - new[2]];
        let size = js_hypot(&delta);
        // Tiny correction noise does not count as a visible correction.
        // Keep that early return before changing offsets or statistics.
        if size < 1e-6 {
            return SmootherNote { size: 0.0, changed: false, reset: false };
        }
        self.count += 1.0;
        if size > self.worst {
            self.worst = size;
        }
        self.offset[0] += delta[0];
        self.offset[1] += delta[1];
        self.offset[2] += delta[2];
        SmootherNote { size, changed: true, reset: self.length() > snap }
    }

    /// Returns whether the host must call its current reset method after the
    /// new offsets are visible. Reset statistics are never changed here.
    pub fn decay(&mut self, tau: f64, dt: f64) -> bool {
        let k = v8math::exp(-js_max(0.0, dt) / tau);
        self.offset[0] *= k;
        self.offset[1] *= k;
        self.offset[2] *= k;
        self.length() < 1e-4
    }

    pub fn length(&self) -> f64 {
        js_hypot(&self.offset)
    }
}
