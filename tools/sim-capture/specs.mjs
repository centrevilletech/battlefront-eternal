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
import { units } from './units.mjs';
import { combat } from './combat.mjs';
import { flightSpec } from './flight.mjs';

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
// hull: the method of their prototype), the hull as the store holds it
// (rust/sim/schema/vehicles.mjs), its calls to the rest of the battle
// through src/vehicles-world.js; and each hull's pose at the tick (aimTick,
// render), its rig written by hand until the store holds rigs.
const vec = (v) => v.clone();
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
// A hull's rig nodes in the order rust/sim/src/rig/hull.rs makes them: the
// root, the bones, the static bone, the markers (as tools/sim-rig/record.mjs
// lists them).
const rigNodeLists = new WeakMap();
function rigNodes(v) {
  let nodes = rigNodeLists.get(v);
  if (!nodes) {
    const bones = new Set(v.bones);
    const statics = v.root.children.filter((c) => c.isBone && !bones.has(c));
    const markers = [];
    v.root.traverse((o) => { if (o !== v.root && !bones.has(o) && !statics.includes(o) && !o.isSkinnedMesh) markers.push(o); });
    markers.sort((a, b) => a.id - b.id);
    rigNodeLists.set(v, nodes = [v.root, ...v.bones, ...statics, ...markers]);
  }
  return nodes;
}
// A hull as its pose (aimTick, render) reads and writes it: what placeRoot,
// the walk cycle, aimSeats, poseSeat and recoilBarrels read, and its rig's
// nodes (each one's position, quaternion and scale, and after the frame its
// world matrix) where the frame poses it: a ground hull that does not
// rest (a flyer's pose is the flight package's).
export function poseState(v, args, end) {
  const dt = args[1];
  const rests = !v.alive || dt > 0 && v.sleeping && !v.repulsor && !v.occupied && !v.pose && v.health >= v.maxHealth * .35;
  const nodes = rigNodes(v), at = (node) => (node ? nodes.indexOf(node) : null);
  const posed = !rests && !v.flying;
  return {
    name: v.name, alive: v.alive, sleeping: v.sleeping, repulsor: v.repulsor, flying: v.flying, creature: v.creature, health: v.health,
    maxHealth: v.maxHealth, pose: Boolean(v.pose), legs: Boolean(v.def.fx?.legs), body: v.body,
    position: vec(v.position), previous: vec(v.previous), visual: vec(v.visual), velocity: vec(v.velocity),
    yaw: v.yaw, previousYaw: v.previousYaw, tilt: v.tilt, previousTilt: v.previousTilt, roll: v.roll, previousRoll: v.previousRoll,
    age: v.age, hoverPhase: v.hoverPhase, walkPhase: v.walkPhase, walkBlend: v.walkBlend, legSide: v.legSide ? [...v.legSide] : null,
    quaternion: v.quaternion.clone(), seatIndex: v.seatIndex, driver: Boolean(v.driver),
    wheels: v.wheels.map((w) => ({ bone: at(w.bone), axis: vec(w.axis), phase: w.phase, previous: w.previous })),
    seats: v.seats.map((seat) => ({
      aimYaw: seat.aimYaw, pitch: seat.pitch, lastYaw: seat.lastYaw, yawRate: seat.yawRate, target: seat.target ? vec(seat.target) : null,
      lease: seat.lease ? { human: seat.lease.human, networkHuman: Boolean(seat.lease.unit?.networkHuman), aiming: seat.lease.input.aiming, aim: vec(seat.lease.input.aim) } : null,
      eye: vec(seat.view.eye), pivot: vec(seat.view.pivot), up: vec(seat.view.up), yawFactor: seat.view.yawFactor, pitchFactor: seat.view.pitchFactor,
      turret: at(seat.view.turret),
      weapons: seat.weapons.map((w) => ({
        barrel: w.barrel,
        aimers: w.aimers.map((a) => ({ bone: at(a.bone), yaw: a.yaw, pitch: a.pitch, inLimits: a.inLimits, onTarget: a.onTarget, facing: vec(a.facing),
          posed: a.posed, owner: a.owner, yawRate: a.yawRate, pitchRate: a.pitchRate, yawLimits: [...a.yawLimits], pitchLimits: [...a.pitchLimits], wrapYaw: a.wrapYaw })),
        muzzles: w.muzzles.map((m) => ({ barrel: at(m.barrel), rest: m.rest ? vec(m.rest) : null, recoil: m.recoil ?? null, kickAt: m.kickAt ?? null })),
      })),
    })),
    rig: posed ? nodes.flatMap((o) => [o.position.x, o.position.y, o.position.z, o.quaternion.x, o.quaternion.y, o.quaternion.z, o.quaternion.w, o.scale.x, o.scale.y, o.scale.z]) : null,
    world: posed && end ? nodes.flatMap((o) => o.matrixWorld.elements) : null,
  };
}
const poseBoundaries = (game) => {
  const w = game.vehicles?.world ?? null;
  return [
    ['collision.cast', game.collision, 'cast'],
    ['units.raycast', game.units, 'raycast'],
    ['world.clock', w, 'clock'],
    ['world.enableBody', w, 'enableBody'],
    ['world.bodyEnabled', w, 'bodyEnabled'],
  ];
};
const hullPrototype = (game) => (game.vehicles?.vehicles?.length ? Object.getPrototypeOf(game.vehicles.vehicles[0]) : null);
// Enter and exit states of a pose frame (exit: with the rig's world matrices).
const poseFrame = (method) => {
  let entering = true;
  return { method, target: hullPrototype, whole: ['SeatInput'], boundaries: poseBoundaries,
    state: (game, args, cap, self) => { const end = !entering; entering = !entering; return { hull: poseState(self, args, end) }; } };
};
const vehicles = {
  frames: [
    poseFrame('vehicle.aimTick'),
    poseFrame('vehicle.render'),
    { method: 'vehicle.tick', target: hullPrototype, whole: ['SeatInput'],
      state: (game, args, cap, self) => ({ hull: cap.export('vehicle', self), water: game.vehicles.water ? { level: game.vehicles.water.level } : null }),
      boundaries: hullBoundaries },
  ],
};

