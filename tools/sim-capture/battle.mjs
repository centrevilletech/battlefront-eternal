// The battle package's frames (tools/sim-capture/specs.mjs `battle`): the
// command posts' tick (src/commandposts.js CommandPosts.update), the battle's
// rules (src/battle.js Battle.update, its scores' tick with it) and leaving
// the battlefield (src/battlefield.js Battlefield.update).
// rust/sim/tests/replay_battle.rs reads them back.
//
// Until the store's generator can hold what these keep (objects keyed by
// team number, Maps keyed by numbers and by Characters), a frame's state is
// written as explicit values, one root for each object the Rust port
// holds, the same at the frame's start and end. A Character is named by
// its place in the battle's list, a unit by its registry id, a hull as the
// capture names hulls. What a frame's Rust reads of the rest of the world
// (the registry's team questions, what each Character's tallies see of
// him, the hulls on the command pads, the bodies the battlefield tests)
// rides with its state: those reads change nothing, and nothing in the
// frame changes them.
import { observe } from '../../src/battle-stats.js';
import { isCommandClass } from '../../src/mobile-posts.js';
import { RemoteDroids } from '../../src/remote.js';
import { Vehicle } from '../../src/vehicles.js';
import { PlayerCombat } from '../../src/combat.js';

const TEAMS = 8;
const indexOf = (characters, c) => (c ? characters.indexOf(c) : null);
const unitId = (u) => (u ? u.id : null);

// ---- Values.

// A built region (src/regions.js buildRegion): its numbers, the inverse's
// sixteen as an array.
const builtValue = (r) => r && { type: r.type, half: [...r.half], radius: r.radius, center: r.center, inverse: [...r.inverse.elements] };
// A region as the world file gives it.
const regionValue = (r) => r && { type: r.type, matrix: r.matrix ? [...r.matrix] : null, center: r.center ? [...r.center] : null, size: [...r.size] };

// A placed post as update() and the rules read and write it.
function postValue(post, live) {
  return {
    name: post.name, team: post.team, active: post.active, valueBleed: post.desc.valueBleed ?? null,
    neutralize: post.neutralize, progress: post.progress, toward: post.toward, counts: post.counts, contested: post.contested,
    building: post.building ? { index: live?.objectives?.indexOf(post.building) ?? -1, alive: post.building.alive } : null,
    capture: builtValue(post.capture), captureDesc: regionValue(post.desc.capture ?? null), killRegion: regionValue(post.desc.killRegion ?? null),
    rules: { neutralizeTime: post.rules.neutralizeTime, captureTime: post.rules.captureTime }, position: post.position,
    ai: aiValue(post.ai),
  };
}

// What the spawn manager reads of a placed post's AI record (src/ai/classic/
// goals.js postAI).
const pathValue = (p) => p && { present: Boolean(p.present), points: (p.points || []).map((q) => [...q]) };
function aiValue(ai) {
  if (!ai) return null;
  return { spawnPath: pathValue(ai.spawnPath), allyPath: pathValue(ai.allyPath), allyCount: ai.allyCount ?? 0, classSpawnPoints: ai.classSpawnPoints ?? 0,
    classSpawnWorld: (ai.classSpawnWorld || []).map((q) => [...q]), yaw: ai.yaw ?? 0 };
}

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

// A command vehicle's post, and the hulls on its pad.
function mobileValue(post) {
  const r = post.reported;
  return {
    slot: post.slot ? post.system.slots.indexOf(post.slot) : -1, label: post.label, team: post.team, active: post.active,
    vehicle: post.vehicle, life: post.life, yaw: post.yaw, radius: post.radius, speed: post.speed, lastPosition: post.lastPosition,
    desc: { name: post.desc.name, displayName: post.desc.displayName, valueBleed: post.desc.valueBleed },
    ai: { atk: [...post.ai.atk], def: [...post.ai.def], valueBleed: post.ai.valueBleed, points: post.ai.points.map((p) => [...p]), classSpawnPoints: post.ai.classSpawnPoints },
    reported: r ? { active: r.active, team: r.team, vehicle: r.vehicle, life: r.life } : null,
    pad: { variants: [...(post.slot?.variants?.values?.() ?? [])].map((v) => hullValue(v, post.classes)), vehicle: hullValue(post.slot?.vehicle ?? null, post.classes) },
    // Its class spawn points where its hull is now (a getter that casts).
    classSpawnWorld: post.vehicle && post.active ? post.ai.classSpawnWorld.map((q) => [...q]) : [],
  };
}

function postsValue(posts, live) {
  return {
    time: posts.time, teams: Boolean(posts.teams),
    posts: posts.posts.map((p) => postValue(p, live)),
    mobile: posts.mobile ? posts.mobile.posts.map(mobileValue) : null,
    events: posts.events.map((e) => ({ post: posts.posts.indexOf(e.post), kind: e.kind, from: e.from ?? null, by: e.by ?? null, destroyed: Boolean(e.destroyed) })),
  };
}

