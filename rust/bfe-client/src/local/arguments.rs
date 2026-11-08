//! The generated operation boundary uses a scalar prefix followed by one
//! record and its heap. Retain both packing arenas and the answer block;
//! encoding never constructs a new Vec for a desktop command or an ACK.
use bfe_sim::{abi::{context::Context, context_step::Fault}, gen, store::block::Block};

pub struct Arguments {
    pub words: Vec<f64>,
    heap: Vec<f64>,
    pub answer: Block,
}
impl Default for Arguments {
    fn default() -> Self {
        Self::new()
    }
}

impl Arguments {
    // A local tick pushes one command, whose largest list is thirty-two
    // actions or receipts. Seven view fields and eleven held names fit the
    // same heap. Result space covers a push's retained view and retired id;
    // neither the queued commands nor an inspection bank cross this boundary.
    pub fn new() -> Self {
        let scalars = gen::OPERATIONS.iter().map(|o| o.scalars).max().unwrap_or(0);
        let row = gen::OPERATIONS.iter().map(|o| o.record).max().unwrap_or(0);
        let heap = 32usize.max(7 * gen::ImField::WORDS + 11);
        let answer = gen::BlockHeader::WORDS + gen::ImResult::WORDS + 7 * gen::ImField::WORDS + 1;
        Self { words: Vec::with_capacity(scalars + row + heap), heap: Vec::with_capacity(heap),
            answer: Block { words: Vec::with_capacity(answer), heap: Vec::new() } }
    }

    // Record::put is generated alongside its offsets. Its heap offsets stay
    // relative to that heap, independent of the operation's scalar prefix.
    pub fn record(&mut self, scalars: &[f64], words: usize, put: impl FnOnce(&mut [f64], &mut Vec<f64>)) {
        self.words.clear(); self.words.extend_from_slice(scalars);
        self.words.resize(scalars.len() + words, 0.); self.heap.clear();
        put(&mut self.words[scalars.len()..], &mut self.heap);
        self.words.extend_from_slice(&self.heap);
    }

    pub fn run(&mut self, context: &Context, op: u32) -> Result<(), String> {
        context.run_operation_into(op, &self.words, &mut self.answer).map_err(|fault| match fault {
            Fault::Context(error) => error.to_string(),
            other => format!("native session operation refused its arguments: {other:?}"),
        })
    }

    pub fn row(&self, words: usize) -> Result<&[f64], &'static str> {
        use gen::BlockHeader as H;
        let block = &self.answer.words;
        if block.len() < H::WORDS || block[H::ROWS] != 1. || block[H::STRIDE] != words as f64 {
            return Err("native session returned an invalid result block");
        }
        block.get(H::WORDS..H::WORDS + words).ok_or("native session returned a short result row")
    }
}

// ImResult's serializer owns its layout but exposes no field constants.
// Project only its accepted flag once at admission: using take() each tick
// would allocate vectors for the retained mailbox view. A changed or ambiguous
// generated representation refuses at admission, before any frame can run.
pub fn mailbox_accepted() -> Result<usize, String> {
    let mut value = gen::ImResult::default(); let mut heap = Vec::new();
    let mut before = [0.; gen::ImResult::WORDS]; let mut after = before;
    value.put(&mut before, &mut heap); value.accepted = true; heap.clear();
    value.put(&mut after, &mut heap);
    let mut fields = before.iter().zip(after).enumerate().filter(|(_, (a, b))| **a != *b);
    let field = fields.next().map(|(at, _)| at).ok_or("missing generated mailbox receipt flag")?;
    if fields.next().is_some() || before[field] != 0. || after[field] != 1. {
        return Err("ambiguous generated mailbox receipt flag".into());
    }
    Ok(field)
}

// Resolve by generated name at admission. Integer ids are table positions,
// never an independently maintained list in the client. A missing operation
// is a hard refusal before gameplay, not a compatible alternate input path.
pub fn required(name: &str) -> Result<u32, String> {
    gen::OPERATION_NAMES.iter().position(|candidate| *candidate == name).filter(|id| *id != 0)
        .map(|id| id as u32).ok_or_else(|| format!("native world API unavailable: {name}"))
}
