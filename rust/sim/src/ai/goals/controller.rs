//! The goal bots: a controller beside the released bot controller, used only
//! by battles whose rules give the bots a goal table.
//!
//! Once per event-rule tick, after the rules handler ran, the controller
//! reads the goal table and walks the battle's characters twice. One walk
//! frees every link that is no longer valid (a bot that died, changed team
//! or whose goal left the table or was suspended); the next fills the
//! vacancies (see [`super::assign`]) and carries each goal out through the
//! controller's goal operations ([`crate::ai::control::goal_orders`]): a base
//! order in the command queue and, while the released wait agent runs that
//! order, a route destination and a look direction. Everything else stays the
//! released controller's: perception, combat, cover, melee, squad orders,
//! boarding, recovery from being stuck. When the bot sees an enemy, the
//! released combat interrupt takes over; when the fight ends, the bot returns
//! to its goal.
//!
//! The 2005 game's own order controllers for these goals are known only in
//! outline (a combat order with no fixed target for Deathmatch, a guard that
//! returns to its post after a fight for Defend, a check of the subject for
//! Destroy, a destination recomputed from the subject's motion for Follow).
//! How a bot finds its next enemy, how far guards stand from their subject,
//! and every distance and cadence below are this engine's choices.

mod soldiers;
pub use soldiers::SoldierInputs;

use super::assign::{Assigned, Assignments, Goal, GoalSpec};
use super::census;
use super::melee::{Binding, BlockMode, MeleeInput, Wanted};
use super::reach::{route, Route};
use super::regions::GoalRegions;
use super::table::{GoalArg, GoalKind, GoalTable, GOAL_SLOTS, GOAL_TEAMS};
use crate::ai::control::goal_orders::{Attack, Base, BotView, GoalCore, Running, Steer, Steering};
use crate::ai::control::world::{AiWorld, Characters, Draws, EntRef, UKey};
use crate::ai::nav::math::Vec3;
use crate::ai::owned::{arsenals::Mounted, entities::Entity, rows::row_context, weapons::Changes, world::{Services, World}};
use crate::battle::rules::events::{ObjectHandle, RuleError};
use crate::battle::rules::objects::{name_order, MissionObjects, ObjectServices};
use crate::gen::{Character, F3UnitKey, F4ObjectKind, SpawnPost, Store};
use crate::store::Ref;

/// Seconds between target choices of a hunting bot: this plus 0.2 s per
/// character index modulo 9, so bots of one wave choose on different ticks.
const HUNT_PICK: f64 = 2.0;
/// Seconds between the cheap checks of a moving bot.
const CHECK: f64 = 0.5;
/// Seconds a walking hunter can spend without closing on its enemy.
const APPROACH_WINDOW: f64 = 4.0;
/// Seconds of walking toward one enemy without engaging it before the bot
/// gives it up: this plus `PURSUIT_PER_METRE` for each metre of the walk the
/// bot first saw (the navigation's route length, else the straight line).
/// Engaging that enemy starts its time again.
const PURSUIT_BASE: f32 = 6.0;
const PURSUIT_PER_METRE: f32 = 0.5;
/// A bot that gave up on every enemy it could hunt patrols at least this
/// long before it tries them all again.
const RETRY: f64 = 10.0;
/// Metres added to an enemy's distance for each ally already hunting it.
const CROWD: f64 = 15.0;
/// Enemies, nearest first, a choice considers; the navigation is asked about
/// those it has not answered for this bot life and enemy life.
const ROUTE_TRIES: usize = 3;
/// Guards stand on a ring this far from their subject.
const GUARD_RING: f64 = 4.0;
/// A guard farther than this from its spot walks back.
const GUARD_LEASH: f64 = 6.0;
const GUARD_CHECK: f64 = 1.5;
/// Followers stop within this distance of a slow subject.
const FOLLOW_NEAR: f64 = 5.0;
/// Seconds of the subject's velocity added to its position.
const FOLLOW_LEAD: f64 = 0.5;
/// A Destroy goal's subject within this distance is attacked.
const ATTACK_RANGE: f64 = 30.0;
/// Seconds before a goal attack that ended is started again.
const ATTACK_RETRY: f64 = 2.0;
/// Patrol legs when no enemy can be hunted.
const PATROL_RADIUS: f64 = 12.0;
const PATROL_LEG: f64 = 6.0;
/// A destination must move this far before the bot takes a new route.
const REROUTE: f64 = 3.0;
/// Seconds between refreshes of the goals' subjects (their positions).
const SUBJECT_CHECK: f64 = 0.5;
/// Passes between new lookups of unchanged goal arguments (half a second at
/// 60 Hz), so a rebuilt named object is found again.
const RESOLVE_PASSES: u32 = 30;
/// Maximum horizontal offset applied to a sampled enemy position under Deathmatch.
const DEATHMATCH_ENEMY_JITTER: f64 = 10.0;
/// Fallback horizontal wander span when no other connected hub is available.
const DEATHMATCH_FALLBACK_SPAN: f64 = 100.0;
/// Horizontal arrival threshold for a static Deathmatch walk destination.
const DEATHMATCH_ARRIVAL: f64 = 2.5;
/// Retry delay in seconds after a Deathmatch walk route fails.
const DEATHMATCH_FAIL_RETRY: f64 = 0.5;
/// Seconds between Deathmatch travel jump checks for jet-capable bots.
const DEATHMATCH_JUMP_INTERVAL: f64 = 5.0;
/// Random extra wait in seconds when jet fuel is at or below its minimum border.
const DEATHMATCH_JUMP_LOW_FUEL_SPAN: f64 = 10.0;
/// A combat target holding a melee weapon this close is blocked.
const BLOCK_REACH: f64 = 3.5;

/// A goal argument as the rules seam resolved it.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Resolved {
    #[default]
    None,
    Character(Ref<Character>),
    /// A mission object at the life the argument names now: a name follows
    /// its placement to its current life, a handle names one life.
    Object { handle: ObjectHandle, alive: bool },
    /// A name no mission object has; the bots look for a post or a region.
    Name,
    /// An ambiguous name, a stale handle, or a placement with no life now.
    Missing,
}

/// Where a goal's subject is now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Subject {
    pub position: Vec3,
    pub velocity: Vec3,
    pub alive: bool,
    /// The combat target the released controller can attack, when there is one.
    pub entity: Option<EntRef>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Plan {
    Idle,
    Hunt,
    Roam { dest: Vec3 },
    Patrol { center: Vec3, leg: u8, until: f64 },
    Guard { spot: Vec3, settled: bool },
    Follow,
    Attack,
    Objective,
}

/// The enemy a bot hunts: its character and the life it had when chosen.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Hunt {
    target: usize,
    life: u32,
    unit: UKey,
    generation: f64,
}

/// One bot's pursuit of one enemy in one round: how long it walked toward
/// that enemy's life without engaging it, how long it may, and the
/// navigation's answer for the walk.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pursuit {
    round: u32,
    life: u32,
    spent: f32,
    budget: f32,
    /// Not asked yet (NaN), no answer (-1), no route (infinite), or the
    /// route's length in metres.
    route: f32,
    /// Origin of a pick for which every route question was rejected.
    rejected_at: Option<Vec3>,
}

impl Default for Pursuit {
    fn default() -> Self { Pursuit { round: 0, life: 0, spent: 0.0, budget: 0.0, route: f32::NAN, rejected_at: None } }
}

impl Pursuit {
    fn fresh(round: u32, life: u32) -> Self { Pursuit { round, life, ..Pursuit::default() } }

    /// The walk this pair's ranking and time use.
    fn walk(&self, straight: f64) -> f64 {
        if self.route.is_finite() && self.route >= 0.0 {
            f64::from(self.route)
        } else {
            straight
        }
    }
}

#[derive(Clone, Debug)]
struct Bot {
    /// The unit and its life this record steers.
    life: Option<(UKey, f64)>,
    /// The bot as this pass's freeing walk saw it.
    view: Option<BotView>,
    team: Option<u8>,
    plan: Plan,
    steering: Steering,
    next_check: f64,
    next_pick: f64,
    next_jump: f64,
    route_ready: [u32; ROUTE_TRIES],
    checking_unknown: bool,
    hunt: Option<Hunt>,
    /// Position, time and range when the hunter last closed on this enemy.
    anchor: Option<(Vec3, f64, f64)>,
    /// The round of pursuits; a new life or a retry starts a new one.
    round: u32,
    round_since: f64,
    goal: Option<Assigned>,
    attacking: Option<EntRef>,
    next_attack: f64,
    sample_slot: Option<usize>,
}

