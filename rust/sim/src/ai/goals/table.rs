//! The mission's goal table as the bots read it, and a storage with the
//! 2005 game's table rules.
//!
//! The script host owns the table a mission writes (`AddAIGoal`,
//! `DeleteAIGoal`, `ClearAIGoals`). The bots only read it, through
//! [`GoalTable`], once per event-rule tick after the handler's tick.
//! [`GoalSlots`] implements the game's rules for that table: one table of
//! [`GOAL_SLOTS`] ordered slots shared by every team, a new goal in the lowest
//! free slot, teams 0 to 7, positive weights kept exactly as given.
//!
//! A world owns the allocator shared by its mission writers. A writer gets
//! it before loading a script, including a mission that never installs.
//! Handles are
//! script numbers, exact up to [`MAX_GOAL_HANDLE`]; a table that would pass it
//! refuses new goals.

use crate::battle::rules::events::ObjectHandle;
use crate::gen::Character;
use crate::store::Ref;

/// Slots in the one goal table all teams share.
pub const GOAL_SLOTS: usize = 20;
/// Teams a goal may name: 0 to 7.
pub const GOAL_TEAMS: usize = 8;
/// Largest handle a script number holds exactly; the last one issued.
pub const MAX_GOAL_HANDLE: u32 = 1 << 24;

/// The seven goal kinds a mission can add.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoalKind {
    Conquest,
    Deathmatch,
    Destroy,
    Defend,
    Follow,
    CtfOffense,
    CtfDefense,
}

const KIND_NAMES: [(&[u8], GoalKind); 7] = [
    (b"conquest", GoalKind::Conquest),
    (b"deathmatch", GoalKind::Deathmatch),
    (b"destroy", GoalKind::Destroy),
    (b"defend", GoalKind::Defend),
    (b"follow", GoalKind::Follow),
    (b"ctfoffense", GoalKind::CtfOffense),
    (b"ctfdefense", GoalKind::CtfDefense),
];

impl GoalKind {
    /// A goal name as a mission passes it, ignoring ASCII case.
    pub fn parse(name: &[u8]) -> Option<Self> {
        KIND_NAMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, k)| *k)
    }

    /// Kinds that are meaningless without their first target argument.
    pub fn needs_target(self) -> bool {
        !matches!(self, GoalKind::Conquest | GoalKind::Deathmatch)
    }
}

/// One optional argument of a goal, as the host resolved it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GoalArg<'a> {
    /// Omitted or nil.
    None,
    /// A script string, exact bytes: an object, region, flag or post name.
    Name(&'a [u8]),
    /// A character the script passed by number.
    Character(Ref<Character>),
    /// An object the script passed as a handle.
    Object(&'a ObjectHandle),
    /// A saved mission prop follows its placement across gameplay lives.
    Placement(&'a ObjectHandle),
}

impl GoalArg<'_> {
    pub fn is_none(&self) -> bool { matches!(self, GoalArg::None) }
}

/// One occupied slot of the table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GoalRow<'a> {
    /// The slot, 0 to `GOAL_SLOTS - 1`.
    pub slot: u8,
    /// What `AddAIGoal` returned. Unique for the life of the table.
    pub handle: u32,
    pub team: u8,
    pub kind: GoalKind,
    /// Finite and positive, exactly the script's number.
    pub weight: f32,
    /// The fourth argument.
    pub target: GoalArg<'a>,
    /// The fifth argument.
    pub extra: GoalArg<'a>,
}

/// A read-only view of the mission's goal table. The writer (the script
/// host) owns the rows, their validation and the handle numbers.
pub trait GoalTable {
    /// Changes whenever a row is added, deleted or cleared.
    fn revision(&self) -> u32;
    /// Occupied rows, at most `GOAL_SLOTS`.
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool { self.len() == 0 }
    /// Row `index` of `0..len()`, in ascending slot order.
    fn row(&self, index: usize) -> GoalRow<'_>;
    /// The handle the next accepted goal would get: past every handle this
    /// table issued, and never below the handle it was bound to start at.
    fn next_handle(&self) -> u32;
    fn handles(&self) -> Option<&GoalHandles> { None }
}

/// The table of rules that give no goals.
pub struct NoGoals;

impl GoalTable for NoGoals {
    fn revision(&self) -> u32 { 0 }
    fn len(&self) -> usize { 0 }
    fn row(&self, index: usize) -> GoalRow<'_> { panic!("no goal row {index}") }
    fn next_handle(&self) -> u32 { 1 }
}

/// An empty goal table.
pub static NO_GOALS: NoGoals = NoGoals;

/// Whether `target` is a first argument `kind` accepts, as the script host
/// checks it: a follower follows a character or an object, a flag offense
/// names its area, and every other kind that needs a target takes any.
pub fn accepts_target(kind: GoalKind, target: &GoalArg<'_>) -> bool {
    match kind {
        GoalKind::Follow => matches!(target, GoalArg::Character(_) | GoalArg::Object(_) | GoalArg::Placement(_)),
        GoalKind::CtfOffense => matches!(target, GoalArg::Name(_)),
        other => !other.needs_target() || !target.is_none(),
    }
}

/// An owned argument of a stored goal.
#[derive(Clone, Debug, PartialEq)]
pub enum StoredArg {
    None,
    Name(Box<[u8]>),
    Character(Ref<Character>),
    Object(ObjectHandle),
    Placement(ObjectHandle),
}

