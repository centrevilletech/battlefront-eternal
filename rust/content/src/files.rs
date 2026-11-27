//! Where content bytes come from. In a client or the local world thread the
//! JavaScript fetches each file and hands its bytes over (`MemFiles`); the
//! native tools read the content folder (`DirFiles`). Paths are the URL
//! paths the game fetches, without the leading slash: `maps/nab2/map.json`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use bfe_family::GameFamily;

use crate::layout::family_dir;

pub trait Files {
    /// The file's bytes, or None where a fetch would fail (404).
    fn read(&self, path: &str) -> Option<Rc<[u8]>>;
}

/// Files handed over by the host, by path.
#[derive(Default)]
pub struct MemFiles {
    pub files: BTreeMap<String, Rc<[u8]>>,
}

impl MemFiles {
    pub fn insert(&mut self, path: &str, bytes: Vec<u8>) {
        self.files.insert(path.trim_start_matches('/').to_string(), Rc::from(bytes));
    }
}

impl Files for MemFiles {
    fn read(&self, path: &str) -> Option<Rc<[u8]>> {
        self.files.get(path.trim_start_matches('/')).cloned()
    }
}

/// A content folder on disk (`BFE_CONTENT_DIR`), read once a file for one game
/// family (flat `<content>` for `swbf1`, `<content>/swbf2/.content` for `swbf2`).
/// Records every path asked for, found or not, so a host knows what to fetch.
pub struct DirFiles {
    pub root: PathBuf,
    pub family: GameFamily,
    cache: RefCell<BTreeMap<String, Option<Rc<[u8]>>>>,
    pub asked: RefCell<Vec<String>>,
}

impl DirFiles {
    /// Opens a shared content root and resolves `family` once via [`family_dir`].
    pub fn new(root: impl Into<PathBuf>, family: GameFamily) -> Self {
        let root = family_dir(&root.into(), family);
        Self::from_resolved_root(root, family)
    }

    /// Opens an already family-resolved content root without applying [`family_dir`] again.
    pub fn from_resolved_root(root: impl Into<PathBuf>, family: GameFamily) -> Self {
        DirFiles {
            root: root.into(),
            family,
            cache: RefCell::new(BTreeMap::new()),
            asked: RefCell::new(Vec::new()),
        }
    }
}

/// A fetched path as a content folder names its file: no leading slash and,
/// as the headless shim resolves a URL, no query string.
fn file_path(path: &str) -> &str {
    let path = path.trim_start_matches('/');
    path.split('?').next().unwrap_or(path)
}

impl Files for DirFiles {
    fn read(&self, path: &str) -> Option<Rc<[u8]>> {
        let path = file_path(path);
        if let Some(hit) = self.cache.borrow().get(path) {
            return hit.clone();
        }
        self.asked.borrow_mut().push(path.to_string());
        let bytes = std::fs::read(self.root.join(path)).ok().map(Rc::from);
        self.cache.borrow_mut().insert(path.to_string(), bytes.clone());
        bytes
    }
}

/// The files one reading saw, by path, found or not: one generation of a
/// content folder that a later reading can be held to. It can move between
/// threads, unlike the readers.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Pinned {
    files: BTreeMap<String, Option<Arc<[u8]>>>,
}

impl Pinned {
    /// How many paths it holds.
    pub fn len(&self) -> usize { self.files.len() }
    pub fn is_empty(&self) -> bool { self.files.is_empty() }
}

impl std::fmt::Debug for Pinned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Pinned({} files)", self.files.len())
    }
}

/// Reads through `files` and keeps what each path gave, found or not.
pub struct Recorder<'a> {
    files: &'a dyn Files,
    seen: RefCell<Pinned>,
}

impl<'a> Recorder<'a> {
    pub fn new(files: &'a dyn Files) -> Self { Self { files, seen: Default::default() } }
    /// The generation it read.
    pub fn pinned(self) -> Pinned { self.seen.into_inner() }
}

impl Files for Recorder<'_> {
    fn read(&self, path: &str) -> Option<Rc<[u8]>> {
        let path = file_path(path);
        if let Some(seen) = self.seen.borrow().files.get(path) {
            return seen.as_deref().map(Rc::from);
        }
        let bytes = self.files.read(path);
        self.seen.borrow_mut().files.insert(path.to_string(), bytes.as_deref().map(Arc::from));
        bytes
    }
}

/// Reads a pinned generation where it has the path, found or not, and
/// `files` for every other path.
pub struct PinnedFiles<'a> {
    pinned: Option<&'a Pinned>,
    files: &'a dyn Files,
}

impl<'a> PinnedFiles<'a> {
    pub fn new(pinned: Option<&'a Pinned>, files: &'a dyn Files) -> Self { Self { pinned, files } }
}

impl Files for PinnedFiles<'_> {
    fn read(&self, path: &str) -> Option<Rc<[u8]>> {
        match self.pinned.and_then(|pinned| pinned.files.get(file_path(path))) {
            Some(pinned) => pinned.as_deref().map(Rc::from),
            None => self.files.read(path),
        }
    }
}

#[cfg(test)]
mod pinned_tests {
    use super::*;

    // What one reading saw stays what a pinned reading gets, whatever the
    // folder holds afterwards: a changed file, a file that appeared where the
    // reading found none; a path it did not read comes from the folder.
    #[test]
    fn a_pinned_reading_gets_the_generation_the_recorder_saw() {
        let root = std::env::temp_dir().join(format!("bfe-content-pinned-{}", std::process::id()));
        std::fs::create_dir_all(root.join("sides/rep")).unwrap();
        std::fs::write(root.join("sides/rep/units.json"), b"first").unwrap();
        std::fs::write(root.join("unread.json"), b"later").unwrap();
        let pinned = {
            let disk = DirFiles::new(&root, GameFamily::Swbf1);
            let recorder = Recorder::new(&disk);
            assert_eq!(recorder.read("/sides/rep/units.json").as_deref(), Some(&b"first"[..]));
            assert_eq!(recorder.read("maps/none.json?x=1"), None);
            assert_eq!(recorder.read("sides/rep/units.json").as_deref(), Some(&b"first"[..]));
            recorder.pinned()
        };
        assert_eq!(pinned.len(), 2);
        std::fs::write(root.join("sides/rep/units.json"), b"second").unwrap();
        std::fs::create_dir_all(root.join("maps")).unwrap();
        std::fs::write(root.join("maps/none.json"), b"appeared").unwrap();
        let disk = DirFiles::new(&root, GameFamily::Swbf1);
        let held = PinnedFiles::new(Some(&pinned), &disk);
        assert_eq!(held.read("sides/rep/units.json").as_deref(), Some(&b"first"[..]));
        assert_eq!(held.read("/maps/none.json"), None);
        assert_eq!(held.read("unread.json").as_deref(), Some(&b"later"[..]));
        let unpinned = PinnedFiles::new(None, &disk);
        assert_eq!(unpinned.read("sides/rep/units.json").as_deref(), Some(&b"second"[..]));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
