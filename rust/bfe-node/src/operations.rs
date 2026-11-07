//! Owned arenas for sim/runtime.js commands and Store mirror barriers. Context
//! still allocates its cold store replies; these counters describe the retained
//! addon arenas, not unexposed world allocations.
use super::*;
use bfe_sim::abi::context_step::Fault;
use bfe_sim::gen::*;
use bfe_sim::store::{block::Block, StrId};
use napi::JsObject;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Request { Operation(u32), Export(u32, u32), Events, Step, Bank(u32), Publish(u32) }
#[napi(object)]
pub struct Reserve {
    pub input_words: u32,
    pub output_words: u32,
    pub publication_bytes: Option<u32>,
}

#[derive(Default)]
pub(super) struct Arenas {
    pub(super) block: Block,
    pub(super) args: Vec<f64>,
    pub(super) pending: Option<Request>,
    pub(super) last_fault: Option<&'static str>,
}

// The foundation commands can create arbitrary synthetic stores or events.
// Bound their counts before dispatch, rather than trusting float-to-int casts.
const MAX_BATCH: usize = 65_536;
pub(super) fn invalid(message: &'static str) -> Error { Error::new(Status::InvalidArg, message) }
fn context_error(error: bfe_sim::abi::context::Error) -> Error { Error::from_reason(error.to_string()) }
fn count(value: f64) -> Result<usize> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > MAX_BATCH as f64 {
        return Err(invalid("Numeric count exceeds admission capacity"));
    }
    Ok(value as usize)
}

// Kind layouts stay tied to generated constants. Singleton rows have no slab
// prefix; treating their first scalar as a slot would reject valid state.
fn layout(kind: u32, full: bool) -> Result<(usize, bool)> {
    macro_rules! kinds {
        ($($code:ident => $ty:ident, $entity:expr);* $(;)?) => {
            match kind {
                $(code if code == $code as u32 => Ok((if full { $ty::FULL_STRIDE } else { $ty::IMPORT_STRIDE }, $entity)),)*
                _ => Err(invalid("Kind has no stored fields")),
            }
        }
    }
    kinds! {
        KIND_BATTLE => Battle, false; KIND_BEAM => Beam, true;
        KIND_BOLT => Bolt, true; KIND_CHARACTER => Character, true;
        KIND_COMMAND_POSTS => CommandPosts, false; KIND_GRENADE => Grenade, true;
        KIND_HUMAN_SPAWN => HumanSpawn, false; KIND_ITEM => Item, true;
        KIND_ITEMS => Items, false; KIND_MOBILE_POST => MobilePost, true;
        KIND_MOBILE_POSTS => MobilePosts, false; KIND_ORDNANCE => Ordnance, false;
        KIND_OWNER_SNAPSHOT => OwnerSnapshot, true; KIND_POST => Post, true;
        KIND_RECON_CONTEXT => ReconContext, true; KIND_RECON_DROID => ReconDroid, true;
        KIND_ROUND => Round, true; KIND_SEAT_LEASE => SeatLease, true;
        KIND_SEATS => Seats, false; KIND_SPAWNING => Spawning, false;
        KIND_TURRET_SEATS => TurretSeats, false; KIND_UNIT => Unit, true;
        KIND_UNITS => Units, false;
        KIND_VEHICLE => Vehicle, true; KIND_VEHICLES => Vehicles, false;
    }
}

