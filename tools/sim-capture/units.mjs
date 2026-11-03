// The units package's frames (tools/sim-capture/specs.mjs `units`): a unit's
// tick (src/units.js Unit.tick, every unit's call, on Unit.prototype), the
// registry's queries (src/registry.js Units.raycast and Units.query) and
// the soldiers pushed apart (src/game.js separateSoldiers, the `separate`
// phase). rust/sim/tests/replay_units.rs reads them back.
//
// A unit's tick writes the unit through the store's exporter (its schema,
// rust/sim/schema/units.mjs), the same before and after each call across
// its boundary. The store holds values where the JavaScript shares objects
// (a box's `half` is its `halfSize`; combat's aimOverride is the intent's
// `aim`; the input's jetDirection the intent's): a capture writes each such
// vector as a copy of its own, as the store holds it. A tick is written in
// full for every droideka, every unit that is dead or not alive, and an
// eighth of the rest by turns (by id); the others' frames carry their
// calls and a `skipped` state.
//
// The queries and the separation read a few fields of every member: their
// states carry those (`hitValue`), and a raycast or a query, made many
// times a tick, carries them on the first few calls of a sampled tick
// (REGISTRY_FRAMES); the rest carry only their arguments and answer.
import { Soldier } from '../../src/soldier.js';
import { Droideka } from '../../src/droideka.js';

const REGISTRY_FRAMES = 2;

// ---- Values.
// A hit shape as units.js and rider-pose.js keep one: `half` is a number,
// or (a box's or a capsule's) the very Vector3 halfSize is, which the
// format keeps as one object met twice.
const shapeValue = (s) => ({ type: s.type, part: s.part, center: s.center, radius: s.radius, half: s.half, yaw: s.yaw, halfSize: s.halfSize });

// The store's value of a unit, its shared vectors written as copies.
function dealias(v, seen = new Set()) {
  if (!v || typeof v !== 'object') return v;
  if (v.isVector3 || v.isQuaternion || v.isMatrix4) {
    if (seen.has(v)) return v.clone();
    seen.add(v);
    return v;
  }
  if (Array.isArray(v)) return v.map((x) => dealias(x, seen));
  if (Object.getPrototypeOf(v) === Object.prototype) for (const k of Object.keys(v)) v[k] = dealias(v[k], seen);
  return v;
}
const unitValue = (cap, u) => dealias(cap.export('unit', u));

// The hidden gun rig of a droideka (droideka.js arm): each barrel's world
// matrix as sampleMuzzles left it (gunAt's answers), and, while its
// animator's mode is the ball (an unfold's blend starts from them), each
// bone's pose.
function rigValue(s) {
  if (!s?.physicalBarrels) return null;
  const a = s.muzzleAnimator;
  return {
    muzzles: s.physicalBarrels.map((barrels) => barrels.map((b) => b.matrixWorld)),
    bones: a?.mode === 12 ? s.muzzleActor.bones.map((b) => ({ q: b.quaternion, p: b.position })) : null,
  };
}

// A unit as a raycast, a query or a hit shape reads it.
function hitValue(u) {
  const s = u.soldier;
  return {
    id: u.id, alive: u.alive, dead: u.dead, removed: u.removed, walker: u.walker, rootScale: u.rootScale,
    seat: Boolean(u.seat), seatExposed: u.seat ? Boolean(u.seat.exposed) : false, held: Boolean(u.held?.frame),
    tickCount: u.tickCount, spheresTick: u.spheresTick, shapeCount: u.shapeCount,
    hitSet: u.hitSet === u.shapes ? 'shapes' : u.hitSet === u.riderHits ? 'riderHits' : 'other',
    bounds: { center: u.bounds.center, radius: u.bounds.radius },
    shapes: u.shapes.map(shapeValue), riderHits: u.riderHits ? u.riderHits.map(shapeValue) : null,
    position: s.position, aimYaw: s.aimYaw, posture: s.posture, dive: s.dive, air: s.air,
    rolled: u.walker ? s.mode >= 11 : false, ballRadius: u.walker ? s.ballRadius : 0,
  };
}

// A member of the registry that is not a unit (a hull's, a turret's or a
// building's target, src/seats.js, src/objectives.js): whether it lives,
// and the radius a hull's target keeps.
function targetValue(t) {
  return { id: t.id, target: t.vehicle ? 'vehicle' : t.turret ? 'turret' : t.building ? 'building' : 'other',
    alive: t.alive, dead: t.dead, removed: t.removed, boundRadius: t.vehicle ? t.vehicle.boundRadius : null,
    // The object itself, as the capture names it (an answer names it so).
    handle: t };
}

