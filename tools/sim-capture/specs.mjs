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

// Nothing but the tick records (every phase's draws and the fingerprints):
// the cheapest capture, for the draw order of every phase.
const ticks = { frames: [] };

export const SPECS = { ordnance, items, ticks };
