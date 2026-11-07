//! Native consumers of existing world cores. These adapt host boundaries;
//! they do not install any of the remaining subsystem singleton owners.
use super::{Context, Error, Failure, Record};
use crate::gen::*;
use crate::players::module as players;
use crate::world::{events, local};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};

#[derive(Debug)]
pub struct WorldFailure<S> {
    pub state: S,
    pub error: Error,
}

// Distinct from clock's marker. Neither is public or crosses an extern C
// function. A nested consumer catches its own failure before returning.
struct Stop;
struct Host<'a> {
    context: &'a Context,
    failure: Option<Error>,
}
impl Host<'_> {
    fn accept<T, S>(
        &mut self,
        reply: Result<T, Failure>,
        state: &mut S,
        take: fn(&[f64], &[f64]) -> T,
        state_of: impl Fn(&T) -> S,
    ) -> T {
        match reply {
            Ok(reply) => {
                *state = state_of(&reply);
                reply
            }
            Err(failure) => {
                if let Some(partial) = failure.partial {
                    *state = state_of(&partial.read(take));
                }
                self.failure = Some(failure.error);
                resume_unwind(Box::new(Stop));
            }
        }
    }
}

impl Context {
    /// createMultiplayerWorld's connected-player owner belongs to this World.
    pub fn players_init(&self) -> Result<(), Error> {
        self.world(players::players_init_in)
    }

    /// addPlayer admits the same mailbox and Character keys as the WASM host.
    pub fn players_create(&self, input: &PlayerSetup) -> Result<u32, Error> {
        self.world(|w| {
            w.sim.calls.result(PlayerSetup::WORDS, |r, h| input.put(r, h));
            players::players_create_in(w)
        })
    }

    /// A reconnect keeps the body while replacing its mailbox and lanes.
    pub fn players_link(&self, handle: u32, input: &PlayerLink) -> Result<u32, Error> {
        self.world(|w| {
            w.sim.calls.result(PlayerLink::WORDS, |r, h| input.put(r, h));
            players::players_link_in(w, handle)
        })
    }

    pub fn players_drop(&self, handle: u32) -> Result<(), Error> {
        self.world(|w| players::players_drop_in(w, handle))
    }

    /// stopInput stills whichever body or seat the player currently controls.
    pub fn players_stop(&self, handle: u32) -> Result<(), Error> {
        self.world(|w| players::players_stop_in(w, handle))
    }

    pub fn players_disconnect(&self, handle: u32, at: f64) -> Result<(), Error> {
        self.world(|w| players::players_disconnect_in(w, handle, at))
    }

    pub fn players_auto(&self, handle: u32, flag: u32) -> Result<(), Error> {
        self.world(|w| players::players_auto_in(w, handle, flag))
    }

    pub fn players_input(&self, handle: u32, at: f64, lead: f64, accepted: u32) -> Result<(), Error> {
        self.world(|w| players::players_input_in(w, handle, at, lead, accepted))
    }

    pub fn players_fire(&self, handle: u32, input: &PlayerFire) -> Result<u32, Error> {
        self.world(|w| {
            w.sim.calls.result(PlayerFire::WORDS, |r, h| input.put(r, h));
            players::players_fire_in(w, handle)
        })
    }

    /// Reliable admission has a fixed-size answer, decoded before the loan ends.
    pub fn players_actions(&self, handle: u32, input: &PlayerActions) -> Result<Option<PlayerAdmission>, Error> {
        self.world(|w| {
            w.sim.calls.result(PlayerActions::WORDS, |r, h| input.put(r, h));
            players::players_actions_in(w, handle)?;
            let (row, heap) = w.sim.calls.args();
            Some(PlayerAdmission::take(row, heap))
        })
    }

    /// Read the retained schema block during its loan; no address or owned copy
    /// enters a native descriptor. The reader must not reenter this Context.
    pub fn players_lanes<T>(&self, handle: u32, lane: u32, drain: u32, read: impl FnOnce(Option<&[f64]>) -> T) -> Result<T, Error> {
        self.world(|w| read(players::players_lanes_in(w, handle, lane, drain)))
    }

    pub fn players_outcomes_enable(&self, handle: u32) -> Result<(), Error> {
        self.world(|w| players::players_outcomes_enable_in(w, handle))
    }

    /// Changed-player publication borrows its reserved block for this call only.
    pub fn players_views<T>(&self, read: impl FnOnce(Option<&[f64]>) -> T) -> Result<T, Error> {
        self.world(|w| read(players::players_views_in(w)))
    }

