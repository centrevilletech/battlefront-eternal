//! Retained post policy and temporal history, ported from Graphics.postSteps,
//! Bloom.steps and Upscaler. The browser publishes facts, never ordered steps.
//! Arrays are reserved with the owner; neither planning nor commit allocates.
#[path = "post_chain_layout.rs"]
pub mod layout;
use crate::{math, post, core::Stats};
use layout::{facts as F, state as S, resource as R, program as P, mode as M, aa as A, uniform as U};
use three::{Matrix4, Vector3};

const IDENTITY: [f64; 16] = [1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.];
// Graphics.passCamera: OrthographicCamera(-1,1,1,-1,0,1), including signed zero.
const PROJECTION: [f64; 16] = [1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,-2.,0.,-0.,-0.,-1.,1.];

pub struct PostChain {
    pub input: [f64; F::LEN],
    // This is also the published state, so capture/replay needs no JS mirror.
    pub state: [f64; S::LEN],
}
impl Default for PostChain {
    fn default() -> Self {
        let mut this = Self { input: [0.; F::LEN], state: [0.; S::LEN] };
        this.input[F::RESET_FRAME] = -1.; this.input[F::RESET_READ] = -1.;
        this.state[S::RESET_SERIAL] = -1.;
        this.state[S::CURRENT..S::CURRENT+16].copy_from_slice(&IDENTITY);
        this.state[S::PREVIOUS..S::PREVIOUS+16].copy_from_slice(&IDENTITY);
        this.state[S::INVERSE_JITTERED..S::INVERSE_JITTERED+16].copy_from_slice(&IDENTITY);
        this
    }
}
// Every imported dimension/index is bounded before use. Invalid host facts
// cannot turn a long pause or resource transition into a frame-path panic.
fn dimension(x: f64) -> f64 { if x.is_finite() { x.clamp(1., 32768.) } else { 1. } }
fn index(x: f64, max: usize) -> usize { (x as usize).min(max) }
fn matrix(data: &[f64], at: usize) -> [f64; 16] {
    let mut out = [0.; 16]; out.copy_from_slice(&data[at..at+16]); out
}
// Upscaler.halton/jitterAt: the same division and accumulation order in f64.
fn halton(mut i: usize, base: usize) -> f64 {
    let (mut fraction, mut value) = (1., 0.);
    while i > 0 { fraction /= base as f64; value += fraction * (i % base) as f64; i /= base; }
    value
}
impl PostChain {
    pub fn invalidate(&mut self) { self.state[S::VALID] = 0.; self.state[S::PENDING] = 0.; }

