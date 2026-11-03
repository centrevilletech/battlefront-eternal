//! Captures, their frames and the corpus.
//!
//! A frame is a phase of a tick (or a method of a live object) as a capture
//! recorded it: its arguments and state at the start, the calls it made
//! across its boundary (arguments before, result and arguments after, the
//! draws made inside each) and the draws it made itself, in order, and its
//! state and result at the end. A replay runs the Rust port of that frame
//! from its start state, serves each boundary call from the capture while
//! checking its arguments (`Log`), and compares the end state.

use crate::format::{FormatError, Kind, Reader, Record};
use crate::value::V;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// The V8 the port's results must match: the production server's (Node
/// 24.22, V8 13.6). Captures made under another V8 are refused (their Math
/// functions differ in the last bit), unless BFE_REPLAY_ANY_V8 is set.
pub const REFERENCE_V8: &str = "13.6.";

/// Something wrong with a capture or a corpus.
#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<FormatError> for Error {
    fn from(e: FormatError) -> Error {
        Error(e.to_string())
    }
}

/// A random draw: its stream ("A", "B", "stream:gameplay", "Math.random",
/// "seeded:SEED", "anon:N") and the number drawn (a ClassicRandom's raw
/// 32-bit draw, or the [0, 1) number of Math.random and a press's spread).
#[derive(Clone, Debug, PartialEq)]
pub struct Draw {
    pub stream: String,
    pub value: f64,
}

/// A call across a frame's boundary.
#[derive(Clone, Debug)]
pub struct Call {
    /// The boundary's label (tools/sim-capture/specs.mjs), e.g. "units.raycast".
    pub name: String,
    /// The record made as the call began (its arguments before the call).
    pub call: Record,
    /// The record made as it returned (its result, its object arguments after, extra state).
    pub ret: Record,
    /// The draws made inside the call (by code the replay does not run).
    pub inner: Vec<Draw>,
}

impl Call {
    /// Its arguments as they were passed.
    pub fn args(&self) -> V<'_> {
        self.call.root().get("a")
    }
    pub fn arg(&self, i: usize) -> V<'_> {
        self.args().at(i)
    }
    /// What it returned (`left out: threw` when it threw).
    pub fn result(&self) -> V<'_> {
        self.ret.root().get("r")
    }
    /// Its i-th argument as the call left it (objects only: an out parameter's result).
    pub fn out(&self, i: usize) -> V<'_> {
        self.ret.root().get("o").at(i)
    }
    /// State of the frame's own the call changed, as the spec named it.
    pub fn extra(&self) -> V<'_> {
        self.ret.root().get("x")
    }
}

/// What a frame did, in order.
#[derive(Clone, Debug)]
pub enum Event {
    Draw(Draw),
    Call(Call),
}

/// A frame of a capture.
#[derive(Clone, Debug)]
pub struct Frame {
    pub id: u32,
    pub parent: Option<u32>,
    pub tick: u32,
    pub name: String,
    pub enter: Record,
    pub events: Vec<Event>,
    pub exit: Record,
    /// The units as the frame found them (their lives, teams and classes), if its spec asked.
    pub world: Option<Rc<Record>>,
}

impl Frame {
    /// The arguments a method frame was called with (none for a phase).
    pub fn args(&self) -> V<'_> {
        self.enter.root().get("a")
    }
    /// The object a method frame was called on (a handle for a hull or a
    /// unit; undefined for a phase).
    pub fn receiver(&self) -> V<'_> {
        self.enter.root().get("t")
    }
    pub fn arg(&self, i: usize) -> V<'_> {
        self.args().at(i)
    }
    /// The state it started from.
    pub fn before(&self) -> V<'_> {
        self.enter.root().get("s")
    }
    /// The state it ended with.
    pub fn after(&self) -> V<'_> {
        self.exit.root().get("s")
    }
    /// What it returned.
    pub fn result(&self) -> V<'_> {
        self.exit.root().get("r")
    }
    pub fn calls(&self) -> impl Iterator<Item = &Call> {
        self.events.iter().filter_map(|e| if let Event::Call(c) = e { Some(c) } else { None })
    }
    /// The draws the frame's own code made, in order.
    pub fn draws(&self) -> impl Iterator<Item = &Draw> {
        self.events.iter().filter_map(|e| if let Event::Draw(d) = e { Some(d) } else { None })
    }
    /// A unit (any member of the battle's registry) as the frame found it, by id.
    pub fn unit(&self, id: f64) -> Option<V<'_>> {
        self.world.as_ref()?.root().get("units").items().find(|u| u.get("id").opt_num() == Some(id))
    }
    /// Where it is, for messages: "tick 1267 ordnance.stepBolt #361".
    pub fn place(&self) -> String {
        format!("tick {} {} #{}", self.tick, self.name, self.id)
    }
}

