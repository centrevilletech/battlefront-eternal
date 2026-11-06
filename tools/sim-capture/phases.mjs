// The phases of a world tick, in the order they run (the local server's step,
// src/runtime/local-server.js, around the multiplayer world's,
// src/runtime/multiplayer-world.js, around the game's, src/game.js): the
// units the simulation's port to Rust captures, replays and moves over, named
// once here and in rust/sim-replay/src/phase.rs (the same names, the same
// order; its tests hold the two together). A capture records a phase between
// markers the capture's module hook (tools/sim-capture/hooks.mjs) puts
// around the statement or block named here, without the game's files being
// changed.
export const PHASES = [
  ['edges', 'Edges', 'the snapshot host takes the clients\' edges in (host.advanceEdges)'],
  ['hitHistory', 'HitHistory', 'the hit history keeps the last tick\'s bodies (hitHistory.capture)'],
  ['players', 'Players', 'each player\'s commands merged from his mailbox: spawns, team, intents, look, boarding'],
  ['views', 'Views', 'the players\' views the AI\'s level of detail reads (updateViews)'],
  ['begin', 'Begin', 'the game step\'s start: the command, the dormant local player, water, the time, the view camera'],
  ['restorePose', 'RestorePose', 'hulls back to their tick poses (vehicles.restoreTickPose)'],
  ['ai', 'Ai', 'the AI (ai.update)'],
  ['unitTick', 'UnitTick', 'every unit\'s tick in factory order, with its water and the death regions'],
  ['vehicles', 'Vehicles', 'the hulls, two 1/120 s substeps on the ground (vehicles.update)'],
  ['turretSeats', 'TurretSeats', 'turrets and their gunners (turretSeats.update)'],
  ['gunner', 'Gunner', 'the local player\'s turret (gunner.update; never on a server world)'],
  ['remotes', 'Remotes', 'recon droids (remotes.update, networkRemotes.finish)'],
  ['separate', 'Separate', 'soldiers pushed apart (separateSoldiers)'],
  ['live', 'Live', 'live props: animated props, turrets\' idle sweep, traps, stations (live.update)'],
  ['ordnance', 'Ordnance', 'rounds in flight (ordnance.update)'],
  ['items', 'Items', 'items: pickups, mines, dropped weapons (items.update)'],
  ['battlefield', 'Battlefield', 'out of the battlefield too long (battlefield.update)'],
  ['battle', 'Battle', 'posts, battle rules, tickets and spawning (updateBattle)'],
  ['targetHud', 'TargetHud', 'the local player\'s target reticle (updateTargetHud)'],
  ['aiGrenades', 'AiGrenades', 'live grenades warn the soldiers near them (ai.grenadeUpdate)'],
  ['spawnFlush', 'SpawnFlush', 'the spawn screen hears the tick (humanSpawn.flush)'],
  ['consumedEdges', 'ConsumedEdges', 'the snapshot host settles the edges the tick used (host.consumedEdges)'],
  ['publish', 'Publish', 'snapshots captured, encoded and sent (host.publish)'],
].map(([name, rust, what]) => ({ name, rust, what }));

export const PHASE_NAMES = PHASES.map((p) => p.name);

// A combined native owner replaces these former JavaScript phase boundaries.
// Keep their names above so historical capture files still decode unchanged.
const nativeMembers = new Set(['restorePose','vehicles','turretSeats','gunner','remotes','separate']);
export const LIVE_PHASES = PHASES.filter(phase => !nativeMembers.has(phase.name));
