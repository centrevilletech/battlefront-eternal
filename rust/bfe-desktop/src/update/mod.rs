//! Desktop auto-updater (SemVer, Ed25519 feed verification, payload slots, and installers).
//! Mirrors `electron/update/`.

pub mod feed;
pub mod install;
pub mod keys;
pub mod state;
pub mod tree;
pub mod version;