impl Default for Bot {
    fn default() -> Self {
        Bot {
            life: None,
            view: None,
            team: None,
            plan: Plan::Idle,
            steering: Steering::default(),
            next_check: 0.0,
            next_pick: 0.0,
            next_jump: 0.0,
            route_ready: [0; ROUTE_TRIES],
            checking_unknown: false,
            hunt: None,
            anchor: None,
            round: 1,
            round_since: f64::NEG_INFINITY,
            goal: None,
            attacking: None,
            next_attack: f64::NEG_INFINITY,
            sample_slot: None,
        }
    }
}

/// Route questions already issued on this fixed simulation tick.
#[derive(Default)]
struct RouteBudget {
    tick: u32,
    used: u8,
}

impl Bot {
    /// Each of the three queries becomes available again after one fixed second.
    fn take_route_credit(&mut self, tick: u32) -> bool {
        if tick < self.route_ready[0] { return false; }
        self.route_ready.rotate_left(1);
        self.route_ready[ROUTE_TRIES - 1] = tick.saturating_add(60);
        true
    }

    fn ask_route<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(
        &mut self, budget: &mut RouteBudget, world: &mut World<'_, '_, C, M, S>, character: usize, from: Vec3, to: Vec3,
    ) -> Option<Route> {
        let tick = crate::units::f3::tick_in(&world.rows.w.units) as u32;
        if budget.tick != tick {
            *budget = RouteBudget { tick, used: 0 };
        }
        if budget.used >= 4 || !self.take_route_credit(tick) { return None; }
        budget.used += 1;
        Some(route(world, character, from, to))
    }
}

/// The goal bots of one installed rule set. Its rules' disposal hands its
/// bots back first ([`GoalController::retire`]).
pub struct GoalController {
    revision: Option<u32>,
    assignments: Assignments,
    targets: [Resolved; GOAL_SLOTS],
    resolved: Option<u32>,
    since_resolve: u32,
    subjects: [Option<Subject>; GOAL_SLOTS],
    buildings: [Option<(u32, u32, Option<usize>)>; GOAL_SLOTS],
    subject_at: f64,
    regions: GoalRegions,
    bots: Vec<Bot>,
    route_budget: RouteBudget,
    pub(crate) soldiers: SoldierInputs,
    /// Bots hunting each character.
    hunters: Vec<u16>,
    /// Each character's lives as seen, and its last living unit.
    lives: Vec<u32>,
    seen: Vec<Option<(UKey, [u8; 8])>>,
    /// `pursuits[bot * capacity + target]`.
    pursuits: Vec<Pursuit>,
    capacity: usize,
    /// The controller orders were written through, so retirement must reach it.
    core: Option<usize>,

}

impl Default for GoalController {
    fn default() -> Self { Self::new() }
}

impl GoalController {
    pub fn new() -> Self {
        census::note();
        Self {
            revision: None,
            assignments: Assignments::default(),
            targets: Default::default(),
            resolved: None,
            since_resolve: 0,
            subjects: [None; GOAL_SLOTS],
            buildings: [None; GOAL_SLOTS],
            subject_at: f64::NEG_INFINITY,
            regions: GoalRegions::default(),
            bots: Vec::new(),
            route_budget: RouteBudget::default(),
            soldiers: SoldierInputs::default(),
            hunters: Vec::new(),
            lives: Vec::new(),
            seen: Vec::new(),
            pursuits: Vec::new(),
            capacity: 0,
            core: None,
        }
    }

    /// Room for the battle's characters, the map's regions and the
    /// installation's number, given when the rules are installed, so no pass
    /// allocates.
    pub fn admit(&mut self, characters: usize, regions: GoalRegions, epoch: u32) -> Option<()> {
        census::note();
        self.reserve(characters)?;
        self.regions = regions;
        self.soldiers.epoch = epoch;
        Some(())
    }

    pub fn set_block_mode(&mut self, mode: BlockMode) { self.soldiers.block = mode; }

    /// The goal a bot is linked to.
    pub fn goal_of(&self, character: usize) -> Option<&Goal> { self.assignments.goal_of(character) }
    pub fn goals(&self) -> impl Iterator<Item = &Goal> { self.assignments.goals() }
    /// The enemy a bot is hunting.
    pub fn hunting(&self, character: usize) -> Option<usize> { self.bots.get(character)?.hunt.map(|h| h.target) }
    /// The resolved first argument of the goal in `slot`.
    pub fn target(&self, slot: u8) -> Option<&Resolved> { self.targets.get(usize::from(slot)) }
    /// The subject of the goal in `slot` as last refreshed.
    pub fn subject(&self, slot: u8) -> Option<Subject> { *self.subjects.get(usize::from(slot))? }

    /// The melee sample of bot `character` for the life `key` and the fixed
    /// tick `tick`, without consuming it. Nothing for another life or tick.
    pub fn input(&self, character: usize, key: &F3UnitKey, tick: f64) -> Option<MeleeInput> {
        let slot = self.bots.get(character)?.sample_slot?;
        let input = self.soldiers.samples.get(slot)?.as_ref()?;
        input.is_for(key, tick).then(|| input.clone())
    }

    fn reserve(&mut self, characters: usize) -> Option<()> {
        if characters <= self.capacity {
            return Some(());
        }
        // A roster larger than the one admitted: keep every pursuit recorded.
        let old = self.capacity;
        let pairs = characters.checked_mul(characters)?;
        let mut pursuits = Vec::new();
        pursuits.try_reserve_exact(pairs).ok()?;
        let added = characters - old;
        self.bots.try_reserve_exact(added).ok()?;
        self.hunters.try_reserve_exact(added).ok()?;
        self.lives.try_reserve_exact(added).ok()?;
        self.seen.try_reserve_exact(added).ok()?;
        self.soldiers.reserve(characters)?;
        self.assignments.try_reserve(characters)?;
        pursuits.resize(pairs, Pursuit::default());
        for bot in 0..old {
            pursuits[bot * characters..bot * characters + old].copy_from_slice(&self.pursuits[bot * old..(bot + 1) * old]);
        }
        self.pursuits = pursuits;
        self.capacity = characters;
        self.bots.resize(characters, Bot::default());
        self.hunters.resize(characters, 0);
        self.lives.resize(characters, 0);
        self.seen.resize(characters, None);
        self.assignments.reserve(characters);
        Some(())
    }

    /// Whether the goal arguments are due for a new lookup: the table
    /// changed, or the last lookup is half a second old.
    pub fn resolve_due(&self, table: &dyn GoalTable) -> bool {
        self.resolved != Some(table.revision()) || self.since_resolve >= RESOLVE_PASSES
    }

    /// Look the goals' first arguments up through the rules seam's object
    /// services: a name follows its placement to the current life, a handle
    /// must still name its life. Second arguments name flags, which the bots
    /// do not carry yet, and are not looked up.
    pub fn resolve(&mut self, table: &dyn GoalTable, objects: &MissionObjects, services: &dyn ObjectServices, store: &Store) {
        census::note();
        census::resolve();
        self.resolved = Some(table.revision());
        self.since_resolve = 0;
        self.subject_at = f64::NEG_INFINITY;
        self.targets.fill(Resolved::None);
        for index in 0..table.len() {
            let row = table.row(index);
            if let Some(cell) = self.targets.get_mut(usize::from(row.slot)) {
                if row.kind.needs_target() {
                    *cell = resolve_arg(row.target, objects, services, store);
                }
            }
        }
    }

    /// One tick of the goal bots.
    pub fn pass<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(
        &mut self, mut core: GoalCore<'_>, world: &mut World<'_, '_, C, M, S>, table: &dyn GoalTable, dt: f64,
    ) {
        census::note();
        #[cfg(test)]
        let started = std::time::Instant::now();
        let now = core.now(world);
        let tick = crate::units::f3::tick_in(&world.rows.w.units);
        let characters = core.characters();
        debug_assert!(characters <= self.capacity);
        self.since_resolve = self.since_resolve.saturating_add(1);
        if self.revision != Some(table.revision()) {
            self.revision = Some(table.revision());
            self.assignments.sync((0..table.len()).map(|i| {
                let row = table.row(i);
                GoalSpec { slot: row.slot, handle: row.handle, team: row.team, kind: row.kind, weight: row.weight }
            }));
            self.subject_at = f64::NEG_INFINITY;
        }
        if now >= self.subject_at {
            self.subject_at = now + SUBJECT_CHECK;
            self.refresh_subjects(world, table);
        }
        // First the wave's departures, then its vacancies.
        for character in 0..characters {
            self.reconcile(&core, world, character, now);
        }
        for character in 0..characters {
            self.carry_out(&mut core, world, character, now, tick, dt);
        }
        #[cfg(test)]
        census::add_pass_time(started.elapsed());
    }

