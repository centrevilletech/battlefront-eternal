//! What the battle's rules ask of the rest of the world: the units
//! registry's team questions, the units and their combat (whether one
//! lives, what a Character controls, a kill), the hulls on the command
//! vehicles' pads, the spawn manager's counts, the live objects' buildings,
//! and the announcements the page shows. Each method is a call the
//! JavaScript makes; the replays serve them from captures
//! (rust/sim/tests/replay_battle.rs).

use super::mobile::Pad;
use super::posts::PostId;
use super::rules::Character;
use super::stats::Seen;

/// What the battlefield reads of a Character's body (battlefield.js:76).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Body {
    pub removed: bool,
    pub alive: bool,
    /// unit.combat.dead.
    pub combat_dead: bool,
    /// The hull or turret he sits in, else his body: where it is.
    pub place: [f64; 3],
    /// The hull he sits in (a turret is not one).
    pub vehicle: Option<u32>,
    /// The recon droid he flies, if it lives: where it is.
    pub droid: Option<[f64; 3]>,
}

pub trait BattleWorld {
    // ---- The registry (src/registry.js).

    /// units.relation(a, b).
    fn relation(&mut self, a: f64, b: f64) -> f64;
    /// units.captureTeam(team).
    fn capture_team(&mut self, team: f64) -> f64;

    // ---- Units and their combat.

    /// unit.alive.
    fn unit_alive(&mut self, unit: u32) -> bool;
    /// observe(character) (src/battle-stats.js): what his tallies see of
    /// him this tick; None without a body (or a removed one).
    fn observe(&mut self, character: u32, unit: Option<u32>) -> Option<Seen>;
    /// What the battlefield reads of his body.
    fn body(&mut self, unit: u32) -> Body;
    /// unit.combat.dead, now.
    fn combat_dead(&mut self, unit: u32) -> bool;
    /// unit.combat.damage(Infinity).
    fn kill(&mut self, unit: u32);
    /// unit.combat.remotes.kill(droid): the droid he flies.
    fn kill_droid(&mut self, unit: u32);
    /// vehicle.lastAttacker = null; vehicle.damage(Infinity).
    fn wreck(&mut self, vehicle: u32);

    // ---- The hulls (src/vehicles.js).

    /// The hulls on a vehicle pad (a command vehicle's post's).
    fn pad(&mut self, slot: usize) -> Pad;
    /// vehicle.position.
    fn vehicle_position(&mut self, vehicle: u32) -> [f64; 3];
    /// A command vehicle's post's class spawn points where its hull is now
    /// (src/mobile-posts.js commandSpawnPoint: through the hull's frame, then
    /// back toward its centre to the first static surface), by its place
    /// among the command vehicles' posts.
    fn class_spawn_world(&mut self, post: usize) -> Vec<Vec<f64>>;

    // ---- Spawning (the world's spawnUnit, the AI's objectives).

    /// world.spawnUnit(character, className, post, at, yaw): a body for
    /// him, or None; the world sets his unit and class on him as it makes
    /// one (src/game.js spawnUnit).
    fn spawn_unit(&mut self, character: &mut Character, place: u32, class_name: &str, post: Option<PostId>, at: [f64; 3], yaw: Option<f64>) -> Option<u32>;
    /// forceKill(character): character.unit.combat.damage(Infinity).
    fn force_kill(&mut self, place: u32, unit: u32);
    /// unit.combat.health (0 without one).
    fn health(&mut self, unit: u32) -> f64;
    /// objectives.request(character): a bot draws a fresh objective
    /// (src/ai/classic/goals.js ClassicObjectives), which it may set on him.
    fn request_objective(&mut self, character: &mut Character, place: u32);
    /// objectives.republish(character).
    fn republish(&mut self, character: &mut Character, place: u32);
    /// heroName(className): a hero class's display name.
    fn hero_name(&mut self, class_name: &str) -> Option<String>;

    // ---- The spawn manager (ai.spawning) and the live objects.

    /// ai.spawning.teams[team].living, if there is a spawn manager.
    fn living(&mut self, team: f64) -> Option<f64>;
    /// A destructible post's building: whether it stands.
    fn building_alive(&mut self, building: u32) -> bool;

    // ---- What the page shows.

    /// battle.onOver(loser): Victory or Defeat.
    fn battle_over(&mut self, loser: f64);
}