impl Arenas {
    pub fn release(&mut self) {
        self.block = Block::default(); self.args = Vec::new(); self.pending = None;
    }
    pub fn reserve(&mut self, reserve: Option<Reserve>) -> Result<()> {
        let reserve = reserve.unwrap_or(Reserve {input_words: 8192, output_words: 8192, publication_bytes: None});
        // Reject unreasonable reservations before asking the allocator. These
        // are admission arenas; Context's cold replies retain their own sizes.
        const MAX_WORDS: u32 = 4 * 1024 * 1024;
        if reserve.input_words > MAX_WORDS || reserve.output_words > MAX_WORDS {
            return Err(invalid("Arena reservation exceeds admission capacity"));
        }
        self.args.try_reserve_exact((reserve.input_words as usize).max(WorldTickInput::WORDS).saturating_sub(self.args.len())).map_err(|_| invalid("Input arena capacity"))?;
        let output = (reserve.output_words as usize).max(WorldTickOutput::WORDS);
        self.block.words.try_reserve_exact(output.saturating_sub(self.block.words.len())).map_err(|_| invalid("Output arena capacity"))?;
        Ok(())
    }
    pub(super) fn idle(&self) -> Result<()> {
        if self.pending.is_some() { return Err(Error::from_reason("Busy: pending arena result")); }
        Ok(())
    }
    pub(super) fn input_capacity(&self, args: &[f64]) -> Result<()> {
        if args.len() > self.args.capacity() {
            return Err(invalid("Input arena requires admission capacity"));
        }
        Ok(())
    }
    pub(super) fn matches(&self, request: Request, args: &[f64]) -> Result<bool> {
        let Some(pending) = self.pending else {return Ok(false);};
        // Numeric equality would collapse signed zero and reject equal NaN bits.
        if pending != request || self.args.len() != args.len()
            || !self.args.iter().zip(args).all(|(a, b)| a.to_bits() == b.to_bits()) {
            return Err(Error::from_reason("Busy: pending arena result"));
        }
        Ok(true)
    }
    pub(super) fn retain(&mut self, request: Request, args: &[f64], words: &[f64]) {
        self.args.clear(); self.args.extend_from_slice(args);
        self.block.words.clear(); self.block.words.extend_from_slice(words);
        self.pending = Some(request);
    }
    pub(super) fn deliver(&mut self, output: &blocks::Arena) -> u32 {
        let required = output.write(&self.block.words);
        if output.length < self.block.words.len() { self.last_fault = Some("Capacity"); }
        else {
            self.pending = None;
            self.block.words.clear(); self.block.heap.clear(); self.args.clear();
            self.last_fault = None;
        }
        required
    }
}

#[napi(object)]
pub struct Diagnostics {
    pub created: u32,
    pub released: u32,
    pub live: u32,
    pub reserved_words: f64,
    pub used_words: f64,
    pub publication_reserved_bytes: f64,
    pub publication_used_bytes: f64,
    pub last_fault: Option<String>,
    pub poisoned: bool,
    pub closed: bool,
}

impl NativeOwner {
    // A RefCell loan is the native Busy guard. The unwind is caught while the
    // owner is still reachable, so a panic marks it before releasing that loan.
    pub(super) fn invoke<T>(&self, call: impl FnOnce(&mut OwnedContext) -> Result<T>) -> Result<T> {
        boundary(|| {
            let mut owner = self.owner.try_borrow_mut().map_err(|_| Error::from_reason("Busy"))?;
            if owner.context.is_none() { return Err(Error::from_reason("Native owner closed")); }
            if owner.poisoned { return Err(Error::from_reason("Poisoned")); }
            match catch_unwind(AssertUnwindSafe(|| call(&mut owner))) {
                Ok(result) => {
                    if result.is_err() && !owner.poisoned { owner.arenas.last_fault = Some("Input"); }
                    result
                }
                Err(_) => {
                    owner.poisoned = true;
                    owner.arenas.last_fault = Some("Poisoned");
                    Err(Error::new(Status::GenericFailure, "Native host panic"))
                }
            }
        })
    }
}

