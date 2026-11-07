//! Native ownership of the existing soldier mover and its query scratch.
//! All calls are callback-free and use the context's collision State.
use crate::abi::context::MovementError;
use crate::collision::State;
use crate::movement::{
    self, MovementOwner,
    scene::{GroundCache, Scene},
    soldier::{Input, Mover},
    IO_LEN,
};
use std::collections::BTreeMap;

/// An owned copy remains valid after another tick, removal or disposal.
#[derive(Clone)]
pub struct MovementResult {
    pub mover: Mover,
    /// Last loaded exchange, with the legacy store's output fields updated.
    pub io: [f64; IO_LEN],
    pub flags: Vec<u8>,
}

struct Entry {
    io: [f64; IO_LEN],
    flags: Vec<u8>,
}

// Only handle validity and copied exchanges live here. Movers and their
// polygon caches remain in MovementOwner's canonical mover bank.
pub(super) struct Copies {
    entries: Vec<Option<Entry>>,
    // lastGround belongs to a collision world, never to the last mover
    // which happened to run. Cast buffers are also private to this scene.
    ground: BTreeMap<usize, GroundCache>,
}
type Result<T> = std::result::Result<T, MovementError>;

impl Copies {
    pub(super) const fn new() -> Self { Self { entries: Vec::new(), ground: BTreeMap::new() } }
}
impl MovementOwner {
    pub(crate) fn native_new_mover(&mut self, world: u32, class: [f64; 21]) -> Result<u32> {
        let len = self.movers.as_ref().map_or(0, |ms| ms.list.len());
        u32::try_from(len).map_err(|_| MovementError::Capacity)?;
        let id = self.new_mover(world as usize, |m| movement::set_class(m, &class));
        let mover = &self.movers.as_ref().unwrap().list[id as usize];
        let mut io = [0.; IO_LEN];
        io[movement::CLASS..movement::CLASS + class.len()].copy_from_slice(&class);
        movement::store(mover, &mut io);
        io[movement::MAX_FLY] = mover.max_fly_height;
        io[movement::GETUP] = mover.getup_time;
        io[movement::DIVE_TIME] = mover.dive_time;
        io[movement::PASS_THROUGH] = mover.pass_through as f64;
        self.native_copies.entries.resize_with(len, || None);
        self.native_copies.entries.push(Some(Entry {
            io,
            flags: Vec::new(),
        }));
        Ok(id)
    }
    fn entry(&self, id: u32) -> Result<&Entry> {
        self.native_copies.entries
            .get(id as usize)
            .and_then(Option::as_ref)
            .ok_or(MovementError::InvalidMover)
    }
    fn entry_mut(&mut self, id: u32) -> Result<&mut Entry> {
        self.native_copies.entries
            .get_mut(id as usize)
            .and_then(Option::as_mut)
            .ok_or(MovementError::InvalidMover)
    }
    pub(crate) fn native_world(&self, id: u32) -> Result<u32> {
        self.entry(id)?;
        Ok(self.movers.as_ref().unwrap().list[id as usize].world as u32)
    }
    pub(crate) fn native_load(&mut self, id: u32, io: [f64; IO_LEN]) -> Result<()> {
        self.entry(id)?;
        let mover = &mut self.movers.as_mut().unwrap().list[id as usize];
        // load validates all state codes before changing any mover field.
        if !movement::load(mover, &io) {
            return Err(MovementError::UnknownState);
        }
        self.entry_mut(id)?.io = io;
        self.io = io;
        Ok(())
    }
    pub(crate) fn native_snapshot(&self, id: u32) -> Result<MovementResult> {
        let entry = self.entry(id)?;
        Ok(MovementResult {
            mover: self.movers.as_ref().unwrap().list[id as usize].clone(),
            io: entry.io,
            flags: entry.flags.clone(),
        })
    }
    pub(crate) fn native_remove(&mut self, id: u32) -> Result<()> {
        self.entry(id)?;
        self.native_copies.entries[id as usize] = None;
        let ms = self.movers.as_mut().unwrap();
        ms.list[id as usize] = Mover::new(0);
        ms.caches[id as usize] = std::array::from_fn(|_| crate::collision::spheres::GatherCache::new(0.75));
        Ok(())
    }
    fn validate_flags(&self, id: u32, s: &State, flags: &[u8]) -> Result<()> {
        if flags.len() != s.worlds[self.native_world(id)? as usize].order.len() {
            return Err(MovementError::InvalidFlags);
        }
        Ok(())
    }
    pub(crate) fn native_tick(
        &mut self,
        id: u32,
        io: [f64; IO_LEN],
        flags: Vec<u8>,
        s: &mut State,
    ) -> Result<MovementResult> {
        self.validate_flags(id, s, &flags)?;
        self.native_load(id, io)?;
        self.native_advance(id, movement::input(&io), io[movement::DT], flags, s)
    }
    pub(crate) fn native_advance(
        &mut self,
        id: u32,
        input: Input,
        dt: f64,
        flags: Vec<u8>,
        s: &mut State,
    ) -> Result<MovementResult> {
        self.validate_flags(id, s, &flags)?;
        let entry = self.native_copies.entries[id as usize].as_mut().unwrap();
        self.flags[..flags.len()].copy_from_slice(&flags);
        entry.flags = flags;
        let ms = self.movers.as_mut().unwrap();
        let mover = &mut ms.list[id as usize];
        let world = mover.world;
        let ground_cache = self.native_copies.ground.entry(world).or_insert_with(GroundCache::default);
        let mut scene = Scene {
            s,
            world,
            flags: &self.flags[..entry.flags.len()],
            gathered: &mut ms.gathered,
            caches: &mut ms.caches[id as usize],
            ground_cache,
        };
        movement::advance_in(mover, &input, dt, &mut scene);
        movement::store(mover, &mut entry.io);
        self.io = entry.io;
        self.native_snapshot(id)
    }
}
