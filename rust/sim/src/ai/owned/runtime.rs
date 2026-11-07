//! The battle's AI entry points. Core decisions, mounted aim, hull decay and
//! Enhanced objectives run at their source points within one native phase.
//! World implements the services over the battle's canonical owners; it must
//! not export individual controller calls or defer their effects to phase end.
use crate::ai::{
    control::{
        world::{AiWorld, UKey},
        Core,
    },
    phase::RandomState,
};

pub trait World: AiWorld {
    /// Native post-movement aim changes its retained filter, intent and weapon
    /// tuning only. Captured worlds still prepare and return all service writes.
    fn prepare_aimer(&mut self,core:&Core,controller:usize)->bool {core.feed_combat(self,controller);true}
    fn difficulty_changed(&mut self,_difficulty:&crate::ai::control::difficulty::Difficulty) {}
    /// The post-movement aim hook also serves mounted controls before F2
    /// latches their intents. The controller's current fields have been fed.
    fn aim_controller(&mut self, controller: usize, dt: f64);
    /// A decay hit can synchronously kill crew and change live controllers.
    /// The active Core and shared generators are explicit through that call.
    fn decay(&mut self, core: &mut Core, random: &mut RandomState, dt: f64);
}

pub struct Runtime {
    pub core: Core,
    pub random: RandomState,
}
impl Runtime {
    pub fn new(core: Core, random: RandomState) -> Self {
        Self { core, random }
    }
    pub fn attach(&mut self, world: &mut impl World, character: usize) -> Option<usize> {
        self.core.attach(world, &mut self.random, character)
    }
    pub fn spawned(&mut self, world: &mut impl World, character: usize) -> Option<usize> {
        self.core.spawned(world, &mut self.random, character)
    }
    /// F4 commits population/ticket changes before delivering this callback.
    pub fn died(&mut self, world: &mut impl World, character: usize) {
        self.core.died(world, &mut self.random, character);
    }
    pub fn phase(&mut self, world: &mut impl World, dt: f64) {
        self.core.update(world, &mut self.random, dt);
        if self.core.options.bind.combat {
            // The source walks the live array, not a snapshot. A synchronous
            // callback can remove an entry and change which one comes next.
            let mut at = 0;
            while at < self.core.live.len() {
                let controller = self.core.live[at];
                if let Some(unit) = self.core.controllers[controller].unit {
                    let view = world.unit_scalars(unit);
                    if view.has_lease && view.lease_is_self {
                        self.aim_controller(world, controller, dt);
                    }
                }
                at += 1;
            }
        }
        if self.core.options.bind.vehicles {
            world.decay(&mut self.core, &mut self.random, dt);
        }
        self.core.enhanced_objectives_tick(world, &mut self.random);
    }
    fn aim_controller(&mut self, world: &mut impl World, controller: usize, dt: f64) {
        super::lifecycle::Callback {core:&mut self.core,random:&mut self.random}.aim_controller(world,controller,dt);
    }
    /// Called by F3 at the individual unit's post-movement aim point, before
    /// its weapon update consumes A. No list of delayed aim writes is built.
    pub fn aim(&mut self, world: &mut impl World, unit: UKey, dt: f64) {
        super::lifecycle::Callback {core:&mut self.core,random:&mut self.random}.aim(world,unit,dt);
    }
    pub fn owner_changed(&mut self, world: &mut impl World, post: usize, old: f64, new: f64) {
        self.core
            .objectives_on_owner_changed(world, &mut self.random, post, old, new);
    }
    pub fn deactivated(&mut self, world: &mut impl World, post: usize) {
        self.core
            .objectives_on_deactivated(world, &mut self.random, post);
    }
}

impl super::lifecycle::Callback<'_> {
    pub(crate) fn aim_controller(&mut self,world:&mut impl World,controller:usize,dt:f64) {
        let writes=world.prepare_aimer(self.core,controller);
        world.aim_controller(controller,dt);
        if writes {self.core.sync_combat_writes(world,controller);}
    }
    /// F3's post-movement slot uses the same explicit Core loan as a nested
    /// lifecycle action. The current combat writes return before weapon fire.
    pub fn aim(&mut self,world:&mut impl World,unit:UKey,dt:f64) {
        if !self.core.options.bind.combat {return;}
        if let Some(controller)=self.core.controller_of_unit(unit) {
            self.aim_controller(world,controller,dt);
        }
    }
}
