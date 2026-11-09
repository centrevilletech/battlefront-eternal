//! Native desktop platform crate for Battlefront Eternal (`bfe-desktop`).
//! Desktop modules for paths, settings, discovery, preparation,
//! archives, updates, launch handoff, rich presence, local server hosting,
//! Steam, and Linux integration.
//! Used by `bfe-client` and `desktop-tool` on Windows and Linux.

use basis_transcode as _;

pub mod archive;
pub mod discover;
pub mod gpu;
pub mod host;
pub mod launch;
pub mod linux;
pub mod paths;
pub mod prepare;
pub mod presence;
pub mod settings;
pub mod slow_links;
pub mod steam;
pub mod update;
