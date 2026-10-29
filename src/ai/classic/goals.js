// Classic objectives: which command post a bot is sent to, and when it is
// sent somewhere else (docs/ai-population.md).
//
// There is no commander. Each bot draws its own objective: from the posts it
// cannot spawn at (enemy, neutral, or its own without a spawn path), weighted
// by each post's authored Value_ATK_* (team 1) or Value_DEF_* (team 2) for its
// team's faction. When nothing is left with a positive weight, any post will
// do. A bot asks again when it dies, when it is activated, when it boards or
// leaves a vehicle, when its post changes hands or goes inactive, and when its
// patrol finds the post has become a spawn point for its team. When a post
// friendly to a team turns neutral, up to five of that team's bots within 60 m
// are sent to retake it. Vehicle drivers hand their objective to their AI
// passengers.
//
// Posts here are the runtime CommandPosts (src/commandposts.js) or plain
// records: { name, team, position: {x, y, z}, active, desc, ai }, where `ai`
// carries the post's AI properties from ai-mission.json (attachPostAI).
// `relation(a, b)` is team a's signed relation to team b (1 friend, 0
// neutral, -1 enemy), as the mission's SetTeamAs* calls leave it; it is
// one-directional, as the 2004 game keeps it.

import { buildRegion } from "../../regions.js";
import { drawRange } from "./difficulty.js";

const f32 = Math.fround;
const FLT_MAX = 3.4028234663852886e38;

export const NONE = 0;
export const ALLIANCE = 1;
export const EMPIRE = 2;
export const REPUBLIC = 3;
export const CIS = 4;
export const LOCALS = 5;

// Traversal categories (class AISizeType) and the posts' *Ban bits.
export const TRAVEL = Object.freeze({ SOLDIER: 1, HOVER: 2, SMALL: 4, MEDIUM: 8, HUGE: 16, FLYER: 32 });
const SIZE_TYPES = { soldier: 1, hover: 2, small: 4, medium: 8, huge: 16, flyer: 32 };

// The objective arrays are fixed at 16 posts in the 2004 game; more would
// overrun them. The port keeps going and reports it (validatePosts).
export const MAX_POSTS = 16;
export const RECAPTURE_RADIUS = 60;
export const RECAPTURE_QUERY_CAP = 10;
export const RECAPTURE_LIMIT = 5;

// SetTeamName's parser: the faction for a team name. "all"/"alliance",
// "imp"/"empire", "rep"/"republic" and "cis" (any case) name the four sides;
// "neutral" is none; everything else, "locals" and "" included, is Locals.
const FACTION_NAMES = { all: 1, alliance: 1, imp: 2, empire: 2, rep: 3, republic: 3, cis: 4, neutral: 0 };
export function factionOf(name) {
  if (!name) return LOCALS;
  return FACTION_NAMES[String(name).toLowerCase()] ?? LOCALS;
}

// A class's traversal mask from its AISizeType (any case; unknown is soldier).
export function travelMaskOf(sizeType) {
  if (!sizeType) return TRAVEL.SOLDIER;
  return SIZE_TYPES[String(sizeType).toLowerCase()] ?? TRAVEL.SOLDIER;
}

// ---------------------------------------------------------------- post data

const DEFAULT_VALUES = () => [100, 100, 100, 100, 100, 100];

// A post's AI properties: the attached ai-mission.json record, or what can be
// said from map.json alone (class-default weights of 100, no bans, the spawn
// path if map.json resolved it).
export function postAI(post) {
  if (post.ai) return post.ai;
  const desc = post.desc || {};
  post.ai = {
    order: null,
    atk: DEFAULT_VALUES(),
    def: DEFAULT_VALUES(),
    bans: 0,
    valueBleed: desc.valueBleed ?? 100,
    spawnPath: desc.spawnPath ? { name: desc.spawnPath, present: !!desc.spawnPoints?.length, points: desc.spawnPoints || [] } : null,
    allyPath: null,
    allyCount: 0,
    classSpawnPoints: 0,
    classSpawnWorld: [],
    capture: desc.capture ? captureOf(desc.capture) : null,
    radius: 0,
    yaw: desc.matrix ? Math.atan2(desc.matrix[8], desc.matrix[10]) : 0,
  };
  return post.ai;
}

