//! Native consumers of existing world cores. These adapt host boundaries;
//! they do not install any of the remaining subsystem singleton owners.
use super::{Context, Error, Failure, Record};
use crate::gen::*;
use crate::world::{events, local, network, step};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};

#[derive(Debug)]
pub struct WorldFailure<S> {
    pub state: S,
    pub error: Error,
}

#[derive(Debug)]
pub struct NetworkResult {
    pub state: WorldNetworkState,
    pub tick: f64,
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
                WorldLocalOperation::Player => local::player(state, input.dt, host),
                WorldLocalOperation::Tail => local::tail(state, input.dt, host),
            },
            WorldLocalState::WORDS,
            WorldLocalState::put,
            |_, _, _| Ok(()),
        )
        .map(|(state, ())| state)
    }

    /// Returns and publishes the final host facts; the WASM step export is
    /// void because its host already has these facts in its world objects.
    pub fn world_step(
        &self,
        input: WorldStepInput,
    ) -> Result<WorldStepState, WorldFailure<WorldStepState>> {
        self.world_step_with_cleanup(input, |_, _, _| Ok(()))
    }

    /// The host owns grenade iterators and their body-entry flags. On an
    /// explicit callback failure it can close a suspended iterator here,
    /// before the original error/partial state is returned. The context is
    /// still in flight, so reentry is allowed and disposal is rejected.
    /// Cleanup may refresh state and return an Error, which is ignored in
    /// favor of the original. Rust panics are not host errors and propagate.
    pub fn world_step_with_cleanup(
        &self,
        input: WorldStepInput,
        cleanup: impl FnOnce(&Context, &mut WorldStepState, &Error) -> Result<(), Error>,
    ) -> Result<WorldStepState, WorldFailure<WorldStepState>> {
        self.world_run(
            input.state,
            |state, host| step::step(state, input.tick, host),
            WorldStepState::WORDS,
            WorldStepState::put,
            cleanup,
        )
        .map(|(state, ())| state)
    }

    pub fn world_network(
        &self,
        input: WorldNetworkInput,
    ) -> Result<NetworkResult, WorldFailure<WorldNetworkState>> {
        self.world_network_with_cleanup(input, |_, _, _| Ok(()))
    }

    /// Like step cleanup, but the host closes choice/player iterators from
    /// inside out according to its own body-entry flags. No host identities
    /// are inferred from numeric records. No cleanup runs on normal return.
    pub fn world_network_with_cleanup(
        &self,
        input: WorldNetworkInput,
        cleanup: impl FnOnce(&Context, &mut WorldNetworkState, &Error) -> Result<(), Error>,
    ) -> Result<NetworkResult, WorldFailure<WorldNetworkState>> {
        self.world_run(
            input.state,
            |state, host| {
                network::step(
                    state,
                    input.tick,
                    input.at,
                    input.grace,
                    input.respawn,
                    host,
                )
            },
            WorldNetworkState::WORDS,
            WorldNetworkState::put,
            cleanup,
        )
        .map(|(state, tick)| NetworkResult { state, tick })
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
impl step::Host for Host<'_> {
    fn call(
        &mut self,
        state: &mut WorldStepState,
        op: WorldStepCall,
        value: f64,
        flag: bool,
    ) -> WorldStepReply {
        let reply = import_world_step_in(self.context, &WorldStepArgs { op, value, flag });
        self.accept(reply, state, WorldStepReply::take, |r| r.state.clone())
    }
}
impl network::Host for Host<'_> {
    fn call(&mut self, state: &mut WorldNetworkState, op: WorldNetworkCall, value: f64) -> bool {
        let reply = import_world_network_in(self.context, &WorldNetworkArgs { op, value });
        self.accept(reply, state, WorldNetworkReply::take, |r| r.state.clone())
            .value
    }
}