    fn world_run<S: Clone, T>(
        &self,
        mut state: S,
        run: impl FnOnce(&mut S, &mut Host<'_>) -> T,
        words: usize,
        put: fn(&S, &mut [f64], &mut Vec<f64>),
        cleanup: impl FnOnce(&Context, &mut S, &Error) -> Result<(), Error>,
    ) -> Result<(S, T), WorldFailure<S>> {
        let _flight = self.enter().map_err(|error| WorldFailure {
            state: state.clone(),
            error,
        })?;
        let mut host = Host {
            context: self,
            failure: None,
        };
        let outcome = catch_unwind(AssertUnwindSafe(|| run(&mut state, &mut host)));
        let value = match outcome {
            Ok(value) => value,
            Err(payload) => {
                if !payload.is::<Stop>() || host.failure.is_none() {
                    resume_unwind(payload);
                }
                let error = host.failure.take().unwrap();
                // Explicit host cleanup failures cannot replace the original
                // callback error. Unexpected Rust panics still propagate.
                let _ = cleanup(self, &mut state, &error);
                self.publish(&Record::new(words, |r, h| put(&state, r, h)))
                    .unwrap();
                return Err(WorldFailure { state, error });
            }
        };
        self.publish(&Record::new(words, |r, h| put(&state, r, h)))
            .unwrap();
        Ok((state, value))
    }

    pub fn world_events(
        &self,
        input: WorldEventInput,
    ) -> Result<WorldEventState, WorldFailure<WorldEventState>> {
        self.world_run(
            input.state,
            |state, host| match input.op {
                WorldEventOperation::Emit => events::emit(state, host),
                WorldEventOperation::Read => events::read(state, host),
                WorldEventOperation::Reset => events::reset(state, host),
            },
            WorldEventState::WORDS,
            WorldEventState::put,
            |_, _, _| Ok(()),
        )
        .map(|(state, ())| state)
    }

    pub fn world_local(
        &self,
        input: WorldLocalInput,
    ) -> Result<WorldLocalState, WorldFailure<WorldLocalState>> {
        self.world_run(
            input.state,
            |state, host| match input.op {
                WorldLocalOperation::Tail => local::tail(state, input.dt, host),
            },
            WorldLocalState::WORDS,
            WorldLocalState::put,
            |_, _, _| Ok(()),
        )
        .map(|(state, ())| state)
    }
}

impl events::Host for Host<'_> {
    fn call(&mut self, state: &mut WorldEventState, op: WorldEventCall, index: f64) -> bool {
        let reply = import_world_events_in(
            self.context,
            &WorldEventArgs {
                op,
                state: state.clone(),
                index,
            },
        );
        self.accept(reply, state, WorldEventReply::take, |r| r.state.clone())
            .present
    }
}
impl local::Host for Host<'_> {
    fn call(&mut self, state: &mut WorldLocalState, op: WorldLocalCall, dt: f64) -> bool {
        let reply = import_world_local_in(
            self.context,
            &WorldLocalArgs {
                op,
                state: state.clone(),
                dt,
            },
        );
        self.accept(reply, state, WorldLocalReply::take, |r| r.state.clone())
            .value
    }
}

#[cfg(test)]
mod player_tests {
    use super::*;
    use crate::abi::context::ErrorKind;
    use crate::input::owner::Mailbox;
    use crate::store::block::HEADER;

    fn mailbox(context: &Context) -> u32 {
        context.world(|w| {
            let mailbox = Mailbox::new(1.0, 8.0, Vec::new(), &mut w.sim.strings).unwrap();
            w.input.mailboxes.insert(mailbox).unwrap() as u32
        }).unwrap()
    }

    fn record<T>(words: &[f64], take: fn(&[f64], &[f64]) -> T) -> T {
        let stride = words[BlockHeader::STRIDE] as usize;
        let start = words[BlockHeader::HEAP] as usize;
        let count = words[BlockHeader::HEAP_WORDS] as usize;
        take(&words[HEADER..HEADER + stride], &words[start..start + count])
    }

