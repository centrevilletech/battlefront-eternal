//! WorldTick's host facts, separate from Host's display accumulator. The
//! enclosing simulation advances its compensated clock; this adapter only
//! stamps the next row and accepts the fields WorldTick.complete publishes.
use bfe_client::host::STEP;
use bfe_sim::gen::{WorldTickInput, WorldTickOutput};

pub struct Clock {
    template: WorldTickInput,
}

impl Clock {
    pub fn new(template: WorldTickInput) -> Self { Self { template } }
    pub fn tick(&self) -> f64 { self.template.clock.tick }

    // The next tick's row. A player's squad orders are not here: they
    // travel in his command, and the world's players take them from it.
    pub fn input(&self) -> WorldTickInput {
        WorldTickInput { dt: STEP, tick: self.tick() + 1., ..self.template.clone() }
    }

    // WorldTick.complete preserves configuration from admission. Copy only
    // the eight mutable clock facts; do not independently add dt or replay a
    // rejected interval. All presentation occurs after this completion slot.
    pub fn complete(&mut self, output: &WorldTickOutput) -> Result<(), &'static str> {
        if !output.accepted || output.clock.tick != self.tick() + 1. { return Err("local world rejected its next tick"); }
        let source = &output.clock;
        let target = &mut self.template.clock;
        target.tick = source.tick; target.elapsed = source.elapsed;
        target.compensation = source.compensation; target.debt = source.debt;
        target.alpha = source.alpha; target.overloads = source.overloads;
        target.dropped = source.dropped; target.step_cost = source.step_cost;
        Ok(())
    }
}
