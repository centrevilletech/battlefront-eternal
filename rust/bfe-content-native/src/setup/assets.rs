//! The files a battle fetches, as the headless host's manifest keys them:
//! URL paths with their leading slash, in fetch order, each with its bytes'
//! SHA-256 (a missing file is recorded too: a 404 is an input as well).
//! game.js start()'s order is the content crate's (rust/content/src/battle.rs);
//! this only names and hashes what it fetched.
use bfe_content::battle::{Battle, Fetched};
use bfe_content::files::Files;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    /// `/maps/nab2/map.json`: the fetched URL's path.
    pub url: String,
    /// None for a file the battle asked for and did not find.
    pub sha256: Option<[u8; 32]>,
    pub bytes: usize,
}
impl Asset {
    pub fn hex(&self) -> Option<String> {
        self.sha256.map(|digest| digest.iter().map(|b| format!("{b:02x}")).collect())
    }
}

/// Every file `battle` fetched, in the order first asked for. The bytes
/// are read again from `files` (cached by the folder reader), because the
/// fetch log keeps parsed JSON, not the text it came from.
pub fn manifest(battle: &Battle, files: &dyn Files) -> Vec<Asset> {
    battle
        .fetched
        .order
        .iter()
        .map(|path| {
            let url = format!("/{path}");
            match battle.fetched.entries.get(path) {
                Some(Fetched::Missing) | None => Asset { url, sha256: None, bytes: 0 },
                Some(_) => match files.read(path) {
                    Some(bytes) => Asset { url, sha256: Some(Sha256::digest(&bytes).into()), bytes: bytes.len() },
                    None => Asset { url, sha256: None, bytes: 0 },
                },
            }
        })
        .collect()
}