    fn refresh_subjects<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(
        &mut self, world: &World<'_, '_, C, M, S>, table: &dyn GoalTable,
    ) {
        self.subjects = [None; GOAL_SLOTS];
        for index in 0..table.len() {
            let row = table.row(index);
            let slot = usize::from(row.slot);
            if slot >= GOAL_SLOTS {
                continue;
            }
            let subject = match row.kind {
                GoalKind::Conquest | GoalKind::Deathmatch => None,
                _ => subject_of(world, &self.regions, row.target, &self.targets[slot], &mut self.buildings[slot]),
            };
            let active = match row.kind {
                GoalKind::Conquest | GoalKind::Deathmatch => true,
                GoalKind::Destroy | GoalKind::Follow => subject.is_some_and(|s| s.alive),
                GoalKind::Defend | GoalKind::CtfOffense | GoalKind::CtfDefense => subject.is_some(),
            };
            self.subjects[slot] = subject;
            self.assignments.set_active(row.slot, active);
        }
    }

    /// Note every character's life, and free the links that are no longer
    /// valid: no living bot, a team outside 0 to 7, a goal gone, suspended or
    /// on another team.
    fn reconcile<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(
        &mut self, core: &GoalCore<'_>, world: &World<'_, '_, C, M, S>, character: usize, now: f64,
    ) {
        if let Some(unit) = world.character(character).unit {
            let s = world.unit_scalars(unit);
            let life = (unit, s.generation.to_le_bytes());
            if s.alive && self.seen[character] != Some(life) {
                self.seen[character] = Some(life);
                self.lives[character] = self.lives[character].wrapping_add(1);
            }
        }
        let view = core.bot(world, character);
        let Some(view) = view else {
            if self.bots[character].life.is_some() {
                self.end_life(character);
            }
            self.bots[character].view = None;
            self.bots[character].team = None;
            self.assignments.keep(character, None);
            return;
        };
        if self.bots[character].life != Some((view.unit, view.generation)) {
            self.end_life(character);
            let bot = &mut self.bots[character];
            bot.life = Some((view.unit, view.generation));
            // Spread a wave's opening checks over the next half second.
            bot.next_check = now + (character % 30) as f64 / 60.0;
            bot.next_pick = bot.next_check;
            bot.next_jump = now;
        }
        let team = goal_team(world.unit_scalars(view.unit).team);
        let bot = &mut self.bots[character];
        bot.view = Some(view);
        bot.team = team;
        self.assignments.keep(character, team);
    }

    fn end_life(&mut self, character: usize) {
        self.assignments.release(character);
        self.set_hunt(character, None);
        let bot = &mut self.bots[character];
        *bot = Bot { round: bot.round.wrapping_add(1), route_ready: bot.route_ready, ..Bot::default() };
    }

    fn set_hunt(&mut self, character: usize, hunt: Option<Hunt>) {
        let bot = &mut self.bots[character];
        if let Some(old) = bot.hunt {
            self.hunters[old.target] = self.hunters[old.target].saturating_sub(1);
        }
        bot.anchor = None;
        bot.hunt = hunt;
        match hunt {
            Some(h) => {
                self.hunters[h.target] += 1;
                bot.plan = Plan::Hunt;
            }
            None => {
                if bot.plan == Plan::Hunt {
                    bot.plan = Plan::Idle;
                }
            }
        }
    }

    fn pair(&self, bot: usize, target: usize) -> usize { bot * self.capacity + target }

    /// The pursuit of `target`'s current life in the bot's current round,
    /// when one was started.
    fn pursuit(&self, bot: usize, target: usize) -> Option<Pursuit> {
        let p = self.pursuits[self.pair(bot, target)];
        (p.round == self.bots[bot].round && p.life == self.lives[target]).then_some(p)
    }

    fn give_up(&mut self, character: usize, target: usize, now: f64) {
        let pair = self.pair(character, target);
        let round = self.bots[character].round;
        let life = self.lives[target];
        let route = self.pursuit(character, target).map_or(f32::NAN, |p| p.route);
        self.pursuits[pair] = Pursuit { round, life, spent: f32::INFINITY, budget: 0.0, route, rejected_at: None };
        self.set_hunt(character, None);
        self.bots[character].next_pick = now;
    }

    fn carry_out<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(
        &mut self, core: &mut GoalCore<'_>, world: &mut World<'_, '_, C, M, S>, character: usize, now: f64, tick: f64, dt: f64,
    ) {
        let Some(view) = self.bots[character].view else { return };
        let link = self.bots[character].team.and_then(|team| self.assignments.fill(character, team));
        if !view.mounted {
            self.core = Some(core.identity());
            match (link, self.bots[character].team) {
                (Some(link), Some(team)) => self.goal(core, world, character, view, team, link, now, dt),
                _ => self.hold(core, world, character, view),
            }
        }
        // Mounted bots keep the released seat policies; their melee input
        // still steps, releasing what they held.
        self.bots[character].sample_slot = self.soldiers.compose(core, world, character, view, tick, dt);
    }

    fn change_goal<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(&mut self, core: &mut GoalCore<'_>, world: &mut World<'_, '_, C, M, S>, character: usize, view: BotView, link: Option<Assigned>, now: f64) {
        if self.bots[character].goal == link {
            return;
        }
        self.set_hunt(character, None);
        if self.bots[character].attacking.take().is_some() {
            let mut draws = world.random.share();
            core.cancel_attack(world, &mut draws, view.ci);
        }
        let bot = &mut self.bots[character];
        bot.goal = link;
        bot.plan = Plan::Idle;
        bot.next_pick = now;
        bot.next_check = now;
    }

    #[allow(clippy::too_many_arguments)]
    fn goal<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(
        &mut self, core: &mut GoalCore<'_>, world: &mut World<'_, '_, C, M, S>, character: usize, view: BotView, team: u8,
        link: Assigned, now: f64, dt: f64,
    ) {
        let goal = *self.assignments.goal(link.slot).expect("assigned goal");
        self.change_goal(core, world, character, view, Some(link), now);
        let base = match (goal.kind, world.character(character).objective) {
            (GoalKind::Conquest, Some(post)) => Base::Objective(post),
            _ => Base::Wait,
        };
        core.set_base(view.ci, base);
        if let Base::Objective(_) = base {
            self.set_hunt(character, None);
            self.bots[character].plan = Plan::Objective;
            return;
        }
        let position = world.unit_scalars(view.unit).position;
        let subject = self.subjects[usize::from(link.slot)];
        match goal.kind {
            GoalKind::Deathmatch if world.rows.state.navigation.nav.swbf2.is_some() => {
                self.deathmatch(core, world, character, view, team, position, now)
            }
            GoalKind::Conquest | GoalKind::Deathmatch => self.hunt(core, world, character, view, team, position, now, dt),
            GoalKind::Defend | GoalKind::CtfDefense => self.guard(core, world, character, view, subject, position, now),
            GoalKind::Destroy => match subject {
                Some(s) if s.entity.is_some() => self.destroy(core, world, character, view, s, position, now),
                // An object the released combat cannot attack: the bots fight
                // whom they meet on the way instead.
                _ => self.hunt(core, world, character, view, team, position, now, dt),
            },
            GoalKind::Follow => self.follow(core, world, character, view, subject, position, now),
            GoalKind::CtfOffense => match subject {
                Some(s) if horizontal(position, s.position) > ATTACK_RANGE => {
                    // No flag model: walk to the area, then hunt.
                    self.set_hunt(character, None);
                    self.bots[character].plan = Plan::Attack;
                    self.steer(core, world, character, view, Some(s.position), None);
                }
                _ => self.hunt(core, world, character, view, team, position, now, dt),
            },
        }
    }

    /// Stand where the bot is.
    fn hold<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(
        &mut self, core: &mut GoalCore<'_>, world: &mut World<'_, '_, C, M, S>, character: usize, view: BotView,
    ) {
        self.change_goal(core, world, character, view, None, f64::NEG_INFINITY);
        self.bots[character].plan = Plan::Idle;
        core.set_base(view.ci, Base::Wait);
        self.steer(core, world, character, view, None, None);
    }

    fn steer<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(
        &mut self, core: &mut GoalCore<'_>, world: &mut World<'_, '_, C, M, S>, character: usize, view: BotView,
        to: Option<Vec3>, facing: Option<Vec3>,
    ) -> Steer {
        core.steer(world, view.ci, &mut self.bots[character].steering, to, facing, REROUTE)
    }