// ---- The frames.
let unitProto = null;
const isUnit = (m) => Object.getPrototypeOf(m) === unitProto;
// The open frames, by their arguments' array (the same array at a frame's
// enter and at its exit): a unit tick's Character as it began, and whether
// a frame is written in full.
const opened = new WeakMap();
// The unit whose tick is open and written in full (the boundaries' `after`).
let current = null;

// Unit.tick: one unit's tick (src/units.js:515), each unit's call.
const unitTick = {
  method: 'Unit.tick',
  target: (game) => (unitProto = Object.getPrototypeOf(game.playerUnit)),
  state: (game, args, cap, self) => {
    let open = opened.get(args);
    const enter = !open;
    if (enter) {
      const tick = game.worldClock.tick;
      open = { character: self.character, full: !self.removed && (self.walker || self.dead || !self.alive || (self.id + tick) % 8 === 0) };
      opened.set(args, open);
      current = open.full ? self : null;
    } else current = null;
    if (!open.full) return { skipped: true };
    const pool = game.factory?.pool?.get(self.className);
    return {
      unit: unitValue(cap, self),
      // The Character the tick began with, and its unit now (a removal
      // lets go of it); the class's free bodies (a removal returns his).
      character: open.character ?? null, characterUnit: open.character ? open.character.unit : null,
      pool: pool ? [...pool] : null,
      // Whether a network player's recon droid steps with him.
      remoteStep: Boolean(self.remoteStep),
      ...(self.walker ? { rig: rigValue(self.soldier) } : {}),
    };
  },
  // The calls across the unit's boundary, each with the unit as it left it
  // (what a combat update, an aimer or a removal did to it).
  boundaries: (game) => {
    const after = (g, cap) => (current ? { unit: unitValue(cap, current) } : { skipped: true });
    const combat = Object.getPrototypeOf(game.playerUnit.combat), context = game.factory?.context ?? null;
    return [
      ['soldier.tick', Soldier.prototype, 'tick', after],
      ['collision.gather', gatherer(game.collision), 'gather'],
      ['collision.cast', game.collision, 'cast'],
      ['combat.damage', combat, 'damage', after],
      ['ai.aim', game.ai, 'aim', after],
      ['remotes.step', game.networkRemotes, 'step', after],
      ['combat.update', combat, 'update', after],
      ['combat.flushMuzzles', combat, 'flushMuzzles', after],
      ['combat.clearCharge', combat, 'clearCharge', after],
      ['units.deathBlast', context, 'deathBlast', after],
      ['units.blowApart', context, 'onBlowApart', after],
    ];
  },
};

// A sphere gather's cache (collision.js gatherCache) holds the collision
// world's own arrays: the battle's gathers go through this object instead,
// which gives each cache a number for the capture and passes the cache on
// as it was (the call is the same; only what the capture writes of it is).
function gatherer(collision) {
  if (!collision?.gather) return null;
  const base = collision.gather, ids = new WeakMap();
  let next = 0, pending = null;
  const proxy = {
    // (center, radius, vehicle, the cache's number or -1, ignoreBody)
    gather(center, radius, vehicle, cacheId, ignoreBody) {
      const cache = pending;
      pending = null;
      return base.call(collision, center, radius, vehicle, cache, ignoreBody);
    },
  };
  collision.gather = function gather(center, radius, vehicle = false, cache = null, ignoreBody = null) {
    let id = -1;
    if (cache) { id = ids.get(cache); if (id === undefined) ids.set(cache, id = next++); }
    pending = cache;
    return proxy.gather(center, radius, vehicle, id, ignoreBody);
  };
  return proxy;
}

// The registry's queries: their members, on the first few calls of a
// sampled tick.
let registryTick = -1;
const registryCounts = { raycast: 0, query: 0 };
function registryFrame(method) {
  return {
    method: `units.${method}`,
    target: (game) => { unitProto ??= Object.getPrototypeOf(game.playerUnit); return game.units; },
    state: (game, args) => {
      const members = () => game.units.list.map((m) => (isUnit(m) ? { unit: hitValue(m) } : { target: targetValue(m) }));
      const open = opened.get(args);
      if (open) return open.full ? { members: members(), filtered: open.filtered } : { skipped: true };
      const tick = game.worldClock.tick;
      if (tick !== registryTick) { registryTick = tick; registryCounts.raycast = registryCounts.query = 0; }
      const full = registryCounts[method]++ < REGISTRY_FRAMES;
      const frame = { full, filtered: [] };
      opened.set(args, frame);
      // The caller's filter (raycast's sixth argument, query's fourth): its
      // answers, in order, with whom it was asked about.
      const at = method === 'raycast' ? 5 : 3, filter = args[at];
      if (full && typeof filter === 'function') {
        args[at] = function filtered(unit) { const answer = filter(unit); frame.filtered.push(unit, answer); return answer; };
      }
      return full ? { members: members() } : { skipped: true };
    },
    // A hull's, a turret's and a building's sphere, as their targets' own
    // getters ask for them (query).
    boundaries: (game) => {
      const proto = (pick) => { for (const m of game.units.list) { const o = pick(m); if (o) return Object.getPrototypeOf(o); } return null; };
      return [
        ['vehicle.sphere', proto((m) => m.vehicle), 'sphere'],
        ['turret.sphere', proto((m) => m.turret), 'sphere'],
        ['building.sphere', proto((m) => m.building), 'sphere'],
      ];
    },
  };
}

