//! Context-local movement. No public method lends an owner or invokes a
//! caller closure while movement or collision storage is borrowed.
use super::collision::CollisionError;
use super::{Context, Error};
use crate::movement::{soldier::Input, IO_LEN};

pub use crate::movement::native::MovementResult;

#[derive(Clone, Debug)]
pub enum MovementError {
    Context(Error),
    Collision(CollisionError),
    InvalidMover,
    InvalidFlags,
    UnknownState,
    Capacity,
}
impl From<Error> for MovementError {
    fn from(error: Error) -> Self {
        Self::Context(error)
    }
}
impl From<CollisionError> for MovementError {
    fn from(error: CollisionError) -> Self {
        Self::Collision(error)
    }
}
impl std::fmt::Display for MovementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Context(e) => e.fmt(f),
            Self::Collision(e) => e.fmt(f),
            Self::InvalidMover => f.write_str("invalid or removed movement handle"),
            Self::InvalidFlags => f.write_str("flags must follow the world's body order"),
            Self::UnknownState => f.write_str("unknown movement state code"),
            Self::Capacity => f.write_str("too many movement handles"),
        }
    }
}
impl std::error::Error for MovementError {}
type Result<T> = std::result::Result<T, MovementError>;

impl Context {
    fn movement_owner<T>(&self, f: impl FnOnce(&mut crate::movement::MovementOwner, &mut crate::collision::CollisionOwner) -> Result<T>) -> Result<T> {
        let _flight = self.enter()?;
        self.world(|w| f(&mut w.movement, &mut w.collision))?
    }
    /// A new local slot, never reused. Class words follow movement's CLASS
    /// exchange slots. The collision world must already be fully installed.
    pub fn movement_new(&self, world: u32, class: [f64; 21]) -> Result<u32> {
        self.movement_owner(|owner, collision| {
            collision.native_movement_state(world)?;
            owner.native_new_mover(world, class)
        })
    }
    /// Load the existing exchange state without ticking. An unknown code
    /// changes nothing and returns UnknownState, matching legacy fallback.
    pub fn movement_load(&self, id: u32, io: [f64; IO_LEN]) -> Result<()> {
        self.movement_owner(|owner, _| owner.native_load(id, io))
    }
    /// Advance the stored mover using owned input and flags.
    pub fn movement_advance(
        &self,
        id: u32,
        input: Input,
        dt: f64,
        flags: Vec<u8>,
    ) -> Result<MovementResult> {
        self.movement_owner(|owner, collision| {
            let world = owner.native_world(id)?;
            owner.native_advance(id, input, dt, flags, collision.native_movement_state(world)?)
        })
    }
    /// The existing movement exchange as an owned, atomic native operation.
    /// Invalid flags or unknown state codes leave the previous mover intact.
    pub fn movement_tick(
        &self,
        id: u32,
        io: [f64; IO_LEN],
        flags: Vec<u8>,
    ) -> Result<MovementResult> {
        self.movement_owner(|owner, collision| {
            let world = owner.native_world(id)?;
            owner.native_tick(id, io, flags, collision.native_movement_state(world)?)
        })
    }
    pub fn movement_snapshot(&self, id: u32) -> Result<MovementResult> {
        self.movement_owner(|owner, _| owner.native_snapshot(id))
    }
    /// Release a mover and its caches. Copies previously returned stay valid.
    pub fn movement_remove(&self, id: u32) -> Result<()> {
        self.movement_owner(|owner, _| owner.native_remove(id))
    }
}
