// The battle package's frames (tools/sim-capture/specs.mjs `battle`): the
// command posts' tick (src/commandposts.js CommandPosts.update), the battle's
// rules (src/battle.js Battle.update, its scores' tick with it), leaving the
// battlefield (src/battlefield.js Battlefield.update) and the Classic spawn
// manager (src/ai/classic/spawning.js), and (battlePhase) the battle phase
// whole. rust/sim/tests/replay_battle.rs reads them back.
//
// A frame's state is the store's (rust/sim/schema/battle.mjs), as the
// generated exporters write it (cap.export): the battle with its Characters,
// the command posts, the command vehicles' posts and the spawn manager.
// What the Rust reads of the rest of the world rides with it as plain
// values, keyed by a Character's place in the battle's list, a post's place
// in its list, a unit's registry id or a hull's handle: the registry's team
// questions, what each Character's tallies see of him, the hulls on the
// command pads, the bodies the battlefield tests. Those reads change
// nothing, and nothing in the frame changes them.
import { observe } from '../../src/battle-stats.js';
import { isCommandClass } from '../../src/mobile-posts.js';
import { RemoteDroids } from '../../src/remote.js';
import { Vehicle } from '../../src/vehicles.js';
import { PlayerCombat } from '../../src/combat.js';

const TEAMS = 8;
const placeOf = (game, c) => (c ? game.battle.characters.indexOf(c) : null);

// ---- The store's state.

// The command vehicles' posts (none: an empty list, as no MobilePosts).
const NO_MOBILE = { posts: [] };
function state(game, cap) {
  const s = game.ai?.spawning ?? null;
  return {
    battle: cap.export('battle', game.battle),
    posts: cap.export('commandPosts', game.posts),
    mobile: cap.export('mobilePosts', game.posts.mobile ?? NO_MOBILE),
    spawning: s ? cap.export('spawning', s) : null,
  };
}

// ---- What the frames read of the rest of the world.

// What observe(character) sees of him.
function seenValue(seen) {
  if (!seen) return null;
  return {
    position: seen.position ? [seen.position.x, seen.position.y, seen.position.z] : null, health: seen.health,
    weapon: seen.weapon ? [seen.weapon.key ?? null, seen.weapon.label ?? null] : null,
    vehicle: seen.vehicle ? [seen.vehicle.key ?? null, seen.vehicle.label ?? null] : null, classLabel: seen.classLabel ?? null,
  };
}

// The registry's answers for every team (src/registry.js); a soldier's
// capture team as the posts ask it (his own without the registry's table).
const relations = (units) => Array.from({ length: TEAMS }, (_, a) => Array.from({ length: TEAMS }, (_, b) => units.relation(a, b)));
const captureTeams = (game) => Array.from({ length: TEAMS }, (_, t) => (game.posts.teams ? game.posts.teams.captureTeam(t) : t));

// A hull on a command pad as its post reads it (src/mobile-posts.js).
function hullValue(v, classes) {
  if (!v) return null;
  return {
    id: v, alive: Boolean(v.alive), command: isCommandClass(v.def), generation: v.aiTarget?.generation ?? 0,
    team: v.team | 0, controllingTeam: v.controllingTeam | 0, seated: Boolean(v.seats?.some((seat) => seat.lease && !seat.lease.released)),
    yaw: v.yaw, name: v.name, label: v.label || v.name, radius: v.size ? v.size.length() / 2 : v.radius ?? 0, speed: v.speed ?? 0,
    valueBleedSetting: Number.parseInt(v.def?.settings?.ValueBleed, 10), record: classes?.[v.name] ?? null, position: v.position,
  };
}

// Each command vehicle's post's pad, the hull it last had (where it is
// now) and its class spawn points where that hull is (a getter that casts).
function padsValue(game) {
  return (game.posts.mobile?.posts ?? []).map((post) => ({
    variants: [...(post.slot?.variants?.values?.() ?? [])].map((v) => hullValue(v, post.classes)),
    vehicle: hullValue(post.slot?.vehicle ?? null, post.classes),
    last: post.vehicle ? { id: post.vehicle, position: post.vehicle.position } : null,
    classSpawnWorld: post.vehicle && post.active ? post.ai.classSpawnWorld.map((q) => [...q]) : [],
  }));
}

// What the battlefield tests of a soldier's body.
function bodyValue(unit) {
  if (!unit) return null;
  const hull = unit.seat?.vehicle ?? unit.seat?.turret ?? null;
  const droid = unit.combat?.remotes?.piloted;
  const flown = droid && !droid.dead ? droid : null;
  const at = hull ? hull.position : unit.position;
  return {
    id: unit.id, removed: Boolean(unit.removed), alive: Boolean(unit.alive), combatDead: Boolean(unit.combat?.dead),
    place: [at.x, at.y, at.z], vehicle: unit.seat?.vehicle ?? null, droid: flown ? [flown.position.x, flown.position.y, flown.position.z] : null,
  };
}