    #[allow(clippy::too_many_arguments)]
    fn deathmatch<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(
        &mut self, core: &mut GoalCore<'_>, world: &mut World<'_, '_, C, M, S>, character: usize, view: BotView, team: u8,
        position: Vec3, now: f64,
    ) {
        self.set_hunt(character, None);
        let running = core.running(view.ci);
        if !matches!(running, Running::Wait { .. } | Running::Idle | Running::Recovery) {
            // Combat or another interrupt took over: pick a fresh leg on return.
            self.bots[character].plan = Plan::Idle;
            self.bots[character].next_pick = now;
            return;
        }
        if let Plan::Roam { dest } = self.bots[character].plan {
            let steer = self.steer(core, world, character, view, Some(dest), None);
            if matches!(steer, Steer::Moving { arrived: true, .. }) || horizontal(position, dest) <= DEATHMATCH_ARRIVAL {
                self.bots[character].plan = Plan::Idle;
                self.bots[character].next_pick = now;
            } else if matches!(steer, Steer::Moving { failed: true, .. }) {
                self.bots[character].plan = Plan::Idle;
                self.bots[character].next_pick = now + DEATHMATCH_FAIL_RETRY;
                return;
            } else {
                if matches!(steer, Steer::Moving { arrived: false, failed: false }) {
                    self.try_deathmatch_jump(core, world, character, view, position, now);
                }
                return;
            }
        }
        if now < self.bots[character].next_pick {
            return;
        }
        let has_graph = world
            .rows
            .state
            .navigation
            .nav
            .swbf2
            .as_ref()
            .and_then(|s| s.graph.as_ref())
            .is_some_and(|g| !g.hubs.is_empty());
        let wander_first = has_graph && Draws::b_uniform(world.random) < 0.5;
        let dest = if wander_first {
            self.pick_wander_point(world, view.unit, position)
        } else {
            self.pick_random_enemy_snapshot(core, world, view, team)
                .unwrap_or_else(|| self.pick_wander_point(world, view.unit, position))
        };
        self.bots[character].plan = Plan::Roam { dest };
        self.steer(core, world, character, view, Some(dest), None);
    }

    fn try_deathmatch_jump<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(
        &mut self, core: &mut GoalCore<'_>, world: &mut World<'_, '_, C, M, S>, character: usize, view: BotView, position: Vec3, now: f64,
    ) {
        let Some(Entity::Unit(reference)) = world.rows.state.entities.get(view.unit) else {
            return;
        };
        let Some((jump, fuel, min_border, airborne)) = world.rows.w.sim.store.unit.get(reference).and_then(|u| match &u.soldier {
            crate::gen::UnitBody::Soldier(s) => Some((
                s.jet.jump,
                s.fuel,
                s.jet.min_border,
                matches!(s.air, Some(crate::gen::Air::Jump | crate::gen::Air::Jet)),
            )),
            crate::gen::UnitBody::Droideka(_) => None,
        }) else {
            return;
        };
        if !(jump > 0.0) {
            return;
        }
        if fuel <= min_border {
            self.bots[character].next_jump = now + DEATHMATCH_JUMP_INTERVAL + Draws::b_uniform(world.random) * DEATHMATCH_JUMP_LOW_FUEL_SPAN;
            return;
        }
        if airborne || core.jet_pending(view.ci) || now <= self.bots[character].next_jump {
            return;
        }
        self.bots[character].next_jump = now + DEATHMATCH_JUMP_INTERVAL;
        let eye = Vec3::new(position.x, position.y + 0.5, position.z);
        if Draws::b_uniform(world.random) < 0.5 && crate::ai::owned::live::clear_above_unit(world.rows.w, eye, reference) {
            core.request_jet(world, view.ci, Vec3::default());
        }
    }

    fn pick_wander_point<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(
        &self, world: &mut World<'_, '_, C, M, S>, unit: UKey, position: Vec3,
    ) -> Vec3 {
        let radius = {
            let soldier = world.unit(unit);
            soldier.nav_radius.unwrap_or_else(|| crate::ai::nav::units::traversal_radius(&soldier.nav_unit()))
        };
        let mask = super::reach::SOLDIER_MASK;
        let selected = world
            .rows
            .state
            .navigation
            .nav
            .swbf2
            .as_ref()
            .and_then(|swbf2| {
                let graph = swbf2.graph.as_ref()?;
                let start = nearest_soldier_hub(graph, position)?;
                let mut count = 0usize;
                for h in 0..graph.hubs.len() {
                    if h != start
                        && graph.hubs[h].valid
                        && (graph.hubs[h].mask & mask) != 0
                        && graph.hub_route_distance(start, h, mask, false).reachable
                    {
                        count += 1;
                    }
                }
                if count == 0 {
                    return None;
                }
                let pick = (v8math::floor(Draws::b_uniform(world.random) * count as f64) as usize).min(count - 1);
                let mut seen = 0usize;
                let mut chosen = None;
                for h in 0..graph.hubs.len() {
                    if h != start
                        && graph.hubs[h].valid
                        && (graph.hubs[h].mask & mask) != 0
                        && graph.hub_route_distance(start, h, mask, false).reachable
                    {
                        if seen == pick {
                            chosen = Some(h);
                            break;
                        }
                        seen += 1;
                    }
                }
                let hub_idx = chosen?;
                let hub = &graph.hubs[hub_idx];
                let floor = swbf2
                    .floors
                    .hub_floor_point(hub_idx, Draws::b_uniform(world.random))
                    .unwrap_or_else(|| hub.pos());
                Some((hub.pos(), hub.radius, floor))
            });
        if let Some((center, hub_radius, floor)) = selected {
            if hub_radius > 0.0 {
                for _ in 0..4 {
                    let angle = Draws::b_uniform(world.random) * (2.0 * std::f64::consts::PI);
                    let dist = v8math::sqrt(Draws::b_uniform(world.random)) * hub_radius;
                    let candidate = Vec3::new(
                        center.x + dist * v8math::cos(angle),
                        floor.y,
                        center.z + dist * v8math::sin(angle),
                    );
                    if world.rows.state.navigation.nav.barriers.point_clear(candidate, radius, mask).0 {
                        return candidate;
                    }
                }
            }
            return floor;
        }
        for _ in 0..4 {
            let dx = (Draws::b_uniform(world.random) * 2.0 - 1.0) * DEATHMATCH_FALLBACK_SPAN;
            let dz = (Draws::b_uniform(world.random) * 2.0 - 1.0) * DEATHMATCH_FALLBACK_SPAN;
            let candidate = Vec3::new(position.x + dx, position.y, position.z + dz);
            if world.rows.state.navigation.nav.barriers.point_clear(candidate, radius, mask).0 {
                return candidate;
            }
        }
        position
    }

    fn pick_random_enemy_snapshot<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(
        &self, core: &GoalCore<'_>, world: &mut World<'_, '_, C, M, S>, view: BotView, team: u8,
    ) -> Option<Vec3> {
        let limit = core.characters().min(self.capacity);
        let mut count = 0usize;
        for target in 0..limit {
            if enemy(core, world, team, target).is_some() {
                count += 1;
            }
        }
        if count == 0 {
            return None;
        }
        let pick = (v8math::floor(Draws::b_uniform(world.random) * count as f64) as usize).min(count - 1);
        let mut seen = 0usize;
        let mut chosen = None;
        for target in 0..limit {
            if let Some((unit, _, at)) = enemy(core, world, team, target) {
                if seen == pick {
                    chosen = Some((unit, at));
                    break;
                }
                seen += 1;
            }
        }
        let (target_unit, at) = chosen?;
        if !world.can_engage_at_any_range(view.unit, target_unit) {
            return None;
        }
        let radius = {
            let soldier = world.unit(view.unit);
            soldier.nav_radius.unwrap_or_else(|| crate::ai::nav::units::traversal_radius(&soldier.nav_unit()))
        };
        let mask = super::reach::SOLDIER_MASK;
        for _ in 0..4 {
            let dx = (Draws::b_uniform(world.random) * 2.0 - 1.0) * DEATHMATCH_ENEMY_JITTER;
            let dz = (Draws::b_uniform(world.random) * 2.0 - 1.0) * DEATHMATCH_ENEMY_JITTER;
            let candidate = Vec3::new(at.x + dx, at.y, at.z + dz);
            if world.rows.state.navigation.nav.barriers.point_clear(candidate, radius, mask).0 {
                return Some(candidate);
            }
        }
        Some(at)
    }