    #[test]
    fn native_players_share_context_owners_and_preserve_reconnect_identity() {
        let context = Context::new();
        let other = Context::new();
        context.players_init().unwrap();
        other.players_init().unwrap();
        let first = mailbox(&context);
        let setup = PlayerSetup { mailbox: first, at: 10.0, ..Default::default() };
        // Identical numeric keys cannot manufacture a mailbox in another World.
        assert_eq!(other.players_create(&setup).unwrap(), 0);
        let handle = context.players_create(&setup).unwrap();
        assert_ne!(handle, 0);
        assert_eq!(context.players_fire(handle, &PlayerFire::default()).unwrap(), 0);
        assert!(!context.players_actions(handle, &PlayerActions::default()).unwrap().unwrap().accepted);
        context.players_auto(handle, 1).unwrap();
        context.players_input(handle, 20.0, f64::MAX, 1).unwrap();
        context.players_disconnect(handle, 9.0).unwrap();
        context.world(|w| {
            let p = w.input.players.pool.get(handle as usize).unwrap();
            assert!(p.auto_respawn);
            assert_eq!(p.last_input_at, 20.0);
            assert_eq!(p.input_lead_ticks, Some(crate::players::owner::MAX_INPUT_LEAD_TICKS));
            assert_eq!(p.disconnected_at, Some(9.0));
        }).unwrap();
        let replacement = mailbox(&context);
        assert_eq!(context.players_link(handle, &PlayerLink { mailbox: replacement, at: 30.0, reliable_fire: true, separate_actions: true }).unwrap(), 1);
        context.players_stop(handle).unwrap();
        context.world(|w| {
            let p = w.input.players.pool.get(handle as usize).unwrap();
            assert_eq!(p.mailbox, replacement as usize);
            assert_eq!(p.last_input_at, 30.0);
            assert_eq!(p.disconnected_at, None);
            assert!(p.reliable_fire && p.separate_actions && p.stopped);
        }).unwrap();
        context.players_drop(handle).unwrap();
        assert_eq!(context.players_link(handle, &PlayerLink { mailbox: replacement, ..Default::default() }).unwrap(), 0);
        assert!(context.players_lanes(handle, 0, 0, |words| words.is_none()).unwrap());
        let alias = context.clone();
        context.dispose().unwrap();
        assert_eq!(alias.players_init().unwrap_err().kind(), ErrorKind::Disposed);
        other.dispose().unwrap();
    }

    #[test]
    fn native_player_readers_drain_the_canonical_retained_blocks() {
        let context = Context::new();
        context.players_init().unwrap();
        let handle = context.players_create(&PlayerSetup { mailbox: mailbox(&context), ..Default::default() }).unwrap();
        context.players_outcomes_enable(handle).unwrap();
        let status = context.intern(&['o' as u16, 'k' as u16]).unwrap();
        context.world(|w| {
            let p = w.input.players.pool.get_mut(handle as usize).unwrap();
            p.actions.received = 7.0;
            p.actions.outcomes.as_mut().unwrap().push(IaOutcome { id: 7.0, status });
            w.input.players.changed.push(PlayerView { handle, team: 2.0, ..Default::default() });
        }).unwrap();
        let read = |words: Option<&[f64]>| record(words.unwrap(), PlayerLanes::take);
        let pending = context.players_lanes(handle, 0, 0, read).unwrap();
        assert_eq!(pending.actions_received, 7.0);
        assert!(pending.outcomes.is_empty());
        let drained = context.players_lanes(handle, 0, 1, read).unwrap();
        assert_eq!(drained.outcomes, vec![IaOutcome { id: 7.0, status }]);
        assert!(context.players_lanes(handle, 0, 1, read).unwrap().outcomes.is_empty());
        let changed = context.players_views(|words| record(words.unwrap(), PlayerViews::take)).unwrap();
        assert_eq!(changed.players.len(), 1);
        assert_eq!((changed.players[0].handle, changed.players[0].team), (handle, 2.0));
        assert!(context.players_views(|words| record(words.unwrap(), PlayerViews::take).players.is_empty()).unwrap());
        // The reader loans keep the publication banks and receipt capacity in
        // their owner, including when a caller chooses not to decode a copy.
        let capacities = || context.world(|w| {
            let p = &w.input.players;
            (p.views.words.capacity(), p.views.heap.capacity(), p.lanes.words.capacity(), p.lanes.heap.capacity(),
                p.pool.get(handle as usize).unwrap().actions.outcomes.as_ref().unwrap().capacity())
        }).unwrap();
        let before = capacities();
        for _ in 0..1024 {
            context.players_views(|words| assert!(words.is_some())).unwrap();
            context.players_lanes(handle, 0, 1, |words| assert!(words.is_some())).unwrap();
        }
        assert_eq!(capacities(), before);
    }
}