    // Upscaler.resize/begin: output resizing and a cut discard history; input
    // scaling alone keeps it. An undrawn frame never flips the history bank.
    pub fn begin(&mut self) {
        let f = &self.input; let s = &mut self.state;
        let (w, h) = (dimension(f[F::INPUT_WIDTH]), dimension(f[F::INPUT_HEIGHT]));
        let (ow, oh) = (dimension(f[F::OUTPUT_WIDTH]), dimension(f[F::OUTPUT_HEIGHT]));
        if f[F::RESET_SERIAL] != s[S::RESET_SERIAL] {
            s[S::VALID] = 0.; s[S::RESET_SERIAL] = f[F::RESET_SERIAL];
            if f[F::RESET_FRAME] >= 0. { s[S::FRAME] = index(f[F::RESET_FRAME], u32::MAX as usize) as f64; }
            if f[F::RESET_READ] >= 0. { s[S::READ] = index(f[F::RESET_READ], 1) as f64; }
        }
        s[S::READ] = index(s[S::READ], 1) as f64;
        s[S::FRAME] = index(s[S::FRAME], u32::MAX as usize) as f64;
        if s[S::OUTPUT_WIDTH] != ow || s[S::OUTPUT_HEIGHT] != oh { s[S::VALID] = 0.; }
        s[S::OUTPUT_WIDTH] = ow; s[S::OUTPUT_HEIGHT] = oh; s[S::PENDING] = 0.;
        let mode = index(f[F::MODE], M::TEMPORAL);
        s[S::TEMPORAL] = (mode == M::TEMPORAL) as u8 as f64;
        // Temporal AA resolves at native size too. Spatial/bilinear need a
        // separate upscale only when the input and output extents differ.
        s[S::UPSCALE] = (mode == M::TEMPORAL || mode != M::OFF && (w != ow || h != oh)) as u8 as f64;
        if mode != M::TEMPORAL { s[S::VALID] = 0.; return; }
        let world = Matrix4 { elements: matrix(f, F::WORLD) };
        let eye = Vector3::new(world.elements[12], world.elements[13], world.elements[14]);
        let mut look = Vector3::new(0., 0., -1.); look.transform_direction(&world);
        let last_eye = Vector3::new(s[S::LAST_EYE], s[S::LAST_EYE+1], s[S::LAST_EYE+2]);
        let last_look = Vector3::new(s[S::LAST_LOOK], s[S::LAST_LOOK+1], s[S::LAST_LOOK+2]);
        if s[S::VALID] != 0. && (eye.distance_to_squared(&last_eye) > 25. || look.dot(&last_look) < 0.77) { s[S::VALID] = 0.; }
        s[S::LAST_EYE..S::LAST_EYE+3].copy_from_slice(&[eye.x,eye.y,eye.z]);
        s[S::LAST_LOOK..S::LAST_LOOK+3].copy_from_slice(&[look.x,look.y,look.z]);
        let projection = matrix(f, F::PROJECTION); let view = matrix(f, F::VIEW);
        let current = math::mul(&projection, &view);
        if s[S::VALID] == 0. { s[S::PREVIOUS..S::PREVIOUS+16].copy_from_slice(&current); }
        s[S::CURRENT..S::CURRENT+16].copy_from_slice(&current);
        // Upscaler.resize scales the Halton period by pixel area. Its f64
        // rounding order matters at the boundaries between phase counts.
        let ratio = ow / w;
        let phases = (8. * ratio * ratio).round().clamp(8., 32.) as usize;
        let i = index(s[S::FRAME], u32::MAX as usize) % phases + 1;
        let (x, y) = (halton(i, 2) - 0.5, halton(i, 3) - 0.5);
        s[S::PHASES] = phases as f64; s[S::JITTER] = x; s[S::JITTER+1] = y;
        s[S::SHIFT] = 2. * x / w; s[S::SHIFT+1] = 2. * y / h;
        // Upscaler.drawMotion used the jittered main projection, before the
        // host restores its camera shells. Compute that inverse only here.
        let mut shifted = projection; shifted[8] -= s[S::SHIFT]; shifted[9] -= s[S::SHIFT+1];
        let mut inverse = Matrix4 { elements: math::mul(&shifted, &view) }; inverse.invert();
        s[S::INVERSE_JITTERED..S::INVERSE_JITTERED+16].copy_from_slice(&inverse.elements);
    }

