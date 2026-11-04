//! Native ownership of the existing soldier mover and its query scratch.
//! All calls are callback-free and use the context's collision State.
use super::MovementError;
use crate::collision::{
    spheres::{GatherCache, Gathered},
    State,
};
use crate::movement::{
    self,
    scene::{GroundCache, Scene},
    soldier::{Input, Mover, PART_CACHES},
    IO_LEN,
};
use std::collections::BTreeMap;

/// An owned copy remains valid after another tick, removal or disposal.
#[derive(Clone)]
pub struct MovementResult {
    pub mover: Mover,
    pub io: [f64; IO_LEN],
    pub flags: Vec<u8>,
}

struct Entry {
    mover: Mover,
    caches: [GatherCache; PART_CACHES],
    io: [f64; IO_LEN],
    flags: Vec<u8>,
}

#[derive(Default)]
pub(crate) struct NativeMovement {
    entries: Vec<Option<Entry>>,
    gathered: Gathered,
    // lastGround belongs to a collision world, never to the last mover
    // which happened to run. Cast buffers are also private to this scene.
    ground: BTreeMap<usize, GroundCache>,
}
type Result<T> = std::result::Result<T, MovementError>;

impl NativeMovement {
    pub(super) fn new_mover(&mut self, world: u32, class: [f64; 21]) -> Result<u32> {
        let id = u32::try_from(self.entries.len()).map_err(|_| MovementError::Capacity)?;
        let mut mover = Mover::new(world as usize);
        movement::set_class(&mut mover, &class);
        let mut io = [0.; IO_LEN];
        io[movement::CLASS..movement::CLASS + class.len()].copy_from_slice(&class);
        movement::store(&mover, &mut io);
        io[movement::MAX_FLY] = mover.max_fly_height;
        io[movement::GETUP] = mover.getup_time;
        io[movement::DIVE_TIME] = mover.dive_time;
        io[movement::PASS_THROUGH] = mover.pass_through as f64;
        self.entries.push(Some(Entry {
            mover,
            caches: std::array::from_fn(|_| GatherCache::new(0.75)),
            io,
            flags: Vec::new(),
        }));
        Ok(id)
    }
    fn entry(&self, id: u32) -> Result<&Entry> {
        self.entries
            .get(id as usize)
            .and_then(Option::as_ref)
            .ok_or(MovementError::InvalidMover)
    }
    fn entry_mut(&mut self, id: u32) -> Result<&mut Entry> {
        self.entries
            .get_mut(id as usize)
            .and_then(Option::as_mut)
            .ok_or(MovementError::InvalidMover)
    }
    pub(super) fn world(&self, id: u32) -> Result<u32> {
        Ok(self.entry(id)?.mover.world as u32)
    }
    pub(super) fn load(&mut self, id: u32, io: [f64; IO_LEN]) -> Result<()> {
        let entry = self.entry_mut(id)?;
        // load validates all state codes before changing any mover field.
        if !movement::load(&mut entry.mover, &io) {
            return Err(MovementError::UnknownState);
        }
        entry.io = io;
        Ok(())
    }
    pub(super) fn snapshot(&self, id: u32) -> Result<MovementResult> {
        let entry = self.entry(id)?;
        Ok(MovementResult {
            mover: entry.mover.clone(),
            io: entry.io,
            flags: entry.flags.clone(),
        })
    }
    pub(super) fn remove(&mut self, id: u32) -> Result<()> {
        self.entry(id)?;
        self.entries[id as usize] = None;
        Ok(())
    }
    fn validate_flags(&self, id: u32, s: &State, flags: &[u8]) -> Result<()> {
        if flags.len() != s.worlds[self.entry(id)?.mover.world].order.len() {
            return Err(MovementError::InvalidFlags);
        }
        Ok(())
    }
    pub(super) fn tick(
        &mut self,
        id: u32,
        io: [f64; IO_LEN],
        flags: Vec<u8>,
        s: &mut State,
    ) -> Result<MovementResult> {
        self.validate_flags(id, s, &flags)?;
        self.load(id, io)?;
        self.advance(id, movement::input(&io), io[movement::DT], flags, s)
    }
    pub(super) fn advance(
        &mut self,
        id: u32,
        input: Input,
        dt: f64,
        flags: Vec<u8>,
        s: &mut State,
    ) -> Result<MovementResult> {
        self.validate_flags(id, s, &flags)?;
        let entry = self.entries[id as usize].as_mut().unwrap();
        entry.flags = flags;
        let world = entry.mover.world;
        let ground_cache = self.ground.entry(world).or_insert_with(GroundCache::owned);
        let mut scene = Scene {
            s,
            world,
            flags: &entry.flags,
            gathered: &mut self.gathered,
            caches: &mut entry.caches,
            ground_cache,
        };
        movement::advance_in(&mut entry.mover, &input, dt, &mut scene);
        movement::store(&entry.mover, &mut entry.io);
        self.snapshot(id)
    }
}
