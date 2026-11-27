//! The 2005 game's prepared tree, `<content>/swbf2/.content`.
//!
//! This module decides where that tree physically is, whether it is a 2005 tree this engine
//! wrote, when its preparation record makes it usable, which of its files an export carries,
//! and how a preparation of it starts. A start takes the preparation tools' own lock
//! (`.prepare.lock` in the tree) before it looks inside the tree, keeps it while it checks,
//! marks and clears the tree, and hands it to the tools (`docs/desktop.md`, "Preparing the
//! 2005 game").
//!
//! Only "not found" counts as absent here. Any other failure of a lookup, read, removal or
//! write stops the operation where it is and is returned to the caller.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

use bfe_content::layout::{SWBF2_MARKER_BODY, SWBF2_MARKER_FILE, SWBF2_PREPARE_STATE_FILE};
use bfe_family::GameFamily;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::archive::{hardlink_key, safe_path, ContentFileRecord, MANIFEST_NAME, RECEIPT_NAME};
use crate::prepare::{
    checkout_content_version, parse_content_version, prepared_content_current,
    ContentVersion, PrepareState, CHECKOUT_CONTENT_VERSION_JSON,
};

/// The lock file the preparation tools take in the folder they write (`tools/prepare.py`).
pub const LOCK_FILE: &str = ".prepare.lock";

/// The tools' per-step locks inside the tree.
const STEP_LOCKS: &str = ".locks";

/// The environment variable telling the tools that their standard input is the lock on
/// their output folder (value [`LOCK_HANDOVER`]).
pub const LOCK_ENV: &str = "BFE_PREPARE_LOCK";

/// The value of [`LOCK_ENV`] for a lock passed as standard input.
pub const LOCK_HANDOVER: &str = "stdin";

/// `tools/content-version.json`: the 2005 output generation the tools write.
const GENERATION_KEY: &str = "swbf2";

/// `tools/content-version.json`: the lock protocol the tools follow for a 2005 run.
const HANDOVER_KEY: &str = "swbf2Lock";

/// `tools/content-version.json`: the format of the inventory the tools record (`outputs`).
const INVENTORY_KEY: &str = "swbf2Outputs";

/// The inventory format this build reads.
const INVENTORY_FORMAT: u32 = 1;

/// The 2004 game's record name. Early 2005 tools wrote their record under it too.
const OLD_RECORD: &str = ".prepare.json";

/// The tools' temporary record, renamed over the record when it is saved.
const RECORD_TEMP: &str = ".prepare-swbf2.tmp";

/// The tree's machinery at its top: never prepared content, never exported, refused in an
/// archive. Temporary marker files (`.bfe-family.json.*.tmp`) belong here too.
pub const BOOKKEEPING: &[&str] = &[
    LOCK_FILE,
    STEP_LOCKS,
    SWBF2_MARKER_FILE,
    RECEIPT_NAME,
    RECORD_TEMP,
    ".prepare.tmp",
];

/// Temporary files left by an interrupted marker publication.
fn is_marker_leftover(name: &str) -> bool {
    name.starts_with(".bfe-family.json.") && name.ends_with(".tmp")
}

/// Whether a top-level name is the tree's machinery, by name and without regard to case (an
/// archive entry that would land on one of them on a case-folding disk is refused too).
pub fn is_bookkeeping_name(name: &str) -> bool {
    BOOKKEEPING.iter().any(|b| b.eq_ignore_ascii_case(name)) || is_marker_leftover(&name.to_ascii_lowercase())
}

/// What a path is, without following a link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Dir,
    File,
    /// A symbolic link, or on Windows a junction or another name surrogate.
    Link,
    Other,
}

fn kind_of(file_type: fs::FileType) -> Kind {
    if file_type.is_symlink() {
        Kind::Link
    } else if file_type.is_dir() {
        Kind::Dir
    } else if file_type.is_file() {
        Kind::File
    } else {
        Kind::Other
    }
}

/// One filesystem operation of this module, as the tests' fault injection sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    Resolve,
    Stat,
    Read,
    List,
    MakeDir,
    Lock,
    Write,
    Rename,
    Remove,
}

/// Called before every operation; an error from it is that operation's result.
pub(crate) type Hook<'a> = &'a (dyn Fn(Op, &Path) -> io::Result<()> + Sync);

/// The filesystem as this module uses it. Production code uses [`Ops::REAL`]; the tests
/// add a hook that fails or stops any single operation.
#[derive(Clone, Copy)]
pub(crate) struct Ops<'a> {
    hook: Option<Hook<'a>>,
}

impl Ops<'static> {
    pub(crate) const REAL: Self = Self { hook: None };
}

fn absent(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound
}

impl<'a> Ops<'a> {
    #[cfg(test)]
    pub(crate) fn with_hook(hook: Hook<'a>) -> Self {
        Self { hook: Some(hook) }
    }

    fn check(&self, op: Op, path: &Path) -> io::Result<()> {
        match self.hook {
            Some(hook) => hook(op, path),
            None => Ok(()),
        }
    }