// half: the size as the 2004 game builds the region (src/regions.js), which
// the objective offset reads: a cylinder's x and z are its radius.
function captureOf(region) {
  const m = region.matrix;
  return { type: region.type, size: region.size, half: buildRegion(region).half, matrix: m, center: region.center || [m[12], m[13], m[14]] };
}

// Joins ai-mission.json's post records to the runtime posts by name (and,
// for unnamed posts, by position). `spawns` (map.json) supplies spawn path
// points for posts without a record.
export function attachPostAI(posts, mission, spawns = {}) {
  const records = mission?.posts || [];
  const byName = new Map(records.filter((r) => r.name).map((r) => [r.name.toLowerCase(), r]));
  const lower = Object.fromEntries(Object.entries(spawns || {}).map(([k, v]) => [k.toLowerCase(), v]));
  const at = (post) =>
    records.find((r) => !r.name && r.position && post.position &&
      Math.hypot(r.position[0] - post.position.x, r.position[1] - post.position.y, r.position[2] - post.position.z) < 0.01);
  posts.forEach((post, i) => {
    // By its world name; an unnamed one (Hoth's probe droid drop, which the
    // network calls "#5") by where it stands. Its runtime name used to be
    // looked up instead: no record matched, the unnamed post took the
    // default weights (100 for every side, where it says 0), and most of
    // the Empire's bots, its AT-ATs included, were sent to it for good.
    const worldName = typeof post.desc?.name === "string" ? post.desc.name : post.name;
    const record = (worldName ? byName.get(worldName.toLowerCase()) : at(post)) || null;
    if (!record) {
      delete post.ai;
      const ai = postAI(post);
      ai.order = i;
      if (ai.spawnPath && !ai.spawnPath.points.length) {
        const points = lower[ai.spawnPath.name.toLowerCase()] || [];
        ai.spawnPath = { name: ai.spawnPath.name, present: points.length > 0, points };
      }
      return;
    }
    const path = (p) => (p ? { name: p.name, present: !!p.present, points: p.points || [] } : null);
    post.ai = {
      order: record.order,
      atk: record.values.atk.slice(),
      def: record.values.def.slice(),
      bans: record.bans | 0,
      valueBleed: record.valueBleed,
      spawnPath: path(record.spawnPath),
      allyPath: path(record.allyPath),
      allyCount: record.allyCount | 0,
      classSpawnPoints: record.classSpawnPoints | 0,
      classSpawnWorld: record.classSpawnWorld || [],
      capture: record.captureRegion?.present ? captureOf(record.captureRegion) : null,
      radius: record.radius || 0,
      yaw: record.yaw ?? 0,
    };
  });
  return posts;
}

// Posts the 2004 game's post iterator visits: the active ones, in order.
export function activePosts(posts) {
  return posts.filter((post) => post.active !== false);
}

export function validatePosts(posts) {
  const count = activePosts(posts).length;
  return { count, overflow: count > MAX_POSTS };
}

export function ownerOf(post) {
  return post.team | 0;
}

// ---------------------------------------------------------------- predicates

// Can `team` spawn at this post? Its own post with a spawn path or class
// spawn points, or a post held by a team friendly to it that has an ally
// path. Neutral and inactive posts never qualify.
export function friendlySpawn(post, team, relation) {
  if (post.active === false) return false;
  const owner = ownerOf(post);
  if (!owner) return false;
  const ai = postAI(post);
  if (owner === team) return !!ai.spawnPath?.present || ai.classSpawnPoints > 0;
  return relation(owner, team) > 0 && !!ai.allyPath?.present;
}

// The objective weight of a post for a team: by team index, not by which team
// attacks. Team 1 reads the ATK value for its faction, team 2 the DEF value,
// team 3 the DEF value when it has a faction; every other team weighs 0.
export function postWeight(post, team, faction) {
  const ai = postAI(post);
  if (team === 1) return ai.atk[faction] ?? 0;
  if (team === 2) return ai.def[faction] ?? 0;
  if (team === 3 && faction > 0) return ai.def[faction] ?? 0;
  return 0;
}