fn draws_of(v: V) -> Result<Vec<Draw>, Error> {
    v.items()
        .map(|d| match (d.at(0).str(), d.at(1).opt_num()) {
            (Some(stream), Some(value)) => Ok(Draw { stream: stream.to_string(), value }),
            _ => Err(Error(format!("a draw that is {}", d.show()))),
        })
        .collect()
}

/// A capture file: its header, its definitions, and its frames on demand.
pub struct Capture {
    pub path: PathBuf,
    bytes: Vec<u8>,
    pub header: Record,
    defs: HashMap<String, Record>,
}

impl Capture {
    /// Opens a capture made under the reference V8.
    pub fn open(path: &Path) -> Result<Capture, Error> {
        let capture = Capture::open_any_v8(path)?;
        let v8 = capture.v8();
        if !v8.starts_with(REFERENCE_V8) && std::env::var_os("BFE_REPLAY_ANY_V8").is_none() {
            return Err(Error(format!(
                "{}: made under V8 {v8}, not the reference {REFERENCE_V8}x (Node 24.22): its Math functions differ; capture again under it (or set BFE_REPLAY_ANY_V8 to compare anyway)",
                path.display()
            )));
        }
        Ok(capture)
    }

    /// Opens a capture whatever V8 made it.
    pub fn open_any_v8(path: &Path) -> Result<Capture, Error> {
        let bytes = std::fs::read(path).map_err(|e| Error(format!("{}: {e}", path.display())))?;
        let mut reader = Reader::new(&bytes).map_err(|e| Error(format!("{}: {e}", path.display())))?;
        let header = match reader.next_record() {
            Some(Ok(r)) if r.kind == Kind::Header => r,
            Some(Err(e)) => return Err(Error(format!("{}: {e}", path.display()))),
            _ => return Err(Error(format!("{}: no header", path.display()))),
        };
        if header.root().get("format").opt_num() != Some(1.0) {
            return Err(Error(format!("{}: format {} (this reads 1)", path.display(), header.root().get("format").show())));
        }
        // The definitions, whole, by key (the frames name them).
        let mut defs = HashMap::new();
        while let Some(prelude) = reader.next_prelude() {
            let prelude = prelude?;
            if prelude.kind == Kind::Def {
                let record = reader.value(&prelude)?;
                defs.insert(record.name.clone(), record);
            }
        }
        Ok(Capture { path: path.to_path_buf(), bytes, header, defs })
    }

    /// The V8 that made it.
    pub fn v8(&self) -> &str {
        self.header.root().get("v8").str().unwrap_or("?")
    }
    /// Its name in the corpus (the file's name without .bfecap).
    pub fn label(&self) -> String {
        self.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
    }
    /// The battle it is of: map, side, seed, ticks, ...
    pub fn battle(&self) -> V<'_> {
        self.header.root().get("battle")
    }
    /// A content definition by the key a handle names ("def" handles).
    pub fn def(&self, key: &str) -> Option<V<'_>> {
        self.defs.get(key).map(|r| r.root())
    }
    /// The definition a "def" handle names.
    pub fn def_of(&self, handle: V) -> Option<V<'_>> {
        self.def(handle.handle_str("def")?)
    }

    /// Its frames of this name (a phase's JavaScript name or a method's label), in the order they ended.
    pub fn frames<'c>(&'c self, name: &'c str) -> Frames<'c> {
        Frames { reader: Reader::new(&self.bytes).unwrap(), name, open: HashMap::new(), world: None, pending: HashMap::new(), done: false }
    }

    /// Its tick records: every tick's fingerprints and its draws by phase and stream.
    pub fn ticks(&self) -> impl Iterator<Item = Result<Record, Error>> + '_ {
        let mut reader = Reader::new(&self.bytes).unwrap();
        std::iter::from_fn(move || loop {
            match reader.next_prelude()? {
                Err(e) => return Some(Err(e.into())),
                Ok(p) if p.kind == Kind::Tick => return Some(reader.value(&p).map_err(Error::from)),
                Ok(_) => continue,
            }
        })
    }
}

struct Building {
    parent: Option<u32>,
    enter: Record,
    events: Vec<Event>,
    world: Option<Rc<Record>>,
}

/// A capture's frames of one name, assembled as their records go by.
pub struct Frames<'c> {
    reader: Reader<'c>,
    name: &'c str,
    open: HashMap<u32, Building>,
    world: Option<Rc<Record>>,
    // A call's record, waiting for its return.
    pending: HashMap<u32, Record>,
    done: bool,
}