// AI navigation's planner (P13): each request and service, with the
// queue the store declares (rust/sim/schema/nav.mjs) and the scalars a
// replay checks while the search's own state is not declared yet. The
// barriers the search tests are the map's content, which a replay loads
// with the map; what a service publishes is on the owners (also not
// declared yet), so the frame records the pools and the served count a
// publish moves. The full request-level gate, native against these
// calls recorded from battles, is tools/nav-twin.mjs.
const navPlannerState = (game, args, cap) => {
  const planner = game.ai.nav.planner;
  return {
    planner: cap.export('pathPlanner', planner),
    pools: { paths: planner.pools.paths.used, nodes: planner.pools.nodes.used },
    searching: !planner.search.finished,
    served: planner.served,
  };
};
const navPlanner = {
  frames: [
    { method: 'navPlanner.request', target: (game) => game.ai.nav.planner,
      state: navPlannerState, boundaries: () => [] },
    { method: 'navPlanner.service', target: (game) => game.ai.nav.planner,
      state: navPlannerState, boundaries: () => [] },
  ],
};

// Nothing but the tick records (every phase's draws and the fingerprints):
// the cheapest capture, for the draw order of every phase.
const ticks = { frames: [] };

// The flight package (flip F2 with the hulls): a craft's tick and its pose
// at the tick, with what its targeting reads of the registry
// (tools/sim-capture/flight.mjs).
const { flight, flightMissiles } = flightSpec({ hullPrototype, rigNodes, poseState, poseBoundaries });

// The units package (flip F3): a unit's tick, the registry's queries and
// the soldiers pushed apart (tools/sim-capture/units.mjs).
// The combat package (flip F3 with units): a soldier's combat, what hurts
// him and his new lives (tools/sim-capture/combat.mjs).
export const SPECS = { ordnance, items, vehicles, ticks, units, navPlanner, combat, flight, flightMissiles };