    // Graphics.postSteps, Bloom.steps and Upscaler.steps. Programs and resource
    // identities were bound at loading/resize; no strings or shells are read.
    pub fn plan(&mut self, out: &mut [f64]) -> usize {
        out.fill(0.);
        let f = &self.input; let s = &self.state;
        let mut p = Planner { f, out, count: 0, at: 0, program: 0, complete: true };
        let temporal = s[S::TEMPORAL] != 0.; let upscale = s[S::UPSCALE] != 0.;
        let blur = f[F::SOFT_FOCUS] != 0. && f[F::BLUR_ENABLED] != 0.;
        let bloom = f[F::BLOOM] != 0.; let mode = index(f[F::MODE], M::TEMPORAL);
        let aa = index(f[F::AA], A::FAST); let fxaa = !temporal && aa != A::OFF;
        let speed = if f[F::SPEED_ENABLED] != 0. { f[F::SPEED_AMOUNT].clamp(0., 1.) * 0.085 } else { 0. };
        let turn = f[F::SPEED_TURN].clamp(-2., 2.);
        let (w,h) = (dimension(f[F::INPUT_WIDTH]),dimension(f[F::INPUT_HEIGHT]));
        // Soft focus and bloom share the scene but use independently sized
        // resources: map blur rounds its extent; Bloom.resize rounds up.
        if blur { p.step(P::COPY, Some(R::QUARTER)); p.sampler(U::copy::MAP,R::SCENE); }
        if bloom {
            p.step(P::EXTRACT,Some(R::BLOOM)); p.sampler(U::extract::MAP,R::SCENE); p.value(U::extract::TEXEL,&[1./w,1./h]);
            p.step(P::BLUR,Some(R::SCRATCH)); p.sampler(U::blur::MAP,R::BLOOM); p.value(U::blur::DIRECTION,&[1./p.width(R::BLOOM),0.]);
            p.step(P::BLUR,Some(R::BLOOM)); p.sampler(U::blur::MAP,R::SCRATCH); p.value(U::blur::DIRECTION,&[0.,1./p.height(R::BLOOM)]);
        }
        // Graphics.postSteps elides a composite only when all three effects
        // are visually inactive. Keep its small vehicle-blur threshold.
        let plain = !blur && !bloom && speed.partial_cmp(&0.0001) != Some(std::cmp::Ordering::Greater);
        let direct = upscale && mode == M::BILINEAR && !fxaa && plain;
        let fxaa_source = if fxaa && plain { R::SCENE } else { R::POST };
        if !(fxaa && plain) {
            p.step(P::COMPOSITE,if (fxaa || upscale) && !direct {Some(R::POST)} else {None});
            p.sampler(U::composite::MAP,R::SCENE); p.sampler(U::composite::BLURRED,R::QUARTER); p.sampler(U::composite::GLOW,R::BLOOM);
            p.value(U::composite::BLEND,&[if blur { f[F::BLUR_BLEND] } else { 0. }]);
            p.value(U::composite::BLOOM_STRENGTH,&[if bloom {0.65} else {0.}]);
            p.value(U::composite::SPEED_BLUR,&[speed]); p.value(U::composite::SPEED_TURN,&[turn]);
        }
        // FXAA reads the original scene for a plain frame. Otherwise its
        // input includes the composite, and an upscale needs its own target.
        if fxaa {
            p.step(if aa == A::FAST {P::FXAA_FAST} else {P::FXAA}, if upscale {Some(R::AA)} else {None});
            p.value(U::fxaa::PROJECTION_MATRIX,&PROJECTION); p.value(U::fxaa::MODEL_VIEW_MATRIX,&IDENTITY);
            p.sampler(U::fxaa::T_DIFFUSE,fxaa_source); p.value(U::fxaa::RESOLUTION,&[1./w,1./h]);
        }
        if upscale && !direct {
            let image = if fxaa {R::AA} else {R::POST};
            if temporal {
                // The read bank stays immutable until the final sharpen has
                // succeeded. Resolve writes the other bank, never in place.
                let read = R::HISTORY0 + index(s[S::READ],1); let write = R::HISTORY0 + 1 - index(s[S::READ],1);
                p.step(P::MOTION,Some(R::MOTION)); p.sampler(U::motion::DEPTH_MAP,R::DEPTH); p.sampler(U::motion::VELOCITY_MAP,R::VELOCITY);
                p.value(U::motion::INVERSE_JITTERED,&s[S::INVERSE_JITTERED..S::INVERSE_JITTERED+16]);
                p.value(U::motion::CURRENT_VIEW_PROJECTION,&s[S::CURRENT..S::CURRENT+16]); p.value(U::motion::PREVIOUS_VIEW_PROJECTION,&s[S::PREVIOUS..S::PREVIOUS+16]);
                p.step(P::RESOLVE,Some(write)); p.sampler(U::resolve::COLOR_MAP,image); p.sampler(U::resolve::MOTION_MAP,R::MOTION); p.sampler(U::resolve::HISTORY_MAP,read);
                p.value(U::resolve::INPUT_SIZE,&[w,h]); p.value(U::resolve::OUTPUT_SIZE,&[dimension(f[F::OUTPUT_WIDTH]),dimension(f[F::OUTPUT_HEIGHT])]);
                p.value(U::resolve::JITTER,&s[S::JITTER..S::JITTER+2]); p.value(U::resolve::RESET,&[if s[S::VALID] != 0. {0.} else {1.}]);
                // Upscaler.resize limits confidence by source-pixel area;
                // sparse input retains fewer old samples after disocclusion.
                let ratio = (w/dimension(f[F::OUTPUT_WIDTH])).min(1.);
                p.value(U::resolve::MAX_WEIGHT,&[(12. * ratio * ratio).clamp(4., 12.)]); p.value(U::resolve::KERNEL,&[2.29]);
                p.step(P::SHARPEN,None); p.sampler(U::sharpen::MAP,write); p.value(U::sharpen::SHARPNESS,&[f[F::SHARPNESS]]);
            } else {
                p.step(if mode == M::BILINEAR {P::BILINEAR} else {P::SPATIAL},None); p.sampler(U::spatial::MAP,image);
                if mode != M::BILINEAR { p.value(U::spatial::INPUT_SIZE,&[w,h]); p.value(U::spatial::OUTPUT_SIZE,&[dimension(f[F::OUTPUT_WIDTH]),dimension(f[F::OUTPUT_HEIGHT])]); }
            }
        }
        let count = p.count; let complete = p.complete;
        self.state[S::STEPS] = count as f64;
        self.state[S::PENDING] = (complete && temporal) as u8 as f64;
        count
    }