// The rules' reads: whose tallies run, what each sees, whose body lives,
// each team's living, the registry's relations, the posts' buildings.
function rulesReads(game) {
  const b = game.battle, s = game.ai?.spawning ?? null;
  return {
    active: b.characters.map((c) => Boolean(b.statsActive(c))),
    seen: b.characters.map((c) => seenValue(c.className || c.unit ? observe(c) : null)),
    alive: b.characters.map((c) => (c.unit ? [c.unit.id, Boolean(c.unit.alive)] : null)),
    living: Array.from({ length: TEAMS }, (_, t) => s?.teams?.[t]?.living ?? null),
    relations: relations(game.units),
    pads: padsValue(game),
    buildings: game.posts.posts.map((p) => (p.building ? Boolean(p.building.alive) : null)),
  };
}

// The spawn manager's reads: each Character's health, the hero names.
function spawnReads(game) {
  const s = game.ai?.spawning ?? null, b = game.battle;
  return {
    // Whether the manager asks the AI's objectives (`this.objectives?.request`).
    objectives: Boolean(s?.objectives),
    health: b.characters.map((c) => (c.unit ? [c.unit.id, s ? s.health(c.unit) : c.unit.combat?.health ?? 0] : null)),
    heroNames: s ? s.teams.flatMap((t) => t.classes.filter((c) => c.hero).map((c) => [c.name, s.heroName?.(c.name) ?? null])) : [],
  };
}

// The state a call across a frame's boundary leaves (a kill's death path:
// the credit, the manager's onDeath; a spawn: the Character's body).
const after = (game, cap) => state(game, cap);

const spawnBoundaries = (game) => {
  const s = game.ai?.spawning;
  if (!s) return [];
  return [
    ['spawning.spawnUnit', s, 'spawnUnit', after],
    ['spawning.forceKill', s, 'forceKill', after],
    ...(s.objectives ? [['objectives.request', s.objectives, 'request', after], ['objectives.republish', s.objectives, 'republish', after]] : []),
  ];
};

// ---- The frames.

// CommandPosts.update(dt, capturers): the posts' counts and captures.
const postsUpdate = {
  method: 'CommandPosts.update',
  target: (game) => game.posts,
  state: (game, args, cap) => ({
    ...state(game, cap),
    soldiers: (args[1] || []).map((u) => ({ team: u.team, position: u.position })),
    captureTeams: captureTeams(game),
    buildings: game.posts.posts.map((p) => (p.building ? Boolean(p.building.alive) : null)),
  }),
};

// Battle.update(dt): the scores' tick, each team's bleed and countdown, the
// end. The command vehicles' posts refresh inside it (owned()).
const battleUpdate = {
  method: 'Battle.update',
  target: (game) => game.battle,
  state: (game, args, cap) => ({ ...state(game, cap), ...rulesReads(game) }),
};

// Battlefield.update(dt, characters): each Character's countdown outside.
// A kill takes the body (and a hull's riders) with it: every soldier's body
// after it, and the battle as the death path left it.
const battlefieldUpdate = {
  method: 'Battlefield.update',
  target: (game) => game.battlefield,
  state: (game, args, cap) => ({
    ...state(game, cap),
    bodies: game.battle.characters.map((c) => bodyValue(c.unit)),
    boundary: game.battlefield.boundary.polygons.map((edges) => edges.map((e) => [e.x, e.z, e.nx, e.nz])),
  }),
  boundaries: () => {
    const killed = (game, cap) => ({ ...state(game, cap), bodies: game.units.all().filter((u) => u.soldier).map(bodyValue) });
    return [
      ['remotes.kill', RemoteDroids.prototype, 'kill', killed],
      ['vehicle.damage', Vehicle.prototype, 'damage', killed],
      ['combat.damage', PlayerCombat.prototype, 'damage', killed],
    ];
  },
};

// ClassicSpawning.clock(dt): the wave and release clocks, the locals' cap.
const spawnClock = {
  method: 'ClassicSpawning.clock',
  target: (game) => game.ai?.spawning ?? null,
  state: (game, args, cap) => ({ ...state(game, cap), pads: padsValue(game) }),
};

