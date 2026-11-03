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
export class Expand {
  constructor(value) { this.value = value; }
}
export const expand = (value) => new Expand(value);

const ordnanceBoundaries = (game) => [
  ['collision.castMoving', game.collision, 'castMoving'],
  ['collision.cast', game.collision, 'cast'],
  ['units.raycast', game.units, 'raycast'],
  ['units.query', game.units, 'query'],
  ['hitHistory.raycast', game.ordnance.hitHistory, 'raycast'],
  ['water.crossing', game.ordnance.water, 'crossing'],
  ['water.ripple', game.ordnance.water, 'ripple'],
  ['effects.play', game.ordnance.effects, 'play'],
  ['ordnance.onImpact', game.ordnance, 'onImpact'],
  ['ordnance.onHit', game.ordnance, 'onHit'],
  ['ordnance.onHitUnit', game.ordnance, 'onHitUnit'],
  ['ordnance.onExplode', game.ordnance, 'onExplode'],
  ['ordnance.onGrenadeContact', game.ordnance, 'onGrenadeContact'],
  ['ordnance.onProjectile', game.ordnance, 'onProjectile'],
  ['simEvents.emit', game.simEvents, 'emit'],
];

// The ordnance pilot (flip F1): the whole phase, and each bolt's step with
// the parts of the round's code a first replay serves from the capture
// (a rider behind a hull, an explosion's fan-out, the impact effect's pick).
// Their rounds are the store's (its ordnance and bolt kinds).
const ordnance = {
  frames: [
    { phase: 'ordnance', world: true, state: (game, args, cap) => cap.export('ordnance', game.ordnance),
      boundaries: ordnanceBoundaries },
    { method: 'ordnance.stepBolt', target: (game) => game.ordnance,
      state: (game, args, cap) => ({ bolt: cap.export('bolt', args[0]), canHit: game.ordnance.canHit, striking: game.ordnance.striking, hitHistory: game.ordnance.hitHistory ?? null, water: game.ordnance.water ?? null, units: game.ordnance.units ?? null }),
      boundaries: (game) => [...ordnanceBoundaries(game),
        ['ordnance.riderBehind', game.ordnance, 'riderBehind', (g) => ({ striking: g.ordnance.striking })],
        ['ordnance.explosion', game.ordnance, 'explosion'],
        ['ordnance.unitImpactEffect', game.ordnance, 'unitImpactEffect']] },
  ],
};

// Items (flip F1 with ordnance): the phase and its call.
const items = {
  frames: [
    { method: 'items.update', target: (game) => game.items, state: (game) => ({ items: expand(game.items) }),
      boundaries: (game) => [
        ['collision.cast', game.collision, 'cast'],
        ['collision.castMoving', game.collision, 'castMoving'],
        ['units.query', game.units, 'query'],
        ['ordnance.explosion', game.ordnance, 'explosion'],
        ['simEvents.emit', game.simEvents, 'emit'],
      ] },
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

export const SPECS = { ordnance, items, ticks, navPlanner };