    // Upscaler.commit runs only after the entire native chain succeeds. A loss
    // or rejected draw leaves the read bank and previous view unchanged.
    // Every planned draw must have run. Refused program handles and context
    // loss cancel the pending bank even before a host error/event arrives.
    pub fn complete(&mut self, drawn: usize, lost: bool) {
        if lost || drawn == 0 || drawn > post::MAX_STEPS || drawn as f64 != self.state[S::STEPS] {
            self.invalidate(); return;
        }
        self.commit();
    }
    fn commit(&mut self) {
        if self.state[S::PENDING] == 0. { return; }
        self.state[S::READ] = (1-index(self.state[S::READ],1)) as f64;
        let current = matrix(&self.state,S::CURRENT);
        self.state[S::PREVIOUS..S::PREVIOUS+16].copy_from_slice(&current);
        self.state[S::VALID] = 1.;
        self.state[S::FRAME] = (index(self.state[S::FRAME],u32::MAX as usize) as u32).wrapping_add(1) as f64;
        self.state[S::PENDING] = 0.;
    }
    // Publish into fixed storage instead of allocating Stats::values each frame.
    pub fn stats(&mut self, stats: Stats) {
        let out = &mut self.state[S::STATS..];
        out[..5].copy_from_slice(&[stats.world_draws as f64,stats.instance_draws as f64,stats.instances as f64,stats.program_switches as f64,stats.gl_calls as f64]);
        for (i,n) in stats.pass_draws.iter().enumerate() { out[5+i] = *n as f64; }
    }
}

struct Planner<'a> { f: &'a [f64], out: &'a mut [f64], count: usize, at: usize, program: usize, complete: bool }
impl Planner<'_> {
    fn resource(&self,r: usize,field: usize) -> f64 { self.f[F::RESOURCES+r*R::STRIDE+field] }
    fn width(&self,r: usize) -> f64 { dimension(self.resource(r,R::WIDTH)) }
    fn height(&self,r: usize) -> f64 { dimension(self.resource(r,R::HEIGHT)) }
    fn step(&mut self, program: usize, target: Option<usize>) {
        // A short host view clips whole steps, never a uniform or index. The
        // normal owner reserves the maximum eight steps before play begins.
        if self.count >= post::MAX_STEPS || (self.count+1)*post::STEP > self.out.len() { self.complete = false; return; }
        self.at = self.count * post::STEP; self.count += 1;
        let o = self.at;
        self.out[o+post::STEP_PROGRAM] = self.f[F::PROGRAMS+program];
        self.out[o+post::STEP_TARGET] = target.map_or(post::TARGET_CANVAS,|r|self.resource(r,R::TARGET));
        self.out[o+post::STEP_WIDTH] = target.map_or(dimension(self.f[F::CANVAS_WIDTH]),|r|self.width(r));
        self.out[o+post::STEP_HEIGHT] = target.map_or(dimension(self.f[F::CANVAS_HEIGHT]),|r|self.height(r));
        self.out[o+post::STEP_UNIFORMS] = layout::TYPES[program].len() as f64;
        // Keep the small program kind outside the step record's GPU id. Its
        // declared uniform types come from the same generated shader layout.
        self.program = program;
    }
    fn value(&mut self,slot: usize,values: &[f64]) {
        if !self.complete { return; }
        let at = self.at+post::STEP_FIRST+slot*post::UNIFORM;
        if at+post::UNIFORM > self.out.len() || slot >= post::MAX_UNIFORMS { self.complete = false; return; }
        // Values stay f64 through the boundary; post.rs performs the same
        // final f32 conversion as its prepacked shader-input callers.
        self.out[at] = slot as f64; self.out[at+1] = layout::TYPES[self.program][slot] as f64;
        self.out[at+2..at+2+values.len()].copy_from_slice(values);
    }
    fn sampler(&mut self,slot: usize,r: usize) { self.value(slot,&[self.resource(r,R::TEXTURE)]); }
}
