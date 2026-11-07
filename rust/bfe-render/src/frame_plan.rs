//! Graphics.render, the game render tail and TransparentPass.afterPhase share
//! this retained schedule. The host publishes semantic views, never operations.
//! Selection is separate from execution so views may be prepared in any order.
#[path = "frame_layout.rs"]
pub mod layout;
use layout::{facts as F, state as S, role as R, operation as O, limit as L, command as C, kind as K};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Operation { pub kind: usize, pub role: usize, pub argument: usize }

pub struct FramePlan {
    // Facts describe semantic views rather than requested operations. The
    // retained state survives skipped main draws and held auxiliary views.
    pub facts: [f64; F::LEN],
    pub state: [f64; S::LEN],
    pub trace: [f64; L::OPERATIONS * C::LEN],
    pub operations: [Operation; L::OPERATIONS],
    pub count: usize,
}
impl Default for FramePlan {
    fn default() -> Self {
        let mut plan = Self { facts: [0.; F::LEN], state: [0.; S::LEN], trace: [0.; L::OPERATIONS * C::LEN], operations: [Operation::default(); L::OPERATIONS], count: 0 };
        for i in 0..L::PROBES { plan.state[S::PROBE_LAST+i] = f64::NEG_INFINITY; plan.state[S::PROBE_EYE+3*i+1] = f64::INFINITY; }
        plan.state[S::CLOUD_LAST] = f64::NEG_INFINITY;
        for i in 0..L::MIRRORS { plan.state[S::MIRROR_LEVEL+i] = f64::NAN; }
        plan
    }
}
fn count(value: f64, max: usize) -> usize { (value as usize).min(max) }
fn distance(a: &[f64], b: &[f64]) -> f64 {
    let x = a[0]-b[0]; let y = a[1]-b[1]; let z = a[2]-b[2]; x*x+y*y+z*z
}
impl FramePlan {
    // Water.reflect's hold policy and SkyProbe.update's cadence. View/clip
    // numerics are facts until the scene owner consumes native camera rows.
    pub fn select(&mut self) {
        let f = &self.facts; let s = &mut self.state;
        s[S::CLOUD_CAPTURE] = (f[F::CLOUDS] != 0. && !(f[F::CLOUD_GENERATION]==s[S::CLOUD_GENERATION] && f[F::CLOUD_TIME]-s[S::CLOUD_LAST] < 0.1 && distance(&f[F::CLOUD_EYE..F::CLOUD_EYE+3],&s[S::CLOUD_EYE..S::CLOUD_EYE+3]) < 256.)) as u8 as f64;
        for i in 0..L::PROBES {
            let eye = F::PROBE_EYE+3*i; let previous = S::PROBE_EYE+3*i;
            s[S::PROBE_CAPTURE+i] = (f[F::PROBE_ENABLED+i] != 0. && !(f[F::PROBE_GENERATION+i]==s[S::PROBE_GENERATION+i] && f[F::PROBE_TIME+i]-s[S::PROBE_LAST+i] < 1. && (f[eye+1]-s[previous+1]).abs() < 4.)) as u8 as f64;
        }
        for i in 0..L::MIRRORS {
            s[S::MIRROR_ELIGIBLE+i] = (f[F::MIRROR_ENABLED+i] != 0. && (f[F::MIRROR_VISIBLE+i] != 0. || f[F::MIRROR_FAR_VISIBLE+i] != 0.) && f[F::MIRROR_LOW+i] == 0. && f[F::MIRROR_EYE+3*i+1]-f[F::MIRROR_LEVEL+i] >= 0.03) as u8 as f64;
            let eye = F::MIRROR_EYE+3*i; let previous = S::MIRROR_EYE+3*i;
            let direction = F::MIRROR_FORWARD+3*i; let last = S::MIRROR_FORWARD+3*i;
            let dot = f[direction]*s[last]+f[direction+1]*s[last+1]+f[direction+2]*s[last+2];
            let held = f[F::MIRROR_NEAR+i] == 0. && count(s[S::MIRROR_FRAMES+i],u32::MAX as usize)%2 == 1
                && s[S::MIRROR_LEVEL+i] == f[F::MIRROR_LEVEL+i]
                && s[S::MIRROR_SIZE+2*i] == f[F::MIRROR_SIZE+2*i] && s[S::MIRROR_SIZE+2*i+1] == f[F::MIRROR_SIZE+2*i+1]
                && distance(&s[previous..previous+3],&f[eye..eye+3]) < 0.09 && dot > 0.9994;
            s[S::MIRROR_HELD+i] = held as u8 as f64;
        }
        // Water.drawUnderwater and its enhanced fog are mutually exclusive.
        // Keep the ocean exclusion attached to the fog decision itself.
        let below = f[F::WATER_EYE_Y] < f[F::WATER_LEVEL] && f[F::WATER_OVER] != 0.;
        s[S::UNDERWATER] = (below && f[F::WATER_SCALE] == 0.) as u8 as f64;
        s[S::FOG] = (below && f[F::WATER_SCALE] != 0. && f[F::WATER_OCEAN] == 0.) as u8 as f64;
        // planCascades: a moved/invalid window refreshes both maps. Only the
        // distant dynamic cascade alternates, and never before its first draw.
        let cascades = count(f[F::SHADOW_COUNT], L::CASCADES);
        for i in 0..L::CASCADES {
            let dirty = f[F::SHADOW_DIRTY+i] != 0.;
            let last = cascades > 2 && i+1 == cascades;
            s[S::SHADOW_STATIC+i] = (i<cascades && dirty) as u8 as f64;
            s[S::SHADOW_DYNAMIC+i] = (i<cascades && (dirty || f[F::SHADOW_MOVED+i] != 0. || !last || count(s[S::FRAME],u32::MAX as usize)%2 == 0)) as u8 as f64;
        }
    }
    // The operation store is bounded independently of imported view counts.
    // Clip deterministically and retain old history instead of panicking on a
    // slow frame or a caller publishing more auxiliary views than content has.
    fn emit(&mut self, kind: usize, role: usize, argument: usize) {
        if self.count == L::OPERATIONS { self.state[S::CLIPPED] = 1.; return; }
        self.operations[self.count] = Operation {kind,role,argument};
        let at = self.count*C::LEN;
        self.trace[at+C::KIND] = kind as f64; self.trace[at+C::ROLE] = role as f64; self.trace[at+C::ARGUMENT] = argument as f64;
        self.count += 1;
    }
    fn draw(&mut self, role: usize) { self.emit(O::DRAW,role,0); }
    fn bound_draw(&mut self, role: usize) { self.emit(O::BIND,role,0); self.draw(role); }
    // NearPass.cover and its transparent tail. The far image remains after
    // opaque depth; every sorted copy/line is consumed by the existing list.
    fn scene(&mut self, opaque: usize, transparent: usize, image: usize, lines: usize, main: bool) {
        if !main {self.emit(O::BIND,opaque,0);}
        self.emit(O::CLEAR,opaque,0); self.draw(opaque);
        self.draw(lines); self.draw(image);
        if main {
            if self.facts[F::DEPTH_EARLY] != 0. { self.emit(O::COPY_DEPTH,R::NEAR,R::DEPTH); }
            for i in 0..count(self.facts[F::VOLUMES],L::VOLUMES) { self.draw(R::VOLUME0+i); self.emit(O::BIND,R::NEAR,0); }
        }
        if main && self.facts[F::FOUNTAIN_COPY]!=0. {self.emit(O::COPY_COLOR_DEPTH,R::NEAR,R::BASIN);}
        if main && self.facts[F::DEPTH_LATE]!=0. {self.emit(O::COPY_DEPTH,R::NEAR,R::DEPTH);}
        // WaterPass.draw formerly took its scene-color snapshot while
        // preparing the first refraction reader, before any transparent draw.
        if main && self.facts[F::SURFACE_COPY]!=0. {self.emit(O::COPY_COLOR_DEPTH,R::NEAR,R::SURFACE);}
        // The transparent owner retains sorted snapshots between readers,
        // including lines and the first particle depth request.
        self.draw(transparent);
        if main && self.state[S::FOG] != 0. {
            self.emit(O::COPY_COLOR_DEPTH,R::NEAR,R::SURFACE); self.draw(R::FOG);
        }
        self.emit(O::RESOLVE,opaque,0);
    }
    // Graphics.render and game.frame formerly invoked these responsibilities
    // in nested hooks. The native owner states their dependencies directly.
    // Preparation order cannot move a reflection behind refreshed shadows or
    // let a surface copy see an incomplete opaque/transparent image.
    pub fn plan(&mut self) {
        self.count = 0; self.state[S::CLIPPED] = 0.;
        if self.state[S::CLOUD_CAPTURE] != 0. { self.bound_draw(R::CLOUDS); }
        for probe in 0..L::PROBES { if self.state[S::PROBE_CAPTURE+probe] != 0. {
            for face in 0..6 { self.bound_draw(R::PROBE0+6*probe+face); }
            self.emit(O::MIPMAP,R::PROBE0+6*probe+5,0);
        }}
        // Water/sky presentation can refresh textures on callbacks whose main
        // frame is skipped. Such a callback never advances temporal history.
        if self.facts[F::MAIN] == 0. { self.state[S::OPERATION_COUNT] = self.count as f64; return; }
        // Mirrors consume the preceding frame's shadow maps, before the main
        // cascade refresh. Each sky uses its plain projection before clipping.
        for i in 0..L::MIRRORS {
            if self.state[S::MIRROR_ELIGIBLE+i] == 0. || self.state[S::MIRROR_HELD+i] != 0. || self.facts[F::MIRROR_CLIP_VALID+i] == 0. { continue; }
            let first = R::MIRROR0_SKY+3*i;
            self.emit(O::BIND,first+1,0);
            if self.facts[F::MIRROR_SKY+i] != 0. { self.draw(first); }
            self.draw(first+1); self.emit(O::RESOLVE,first+1,0);
            if self.facts[F::MIRROR_NEAR+i] != 0. {
                self.scene(first+2,R::MIRROR0_TRANSPARENT+i,R::MIRROR0_IMAGE+i,R::LINES0+i,false);
            }
        }
        self.bound_draw(R::FAR); self.emit(O::RESOLVE,R::FAR,0);
        self.emit(O::BIND,R::NEAR,0);
        for i in 0..count(self.facts[F::SHADOW_COUNT],L::CASCADES) {
            if self.state[S::SHADOW_STATIC+i] != 0. { self.draw(R::SHADOW_STATIC0+i); self.emit(O::BIND,R::NEAR,0); }
            if self.state[S::SHADOW_DYNAMIC+i] != 0. { self.draw(R::SHADOW_DYNAMIC0+i); self.emit(O::BIND,R::NEAR,0); }
        }
        self.scene(R::NEAR,R::TRANSPARENT,R::IMAGE,R::LINES2,true);
        // Preserve an already scheduled early/sorted snapshot. Only a frame
        // without either reader takes the final pre-weapon scene depth here.
        if self.facts[F::TEMPORAL] != 0. && self.facts[F::DEPTH_EARLY] == 0. && self.facts[F::DEPTH_SORTED] == 0. && self.facts[F::DEPTH_LATE] == 0. { self.emit(O::COPY_DEPTH,R::NEAR,R::DEPTH); }
        if self.facts[F::VIEW_SHOWN] != 0. { self.emit(O::CLEAR_DEPTH,R::NEAR,0); self.draw(R::VIEW); self.emit(O::RESOLVE,R::NEAR,0); }
        if self.facts[F::TEMPORAL] != 0. { self.draw(R::MOTION); }
        // Post writes the inactive history image. Only its explicit completion
        // flips the read bank; an interrupted frame retains the preceding one.
        self.emit(O::POST,R::POST,0); self.emit(O::HISTORY_COMMIT,R::POST,0);
        if self.state[S::UNDERWATER] != 0. { self.bound_draw(R::UNDERWATER); }
        // BattleHud.render publishes semantic layers in any preparation order.
        // Their slots fix before/native/custom/banner/after/overlay execution.
        for i in 0..L::HUD_BATCHES {
            let kind=self.facts[F::HUD_KINDS+i] as usize;
            if kind!=K::HUD && kind!=K::INTERFACE {continue;}
            let role=if i<8 {R::HUD0+i}else{R::HUD8+i-8};
            if kind==K::INTERFACE {self.emit(O::BIND,role,0);}
            if self.facts[F::HUD_CLEARS+i]!=0. {self.emit(O::CLEAR,role,self.facts[F::HUD_CLEARS+i] as usize);}
            self.draw(role);
        }
        self.state[S::OPERATION_COUNT] = self.count as f64;
    }
    // Successful views publish cache cadence once. Failed/clipped frames keep
    // old cache state so recovery cannot mistake a partial image for history.
    pub fn complete(&mut self, success: bool) {
        if !success || self.state[S::CLIPPED] != 0. { return; }
        let f = &self.facts; let s = &mut self.state;
        if s[S::CLOUD_CAPTURE] != 0. { s[S::CLOUD_GENERATION]=f[F::CLOUD_GENERATION];s[S::CLOUD_LAST] = f[F::CLOUD_TIME]; s[S::CLOUD_EYE..S::CLOUD_EYE+3].copy_from_slice(&f[F::CLOUD_EYE..F::CLOUD_EYE+3]); }
        for i in 0..L::PROBES { if s[S::PROBE_CAPTURE+i] != 0. {
            s[S::PROBE_GENERATION+i]=f[F::PROBE_GENERATION+i];s[S::PROBE_LAST+i] = f[F::PROBE_TIME+i];
            s[S::PROBE_EYE+3*i..S::PROBE_EYE+3*i+3].copy_from_slice(&f[F::PROBE_EYE+3*i..F::PROBE_EYE+3*i+3]);
        }}
        for i in 0..L::MIRRORS {
            if s[S::MIRROR_ELIGIBLE+i] == 0. { continue; }
            s[S::MIRROR_FRAMES+i] = (count(s[S::MIRROR_FRAMES+i],u32::MAX as usize) as u32).wrapping_add(1) as f64;
            if s[S::MIRROR_HELD+i] != 0. { continue; }
            if f[F::MIRROR_CLIP_VALID+i] == 0. { continue; }
            s[S::MIRROR_LEVEL+i] = f[F::MIRROR_LEVEL+i];
            s[S::MIRROR_SIZE+2*i..S::MIRROR_SIZE+2*i+2].copy_from_slice(&f[F::MIRROR_SIZE+2*i..F::MIRROR_SIZE+2*i+2]);
            s[S::MIRROR_EYE+3*i..S::MIRROR_EYE+3*i+3].copy_from_slice(&f[F::MIRROR_EYE+3*i..F::MIRROR_EYE+3*i+3]);
            s[S::MIRROR_FORWARD+3*i..S::MIRROR_FORWARD+3*i+3].copy_from_slice(&f[F::MIRROR_FORWARD+3*i..F::MIRROR_FORWARD+3*i+3]);
        }
        if f[F::MAIN] == 0. { return; }
        s[S::FRAME] = (count(s[S::FRAME],u32::MAX as usize) as u32).wrapping_add(1) as f64;
    }
}
