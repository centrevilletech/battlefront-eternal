//! The operations the goal bots of the 2005 game's modes perform on this
//! controller. Only the goal bots call them; a 2004 battle never does.
//!
//! A goal bot carries a base order in its command queue: a wait order the
//! goal bots steer, or the objective order for a command post. Everything
//! the released controller queues for a bot (a fight, a squad order,
//! boarding, recovery) keeps its place and its turn:
//!
//! - a changed base order replaces the base order where it sits;
//! - a goal attack on a designated target enters as an interrupt that
//!   returns to the base order, and waits while the queue has no room for it
//!   and its saved base order, so no accepted order is ever pushed out;
//! - an objective publication updates the pending base in the live queue.
//!
//! While the bot's own wait order runs, the goal bots alone send its route
//! destination and its look direction; the released wait agent then looks
//! around that direction as it looks around a spawn direction.

use super::controller::{goal, CommandQueue, SourceGoal, COMMAND_CAPACITY};
use super::agents::AgentKind;
use super::world::{AiWorld, Draws, EntRef, GotoMode, Policy, UKey};
use super::{helpers, Core};
use super::super::nav::math::Vec3;

/// The `mode` of a base order the goal bots wrote. The released controller
/// never gives a wait or objective order this mode and never reads it.
pub const GOAL_ORDER: f64 = -12.0;
/// `ref_b` of a goal attack. The released controller never reads `ref_b` of a
/// combat order.
const GOAL_ATTACK: EntRef = EntRef { unit: UKey::MAX, generation: GOAL_ORDER };

/// A goal bot's base order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Base {
    /// The wait order the goal bots steer.
    Wait,
    /// The objective order for a command post.
    Objective(usize),
}

impl Base {
    fn order(self) -> SourceGoal {
        let mut order = match self {
            Base::Wait => SourceGoal::new(goal::WAIT),
            Base::Objective(post) => SourceGoal::objective(Some(post)),
        };
        order.mode = GOAL_ORDER;
        order
    }
}

/// An order the goal bots wrote as a base order.
pub fn is_goal_base(order: &SourceGoal) -> bool {
    order.mode == GOAL_ORDER && (order.kind == goal::WAIT || order.kind == goal::OBJECTIVE)
}

/// A goal attack on a designated target.
pub fn is_goal_attack(order: &SourceGoal) -> bool { order.kind == goal::COMBAT && order.ref_b == Some(GOAL_ATTACK) }

/// A base order: the goal bots' own, or an objective order published by the
/// released objective flow.
fn is_base(order: &SourceGoal) -> bool { is_goal_base(order) || order.kind == goal::OBJECTIVE }

/// Base orders that make the controller do the same thing.
fn same_base(a: &SourceGoal, b: &SourceGoal) -> bool {
    a.kind == b.kind && if a.kind == goal::OBJECTIVE { a.post == b.post } else { is_goal_base(a) == is_goal_base(b) }
}

/// What `set_base` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Kept,
    /// A pending base order was replaced where it sat.
    Replaced,
    /// The base order runs next.
    Next,
    /// The base order runs after the pending orders.
    Queued,
    /// No room; tried again on the next pass.
    Full,
}

/// Make `base` the bot's base order without moving any other order.
pub fn set_base(queue: &mut CommandQueue, base: Base) -> Change {
    let base = base.order();
    if let Some(pending) = queue.queue.iter_mut().find(|order| is_base(order)) {
        if same_base(pending, &base) {
            return Change::Kept;
        }
        *pending = base;
        return Change::Replaced;
    }
    let running_base = is_base(&queue.current) && !queue.current_done;
    if running_base && same_base(&queue.current, &base) {
        return Change::Kept;
    }
    let idle = queue.current.kind == goal::NONE || queue.current_done || running_base;
    if idle && queue.queue.is_empty() && queue.prepend_one(base) {
        return Change::Next;
    }
    if queue.append(base) { Change::Queued } else { Change::Full }
}

/// Replace the pending base in place. The queue's spare inline slot can
/// hold the publication behind a full set of accepted interrupts.
pub(crate) fn publish_base(queue: &mut CommandQueue, post: usize) {
    let objective = SourceGoal::objective(Some(post));
    if let Some(base) = queue.queue.iter_mut().find(|order| is_base(order)) {
        *base = objective;
    } else {
        queue.queue.push(objective);
    }
    if is_base(&queue.current) || queue.current.kind == goal::NONE || queue.current_done {
        queue.latch = true;
    }
}

impl Core {
    pub(crate) fn goal_publications(&mut self, enabled: bool) {
        self.objective_publication = if enabled { Self::event_objective } else { Self::released_objective };
    }

    fn event_objective(&mut self, ci: usize, post: usize) {
        let c = &mut self.controllers[ci];
        publish_base(&mut c.commands, post);
        c.published = true;
    }
}