    #[allow(clippy::too_many_arguments)]
    fn hunt<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(
        &mut self, core: &mut GoalCore<'_>, world: &mut World<'_, '_, C, M, S>, character: usize, view: BotView, team: u8,
        position: Vec3, now: f64, dt: f64,
    ) {
        // A target that died, left or changed side is dropped at once.
        if let Some(h) = self.bots[character].hunt {
            let same = enemy(core, world, team, h.target).is_some_and(|(unit, generation, _)| {
                unit == h.unit && generation.to_bits() == h.generation.to_bits()
            });
            if !same {
                self.set_hunt(character, None);
                self.bots[character].next_pick = now;
            }
        }
        let running = core.running(view.ci);
        let pursuing = matches!(running, Running::Wait { .. } | Running::Idle | Running::Recovery);
        if let Some(h) = self.bots[character].hunt {
            // Engaging the hunted enemy is the only progress: its pursuit
            // starts again. Walking toward it spends its pursuit time.
            let engaged = running == Running::Fight
                && core.combat_target(view.ci) == Some(EntRef { unit: h.unit, generation: h.generation });
            let pair = self.pair(character, h.target);
            if engaged {
                self.pursuits[pair].spent = 0.0;
                self.bots[character].anchor = None;
            } else if pursuing {
                self.pursuits[pair].spent += dt as f32;
            }
        }
        if !pursuing {
            self.bots[character].anchor = None;
        }
        if (self.bots[character].hunt.is_none() || self.bots[character].checking_unknown) && now >= self.bots[character].next_pick {
            self.bots[character].next_pick = now + HUNT_PICK + 0.2 * (character % 9) as f64;
            let pick = self.pick(core, world, character, team, position, now);
            if pick != self.bots[character].hunt { self.set_hunt(character, pick); }
            if pick.is_none() && !matches!(self.bots[character].plan, Plan::Patrol { .. }) {
                self.bots[character].plan = Plan::Patrol { center: position, leg: (character % 6) as u8, until: now + PATROL_LEG };
            }
        }
        match self.bots[character].plan {
            Plan::Hunt => {
                let Some(h) = self.bots[character].hunt else { return };
                let Some((_, _, at)) = enemy(core, world, team, h.target) else { return };
                if pursuing && now >= self.bots[character].next_check {
                    self.bots[character].next_check = now + CHECK;
                    let (dx, dz) = (at.x - position.x, at.z - position.z);
                    let distance = v8math::sqrt(dx * dx + dz * dz);
                    let anchor = *self.bots[character].anchor.get_or_insert((position, now, distance));
                    let approach = if distance > 1e-6 {
                        ((position.x - anchor.0.x) * dx + (position.z - anchor.0.z) * dz) / distance
                    } else {
                        f64::INFINITY
                    };
                    let mut stalled = false;
                    if approach > 0.0 && distance < anchor.2 {
                        self.bots[character].anchor = Some((position, now, distance));
                    } else if now - anchor.1 > APPROACH_WINDOW {
                        // The approach clock ran out: a walk the navigation now
                        // gives as shorter keeps the pursuit, anything else ends it.
                        match self.route_progress(world, character, h.target, position, at) {
                            Some(true) => self.bots[character].anchor = Some((position, now, distance)),
                            Some(false) => stalled = true,
                            None => {} // Retry the progress check when another query is available.
                        }
                    }
                    let spent = self.pursuit(character, h.target).is_none_or(|p| p.spent >= p.budget);
                    if stalled || spent {
                        self.give_up(character, h.target, now);
                        return;
                    }
                }
                if let Steer::Moving { failed: true, .. } = self.steer(core, world, character, view, Some(at), None) {
                    self.give_up(character, h.target, now);
                }
            }
            Plan::Patrol { center, leg, until } => {
                let (leg, until) = if now >= until { (leg.wrapping_add(1), now + PATROL_LEG) } else { (leg, until) };
                self.bots[character].plan = Plan::Patrol { center, leg, until };
                let angle = f64::from(leg % 6) * (std::f64::consts::PI / 3.0);
                let to = Vec3::new(center.x + PATROL_RADIUS * v8math::cos(angle), center.y, center.z + PATROL_RADIUS * v8math::sin(angle));
                self.steer(core, world, character, view, Some(to), None);
            }
            _ => {
                self.steer(core, world, character, view, None, None);
            }
        }
    }

    /// Whether the navigation gives a shorter walk than its last answer for this pair. The
    /// answer is kept; without a route or an answer there is no progress.
    fn route_progress<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(
        &mut self, world: &mut World<'_, '_, C, M, S>, character: usize, target: usize, position: Vec3, at: Vec3,
    ) -> Option<bool> {
        let pair = self.pair(character, target);
        let cached = self.pursuits[pair].route;
        if !(cached.is_finite() && cached >= 0.0) {
            return Some(false);
        }
        Some(match self.bots[character].ask_route(&mut self.route_budget, world, character, position, at)? {
            Route::Reachable { distance } => {
                let distance = distance as f32;
                self.pursuits[pair].route = distance;
                distance < cached
            }
            Route::NoRoute => {
                self.pursuits[pair].route = f32::INFINITY;
                false
            }
            Route::Unknown => false,
        })
    }

    /// The nearest living enemy the bot has not given up on in this round,
    /// by the walk the navigation gives (the straight distance without an
    /// answer, behind every enemy with a route when it gives none), counting
    /// each ally already hunting an enemy as extra distance; ties keep the
    /// lower character index. The navigation is asked once per bot life and
    /// enemy life, about the nearest few new pairs of a choice. When every
    /// enemy was given up, a new round starts after the bot has patrolled for
    /// `RETRY` seconds.
    fn pick<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(
        &mut self, core: &GoalCore<'_>, world: &mut World<'_, '_, C, M, S>, character: usize, team: u8, position: Vec3,
        now: f64,
    ) -> Option<Hunt> {
        let mut best = [(f64::INFINITY, usize::MAX); ROUTE_TRIES];
        let mut skipped = false;
        // False sorts before true, so a route beats an unknown walk at any distance.
        let current = self.bots[character].hunt.map(|hunt| hunt.target);
        let mut chosen: Option<(bool, f64, usize)> = None;
        let mut unanswered = 0;
        let mut consider = |p: Pursuit, straight: f64, crowd: u16, target: usize| {
            let crowd = crowd.saturating_sub(u16::from(current == Some(target)));
            let rank = (p.route < 0.0, p.walk(straight) + f64::from(crowd) * CROWD, target);
            if chosen.is_none_or(|best| rank < best) { chosen = Some(rank); }
        };
        for target in 0..core.characters().min(self.capacity) {
            let Some((_, _, at)) = enemy(core, world, team, target) else { continue };
            census::scan();
            let mut p = self.pursuit(character, target);
            if let Some(mut stale) = p.filter(|p| p.rejected_at.is_some_and(|from| horizontal(from, position) >= 1.0 ||
                (from.y - position.y).abs() >= 1.0)) {
                let pair = self.pair(character, target);
                stale.route = f32::NAN;
                stale.rejected_at = None;
                self.pursuits[pair] = stale;
                p = Some(stale);
            }
            if p.is_some_and(|p| p.spent >= p.budget || p.route.is_infinite()) {
                skipped = true;
                continue;
            }
            let straight = horizontal(position, at);
            if let Some(answer) = p.filter(|p| !p.route.is_nan()) {
                consider(answer, straight, self.hunters[target], target);
                continue;
            }
            unanswered += 1;
            let score = straight + f64::from(self.hunters[target]) * CROWD;
            if score < best[ROUTE_TRIES - 1].0 {
                let mut i = ROUTE_TRIES - 1;
                while i > 0 && score < best[i - 1].0 {
                    best[i] = best[i - 1];
                    i -= 1;
                }
                best[i] = (score, target);
            }
        }
        // Ask about the new pairs among the nearest few, then take the best.
        let round = self.bots[character].round;
        let mut rejected = [None; ROUTE_TRIES];
        let mut all_rejected = true;
        for (index, (_, target)) in best.into_iter().enumerate() {
            if target == usize::MAX {
                break;
            }
            let Some((_, _, at)) = enemy(core, world, team, target) else { continue };
            let straight = horizontal(position, at);
            let pair = self.pair(character, target);
            let mut p = self.pursuit(character, target).unwrap_or(Pursuit::fresh(round, self.lives[target]));
            let Some(answer) = self.bots[character].ask_route(&mut self.route_budget, world, character, position, at) else {
                break;
            };
            unanswered -= 1;
            if answer == Route::NoRoute { rejected[index] = Some(pair); } else { all_rejected = false; }
            p.route = match answer {
                Route::Unknown => -1.0,
                Route::NoRoute => f32::INFINITY,
                Route::Reachable { distance } => distance as f32,
            };
            if p.budget == 0.0 { p.budget = PURSUIT_BASE + PURSUIT_PER_METRE * p.walk(straight) as f32; }
            self.pursuits[pair] = p;
            if p.route.is_infinite() { skipped = true; continue; }
            consider(p, straight, self.hunters[target], target);
        }
        if all_rejected {
            for pair in rejected.into_iter().flatten() { self.pursuits[pair].rejected_at = Some(position); }
        }
        let bot = &mut self.bots[character];
        bot.checking_unknown = unanswered > 0 && chosen.is_none_or(|(unknown, _, _)| unknown);
        if bot.checking_unknown {
            let wait = bot.route_ready[0].saturating_sub(self.route_budget.tick);
            bot.next_pick = now + wait as f64 / 60.0;
        }
        if let Some((unknown, _, best)) = chosen {
            let target = if unknown { current.unwrap_or(best) } else { best };
            if let Some((unit, generation, _)) = enemy(core, world, team, target) {
                bot.round_since = f64::NEG_INFINITY;
                return Some(Hunt { target, life: self.lives[target], unit, generation });
            }
        }
        if !bot.checking_unknown && skipped {
            let bot = &mut self.bots[character];
            if bot.round_since.is_infinite() {
                bot.round_since = now;
            } else if now - bot.round_since >= RETRY {
                bot.round = bot.round.wrapping_add(1);
                if let Some(fighter) = bot.sample_slot.and_then(|s| self.soldiers.fighters.get_mut(s)) {
                    fighter.new_round();
                }
                bot.round_since = f64::NEG_INFINITY;
            }
        }
        None
    }