// The port's objective focus (docs/objectives.md, "The AI"): a standing
// destructible post held by an enemy weighs this many times its authored
// value in the draw, so more of the attackers go for the shield generator
// or the Techno Union ships, but only while it is on the team's front: the
// candidate nearest one of the posts the team spawns at. Further back it
// keeps its authored value, so the posts in front of it are drawn as often
// as the 2004 game draws them: on Endor the bunker sits behind the
// Empire's cp6 and cp4, and drawn three times over it sent most of the
// Rebels past those posts, which they then never took. The 2004 game
// draws by the values alone.
export const OBJECTIVE_ATTACK_BIAS = 3;
function standingEnemyObjective(post, team, relation) {
  return Boolean(post.building?.alive) && relation(team, ownerOf(post)) < 0;
}
function drawWeight(post, team, faction, front) {
  const weight = postWeight(post, team, faction);
  return front.has(post) ? f32(weight * OBJECTIVE_ATTACK_BIAS) : weight;
}

// The standing enemy objectives on the team's front: for each post the
// team can spawn at, the drawable candidate nearest it (horizontally), when
// that is one.
const frontScratch = new Set();
function objectiveFront(list, kept, team, faction, relation) {
  frontScratch.clear();
  for (const spawn of list) {
    const from = spawn.position;
    if (!from || !friendlySpawn(spawn, team, relation)) continue;
    let best = Infinity;
    let nearest = null;
    for (let i = 0; i < list.length; i++) {
      const at = list[i].position;
      if (!kept[i] || !at || !(postWeight(list[i], team, faction) > 0)) continue;
      const dx = at.x - from.x;
      const dz = at.z - from.z;
      const d = dx * dx + dz * dz;
      if (d < best) {
        best = d;
        nearest = list[i];
      }
    }
    if (nearest && standingEnemyObjective(nearest, team, relation)) frontScratch.add(nearest);
  }
  return frontScratch;
}

// ---------------------------------------------------------------- selection

// A bot's objective. Candidates are the posts it can't spawn at, in world
// order; a driver also skips posts that ban its vehicle's traversal
// category; `previous` is skipped when asked. The weights add up in float32;
// a draw in [0, total) picks the first post whose running total reaches it.
// With no positive total, any active post (unfiltered) is picked uniformly,
// again avoiding `previous` when asked, but only when there are two or more.
export function selectObjective({
  posts,
  team,
  faction,
  random,
  relation,
  driver = false,
  vehicleMask = 0,
  excludePrevious = false,
  previous = null,
}) {
  const list = activePosts(posts);
  const kept = new Array(list.length).fill(false);
  for (let i = 0; i < list.length; i++) {
    const post = list[i];
    if (friendlySpawn(post, team, relation)) continue;
    kept[i] = !(driver && postAI(post).bans & vehicleMask) && !(excludePrevious && post === previous);
  }
  const front = objectiveFront(list, kept, team, faction, relation);
  let total = 0;
  for (let i = 0; i < list.length; i++) if (kept[i]) total = f32(drawWeight(list[i], team, faction, front) + total);
  if (total > 0) {
    const draw = drawRange(random, 0, total);
    let sum = 0;
    for (let i = 0; i < list.length; i++) {
      if (!kept[i]) continue;
      sum = f32(drawWeight(list[i], team, faction, front) + sum);
      if (draw <= sum) return list[i];
    }
  }
  if (list.length > 1) {
    for (;;) {
      const post = list[random.int(0, list.length - 1)];
      if (!excludePrevious || post !== previous) return post;
    }
  }
  return null;
}

// Where an AI spawns: uniformly among the posts its team can spawn at (no
// draw when there is just one), or nowhere.
export function selectSpawnPost({ posts, team, random, relation }) {
  const eligible = activePosts(posts).filter((post) => friendlySpawn(post, team, relation));
  if (!eligible.length) return null;
  return eligible[random.int(0, eligible.length - 1)];
}

// An AI keeps its post while it stays a valid spawn point for its team.
export function retainSpawnPost(post, { posts, team, random, relation }) {
  if (post && post.active !== false && friendlySpawn(post, team, relation)) return post;
  return selectSpawnPost({ posts, team, random, relation });
}

function distanceSq(a, b) {
  const x = a.x - b.x;
  const y = a.y - b.y;
  const z = a.z - b.z;
  return f32(x * x + y * y + z * z);
}

