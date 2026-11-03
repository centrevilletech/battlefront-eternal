// What a capture records (tools/sim-capture.mjs --specs NAME,...): for each
// package of the simulation's port, the frames it replays, the state each
// frame starts and ends with, and the calls across its boundary (the calls
// its Rust replay is served from the capture, their arguments checked).
//
// A spec is { frames: [...] } where each frame is either a phase of the
// tick (tools/sim-capture/phases.mjs) or a method of a live object or of a
// prototype (every instance's, as each hull's tick):
//   { phase: 'ordnance', state(game, args, cap), boundaries }
//   { method: 'ordnance.stepBolt', target(game), state(game, args, cap, self), boundaries }
// A method frame's receiver is `self` in its state() and `t` in its enter
// record (a handle, for a hull or a unit); a target(game) of null (a map
// without hulls) leaves the frame out of that battle.
// state(game, args, cap) returns the roots to record at the frame's start
// and end: live objects, written out whole (units, hulls, bodies and
// definitions they reach become handles, tools/sim-capture/plugin.mjs), or,
// for kinds the store's schemas declare (rust/sim/schema), the store's own
// export of them, cap.export(kind, object), whose references are the same
// handles. boundaries lists
// the calls to record: [label, target(game), method name, extra(game)?],
// extra naming state of the frame's own that the call changes, recorded
// with its result. A boundary call made inside another of the same frame's
// is part of the outer one and not recorded on its own; the draws made
// inside one are recorded with it. world: true writes the units' lives,
// teams and classes before the frame; whole: [class names] writes
// instances of those classes out instead of naming them.
//
// Until each package declares its kinds' schemas (rust/sim, the store's
// generator), state is the raw JavaScript objects; a package's schemas then
// replace its raw roots with their exporters. A system of the game (an
// object the game holds, such as game.items) is a handle wherever it is met
// unless the state asks for it whole: expand(object).
import { hoverClass } from '../../src/vehicle-hover.js';

export class Expand {
  constructor(value) { this.value = value; }
}
export const expand = (value) => new Expand(value);

// The ordnance pilot (flip F1): the rounds in flight and the things lying
// about. Everything they ask of the rest of the battle goes through their
// boundary object (src/ordnance-world.js), whose methods are the trait the
// Rust port reaches the battle through: each frame's calls across its
// boundary are exactly those, labelled by method. Their state is the
// store's own export of them.
const ORDNANCE_WORLD = ['cast', 'castMoving', 'raycastUnits', 'riderMount', 'fatQuery', 'unitSphere', 'isDroid', 'gone',
  'beamLocalOrigin', 'beamPose', 'shotTick', 'historyTick', 'shooter', 'missileTarget', 'navigateMissile', 'waterCrossing',
  'waver', 'spin', 'explosion', 'hit', 'hitUnit', 'projectile', 'launch', 'impact', 'grenadeContact', 'bounce'];
const ITEMS_WORLD = ['cast', 'blocked', 'waterCrossing', 'model', 'ownerKey', 'dropRoll', 'explode', 'watch', 'standing', 'give', 'pickup'];
const ordnanceWorld = (game) => ORDNANCE_WORLD.map((m) => [`ordnance.world.${m}`, game.ordnance.world, m]);
// An item's explosion reaches back into the items (a mine's blast sets off
// the mines beside it, a death drops one): the items as the call leaves
// them come with its return, for a replay that serves the call.
const itemsAfter = (game, cap) => ({ items: cap.export('items', game.items) });
const itemsWorld = (game) => ITEMS_WORLD.map((m) => [`items.world.${m}`, game.items.world, m, ...(m === 'explode' ? [itemsAfter] : [])]);

