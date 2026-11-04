//! Private command replay with a fixed endpoint and fresh input per frame.
//!
//! JavaScript retains observable property reads and object identities. This
//! cursor consumes inert snapshots, returning owned token lists before apply.
//! Tokens belong to one frame exchange and are never retained by the cursor.

pub type Result<T> = std::result::Result<T, &'static str>;
pub type Token = u64;
const EXCHANGE: &str = "invalid replay exchange";
const PHASE: &str = "invalid replay phase";
const ARITHMETIC: &str = "unsupported replay arithmetic";
const TICK_BOUND: f64 = 4503599627370496.0;
const MAX_TOKEN: Token = 9007199254740991;
const MAX_ARRAY: usize = u32::MAX as usize;
pub const VIEW_FIELDS: usize = 7;

fn flag(value: bool) -> f64 {
    u8::from(value) as f64
}

fn tick_supported(tick: f64) -> bool {
    tick.is_finite() && tick.abs() < TICK_BOUND
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Phase {
    Unstarted = 0,
    Ready = 1,
    AwaitApply = 2,
    Done = 3,
}

/// A caller copies this position before any foreign callback. Mine remains
/// true for a fractional interval containing no tick: its initial view still
/// has to be read before returning last.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Position {
    pub mine: bool,
    pub last: f64,
    pub index: usize,
    pub tick: f64,
    pub phase: Phase,
}

