//! Fixed authority time and visual interpolation remain separate, as in
//! game.js and reconcile.js::advanceInputClock. A pause never replays a backlog.
use bfe_native::Frame;
use crate::generated_input::{ClientCommand, ClientLook, ClientFlightCommand};
use crate::input::{Input, ViewActions};

pub const STEP: f64 = 1.0 / 60.0;
const MAX_STEPS: usize = 8;

/// A battle owner receives commands at the authoritative fixed step, then
/// produces presentation rows once per displayed frame. Implementations own
/// their storage; the host borrows it only until the renderer finishes copying.
/// No callback reaches a device from inside a simulation entity loop.
pub trait World {
    fn step(&mut self, command: &ClientCommand, look: &ClientLook, flight: &ClientFlightCommand, view: &ViewActions, dt: f64) -> Result<(), String>;
    fn present(&mut self, alpha: f64) -> &Frame;
}

// Captured rows are immutable load-time data. They exercise native device
// integration while the live World and content owners are being joined.
// This owner deliberately has no gameplay state or speculative camera math.
pub struct CaptureWorld {
    frames: Vec<Frame>,
    at: usize,
}
impl CaptureWorld {
    pub fn new(frames: Vec<Frame>) -> Result<Self, String> {
        if frames.is_empty() { return Err("capture has no frames".into()); }
        Ok(Self { frames, at: 0 })
    }
    pub fn frame(&self, index: usize) -> Option<&Frame> { self.frames.get(index) }
}
impl World for CaptureWorld {
    // Captures have no authoritative gameplay. Advancing the source at fixed
    // ticks preserves replay speed on displays whose refresh differs from 60 Hz.
    fn step(&mut self, _command: &ClientCommand, _look: &ClientLook, _flight: &ClientFlightCommand, _view: &ViewActions, _dt: f64) -> Result<(), String> {
        self.at = (self.at + 1) % self.frames.len();
        Ok(())
    }
    fn present(&mut self, _alpha: f64) -> &Frame { &self.frames[self.at] }
}

// The authority accumulator is local to this battle and is never the audio
// device clock. Device output proceeds independently at its negotiated rate.
// A resumed frame starts from a cleared accumulator, with no paused input.
pub struct Host<W: World> {
    pub world: W,
    pub input: Input,
    pub tick: u32,
    accumulator: f64,
}
impl<W: World> Host<W> {
    pub fn new(world: W) -> Self {
        Self { world, input: Input::default(), tick: 0, accumulator: 0.0 }
    }
    pub fn pause(&mut self) { self.accumulator = 0.0; self.input.clear(); }
    // Limit debt once at frame admission. A long load cannot run an unbounded
    // catch-up loop, duplicate press actions or starve the window event queue.
    // Fractional authority time is passed explicitly to presentation.
    pub fn frame(&mut self, elapsed: f64) -> Result<&Frame, String> {
        let elapsed = if elapsed.is_finite() { elapsed.clamp(0.0, STEP * MAX_STEPS as f64) } else { 0.0 };
        self.accumulator += elapsed;
        let mut steps = 0;
        while self.accumulator >= STEP && steps < MAX_STEPS {
            self.tick = self.tick.wrapping_add(1);
            // Each completed step consumes a fresh command sequence, including
            // catch-up steps whose held state remains unchanged.
            let (command, look) = self.input.take(self.tick, 1);
            let flight = self.input.take_flight();
            let view = self.input.take_view();
            self.world.step(&command, &look, &flight, &view, STEP)?;
            self.accumulator -= STEP;
            steps += 1;
        }
        Ok(self.world.present(self.accumulator / STEP))
    }
}