    /// The kind and size of `path`, or `None` when nothing is there.
    fn stat(&self, path: &Path) -> io::Result<Option<(Kind, u64)>> {
        self.check(Op::Stat, path)?;
        match fs::symlink_metadata(path) {
            Ok(meta) => Ok(Some((kind_of(meta.file_type()), meta.len()))),
            Err(error) if absent(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn kind(&self, path: &Path) -> io::Result<Option<Kind>> {
        Ok(self.stat(path)?.map(|(kind, _)| kind))
    }

    fn read(&self, path: &Path) -> io::Result<Option<Vec<u8>>> {
        self.check(Op::Read, path)?;
        match fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if absent(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// The SHA-256 of a file, as lowercase hex.
    fn sha256(&self, path: &Path) -> io::Result<String> {
        self.check(Op::Read, path)?;
        let mut file = File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buf = [0u8; 65536];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Ok(hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect())
    }

    /// The entries of a folder with their kinds, in the order the folder lists them.
    fn list_raw(&self, path: &Path) -> io::Result<Vec<(OsString, Kind)>> {
        self.check(Op::List, path)?;
        let mut entries = Vec::new();
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            entries.push((entry.file_name(), kind_of(entry.file_type()?)));
        }
        Ok(entries)
    }

    /// The entries of a folder with their kinds, sorted by name.
    fn list(&self, path: &Path) -> io::Result<Vec<(OsString, Kind)>> {
        let mut entries = self.list_raw(path)?;
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(entries)
    }

    /// The path with every link and spelling resolved, or `None` when it does not exist.
    fn resolve(&self, path: &Path) -> io::Result<Option<PathBuf>> {
        self.check(Op::Resolve, path)?;
        match fs::canonicalize(path) {
            Ok(found) => Ok(Some(simplify(found))),
            Err(error) if absent(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// The identity of what `path` names (links followed), or `None` when nothing is there.
    fn identity(&self, path: &Path) -> io::Result<Option<same_file::Handle>> {
        self.check(Op::Stat, path)?;
        match same_file::Handle::from_path(path) {
            Ok(handle) => Ok(Some(handle)),
            Err(error) if absent(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Makes one folder; an existing plain folder is fine, anything else is an error.
    fn make_dir(&self, path: &Path) -> io::Result<()> {
        self.check(Op::MakeDir, path)?;
        match fs::create_dir(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                match fs::symlink_metadata(path) {
                    Ok(meta) if kind_of(meta.file_type()) == Kind::Dir => Ok(()),
                    Ok(_) => Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "something other than a plain folder is there",
                    )),
                    Err(error) => Err(error),
                }
            }
            Err(error) => Err(error),
        }
    }

    /// Opens an existing lock file without changing it.
    fn open_lock(&self, path: &Path) -> io::Result<File> {
        self.check(Op::Lock, path)?;
        fs::OpenOptions::new().read(true).append(true).open(path)
    }

    /// Opens a lock file, making it when missing (never truncating), as the tools do.
    fn create_lock(&self, path: &Path) -> io::Result<File> {
        self.check(Op::Lock, path)?;
        fs::OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(path)
    }

    /// Writes a new file (never an existing one) and flushes the file's bytes to the disk.
    /// Making the file and filling it are separate steps, as an interruption can come between
    /// them.
    fn write_new(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.check(Op::Write, path)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        self.check(Op::Write, path)?;
        file.write_all(bytes)?;
        file.sync_all()
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.check(Op::Rename, to)?;
        fs::rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.check(Op::Remove, path)?;
        match fs::remove_file(path) {
            Err(error) if !absent(&error) => Err(error),
            _ => Ok(()),
        }
    }

    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        self.check(Op::Remove, path)?;
        match fs::remove_dir(path) {
            Err(error) if !absent(&error) => Err(error),
            _ => Ok(()),
        }
    }

    /// Whether the open `file` is the file now at `path`.
    fn same_file(&self, file: &File, path: &Path) -> io::Result<bool> {
        self.check(Op::Stat, path)?;
        let held = same_file::Handle::from_file(file.try_clone()?)?;
        let named = same_file::Handle::from_path(path)?;
        Ok(held == named)
    }
}

/// `canonicalize` on Windows answers with a `\\?\` path, which some programs the tools run
/// do not accept. A drive or share path whose every name is an ordinary one means the same
/// place without that prefix.
#[cfg(windows)]
fn simplify(path: PathBuf) -> PathBuf {
    use std::ffi::OsStr;
    use std::path::Prefix;
    let ordinary = |name: &OsStr| {
        let name = name.to_string_lossy();
        let stem = name.split('.').next().unwrap_or("").to_ascii_lowercase();
        !name.ends_with('.')
            && !name.ends_with(' ')
            && !matches!(stem.as_str(), "con" | "prn" | "aux" | "nul" | "conin$" | "conout$")
            && !((stem.starts_with("com") || stem.starts_with("lpt")) && stem.len() == 4)
    };
    let mut components = path.components();
    let start = match components.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::VerbatimDisk(letter) => format!("{}:\\", letter as char),
            Prefix::VerbatimUNC(server, share) => format!(
                "\\\\{}\\{}\\",
                server.to_string_lossy(),
                share.to_string_lossy()
            ),
            _ => return path,
        },
        _ => return path,
    };
    let mut out = PathBuf::from(start);
    for component in components {
        match component {
            Component::RootDir => {}
            Component::Normal(name) if ordinary(name) => out.push(name),
            _ => return path,
        }
    }
    if out.as_os_str().len() >= 260 {
        return path;
    }
    out
}

#[cfg(not(windows))]
fn simplify(path: PathBuf) -> PathBuf {
    path
}

#[cfg(test)]
mod tests;

/// What stopped a check: a lookup, read or write that failed, or something found that the
/// operation must not touch.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Problem {
    Io(String),
    PayloadIo(String),
    Found(String),
}

type Checked<T> = Result<T, Problem>;

impl Problem {
    fn message(self) -> String {
        match self {
            Problem::Io(message) | Problem::PayloadIo(message) | Problem::Found(message) => message,
        }
    }
}

fn io_failure<'p>(what: &'p str, path: &'p Path) -> impl Fn(io::Error) -> Problem + 'p {
    move |error| Problem::Io(format!("Could not {what} {}: {error}", path.display()))
}

/// What the preparation tools a build ships with say about the 2005 game
/// (`tools/content-version.json`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Swbf2Tools {
    /// The versions the 2004 and 2005 records share.
    pub content: ContentVersion,
    /// The 2005 output generation the tools write (`swbf2`).
    pub generation: u32,
    /// Whether the tools take over the client's lock on a 2005 run (`swbf2Lock: "stdin"`).
    pub handover: bool,
    /// Whether the tools record the inventory of what each step wrote in the format this
    /// build reads (`swbf2Outputs: 1`).
    pub inventory: bool,
}

/// Reads [`Swbf2Tools`] from `<tools_dir>/content-version.json`, or from the checkout's copy.
/// Tools that publish no 2005 generation cannot prepare the 2005 game.
pub fn swbf2_tools(tools_dir: Option<&Path>) -> Result<Swbf2Tools, String> {
    let (raw, label) = match tools_dir {
        Some(dir) => {
            let path = dir.join("content-version.json");
            let raw = fs::read_to_string(&path)
                .map_err(|e| format!("Could not read {}: {e}", path.display()))?;
            (raw, path.display().to_string())
        }
        None => (
            CHECKOUT_CONTENT_VERSION_JSON.to_string(),
            "tools/content-version.json".to_string(),
        ),
    };
    let content = match tools_dir {
        Some(_) => parse_content_version(&raw, &label)?,
        None => checkout_content_version()?,
    };
    let value: Value =
        serde_json::from_str(&raw).map_err(|e| format!("Could not read {label}: {e}"))?;
    let generation = value
        .get(GENERATION_KEY)
        .and_then(Value::as_u64)
        .and_then(|g| u32::try_from(g).ok())
        .filter(|g| *g > 0)
        .ok_or_else(|| "These preparation tools do not prepare the 2005 game.".to_string())?;
    let handover = value.get(HANDOVER_KEY).and_then(Value::as_str) == Some(LOCK_HANDOVER);
    let inventory = value.get(INVENTORY_KEY).and_then(Value::as_u64).and_then(|f| u32::try_from(f).ok()) == Some(INVENTORY_FORMAT);
    Ok(Swbf2Tools {
        content,
        generation,
        handover,
        inventory,
    })
}

/// Where the 2005 tree of a content folder physically is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Swbf2Tree {
    /// The content folder, resolved through every link and spelling (a part that does not
    /// exist yet appended as named).
    pub root: PathBuf,
    /// `<root>/swbf2/.content`. Both levels are plain folders or not made yet.
    pub dir: PathBuf,
}

/// The content folder with every link and spelling resolved. A folder that does not exist
/// yet resolves through its nearest existing parent. `..` is resolved physically, never
/// against the spelling.
fn physical(ops: &Ops, path: &Path) -> Checked<PathBuf> {
    let unusable = || Problem::Found(format!("{} cannot be used as a content folder.", path.display()));
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| Problem::Io(format!("Could not read the current folder: {e}")))?
            .join(path)
    };
    let mut missing: Vec<OsString> = Vec::new();
    let mut at = absolute.as_path();
    loop {
        if let Some(found) = ops.resolve(at).map_err(io_failure("look up", at))? {
            let mut out = found;
            for name in missing.iter().rev() {
                out.push(name);
            }
            return Ok(out);
        }
        match at.components().next_back() {
            Some(Component::Normal(name)) => missing.push(name.to_os_string()),
            _ => return Err(unusable()),
        }
        at = at.parent().ok_or_else(unusable)?;
    }
}

fn marker_says_swbf2(bytes: &[u8]) -> bool {
    serde_json::from_slice::<Value>(bytes).is_ok_and(|value| {
        value.get("family").and_then(Value::as_str) == Some(GameFamily::Swbf2.as_str())
    })
}

fn exists(ops: &Ops, path: &Path) -> Checked<bool> {
    Ok(ops.kind(path).map_err(io_failure("look up", path))?.is_some())
}

/// Whether `dir` holds a prepared tree of either game: a preparation record, the shared
/// `common/common.json`, or the 2005 marker.
fn holds_prepared_tree(ops: &Ops, dir: &Path) -> Checked<bool> {
    Ok(exists(ops, &dir.join(OLD_RECORD))?
        || exists(ops, &dir.join(SWBF2_PREPARE_STATE_FILE))?
        || exists(ops, &dir.join("common").join("common.json"))?
        || exists(ops, &dir.join(SWBF2_MARKER_FILE))?)
}

/// Finds the 2005 tree of `content_root` without changing anything, and refuses a place
/// where 2005 files would touch other data: a content folder inside a prepared tree or a
/// 2005 tree itself, a `swbf2` folder that holds prepared content of its own (a 2004 copy
/// of that name), a `swbf2` or `.content` that is a link or not a folder, and a path the
/// preparation tools could not receive exactly (not valid Unicode).
pub fn locate(content_root: &Path) -> Result<Swbf2Tree, String> {
    locate_with(&Ops::REAL, content_root).map_err(Problem::message)
}

fn locate_with(ops: &Ops, content_root: &Path) -> Checked<Swbf2Tree> {
    if content_root.as_os_str().is_empty() {
        return Err(Problem::Found("No content folder was given.".to_string()));
    }
    if content_root.to_str().is_none() {
        return Err(Problem::Found(format!(
            "{} cannot be passed to the preparation tools as it is spelled. Choose a content folder whose path is valid Unicode.",
            content_root.display()
        )));
    }
    let root = physical(ops, content_root)?;
    match ops.kind(&root).map_err(io_failure("look up", &root))? {
        None | Some(Kind::Dir) => {}
        Some(_) => return Err(Problem::Found(format!("{} is not a folder.", root.display()))),
    }
    for ancestor in root.ancestors().skip(1) {
        if holds_prepared_tree(ops, ancestor)? {
            return Err(Problem::Found(format!(
                "{} is inside the prepared game content in {}. Choose a content folder outside it.",
                root.display(),
                ancestor.display()
            )));
        }
    }
    if bfe_content::layout::is_swbf2_marked_dir(&root) {
        return Err(Problem::Found(format!(
            "{} is a 2005 game data folder itself. Choose the content folder that holds it.",
            root.display()
        )));
    }
    let container = root.join(GameFamily::Swbf2.as_str());
    match ops.kind(&container).map_err(io_failure("look up", &container))? {
        None => {}
        Some(Kind::Dir) => {
            let mut own = exists(ops, &container.join(OLD_RECORD))?
                || exists(ops, &container.join(SWBF2_PREPARE_STATE_FILE))?
                || exists(ops, &container.join("common").join("common.json"))?;
            for name in ["maps", "sides", "common", "audio"] {
                own = own || exists(ops, &container.join(name))?;
            }
            if own {
                return Err(Problem::Found(format!(
                    "{} holds prepared game content of its own, so the 2005 game's data cannot go inside it. Move that folder or choose another content folder.",
                    container.display()
                )));
            }
        }
        Some(_) => {
            return Err(Problem::Found(format!(
                "{} is a link or a file. The 2005 game's data needs a plain folder there.",
                container.display()
            )))
        }
    }
    let dir = container.join(".content");
    match ops.kind(&dir).map_err(io_failure("look up", &dir))? {
        None | Some(Kind::Dir) => {}
        Some(_) => {
            return Err(Problem::Found(format!(
                "{} is a link or a file. The 2005 game's data needs a plain folder there.",
                dir.display()
            )))
        }
    }
    if dir.to_str().is_none() {
        return Err(Problem::Found(format!(
            "{} cannot be passed to the preparation tools as it is spelled. Choose a content folder whose path is valid Unicode.",
            dir.display()
        )));
    }
    Ok(Swbf2Tree { root, dir })
}

