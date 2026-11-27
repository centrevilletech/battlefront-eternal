//! Which goal each bot carries out.
//!
//! The 2005 game links each bot to one goal of its own team. A bot without a
//! goal takes the active goal of its team with the lowest ratio of assigned
//! bots to weight; the earlier table slot wins a tie. Adding a goal or
//! raising a weight never takes a bot away from a valid goal: the new goal
//! fills as bots become free. A bot is freed when its goal is deleted or
//! suspended, when it dies, or when it is on another team than its goal.
//!
//! A pass over a wave of bots first frees every link that is no longer valid
//! ([`Assignments::keep`]) and only then fills the vacancies
//! ([`Assignments::fill`]), so a bot choosing a goal sees the counts the wave
//! ends with, not the counts of bots that are leaving.

use super::table::{GoalKind, GOAL_SLOTS};

/// The bots' copy of one table row with its live state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Goal {
    pub slot: u8,
    pub handle: u32,
    pub team: u8,
    pub kind: GoalKind,
    pub weight: f32,
    /// False while the goal's subject is unavailable.
    pub active: bool,
    /// Bots linked to this goal now.
    pub assigned: u32,
}

/// A bot's link to a goal: the slot and the handle that occupied it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Assigned {
    pub slot: u8,
    pub handle: u32,
}

/// One row of a table synchronisation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GoalSpec {
    pub slot: u8,
    pub handle: u32,
    pub team: u8,
    pub kind: GoalKind,
    pub weight: f32,
}

#[derive(Default)]
pub struct Assignments {
    goals: [Option<Goal>; GOAL_SLOTS],
    of: Vec<Option<Assigned>>,
}

impl Assignments {
    /// Room for `characters` bots, reserved once.
    pub(crate) fn try_reserve(&mut self, characters: usize) -> Option<()> {
        self.of.try_reserve_exact(characters.saturating_sub(self.of.len())).ok()
    }

    pub fn reserve(&mut self, characters: usize) {
        if self.of.len() < characters {
            self.of.resize(characters, None);
        }
    }

    pub fn goal(&self, slot: u8) -> Option<&Goal> { self.goals.get(usize::from(slot))?.as_ref() }
    pub fn goals(&self) -> impl Iterator<Item = &Goal> { self.goals.iter().flatten() }
    pub fn of(&self, character: usize) -> Option<Assigned> { self.of.get(character).copied().flatten() }

    /// The goal a bot is linked to, if that goal still exists.
    pub fn goal_of(&self, character: usize) -> Option<&Goal> {
        let a = self.of(character)?;
        self.goal(a.slot).filter(|g| g.handle == a.handle)
    }

    /// Replace the goals with the table's rows. A goal whose slot keeps the
    /// same handle keeps its bots and its active state; bots of goals that
    /// left the table are freed.
    pub fn sync(&mut self, rows: impl IntoIterator<Item = GoalSpec>) {
        let mut next: [Option<Goal>; GOAL_SLOTS] = Default::default();
        for row in rows {
            let Some(cell) = next.get_mut(usize::from(row.slot)) else { continue };
            let kept = self.goals[usize::from(row.slot)].filter(|g| g.handle == row.handle);
            *cell = Some(Goal {
                slot: row.slot,
                handle: row.handle,
                team: row.team,
                kind: row.kind,
                weight: row.weight,
                active: kept.is_none_or(|g| g.active),
                assigned: 0,
            });
        }
        self.goals = next;
        for link in &mut self.of {
            let Some(a) = *link else { continue };
            match self.goals[usize::from(a.slot)].as_mut().filter(|g| g.handle == a.handle) {
                Some(goal) => goal.assigned += 1,
                None => *link = None,
            }
        }
    }

    /// Suspend or resume a goal. Suspending frees its bots; resuming does
    /// not take bots from other goals.
    pub fn set_active(&mut self, slot: u8, active: bool) {
        let Some(goal) = self.goals.get_mut(usize::from(slot)).and_then(Option::as_mut) else { return };
        if goal.active == active {
            return;
        }
        goal.active = active;
        if !active {
            let handle = goal.handle;
            goal.assigned = 0;
            for link in &mut self.of {
                if link.is_some_and(|a| a.slot == slot && a.handle == handle) {
                    *link = None;
                }
            }
        }
    }

    /// Free a bot from its goal.
    pub fn release(&mut self, character: usize) {
        let Some(link) = self.of.get_mut(character) else { return };
        let Some(a) = link.take() else { return };
        if let Some(goal) = self.goals[usize::from(a.slot)].as_mut().filter(|g| g.handle == a.handle) {
            goal.assigned -= 1;
        }
    }

    /// Keep a bot's link while its goal is still in the table, active and on
    /// the bot's team; free it otherwise. `team` is `None` for a bot that may
    /// carry out no goal (no living bot, or a team outside 0 to 7).
    pub fn keep(&mut self, character: usize, team: Option<u8>) -> Option<Assigned> {
        if let (Some(goal), Some(team)) = (self.goal_of(character), team) {
            if goal.active && goal.team == team {
                return self.of(character);
            }
        }
        self.release(character);
        None
    }

    /// The goal a living bot of `team` carries out: its current goal when
    /// that is still active and on its team, else the active goal of its
    /// team with the lowest assigned-to-weight ratio, the earlier slot
    /// winning a tie. `None` when the team has no active goal.
    pub fn assign(&mut self, character: usize, team: u8) -> Option<Assigned> {
        self.keep(character, Some(team));
        self.fill(character, team)
    }

    /// A free bot of `team` takes the active goal of its team with the lowest
    /// assigned-to-weight ratio, the earlier slot winning a tie. A linked bot
    /// keeps its link.
    pub fn fill(&mut self, character: usize, team: u8) -> Option<Assigned> {
        if let Some(link) = self.of(character) {
            return Some(link);
        }
        let mut best: Option<&Goal> = None;
        for goal in self.goals.iter().flatten() {
            if !goal.active || goal.team != team {
                continue;
            }
            // assigned / weight below the best's, by exact cross
            // multiplication: counts and f32 weights fit a double exactly.
            let better = best.is_none_or(|b| {
                f64::from(goal.assigned) * f64::from(b.weight) < f64::from(b.assigned) * f64::from(goal.weight)
            });
            if better {
                best = Some(goal);
            }
        }
        let best = *best?;
        let link = Assigned { slot: best.slot, handle: best.handle };
        self.goals[usize::from(best.slot)].as_mut().unwrap().assigned += 1;
        if character >= self.of.len() {
            self.of.resize(character + 1, None);
        }
        self.of[character] = Some(link);
        Some(link)
    }
}
