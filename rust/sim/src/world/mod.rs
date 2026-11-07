//! World-step state and the remaining phase boundaries.
pub mod owner;
pub mod clock;
pub mod events;
pub mod local;
#[cfg(target_arch = "wasm32")]
pub mod step;
pub mod network;
pub mod network_lifecycle;
pub mod network_controls;
#[cfg(target_arch = "wasm32")]
mod module;

/// Reserved whole-tick input and completed output. These buffers contain no
/// borrowed host identities and stay valid across ordinary phase entries.
pub struct Tick {
    pub running: bool,
    pub water: Option<Water>,
    pub input: [f64; crate::gen::WorldTickInput::WORDS],
    pub output: crate::store::block::Block,
}
impl Tick {
    pub const fn cold() -> Self { Self { running: false, water: None, input: [0.0; crate::gen::WorldTickInput::WORDS], output: crate::store::block::Block::new() } }
    pub(crate) fn warm(&mut self) { self.output.room(1, crate::gen::WorldTickOutput::WORDS, 0); }
}
impl Default for Tick {
    fn default() -> Self {
        let mut output=crate::store::block::Block::default();
        output.room(1,crate::gen::WorldTickOutput::WORDS,0);
        Self {running:false,water:None,input:[0.0;crate::gen::WorldTickInput::WORDS],output}
    }
}

/// Mirror banks commit only at the outer world return. Canonical stores and
/// source-point event samples remain available to every intervening phase.
pub(crate) fn deferred() -> bool {crate::abi::world().tick.running}

/// WaterState.step's independent gameplay clock. Presentation can sample the
/// completed level, but render dt never advances authoritative water.
pub struct Water {pub time:f64,pub height:f64,pub bobbing:bool,pub wave:f64}
impl Water {
    pub fn step(&mut self,dt:f64)->f64 {
        self.time+=dt;
        self.wave=if self.bobbing {0.06*v8math::sin(2.0*std::f64::consts::PI*0.3*self.time)}else{0.0};
        self.height+self.wave
    }
}
