//! Native desktop platform crate for Battlefront Eternal (`bfe-desktop`).
//! Mirrors `electron/` modules for paths, settings, discovery, preparation,
//! archives, updates, rich presence, local server hosting, Steam, and Linux integration.
//! Used by `bfe-client` and `desktop-tool` on Windows and Linux.

pub mod archive;
pub mod discover;
pub mod gpu;
pub mod host;
pub mod linux;
pub mod paths;
pub mod prepare;
pub mod presence;
pub mod settings;
pub mod slow_links;
pub mod steam;
pub mod update;