    #[allow(clippy::too_many_arguments)]
    fn guard<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(
        &mut self, core: &mut GoalCore<'_>, world: &mut World<'_, '_, C, M, S>, character: usize, view: BotView,
        subject: Option<Subject>, position: Vec3, now: f64,
    ) {
        self.set_hunt(character, None);
        let Some(subject) = subject else {
            self.steer(core, world, character, view, None, None);
            return;
        };
        let angle = (character % 8) as f64 * (std::f64::consts::PI / 4.0);
        let spot = Vec3::new(
            subject.position.x + GUARD_RING * v8math::cos(angle),
            subject.position.y,
            subject.position.z + GUARD_RING * v8math::sin(angle),
        );
        let (old, settled) = match self.bots[character].plan {
            Plan::Guard { spot, settled } => (Some(spot), settled),
            _ => (None, false),
        };
        // A subject that moved gives the guard a new spot; arrival is a
        // transition to standing, checked again only every GUARD_CHECK.
        let mut settled = settled && old.is_some_and(|o| horizontal(o, spot) <= REROUTE);
        let spot = match (settled, old) {
            (true, Some(old)) => old,
            _ => spot,
        };
        let bot = &mut self.bots[character];
        if settled {
            if now >= bot.next_check {
                bot.next_check = now + GUARD_CHECK;
                if horizontal(position, spot) > GUARD_LEASH {
                    settled = false;
                }
            }
        } else if horizontal(position, spot) < 2.0 {
            settled = true;
            bot.next_check = now + GUARD_CHECK;
        }
        self.bots[character].plan = Plan::Guard { spot, settled };
        let steer = self.steer(core, world, character, view, (!settled).then_some(spot), Some(subject.position));
        if let (false, Steer::Moving { arrived: true, .. }) = (settled, steer) {
            self.bots[character].plan = Plan::Guard { spot, settled: true };
            self.bots[character].next_check = now + GUARD_CHECK;
        }
    }

    /// Walk to the object and, within reach, attack it through the released
    /// combat order, as an interrupt that returns to the base order.
    #[allow(clippy::too_many_arguments)]
    fn destroy<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(
        &mut self, core: &mut GoalCore<'_>, world: &mut World<'_, '_, C, M, S>, character: usize, view: BotView,
        subject: Subject, position: Vec3, now: f64,
    ) {
        self.set_hunt(character, None);
        self.bots[character].plan = Plan::Attack;
        let target = subject.entity.expect("attackable subject");
        if self.bots[character].attacking.is_some_and(|t| t != target) {
            // The subject's combat target changed (a rebuilt object).
            self.bots[character].attacking = None;
            let mut draws = world.random.share();
            core.cancel_attack(world, &mut draws, view.ci);
        }
        if horizontal(position, subject.position) < ATTACK_RANGE {
            if now >= self.bots[character].next_attack && core.attack(view.ci, target) == Attack::Started {
                self.bots[character].attacking = Some(target);
                self.bots[character].next_attack = now + ATTACK_RETRY;
            }
            self.steer(core, world, character, view, None, Some(subject.position));
            return;
        }
        self.steer(core, world, character, view, Some(subject.position), None);
    }

    #[allow(clippy::too_many_arguments)]
    fn follow<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(
        &mut self, core: &mut GoalCore<'_>, world: &mut World<'_, '_, C, M, S>, character: usize, view: BotView,
        subject: Option<Subject>, position: Vec3, now: f64,
    ) {
        self.set_hunt(character, None);
        self.bots[character].plan = Plan::Follow;
        let Some(subject) = subject else {
            self.steer(core, world, character, view, None, None);
            return;
        };
        if now < self.bots[character].next_check {
            return;
        }
        self.bots[character].next_check = now + CHECK;
        let angle = (character % 6) as f64 * (std::f64::consts::PI / 3.0);
        let lead = Vec3::new(subject.velocity.x * FOLLOW_LEAD, 0.0, subject.velocity.z * FOLLOW_LEAD);
        let to = Vec3::new(
            subject.position.x + lead.x + 3.0 * v8math::cos(angle),
            subject.position.y,
            subject.position.z + lead.z + 3.0 * v8math::sin(angle),
        );
        let slow = subject.velocity.x * subject.velocity.x + subject.velocity.z * subject.velocity.z < 1.0;
        let dest = (!(slow && horizontal(position, subject.position) < FOLLOW_NEAR)).then_some(to);
        self.steer(core, world, character, view, dest, Some(subject.position));
    }

    /// Hand every bot this controller steered back to the released
    /// controller as a freshly bound bot, and forget them.
    pub fn retire<W: AiWorld + crate::ai::control::helpers::World>(
        &mut self, mut core: GoalCore<'_>, world: &mut W, draws: &mut dyn Draws,
    ) {
        census::note();
        for character in 0..core.characters() {
            let steered = self.bots.get(character).is_some_and(|b| b.life.is_some());
            core.retire(world, draws, character, steered);
            if character < self.bots.len() {
                self.end_life(character);
            }
        }
        self.core = None;
    }

    /// The controller this controller wrote orders through, if any.
    pub fn steered(&self) -> Option<usize> { self.core }
    /// Forget the bots of a controller that no longer exists.
    pub fn forget(&mut self) {
        for character in 0..self.bots.len() {
            self.end_life(character);
        }
        self.core = None;
    }
}

/// A team a goal can name, from a unit's team.
fn goal_team(team: f64) -> Option<u8> {
    (team.fract() == 0.0 && (0.0..GOAL_TEAMS as f64).contains(&team)).then_some(team as u8)
}

fn horizontal(a: Vec3, b: Vec3) -> f64 {
    let (x, z) = (a.x - b.x, a.z - b.z);
    v8math::sqrt(x * x + z * z)
}

fn nearest_soldier_hub(graph: &crate::ai::nav::swbf2::Swbf2Graph, position: Vec3) -> Option<usize> {
    let mask = super::reach::SOLDIER_MASK;
    let mut best = None;
    let mut best_d2 = f64::INFINITY;
    for hub in &graph.hubs {
        if !hub.valid || (hub.mask & mask) == 0 {
            continue;
        }
        let dx = hub.x - position.x;
        let dz = hub.z - position.z;
        let d2 = dx * dx + dz * dz;
        if d2 < best_d2 {
            best_d2 = d2;
            best = Some(hub.index);
        }
    }
    best
}

/// A living enemy of `team` a soldier hunts on foot: its unit, life and
/// position. Flyers and items are not hunted on foot.
fn enemy<W: AiWorld + ?Sized>(core: &GoalCore<'_>, world: &W, team: u8, character: usize) -> Option<(UKey, f64, Vec3)> {
    let unit = world.character(character).unit?;
    let s = world.unit_scalars(unit);
    if !s.alive || s.kind == 5.0 || s.kind == 10.0 || core.relation(f64::from(team), s.team) >= 0.0 {
        return None;
    }
    Some((unit, s.generation, s.position))
}