#[napi]
impl NativeOwner {
    // Binds sim/runtime.js operations to the generated native table. Arguments
    // are scalars followed by a generated record and its block-relative heap;
    // the Context writes directly into the retained result block.
    #[napi]
    pub fn operation(&self, env: Env, op: u32, args: JsObject, out: JsObject) -> Result<u32> {
        self.invoke(|owner| {
            let input = blocks::arena(env, &args)?;
            let output = blocks::arena(env, &out)?;
            input.read(|words| {
                blocks::validate(words)?;
                owner.arenas.input_capacity(words)?;
                let request = Request::Operation(op);
                if !owner.arenas.matches(request, words)? {
                    let shape = bfe_sim::gen::operation(op).ok_or_else(|| invalid("Unsupported native operation"))?;
                    if words[BlockHeader::ROWS] != 1.0 || words[BlockHeader::RETIRED] != 0.0
                        || words[BlockHeader::STRIDE] != (shape.scalars + shape.record) as f64
                        || (!shape.heap && words[BlockHeader::HEAP_WORDS] != 0.0) {
                        return Err(invalid("Invalid operation block"));
                    }
                    let row = &words[BlockHeader::WORDS..];
                    // Synthetic fill/echo are deliberately bounded before the
                    // shared bodies can create an arbitrary store or event run.
                    if op == OPERATION_FILL {
                        count(row[0])?; count(row[1])?;
                        if row[0] == 0.0 || row[0] > KINDS as f64 {return Err(invalid("Invalid kind"));}
                    } else if op == OPERATION_ECHO { count(row[Echo::WORDS - 1])?; }
                    let result = owner.context.as_ref().unwrap().run_operation_into(op, row, &mut owner.arenas.block);
                    match result {
                        Ok(()) => {},
                        Err(Fault::Context(error)) => super::world::context_result(owner, Err(error))?,
                        Err(Fault::UnknownOperation(_)) => return Err(invalid("Unsupported native operation")),
                        Err(Fault::Arguments {..}) => return Err(invalid("Invalid operation row")),
                        Err(Fault::Refused) => return Err(Error::from_reason("Native operation refused")),
                    }
                    owner.arenas.args.clear(); owner.arenas.args.extend_from_slice(words);
                    owner.arenas.pending = Some(request);
                }
                Ok(())
            })?;
            Ok(owner.arenas.deliver(&output))
        })
    }

    // Binds the Store import in sim/runtime.js. The shared generated decoder owns each kind's
    // field layout. A malformed heap may panic there; invoke poisons the owner.
    #[napi]
    pub fn import_kind(&self, env: Env, kind: u32, flags: u32, block: JsObject) -> Result<()> {
        self.invoke(|owner| {
            owner.arenas.idle()?;
            if kind == 0 || kind > KINDS as u32 || flags > 3 {return Err(invalid("Invalid import kind or flags"));}
            let input = blocks::arena(env, &block)?;
            input.read(|words| {
                blocks::validate(words)?;
                let rows = count(words[BlockHeader::ROWS])?;
                let stride = words[BlockHeader::STRIDE] as usize;
                let (expected, entity) = layout(kind, flags & 1 != 0)?;
                if stride != expected || (!entity && rows != 1) || words[BlockHeader::RETIRED] != 0.0 {
                    return Err(invalid("Invalid import block layout"));
                }
                // Full and partial rows share the slot/generation prefix when
                // present. Reject enormous slots before Slab::ensure can grow.
                if entity {
                    for row in words[BlockHeader::WORDS..BlockHeader::WORDS + rows * stride].chunks_exact(stride) {
                        count(row[0])?;
                    }
                }
                owner.context.as_ref().unwrap().import(kind, flags, Block {words: words.to_vec(), heap: Vec::new()})
                    .map_err(context_error)
            })
        })
    }

    // Mode zero consumes retirements in Context. Retain that exact snapshot
    // across short output arenas, so the next admission can copy it once.
    #[napi]
    pub fn export_kind(&self, env: Env, kind: u32, mode: u32, out: JsObject) -> Result<u32> {
        self.invoke(|owner| {
            let output = blocks::arena(env, &out)?;
            if mode > 2 {return Err(invalid("Invalid export mode"));}
            let request = Request::Export(kind, mode);
            if !owner.arenas.matches(request, &[])? {
                if kind == 0 || kind > KINDS as u32 {return Err(invalid("Invalid kind"));}
                let words = owner.context.as_ref().unwrap().export(kind, mode).map_err(context_error)?;
                owner.arenas.retain(request, &[], &words);
            }
            Ok(owner.arenas.deliver(&output))
        })
    }