// A Character's end-of-match record (src/battle-stats.js), its victims by
// place.
function recordValue(record, characters) {
  if (!record) return null;
  return {
    anchor: record.anchor ? [record.anchor.x, record.anchor.y, record.anchor.z] : null, health: record.health, low: record.low,
    slots: record.slots.map((s) => ({
      teamKills: s.teamKills, vehiclesDestroyed: s.vehiclesDestroyed, spree: s.spree, survivalist: s.survivalist, lifeKills: s.lifeKills,
      lifeKillsBest: s.lifeKillsBest, headshots: s.headshots, deaths: s.deaths, life: s.life, lifeBest: s.lifeBest, camp: s.camp,
      campBest: s.campBest, escapes: s.escapes, victims: [...s.victims].map(([c, n]) => [characters.indexOf(c), n]),
      classes: s.classes.map((e) => ({ ...e })), weapons: s.weapons.map((e) => ({ ...e })), vehicles: s.vehicles.map((e) => ({ ...e })),
    })),
  };
}

function characterValue(c, characters) {
  return {
    index: c.index, team: c.team, human: c.human, name: c.name ?? null, className: c.className ?? null, unit: unitId(c.unit), hero: Boolean(c.hero),
    kills: c.kills, teamKills: c.teamKills, deaths: c.deaths, captures: c.captures, outOfBounds: c.outOfBounds, record: recordValue(c.record, characters),
  };
}

// Plain objects keyed by team number, as [team, value] pairs in their key
// order (undefined values kept).
const teamPairs = (o) => Object.entries(o).map(([t, v]) => [Number(t), v]);

function battleValue(b) {
  const ring = b.stats.ring;
  return {
    own: b.own, foe: b.foe, teams: teamPairs(b.teams).map(([t, v]) => [t, { units: v.units ?? 0, reinforcements: v.reinforcements ?? null, bleed: (v.bleed || []).map((r) => [...r]) }]),
    reinforcements: teamPairs(b.reinforcements), start: teamPairs(b.start), teamIds: [...b.teamIds], noPost: teamPairs(b.noPost),
    bleed: teamPairs(b.bleed), victoryClock: b.victoryClock, countdown: b.countdown, losing: b.losing, over: b.over, loser: b.loser,
    overTime: b.overTime, gameOver: b.gameOver, finished: b.finished,
    bleedRules: teamPairs(b.bleedRules).map(([t, rules]) => [t, rules.map((r) => [...r])]), bleedClock: teamPairs(b.bleedClock),
    stats: { time: b.stats.time, ring: { killers: ring.killers.map((c) => indexOf(b.characters, c)), times: ring.times, slots: ring.slots, head: ring.head, tail: ring.tail } },
    characters: b.characters.map((c) => characterValue(c, b.characters)),
  };
}

// What observe(character) sees of him, as the Rust port's Seen.
function seenValue(seen) {
  if (!seen) return null;
  return {
    position: seen.position ? [seen.position.x, seen.position.y, seen.position.z] : null, health: seen.health,
    weapon: seen.weapon ? [seen.weapon.key ?? null, seen.weapon.label ?? null] : null,
    vehicle: seen.vehicle ? [seen.vehicle.key ?? null, seen.vehicle.label ?? null] : null, classLabel: seen.classLabel ?? null,
  };
}

// The registry's answers for every team (src/registry.js).
const relations = (units) => Array.from({ length: TEAMS }, (_, a) => Array.from({ length: TEAMS }, (_, b) => units.relation(a, b)));
const captureTeams = (units) => Array.from({ length: TEAMS }, (_, t) => units.captureTeam(t));

// What the battlefield tests of a Character's body.
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

// ---- The spawn manager (src/ai/classic/spawning.js).

// A post in the manager's list (CommandPosts.all(): the placed ones, then
// the command vehicles'), by its place there.
const listed = (posts) => (posts.mobile?.posts.length ? [...posts.posts, ...posts.mobile.posts] : posts.posts);
const postPlace = (posts, p) => (p ? listed(posts).indexOf(p) : null);

function spawnCharacterValue(c, posts) {
  const sp = c.spawn;
  return {
    team: c.team, human: c.human, hero: Boolean(c.hero), name: c.name ?? null, className: c.className ?? null, classIndex: c.classIndex ?? null,
    slot: c.slot ?? null, managerIndex: c.managerIndex ?? null, unit: unitId(c.unit), objective: postPlace(posts, c.objective),
    spawn: sp ? { timer: sp.timer, wave: sp.wave, ordinal: sp.ordinal, post: postPlace(posts, sp.post), ready: sp.ready } : null,
  };
}

function spawningValue(s, posts) {
  return {
    A: s.A, B: s.B, Q: s.Q, waveCountdown: s.waveCountdown, wave: s.wave, stagger: s.stagger, released: s.released, localsCap: s.localsCap, demo: Boolean(s.demo),
    teams: s.teams.map((t) => ({
      index: t.index, faction: t.faction, unitCount: t.unitCount, tickets: t.tickets, classes: t.classes.map((c) => ({ name: c.name, target: c.target, hero: c.hero })),
      heroCount: t.heroCount, relations: [...t.relations], characters: t.characters.map((c) => s.characters.indexOf(c)), living: t.living, capacity: t.capacity,
    })),
    queues: [...s.queues].map(([p, q]) => [postPlace(posts, p), [...q]]),
  };
}