fn resolve_arg(arg: GoalArg<'_>, objects: &MissionObjects, services: &dyn ObjectServices, store: &Store) -> Resolved {
    let alive = |handle: &ObjectHandle| match services.goal_alive(store, handle) {
        Ok(alive) => alive,
        // A prop with no life state (an animated prop) is there while it exists.
        Err(RuleError::NotAvailable) => true,
        Err(_) => false,
    };
    match arg {
        GoalArg::None => Resolved::None,
        GoalArg::Character(c) => Resolved::Character(c),
        // A handle names one life; a rebuilt object is another.
        GoalArg::Object(handle) => match services.current(store, handle) {
            Ok(current) if current == *handle => Resolved::Object { alive: alive(handle), handle: current },
            _ => Resolved::Missing,
        },
        GoalArg::Placement(handle @ ObjectHandle::Prop(_)) => match services.current(store, handle) {
            Ok(current) => Resolved::Object { alive: alive(&current), handle: current },
            Err(_) => Resolved::Missing,
        },
        GoalArg::Placement(_) => Resolved::Missing,
        // A name follows its placement to the current life.
        GoalArg::Name(name) => match objects.find_bytes(name) {
            Ok(placement) => match services.current(store, placement) {
                Ok(current) => Resolved::Object { alive: alive(&current), handle: current },
                Err(_) => Resolved::Missing,
            },
            Err(RuleError::UnknownName) => Resolved::Name,
            Err(_) => Resolved::Missing,
        },
    }
}

fn subject_of<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(
    world: &World<'_, '_, C, M, S>, regions: &GoalRegions, arg: GoalArg<'_>, resolved: &Resolved, building: &mut Option<(u32, u32, Option<usize>)>,
) -> Option<Subject> {
    let still = |position: Vec3, alive: bool| Subject { position, velocity: Vec3::default(), alive, entity: None };
    match resolved {
        Resolved::None | Resolved::Missing => None,
        Resolved::Character(reference) => {
            let rows = &*world.rows;
            let index = rows.state.roster.index(row_context!(rows), &*rows.characters, *reference)?;
            let unit = world.character(index).unit?;
            Some(unit_subject(world, unit))
        }
        Resolved::Object { handle, alive } => object_subject(world, handle, *alive, building),
        Resolved::Name => {
            let GoalArg::Name(name) = arg else { return None };
            let mut found = None;
            for index in 0..world.posts_len() {
                let post = world.post(index);
                if post.name.as_deref().is_some_and(|n| name_order(n.as_bytes(), name).is_eq()) {
                    if found.is_some() {
                        // Two posts share the name: ambiguous, never guessed.
                        return None;
                    }
                    found = Some(still(post.position, post.active));
                }
            }
            found.or_else(|| regions.find(name).map(|position| still(position, true)))
        }
    }
}

fn unit_subject<W: AiWorld + ?Sized>(world: &W, unit: UKey) -> Subject {
    let s = world.unit_scalars(unit);
    let velocity = world.unit_motion(unit).velocity.unwrap_or_default();
    Subject { position: s.position, velocity, alive: s.alive, entity: Some(EntRef { unit, generation: s.generation }) }
}

/// An object's position and life, and the combat target the released
/// controller attacks it through when it has one.
fn object_subject<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(
    world: &World<'_, '_, C, M, S>, handle: &ObjectHandle, alive: bool, building: &mut Option<(u32, u32, Option<usize>)>,
) -> Option<Subject> {
    let rows = &*world.rows;
    let store = &rows.w.sim.store;
    let at = |p: [f64; 3]| Vec3::new(p[0], p[1], p[2]);
    let (position, entity) = match handle {
        ObjectHandle::Prop(h) => {
            let live = crate::battle::f4::live_if(&rows.w.battle)?;
            match h.kind {
                F4ObjectKind::Building => {
                    let m = &live.buildings.get(h)?.settings.matrix.elements;
                    let index = match *building {
                        Some((slot, generation, index)) if slot == h.slot && generation == h.generation => index,
                        _ => {
                            #[cfg(test)]
                            census::building_lookup();
                            let index = crate::units::f3_props::building_of_in(&rows.w.units, h);
                            *building = Some((h.slot, h.generation, index));
                            index
                        }
                    };
                    let entity = index
                        .and_then(|index| crate::units::f3_props::building_target_id_in(&rows.w.units, index))
                        .and_then(|id| store.units.by_id(id))
                        .and_then(|member| rows.state.queries.object(&rows.state.entities, member));
                    (Vec3::new(m[12], m[13], m[14]), entity)
                }
                F4ObjectKind::Trap => (at(live.traps.get(h)?.settings.point), None),
                F4ObjectKind::Animated => {
                    let p = &live.animated.get(h)?.settings.position;
                    (Vec3::new(p.x, p.y, p.z), None)
                }
                _ => return None,
            }
        }
        ObjectHandle::Vehicle { vehicle, .. } => {
            let hull = store.vehicle.get(*vehicle)?;
            (at(hull.position), rows.state.entities.find(Entity::HullEntry(*vehicle)))
        }
        ObjectHandle::Door(instance) => {
            let door = rows.w.space()?.doors.iter().find(|d| d.instance == *instance as usize)?;
            (at(door.center), None)
        }
        ObjectHandle::Post(SpawnPost::Post(post)) => (at(store.post.get(*post)?.position), None),
        ObjectHandle::Post(SpawnPost::MobilePost(post)) => {
            let post = store.mobile_post.get(*post)?;
            let position = match post.vehicle {
                Some(v) if post.active => store.vehicle.get(v).map_or(post.last_position, |hull| hull.position),
                _ => post.last_position,
            };
            (at(position), None)
        }
    };
    let entity = entity.map(|unit| EntRef { unit, generation: world.unit_scalars(unit).generation });
    let velocity = entity.and_then(|e| world.unit_motion(e.unit).velocity).unwrap_or_default();
    Some(Subject { position, velocity, alive, entity })
}

#[cfg(test)]
impl GoalController {
    pub(crate) fn test_reserve(&mut self, characters: usize) { self.reserve(characters).unwrap(); }
    pub(crate) fn test_give_up(&mut self, bot: usize, target: usize) { self.give_up(bot, target, 0.0) }
    pub(crate) fn test_given_up(&self, bot: usize, target: usize) -> bool { self.pursuit(bot, target).is_some_and(|p| p.spent >= p.budget) }
    pub(crate) fn test_new_life(&mut self, character: usize) { self.lives[character] = self.lives[character].wrapping_add(1) }
    pub(crate) fn test_end_life(&mut self, character: usize) { self.end_life(character) }
    pub(crate) fn test_capacity(&self) -> usize { self.capacity }
    pub(crate) fn test_roam_dest(&self, character: usize) -> Option<Vec3> {
        match self.bots.get(character)?.plan {
            Plan::Roam { dest } => Some(dest),
            _ => None,
        }
    }
    pub(crate) fn test_clear_plan(&mut self, character: usize) {
        if let Some(bot) = self.bots.get_mut(character) {
            bot.plan = Plan::Idle;
        }
    }
}

#[cfg(test)]
mod route_cadence {
    use super::*;

    #[test]
    fn a_new_life_keeps_the_character_route_schedule() {
        let mut goals = GoalController::new();
        goals.admit(2, GoalRegions::default(), 1).unwrap();
        for tick in [10, 18, 25] { assert!(goals.bots[0].take_route_credit(tick)); }
        goals.end_life(0);
        assert!(!goals.bots[0].take_route_credit(69));
        assert!(goals.bots[1].take_route_credit(69));
        assert!(goals.bots[0].take_route_credit(70));
        assert!(!goals.bots[0].take_route_credit(77));
        assert!(goals.bots[0].take_route_credit(78));
    }