/// Attack a designated target through the released combat order, as an
/// interrupt that returns to the base order. Never pushes out a pending
/// order: with no room for the attack and its saved base order, it waits.
pub fn start_attack(queue: &mut CommandQueue, target: EntRef) -> Attack {
    let underway = |o: &SourceGoal| is_goal_attack(o) && o.ref_a == Some(target);
    if (underway(&queue.current) && !queue.current_done) || queue.queue.iter().any(underway) {
        return Attack::Underway;
    }
    if queue.latch || queue.queue.len() + 2 > COMMAND_CAPACITY {
        return Attack::Deferred;
    }
    let mut order = SourceGoal::combat(super::controller::combat_mode::TARGET, None, Some(target));
    order.ref_b = Some(GOAL_ATTACK);
    let saved = queue.current_copy();
    if queue.prepend_two(saved, order) { Attack::Started } else { Attack::Deferred }
}

/// End the goal attacks: pending ones leave the queue, a running one hands
/// over to the next queued order (the base order when nothing else waits)
/// at the next service.
pub fn cancel_attacks(queue: &mut CommandQueue) {
    queue.queue.retain(|o| !is_goal_attack(o));
    if is_goal_attack(&queue.current) {
        queue.current = SourceGoal::new(goal::NONE);
        queue.current_done = true;
        if queue.queue.is_empty() {
            queue.queue.push(Base::Wait.order());
        }
        queue.latch = true;
    }
}

/// Remove every order the goal bots wrote. True when there was one.
pub fn retire_orders(queue: &mut CommandQueue) -> bool {
    let before = queue.queue.len();
    queue.queue.retain(|order| !is_goal_base(order) && !is_goal_attack(order));
    let mut removed = queue.queue.len() != before;
    if is_goal_base(&queue.current) || is_goal_attack(&queue.current) {
        queue.current = SourceGoal::new(goal::NONE);
        queue.current_done = true;
        removed = true;
    }
    removed
}

/// What the bot is running now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Running {
    /// The goal bots' wait order, with its agent entered at this time.
    Wait { since: f64 },
    /// An objective order (the goal bots' or a published one).
    Objective,
    /// A goal attack.
    GoalAttack,
    /// A fight the released controller started.
    Fight,
    /// The movement helper is trying to free the body.
    Recovery,
    /// Another interrupt, such as a squad order or boarding.
    Other,
    /// Nothing, or a finished order.
    Idle,
}

/// The route the goal bots last sent a bot, per wait agent.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Steering {
    pub dest: Option<Vec3>,
    since: Option<f64>,
}

/// What `steer` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Steer {
    /// The bot is not running its wait order; nothing was sent.
    Inactive,
    Moving { arrived: bool, failed: bool },
    Stopped,
}

/// What `attack` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attack {
    Started,
    /// Already pending or running.
    Underway,
    /// No room in the queue yet.
    Deferred,
}

/// A living bot the goal bots may steer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BotView {
    pub ci: usize,
    pub unit: UKey,
    pub generation: f64,
    /// Seated in a vehicle or turret: the released seat policies drive it.
    pub mounted: bool,
}

/// The goal bots' narrow view of the controller.
pub struct GoalCore<'a> {
    core: &'a mut Core,
}