// The human's post when the current one no longer serves: the eligible post
// nearest the old one; with no post yet, the eligible post nearest any post
// held by an enemy of the human's team (nothing when no enemy holds a post).
export function humanSpawnPost(post, { posts, team, relation }) {
  const list = activePosts(posts);
  if (post && post.active !== false) {
    if (friendlySpawn(post, team, relation)) return post;
    let best = FLT_MAX;
    let pick = null;
    for (const other of list) {
      if (!friendlySpawn(other, team, relation)) continue;
      const d = distanceSq(other.position, post.position);
      if (best > d) {
        best = d;
        pick = other;
      }
    }
    return pick;
  }
  let best = FLT_MAX;
  let pick = null;
  for (const other of list) {
    if (!friendlySpawn(other, team, relation)) continue;
    for (const enemy of list) {
      if (!(relation(team, ownerOf(enemy)) < 0)) continue;
      const d = distanceSq(other.position, enemy.position);
      if (best > d) {
        best = d;
        pick = other;
      }
    }
  }
  return pick;
}

// ---------------------------------------------------------------- destinations

// The objective's centre: its capture region's origin, else the post itself.
export function objectiveCenter(post, out = { x: 0, y: 0, z: 0 }) {
  const capture = postAI(post).capture;
  if (capture) {
    out.x = capture.center[0];
    out.y = capture.center[1];
    out.z = capture.center[2];
  } else {
    out.x = post.position.x;
    out.y = post.position.y;
    out.z = post.position.z;
  }
  return out;
}

// A patrol's offset from the centre, in world axes: z first, in
// [-size.z, size.z), then x in [-size.x, size.x) of the built region, y 0. Without a capture
// region both ranges are 3 m. No rotation or containment test: the point
// need not be inside an oriented region.
export function objectiveOffset(post, random, out = { x: 0, y: 0, z: 0 }) {
  const capture = postAI(post).capture;
  const sx = capture ? f32((capture.half ?? capture.size)[0]) : 3;
  const sz = capture ? f32((capture.half ?? capture.size)[2]) : 3;
  out.z = drawRange(random, -sz, sz);
  out.x = drawRange(random, -sx, sx);
  out.y = 0;
  return out;
}

// Has a bot reached its objective point (centre + offset)? Distances are
// horizontal and the comparisons strict. With a capture region: inside it,
// or within half the bot's nominal forward speed. Without: within
// max(10, bot radius + post radius + 5), plus 20 when the post itself moves.
//   actor: { position, forwardSpeed, radius }
//   inside: (position) => bool, the capture region test (Post.contains)
//   postRadius / postSpeed: the post object's collision radius and speed
export function objectiveArrived(post, offset, actor, { inside = null, postRadius = 0, postSpeed = 0 } = {}) {
  const center = objectiveCenter(post);
  const dx = actor.position.x - (center.x + offset.x);
  const dz = actor.position.z - (center.z + offset.z);
  const d = f32(dx * dx + dz * dz);
  if (postAI(post).capture) {
    if (inside && inside(actor.position)) return true;
    const half = f32(actor.forwardSpeed * 0.5);
    return d < f32(half * half);
  }
  const r = Math.max(10, f32(f32(postRadius + actor.radius) + 5));
  if (postSpeed > 0) return d < f32(f32(r + 20) * f32(r + 20));
  return d < f32(r * r);
}

// ---------------------------------------------------------------- objectives

// Objective assignment for a team's bots. Characters are the Classic roster
// records (spawning.js): { team, human, unit, controller, objective }.
//
//   relation(a, b), faction(team): from the mission (ClassicSpawning has both)
//   crew(character): null on foot, else { vehicle, isDriver, driver, occupants }
//     with driver/occupants as Characters (vehicle code)
//   vehicleMask(vehicle): the vehicle class's traversal mask (AISizeType)
//   units: the registry (units.query) for the recapture response
//   travelMask(unit): a unit's class traversal mask (default from AISizeType)
//   onPublish(character, post): hands the objective to the bot's controller
//     (default: character.controller.publishObjective(post))
export class ClassicObjectives {
  constructor({
    posts = [],
    random,
    relation,
    faction = () => 0,
    crew = null,
    vehicleMask = null,
    units = null,
    travelMask = null,
    onPublish = null,
  }) {
    this.postList = posts;
    this.random = random;
    this.relation = relation;
    this.faction = faction;
    this.crew = crew;
    this.vehicleMask = vehicleMask;
    this.units = units;
    this.travelMask = travelMask || ((unit) => travelMaskOf(unit?.classDef?.AISizeType));
    this.onPublish = onPublish;
  }

  get posts() {
    return typeof this.postList === "function" ? this.postList() : this.postList;
  }

