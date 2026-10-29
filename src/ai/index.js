// The bots' brain, as main.js asks for it (?ai=): "classic" runs the Classic
// AI (src/ai/classic, the 2004 game's behavior, docs/ai.md), "enhanced" the
// Enhanced AI (src/ai/enhanced: the Classic machinery with modern objective,
// focus, aim and difficulty decisions, docs/ai-enhanced.md), "stub" the
// foundation's stand-in, "off" none.
//
// The Classic AI reads the map's AI data (public/maps/<map>/ai-nav.json and
// ai-mission.json, from tools/zero_ai.py and tools/zero_ai_mission.py) and
// each side's class data (public/sides/<side>/ai.json, from
// tools/zero_ai_classes.py). A map without them still plays; the AI says what
// it is missing in the console.

import { Character } from "../battle.js";
import { selectAIMission } from "../mission.js";
import { ClassicRandom } from "./random.js";
import { createClassicAI, loadClassicModules } from "./classic/index.js";
import { EnhancedDifficulty, EnhancedSpawning, enhancedOptions } from "./enhanced/index.js";
import { StubAI } from "./stub.js";
import { traversalMask } from "./classic/nav/units.js";

// Population reasons about Characters; shared seats contain the corresponding
// living soldier lease. Resolve those dynamically so boarding and seat swaps
// immediately change objective sharing and driver-only vehicle restrictions.
export function objectiveCrew(character) {
  const lease = character?.unit?.seat;
  if (!lease || lease.released || lease.live === false) return null;
  const vehicle = lease.vehicle ?? lease.turret;
  if (!vehicle) return null;
  const leases = vehicle.seats ? vehicle.seats.map((seat) => seat.lease) : [vehicle.lease];
  const valid = (seat) => seat && !seat.released && seat.live !== false;
  const driverLease = vehicle.seats?.[0]?.lease;
  return {
    vehicle,
    isDriver: Boolean(lease.vehicle) && lease.index === 0,
    driver: valid(driverLease) ? driverLease.unit?.character ?? null : null,
    occupants: leases.filter(valid).map((seat) => seat.unit?.character).filter(Boolean),
  };
}

export function objectiveVehicleMask(vehicle) {
  const target = vehicle.aiTarget;
  const def = target?.classDef ?? vehicle.def ?? vehicle.classDef ?? {};
  return traversalMask({ navMask: target?.navMask, classDef: def.settings ?? def });
}

// Each living bot's aim curves, which its controller took at its spawn
// (curvesFor its team): again, for its team as the difficulty now sees it,
// after the human has changed teams.
export function refreshCurves(ai, controllers) {
  for (const controller of controllers) if (controller.unit) controller.difficulty = ai.curvesFor(controller.unit.team);
}

async function fetchJson(path) {
  const response = await fetch(path);
  if (!response.ok) throw new Error(`${path}: ${response.status}`);
  return response.json();
}

async function fetchBin(path) {
  const response = await fetch(path);
  if (!response.ok) throw new Error(`${path}: ${response.status}`);
  return response.arrayBuffer();
}

// The AI data for a map and its sides; missing files come back null.
export async function loadAIData(map, sides) {
  const optional = (promise) => promise.catch(() => null);
  const [nav, mission, ...classes] = await Promise.all([
    optional(fetchJson(`/maps/${map}/ai-nav.json`)),
    optional(fetchJson(`/maps/${map}/ai-mission.json`)),
    ...sides.map((side) => optional(fetchJson(`/sides/${side}/ai.json`))),
  ]);
  const navBin = nav?.bin?.file ? await optional(fetchBin(`/maps/${map}/${nav.bin.file}`)) : null;
  return { nav, navBin, mission, classes: Object.fromEntries(sides.map((side, i) => [side, classes[i]])) };
}