    // Ports sim/runtime.js drainEvents: a failed capacity check leaves the
    // ordered native event list intact. Clear only after a successful copy.
    #[napi]
    pub fn events(&self, env: Env, out: JsObject) -> Result<u32> {
        self.invoke(|owner| {
            let output = blocks::arena(env, &out)?;
            if !owner.arenas.matches(Request::Events, &[])? {
                let words = owner.context.as_ref().unwrap().events().map_err(context_error)?;
                owner.arenas.retain(Request::Events, &[], &words);
            }
            let fits = output.length >= owner.arenas.block.words.len();
            let result = owner.arenas.deliver(&output);
            if fits { owner.context.as_ref().unwrap().clear_events().map_err(context_error)?; }
            Ok(result)
        })
    }

    // UTF-16 code units preserve lone surrogates, unlike a Rust String. These
    // are cold admissions and reads, using the Context's battle-local IDs.
    #[napi]
    pub fn intern(&self, units: Vec<u16>) -> Result<u32> {
        self.invoke(|owner| {
            owner.arenas.idle()?;
            owner.context.as_ref().unwrap().intern(&units).map(|id| id.0).map_err(context_error)
        })
    }
    #[napi]
    pub fn string(&self, id: u32) -> Result<Vec<u16>> {
        self.invoke(|owner| {
            let context = owner.context.as_ref().unwrap();
            if id as usize >= context.string_count().map_err(context_error)? {return Err(invalid("Invalid string ID"));}
            context.string(StrId(id)).map_err(context_error)
        })
    }
    #[napi]
    pub fn retire(&self, kind: u32, slot: u32, generation: u32) -> Result<bool> {
        self.invoke(|owner| {
            owner.arenas.idle()?;
            if kind == 0 || kind > KINDS as u32 {return Err(invalid("Invalid kind"));}
            owner.context.as_ref().unwrap().retire(kind, slot, generation).map_err(context_error)
        })
    }
    #[napi]
    pub fn diagnostics(&self) -> Result<Diagnostics> {
        boundary(|| {
            let owner = self.owner.try_borrow().map_err(|_| Error::from_reason("Busy"))?;
            let released = RELEASED.load(Ordering::SeqCst);
            let created = CREATED.load(Ordering::SeqCst);
            let (publication_reserved, publication_used) = owner.publication.memory_bytes();
            Ok(Diagnostics {
                created, released, live: created.saturating_sub(released),
                reserved_words: (owner.arenas.block.words.capacity() + owner.arenas.block.heap.capacity() + owner.arenas.args.capacity()) as f64,
                used_words: (owner.arenas.block.words.len() + owner.arenas.block.heap.len() + owner.arenas.args.len()) as f64,
                publication_reserved_bytes: publication_reserved as f64,
                publication_used_bytes: publication_used as f64,
                last_fault: owner.arenas.last_fault.map(str::to_owned),
                poisoned: owner.poisoned, closed: owner.context.is_none(),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_requests_compare_float_bits_and_block_other_mutations() {
        let mut arena = Arenas::default();
        let args = [f64::from_bits(0x7ff8_0000_0000_0042), -0.0];
        arena.retain(Request::Operation(COMMAND_ECHO), &args, &args);
        assert!(arena.matches(Request::Operation(COMMAND_ECHO), &args).unwrap());
        assert!(arena.matches(Request::Operation(COMMAND_ECHO), &[args[0], 0.0]).is_err());
        assert!(arena.matches(Request::Events, &[]).is_err());
        assert!(arena.idle().is_err());
        assert_eq!(arena.block.words[0].to_bits(), args[0].to_bits());
        assert_eq!(arena.block.words[1].to_bits(), args[1].to_bits());
    }
    #[test]
    fn generated_layouts_distinguish_singletons_and_entities() {
        assert_eq!(layout(KIND_BOLT as u32, true).unwrap(), (Bolt::FULL_STRIDE, true));
        assert_eq!(layout(KIND_BATTLE as u32, false).unwrap(), (Battle::IMPORT_STRIDE, false));
        assert!(layout(KIND_BODY as u32, true).is_err());
    }
    #[test]
    fn count_limits_reject_unbounded_work_without_panicking() {
        for value in [f64::NAN, f64::INFINITY, -1.0, 0.5, u32::MAX as f64] {
            assert!(count(value).is_err());
        }
        assert_eq!(count(MAX_BATCH as f64).unwrap(), MAX_BATCH);
    }
}