  // Draw an objective for this character (no publication).
  select(character, excludePrevious = false, previous = null) {
    const seat = this.crew ? this.crew(character) : null;
    const driver = !!seat?.isDriver;
    return selectObjective({
      posts: this.posts,
      team: character.team,
      faction: this.faction(character.team),
      random: this.random,
      relation: this.relation,
      driver,
      vehicleMask: driver && this.vehicleMask ? this.vehicleMask(seat.vehicle) : 0,
      excludePrevious,
      previous,
    });
  }

  // A fresh objective, as the 2004 game's request does it: a driver draws
  // one and hands it to its AI co-occupants; a passenger takes its AI
  // driver's; anyone else draws its own.
  request(character, excludePrevious = false, previous = null) {
    const seat = this.crew ? this.crew(character) : null;
    if (seat?.isDriver) {
      const post = this.select(character, excludePrevious, previous);
      this.publish(character, post);
      for (const other of seat.occupants || []) {
        if (other && other !== character && !other.human) this.publish(other, post);
      }
      return post;
    }
    if (seat) {
      const driver = seat.driver;
      if (driver && !driver.human) {
        const post = driver.objective ?? null;
        this.publish(character, post);
        return post;
      }
    }
    const post = this.select(character, excludePrevious, previous);
    this.publish(character, post);
    return post;
  }

  // The bot's controller gets the objective as a new command (when the bot
  // has a body and a controller); the Character remembers it either way.
  publish(character, post) {
    const controller = character.controller;
    if (character.unit && controller) {
      if (this.onPublish) this.onPublish(character, post);
      else if (controller.publishObjective) controller.publishObjective(post);
      else controller.onObjective?.(post);
    }
    character.objective = post;
  }

  // On spawn or controller attachment: the existing objective again, no draw.
  republish(character) {
    this.publish(character, character.objective ?? null);
  }

  // A supplied post (unboarding with a stored objective, attached controllers).
  assign(character, post) {
    this.publish(character, post);
  }

  // Boarding or leaving a vehicle: a fresh objective when the vehicle changed.
  onBoarded(character, previous, vehicle) {
    if (previous !== vehicle && !character.human) this.request(character);
  }

  // Every AI of `team` sent to `post` draws again (no exclusion).
  reassign(post, team, characters) {
    for (const character of characters) {
      if (character.team === team && !character.human && character.objective === post) this.request(character);
    }
  }

  // A friendly post turned neutral: up to five of the team's bots near it are
  // sent to retake it. The query is the registry's, in its order, capped at
  // ten allied units before any test; humans, flyers and units whose class
  // the post bans are passed over.
  recruit(post, team) {
    if (!this.units) return 0;
    const found = this.units.query(post.position, RECAPTURE_RADIUS, [], (unit) => this.relation(team, unit.team) > 0) || [];
    const capped = found.slice(0, RECAPTURE_QUERY_CAP);
    let sent = 0;
    for (const unit of capped) {
      if (sent >= RECAPTURE_LIMIT) break;
      if (!unit.controller || unit.human) continue;
      if (unit.kind === 5) continue; // flyers stay on their own business
      if (postAI(post).bans & this.travelMask(unit)) continue;
      const character = unit.character;
      if (!character) continue;
      this.publish(character, post);
      sent++;
    }
    return sent;
  }

  // A post changed hands. For each team whose relation to the owner crossed
  // between positive and not, its bots sent there draw again; a team that
  // lost a friendly post to neutral also recruits nearby bots to retake it.
  onOwnerChanged(post, oldOwner, newOwner, characters) {
    if (oldOwner === newOwner) return;
    for (let team = 1; team < 8; team++) {
      const before = this.relation(team, oldOwner);
      const after = this.relation(team, newOwner);
      if ((before < 1 && after > 0) || (before >= 1 && after < 1)) {
        this.reassign(post, team, characters);
        if (before > 0 && after === 0) this.recruit(post, team);
      }
    }
  }

  // A post went inactive (destroyed): everyone sent there draws again.
  onDeactivated(post, characters) {
    for (let team = 1; team < 8; team++) this.reassign(post, team, characters);
  }

  // The patrol's one-time check, 5 s after it starts: is the objective gone,
  // or now a spawn point for the bot's team?
  stale(character) {
    const post = character.objective;
    return !post || post.active === false || friendlySpawn(post, character.team, this.relation);
  }

