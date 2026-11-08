//! WorldTick's host facts, separate from Host's display accumulator. The
//! enclosing simulation advances its compensated clock; this adapter only
//! stamps the next row and accepts the fields WorldTick.complete publishes.
use bfe_client::{host::STEP, input::{SquadOrder, ViewActions}};
use bfe_sim::gen::{WorldTickInput, WorldTickOutput};

pub struct Clock {
    template: WorldTickInput,
}

impl Clock {
    pub fn new(template: WorldTickInput) -> Self { Self { template } }
    pub fn tick(&self) -> f64 { self.template.clock.tick }

    // Game.step's squad order is a protocol order, not the order of desktop
    // function keys. The simulation owns whether that command is accepted;
    // the window supplies one coalesced semantic intention at this boundary.
    pub fn input(&self, view: &ViewActions) -> WorldTickInput {
        let squads = view.squad.map_or(0, |order| 1 << match order {
            SquadOrder::MoveOut => 0, SquadOrder::Hold => 1,
            SquadOrder::Follow => 2, SquadOrder::AtEase => 3,
        });
        WorldTickInput { dt: STEP, tick: self.tick() + 1., squads, ..self.template.clone() }
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
