// The combat package's frames (tools/sim-capture/specs.mjs `combat`): a
// soldier's combat each tick (src/combat.js PlayerCombat.update, on its
// prototype: every unit's call), the rounds a droideka's guns owe after the
// tick samples its gun rig (flushMuzzles), what hurts him (damage), a blast
// near him (explode, once a unit for every explosion) and a new life
// (reset). rust/sim/tests/replay_combat.rs reads them back.
//
// A frame's state is the unit, through the store's exporter (its schema,
// rust/sim/schema/units.mjs, shared vectors written as copies as the store
// holds them), his combat among its fields (rust/sim/schema/combat.mjs, read
// through the getters src/combat.js and src/weapons.js keep for the store:
// weapons by value, a weapon's level as a number, its generators as whether
// it has one and a press's seed with the draws taken from it), and the
// presses a network player's trigger edges carry. Its boundary is src/combat-world.js (every
// method, labelled world.NAME), a droideka's hidden gun rig (nextGun,
// gunAt) and its shield's hook (units.shieldHit); the calls that can change
// the unit or his combat (a round that blows up beside him, a death, a
// link of the arc) carry both as they left them.
//
// An update is written in full for every droideka, every network player,
// every dead unit, every unit whose trigger is down or whose weapon can
// fire without it (a charge, a throw, a salvo, a repair tool at work), and
// an eighth of the rest by turns (by id), with every unit's life (a lock's
// target's); a blast in full for a unit within its reach; the others carry
// their calls and `skipped`.
//
// The spec reaches PlayerCombat and CombatWorld through the battle's own
// objects, never by importing src/combat.js: that would load
// src/weapons.js before the capture's module hook is in place, and a
// press's seeded draws would go unseen (tools/sim-capture/hooks.mjs).
import { Droideka } from '../../src/droideka.js';

const combatProto = (game) => Object.getPrototypeOf(game.playerUnit.combat);
const worldProto = (game) => Object.getPrototypeOf(game.playerUnit.combat.world);

// ---- Values.
// The store's value of a unit, its shared vectors written as copies
// (tools/sim-capture/units.mjs).
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
const isUnit = (u) => Boolean(u && typeof u === 'object' && u.combat && 'generation' in u && u.soldier);
// A repair tool's target goes to the capture's writer as the object itself,
// which names a building as it names one in a boundary call's arguments
// (the store's exporter numbers buildings apart).
function unitValue(cap, u) {
  if (!isUnit(u)) return null;
  const v = dealias(cap.export('unit', u));
  if (v.combat && u.combat.repairTarget) v.combat.repairTarget = u.combat.repairTarget;
  return v;
}

// The presses a network player's trigger edges carry (network-shot-aim.js).
const edge = (e) => ({ button: e.button, id: e.id, late: e.late, aimYaw: e.aimYaw, aimPitch: e.aimPitch });
function shotsValue(owner) {
  if (!owner || typeof owner !== 'object') return { shots: null, shot: null };
  return { shots: Array.isArray(owner.networkShots) ? owner.networkShots.map(edge) : null, shot: owner.networkShot ? edge(owner.networkShot) : null };
}

// What a world gives a combat that the port takes as fixed: no camera, no
// HUD, no first-person view (a world's combats are bots and network
// players, whose combats play by a player's rules, `player`, without a
// player's camera); a frame that finds otherwise stops the capture.
function checkWorldCombat(c) {
  if (c.camera || c.hud || c.view || c.firstPerson) throw Error(`sim-capture combat: unit ${c.owner?.id}'s combat has a camera, HUD or view in a world`);
}

// ---- The frames.
// The open frames, by their arguments' array: whether written in full.
const opened = new WeakMap();
// The combat whose frame is open and written in full, innermost last (the
// boundaries' `after`).
const current = [];

// Every registry member's life (its id, generation and whether it lives:
// a unit, or a hull's, a turret's or a building's target), for what a
// launcher's lock reads of what it picks (an update's).
const livesValue = (game) => game.units.all().map((m) => ({ id: m.id, generation: m.generation ?? 0, alive: Boolean(m.alive) }));

function combatState(game, cap, self, full, lives) {
  if (!full) return { skipped: true };
  checkWorldCombat(self);
  return { unit: unitValue(cap, self.owner), ...shotsValue(self.owner), ...(lives ? { lives: livesValue(game) } : {}) };
}

function frame(method, decide, lives = false) {
  return {
    method: `PlayerCombat.${method}`,
    target: (game) => combatProto(game),
    state: (game, args, cap, self) => {
      let open = opened.get(args);
      if (!open) {
        open = { full: decide(game, self, args) };
        opened.set(args, open);
        current.push(open.full ? self : null);
        return combatState(game, cap, self, open.full, lives);
      }
      current.pop();
      return combatState(game, cap, self, open.full, lives);
    },
    boundaries,
  };
}

// A weapon whose shot can come without the trigger down this tick: a
// charge let go, a grenade leaving the hand, a salvo's next round, a repair
// tool's interval.
const busy = (c) => c.arsenal.weapons.some((w) => w.charging || w.firing || w.salvosLeft > 0 || w.throwTime >= 0);
const updateFull = (game, self) => {
  const u = self.owner;
  if (!isUnit(u)) return false;
  const tick = game.worldClock.tick;
  return Boolean(u.walker || u.networkHuman || self.dead || u.intent?.buttons?.[0] > 0 || u.intent?.buttons?.[1] > 0 || busy(self) || (u.id + tick) % 8 === 0);
};
// A blast reaches him (explode's early returns aside) within its outer radii
// of his blast sphere's centre, less half its radius: a margin over both.
const explodeFull = (game, self, [explosion, position]) => {
  if (!isUnit(self.owner)) return false;
  const reach = Math.max(explosion?.DamageRadiusOuter || 0, explosion?.PushRadiusOuter || 0) + 3;
  return self.soldier.position.distanceTo(position) <= reach;
};

// The calls across combat's boundary, each that can change the unit or his
// combat with both as they left them.
const WORLD = ['cast', 'raycastUnits', 'admitRemote', 'remoteOf', 'remoteEnter', 'remoteKill', 'remoteLaunch', 'lockOn', 'lockKey', 'repair',
  'lockPick', 'fireRound', 'dispense', 'meleeStrike', 'arcTargets', 'hurtArcTarget', 'lightning', 'fired', 'damaged', 'died', 'blownApart',
  'respawn', 'presentation'];
const CHANGING = new Set(['remoteEnter', 'remoteKill', 'remoteLaunch', 'fireRound', 'meleeStrike', 'hurtArcTarget', 'fired', 'damaged', 'died',
  'blownApart', 'respawn', 'dispense', 'repair']);
function boundaries(game) {
  const after = (g, cap) => {
    const c = current[current.length - 1];
    return c ? { unit: unitValue(cap, c.owner) } : { skipped: true };
  };
  return [
    ...WORLD.map((m) => [`world.${m}`, worldProto(game), m, ...(CHANGING.has(m) ? [after] : [])]),
    ['body.nextGun', Droideka.prototype, 'nextGun', after],
    ['body.gunAt', Droideka.prototype, 'gunAt'],
    ['units.shieldHit', game.factory?.context ?? null, 'shieldHit'],
  ];
}

export const combat = {
  frames: [
    frame('update', updateFull, true),
    frame('flushMuzzles', (game, self) => isUnit(self.owner) && self.pendingMuzzles.length > 0),
    frame('damage', (game, self) => isUnit(self.owner)),
    frame('explode', explodeFull),
    frame('reset', (game, self) => isUnit(self.owner)),
  ],
};
