//! Entry points fixed at admission for the complete native callback scope.
use super::{events::Invalid, owner::Battle, world::Facts};
use crate::{
    battle::live::Queries,
    gen::{AiJournalInput, AiPhaseCamera, AiPhaseFacts, AiPlayerCommand},
    recon::Authority,
    units::f3::WorldServices,
    world::owner::World,
};

pub(super) type Callback<'a> = dyn FnMut(&mut World, WorldServices<'_>) + 'a;
type Scope = fn(&mut Battle, &mut World, &mut Authority, &mut Queries);
type Phase = fn(
    &mut Battle,
    &mut World,
    &mut Authority,
    &mut Queries,
    &Facts,
    &AiPhaseFacts,
    &[AiPhaseCamera],
    &AiJournalInput,
) -> Result<(), Invalid>;

pub(super) struct Entries {
    callbacks: fn(&mut Battle, &mut World, &mut Authority, &mut Queries, &mut Callback<'_>),
    attach: Scope,
    phase: Phase,
    goal_phase: Phase,
    dangers: Scope,
    command: fn(&mut Battle, &mut World, &mut Authority, &mut Queries, &AiPlayerCommand) -> bool,
}

impl Entries {
    const fn for_game<const SWBF2: bool>() -> Self {
        Self {
            callbacks: Battle::scoped_callbacks::<SWBF2>,
            attach: Battle::scoped_attach::<SWBF2>,
            phase: Battle::scoped_phase::<SWBF2, false>,
            goal_phase: Battle::scoped_phase::<SWBF2, true>,
            dangers: Battle::scoped_dangers::<SWBF2>,
            command: Battle::scoped_command::<SWBF2>,
        }
    }
    pub(super) const CLASSIC: Self = Self::for_game::<false>();
    pub(super) const SWBF2: Self = Self::for_game::<true>();
}

impl Battle {
    pub fn with_native_callbacks<R>(
        &mut self,
        world: &mut World,
        recon: &mut Authority,
        queries: &mut Queries,
        run: impl FnOnce(&mut World, WorldServices<'_>) -> R,
    ) -> R {
        let mut run = Some(run);
        let mut result = None;
        (self.scopes_for_map.callbacks)(self, world, recon, queries, &mut |world, services| {
            result = Some(run.take().expect("one native callback scope")(
                world, services,
            ));
        });
        result.expect("native callback ran")
    }

    pub fn attach_initial(&mut self, w: &mut World, r: &mut Authority, q: &mut Queries) {
        (self.scopes_for_map.attach)(self, w, r, q);
    }
    #[allow(clippy::too_many_arguments)]
    pub fn native_phase<const GOALS: bool>(
        &mut self,
        w: &mut World,
        r: &mut Authority,
        q: &mut Queries,
        facts: &Facts,
        input: &AiPhaseFacts,
        cameras: &[AiPhaseCamera],
        journal: &AiJournalInput,
    ) -> Result<(), Invalid> {
        let phase = if GOALS { self.scopes_for_map.goal_phase } else { self.scopes_for_map.phase };
        phase(self, w, r, q, facts, input, cameras, journal)
    }
    pub fn native_dangers(&mut self, w: &mut World, r: &mut Authority, q: &mut Queries) {
        (self.scopes_for_map.dangers)(self, w, r, q);
    }
    pub fn native_command(
        &mut self,
        w: &mut World,
        r: &mut Authority,
        q: &mut Queries,
        input: &AiPlayerCommand,
    ) -> bool {
        (self.scopes_for_map.command)(self, w, r, q, input)
    }
}
