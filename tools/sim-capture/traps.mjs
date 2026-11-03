// Real log-trap methods and their foreign hull/presentation calls. Trap
// state and cooked settings are plain values; no entity schema is needed.
import { observeSounds } from '../../src/audio/events.js';

const xyz = v => v ? [v.x, v.y, v.z] : null;
const open = [], entered = new WeakSet(), prepared = new WeakSet(), kills = [];
const sound = { emit() {} };
const own = t => ({
  built: t.state === 'built', health: t.health, phase: t.phase, started: t.started,
  time: t.time, sprung: t.sprung, tick: t.tick, crushed: t.crushed,
});
const shown = t => ({
  state: t.state, rig: t.rig?.root.visible ?? null, pad: t.pad?.root.visible ?? null,
  bodies: Object.entries(t.bodies || {}).map(([name, body]) => [name, body?.enabled ?? null]),
  hologram: t.hologram ? [t.hologram.on, t.hologram.fade] : null,
});
const hull = v => ({
  self: v, alive: Boolean(v.alive), health: v.health, dying: v.dying,
  lastAttacker: v.lastAttacker, visible: v.root?.visible ?? null,
  team: v.team, position: xyz(v.position), radius: v.radius,
  sphere: typeof v.blastSphere === 'function',
});
const hulls = game => (game.vehicles?.vehicles ?? []).map(hull);
const settings = t => ({
  maxHealth: t.maxHealth, reach: t.reach, minRadius: t.minRadius, deathTime: t.deathTime,
  buildFrames: t.buildClip?.frames ?? null, triggerFrames: t.triggerClip?.frames ?? null,
  triggerTeam: t.settings.triggerTeam ?? null, sound: t.settings.sound ?? null,
  deathEffect: t.settings.deathEffect ?? null, point: xyz(t.point), effectAt: xyz(t.effectAt),
});

function prepare(game) {
  if (prepared.has(game)) return;
  prepared.add(game);
  observeSounds((...args) => sound.emit(...args));
  for (const vehicle of game.vehicles?.vehicles ?? []) {
    const base = vehicle.setHealth;
    vehicle.setHealth = function (...args) {
      const call = { before: hulls(game), trapBefore: game.live.traps.map(own), trapCalls: [] };
      kills.push(call);
      try { return base.apply(this, args); }
      finally {
        kills.pop();
        call.after = hulls(game);
        call.trapAfter = game.live.traps.map(own);
        lastKill.set(vehicle, call);
      }
    };
  }
}
const lastKill = new WeakMap();

function state(method, game, args, self) {
  if (!entered.has(args)) {
    entered.add(args);
    open.push(self);
    if (method === 'setHealth') {
      // A hull's explosion can synchronously hurt the trap. Keep the
      // actual nested operations, not an opaque frame-end replacement.
      for (const call of kills) call.trapCalls.push([game.live.traps.indexOf(self), args[0]]);
    }
  } else {
    if (open.pop() !== self) throw Error('trap capture frame order');
  }
  const vehicles = method === 'step' ? args[2] ?? [] : method === 'crush' ? args[0] : [];
  return {
    index: game.live.traps.indexOf(self), trap: own(self), settings: settings(self), shown: shown(self),
    vehicles: vehicles.map(v => game.vehicles.vehicles.indexOf(v)), hulls: hulls(game),
  };
}

function boundaries(game) {
  if (!game.live?.traps?.length) return [];
  prepare(game);
  const out = [['trap.sound', sound, 'emit']];
  for (const [i, t] of (game.live?.traps ?? []).entries()) {
    out.push([`trap.show:${i}`, t, 'show', () => ({ shown: shown(t) })]);
    out.push([`trap.side:${i}`, t, 'sideOf']);
    out.push([`trap.effect:${i}`, t, 'onEffect']);
  }
  for (const [i, v] of (game.vehicles?.vehicles ?? []).entries()) {
    if (typeof v.blastSphere === 'function') out.push([`trap.sphere:${i}`, v, 'blastSphere']);
    out.push([`trap.health:${i}`, v, 'setHealth', () => lastKill.get(v)]);
  }
  return out;
}

const previous = new WeakMap();
export const traps = {
  frames: ['step', 'setHealth', 'crush'].map(method => ({
    method: `Trap.${method}`,
    target: game => game.live?.traps?.length ? Object.getPrototypeOf(game.live.traps[0]) : null,
    state: (game, args, cap, self) => state(method, game, args, self), boundaries,
  })),
  events(game, note) {
    for (const [i, t] of (game.live?.traps ?? []).entries()) {
      const now = [t.state, t.phase, t.crushed], before = previous.get(t);
      if (before && now.some((value, k) => value !== before[k])) note('trap', [i, ...now]);
      previous.set(t, now);
    }
  },
};
