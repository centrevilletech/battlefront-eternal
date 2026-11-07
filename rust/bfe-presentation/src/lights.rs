//! LightPool's frame owns flash lifetimes and stable strongest-request
//! selection. The page holds only scene lights and borrowed command rows.
use crate::page_layout::{light_command as c, light_frame as f, light_handle as h,
    light_header as header, light_kind as kind, light_limits as lim, light_output as o};

#[derive(Clone, Copy, Default)]
struct Light {
    position: [f64; 3],
    rgb: [f64; 3],
    radius: f64,
    scale: f64,
    weight: f64,
    serial: f64,
    duration: f64,
    left: f64,
}
pub struct Lights {
    pub input: [f64; f::STRIDE],
    pub commands: Vec<f64>,
    pub output: Vec<f64>,
    pub handles: Vec<f64>,
    pub header: [f64; header::STRIDE],
    timed: Vec<Light>,
    top: Vec<Light>,
    count: usize,
    oldest: usize,
}
impl Lights {
    // Only construction allocates. Both live storage and the output bank
    // remain reserved after flashes expire or a frame requests no lights.
    pub fn new(count: usize) -> Self {
        Self {
            input: [0.0; f::STRIDE],
            commands: vec![0.0; lim::COMMANDS * c::STRIDE],
            output: vec![0.0; lim::LIGHTS * o::STRIDE],
            handles: vec![0.0; lim::TIMED * h::STRIDE],
            header: [0.0; header::STRIDE],
            timed: Vec::with_capacity(lim::TIMED),
            top: Vec::with_capacity(lim::LIGHTS),
            count: count.min(lim::LIGHTS),
            oldest: 0,
        }
    }
    // Ports LightPool.add's weighting and update's insertion selection.
    // Strict greater-than preserves emitter order when weights tie.
    fn select(&mut self, light: Light) {
        let mut at = self.top.len();
        while at > 0 && light.weight > self.top[at - 1].weight { at -= 1; }
        if at >= self.count { return; }
        if self.top.len() == self.count { self.top.pop(); }
        self.top.insert(at, light);
    }
    // The old arithmetic order matters at ties: multiply the summed colour
    // by scale first, then by radius, without narrowing to shader floats.
    fn weight(light: &mut Light) {
        light.weight = (light.rgb[0] + light.rgb[1] + light.rgb[2]) * light.scale * light.radius;
    }
    pub fn frame(&mut self) {
        // Counts and ring offsets originate in a borrowed host row. Saturate
        // hostile, non-finite and oversized values before indexing storage.
        // Clearing top retains its loading-time allocation for every frame.
        self.top.clear();
        // The host ring is already bounded, but the native boundary also
        // defends against malformed counts before it computes row spans.
        let count = (self.input[f::COMMANDS] as usize).min(lim::COMMANDS);
        let start = (self.input[f::START] as usize).min(lim::COMMANDS - 1);
        for i in 0..count {
            let at = ((start + i) % lim::COMMANDS) * c::STRIDE;
            let row = &self.commands[at..at + c::STRIDE];
            // Copy numeric facts onto the stack before mutating the owner;
            // no scene objects or temporary request vectors cross the phase.
            let mut light = Light {
                position: [row[c::POSITION], row[c::POSITION + 1], row[c::POSITION + 2]],
                rgb: [row[c::RGB], row[c::RGB + 1], row[c::RGB + 2]],
                radius: row[c::RADIUS], scale: row[c::SCALE], serial: row[c::SERIAL],
                duration: row[c::DURATION], left: row[c::DURATION], ..Light::default()
            };
            if row[c::KIND] == kind::FLASH as f64 {
                self.header[header::COMPLETED] = self.header[header::COMPLETED].max(light.serial);
                if light.radius <= 0.0 || light.duration <= 0.0 { continue; }
                // The reservation exceeds bursts from every loaded actor.
                // A paused queue can exceed it: replace the oldest flash,
                // preserving chronological order and never growing a store.
                if self.timed.len() == lim::TIMED {
                    let old = self.timed[self.oldest];
                    let at = (old.serial as usize % lim::TIMED) * h::STRIDE;
                    if self.handles[at + h::SERIAL] == old.serial {
                        self.handles[at + h::SERIAL] = 0.0;
                    }
                    self.timed[self.oldest] = light;
                    self.oldest = (self.oldest + 1) % lim::TIMED;
                    self.header[header::REFUSED_TIMED] += 1.0;
                } else { self.timed.push(light); }
            } else if row[c::KIND] == kind::ADD as f64 && light.radius > 0.0 {
                Self::weight(&mut light);
                self.select(light);
            }
        }
        // Ports LightPool.update's compaction: fading requests follow every
        // one-frame request, including flashes queued in this same frame.
        // A paused burst replaces a ring slot in constant time per command.
        // Restore chronological order once, then compact in place as usual.
        self.timed.rotate_left(self.oldest);
        self.oldest = 0;
        let mut kept = 0;
        for i in 0..self.timed.len() {
            let mut light = self.timed[i];
            light.left -= self.input[f::DT];
            // Publish by serial even while the live list compacts. An old
            // caller's token cannot accidentally address a different flash.
            let at = (light.serial as usize % lim::TIMED) * h::STRIDE;
            self.handles[at + h::SERIAL] = light.serial;
            self.handles[at + h::DURATION] = light.duration;
            self.handles[at + h::LEFT] = light.left;
            if light.left > 0.0 {
                self.timed[kept] = light;
                kept += 1;
                light.scale = light.left / light.duration;
                Self::weight(&mut light);
                self.select(light);
            }
        }
        // truncate drops only plain numeric records. Capacity stays reserved
        // for bursts after loading, even when no flash survives this frame.
        self.timed.truncate(kept);
        self.header[header::TIMED] = kept as f64;
        self.header[header::COUNT] = self.top.len() as f64;
        // Unused output slots keep their position/colour as the old scene
        // lights did; only intensity is cleared when a request disappears.
        // Output order is the insertion order of the selected requests.
        // A fixed bank avoids allocating a result for each scene light.
        for i in 0..self.count {
            let out = &mut self.output[i * o::STRIDE..(i + 1) * o::STRIDE];
            if let Some(light) = self.top.get(i) {
                out[o::POSITION..o::POSITION + 3].copy_from_slice(&light.position);
                out[o::RGB..o::RGB + 3].copy_from_slice(&light.rgb);
                out[o::RADIUS] = light.radius;
                out[o::SCALE] = light.scale;
            } else { out[o::SCALE] = 0.0; }
        }
    }
}