// mode: "classic" | "enhanced" | "stub" | "off". world: the services docs/ai.md lists.
// options: { map, sides, playerTeam, enemyTeam, tier, heroes, seed, stub }.
// Returns null (off), the stub, or the Classic (or Enhanced) AI's handle for main.js:
//   update(h)                 every simulation step, before units tick
//   aim(unit, h)              after a bot's soldier moves, before it fires
//   spawned(character)        from world.spawnUnit
//   died(character)           a Character's unit died (spends its ticket)
//   beforeBattle(dt) / afterBattle(dt)   around battle.update (the spawn
//                             manager's clock, then each Character)
//   grenadeUpdate(round, owner)  each live grenade, each step (danger warnings)
//   postEvent(event)          a command post was captured or neutralized, or
//                             a command vehicle's was lost or changed hands
//   humanReady(character, choice)  the player is ready to respawn
//   awaitingHuman(character)  the spawn manager waits for his choice
//   switchTeam(character, team)    he moves team while he has no unit
export async function createAI(mode, world, options) {
  if (mode === "off") return null;
  if (mode === "stub") return new StubAI(options.stub);
  if (mode !== "classic" && mode !== "enhanced") throw new Error(`unknown AI mode ${mode}`);
  const enhanced = mode === "enhanced";
  const data = await loadAIData(options.map, options.sides);
  for (const [name, value] of Object.entries({ "ai-nav.json": data.nav, "ai-mission.json": data.mission })) {
    if (!value) console.warn(`AI: ${options.map} has no ${name}; run the AI extraction for it`);
  }
  const modules = await loadClassicModules();
  const { ClassicNav } = modules.nav;
  const { ClassicDifficulty } = modules.difficulty;
  const { ClassicSpawning, callsFromMapMission } = modules.spawning;
  const { attachPostAI, missionControls } = modules.goals;

  // The two generators the 2004 game keeps: the AI's decisions (A), and the
  // game's own (B: squad rolls, evade cubes, recovery, jet retries).
  const seed = options.seed >>> 0;
  world.randomB = world.gameRandom = new ClassicRandom((seed ^ 0x9e3779b9) >>> 0);
  world.nav = data.nav ? new ClassicNav(data.nav, data.navBin, { posts: world.posts.posts }) : ClassicNav.empty(options.map);
  world.classData = data.classes;

  // The mission's AI calls, in source order: the setters for difficulty,
  // spawning, and the mission controls. The file holds every script of the
  // map; the battle's is the selected mission's (src/mission.js).
  const mission = selectAIMission(data.mission, world.mission);
  const calls = mission?.scripts?.[mission.mission]?.calls ?? callsFromMapMission(world.mission);
  if (mission) attachPostAI(world.posts.posts, mission, world.spawns);
  // Command vehicles are posts while they live (src/mobile-posts.js), from
  // the match's start: their classes' spawn points, weights and ValueBleed.
  world.posts.attachVehicles?.(world.vehicleSystem, mission?.commandClasses);
  world.missionControls = missionControls(calls);
  world.difficulty = enhanced
    ? new EnhancedDifficulty({ tier: options.tier, playerTeam: options.playerTeam })
    : new ClassicDifficulty({ tier: options.tier, playerTeam: options.playerTeam });
  world.difficulty.apply(calls);

  // The roster: each team's Characters, the player first among his own.
  const Spawning = enhanced ? EnhancedSpawning : ClassicSpawning;
  world.spawning = new Spawning({
    unitScale: options.unitScale ?? 1,
    calls,
    posts: () => world.posts.all?.() ?? world.posts.posts,
    random: world.random,
    humans: [{ team: options.playerTeam }],
    heroes: options.heroes,
    names: mission?.names,
    heroName: (name) => world.factory.sides[world.factory.sideOf(name)]?.units[name]?.displayName ?? null,
    battle: world.battle,
    spawnUnit: (...args) => world.spawnUnit(...args),
    makeCharacter: (slot, team, human) => new Character(slot, team, human),
    hasClass: (name) => world.factory.sideOf(name) != null && world.factory.prepared(name),
    forceKill: (character) => character.unit?.combat?.damage(Infinity),
    units: world.units,
    crew: objectiveCrew,
    vehicleMask: objectiveVehicleMask,
    start: false,
    ...(enhanced ? { now: () => world.time ?? 0 } : {}),
  });
  world.spawning.begin();
  world.battle.characters = world.spawning.characters;

  const classic = createClassicAI(world, { modules, ...(enhanced ? enhancedOptions(world, { tier: options.tier, seed }) : {}) });
  await classic.ready;
  modules.vehicles?.adaptClassicAI?.(classic.ai);
  for (const character of world.spawning.characters) classic.attach(character);
  const combat = classic.ai.combat;
  const spawning = world.spawning;
  return {
    mode,
    enhanced,
    classic,
    spawning,
    nav: world.nav,
    difficulty: world.difficulty,
    update(h) {
      classic.update(h);
      // Mounted bodies skip Unit.tick; their control still needs the same
      // filtered aim before the seat latches and fires its input batch.
      for (const controller of classic.controllers) {
        if (controller.unit?.lease?.controllable === controller.unit) combat?.aim?.(controller, h);
      }
      modules.vehicles?.serviceOf?.(classic.ai)?.updateDecay(h);
      if (enhanced) spawning.objectives?.tick?.(h);
    },
    playerCommand(kind, character = world.battle.human) {
      return classic.playerCommand(character, kind);
    },
    aim(unit, h) {
      const controller = unit.controller;
      if (controller && combat?.aim) combat.aim(controller, h);
    },
    spawned(character) {
      classic.spawned(character);
    },
    died(character) {
      if (!character) return;
      spawning.onDeath(character);
      classic.died(character);
    },
    beforeBattle(dt) {
      spawning.clock(dt);
    },
    afterBattle(dt) {
      spawning.updateCharacters(dt);
    },
    grenadeUpdate(round, owner) {
      combat?.grenadeUpdate?.(round, round.position, owner && typeof owner === "object" ? owner : null, round.contactTime ?? Infinity);
    },
    postEvent(event) {
      const objectives = spawning.objectives;
      if (!objectives) return;
      if (event.kind === "captured") objectives.onOwnerChanged(event.post, 0, event.by, spawning.characters);
      else if (event.kind === "neutralized") objectives.onOwnerChanged(event.post, event.from, 0, spawning.characters);
      // A command vehicle's post (src/mobile-posts.js): gone with the
      // vehicle, or taken with a seat.
      else if (event.kind === "lost") objectives.onDeactivated(event.post, spawning.characters);
      else if (event.kind === "owner") objectives.onOwnerChanged(event.post, event.from, event.to, spawning.characters);
    },
    humanReady(character, choice = {}) {
      if (character && spawning.awaitingHuman(character)) spawning.onHumanReady(character, choice);
    },
    // Whether the human waits for the spawn screen's choice: no unit, and
    // his respawn timer done.
    awaitingHuman(character) {
      return Boolean(character) && spawning.awaitingHuman(character);
    },
    // The human moves to another team while he has no unit: the rosters,
    // his class and post (spawning.switchTeam), and the aim curves of the
    // bots on his side, the living ones included.
    presentHumanTeam(team) {
      world.difficulty.playerTeam = team;
      refreshCurves(classic.ai, classic.controllers);
    },
    switchTeam(character, team) {
      if (!character || !spawning.switchTeam(character, team)) return false;
      world.difficulty.playerTeam = team;
      refreshCurves(classic.ai, classic.controllers);
      return true;
    },
    debug: classic.debug,
  };
}