    #[test]
    fn deathmatch_with_planning_graph_splits_wander_and_static_random_enemy_snapshots() {
        use super::super::table::StoredArg;
        use super::super::tests::arena::{Arena, ArenaRules, BotSpec, Op};
        use crate::ai::nav::swbf2::{
            graph::{Swbf2ArcInput, Swbf2HubInput},
            BarrierToggles, Swbf2Graph, Swbf2Nav,
        };
        use crate::gen::UnitBody;

        let make_bot = |team, x, z, human| BotSpec {
            team,
            pos: [x, 0.0, z],
            yaw: 0.0,
            hero: false,
            melee: false,
            health: 400.0,
            human,
            mixed: false,
        };
        let mut roster = vec![make_bot(1.0, 0.0, 0.0, false)];
        // Two distant enemies out of sensor range on opposing sides.
        roster.push(make_bot(2.0, 150.0, 0.0, true));
        roster.push(make_bot(2.0, -170.0, 0.0, true));
        let dm = Op::Add { team: 1, kind: "Deathmatch", weight: 1.0, target: StoredArg::None, extra: StoredArg::None };
        let mut arena = Arena::build(&roster, None, 99, ArenaRules::Goals(vec![dm]));
        let hubs = vec![
            Swbf2HubInput { name: Some("hub_center".into()), position: [0.0, 0.0, 0.0], radius: 4.0, slots: vec![0.0, 1.0], weight_counts: [0; 5], weights: Vec::new() },
            Swbf2HubInput { name: Some("hub_north".into()), position: [0.0, 0.0, 80.0], radius: 4.0, slots: vec![0.0], weight_counts: [0; 5], weights: Vec::new() },
            Swbf2HubInput { name: Some("hub_south".into()), position: [0.0, 0.0, -80.0], radius: 4.0, slots: vec![1.0], weight_counts: [0; 5], weights: Vec::new() },
        ];
        let arcs = vec![
            Swbf2ArcInput { name: None, start: 0.0, end: 1.0, filter_flags: 1, type_flags: 0 },
            Swbf2ArcInput { name: None, start: 0.0, end: 2.0, filter_flags: 1, type_flags: 0 },
        ];
        let graph = Swbf2Graph::new(1, &hubs, &arcs, None);
        let swbf2 = Swbf2Nav::new(Some(graph), BarrierToggles::default(), 0);
        arena.w.ai.battle_mut().unwrap().state.navigation.set_swbf2(Box::new(swbf2), 16, 0);
        arena.activate();

        // First choice establishes a static roam destination that does not track moving enemies.
        arena.run(5);
        let first = arena.bots().unwrap().test_roam_dest(0).expect("deathmatch chose a static destination");
        assert!(arena.bots().unwrap().hunting(0).is_none(), "deathmatch does not use continuous hunt");
        for idx in [1, 2] {
            let UnitBody::Soldier(s) = &mut arena.w.sim.store.unit.get_mut(arena.units[idx]).unwrap().soldier else { unreachable!() };
            s.position = [s.position[0], 0.0, 45.0];
            s.previous = s.position;
            s.settled = s.position;
        }
        arena.run(5);
        assert_eq!(arena.bots().unwrap().test_roam_dest(0), Some(first), "snapshot destination stays static while walking");

        // Sampling across repeated picks covers hub wander and both enemies (not only nearest).
        for (idx, x) in [(1, 150.0), (2, -170.0)] {
            let UnitBody::Soldier(s) = &mut arena.w.sim.store.unit.get_mut(arena.units[idx]).unwrap().soldier else { unreachable!() };
            s.position = [x, 0.0, 0.0];
            s.previous = s.position;
            s.settled = s.position;
        }
        let (mut saw_hub, mut saw_near_enemy, mut saw_far_enemy) = (0usize, 0usize, 0usize);
        let mut max_enemy_jitter = 0.0f64;
        for _ in 0..80 {
            {
                let UnitBody::Soldier(s) = &mut arena.w.sim.store.unit.get_mut(arena.units[0]).unwrap().soldier else { unreachable!() };
                s.position = [0.0, 0.0, 0.0];
                s.previous = s.position;
                s.settled = s.position;
            }
            arena.bots().unwrap().test_clear_plan(0);
            arena.step_one();
            if let Some(d) = arena.bots().unwrap().test_roam_dest(0) {
                if horizontal(d, Vec3::new(0.0, 0.0, 80.0)) <= 5.0 || horizontal(d, Vec3::new(0.0, 0.0, -80.0)) <= 5.0 {
                    saw_hub += 1;
                } else if (d.x - 150.0).abs() <= 10.5 && d.z.abs() <= 10.5 {
                    saw_near_enemy += 1;
                    max_enemy_jitter = max_enemy_jitter.max((d.x - 150.0).abs().max(d.z.abs()));
                } else if (d.x + 170.0).abs() <= 10.5 && d.z.abs() <= 10.5 {
                    saw_far_enemy += 1;
                    max_enemy_jitter = max_enemy_jitter.max((d.x + 170.0).abs().max(d.z.abs()));
                }
            }
        }
        assert!(saw_hub >= 15, "expected hub wander picks, got {saw_hub}");
        assert!(saw_near_enemy >= 5, "expected near enemy snapshots, got {saw_near_enemy}");
        assert!(saw_far_enemy >= 5, "expected far enemy snapshots, got {saw_far_enemy}");
        assert!(
            max_enemy_jitter > 2.0 && max_enemy_jitter <= 10.5,
            "enemy snapshots should be scattered across the +/-10 m jitter square, got max offset {max_enemy_jitter}"
        );
    }

    #[test]
    fn deathmatch_travel_jump_triggers_only_for_jet_units_with_fuel_above_border() {
        use super::super::table::StoredArg;
        use super::super::tests::arena::{Arena, ArenaRules, BotSpec, Op};
        use crate::ai::nav::swbf2::{
            graph::{Swbf2ArcInput, Swbf2HubInput},
            BarrierToggles, Swbf2Graph, Swbf2Nav,
        };
        use crate::gen::UnitBody;

        let make_bot = |team, x, z, human| BotSpec {
            team,
            pos: [x, 0.0, z],
            yaw: 0.0,
            hero: false,
            melee: false,
            health: 400.0,
            human,
            mixed: false,
        };
        let roster = vec![
            make_bot(1.0, 0.0, 0.0, false),
            make_bot(1.0, 2.0, 0.0, false),
            make_bot(2.0, 130.0, 0.0, true),
        ];
        let dm = Op::Add { team: 1, kind: "Deathmatch", weight: 1.0, target: StoredArg::None, extra: StoredArg::None };
        let mut arena = Arena::build(&roster, None, 77, ArenaRules::Goals(vec![dm]));
        let hubs = vec![
            Swbf2HubInput { name: Some("hub_a".into()), position: [0.0, 0.0, 0.0], radius: 4.0, slots: vec![0.0], weight_counts: [0; 5], weights: Vec::new() },
            Swbf2HubInput { name: Some("hub_b".into()), position: [0.0, 0.0, 120.0], radius: 4.0, slots: vec![0.0], weight_counts: [0; 5], weights: Vec::new() },
        ];
        let arcs = vec![Swbf2ArcInput { name: None, start: 0.0, end: 1.0, filter_flags: 1, type_flags: 0 }];
        let graph = Swbf2Graph::new(1, &hubs, &arcs, None);
        let swbf2 = Swbf2Nav::new(Some(graph), BarrierToggles::default(), 0);
        arena.w.ai.battle_mut().unwrap().state.navigation.set_swbf2(Box::new(swbf2), 16, 0);
        arena.activate();

        // Give bot 0 jet jump with full fuel above border; bot 1 has fuel at border.
        if let UnitBody::Soldier(s) = &mut arena.w.sim.store.unit.get_mut(arena.units[0]).unwrap().soldier {
            s.jet.jump = 10.0;
            s.jet.min_border = 0.0;
            s.fuel = 1.0;
        }
        if let UnitBody::Soldier(s) = &mut arena.w.sim.store.unit.get_mut(arena.units[1]).unwrap().soldier {
            s.jet.jump = 7.0;
            s.jet.min_border = 1.0;
            s.fuel = 1.0;
        }
        let (mut saw_jet_0, mut saw_jet_1) = (false, false);
        let mut jump_ticks = Vec::new();
        let mut prev_pending = false;
        for tick in 0..1800u32 {
            arena.step_one();
            let pending_0 = {
                let ctl = arena.controller(0);
                ctl.jet_pending || ctl.nav_control.as_ref().is_some_and(|n| n.jet_pending)
            };
            if pending_0 && !prev_pending {
                jump_ticks.push(tick);
            }
            prev_pending = pending_0;
            let jumped = |idx: usize| {
                let u = arena.w.sim.store.unit.get(arena.units[idx]).unwrap();
                let air = matches!(
                    &u.soldier,
                    UnitBody::Soldier(s) if matches!(s.air, Some(crate::gen::Air::Jump | crate::gen::Air::Jet))
                );
                let ctl = arena.controller(idx);
                let pending = ctl.jet_pending || ctl.nav_control.as_ref().is_some_and(|n| n.jet_pending);
                u.intent.jump || air || pending
            };
            if jumped(0) {
                saw_jet_0 = true;
            }
            if jumped(1) {
                saw_jet_1 = true;
            }
        }
        assert!(saw_jet_0, "jet-capable bot with fuel above border should trigger periodic travel jump");
        assert!(!saw_jet_1, "bot with fuel at min_border should defer travel jump instead of jumping");
        assert!(jump_ticks.len() >= 2, "expected multiple travel jumps across 30 s, got {jump_ticks:?}");
        for pair in jump_ticks.windows(2) {
            assert!(
                pair[1] - pair[0] >= 295,
                "travel jumps must wait at least 5.0 s between checks, got ticks {:?}",
                pair
            );
        }
    }
}