impl<'c> Frames<'c> {
    fn step(&mut self) -> Result<Option<Frame>, Error> {
        while let Some(prelude) = self.reader.next_prelude() {
            let prelude = prelude?;
            match prelude.kind {
                Kind::World => self.world = Some(Rc::new(self.reader.value(&prelude)?)),
                Kind::Frame if prelude.name == self.name => {
                    let record = self.reader.value(&prelude)?;
                    let root = record.root();
                    let id = root.get("f").opt_num().ok_or_else(|| Error(format!("a frame record without its frame: {}", root.show())))? as u32;
                    match root.get("k").str() {
                        Some("enter") => {
                            let parent = root.get("p").opt_num().map(|p| p as u32);
                            self.open.insert(id, Building { parent, enter: record, events: Vec::new(), world: self.world.clone() });
                        }
                        Some("call") => {
                            let frame = self.open.get_mut(&id).ok_or_else(|| Error(format!("a call in frame {id}, not open")))?;
                            frame.events.extend(draws_of(root.get("d"))?.into_iter().map(Event::Draw));
                            self.pending.insert(id, record);
                        }
                        Some("ret") => {
                            let call = self.pending.remove(&id).ok_or_else(|| Error(format!("a return in frame {id} without its call")))?;
                            let name = root.get("n").str().unwrap_or("?").to_string();
                            if call.root().get("n").str() != Some(&name) {
                                return Err(Error(format!("frame {id}: the return from {name} answers {}", call.root().get("n").show())));
                            }
                            let inner = draws_of(root.get("i"))?;
                            let frame = self.open.get_mut(&id).ok_or_else(|| Error(format!("a return in frame {id}, not open")))?;
                            frame.events.push(Event::Call(Call { name, call, ret: record, inner }));
                        }
                        Some("exit") => {
                            let mut frame = self.open.remove(&id).ok_or_else(|| Error(format!("the end of frame {id}, not open")))?;
                            frame.events.extend(draws_of(root.get("d"))?.into_iter().map(Event::Draw));
                            return Ok(Some(Frame {
                                id,
                                parent: frame.parent,
                                tick: prelude.tick,
                                name: prelude.name,
                                enter: frame.enter,
                                events: std::mem::take(&mut frame.events),
                                exit: record,
                                world: frame.world,
                            }));
                        }
                        _ => return Err(Error(format!("a frame record of kind {}", root.get("k").show()))),
                    }
                }
                _ => {}
            }
        }
        if !self.open.is_empty() {
            return Err(Error(format!("{} frames never ended (a capture cut short?)", self.open.len())));
        }
        Ok(None)
    }
}

impl<'c> Iterator for Frames<'c> {
    type Item = Result<Frame, Error>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        match self.step() {
            Ok(Some(f)) => Some(Ok(f)),
            Ok(None) => {
                self.done = true;
                None
            }
            Err(e) => {
                self.done = true;
                Some(Err(e))
            }
        }
    }
}

/// A directory of captures (tools/sim-capture.mjs --out DIR), all made under one V8.
pub struct Corpus {
    pub dir: PathBuf,
    pub files: Vec<PathBuf>,
}

impl Corpus {
    /// The corpus BFE_REPLAY_CORPUS names, or None when it names none
    /// (replay tests then pass without replaying: CI has no game data).
    pub fn from_env() -> Result<Option<Corpus>, Error> {
        match std::env::var_os("BFE_REPLAY_CORPUS") {
            None => Ok(None),
            Some(dir) if dir.is_empty() => Ok(None),
            Some(dir) => Corpus::open(Path::new(&dir)).map(Some),
        }
    }

    pub fn open(dir: &Path) -> Result<Corpus, Error> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(|e| Error(format!("{}: {e}", dir.display())))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "bfecap"))
            .collect();
        files.sort();
        if files.is_empty() {
            return Err(Error(format!("{}: no .bfecap captures", dir.display())));
        }
        Ok(Corpus { dir: dir.to_path_buf(), files })
    }

    /// Its captures, one at a time; the first decides the corpus's V8 and every other must match it.
    pub fn captures(&self) -> impl Iterator<Item = Result<Capture, Error>> + '_ {
        let mut v8: Option<String> = None;
        self.files.iter().map(move |path| {
            let capture = Capture::open(path)?;
            match &v8 {
                None => v8 = Some(capture.v8().to_string()),
                Some(first) if first != capture.v8() => {
                    return Err(Error(format!("{}: V8 {} in a corpus of V8 {first}", path.display(), capture.v8())));
                }
                _ => {}
            }
            Ok(capture)
        })
    }
}