// The rounds: a tick's flight (Ordnance.update) and every round fired
// (Ordnance.fireRound, the fire command, with the lag-compensated bolt's
// flight up to the world's tick in it).
const ordnance = {
  frames: [
    { method: 'ordnance.update', target: (game) => game.ordnance, state: (game, args, cap) => cap.export('ordnance', game.ordnance), boundaries: ordnanceWorld },
    { method: 'ordnance.fireRound', target: (game) => game.ordnance, state: (game, args, cap) => cap.export('ordnance', game.ordnance), boundaries: ordnanceWorld },
  ],
};

// The items: a tick's (Items.update, its targets as its argument) and the
// commands JavaScript gives them (a dispenser's throw, a death's drop, a
// blast's damage, a hit to one mine), with the battle's item pools (its
// configuration, not the store's).
const itemsState = (game, args, cap) => ({ items: cap.export('items', game.items), pools: game.items.pools });
const items = {
  frames: [
    { method: 'items.update', target: (game) => game.items, state: itemsState, boundaries: itemsWorld },
    { method: 'items.dispense', target: (game) => game.items, state: itemsState, boundaries: itemsWorld },
    { method: 'items.dropFor', target: (game) => game.items, state: itemsState, boundaries: itemsWorld },
    { method: 'items.blast', target: (game) => game.items, state: itemsState, boundaries: itemsWorld },
    { method: 'items.hurt', target: (game) => game.items,
      state: (game, args, cap) => ({ ...itemsState(game, args, cap), item: cap.export('item', args[0]) }), boundaries: itemsWorld },
  ],
};