impl StoredArg {
    fn view(&self) -> GoalArg<'_> {
        match self {
            StoredArg::None => GoalArg::None,
            StoredArg::Name(name) => GoalArg::Name(name),
            StoredArg::Character(c) => GoalArg::Character(*c),
            StoredArg::Object(o) => GoalArg::Object(o),
            StoredArg::Placement(o) => GoalArg::Placement(o),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Stored {
    handle: u32,
    team: u8,
    kind: GoalKind,
    weight: f32,
    target: StoredArg,
    extra: StoredArg,
}

/// The exact script numbers available to all goal writers in one world.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoalHandles(std::rc::Rc<std::cell::Cell<u32>>);

impl Default for GoalHandles {
    fn default() -> Self { Self(std::rc::Rc::new(std::cell::Cell::new(1))) }
}

impl GoalHandles {
    /// Allocate after validating a goal and finding an available slot.
    pub fn issue(&self) -> Option<u32> {
        let next = self.next();
        if next > MAX_GOAL_HANDLE { return None; }
        self.0.set(next + 1);
        Some(next)
    }

    pub fn next(&self) -> u32 { self.0.get() }
    pub fn same_world(&self, other: &Self) -> bool { std::rc::Rc::ptr_eq(&self.0, &other.0) }
}

/// The game's goal table rules over owned storage.
#[derive(Clone, Debug, PartialEq)]
pub struct GoalSlots {
    slots: [Option<Stored>; GOAL_SLOTS],
    occupied: usize,
    handles: GoalHandles,
    revision: u32,
}

impl Default for GoalSlots {
    fn default() -> Self { Self::new() }
}

impl GoalSlots {
    /// An independent table for a single writer.
    pub fn new() -> Self { Self::with_handles(GoalHandles::default()) }

    /// A mission table, sharing the world's allocator from its first write.
    pub fn with_handles(handles: GoalHandles) -> Self {
        Self { slots: Default::default(), occupied: 0, handles, revision: 0 }
    }

    #[cfg(test)]
    pub(crate) fn bind(&mut self, first: u32) { self.handles.0.set(first.max(self.handles.next())); }

    /// `AddAIGoal(team, kind, weight[, target[, extra]])`: the handle of the
    /// new goal, or `None` when the call is refused with the script host's
    /// checks: a team outside 0 to 7, a weight that is not finite and
    /// positive, an unknown kind, a first argument the kind does not accept
    /// ([`accepts_target`]), a full table, or no exact handle left.
    pub fn add(&mut self, team: u8, kind: &[u8], weight: f32, target: StoredArg, extra: StoredArg) -> Option<u32> {
        let kind = GoalKind::parse(kind)?;
        if usize::from(team) >= GOAL_TEAMS || !weight.is_finite() || weight <= 0.0 {
            return None;
        }
        if !accepts_target(kind, &target.view()) {
            return None;
        }
        let slot = self.slots.iter().position(Option::is_none)?;
        let handle = self.handles.issue()?;
        self.slots[slot] = Some(Stored { handle, team, kind, weight, target, extra });
        self.occupied += 1;
        self.revision = self.revision.wrapping_add(1);
        Some(handle)
    }

    /// `DeleteAIGoal(handle)`: false for a handle that is not in the table.
    pub fn delete(&mut self, handle: u32) -> bool {
        let Some(slot) = self.slots.iter().position(|s| s.as_ref().is_some_and(|g| g.handle == handle)) else {
            return false;
        };
        self.slots[slot] = None;
        self.occupied -= 1;
        self.revision = self.revision.wrapping_add(1);
        true
    }

    /// `ClearAIGoals(team)`: deletes every goal of that team.
    pub fn clear(&mut self, team: u8) {
        let mut removed = 0;
        for slot in &mut self.slots {
            if slot.as_ref().is_some_and(|g| g.team == team) {
                *slot = None;
                removed += 1;
            }
        }
        if removed > 0 {
            self.occupied -= removed;
            self.revision = self.revision.wrapping_add(1);
        }
    }
}

impl GoalTable for GoalSlots {
    fn revision(&self) -> u32 { self.revision }
    fn len(&self) -> usize { self.occupied }
    fn next_handle(&self) -> u32 { self.handles.next() }
    fn handles(&self) -> Option<&GoalHandles> { Some(&self.handles) }
    fn row(&self, index: usize) -> GoalRow<'_> {
        let (slot, goal) = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|g| (i, g)))
            .nth(index)
            .expect("goal row index below len");
        GoalRow {
            slot: slot as u8,
            handle: goal.handle,
            team: goal.team,
            kind: goal.kind,
            weight: goal.weight,
            target: goal.target.view(),
            extra: goal.extra.view(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A revision is a change token: at the largest 32-bit number the next change wraps to zero
    /// instead of panicking, and the token still differs from the one before it.
    #[test]
    fn a_revision_at_the_bound_wraps_and_still_differs() {
        let mut goals = GoalSlots::new();
        goals.revision = u32::MAX;
        let handle = goals.add(1, b"Deathmatch", 1.0, StoredArg::None, StoredArg::None).unwrap();
        assert_eq!(goals.revision(), 0);
        assert!(goals.delete(handle));
        assert_eq!(goals.revision(), 1);
        goals.revision = u32::MAX;
        goals.clear(1);
        assert_eq!(goals.revision(), u32::MAX, "nothing to clear: the token does not move");
    }
}
