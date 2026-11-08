//! Desktop hosting around the shared authoritative world boundary.
//! Cold admission supplies Context bindings; the host only submits input,
//! advances accepted ticks and presents their canonical publications.
pub mod battle;
pub mod arguments;
pub mod context;
pub mod clock;
pub mod commands;
pub mod options;
pub mod launch;
pub mod publication;
pub mod runtime;
pub mod setup;
pub mod thread;
pub mod window;
