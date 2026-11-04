//! What the battle's rules ask of the rest of the world: the units
//! registry's team questions, the units and their combat (whether one
//! lives, what a Character controls, a kill), the hulls on the command
//! vehicles' pads, the spawn manager's counts, the live objects' buildings,
//! and the announcements the page shows. Each method is a call the
//! JavaScript makes; the replays serve them from captures
//! (rust/sim/tests/replay_battle.rs).

use super::mobile::{MobileEvent, Pad};
use super::stats::Seen;
use crate::gen::{Building, Character, MobilePost, PostEvent, SpawnPost, Store, Unit, Vehicle};
use crate::store::{Ref, StrId};

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
    pub vehicle: Option<Ref<Vehicle>>,
    /// The recon droid he flies, if it lives: where it is.
    pub droid: Option<[f64; 3]>,
}

/// Services the command posts read. Kept separate so the live post phase
/// can use its own store without taking ownership of the battle's rules,
/// Characters or spawn manager during a nested query.
pub trait PostsWorld {
    fn capture_team(&mut self, team: f64) -> f64;
    fn building_alive(&mut self, building: Ref<Building>) -> bool;
    fn pad(&mut self, slot: f64) -> Pad;
    fn vehicle_position(&mut self, vehicle: Ref<Vehicle>) -> [f64; 3];
    fn class_spawn_world(&mut self, s: &Store, post: Ref<MobilePost>) -> Vec<[f64; 4]>;
}

impl<T: BattleWorld + ?Sized> PostsWorld for T {
    fn capture_team(&mut self, team: f64) -> f64 { BattleWorld::capture_team(self, team) }
    fn building_alive(&mut self, building: Ref<Building>) -> bool { BattleWorld::building_alive(self, building) }
    fn pad(&mut self, slot: f64) -> Pad { BattleWorld::pad(self, slot) }
    fn vehicle_position(&mut self, vehicle: Ref<Vehicle>) -> [f64; 3] { BattleWorld::vehicle_position(self, vehicle) }
    fn class_spawn_world(&mut self, _: &Store, post: Ref<MobilePost>) -> Vec<[f64; 4]> { BattleWorld::class_spawn_world(self, post) }
}

/// The spawn manager's foreign actions; its teams, Characters, queues
/// and class choices remain in the caller's store.
pub trait SpawningWorld: PostsWorld {
    fn health(&mut self, unit: Ref<Unit>) -> f64;
    fn force_kill(&mut self, s: &mut Store, character: Ref<Character>, unit: Ref<Unit>);
    fn request_objective(&mut self, s: &mut Store, character: Ref<Character>);
    fn republish(&mut self, s: &mut Store, character: Ref<Character>);
    fn hero_name(&mut self, class_name: StrId) -> Option<StrId>;
    fn spawn_unit(&mut self, s: &mut Store, character: Ref<Character>, class_name: Option<StrId>, post: SpawnPost, at: [f64; 3], yaw: Option<f64>) -> Option<Ref<Unit>>;
}

impl<T: BattleWorld + ?Sized> SpawningWorld for T {
    fn health(&mut self, unit: Ref<Unit>) -> f64 { BattleWorld::health(self, unit) }
    fn force_kill(&mut self, s: &mut Store, character: Ref<Character>, unit: Ref<Unit>) { BattleWorld::force_kill(self, s, character, unit); }
    fn request_objective(&mut self, s: &mut Store, character: Ref<Character>) { BattleWorld::request_objective(self, s, character); }
    fn republish(&mut self, s: &mut Store, character: Ref<Character>) { BattleWorld::republish(self, s, character); }
    fn hero_name(&mut self, class_name: StrId) -> Option<StrId> { BattleWorld::hero_name(self, class_name) }
    fn spawn_unit(&mut self, s: &mut Store, character: Ref<Character>, class_name: Option<StrId>, post: SpawnPost, at: [f64; 3], yaw: Option<f64>) -> Option<Ref<Unit>> {
        BattleWorld::spawn_unit(self, s, character, class_name, post, at, yaw)
    }
}

pub trait BattleWorld {
    // ---- The registry (src/registry.js).

    /// units.relation(a, b).
    fn relation(&mut self, a: f64, b: f64) -> f64;
    /// The team a soldier of `team` captures for: units.captureTeam(team)
    /// when the posts have the registry's table, else his own.
    fn capture_team(&mut self, team: f64) -> f64;

    // ---- Units and their combat.

    /// unit.alive.
    fn unit_alive(&mut self, unit: Ref<Unit>) -> bool;
    /// observe(character) (src/battle-stats.js): what his tallies see of
    /// him this tick; None without a body (or a removed one).
    fn observe(&mut self, character: Ref<Character>, unit: Option<Ref<Unit>>) -> Option<Seen>;
    /// What the battlefield reads of his body.
    fn body(&mut self, unit: Ref<Unit>) -> Body;
    /// unit.combat.dead, now.
    fn combat_dead(&mut self, unit: Ref<Unit>) -> bool;
    /// unit.combat.damage(Infinity): the death path follows (the battle's
    /// credit, the spawn manager's onDeath), applied to the store.
    fn kill(&mut self, s: &mut Store, unit: Ref<Unit>);
    /// unit.combat.remotes.kill(droid): the droid he flies.
    fn kill_droid(&mut self, s: &mut Store, unit: Ref<Unit>);
    /// vehicle.lastAttacker = null; vehicle.damage(Infinity).
    fn wreck(&mut self, s: &mut Store, vehicle: Ref<Vehicle>);