impl Position {
    pub fn words(self) -> [f64; 5] {
        [
            flag(self.mine),
            self.last,
            self.index as f64,
            self.tick,
            self.phase as u8 as f64,
        ]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ScanEntry {
    pub generation: f64,
    /// Foreign generations omit a tick that the initial scans never read.
    pub tick: Option<f64>,
}

/// Field numbers follow the original seven-field replay view list. The vector
/// of bindings carries insertion order separately from these field numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewBinding {
    pub field: u8,
    pub value: Token,
}

#[derive(Clone, Debug)]
pub struct SentCommand {
    pub down: Token,
    /// Samples from worldLook ?? look, preserving each original element token.
    pub look: Vec<Token>,
    /// Defined fields in the original fixed field-read order.
    pub view: Vec<ViewBinding>,
}

#[derive(Clone, Debug)]
pub struct MatchingCommand {
    pub tick: f64,
    pub generation: f64,
    pub sequence: Token,
    pub actions: Vec<Token>,
    /// None represents strict sent === false. No later fields were read.
    pub sent: Option<SentCommand>,
}

#[derive(Clone, Debug)]
pub enum Slot {
    /// A future tick stops consumption before generation or body fields.
    Blocker {
        tick: f64,
    },
    Foreign {
        tick: f64,
        generation: f64,
    },
    Matching(MatchingCommand),
}

impl Slot {
    fn tick(&self) -> f64 {
        match self {
            Self::Blocker { tick } | Self::Foreign { tick, .. } => *tick,
            Self::Matching(command) => command.tick,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FrameInput {
    pub pending_length: usize,
    pub down: Token,
    pub view: Vec<ViewBinding>,
    /// The current reachable prefix, starting at the cursor's retained index.
    /// It ends at the current pending length or includes its first blocker.
    pub slots: Vec<Slot>,
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub position: Position,
    pub last: bool,
    pub any: bool,
    pub down: Token,
    pub actions: Vec<Token>,
    pub look: Vec<Token>,
    pub sequences: Vec<Token>,
    pub view: Vec<ViewBinding>,
}

fn append_tokens(output: &mut Vec<f64>, tokens: &[Token]) {
    output.push(tokens.len() as f64);
    output.extend(tokens.iter().map(|&token| token as f64));
}

fn append_view(output: &mut Vec<f64>, view: &[ViewBinding]) {
    output.push(view.len() as f64);
    for binding in view {
        output.extend([binding.field as f64, binding.value as f64]);
    }
}

impl Frame {
    pub fn words(&self) -> Vec<f64> {
        let mut output = self.position.words().to_vec();
        output.extend([flag(self.last), flag(self.any), self.down as f64]);
        append_tokens(&mut output, &self.actions);
        append_tokens(&mut output, &self.look);
        append_tokens(&mut output, &self.sequences);
        append_view(&mut output, &self.view);
        output
    }
}

fn valid_token(token: Token) -> Result<()> {
    if token <= MAX_TOKEN {
        Ok(())
    } else {
        Err(EXCHANGE)
    }
}

fn valid_tokens(tokens: &[Token]) -> Result<()> {
    for &token in tokens {
        valid_token(token)?;
    }
    Ok(())
}

fn valid_view(view: &[ViewBinding], updates: bool) -> Result<()> {
    let mut seen = 0u8;
    let mut previous = None;
    for binding in view {
        if binding.field as usize >= VIEW_FIELDS || seen & (1 << binding.field) != 0 {
            return Err(EXCHANGE);
        }
        if updates && previous.is_some_and(|field| binding.field <= field) {
            return Err(EXCHANGE);
        }
        seen |= 1 << binding.field;
        previous = Some(binding.field);
        valid_token(binding.value)?;
    }
    Ok(())
}

/// The endpoint never changes after Begin. The next frame gets a new pending
/// snapshot, so callbacks may insert, remove or replace entries without the
/// cursor remembering stale commands. Only these scalars survive callbacks.
#[derive(Clone, Debug)]
pub struct Cursor {
    generation: f64,
    position: Position,
}

impl Cursor {
    pub fn begin(from: f64, generation: f64, entries: &[ScanEntry]) -> Result<Self> {
        if !tick_supported(from) || !generation.is_finite() {
            return Err(ARITHMETIC);
        }
        let mut last = from;
        for entry in entries {
            if !entry.generation.is_finite() {
                return Err(ARITHMETIC);
            }
            if entry.generation == generation {
                let tick = entry.tick.ok_or(EXCHANGE)?;
                if !tick_supported(tick) {
                    return Err(ARITHMETIC);
                }
                if tick > last {
                    last = tick;
                }
            }
        }
        let mut mine = false;
        for entry in entries {
            if entry.generation == generation {
                mine = true;
                break;
            }
        }
        if mine && last == from {
            last = from + 1.0;
        }
        let tick = if mine { from + 1.0 } else { from };
        let phase = if mine && tick <= last {
            Phase::Ready
        } else {
            Phase::Done
        };
        Ok(Self {
            generation,
            position: Position {
                mine,
                last,
                index: 0,
                tick,
                phase,
            },
        })
    }

    pub fn position(&self) -> Position {
        self.position
    }

    /// Assemble one complete inert frame. An invalid exchange leaves the
    /// position unchanged; it cannot consume a prefix before validation fails.
    pub fn frame(&mut self, input: FrameInput) -> Result<Frame> {
        if self.position.phase != Phase::Ready {
            return Err(PHASE);
        }
        if input.pending_length > MAX_ARRAY {
            return Err(EXCHANGE);
        }
        valid_token(input.down)?;
        valid_view(&input.view, false)?;
        let mut index = self.position.index;
        let mut output = Frame {
            position: self.position,
            last: self.position.tick == self.position.last,
            any: false,
            down: input.down,
            actions: Vec::new(),
            look: Vec::new(),
            sequences: Vec::new(),
            view: input.view,
        };
        let mut blocked = false;
        let count = input.slots.len();
        for (at, slot) in input.slots.into_iter().enumerate() {
            if index >= input.pending_length {
                return Err(EXCHANGE);
            }
            let tick = slot.tick();
            if !tick.is_finite() {
                return Err(ARITHMETIC);
            }
            if tick > self.position.tick {
                if !matches!(slot, Slot::Blocker { .. }) || at + 1 != count {
                    return Err(EXCHANGE);
                }
                blocked = true;
                break;
            }
            match slot {
                Slot::Blocker { .. } => return Err(EXCHANGE),
                Slot::Foreign { generation, .. } => {
                    if !generation.is_finite() {
                        return Err(ARITHMETIC);
                    }
                    if generation == self.generation {
                        return Err(EXCHANGE);
                    }
                }
                Slot::Matching(command) => {
                    if !command.generation.is_finite() {
                        return Err(ARITHMETIC);
                    }
                    if command.generation != self.generation {
                        return Err(EXCHANGE);
                    }
                    valid_token(command.sequence)?;
                    valid_tokens(&command.actions)?;
                    output.any = true;
                    output.sequences.push(command.sequence);
                    output.actions.extend(command.actions);
                    if let Some(sent) = command.sent {
                        valid_token(sent.down)?;
                        valid_tokens(&sent.look)?;
                        valid_view(&sent.view, true)?;
                        output.down = sent.down;
                        output.look.extend(sent.look);
                        for binding in sent.view {
                            if let Some(previous) = output
                                .view
                                .iter_mut()
                                .find(|old| old.field == binding.field)
                            {
                                previous.value = binding.value;
                            } else {
                                output.view.push(binding);
                            }
                        }
                    }
                }
            }
            index += 1;
        }
        if !blocked && index < input.pending_length {
            return Err(EXCHANGE);
        }
        output.position.index = index;
        output.position.phase = Phase::AwaitApply;
        self.position = output.position;
        Ok(output)
    }

    /// Call only after apply returns. Dropping a cursor after an exception or
    /// a one-way JavaScript handoff does not require this phase to complete.
    pub fn advance(&mut self) -> Result<Position> {
        if self.position.phase != Phase::AwaitApply {
            return Err(PHASE);
        }
        let tick = self.position.tick + 1.0;
        if !tick.is_finite() || tick <= self.position.tick {
            return Err(ARITHMETIC);
        }
        self.position.tick = tick;
        self.position.phase = if tick <= self.position.last {
            Phase::Ready
        } else {
            Phase::Done
        };
        Ok(self.position)
    }
}

struct Words<'a> {
    input: &'a [f64],
    at: usize,
}

impl<'a> Words<'a> {
    fn remaining(&self) -> usize {
        self.input.len() - self.at
    }

    fn number(&mut self) -> Result<f64> {
        let value = *self.input.get(self.at).ok_or(EXCHANGE)?;
        self.at += 1;
        Ok(value)
    }

    fn flag(&mut self) -> Result<bool> {
        match self.number()? {
            0.0 => Ok(false),
            1.0 => Ok(true),
            _ => Err(EXCHANGE),
        }
    }

    fn count(&mut self, bound: usize) -> Result<usize> {
        let value = self.number()?;
        if value.is_finite() && value >= 0.0 && value.fract() == 0.0 && value <= bound as f64 {
            Ok(value as usize)
        } else {
            Err(EXCHANGE)
        }
    }

    fn token(&mut self) -> Result<Token> {
        let value = self.number()?;
        if value.is_finite() && value >= 0.0 && value.fract() == 0.0 && value <= MAX_TOKEN as f64 {
            Ok(value as Token)
        } else {
            Err(EXCHANGE)
        }
    }

    fn tokens(&mut self) -> Result<Vec<Token>> {
        let count = self.count(self.remaining().saturating_sub(1))?;
        let mut tokens = Vec::with_capacity(count);
        for _ in 0..count {
            tokens.push(self.token()?);
        }
        Ok(tokens)
    }

    fn view(&mut self) -> Result<Vec<ViewBinding>> {
        let bound = ((self.remaining().saturating_sub(1)) / 2).min(VIEW_FIELDS);
        let count = self.count(bound)?;
        let mut view = Vec::with_capacity(count);
        for _ in 0..count {
            view.push(ViewBinding {
                field: self.count(VIEW_FIELDS - 1)? as u8,
                value: self.token()?,
            });
        }
        Ok(view)
    }

    fn slot(&mut self) -> Result<Slot> {
        let kind = self.count(2)?;
        let tick = self.number()?;
        match kind {
            0 => Ok(Slot::Blocker { tick }),
            1 => Ok(Slot::Foreign {
                tick,
                generation: self.number()?,
            }),
            2 => {
                let generation = self.number()?;
                let sequence = self.token()?;
                let actions = self.tokens()?;
                let sent = if self.flag()? {
                    None
                } else {
                    Some(SentCommand {
                        down: self.token()?,
                        look: self.tokens()?,
                        view: self.view()?,
                    })
                };
                Ok(Slot::Matching(MatchingCommand {
                    tick,
                    generation,
                    sequence,
                    actions,
                    sent,
                }))
            }
            _ => unreachable!(),
        }
    }

    fn frame(&mut self) -> Result<FrameInput> {
        let pending_length = self.count(MAX_ARRAY)?;
        let down = self.token()?;
        let view = self.view()?;
        let count = self.count(self.remaining().saturating_sub(1) / 2)?;
        let mut slots = Vec::with_capacity(count);
        for _ in 0..count {
            slots.push(self.slot()?);
        }
        Ok(FrameInput {
            pending_length,
            down,
            view,
            slots,
        })
    }

    fn finish(&self) -> Result<()> {
        if self.at == self.input.len() {
            Ok(())
        } else {
            Err(EXCHANGE)
        }
    }
}

/// Per-invocation cursor and scalar exchange. It makes no foreign calls.
/// Create/drop trace records belong to the adapter, not this numeric protocol.
///
/// Position S = [mine,last,index,tick,phase], with phases Unstarted=0, Ready=1,
/// AwaitApply=2 and Done=3. Lists are [length,...tokens]. Views are
/// [length,...(field,token)], where fields 0..6 follow REPLAY_VIEW_FIELDS.
///
/// - 0 Begin: [from,generation,count,...(generation,hasTick,tick)] -> S.
///   Foreign entries can omit their unread tick using hasTick=0.
/// - 1 Frame: [pendingLength,downToken,view,slotCount,...slots]
///   -> [S,lastFrame,any,downToken,actions,look,sequences,view].
///   Slot 0 is [0,tick] and must be the final, blocking slot.
///   Slot 1 is [1,tick,generation] and must be a consumed foreign life.
///   Slot 2 is [2,tick,generation,sequenceToken,actions,sentFalse], followed
///   by [downToken,look,viewUpdates] only when sentFalse is 0.
/// - 2 Advance: [] -> S. The caller runs apply before this operation.
/// - 3 Position: [] -> S without changing state.
///
/// Begin eligibility uses finite numbers with abs(from/matching ticks)<2**52.
/// Frame tokens are opaque safe integer IDs, valid only in that frame's host
/// arena. The host copies output, resolves its references and retains the
/// actual down/view objects before any trace or apply callback.
#[derive(Default)]
pub struct Session {
    pub input: Vec<f64>,
    pub output: Vec<f64>,
    pub cursor: Option<Cursor>,
    error: Option<&'static str>,
}

impl Session {
    pub fn run(&mut self, op: u32) -> Result<()> {
        self.output.clear();
        let mut r = Words {
            input: &self.input,
            at: 0,
        };
        self.output = match op {
            0 => {
                if self.cursor.is_some() {
                    return Err(PHASE);
                }
                let from = r.number()?;
                let generation = r.number()?;
                let count = r.count(r.remaining().saturating_sub(1) / 3)?;
                let mut entries = Vec::with_capacity(count);
                for _ in 0..count {
                    let generation = r.number()?;
                    let present = r.flag()?;
                    let tick = r.number()?;
                    entries.push(ScanEntry {
                        generation,
                        tick: present.then_some(tick),
                    });
                }
                r.finish()?;
                let cursor = Cursor::begin(from, generation, &entries)?;
                let output = cursor.position().words().to_vec();
                self.cursor = Some(cursor);
                output
            }
            1 => {
                let input = r.frame()?;
                r.finish()?;
                self.cursor.as_mut().ok_or(PHASE)?.frame(input)?.words()
            }
            2 => {
                r.finish()?;
                self.cursor
                    .as_mut()
                    .ok_or(PHASE)?
                    .advance()?
                    .words()
                    .to_vec()
            }
            3 => {
                r.finish()?;
                self.cursor
                    .as_ref()
                    .map_or(
                        Position {
                            mine: false,
                            last: 0.0,
                            index: 0,
                            tick: 0.0,
                            phase: Phase::Unstarted,
                        },
                        Cursor::position,
                    )
                    .words()
                    .to_vec()
            }
            _ => return Err(EXCHANGE),
        };
        Ok(())
    }
}

#[no_mangle]
pub extern "C" fn prediction_replay_create() -> *mut Session {
    Box::into_raw(Box::new(Session::default()))
}

#[no_mangle]
pub unsafe extern "C" fn prediction_replay_drop(p: *mut Session) {
    if !p.is_null() {
        drop(Box::from_raw(p));
    }
}

#[no_mangle]
pub unsafe extern "C" fn prediction_replay_room(p: *mut Session, n: usize) -> *mut f64 {
    let s = &mut *p;
    s.input.resize(n, 0.0);
    s.input.as_mut_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn prediction_replay_run(p: *mut Session, op: u32) -> u32 {
    let s = &mut *p;
    s.error = s.run(op).err();
    u32::from(s.error.is_none())
}

#[no_mangle]
pub unsafe extern "C" fn prediction_replay_output(p: *const Session) -> *const f64 {
    (&*p).output.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn prediction_replay_output_len(p: *const Session) -> usize {
    (&*p).output.len()
}

#[no_mangle]
pub unsafe extern "C" fn prediction_replay_error(p: *const Session) -> *const u8 {
    (&*p).error.unwrap_or("").as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn prediction_replay_error_len(p: *const Session) -> usize {
    (&*p).error.unwrap_or("").len()
}