// The hulls (P7, flip F2 with flight and seats): each hull's tick, a 1/120 s
// substep on the ground or a 1/60 s one in flight (Vehicle.tick, on every
// hull: the method of their prototype), its state as the tick reads and
// writes it (hullState), its calls to the rest of the battle through
// src/vehicles-world.js. Until the store's generator holds a hull's nested
// lists (rust/sim/schema/vehicles.mjs), the state is written here by hand,
// field names as the generator would make them.
const vec = (v) => v.clone();
const num = (v, fallback = 0) => (Number.isFinite(parseFloat(v)) ? parseFloat(v) : fallback);
function hoverState(v) {
  const h = hoverClass(v.def);
  return { altitude: h.altitude, gravityScale: h.gravityScale, liftSpring: h.liftSpring, liftDamp: h.liftDamp, traction: h.traction,
    levelSpring: h.levelSpring, levelDamp: h.levelDamp, velocitySpring: h.velocitySpring, velocityDamp: h.velocityDamp,
    omegaXSpring: h.omegaXSpring, omegaXDamp: h.omegaXDamp, omegaZSpring: h.omegaZSpring, omegaZDamp: h.omegaZDamp,
    springs: h.springs.map((k) => ({ at: vec(k.at), radius: k.radius, length: k.length, factors: [...k.factors] })) };
}
function legsState(legs) {
  return legs ? { rayLength: legs.rayLength, orientRoll: Boolean(legs.orientRoll), width: legs.width,
    pairs: legs.pairs.map((p) => ({ left: [...p.left.at], right: [...p.right.at] })) } : null;
}
export function hullState(v) {
  const settings = v.def.settings || {};
  return {
    // What it is (fixed for the battle).
    name: v.name, flying: v.flying, walking: v.walking, creature: v.creature, wheeled: v.wheeled, repulsor: v.repulsor,
    speed: v.speed, reverseSpeed: v.reverseSpeed, strafeSpeed: v.strafeSpeed, acceleration: v.acceleration, deceleration: v.deceleration,
    turnRate: v.turnRate, hover: v.hover, boundsMin: vec(v.bounds.min), boundsMax: vec(v.bounds.max), center: vec(v.center), size: vec(v.size),
    boundRadius: v.boundRadius, probeRadius: v.probeRadius, probes: v.probes.map(vec), supportProbes: v.supportProbes.map(vec),
    maxHealth: v.maxHealth, waterRule: v.waterRule ? { interval: v.waterRule.interval, amount: v.waterRule.amount } : null, waterDepth: v.waterDepth,
    hoverClass: v.walking || v.flying ? null : hoverState(v), legs: v.creature ? null : legsState(v.def.fx?.legs), body: v.body,
    dyingSpan: v.def.fx?.dying?.span || 0, explosionOffset: v.def.fx?.explosionOffset ? [...v.def.fx.explosionOffset] : null, hasExplosion: Boolean(v.def.explosion),
    // What it does.
    position: vec(v.position), previous: vec(v.previous), velocity: vec(v.velocity),
    yaw: v.yaw, previousYaw: v.previousYaw, tilt: v.tilt, roll: v.roll, previousTilt: v.previousTilt, previousRoll: v.previousRoll,
    yawRate: v.yawRate, tiltRate: v.tiltRate, rollRate: v.rollRate, support: v.support, forwardInput: v.forwardInput, rightInput: v.rightInput,
    alive: v.alive, sleeping: v.sleeping, health: v.health, dying: v.dying, waterClock: v.waterClock,
    quaternion: v.quaternion.clone(), matrix: [...v.matrix.elements], itemCenter: vec(v.itemTarget.center), supportNormal: vec(v.supportNormal),
    legSide: v.legSide ? [...v.legSide] : null, seatIndex: v.seatIndex, driver: Boolean(v.driver),
    wheels: v.wheels.map((w) => ({ x: w.x, radius: w.radius, phase: w.phase, previous: w.previous })),
    seats: v.seats.map((seat) => {
      const view = seat.view, lease = seat.lease;
      return {
        // Its aim's rates for a bot (aimCrew) and limits (clampAim).
        turnRate: num(seat.settings?.TurnRate, num(settings.TurnRate, v.turnRate)), pitchRate: num(seat.settings?.PitchRate, num(settings.PitchRate, 1)),
        pitchLimits: [...view.pitchLimits], yawLimits: view.yawLimits ? [...view.yawLimits] : null, wrapYaw: view.wrapYaw,
        aimYaw: seat.aimYaw, pitch: seat.pitch, lookYaw: seat.lookYaw, lookPitch: seat.lookPitch,
        lease: lease ? { human: lease.human, networkHuman: Boolean(lease.unit?.networkHuman), turn: lease.input.turn, pitch: lease.input.pitch } : null,
      };
    }),
  };
}
const hullBoundaries = (game) => {
  const w = game.vehicles?.world ?? null;
  return [
    ['world.pushWalls', w, 'pushWalls'],
    ['world.springPolygons', w, 'springPolygons'],
    ['collision.cast', game.collision, 'cast'],
    ['collision.ground', game.collision, 'ground'],
    ['collision.moveBody', game.collision, 'moveBody'],
    ['world.enableBody', w, 'enableBody'],
    ['world.bodyEnabled', w, 'bodyEnabled'],
    ['water.over', game.vehicles?.water ?? null, 'over'],
    ['world.inDeathRegion', w, 'inDeathRegion'],
    ['world.damaged', w, 'damaged'],
    ['vehicles.destroyedCrew', game.vehicles ?? null, 'destroyedCrew'],
    ['world.destroyed', w, 'destroyed'],
    ['world.explosion', w, 'explosion'],
    ['world.effect', w, 'effect'],
    ['world.sound', w, 'sound'],
  ];
};
const vehicles = {
  frames: [
    { method: 'vehicle.tick', target: (game) => (game.vehicles?.vehicles?.length ? Object.getPrototypeOf(game.vehicles.vehicles[0]) : null),
      whole: ['SeatInput'], state: (game, args, cap, self) => ({ hull: hullState(self), water: game.vehicles.water ? { level: game.vehicles.water.level } : null }),
      boundaries: hullBoundaries },
  ],
};

// Nothing but the tick records (every phase's draws and the fingerprints):
// the cheapest capture, for the draw order of every phase.
const ticks = { frames: [] };

export const SPECS = { ordnance, items, vehicles, ticks };
