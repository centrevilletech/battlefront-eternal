//! Where content bytes come from. In a browser or the world worker the
//! JavaScript fetches each file and hands its bytes over (`MemFiles`); the
//! native tools read the content folder (`DirFiles`). Paths are the URL
//! paths the game fetches, without the leading slash: `maps/nab2/map.json`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

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

/// A content folder on disk (BFE_CONTENT_DIR), read once a file. Records
/// every path asked for, found or not, so a host knows what to fetch.
pub struct DirFiles {
    pub root: PathBuf,
    cache: RefCell<BTreeMap<String, Option<Rc<[u8]>>>>,
    pub asked: RefCell<Vec<String>>,
}

impl DirFiles {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        DirFiles { root: root.into(), cache: RefCell::new(BTreeMap::new()), asked: RefCell::new(Vec::new()) }
    }
}

impl Files for DirFiles {
    fn read(&self, path: &str) -> Option<Rc<[u8]>> {
        let path = path.trim_start_matches('/');
        // As the headless shim resolves a URL: no query string.
        let path = path.split('?').next().unwrap_or(path);
        if let Some(hit) = self.cache.borrow().get(path) {
            return hit.clone();
        }
        self.asked.borrow_mut().push(path.to_string());
        let bytes = std::fs::read(self.root.join(path)).ok().map(Rc::from);
        self.cache.borrow_mut().insert(path.to_string(), bytes.clone());
        bytes
    }
}
