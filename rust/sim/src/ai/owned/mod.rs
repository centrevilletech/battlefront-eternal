//! AI services over canonical battle stores. No method here crosses the host
//! boundary; the world calls these within its AI and post-movement phases.
pub mod navigation;
pub mod lod;
pub mod population;
pub mod classes;
pub mod scratch;
pub mod entities;
pub mod mounts;
pub mod vehicles;
pub mod vehicle_frame;
pub mod weapons;
pub mod arsenals;
pub mod events;
pub mod sight;
pub mod state;
pub mod jets;
pub mod rows;
pub mod frame;
pub mod world;
pub mod crew;
mod index;
pub mod battle;
pub mod registry;
pub mod units;
pub mod roster;
pub mod posts;
pub mod combat;
pub mod controls;
pub mod hints;
pub mod items;
pub mod buffers;
pub mod controllers;
pub mod geometry;
pub mod observe;
pub mod policy;

pub mod agents;

pub mod process;

pub mod intent;

pub mod turrets;

pub mod runtime;
pub mod lifecycle;

pub mod services;

pub mod objects;

pub mod buildings;

pub mod content;

pub mod seats;

pub mod scene;

pub mod live;

pub mod active;

pub mod helpers;

mod active_vehicles;

pub mod vehicle_agent;

mod vehicle_host;

mod vehicle_driver;

mod patterns;

pub mod owner;

pub mod load;

pub mod callbacks;

pub mod physical;

pub mod pilot;

pub mod host;

pub mod publication;

pub mod entry;

pub mod exchange;

pub mod danger;

pub mod control_views;

pub mod initial;

// Cold policy readback for player, seat and HUD resource hosts.
pub mod policies;

#[cfg(test)]
pub(crate) mod tests;