    // ---- The hulls (src/vehicles.js).

    /// The hulls on a vehicle pad (a command vehicle's post's: its index).
    fn pad(&mut self, slot: f64) -> Pad;
    /// vehicle.position.
    fn vehicle_position(&mut self, vehicle: Ref<Vehicle>) -> [f64; 3];
    /// A command vehicle's post's class spawn points where its hull is now
    /// (src/mobile-posts.js commandSpawnPoint: through the hull's frame, then
    /// back toward its centre to the first static surface).
    fn class_spawn_world(&mut self, post: Ref<MobilePost>) -> Vec<[f64; 4]>;

    // ---- Spawning (the world's spawnUnit, the AI's objectives).

    /// world.spawnUnit(character, className, post, at, yaw): a body for
    /// him, or None; the world sets his unit and class on him as it makes
    /// one (src/game.js spawnUnit).
    #[allow(clippy::too_many_arguments)]
    fn spawn_unit(&mut self, s: &mut Store, character: Ref<Character>, class_name: Option<StrId>, post: SpawnPost, at: [f64; 3], yaw: Option<f64>) -> Option<Ref<Unit>>;
    /// forceKill(character): character.unit.combat.damage(Infinity).
    fn force_kill(&mut self, s: &mut Store, character: Ref<Character>, unit: Ref<Unit>);
    /// unit.combat.health (0 without one).
    fn health(&mut self, unit: Ref<Unit>) -> f64;
    /// objectives.request(character): a bot draws a fresh objective
    /// (src/ai/classic/goals.js ClassicObjectives), which it may set on him.
    fn request_objective(&mut self, s: &mut Store, character: Ref<Character>);
    /// objectives.republish(character).
    fn republish(&mut self, s: &mut Store, character: Ref<Character>);
    /// heroName(className): a hero class's display name.
    fn hero_name(&mut self, class_name: StrId) -> Option<StrId>;

    // ---- The spawn manager (ai.spawning) and the live objects.

    /// ai.spawning.teams[team].living, if there is a spawn manager.
    fn living(&mut self, team: f64) -> Option<f64>;
    /// A destructible post's building: whether it stands.
    fn building_alive(&mut self, building: Ref<Building>) -> bool;

    // ---- The battle phase's glue (src/game.js updateBattle).

    /// units.all(): the registry's members in its order, as updateBattle
    /// reads them.
    fn members(&mut self) -> Vec<Member>;
    /// The Character that last hurt a destructible post's building
    /// (building.lastAttacker, "player" read as the local player).
    fn building_attacker(&mut self, building: Ref<Building>) -> Option<Ref<Character>>;
    /// The Character that last hurt a hull (vehicle.lastAttacker).
    fn vehicle_attacker(&mut self, vehicle: Ref<Vehicle>) -> Option<Ref<Character>>;
    /// A kill region's victims as it goes: the living soldiers (not
    /// removed), then the living hulls.
    fn kill_region_victims(&mut self) -> Vec<Victim>;
    /// victim.kill(): combat.damage(Infinity), or a hull's setHealth(0). The
    /// death path that follows credits the battle and tells the spawn
    /// manager (onDeath).
    fn kill_victim(&mut self, s: &mut Store, victim: Victim);
    /// presentEffect(name, point): what the page shows.
    fn effect(&mut self, name: &str, at: [f64; 3]);
    /// A post captured, neutralized or lost: the audio's, the AI's
    /// (ai.postEvent, which may send Characters to new objectives) and the
    /// HUD's.
    fn post_event(&mut self, s: &mut Store, event: &PostEvent);
    /// A command vehicle's post lost or changed hands: announced, and the
    /// AI's.
    fn mobile_event(&mut self, s: &mut Store, event: &MobileEvent);

    // ---- The multiplayer roster (src/runtime/multiplayer-roster.js).
    /// classic.detach(character): the AI lets him go.
    fn ai_detach(&mut self, s: &mut Store, character: Ref<Character>);
    /// classic.attach(character): the AI takes him (a bot).
    fn ai_attach(&mut self, s: &mut Store, character: Ref<Character>);
    /// His body out of the battle (the roster's retire): out of any seat
    /// (unplaced), no longer a player's, removed.
    fn retire_unit(&mut self, s: &mut Store, character: Ref<Character>, unit: Ref<Unit>);
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
    /// Its Character (unit.character).
    pub character: Option<Ref<Character>>,
}

/// One a kill region may kill.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Victim {
    Unit { id: Ref<Unit>, position: [f64; 3] },
    Hull { id: Ref<Vehicle>, position: [f64; 3] },
}