  // If stale, draw a replacement that avoids the old post. True when it did
  // (the patrol then ends).
  revalidate(character) {
    if (!this.stale(character)) return false;
    this.request(character, true, character.objective ?? null);
    return true;
  }
}

// ---------------------------------------------------------------- Classic AI binding

// The services the Classic entry point (index.js) binds from Population:
// objective requests and the friendly-spawn test, through the match's
// ClassicObjectives (world.objectives, or world.spawning.objectives).
export function classicBinding(ai) {
  const world = ai?.world || {};
  const objectives = () => world.objectives ?? world.spawning?.objectives ?? null;
  const relationOf = () => {
    const o = objectives();
    if (o?.relation) return o.relation;
    if (world.relation) return world.relation;
    return (a, b) => (a === b ? (a ? 1 : 0) : !a || !b ? 0 : -1);
  };
  return {
    requestObjective(character, excludePrevious = false, previous = null) {
      const o = objectives();
      return o ? o.request(character, excludePrevious, previous) : null;
    },
    friendlySpawnEligible(post, team) {
      return !!post && friendlySpawn(post, team, relationOf());
    },
  };
}

// ---------------------------------------------------------------- mission controls

// Mission-wide AI settings the scripts set, applied in call order over the
// 2004 game's defaults. Aggressiveness is stored and nothing in the 2004
// game is known to read it: keep it, never use it as a multiplier.
export function missionControls(calls) {
  const controls = {
    aggressiveness: {}, // team -> float32, default 1
    dense: { explicit: false, value: false },
    attackingTeam: 1,
    voTeam: 0, // the voice-over manager's team index, attackingTeam - 1
    snipe: { attacker: 140, defender: 140 },
    stayInTurrets: false,
    stationaryWait: 10, // seconds before an AI leaves a turret (10000 once told to stay)
    vehicleNotifyRadius: 50,
    formation: { near: 0, far: 3 },
    blindJetJumps: true,
    flyHeight: { min: null, max: null, playerMin: null, playerMax: null },
  };
  const int = (v) => Math.trunc(f32(Number(v) || 0));
  for (const call of calls || []) {
    const name = Array.isArray(call) ? call[0] : call.name;
    const args = (Array.isArray(call) ? call[1] : call.args) || [];
    switch (name) {
      case "SetTeamAggressiveness":
        controls.aggressiveness[int(args[0])] = f32(Number(args[1]));
        break;
      case "SetDenseEnvironment":
      case "SetUrbanEnvironment": {
        const v = args[0];
        if (typeof v === "string") {
          const s = v.toLowerCase();
          if (s === "true" || s === "false") controls.dense = { explicit: true, value: s === "true" };
        } else if (v != null) {
          controls.dense = { explicit: true, value: !!v };
        }
        break;
      }
      case "SetAttackingTeam":
        controls.attackingTeam = int(args[0]);
        controls.voTeam = controls.attackingTeam - 1;
        break;
      case "SetAttackerSnipeRange":
        controls.snipe.attacker = int(args[0]);
        break;
      case "SetDefenderSnipeRange":
        controls.snipe.defender = int(args[0]);
        break;
      case "SetStayInTurrets":
        if (f32(Number(args[0]) || 0) !== 0) {
          controls.stayInTurrets = true;
          controls.stationaryWait = 10000;
        } else {
          controls.stayInTurrets = false; // the wait time stays as it was
        }
        break;
      case "SetStationaryWaitTime":
        controls.stationaryWait = f32(Number(args[0]) || 0);
        break;
      case "SetAIVehicleNotifyRadius":
        controls.vehicleNotifyRadius = f32(Number(args[0]) || 0);
        break;
      case "SetAIFormationRadius":
        controls.formation = { near: f32(Number(args[0]) || 0), far: f32(Number(args[1]) || 0) };
        break;
      case "SetAllowBlindJetJumps":
        controls.blindJetJumps = f32(Number(args[0]) || 0) !== 0;
        break;
      case "SetMinFlyHeight":
        controls.flyHeight.min = f32(Number(args[0]));
        break;
      case "SetMaxFlyHeight":
        controls.flyHeight.max = f32(Number(args[0]));
        break;
      case "SetMinPlayerFlyHeight":
        controls.flyHeight.playerMin = f32(Number(args[0]));
        break;
      case "SetMaxPlayerFlyHeight":
        controls.flyHeight.playerMax = f32(Number(args[0]));
        break;
      default:
        break;
    }
  }
  return controls;
}