// separateSoldiers (src/game.js:2201): every pair of bodies on foot.
const separate = {
  phase: 'separate',
  state: (game) => {
    return {
      members: game.units.list.map((m) => (isUnit(m) ? {
        id: m.id, unit: true, seat: Boolean(m.seat), held: Boolean(m.held), soldier: Boolean(m.soldier), removed: m.removed, alive: m.alive, dead: m.dead,
        walker: m.walker, position: m.position, tickCount: m.tickCount, posture: m.soldier?.posture, dive: m.soldier?.dive, air: m.soldier?.air,
      } : { id: m.id, unit: false })),
      player: game.playerUnit.id, global: Boolean(game.separationScope?.global), camera: game.simulationCamera?.position ?? null,
      driver: Boolean(game.driver?.active), gunner: Boolean(game.gunner?.active), playerDead: Boolean(game.combat?.dead),
    };
  },
};

// Unit.spawn: a unit into the world, a life later (src/units.js:375).
const unitSpawn = {
  method: 'Unit.spawn',
  target: (game) => (unitProto ??= Object.getPrototypeOf(game.playerUnit)),
  state: (game, args, cap, self) => {
    const enter = !opened.has(args);
    if (enter) opened.set(args, { spawning: true });
    current = enter ? self : null;
    return { unit: unitValue(cap, self) };
  },
  boundaries: (game) => {
    const after = (g, cap) => (current ? { unit: unitValue(cap, current) } : { skipped: true });
    return [
      ['collision.cast', game.collision, 'cast'],
      ['combat.reset', Object.getPrototypeOf(game.playerUnit.combat), 'reset', after],
    ];
  },
};

// What combat and the network ask of a body between its ticks: a
// droideka's shield and guns (src/droideka.js), a player's mouse look
// (src/soldier.js lookRaw, src/droideka.js look). The body's unit is its
// receiver's (`unit.soldier`).
const unitOf = (game, body) => game.units.list.find((u) => u.soldier === body) ?? null;
function bodyFrame(method, proto, extra = () => ({})) {
  return {
    method,
    target: () => proto,
    state: (game, args, cap, self) => {
      const unit = unitOf(game, self);
      return { unit: unit ? unitValue(cap, unit) : null, ...extra(self) };
    },
    boundaries: (game) => [['units.shieldHit', game.factory?.context ?? null, 'shieldHit']],
  };
}
// nextGun reads the hidden gun rig as sampleMuzzles left it this tick.
const barrels = (self) => ({ muzzles: self.physicalBarrels?.map((list) => list.map((b) => b.matrixWorld)) ?? null });

// A soldier's tick in the water (src/runtime/water-state.js soldierStep),
// on maps with water: the contact it keeps, the surface's level, the
// splashes it asked for (its seventh argument, recorded), and its grid
// lookups served as calls.
const soldierStep = {
  method: 'WaterState.soldierStep',
  target: (game) => game.water ?? null,
  state: (game, args, cap, self) => {
    let open = opened.get(args);
    if (!open) {
      open = { splashes: [] };
      opened.set(args, open);
      const splash = args[6];
      if (typeof splash === 'function') args[6] = function splashed(at) { open.splashes.push(at.clone()); return splash(at); };
      return { height: self.height, physicsWave: self.physicsWave, contact: { ...args[1] } };
    }
    return { contact: { ...args[1] }, splashes: open.splashes };
  },
  boundaries: (game) => [['water.over', game.water ?? null, 'over']],
};

export const units = {
  frames: [unitTick, unitSpawn, registryFrame('raycast'), registryFrame('query'), separate,
    bodyFrame('Droideka.absorb', Droideka.prototype), bodyFrame('Droideka.toggleShield', Droideka.prototype),
    bodyFrame('Droideka.nextGun', Droideka.prototype, barrels), bodyFrame('Droideka.look', Droideka.prototype),
    bodyFrame('Soldier.lookRaw', Soldier.prototype), soldierStep],
};
