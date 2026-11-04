//! Context-local movement. No public method lends an owner or invokes a
//! caller closure while movement or collision storage is borrowed.
use super::collision::CollisionError;
use super::{Context, Error};
use crate::movement::{soldier::Input, IO_LEN};

#[path = "../movement/native.rs"]
mod owner;
pub use owner::MovementResult;
pub(super) use owner::NativeMovement;

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
    fn movement_owner<T>(&self, f: impl FnOnce(&mut NativeMovement) -> Result<T>) -> Result<T> {
        let _flight = self.enter()?;
        f(self
            .0
            .movement
            .borrow_mut()
            .get_or_insert_with(NativeMovement::default))
    }
    /// A new local slot, never reused. Class words follow movement's CLASS
    /// exchange slots. The collision world must already be fully installed.
    pub fn movement_new(&self, world: u32, class: [f64; 21]) -> Result<u32> {
        self.movement_owner(|owner| {
            self.movement_collision(world, |_| owner.new_mover(world, class))?
        })
    }
    /// Load the existing exchange state without ticking. An unknown code
    /// changes nothing and returns UnknownState, matching legacy fallback.
    pub fn movement_load(&self, id: u32, io: [f64; IO_LEN]) -> Result<()> {
        self.movement_owner(|owner| owner.load(id, io))
    }
    /// Advance the stored mover using owned input and flags.
    pub fn movement_advance(
        &self,
        id: u32,
        input: Input,
        dt: f64,
        flags: Vec<u8>,
    ) -> Result<MovementResult> {
        self.movement_owner(|owner| {
            let world = owner.world(id)?;
            self.movement_collision(world, |s| owner.advance(id, input, dt, flags, s))?
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
        self.movement_owner(|owner| {
            let world = owner.world(id)?;
            self.movement_collision(world, |s| owner.tick(id, io, flags, s))?
        })
    }
    pub fn movement_snapshot(&self, id: u32) -> Result<MovementResult> {
        self.movement_owner(|owner| owner.snapshot(id))
    }
    /// Release a mover and its caches. Copies previously returned stay valid.
    pub fn movement_remove(&self, id: u32) -> Result<()> {
        self.movement_owner(|owner| owner.remove(id))
    }
}