/// The 2005 copy a preparation reads, identified once: its `_lvl_pc` folder, found the way
/// the preparation tools find it (each name exactly when it is there, otherwise the earliest
/// listed entry of that name in another case).
#[derive(Debug)]
pub struct Swbf2Install {
    /// The absolute installation root supplied to the tools.
    pub root: PathBuf,
    /// The physical `_lvl_pc` folder.
    pub lvl_pc: PathBuf,
    handle: same_file::Handle,
}

/// The entry of `base` named `name`: exactly, or else the earliest listed in another case, in
/// the order the folder lists them. A missing one is `Found`; a failed listing is `Io`.
fn tools_child(ops: &Ops, base: &Path, name: &str) -> Checked<PathBuf> {
    let entries = ops.list_raw(base).map_err(io_failure("read", base))?;
    if entries.iter().any(|(entry, _)| entry == name) {
        return Ok(base.join(name));
    }
    entries
        .iter()
        .find(|(entry, _)| entry.to_string_lossy().eq_ignore_ascii_case(name))
        .map(|(entry, _)| base.join(entry))
        .ok_or_else(|| {
            Problem::Found(format!(
                "{} has no {name}: it is not a copy of the 2005 game.",
                base.display()
            ))
        })
}

/// Identifies the copy of the 2005 game at `install_dir` (the folder holding `GameData`).
pub fn identify_install(install_dir: &Path) -> Result<Swbf2Install, String> {
    identify_install_with(&Ops::REAL, install_dir).map_err(Problem::message)
}

fn identify_install_with(ops: &Ops, install_dir: &Path) -> Checked<Swbf2Install> {
    let base = ops
        .resolve(install_dir)
        .map_err(io_failure("look up", install_dir))?
        .ok_or_else(|| Problem::Found(format!("{} does not exist.", install_dir.display())))?;
    let require = |path: PathBuf, expected| -> Checked<PathBuf> {
        let found = ops.resolve(&path).map_err(io_failure("look up", &path))?
            .ok_or_else(|| Problem::Found(format!("{} is missing from the 2005 installation.", path.display())))?;
        if ops.kind(&found).map_err(io_failure("look up", &found))? != Some(expected) {
            return Err(Problem::Found(format!("{} is not a valid entry of a 2005 installation.", path.display())));
        }
        Ok(found)
    };
    let data = tools_child(ops, &base, "GameData")?;
    require(tools_child(ops, &data, "BattlefrontII.exe")?, Kind::File)?;
    let at = tools_child(ops, &data, "data")?;
    let lvl_pc = require(tools_child(ops, &at, "_lvl_pc")?, Kind::Dir)?;
    require(tools_child(ops, &lvl_pc, "side")?, Kind::Dir)?;
    for name in ["common.lvl", "core.lvl", "mission.lvl", "ingame.lvl", "shell.lvl"] {
        require(tools_child(ops, &lvl_pc, name)?, Kind::File)?;
    }
    for path in [&base, &lvl_pc] {
        if path.to_str().is_none() {
            return Err(Problem::Found(format!("{} cannot be passed to the preparation tools as it is spelled.", path.display())));
        }
    }
    let handle = ops
        .identity(&lvl_pc)
        .map_err(io_failure("look up", &lvl_pc))?
        .ok_or_else(|| Problem::Found(format!("{} does not exist.", lvl_pc.display())))?;
    Ok(Swbf2Install { root: base, lvl_pc, handle })
}

/// The recorded source can come from another desktop platform when a tree is imported.
fn absolute_game(game: &str) -> bool {
    let bytes = game.as_bytes();
    game.starts_with('/')
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
        || game.strip_prefix(r"\\").is_some_and(|rest| {
            let mut parts = rest.split(['/', '\\']);
            parts.next().is_some_and(|s| !s.is_empty()) && parts.next().is_some_and(|s| !s.is_empty())
        })
}

/// Whether the folder a record names is this copy. A record naming a folder that is not
/// there is from another copy; a failed lookup is an error.
fn names_install(ops: &Ops, game: &str, install: &Swbf2Install) -> Checked<bool> {
    let path = Path::new(game);
    if !path.is_absolute() {
        return Ok(false);
    }
    Ok(match ops.identity(path).map_err(io_failure("look up", path))? {
        Some(handle) => handle == install.handle,
        None => false,
    })
}

#[derive(Debug, PartialEq, Eq)]
enum Origin {
    Fresh,
    Early,
    Dedicated {
        fields: serde_json::Map<String, Value>,
        generation: u32,
    },
}

fn unconfirmed(dir: &Path) -> Problem {
    Problem::Found(format!(
        "{} contains files that cannot be identified as this engine's 2005 preparation. Nothing was changed.",
        dir.display()
    ))
}

fn read_object(ops: &Ops, path: &Path) -> Checked<Option<Value>> {
    Ok(ops
        .read(path)
        .map_err(io_failure("read", path))?
        .and_then(|bytes| serde_json::from_slice(&bytes).ok()))
}

/// Keep looking for foreign content in readable siblings. A failed read still prevents
/// writing, even when the receipt and every readable product identify the 2005 game.
#[derive(Default)]
struct PayloadCheck {
    read_error: Option<String>,
}

impl PayloadCheck {
    fn read<T: Default>(&mut self, result: Checked<T>) -> Checked<T> {
        match result {
            Err(Problem::Io(error)) => {
                self.read_error.get_or_insert(error);
                Ok(T::default())
            }
            other => other,
        }
    }
}