impl<'a> GoalCore<'a> {
    pub fn new(core: &'a mut Core) -> Self { Self { core } }

    pub fn characters(&self) -> usize { self.core.character_count }
    /// The controller's identity while it lives: goal bots that wrote orders
    /// through it retire through it, and through no other.
    pub fn identity(&self) -> usize { std::ptr::from_ref::<Core>(self.core) as usize }
    pub fn now<W: AiWorld + ?Sized>(&self, w: &W) -> f64 { self.core.now(w) }
    pub fn relation(&self, a: f64, b: f64) -> f64 { self.core.relation(a, b) }

    /// The bot bound to `character`, when it has a living body and is no
    /// player.
    pub fn bot<W: AiWorld + ?Sized>(&self, w: &W, character: usize) -> Option<BotView> {
        let ci = self.core.controller_of_character(character)?;
        let c = &self.core.controllers[ci];
        let unit = c.unit?;
        let s = w.unit_scalars(unit);
        if w.character(character).human || !s.alive || c.generation != s.generation {
            return None;
        }
        Some(BotView { ci, unit, generation: s.generation, mounted: c.policy != Policy::None || s.has_lease })
    }

    /// The bot's current combat target.
    pub fn combat_target(&self, ci: usize) -> Option<EntRef> { self.core.controllers[ci].target }
    /// Monotonic count of serviced think ticks completed by `ci`.
    pub fn think_count(&self, ci: usize) -> u32 { self.core.controllers[ci].think_count }

    pub fn running(&self, ci: usize) -> Running {
        let c = &self.core.controllers[ci];
        let current = &c.commands.current;
        if c.commands.current_done || current.kind == goal::NONE {
            return Running::Idle;
        }
        if is_goal_attack(current) {
            return Running::GoalAttack;
        }
        if current.kind == goal::COMBAT {
            return Running::Fight;
        }
        if current.kind == goal::RECOVERY { return Running::Recovery; }
        if current.kind == goal::OBJECTIVE {
            return Running::Objective;
        }
        if is_goal_base(current) {
            if let Some(agent) = c.agent.as_ref() {
                if let AgentKind::Wait { since, .. } = agent.kind {
                    if agent.fsm.state == 2.0 && agent.goal.is_some_and(|g| is_goal_base(&g)) {
                        return Running::Wait { since };
                    }
                }
            }
            return Running::Idle;
        }
        Running::Other
    }

    pub fn set_base(&mut self, ci: usize, base: Base) -> Change { set_base(&mut self.core.controllers[ci].commands, base) }

    /// Request a jet or force-jump on `ci` along `vector`.
    pub fn request_jet<W: AiWorld + ?Sized>(&mut self, w: &mut W, ci: usize, vector: Vec3) {
        self.core.request_jet(w, ci, vector);
    }

    /// Whether a jet request is already pending on `ci`.
    pub fn jet_pending(&self, ci: usize) -> bool {
        let c = &self.core.controllers[ci];
        c.jet_pending || c.nav_control.as_ref().is_some_and(|n| n.jet_pending)
    }

    /// Send the bot's wait agent toward `to` (or stop it) and point its look
    /// around `facing`, when the wait order runs.
    pub fn steer<W: AiWorld + helpers::World + ?Sized>(
        &mut self, w: &mut W, ci: usize, state: &mut Steering, to: Option<Vec3>, facing: Option<Vec3>, reroute: f64,
    ) -> Steer {
        let Running::Wait { since } = self.running(ci) else {
            return Steer::Inactive;
        };
        let fresh = state.since.is_none_or(|s| s.to_bits() != since.to_bits());
        let position = self.core.controllers[ci].unit.map(|u| AiWorld::unit_scalars(w, u).position);
        let result = match to {
            Some(point) => {
                let far = state.dest.is_none_or(|d| horizontal(d, point) > reroute);
                if fresh || far {
                    self.core.helper_goto(w, ci, point, GotoMode::Route);
                    state.dest = Some(point);
                    if let Some(AgentKind::Wait { look_point, .. }) = self.core.controllers[ci].agent.as_mut().map(|a| &mut a.kind) {
                        *look_point = point;
                    }
                }
                let agent = self.core.controllers[ci].agent.as_ref();
                Steer::Moving {
                    arrived: agent.is_some_and(|a| a.helper_arrived()),
                    failed: agent.is_some_and(|a| a.helper_failed()),
                }
            }
            None => {
                if state.dest.is_some() && !fresh {
                    self.core.helper_stop(w, ci);
                }
                state.dest = None;
                Steer::Stopped
            }
        };
        state.since = Some(since);
        // The released look picks stay within 30 degrees of this direction.
        let toward = to.or(facing).zip(position).map(|(t, p)| Vec3::new(t.x - p.x, 0.0, t.z - p.z));
        if let Some(dir) = toward.filter(|d| d.x * d.x + d.z * d.z > 1e-6) {
            let c = &mut self.core.controllers[ci];
            c.home_direction = super::vec::normalize(dir);
            c.home_facing = true;
        }
        result
    }

    /// See [`start_attack`].
    pub fn attack(&mut self, ci: usize, target: EntRef) -> Attack { start_attack(&mut self.core.controllers[ci].commands, target) }

    /// See [`cancel_attacks`].
    pub fn cancel_attack<W: AiWorld + ?Sized>(&mut self, world: &mut W, draws: &mut dyn Draws, ci: usize) {
        if is_goal_attack(&self.core.controllers[ci].commands.current) {
            self.core.exit_agent(world, draws, ci);
            self.core.controllers[ci].target = None;
        }
        cancel_attacks(&mut self.core.controllers[ci].commands);
    }

    /// Hand a bot back to the released controller: every order the goal
    /// bots wrote leaves its queue, and a bot they steered or tagged is bound
    /// afresh, as after a respawn, so no route, look or agent of theirs stays.
    pub fn retire<W: AiWorld + helpers::World + ?Sized>(&mut self, w: &mut W, d: &mut dyn Draws, character: usize, steered: bool) {
        let Some(ci) = self.core.controller_of_character(character) else { return };
        let tagged = retire_orders(&mut self.core.controllers[ci].commands);
        if !(tagged || steered) || self.core.controllers[ci].unit.is_none() {
            return;
        }
        self.core.helper_stop(w, ci);
        self.core.died(w, d, character);
        self.core.spawned(w, d, character);
    }
}

fn horizontal(a: Vec3, b: Vec3) -> f64 {
    let (x, z) = (a.x - b.x, a.z - b.z);
    v8math::sqrt(x * x + z * z)
}
