//! What the battle's rules ask of the rest of the world: the units
//! registry's team questions, the units and their combat (whether one
//! lives, what a Character controls, a kill), the hulls on the command
//! vehicles' pads, the spawn manager's counts, the live objects' buildings,
//! and the announcements the page shows. Each method is a call the
//! JavaScript makes; the replays serve them from captures
//! (rust/sim/tests/replay_battle.rs).

use super::mobile::{MobileEvent, Pad};
use super::posts::{PostEvent, PostId};
use super::rules::{Battle, Character};
use super::spawning::ClassicSpawning;
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

    // ---- The battle phase's glue (src/game.js updateBattle).

    /// units.all(): the registry's members in its order, as updateBattle
    /// reads them.
    fn members(&mut self) -> Vec<Member>;
    /// The Character that last hurt a destructible post's building
    /// (building.lastAttacker, "player" read as the local player).
    fn building_attacker(&mut self, building: u32) -> Option<u32>;
    /// The Character that last hurt a hull (vehicle.lastAttacker).
    fn vehicle_attacker(&mut self, vehicle: u32) -> Option<u32>;
    /// A kill region's victims as it goes: the living soldiers (not
    /// removed), then the living hulls.
    fn kill_region_victims(&mut self) -> Vec<Victim>;
    /// victim.kill(): combat.damage(Infinity), or a hull's setHealth(0). The
    /// death path that follows credits the battle and tells the spawn
    /// manager (onDeath).
    fn kill_victim(&mut self, victim: Victim, battle: &mut Battle, spawning: Option<&mut ClassicSpawning>);
    /// presentEffect(name, point): what the page shows.
    fn effect(&mut self, name: &str, at: [f64; 3]);
    /// A post captured, neutralized or lost: the audio's, the AI's
    /// (ai.postEvent, which may send Characters to new objectives) and the
    /// HUD's.
    fn post_event(&mut self, event: &PostEvent, characters: &mut [Character]);
    /// A command vehicle's post lost or changed hands: announced, and the
    /// AI's.
    fn mobile_event(&mut self, event: &MobileEvent, characters: &mut [Character]);

    // ---- What the page shows.

    /// battle.onOver(loser): Victory or Defeat.
    fn battle_over(&mut self, loser: f64);
}

/// A registry member as updateBattle reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Member {
    /// A soldier or droideka (unit.soldier).
    pub soldier: bool,
    pub alive: bool,
    pub removed: bool,
    pub seated: bool,
    pub held: bool,
    /// Its class captures posts (classDef.CapturePosts ?? 1, not 0).
    pub captures: bool,
    /// The local player while he drives or mans a gun.
    pub player_mounted: bool,
    pub team: f64,
    pub position: [f64; 3],
    /// Its Character's place (unit.character).
    pub character: Option<u32>,
}

/// One a kill region may kill.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Victim {
    Unit { id: u32, position: [f64; 3] },
    Hull { id: u32, position: [f64; 3] },
}