/// Check the map and side layout and look for recognizable products of the released writer.
/// Damaged or missing 2005 JSON can be regenerated from an otherwise owned tree.
fn contradicts_2005_layout(ops: &Ops, dir: &Path, check: &mut PayloadCheck) -> Checked<bool> {
    if check.read(exists(ops, &dir.join("common/shell")))? {
        return Ok(true);
    }
    let foreign = |path: &Path, signature: &[&str]| -> Checked<bool> {
        let Some(value) = read_object(ops, path)? else {
            return Ok(false);
        };
        Ok(match value.get("family") {
            Some(family) => family.as_str() != Some(GameFamily::Swbf2.as_str()),
            None => signature.iter().all(|key| value.get(*key).is_some()),
        })
    };
    if check.read(foreign(&dir.join("common/common.json"), &["holograms", "rasterHud"]))? {
        return Ok(true);
    }
    for (folder, record, signature) in [
        ("maps", "map.json", ["world", "missions"]),
        ("sides", "units.json", ["firstPersonBones", "wantedEffects"]),
    ] {
        let base = dir.join(folder);
        if check.read(exists(ops, &base))? {
            for (name, kind) in check.read(ops.list(&base).map_err(io_failure("list", &base)))? {
                if kind != Kind::Dir || check.read(foreign(&base.join(name).join(record), &signature))? {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

/// The permission to write a preparation destination. A missing or empty directory may
/// be initialized. Every other directory needs a supported receipt and consistent payload.
/// A caller without a tools version can only initialize a new directory.
fn write_origin(
    ops: &Ops, dir: &Path, tools: Option<&Swbf2Tools>, handover: Option<&TreeLock>,
) -> Checked<Origin> {
    match ops.kind(dir).map_err(io_failure("look up", dir))? {
        None => return Ok(Origin::Fresh),
        Some(Kind::Dir) => {}
        _ => return Err(unconfirmed(dir)),
    }
    let entries = ops.list(dir).map_err(io_failure("list", dir))?;
    if entries.is_empty() {
        return Ok(Origin::Fresh);
    }
    // Older clients hand over a new interior with only their canonical marker and the
    // locked file used as the child's stdin. An ordinary start has no such handover.
    if let Some(stdin) = handover {
        let lock = dir.join(LOCK_FILE);
        if entries.len() == 2
            && entries.iter().all(|(name, kind)| *kind == Kind::File
                && (name == LOCK_FILE || name == SWBF2_MARKER_FILE))
            && ops.same_file(stdin.file(), &lock).map_err(io_failure("identify", &lock))?
            && read_object(ops, &dir.join(SWBF2_MARKER_FILE))?
                == Some(serde_json::json!({"family": GameFamily::Swbf2.as_str(), "format": 1}))
        {
            return Ok(Origin::Fresh);
        }
    }
    let tools = tools.ok_or_else(|| unconfirmed(dir))?;
    let mut origin = None;
    for name in [OLD_RECORD, SWBF2_PREPARE_STATE_FILE] {
        let path = dir.join(name);
        match ops.kind(&path).map_err(io_failure("look up", &path))? {
            None => continue,
            Some(Kind::File) => {}
            _ => return Err(unconfirmed(dir)),
        }
        let Some(Value::Object(fields)) = read_object(ops, &path)? else {
            return Err(unconfirmed(dir));
        };
        let generation = fields
            .get("swbf2Version")
            .and_then(Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .filter(|v| (1..=tools.generation).contains(v))
            .ok_or_else(|| unconfirmed(dir))?;
        if fields.get("family").and_then(Value::as_str) != Some(GameFamily::Swbf2.as_str()) {
            return Err(unconfirmed(dir));
        }
        if name == OLD_RECORD {
            // For write protection only, optional metadata cannot invalidate the released
            // version and completion fields. Discovery still reads the entire typed record.
            let current_fields = [
                "version", "mapImportVersion", "muzzleVersion", "commonVersion",
                "complete", "failed", "skipped",
            ].into_iter()
                .filter_map(|key| fields.get(key).map(|value| (key.to_owned(), value.clone())))
                .collect();
            let state = serde_json::from_value::<PrepareState>(Value::Object(current_fields)).ok();
            let released = checkout_content_version().map_err(Problem::Found)?;
            if prepared_content_current(state.as_ref(), &released)
                || fields.get("version").and_then(Value::as_u64) != Some(u64::from(tools.content.state))
                || fields.get("complete").and_then(Value::as_bool).is_none()
                || !fields
                    .get("game")
                    .and_then(Value::as_str)
                    .is_some_and(absolute_game)
                || strings(fields.get("done")).is_none()
            {
                return Err(unconfirmed(dir));
            }
            origin = Some(Origin::Early);
        } else if origin.is_none() {
            origin = Some(Origin::Dedicated { fields, generation });
        }
    }
    let origin = origin.ok_or_else(|| unconfirmed(dir))?;
    let marker = dir.join(SWBF2_MARKER_FILE);
    if !matches!(
        ops.kind(&marker).map_err(io_failure("look up", &marker))?,
        None | Some(Kind::File)
    ) {
        return Err(unconfirmed(dir));
    }
    if let Some(value) = read_object(ops, &marker)? {
        if value
            .get("family")
            .is_some_and(|f| f.as_str() != Some(GameFamily::Swbf2.as_str()))
        {
            return Err(unconfirmed(dir));
        }
    }
    let mut check = PayloadCheck::default();
    walk_plain(ops, dir, dir, &mut check)?;
    if contradicts_2005_layout(ops, dir, &mut check)? {
        return Err(unconfirmed(dir));
    }
    match check.read_error {
        Some(error) => Err(Problem::PayloadIo(error)),
        None => Ok(origin),
    }
}

fn producer_suffix(value: &str) -> bool {
    value.len() == 8 && value.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn walk_plain(ops: &Ops, root: &Path, dir: &Path, check: &mut PayloadCheck) -> Checked<()> {
    for (name, kind) in check.read(ops.list(dir).map_err(io_failure("read", dir)))? {
        let path = dir.join(&name);
        match kind {
            Kind::File => {}
            Kind::Dir => {
                // Interrupted extractors leave ordinary products under a private step lock.
                let workspace = dir == root.join(STEP_LOCKS)
                    && name.to_str().and_then(|n| n.strip_prefix("products-")).is_some_and(producer_suffix);
                let nested = if workspace {
                    check.read(exists(ops, &path.join(OLD_RECORD)))?
                        || check.read(exists(ops, &path.join(SWBF2_PREPARE_STATE_FILE)))?
                        || check.read(exists(ops, &path.join(SWBF2_MARKER_FILE)))?
                } else {
                    check.read(holds_prepared_tree(ops, &path))?
                };
                if nested {
                    return Err(Problem::Found(format!(
                        "{} holds another prepared tree. Move it before preparing the 2005 game here.",
                        path.display()
                    )));
                }
                walk_plain(ops, root, &path, check)?;
            }
            Kind::Link | Kind::Other => {
                return Err(Problem::Found(format!(
                    "{} is a link or a special file. The 2005 game's data folder may hold only plain files and folders.",
                    path.display()
                )))
            }
        }
    }
    Ok(())
}

/// What a 2005 tree is. Preparation, verification, export, import and relocation all decide
/// with [`inspect`], so they never disagree about a tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Swbf2Record {
    /// No record and no prepared files.
    Empty,
    /// Every planned step finished with nothing failed or skipped, and every file the record
    /// lists is there at its recorded size (and hash, where it records one): ready to play,
    /// verify, export and import.
    Complete,
    /// A preparation of this copy that the tools continue: one that has not finished, one of
    /// an older generation (the tools bring it up to date), one recorded before the tools kept
    /// an inventory, or one whose listed files are not all there. The tools rebuild every step
    /// whose files are missing or not listed.
    Resumable,
    /// Anything else (prepared files without a record, an unreadable or foreign record,
    /// other versions, a future generation or one that is not a positive whole number,
    /// another copy, a blank or relative game folder, an inventory that cannot be read or
    /// leaves out what an export needs, two records). Never usable; a preparation clears it
    /// before the tools run.
    Stale(String),
    /// Not a tree this build may own or change (a link inside, another prepared tree inside,
    /// a 2004 record, files with no 2005 record or marker). Never usable and never cleared.
    Foreign(String),
}

impl Swbf2Record {
    /// Why a tree is not [`Swbf2Record::Complete`], in a few words.
    pub fn reason(&self) -> &str {
        match self {
            Swbf2Record::Empty => "nothing is prepared",
            Swbf2Record::Complete => "complete",
            Swbf2Record::Resumable => "the preparation has not finished, or some of its files are missing",
            Swbf2Record::Stale(why) | Swbf2Record::Foreign(why) => why,
        }
    }
}

/// Inspects the 2005 tree `dir` (a [`Swbf2Tree::dir`]): its contents, then its record
/// against the tools and, when `install` is given, the copy it must come from. An absent
/// folder is [`Swbf2Record::Empty`]. Only a failed lookup or read is an error.
pub fn inspect(
    dir: &Path,
    tools: &Swbf2Tools,
    install: Option<&Swbf2Install>,
) -> Result<Swbf2Record, String> {
    inspect_with(&Ops::REAL, dir, tools, install).map_err(Problem::message)
}

fn inspect_with(
    ops: &Ops,
    dir: &Path,
    tools: &Swbf2Tools,
    install: Option<&Swbf2Install>,
) -> Checked<Swbf2Record> {
    match write_origin(ops, dir, Some(tools), None) {
        Ok(Origin::Fresh) => Ok(Swbf2Record::Empty),
        Ok(origin) => classify(ops, dir, tools, install, origin),
        Err(Problem::Found(why)) => Ok(Swbf2Record::Foreign(why)),
        Err(failure) => Err(failure),
    }
}

fn stale(why: impl Into<String>) -> Checked<Swbf2Record> {
    Ok(Swbf2Record::Stale(why.into()))
}

/// One file a record lists: where, how large, and its SHA-256 where the producer has one.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Listed {
    path: String,
    size: u64,
    sha256: Option<String>,
}

fn is_hex64(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The inventory a record carries: `outputs`, an object of step names (and `"*"` for the
/// shared files) to lists of `[path, size]` or `[path, size, sha256]`. A size may be 0: a file
/// the step wrote empty must be there and stay empty. `None` when it is not a well-formed
/// inventory.
fn read_outputs(value: &Value) -> Option<Vec<(String, Vec<Listed>)>> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for (step, files) in value.as_object()? {
        let mut listed = Vec::new();
        for entry in files.as_array()? {
            let parts = entry.as_array()?;
            let path = parts.first()?.as_str()?.to_string();
            let size = parts.get(1)?.as_u64()?;
            let sha256 = match parts.get(2) {
                None => None,
                Some(hash) => Some(hash.as_str().filter(|h| is_hex64(h))?.to_string()),
            };
            if parts.len() > 3 || !content_path_ok(&path) || !seen.insert(path.to_ascii_lowercase()) {
                return None;
            }
            listed.push(Listed { path, size, sha256 });
        }
        out.push((step.clone(), listed));
    }
    Some(out)
}

/// A path inside the tree that can be prepared content: safe, and not the tree's machinery.
fn content_path_ok(path: &str) -> bool {
    match safe_path(path, false) {
        Ok(parts) => {
            !is_bookkeeping_name(&parts[0])
                && !parts[0].eq_ignore_ascii_case(SWBF2_PREPARE_STATE_FILE)
                && !parts[0].eq_ignore_ascii_case(OLD_RECORD)
                && !(parts.len() == 1 && parts[0].eq_ignore_ascii_case(MANIFEST_NAME))
        }
        Err(_) => false,
    }
}

fn strings(value: Option<&Value>) -> Option<Vec<&str>> {
    value?.as_array()?.iter().map(Value::as_str).collect()
}

fn classify(
    ops: &Ops,
    dir: &Path,
    tools: &Swbf2Tools,
    install: Option<&Swbf2Install>,
    origin: Origin,
) -> Checked<Swbf2Record> {
    let (fields, generation) = match origin {
        Origin::Fresh => return Ok(Swbf2Record::Empty),
        Origin::Early => return stale("a record written by earlier tools"),
        Origin::Dedicated { fields, generation } => (fields, generation),
    };
    if fields.get("clearing") == Some(&Value::Bool(true)) {
        return stale("an interrupted removal");
    }
    for (key, want) in [
        ("version", tools.content.state),
        ("mapImportVersion", tools.content.maps),
        ("muzzleVersion", tools.content.sides),
        ("commonVersion", tools.content.common),
    ] {
        if fields.get(key).and_then(Value::as_u64) != Some(u64::from(want)) {
            return stale(format!("prepared by other tools ({key})"));
        }
    }
    let Some(game) = fields.get("game").and_then(Value::as_str) else {
        return stale("the record names no game folder");
    };
    if !absolute_game(game) {
        return stale("the record's game folder is blank or relative");
    }
    let Some(complete) = fields.get("complete").and_then(Value::as_bool) else {
        return stale("the record does not say whether it finished");
    };
    let Some(done) = strings(fields.get("done")) else {
        return stale("the record has no list of finished steps");
    };
    // The same typed shape every reader of the record parses.
    let Ok(state) = serde_json::from_value::<PrepareState>(Value::Object(fields.clone())) else {
        return stale("the record has the wrong shape");
    };
    if let Some(install) = install {
        if !names_install(ops, game, install)? {
            return stale("prepared from another copy of the game");
        }
    }
    // An older generation is brought up to date by the tools; a record from before the tools
    // kept an inventory has every step rebuilt that lists nothing.
    if generation < tools.generation {
        return Ok(Swbf2Record::Resumable);
    }
    let outputs = match fields.get("outputs") {
        None => return Ok(Swbf2Record::Resumable),
        Some(value) => match read_outputs(value) {
            Some(outputs) => outputs,
            None => return stale("the record's inventory cannot be read"),
        },
    };
    let plan = strings(fields.get("plan"));
    let finished = complete
        && state.failed.as_ref().is_some_and(Vec::is_empty)
        && state.skipped.as_ref().is_some_and(Vec::is_empty)
        && plan.as_ref().is_some_and(|plan| {
            !plan.is_empty() && plan.iter().all(|step| done.contains(step))
        });
    if !finished {
        return Ok(Swbf2Record::Resumable);
    }
    let plan = plan.unwrap_or_default();
    if !plan.iter().any(|step| step.starts_with("map:")) {
        return stale("the record plans no map");
    }
    match state.add_ons.as_deref() {
        Some([]) => {}
        Some(_) => return stale("add-ons are not prepared for the 2005 game yet"),
        None => return stale("the record does not list its add-ons"),
    }
    // A completed step omitted from this run (such as Enhanced Sound) may keep its
    // empty list. Every planned step and the shared files still need their own list.
    let mut lists = Vec::new();
    for (step, files) in &outputs {
        let step = step.as_str();
        if step == "*" || plan.contains(&step) {
            lists.push((step, files));
        } else if !files.is_empty() || !done.contains(&step) {
            return Ok(Swbf2Record::Resumable);
        }
    }
    if !plan.contains(&"common") || lists.len() != plan.len() + 1 {
        return Ok(Swbf2Record::Resumable);
    }
    for (step, listed) in &lists {
        if !required_products_present(step, listed)
            || (*step == "*" && !shared_streams_present(ops, dir, listed)?)
            || (*step == "common" && generation >= 9 && !first_person_listed(ops, dir, listed)?)
        {
            return Ok(Swbf2Record::Resumable);
        }
        for file in listed.iter() {
            if !listed_file_present(ops, dir, file)? {
                return Ok(Swbf2Record::Resumable);
            }
        }
    }
    let mut expected = BTreeSet::new();
    for (step, files) in lists {
        for file in files {
            if !belongs_to_step(&file.path, step) {
                return Ok(Swbf2Record::Resumable);
            }
            expected.insert(file.path.as_str());
        }
    }
    if !all_products_listed(ops, dir, "", &expected)? {
        return Ok(Swbf2Record::Resumable);
    }
    for step in plan {
        if let Some(id) = step.strip_prefix("map:") {
            let map = read_object(ops, &dir.join(format!("maps/{id}/map.json")))?;
            let missions = map.as_ref().and_then(|v| v.get("missions")).and_then(Value::as_object);
            for mission in missions.into_iter().flat_map(|v| v.values()) {
                if let Some(banks) = mission.get("sound").and_then(Value::as_array) {
                    for bank in banks {
                        let Some(name) = bank.as_str() else { return Ok(Swbf2Record::Resumable) };
                        if !expected.contains("audio/index.json")
                            || !expected.contains(format!("audio/{name}/sound.json").as_str()) {
                            return Ok(Swbf2Record::Resumable);
                        }
                    }
                }
            }
        }
    }
    Ok(Swbf2Record::Complete)
}

fn first_person_listed(ops: &Ops, dir: &Path, listed: &[Listed]) -> Checked<bool> {
    let files: std::collections::BTreeMap<_, _> = listed.iter()
        .filter_map(|f| f.path.strip_prefix("firstperson/").map(|name| (name, f))).collect();
    if files.values().any(|f| f.size == 0 || f.sha256.is_none())
        || !files.get("manifest.json").is_some_and(|f| f.size <= 8 << 20)
        || !files.contains_key("anims.bin")
    {
        return Ok(false);
    }
    let Some(manifest) = read_object(ops, &dir.join("firstperson/manifest.json"))? else {
        return Ok(false);
    };
    let has = |rel: &str| files.contains_key(rel);
    let (Some(anims), Some(rigs), Some(textures)) = (
        manifest["animsData"].as_str(), manifest["rigs"].as_object(), manifest["textures"].as_object()
    ) else { return Ok(false) };
    if manifest["version"].as_u64() != Some(2) || !has(anims) || !textures.values().all(|v| v.as_str().is_some_and(has)) {
        return Ok(false);
    }
    Ok(rigs.values().all(|rig| rig["file"].as_str().is_some_and(|rel| {
        rel.strip_suffix(".json").is_some_and(|stem| {
            has(rel) && has(&format!("{stem}.bin")) && has(&format!("{stem}_index.bin"))
        })
    })))
}
/// Each map, side and the common step have private products. The remaining original
/// products are shared. Enhanced Sound has separate receipts and empty step inventories.
fn belongs_to_step(path: &str, step: &str) -> bool {
    let mut parts = path.splitn(3, '/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("audio"), Some("enhanced"), _) => false,
        (Some("maps"), map, Some(rest)) if rest.starts_with("effects/") => step.strip_prefix("effects:") == map,
        (Some("maps"), map, Some(_)) => step.strip_prefix("map:") == map,
        (Some("sides"), side, Some(_)) => step.strip_prefix("side:") == side,
        (Some("common" | "firstperson"), _, _) => step == "common",
        _ => step == "*",
    }
}

/// Temporary names used by the original audio and compiled-script writers. The
/// preparation tools clean these under the lock before saving the next inventory.
fn publication_file(path: &str) -> bool {
    let parts: Vec<_> = path.split('/').collect();
    match parts.as_slice() {
        ["audio", "index.json.part"] => true,
        ["audio", "streams", name] => name
            .strip_suffix(".part")
            .and_then(|v| v.rsplit_once('.'))
            .is_some_and(|(kind, pid)| {
                (kind.ends_with(".bin") || kind.ends_with(".json"))
                    && !pid.is_empty()
                    && pid.bytes().all(|b| b.is_ascii_digit())
            }),
        ["audio", _, name] => name
            .strip_prefix(".bfe-bank-")
            .and_then(|v| v.strip_suffix(".part"))
            .is_some_and(producer_suffix),
        ["scripts", stage, "body"]
        | ["maps", _, "scripts", stage, "body"]
        | ["audio", _, stage, "sound.json"] => stage.strip_prefix("tmp").is_some_and(producer_suffix),
        _ => false,
    }
}

/// Enumerate the actual tree too. A file dropped from the receipt cannot disappear
/// from verification merely because the producer omitted its row.
fn all_products_listed(ops: &Ops, root: &Path, relative: &str, expected: &BTreeSet<&str>) -> Checked<bool> {
    let dir = root.join(relative);
    for (name, kind) in ops.list(&dir).map_err(io_failure("list", &dir))? {
        let Some(name) = name.to_str() else {
            return Ok(false);
        };
        if relative.is_empty()
            && (is_bookkeeping_name(name) || name.eq_ignore_ascii_case(SWBF2_PREPARE_STATE_FILE))
        {
            continue;
        }
        let path = if relative.is_empty() {
            name.to_owned()
        } else {
            format!("{relative}/{name}")
        };
        if path == "audio/enhanced" {
            continue;
        }
        let listed = match kind {
            // A leftover still needs a preparation pass. It is never an expected product.
            Kind::File => !publication_file(&path) && expected.contains(path.as_str()),
            Kind::Dir => all_products_listed(ops, root, &path, expected)?,
            _ => false,
        };
        if !listed {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Core records, geometry, scripts and stream pairs carry data. Other buffers, such
/// as an unused animation bank, can legitimately be emitted empty.
fn required_products_present(step: &str, files: &[Listed]) -> bool {
    if files.iter().any(|f| {
        f.path.starts_with("scripts/")
            && !f
                .sha256
                .as_ref()
                .is_some_and(|hash| f.path == format!("scripts/{hash}.luac"))
    }) {
        return false;
    }
    if files.iter().any(|f| {
        f.size == 0
            && (f.path.starts_with("audio/streams/")
                || (f.path.starts_with("scripts/") && f.path.ends_with(".luac")))
    }) {
        return false;
    }
    let has = |path: &str| files.iter().any(|f| f.path == path && f.size > 0);
    match step.split_once(':') {
        Some(("map", id)) => has(&format!("maps/{id}/map.json")) && has(&format!("maps/{id}/geom.bin")),
        Some(("effects", id)) => {
            let prefix = format!("maps/{id}/effects/");
            files.iter().any(|f| f.size > 0 && f.path.strip_prefix(&prefix)
                .and_then(|name| name.split_once('/'))
                .is_some_and(|(mission, name)| !mission.is_empty() && name == "effects.json"))
        }
        Some(("side", id)) => has(&format!("sides/{id}/units.json")),
        Some(("audio", _)) => files.is_empty(),
        _ if step == "common" => has("common/common.json"),
        _ if step == "*" => true,
        _ => false,
    }
}

/// The audio index names the banks, each bank names its streams, and stream indexes
/// give the byte spans in their binary files. A shortened stream can have a positive
/// size even in a receipt written by a previous producer.
fn shared_streams_present(ops: &Ops, dir: &Path, files: &[Listed]) -> Checked<bool> {
    let sizes: std::collections::BTreeMap<_, _> =
        files.iter().map(|file| (file.path.as_str(), file.size)).collect();
    let size = |name: &str| sizes.get(name).copied().unwrap_or(0);
    let index = "audio/index.json";
    if exists(ops, &dir.join(index))? {
        if size(index) == 0 {
            return Ok(false);
        }
        let Some(index) = read_object(ops, &dir.join(index))? else {
            return Ok(false);
        };
        for bank in ["maps", "missions"]
            .iter()
            .filter_map(|group| index.get(group).and_then(Value::as_object))
            .flat_map(|group| group.values())
        {
            let Some(name) = bank.as_str() else {
                return Ok(false);
            };
            if size(&format!("audio/{name}/sound.json")) == 0 {
                return Ok(false);
            }
        }
    }
    for file in files {
        let path = &file.path;
        if path.starts_with("audio/") && path.ends_with("/sound.json") {
            let Some(bank) = read_object(ops, &dir.join(path))? else {
                return Ok(false);
            };
            if let Some(samples) = bank.get("samples") {
                let Some(samples) = samples.as_object() else {
                    return Ok(false);
                };
                let binary = path.strip_suffix("sound.json").unwrap().to_owned() + "bank.bin";
                let Some(&length) = sizes.get(binary.as_str()) else {
                    return Ok(false);
                };
                if !samples.values().all(|span| span_fits(span, length, 2)) {
                    return Ok(false);
                }
            }
            if let Some(streams) = bank.get("streams") {
                let Some(streams) = streams.as_array() else {
                    return Ok(false);
                };
                for name in streams {
                    let Some(name) = name.as_str() else {
                        return Ok(false);
                    };
                    if size(&format!("audio/streams/{name}.json")) == 0 {
                        return Ok(false);
                    }
                }
            }
        }
        if !path.starts_with("audio/streams/") {
            continue;
        }
        if let Some(stem) = path.strip_suffix(".bin") {
            if size(&format!("{stem}.json")) == 0 {
                return Ok(false);
            }
        } else if let Some(stem) = path.strip_suffix(".json") {
            let length = size(&format!("{stem}.bin"));
            let stream = read_object(ops, &dir.join(path))?;
            let Some(segments) = stream
                .as_ref()
                .and_then(|v| v.get("segments"))
                .and_then(Value::as_object)
            else {
                return Ok(false);
            };
            if length == 0 || !segments.values().all(|span| span_fits(span, length, 1)) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn span_fits(span: &Value, length: u64, bytes_per_unit: u64) -> bool {
    let Some(offset) = span.get(0).and_then(Value::as_u64) else {
        return false;
    };
    span.get(1)
        .and_then(Value::as_u64)
        .and_then(|count| count.checked_mul(bytes_per_unit))
        .and_then(|bytes| offset.checked_add(bytes))
        .is_some_and(|end| end <= length)
}

/// Whether a listed file is there as a plain file of its recorded size and hash.
fn listed_file_present(ops: &Ops, dir: &Path, file: &Listed) -> Checked<bool> {
    let path = file.path.split('/').fold(dir.to_path_buf(), |path, part| path.join(part));
    match ops.stat(&path).map_err(io_failure("look up", &path))? {
        Some((Kind::File, len)) if len == file.size => {}
        _ => return Ok(false),
    }
    match &file.sha256 {
        None => Ok(true),
        Some(want) => Ok(&ops.sha256(&path).map_err(io_failure("read", &path))? == want),
    }
}

/// Every file of a 2005 tree that an export carries, with the record under its own name:
/// everything under the tree except its machinery ([`BOOKKEEPING`]), recognised by what the
/// entries are rather than how a listing spells them. A link anywhere, or an entry at a name
/// the archive keeps for itself, is an error.
pub fn content_files(dir: &Path) -> Result<Vec<ContentFileRecord>, String> {
    let ops = Ops::REAL;
    let identity = |path: &Path| ops.identity(path).map_err(|e| format!("Could not look up {}: {e}", path.display()));
    let receipt = identity(&dir.join(SWBF2_PREPARE_STATE_FILE))?;
    let mut machinery = Vec::new();
    for name in BOOKKEEPING {
        if let Some(handle) = identity(&dir.join(name))? {
            machinery.push(handle);
        }
    }
    let mut out = Vec::new();
    let mut receipt_seen = false;
    for (name, kind) in ops
        .list(dir)
        .map_err(|e| format!("Failed to read directory {}: {e}", dir.display()))?
    {
        let Some(text) = name.to_str() else {
            return Err("Content directory contains non-UTF-8 filename.".to_string());
        };
        let path = dir.join(&name);
        if is_marker_leftover(&text.to_ascii_lowercase()) {
            continue;
        }
        let handle = identity(&path)?;
        if let (Some(handle), Some(receipt)) = (&handle, &receipt) {
            if handle == receipt {
                if !receipt_seen {
                    receipt_seen = true;
                    push_file(&mut out, SWBF2_PREPARE_STATE_FILE.to_string(), &path)?;
                }
                continue;
            }
        }
        if handle.as_ref().is_some_and(|h| machinery.contains(h)) {
            continue;
        }
        if text.eq_ignore_ascii_case(MANIFEST_NAME) || text.eq_ignore_ascii_case(OLD_RECORD) {
            return Err(format!(
                "Prepared content holds {text}, a name the content archive keeps for itself."
            ));
        }
        collect(&mut out, text.to_string(), &path, kind)?;
    }
    if !receipt_seen {
        return Err("Prepared content has no 2005 preparation record.".to_string());
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn collect(
    out: &mut Vec<ContentFileRecord>,
    relative: String,
    path: &Path,
    kind: Kind,
) -> Result<(), String> {
    match kind {
        Kind::File => push_file(out, relative, path),
        Kind::Dir => {
            for (name, kind) in Ops::REAL
                .list(path)
                .map_err(|e| format!("Failed to read directory {}: {e}", path.display()))?
            {
                let Some(text) = name.to_str() else {
                    return Err("Content directory contains non-UTF-8 filename.".to_string());
                };
                collect(out, format!("{relative}/{text}"), &path.join(&name), kind)?;
            }
            Ok(())
        }
        Kind::Link | Kind::Other => Err(format!(
            "Prepared content cannot contain symbolic links: {relative}"
        )),
    }
}

fn push_file(
    out: &mut Vec<ContentFileRecord>,
    relative: String,
    path: &Path,
) -> Result<(), String> {
    safe_path(&relative, false)?;
    let meta = fs::symlink_metadata(path)
        .map_err(|e| format!("Failed to stat {}: {e}", path.display()))?;
    let key = hardlink_key(path, &meta);
    out.push((relative, path.to_path_buf(), meta.len(), key));
    Ok(())
}

/// Whether an archive entry may be restored into a 2005 tree: the record, or a safe path
/// that is not the tree's machinery and not a name the archive keeps for itself.
pub fn archive_entry_ok(name: &str) -> bool {
    name == SWBF2_PREPARE_STATE_FILE || content_path_ok(name)
}

/// Archives can be saved in an ordinary backup folder. No preparation directory or its
/// children are backup destinations, whether or not the selected source is elsewhere.
pub(crate) fn archive_destination(
    path: &Path,
    tools: &Swbf2Tools,
    directory: bool,
) -> Result<PathBuf, String> {
    let ops = Ops::REAL;
    let path = physical(&ops, path).map_err(Problem::message)?;
    let parents = if directory {
        Some(path.as_path())
    } else {
        path.parent()
    };
    if let Some(parent) = parents {
        for dir in parent.ancestors() {
            if dir.ends_with(Path::new("swbf2").join(".content"))
                || holds_prepared_tree(&ops, dir).map_err(Problem::message)?
            {
                write_origin(&ops, dir, Some(tools), None).map_err(Problem::message)?;
                return Err("Save the archive or its imported copy outside prepared game content.".into());
            }
        }
    }
    Ok(path)
}

/// Publishes the marker in `dir` by atomic replacement, leaving a correct one alone.
/// Returns whether it wrote one.
fn publish_marker(ops: &Ops, dir: &Path) -> Checked<bool> {
    // Leftovers of an interrupted publication go first; nothing else writes these names.
    for (name, kind) in ops.list(dir).map_err(io_failure("read", dir))? {
        if kind == Kind::File && is_marker_leftover(&name.to_string_lossy()) {
            let path = dir.join(&name);
            ops.remove_file(&path).map_err(io_failure("remove", &path))?;
        }
    }
    let path = dir.join(SWBF2_MARKER_FILE);
    if ops
        .read(&path)
        .map_err(io_failure("read", &path))?
        .is_some_and(|bytes| marker_says_swbf2(&bytes))
    {
        return Ok(false);
    }
    let temp = dir.join(format!("{SWBF2_MARKER_FILE}.{}.tmp", unique_suffix()));
    ops.write_new(&temp, SWBF2_MARKER_BODY.as_bytes())
        .map_err(io_failure("write", &temp))?;
    if let Err(error) = ops.rename(&temp, &path) {
        let _ = ops.remove_file(&temp);
        return Err(io_failure("publish", &path)(error));
    }
    Ok(true)
}

/// Checks the actual destination of a new archive import before creating its parent.
pub(crate) fn require_new_tree(dir: &Path) -> Result<(), String> {
    write_origin(&Ops::REAL, dir, None, None).map(|_| ()).map_err(Problem::message)
}

/// Makes `dir` (with the folders above it that are missing) and publishes its marker: the
/// start of a 2005 tree in a new folder, such as an import's staging folder.
pub fn mark_new_tree(dir: &Path) -> Result<(), String> {
    require_new_tree(dir)?;
    fs::create_dir_all(dir).map_err(|e| format!("Could not make {}: {e}", dir.display()))?;
    publish_marker(&Ops::REAL, dir)
        .map(|_| ())
        .map_err(Problem::message)
}

fn unique_suffix() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("{}-{nanos}", std::process::id())
}

/// Publish an unfinished receipt before the lock or any payload can be left in a new
/// tree. A simultaneous initializer never replaces the receipt that won the race.
/// During a clear the receipt stays readable, so an interrupted removal can be retried.
fn write_initial_record(
    ops: &Ops,
    tree: &Swbf2Tree,
    tools: &Swbf2Tools,
    install: &Swbf2Install,
    fresh: bool,
    clearing: bool,
) -> Checked<bool> {
    let dest = tree.dir.join(SWBF2_PREPARE_STATE_FILE);
    let parent = if fresh { tree.dir.parent().expect("a family tree has a container") } else { &tree.dir };
    let failure = io_failure("publish", &dest);
    ops.check(Op::Write, &dest).map_err(&failure)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(&failure)?;
    let record = serde_json::json!({
        "family": GameFamily::Swbf2.as_str(), "swbf2Version": tools.generation,
        "version": tools.content.state, "mapImportVersion": tools.content.maps,
        "muzzleVersion": tools.content.sides, "commonVersion": tools.content.common,
        "game": install.lvl_pc, "done": [], "complete": false, "clearing": clearing,
        "failed": [], "skipped": [], "addOns": []
    });
    temp.write_all(record.to_string().as_bytes()).map_err(&failure)?;
    temp.as_file().sync_all().map_err(&failure)?;
    ops.check(Op::Rename, &dest).map_err(&failure)?;
    let result = if fresh {
        temp.persist_noclobber(&dest)
    } else {
        temp.persist(&dest)
    };
    match result {
        Ok(_) => Ok(true),
        Err(e) if fresh && e.error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(failure(e.error)),
    }
}

/// The locked start has verified ownership and published an unfinished receipt.
fn clear(ops: &Ops, dir: &Path) -> Checked<()> {
    for (name, _) in ops.list(dir).map_err(io_failure("read", dir))? {
        let text = name.to_string_lossy();
        let keep = matches!(
            text.as_ref(),
            LOCK_FILE | STEP_LOCKS | SWBF2_MARKER_FILE | SWBF2_PREPARE_STATE_FILE
        );
        if !keep {
            remove_tree(ops, &dir.join(&name))?;
        }
    }
    Ok(())
}

/// Removes a file, or a folder and everything in it. A link is never followed or removed:
/// the tree was checked to hold none, so finding one stops the removal.
fn remove_tree(ops: &Ops, path: &Path) -> Checked<()> {
    match ops.kind(path).map_err(io_failure("look up", path))? {
        None => Ok(()),
        Some(Kind::File) => ops.remove_file(path).map_err(io_failure("remove", path)),
        Some(Kind::Dir) => {
            for (name, _) in ops.list(path).map_err(io_failure("read", path))? {
                remove_tree(ops, &path.join(name))?;
            }
            ops.remove_dir(path).map_err(io_failure("remove", path))
        }
        Some(Kind::Link | Kind::Other) => Err(Problem::Found(format!(
            "{} is a link or a special file; it is left alone.",
            path.display()
        ))),
    }
}

/// Why a 2005 operation could not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartError {
    /// Another preparation holds the tree's lock. Nothing was changed.
    Busy(String),
    /// The place, its contents or the tools are not ones a 2005 preparation may use.
    /// Nothing was changed.
    Refused(String),
    /// A lookup, removal or write failed; the start stopped there.
    Failed(String),
}

impl StartError {
    /// The message for the player or the log.
    pub fn message(&self) -> &str {
        match self {
            StartError::Busy(m) | StartError::Refused(m) | StartError::Failed(m) => m,
        }
    }
}

impl From<Problem> for StartError {
    fn from(problem: Problem) -> Self {
        match problem {
            Problem::Io(message) | Problem::PayloadIo(message) => StartError::Failed(message),
            Problem::Found(message) => StartError::Refused(message),
        }
    }
}

/// The preparation lock on a tree, held until dropped.
///
/// On Linux and macOS it is an exclusive `flock` of the whole lock file, shared with the
/// tools through their standard input. On Windows it is two one-byte locks: byte 0 keeps
/// every other preparation out, and byte 1 is the tools' own lock, which a client holds
/// while it checks and lets go just before it starts the tools, so that the tools take it
/// in their own name and keep it if the client ends first.
#[derive(Debug)]
pub struct TreeLock {
    file: File,
}

impl TreeLock {
    /// The lock file, for passing on as the tools' standard input.
    pub fn file(&self) -> &File {
        &self.file
    }

    /// Lets go of the part of the lock the tools take in their own name (byte 1 on Windows;
    /// nothing elsewhere, where the tools share the client's lock).
    pub fn release_tools_part(&self) -> io::Result<()> {
        #[cfg(windows)]
        {
            windows_lock::unlock(&self.file, 1)?;
        }
        Ok(())
    }
}

#[cfg(windows)]
mod windows_lock {
    use std::fs::File;
    use std::io;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Storage::FileSystem::{LockFile, UnlockFile};

    /// `ERROR_LOCK_VIOLATION`: another handle holds an overlapping range.
    const LOCK_VIOLATION: i32 = 33;

    /// Locks one byte of `file` for this handle; `Ok(false)` when another handle holds it.
    pub(super) fn lock(file: &File, byte: u32) -> io::Result<bool> {
        // SAFETY: the handle comes from a `File` borrowed for the whole call, so it stays open; the range is the one
        // byte `byte`; no memory of ours is passed to or kept by the call.
        let ok = unsafe { LockFile(file.as_raw_handle() as HANDLE, byte, 0, 1, 0) };
        if ok != 0 {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(LOCK_VIOLATION) {
            Ok(false)
        } else {
            Err(error)
        }
    }

    pub(super) fn unlock(file: &File, byte: u32) -> io::Result<()> {
        // SAFETY: the handle comes from a `File` borrowed for the whole call, so it stays open; the range is the one
        // byte `byte`; no memory of ours is passed to or kept by the call.
        let ok = unsafe { UnlockFile(file.as_raw_handle() as HANDLE, byte, 0, 1, 0) };
        if ok != 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

/// Takes the preparation lock without waiting: `Ok(false)` when another holds it.
fn try_lock_file(file: &File) -> io::Result<bool> {
    #[cfg(windows)]
    {
        if !windows_lock::lock(file, 0)? {
            return Ok(false);
        }
        if !windows_lock::lock(file, 1)? {
            windows_lock::unlock(file, 0)?;
            return Ok(false);
        }
        Ok(true)
    }
    #[cfg(not(windows))]
    {
        match file.try_lock() {
            Ok(()) => Ok(true),
            Err(std::fs::TryLockError::WouldBlock) => Ok(false),
            Err(std::fs::TryLockError::Error(error)) => Err(error),
        }
    }
}

fn busy(dir: &Path) -> StartError {
    StartError::Busy(format!(
        "Another preparation is already running in {}",
        dir.display()
    ))
}

/// Takes an authorized tree's existing lock. Returns `None` if the file is absent.
fn lock_existing(ops: &Ops, tree: &Swbf2Tree) -> Result<Option<TreeLock>, StartError> {
    let path = tree.dir.join(LOCK_FILE);
    match ops.kind(&path).map_err(io_failure("look up", &path))? {
        None => return Ok(None),
        Some(Kind::File) => {}
        _ => return Err(unconfirmed(&tree.dir).into()),
    }
    let file = ops.open_lock(&path).map_err(io_failure("open", &path))?;
    if !try_lock_file(&file).map_err(io_failure("lock", &path))? {
        return Err(busy(&tree.dir));
    }
    Ok(Some(TreeLock { file }))
}

/// Makes the lock file where it is missing and takes the lock.
fn lock_new(ops: &Ops, tree: &Swbf2Tree) -> Result<TreeLock, StartError> {
    let path = tree.dir.join(LOCK_FILE);
    let file = ops.create_lock(&path).map_err(io_failure("open", &path))?;
    if !try_lock_file(&file).map_err(io_failure("lock", &path))? {
        return Err(busy(&tree.dir));
    }
    Ok(TreeLock { file })
}

/// Confirms, under the lock, that the tree is still where it was and that the lock held is
/// the tree's own.
fn confirm_lock(ops: &Ops, content_root: &Path, tree: &Swbf2Tree, lock: &TreeLock) -> Result<(), StartError> {
    let path = tree.dir.join(LOCK_FILE);
    let again = locate_with(ops, content_root)?;
    if again != *tree || !ops.same_file(&lock.file, &path).map_err(io_failure("look up", &path))? {
        return Err(StartError::Refused(format!(
            "{} changed while the preparation started.",
            tree.dir.display()
        )));
    }
    Ok(())
}

/// A started 2005 preparation: the tree, checked, marked and cleared when it was stale, and
/// the lock on it, held until this value is dropped.
#[derive(Debug)]
pub struct Swbf2Start {
    /// Where the tools write.
    pub tree: Swbf2Tree,
    /// The copy the tools read, as identified for this start.
    pub install: Swbf2Install,
    /// The tools' lock on [`Swbf2Tree::dir`], held. Pass [`TreeLock::file`] on as the tools'
    /// standard input with [`LOCK_ENV`] set to [`LOCK_HANDOVER`], after
    /// [`TreeLock::release_tools_part`].
    pub lock: TreeLock,
    /// What the tree was when the start found it (a stale one has since been cleared).
    pub found: Swbf2Record,
}

/// Starts a 2005 preparation of `content_root` from the copy at `install_dir`. Before it
/// changes anything it checks that the tools take over the lock and record their inventory,
/// identifies the copy and checks the tree's origin before opening its lock. A new tree
/// gets an unfinished receipt before its lock. Under the lock the desktop checks the origin
/// again, publishes the marker and clears an owned stale tree. A concurrent start in an
/// owned tree returns [`StartError::Busy`].
pub fn begin(
    content_root: &Path,
    install_dir: &Path,
    tools: &Swbf2Tools,
) -> Result<Swbf2Start, StartError> {
    begin_with(&Ops::REAL, content_root, install_dir, tools)
}

pub(crate) fn begin_with(
    ops: &Ops,
    content_root: &Path,
    install_dir: &Path,
    tools: &Swbf2Tools,
) -> Result<Swbf2Start, StartError> {
    let install = identify_install_with(ops, install_dir)?;
    begin_identified(ops, content_root, install, tools)
}

pub(crate) fn begin_identified(
    ops: &Ops, content_root: &Path, install: Swbf2Install, tools: &Swbf2Tools,
) -> Result<Swbf2Start, StartError> {
    if !tools.handover {
        return Err(StartError::Refused(
            "These preparation tools do not take over the 2005 folder's lock, so they cannot prepare the 2005 game."
                .to_string(),
        ));
    }
    if !tools.inventory {
        return Err(StartError::Refused(
            "These preparation tools do not record what they write in a form this build reads, so they cannot prepare the 2005 game."
                .to_string(),
        ));
    }
    let tree = locate_with(ops, content_root)?;
    let origin = match write_origin(ops, &tree.dir, Some(tools), None) {
        Err(error @ Problem::PayloadIo(_)) => {
            // A live writer can remove a product while we are reading it. Only probe an
            // existing lock, after checking the receipts, marker and accessible payload.
            if let Err(busy @ StartError::Busy(_)) = lock_existing(ops, &tree) {
                return Err(busy);
            }
            return Err(error.into());
        }
        result => result?,
    };
    let existing = lock_existing(ops, &tree)?;
    let fresh = if origin == Origin::Fresh {
        make_missing(ops, &tree)?;
        write_initial_record(ops, &tree, tools, &install, true, false)?
    } else {
        // Check recorded install lookup errors before creating a missing lock.
        classify(ops, &tree.dir, tools, Some(&install), origin)?;
        false
    };
    let lock = match existing {
        Some(lock) => lock,
        None => lock_new(ops, &tree)?,
    };
    confirm_lock(ops, content_root, &tree, &lock)?;
    let origin = write_origin(ops, &tree.dir, Some(tools), Some(&lock))?;
    let mut found = classify(ops, &tree.dir, tools, Some(&install), origin)?;
    if fresh && found == Swbf2Record::Resumable {
        found = Swbf2Record::Empty;
    }
    publish_marker(ops, &tree.dir)?;
    if matches!(found, Swbf2Record::Stale(_)) {
        write_initial_record(ops, &tree, tools, &install, false, true)?;
        clear(ops, &tree.dir)?;
        write_initial_record(ops, &tree, tools, &install, false, false)?;
    }
    Ok(Swbf2Start {
        tree,
        install,
        lock,
        found,
    })
}

/// Makes the content folder, `swbf2` and `.content` where they are missing, as plain
/// folders.
fn make_missing(ops: &Ops, tree: &Swbf2Tree) -> Checked<()> {
    let mut missing = Vec::new();
    for path in tree.dir.ancestors() {
        match ops.kind(path).map_err(io_failure("look up", path))? {
            None => missing.push(path.to_path_buf()),
            Some(Kind::Dir) => break,
            Some(_) => return Err(Problem::Found(format!("{} is not a folder.", path.display()))),
        }
    }
    for path in missing.iter().rev() {
        ops.make_dir(path).map_err(io_failure("make", path))?;
    }
    Ok(())
}

/// Whether the 2005 tree of `content_root` is complete for the copy at `install_dir`. A place
/// [`locate`] refuses, or a folder that is not a copy of the game, is not complete; a failed
/// lookup is an error.
pub fn verify(
    content_root: &Path,
    install_dir: &Path,
    tools: &Swbf2Tools,
) -> Result<bool, String> {
    let ops = Ops::REAL;
    let tree = match locate_with(&ops, content_root) {
        Ok(tree) => tree,
        Err(Problem::Found(_)) => return Ok(false),
        Err(failure) => return Err(failure.message()),
    };
    let install = match identify_install_with(&ops, install_dir) {
        Ok(install) => install,
        Err(Problem::Found(_)) => return Ok(false),
        Err(failure) => return Err(failure.message()),
    };
    Ok(inspect(&tree.dir, tools, Some(&install))? == Swbf2Record::Complete)
}

/// A complete 2005 tree locked for reading it whole (an export) or changing its record (a
/// relocation): ownership is established before anything is made in the tree, and checked
/// again under the lock.
#[derive(Debug)]
pub struct Swbf2Session {
    /// The tree.
    pub tree: Swbf2Tree,
    /// Its lock, held until this value is dropped.
    pub lock: TreeLock,
}

/// Locks the complete 2005 tree of `content_root` (resolved physically, as a start resolves
/// it), whatever copy it came from.
pub fn complete_session(content_root: &Path, tools: &Swbf2Tools) -> Result<Swbf2Session, String> {
    let ops = Ops::REAL;
    let not_complete = |found: &Swbf2Record| {
        format!(
            "Prepared content is incomplete or outdated ({}); prepare it again.",
            found.reason()
        )
    };
    let tree = locate_with(&ops, content_root).map_err(Problem::message)?;
    let found = inspect(&tree.dir, tools, None)?;
    if found != Swbf2Record::Complete {
        return Err(not_complete(&found));
    }
    let lock = match lock_existing(&ops, &tree).map_err(|e| e.message().to_string())? {
        Some(lock) => lock,
        None => lock_new(&ops, &tree).map_err(|e| e.message().to_string())?,
    };
    confirm_lock(&ops, content_root, &tree, &lock).map_err(|e| e.message().to_string())?;
    let found = inspect(&tree.dir, tools, None)?;
    if found != Swbf2Record::Complete {
        return Err(not_complete(&found));
    }
    Ok(Swbf2Session { tree, lock })
}

/// The record of the tree at `dir` as the typed reader parses it.
pub fn record_state(dir: &Path) -> Option<PrepareState> {
    let raw = fs::read(dir.join(SWBF2_PREPARE_STATE_FILE)).ok()?;
    serde_json::from_slice(&raw).ok()
}

/// Checks a restored tree (an import's staging folder) the way verification would, with no
/// installed copy needed.
pub fn check_restored(dir: &Path, tools: &Swbf2Tools) -> Result<(), String> {
    match inspect(dir, tools, None)? {
        Swbf2Record::Complete => Ok(()),
        found => Err(format!(
            "The imported 2005 content is not complete: {}.",
            found.reason()
        )),
    }
}

/// Replaces the record of a locked tree by atomic replacement: a new file is written and
/// flushed, then renamed over the record. (The folder itself is not flushed, so after a
/// power loss the old record may come back; it is never half written.)
pub fn replace_record(session: &Swbf2Session, bytes: &[u8]) -> Result<(), String> {
    let path = session.tree.dir.join(SWBF2_PREPARE_STATE_FILE);
    let temp = session.tree.dir.join(RECORD_TEMP);
    Ops::REAL
        .remove_file(&temp)
        .map_err(|e| format!("Could not remove {}: {e}", temp.display()))?;
    Ops::REAL
        .write_new(&temp, bytes)
        .map_err(|e| format!("Could not write {}: {e}", temp.display()))?;
    fs::rename(&temp, &path).map_err(|e| {
        let _ = fs::remove_file(&temp);
        format!("Could not publish {}: {e}", path.display())
    })
}
