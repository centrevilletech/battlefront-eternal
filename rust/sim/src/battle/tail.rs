//! The authority's battle-tail slot (updateBattle). Battlefield
//! deaths precede post capture and rules; local spawn realization and corpse
//! removal follow the native character-spawn pass. HUD work stays outside this span.
use super::{battlefield::{Battlefield, BattlefieldWorld}, human::Command,
    phase::{self, PhaseWorld, Scratch}, rules::RuleSet, spawning::ContextDraws, stats::ContextText};
use crate::gen::{Character, F4TailInput, Store};
use crate::store::Ref;

/// These are native lifecycle services. Human choices have already passed the
/// input validator; spawning and removal must finish before the phase returns.
pub trait World: PhaseWorld + BattlefieldWorld {
    fn human_tail(&mut self, s: &mut Store, dt: f64, commands: &[Command]);
    fn corpse_tail(&mut self, s: &mut Store);
}
pub struct Tail {
    pub battlefield: Battlefield,
    scratch: Scratch,
    pub(super) rules: RuleSet,
}
impl Tail {
    pub fn reserve_spawn_points(&mut self,points:usize){self.scratch.reserve_spawn_points(points);}
    pub fn new(battlefield: Battlefield, members: usize, posts: usize, mobile: usize, victims: usize) -> Self {
        Self { battlefield, scratch: Scratch::new(members, posts, mobile, victims), rules: RuleSet::Conquest }
    }
    /// A single native phase entry preserves the interposed ordnance/item
    /// span by remaining separate from LiveObjects' earlier phase entry.
    #[allow(clippy::too_many_arguments)]
    pub fn step<W: World + ?Sized>(&mut self, input: &F4TailInput, s: &mut Store, strings: &dyn ContextText<W>,
        spawning: bool, active: impl Fn(Ref<Character>, &Character) -> bool,
        commands: &[Command], world: &mut W, random: &mut dyn ContextDraws<W>, gameplay: &mut dyn ContextDraws<W>) {
        // A 2005 battle (its event-driven rules) gives the 2005 time to come back.
        match self.rules {
            RuleSet::EventDriven(_) => self.battlefield.update_swbf2(input.dt, s, world),
            RuleSet::Conquest => self.battlefield.update(input.dt, s, world),
        }
        phase::update_battle_with_scratch(input.dt, s, strings, spawning, active, world,
            random, gameplay, &mut self.scratch, &mut self.rules);
        if !input.server { world.human_tail(s, input.dt, commands); }
        world.corpse_tail(s);
    }
}