// Everything a call across the manager's boundary may change of the
// manager's (a forced kill runs the death path, onDeath and all).
function spawnAfter(game) {
  const s = game.ai.spawning;
  return {
    characters: s.characters.map((c) => spawnCharacterValue(c, game.posts)), spawning: spawningValue(s, game.posts),
    reinforcements: teamPairs(game.battle.reinforcements),
  };
}

function spawnState(game) {
  const s = game.ai.spawning;
  const b = game.battle;
  return {
    spawning: spawningValue(s, game.posts),
    characters: s.characters.map((c) => spawnCharacterValue(c, game.posts)),
    battleCharacters: b.characters === s.characters,
    reinforcements: teamPairs(b.reinforcements),
    posts: postsValue(game.posts, game.live),
    health: s.characters.map((c) => (c.unit ? s.health(c.unit) : null)),
    heroNames: s.teams.flatMap((t) => t.classes.filter((c) => c.hero).map((c) => [c.name, s.heroName?.(c.name) ?? null])),
  };
}

const spawnBoundaries = (game) => {
  const s = game.ai?.spawning;
  if (!s) return [];
  return [
    ['spawning.spawnUnit', s, 'spawnUnit', spawnAfter],
    ['spawning.forceKill', s, 'forceKill', spawnAfter],
    ...(s.objectives ? [['objectives.request', s.objectives, 'request', spawnAfter], ['objectives.republish', s.objectives, 'republish', spawnAfter]] : []),
  ];
};

// ---- The frames.

// CommandPosts.update(dt, capturers): the posts' counts and captures.
const postsUpdate = {
  method: 'CommandPosts.update',
  target: (game) => game.posts,
  state: (game, args) => ({
    posts: postsValue(game.posts, game.live),
    soldiers: (args[1] || []).map((u) => ({ team: u.team, position: u.position })),
    captureTeams: captureTeams(game.units),
  }),
};

// Battle.update(dt): the scores' tick, each team's bleed and countdown, the
// end. The command vehicles' posts refresh inside it (owned()).
const battleUpdate = {
  method: 'Battle.update',
  target: (game) => game.battle,
  state: (game) => {
    const b = game.battle;
    return {
      battle: battleValue(b),
      posts: postsValue(game.posts, game.live),
      active: b.characters.map((c) => Boolean(b.statsActive(c))),
      seen: b.characters.map((c) => seenValue(c.className || c.unit ? observe(c) : null)),
      alive: b.characters.map((c) => Boolean(c.unit?.alive)),
      living: Array.from({ length: TEAMS }, (_, t) => game.ai?.spawning?.teams?.[t]?.living ?? null),
      relations: relations(game.units),
    };
  },
};

// Battlefield.update(dt, characters): each Character's countdown outside.
const battlefieldBodies = (game) => game.battle.characters.map((c) => bodyValue(c.unit));
const battlefieldUpdate = {
  method: 'Battlefield.update',
  target: (game) => game.battlefield,
  state: (game) => ({
    characters: game.battle.characters.map((c) => ({ outOfBounds: c.outOfBounds, unit: unitId(c.unit) })),
    bodies: battlefieldBodies(game),
    boundary: game.battlefield.boundary.polygons.map((edges) => edges.map((e) => [e.x, e.z, e.nx, e.nz])),
  }),
  // A kill takes the body (and a hull's riders) with it.
  boundaries: () => {
    const after = (game) => ({ bodies: battlefieldBodies(game) });
    return [
      ['remotes.kill', RemoteDroids.prototype, 'kill', after],
      ['vehicle.damage', Vehicle.prototype, 'damage', after],
      ['combat.damage', PlayerCombat.prototype, 'damage', after],
    ];
  },
};

// ClassicSpawning.clock(dt): the wave and release clocks, the locals' cap.
const spawnClock = {
  method: 'ClassicSpawning.clock',
  target: (game) => game.ai?.spawning ?? null,
  state: (game) => ({ spawning: spawningValue(game.ai.spawning, game.posts), posts: postsValue(game.posts, game.live) }),
};

// ClassicSpawning.updateCharacters(dt): each Character's respawn: class,
// post, queue, wave, and into the world.
const spawnCharacters = {
  method: 'ClassicSpawning.updateCharacters',
  target: (game) => game.ai?.spawning ?? null,
  state: spawnState,
  boundaries: spawnBoundaries,
};

// ClassicSpawning.onDeath(character): a Character's body died (`place`:
// his place in the list).
const spawnDeath = {
  method: 'ClassicSpawning.onDeath',
  target: (game) => game.ai?.spawning ?? null,
  state: (game, args) => ({ ...spawnState(game), place: game.ai.spawning.characters.indexOf(args[0]) }),
  boundaries: spawnBoundaries,
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