// ClassicSpawning.updateCharacters(dt): each Character's respawn: class,
// post, queue, wave, and into the world.
const spawnCharacters = {
  method: 'ClassicSpawning.updateCharacters',
  target: (game) => game.ai?.spawning ?? null,
  state: (game, args, cap) => ({ ...state(game, cap), ...spawnReads(game), pads: padsValue(game) }),
  boundaries: spawnBoundaries,
};

// ClassicSpawning.onDeath(character): a Character's body died (`place`: his
// place in the list).
const spawnDeath = {
  method: 'ClassicSpawning.onDeath',
  target: (game) => game.ai?.spawning ?? null,
  state: (game, args, cap) => ({ ...state(game, cap), ...spawnReads(game), pads: padsValue(game), place: placeOf(game, args[0]) }),
  boundaries: spawnBoundaries,
};

// The battle phase (src/game.js updateBattle) whole: everything the frames
// above read, and what its glue reads of the registry's members, the
// buildings' and hulls' last attackers and a kill region's victims. A world
// step's dt is 1/60 (game.step's FixedClock).
const lastAttacker = (game, unit) => {
  const c = unit === 'player' ? game.playerUnit?.character : unit?.character;
  return c ? placeOf(game, c) : null;
};
function memberValue(game, u) {
  const p = u.position;
  return {
    soldier: Boolean(u.soldier), alive: Boolean(u.alive), removed: Boolean(u.removed), seated: Boolean(u.seat), held: Boolean(u.held),
    captures: (u.classDef?.CapturePosts ?? 1) !== 0, playerMounted: u === game.playerUnit && Boolean(game.driver?.active || game.gunner?.active),
    team: u.team ?? 0, position: p ? [p.x, p.y, p.z] : [NaN, NaN, NaN], character: u.character ? placeOf(game, u.character) : null,
  };
}
const phaseFrame = {
  phase: 'battle',
  state: (game, args, cap) => ({
    dt: 1 / 60, ai: Boolean(game.ai),
    ...state(game, cap), ...rulesReads(game), ...spawnReads(game),
    members: game.units.all().map((u) => memberValue(game, u)),
    attackers: game.posts.posts.map((p) => (p.building ? lastAttacker(game, p.building.lastAttacker) : null)),
    hulls: (game.posts.mobile?.posts ?? []).map((p) => (p.reported?.vehicle ? lastAttacker(game, p.reported.vehicle.lastAttacker) : null)),
    victims: [
      ...game.units.all().filter((u) => u.soldier && u.alive && !u.removed).map((u) => ({ unit: u.id, position: [u.position.x, u.position.y, u.position.z] })),
      ...(game.vehicles?.vehicles || []).filter((v) => v.alive).map((v) => ({ hull: v, position: [v.position.x, v.position.y, v.position.z] })),
    ],
    captureTeams: captureTeams(game),
  }),
  boundaries: (game) => [
    ...(game.ai ? [['ai.postEvent', game.ai, 'postEvent', after]] : []),
    ...spawnBoundaries(game),
    ['combat.damage', PlayerCombat.prototype, 'damage', after],
    ['vehicle.setHealth', Vehicle.prototype, 'setHealth', after],
  ],
};

// What makes a tick worth sampling for these frames, beyond deaths and
// spawns: a post changing hands or going down, a Character's respawn timer
// running out (his preparation: class, post and queue), a countdown
// outside the battlefield, and the battle's end.
let last = null;
function events(game, note) {
  const b = game.battle;
  const now = {
    posts: game.posts.posts.map((p) => `${p.team}:${p.active}`),
    mobile: (game.posts.mobile?.posts ?? []).map((p) => `${p.team}:${p.active}:${p.life}`),
    timers: b.characters.map((c) => c.spawn?.timer ?? 0),
    outside: b.characters.map((c) => c.outOfBounds >= 0),
    over: b.over, gameOver: b.gameOver,
  };
  if (last) {
    now.posts.forEach((p, i) => { if (p !== last.posts[i]) note('post', game.posts.posts[i].name); });
    now.mobile.forEach((p, i) => { if (p !== last.mobile[i]) note('mobilePost', i); });
    now.timers.forEach((t, i) => { if (last.timers[i] > 0 && !(t > 0)) note('prepare', i); });
    now.outside.forEach((o, i) => { if (o || last.outside[i]) note('outside', i); });
    if (now.over !== last.over || now.gameOver !== last.gameOver) note('over', b.loser ?? 0);
  }
  last = now;
}

export const battle = {
  frames: [postsUpdate, battleUpdate, battlefieldUpdate, spawnClock, spawnCharacters, spawnDeath],
  events,
};

// The battle phase whole, alone (its state holds what the frames above
// hold, so a corpus of it is not doubled by theirs).
export const battlePhase = {
  frames: [phaseFrame],
  events,
};
