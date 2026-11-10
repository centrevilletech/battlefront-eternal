//! Direct online battle host. The server supplies snapshots and accepts input;
//! this owner runs no authoritative simulation and never creates a world.
pub mod client;
pub mod chat;
pub mod clock;
pub mod feedback;
pub mod index;
pub mod info;
pub mod input;
pub mod receiver;
pub mod replay;
pub mod replicas;

pub mod checkpoint;

pub mod hulls;

pub mod publications;

pub mod hud;
mod hud_feed;

pub mod models;

pub mod items;

pub mod sound;
pub mod sound_data;

pub mod combat;
pub mod particles;

pub mod turrets;

pub mod cameras;

pub mod hull_replay;

mod events;

pub mod first_person;

mod water;

mod collision;

mod stream_assets;

mod seat_view;

pub mod roster;

pub mod rotation;

mod sound_events;

mod own_view;
mod radar;
