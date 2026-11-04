//! Thread-affine native store/callback ownership, not a complete native world.
//!
//! Available only with panic=unwind. Clock's existing host trait is infallible:
//! a private unwind stops it on a host error, entirely inside Rust. Unrelated
//! panics retain their payload and propagate to the Rust caller. No method here
//! calls an extern C export. An eventual foreign boundary must catch panics.
//! Raw blocks and records are trusted, schema-encoded inputs, just as in the
//! existing Store API; malformed records may panic. This is not a wire decoder.
//!
//! Clones are aliases, not new worlds. Dispose rejects active invocations and
//! invalidates all aliases; creating a replacement never revives an old handle.
//! Rc makes handles !Send and !Sync. All buffers returned here are owned copies,
//! valid after growth, nested calls and disposal. Entity and string IDs are local
//! to this context. Store-only reset is deliberately not a context operation.
#[path = "context_input.rs"]
mod input;
#[path = "context_world.rs"]
mod world;
pub use world::{NetworkResult, WorldFailure};

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::rc::Rc;

use super::Sim;
use crate::gen::*;
use crate::store::block::{Block, HEADER};
use crate::store::calls::{Calls, ImportExchange, EXCHANGE};
use crate::store::{KindCode, StrId};
use crate::world::clock;

#[derive(Clone, Debug, Default)]
pub struct Record {
    pub row: Vec<f64>,
    pub heap: Vec<f64>,
}
impl Record {
    pub fn new(words: usize, put: impl FnOnce(&mut [f64], &mut Vec<f64>)) -> Self {
        let mut calls = Calls::default();
        calls.result(words, put);
        let (row, heap) = calls.args();
        Self::copy(row, heap)
    }
    fn copy(row: &[f64], heap: &[f64]) -> Self {
        Self {
            row: row.to_vec(),
            heap: heap.to_vec(),
        }
    }
    pub fn read<T>(&self, take: impl FnOnce(&[f64], &[f64]) -> T) -> T {
        take(&self.row, &self.heap)
    }
    pub fn put(&self, row: &mut [f64], heap: &mut Vec<f64>) {
        row.copy_from_slice(&self.row);
        heap.extend_from_slice(&self.heap);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Disposed,
    Busy,
    MissingHandler,
    UnsupportedCommand,
    InvalidKind,
    ExchangeCapacity,
    Host,
}

/// Cloning preserves the original error's identity, even for an empty message.
#[derive(Clone, Debug)]
pub struct Error(Rc<(ErrorKind, String)>);
impl Error {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self(Rc::new((kind, message.into())))
    }
    pub fn host(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Host, message)
    }
    pub fn kind(&self) -> ErrorKind {
        self.0 .0
    }
    pub fn message(&self) -> &str {
        &self.0 .1
    }
    pub fn same(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Debug)]
pub struct Failure {
    pub error: Error,
    /// A callback may publish edits made before it failed. Clock replies use
    /// this to retain the failing tick's state, without completing that tick.
    pub partial: Option<Record>,
}
impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        Self {
            error,
            partial: None,
        }
    }
}
pub type Reply = Result<Record, Failure>;
type Handler = dyn Fn(&Context, &Record) -> Reply;

struct Owner {
    sim: RefCell<Option<Sim>>,
    exchange: RefCell<ImportExchange>,
    handlers: RefCell<HashMap<u32, Rc<Handler>>>,
    active: Cell<usize>,
}

