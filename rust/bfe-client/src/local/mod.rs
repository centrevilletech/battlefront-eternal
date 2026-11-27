//! Desktop hosting around the shared authoritative world boundary.
//! Cold admission supplies Context bindings; the host only submits input,
//! advances accepted ticks and presents their canonical publications.
pub(crate) mod door_frames;
pub mod arguments;
pub mod context;
pub mod clock;
pub mod commands;
pub mod options;
pub mod launch;
pub mod mission_input;
pub mod mission_view;
pub mod publication;
pub mod runtime;
pub mod setup;
pub mod thread;
pub mod space;
pub(crate) mod flight_view;