/// A handle cannot move to a different thread, even before its first call.
/// ```compile_fail
/// use bfe_sim::abi::context::Context;
/// let context = Context::new();
/// std::thread::spawn(move || context.dispose().unwrap());
/// ```
#[derive(Clone)]
pub struct Context(Rc<Owner>);
struct Flight(Context);
impl Drop for Flight {
    fn drop(&mut self) {
        self.0 .0.active.set(self.0 .0.active.get() - 1);
    }
}
impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}
impl Context {
    pub fn new() -> Self {
        Self(Rc::new(Owner {
            sim: RefCell::new(Some(Sim::new())),
            exchange: RefCell::new(ImportExchange::new()),
            handlers: RefCell::new(HashMap::new()),
            active: Cell::new(0),
        }))
    }
    fn check(&self) -> Result<(), Error> {
        if self.0.sim.borrow().is_some() {
            Ok(())
        } else {
            Err(Error::new(ErrorKind::Disposed, "context disposed"))
        }
    }
    fn enter(&self) -> Result<Flight, Error> {
        self.check()?;
        self.0.active.set(self.0.active.get() + 1);
        Ok(Flight(self.clone()))
    }
    fn store<T>(&self, f: impl FnOnce(&mut Sim) -> T) -> Result<T, Error> {
        let mut owner = self.0.sim.borrow_mut();
        let sim = owner
            .as_mut()
            .ok_or_else(|| Error::new(ErrorKind::Disposed, "context disposed"))?;
        Ok(f(sim))
    }
    pub fn dispose(&self) -> Result<(), Error> {
        self.check()?;
        if self.0.active.get() != 0 {
            return Err(Error::new(
                ErrorKind::Busy,
                "context has active invocations",
            ));
        }
        let sim = self.0.sim.borrow_mut().take();
        let handlers = std::mem::take(&mut *self.0.handlers.borrow_mut());
        // Drop user-owned closures only after releasing all owner borrows.
        drop(handlers);
        drop(sim);
        Ok(())
    }
    /// Replacement affects the next invocation. The current handler is cloned
    /// before dispatch, so a handler can replace or unregister itself.
    pub fn register(
        &self,
        code: u32,
        handler: impl Fn(&Context, &Record) -> Reply + 'static,
    ) -> Result<(), Error> {
        self.check()?;
        let old = self.0.handlers.borrow_mut().insert(code, Rc::new(handler));
        drop(old);
        Ok(())
    }
    pub fn unregister(&self, code: u32) -> Result<(), Error> {
        self.check()?;
        let old = self.0.handlers.borrow_mut().remove(&code);
        drop(old);
        Ok(())
    }
    pub fn intern(&self, units: &[u16]) -> Result<StrId, Error> {
        self.store(|s| s.strings.intern(units))
    }
    pub fn string(&self, id: StrId) -> Result<Vec<u16>, Error> {
        self.store(|s| s.strings.units(id).to_vec())
    }
    pub fn string_count(&self) -> Result<usize, Error> {
        self.store(|s| s.strings.list.len())
    }
    pub fn scratch(&self, bytes: &[u8]) -> Result<(), Error> {
        self.store(|s| s.scratch_room(bytes.len()).copy_from_slice(bytes))
    }
    pub fn intern_scratch(&self, units: usize) -> Result<StrId, Error> {
        self.store(|s| s.intern_scratch(units))
    }
    pub fn export(&self, kind: u32, mode: u32) -> Result<Vec<f64>, Error> {
        self.store(|s| s.export(kind, mode).map(|b| b.words.clone()))?
            .ok_or_else(|| Error::new(ErrorKind::InvalidKind, "kind has no stored fields"))
    }
    /// Takes a schema-encoded block, using the same Store import as WASM.
    pub fn import(&self, kind: u32, flags: u32, block: Block) -> Result<(), Error> {
        self.store(|s| {
            let Some(dest) = s.blocks.get_mut(kind as usize) else {
                return false;
            };
            *dest = block;
            s.import(kind, flags)
        })?
        .then_some(())
        .ok_or_else(|| Error::new(ErrorKind::InvalidKind, "kind has no stored fields"))
    }
    pub fn retire(&self, kind: u32, slot: u32, generation: u32) -> Result<bool, Error> {
        self.store(|s| s.store.retire(kind as KindCode, slot, generation))
    }
    pub fn calls(&self) -> Result<Record, Error> {
        self.store(|s| {
            if s.calls.block.words.is_empty() {
                Record::default()
            } else {
                let (r, h) = s.calls.args();
                Record::copy(r, h)
            }
        })
    }
    fn publish(&self, record: &Record) -> Result<(), Error> {
        self.store(|s| s.calls.result(record.row.len(), |r, h| record.put(r, h)))
    }
    pub fn events(&self) -> Result<Vec<f64>, Error> {
        self.store(|s| s.events.snapshot())
    }
    pub fn clear_events(&self) -> Result<(), Error> {
        self.store(|s| s.events.clear())
    }

    fn exchange(&self, record: &Record) -> Reply {
        if record.row.len().saturating_add(record.heap.len()) > EXCHANGE - HEADER {
            return Err(Error::new(
                ErrorKind::ExchangeCapacity,
                "record outgrows the import exchange",
            )
            .into());
        }
        let mut exchange = self.0.exchange.borrow_mut();
        exchange.out(record.row.len(), |r, h| record.put(r, h));
        Ok(exchange.read(Record::copy))
    }
    pub fn call_import(&self, code: u32, args: Record) -> Reply {
        let _flight = self.enter()?;
        let handler = self
            .0
            .handlers
            .borrow()
            .get(&code)
            .cloned()
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::MissingHandler,
                    format!("missing import handler {code}"),
                )
            })?;
        let args = self.exchange(&args)?;
        // No Sim, Store, handler-table or exchange borrow spans this call.
        let result = handler(self, &args)?;
        self.exchange(&result)
    }
    /// Only these store commands have native context ownership. Other live
    /// adapters must migrate their owners before being admitted here.
    pub fn command(&self, code: u32, args: Record) -> Reply {
        let _flight = self.enter()?;
        let result = match code {
            COMMAND_ECHO => {
                let args = args.read(Echo::take);
                let out = self.store(|s| super::echo(s, args))?;
                Record::new(Echo::WORDS, |r, h| out.put(r, h))
            }
            COMMAND_FILL => {
                let args = args.read(Fill::take);
                self.store(|s| super::fill(s, args))?;
                Record::default()
            }
            COMMAND_RELAY => {
                let args = args.read(Echo::take);
                let out = import_ask_in(self, &args)?;
                Record::new(Echo::WORDS, |r, h| out.put(r, h))
            }
            _ => {
                return Err(Error::new(
                    ErrorKind::UnsupportedCommand,
                    "command has no native context adapter",
                )
                .into())
            }
        };
        self.publish(&result)?;
        Ok(result)
    }

    pub fn clock(&self, input: WorldClockInput) -> Result<WorldClockResult, ClockFailure> {
        let mut state = input.state;
        let _flight = self.enter().map_err(|error| ClockFailure {
            state: state.clone(),
            error,
        })?;
        let mut host = ClockHost {
            context: self,
            failure: None,
        };
        let result = catch_unwind(AssertUnwindSafe(|| match input.op {
            WorldClockOperation::Advance => {
                clock::advance(&mut state, input.seconds, input.limited, &mut host)
            }
            WorldClockOperation::Reset => {
                clock::reset(&mut state);
                Ok(0.0)
            }
            WorldClockOperation::Rebase => {
                clock::rebase(&mut state);
                Ok(0.0)
            }
        }));
        let (ran, error, failure) = match result {
            Ok(Ok(ran)) => (ran, None, None),
            Ok(Err(message)) => (0.0, Some(message.to_string()), None),
            Err(payload) => {
                if !payload.is::<Stop>() || host.failure.is_none() {
                    resume_unwind(payload);
                }
                let error = host.failure.take().unwrap();
                (0.0, Some(error.message().to_string()), Some(error))
            }
        };
        let error = error.map(|message| self.store(|s| s.strings.intern_str(&message)).unwrap());
        let out = WorldClockResult { state, ran, error };
        self.publish(&Record::new(WorldClockResult::WORDS, |r, h| out.put(r, h)))
            .unwrap();
        if let Some(error) = failure {
            Err(ClockFailure {
                state: out.state,
                error,
            })
        } else {
            Ok(out)
        }
    }
}

#[derive(Debug)]
pub struct ClockFailure {
    pub state: WorldClockState,
    pub error: Error,
}
// No public constructor, so a callback cannot manufacture our stop marker.
struct Stop;
struct ClockHost<'a> {
    context: &'a Context,
    failure: Option<Error>,
}
impl ClockHost<'_> {
    fn call(&mut self, state: &mut WorldClockState, op: WorldClockCall, dt: f64, tick: f64) -> f64 {
        match import_world_clock_in(
            self.context,
            &WorldClockArgs {
                op,
                state: state.clone(),
                dt,
                tick,
            },
        ) {
            Ok(reply) => {
                *state = reply.state;
                reply.time
            }
            Err(failure) => {
                if let Some(partial) = failure.partial {
                    *state = partial.read(WorldClockReply::take).state;
                }
                self.failure = Some(failure.error);
                resume_unwind(Box::new(Stop));
            }
        }
    }
}
impl clock::Host for ClockHost<'_> {
    fn now(&mut self, state: &mut WorldClockState) -> f64 {
        self.call(state, WorldClockCall::Now, 0.0, 0.0)
    }
    fn step(&mut self, state: &mut WorldClockState, dt: f64, tick: f64) {
        self.call(state, WorldClockCall::Step, dt, tick);
    }
}
