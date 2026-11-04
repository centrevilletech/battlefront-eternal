// Animated prop poses and holds, including the soldier root sample before
// it is placed on a prop's node. No held-frame result is served wholesale.
import { observeSounds } from '../../src/audio/events.js';
const xyz = p => p ? [p.x, p.y, p.z] : null;
const quat = q => [q.x, q.y, q.z, q.w];
const clips = new WeakMap();
function clip(c) {
  if (!c) return null;
  let def = clips.get(c);
  if (!def) {
    def = { class: `prop-clip:${c.name}`, base: 'animation', frames: c.frames, fps: c.fps,
      duration: c.duration, rootMotion: c.rootMotion, tracks: c.tracks, data: c.data };
    clips.set(c, def);
  }
  return def;
}
export function animatedSettings(p) {
  return { door: p.door, clips: p.slots.map(clip), doorClip: clip(p.doorClip),
    idleDelay: p.idleDelay, openSlots: p.names.map(n => n === 'open'), position: xyz(p.position), killSound: p.killSound,
    triggers: p.triggers.map(t => ({ slot: t.slot, point: xyz(t.point), radius: t.radius, sound: t.sound || null, opens: t.action === 'open' })),
    grabs: p.grabs.map(g => ({ slot: g.slot, window: g.window, radius: g.radius, node: g.node ? p.rig.bones.indexOf(g.node) : null, clip: g.clip })) };
}
export function animatedOwn(p, game) {
  return { index: game.live.animated.indexOf(p), slot: p.slot, time: p.time, weight: p.weight, loop: p.loop, hold: p.hold,
    started: p.started, startTime: p.startTime, victim: p.victim ? { unit: p.victim.unit, generation: p.victim.generation,
      grab: p.grabs.indexOf(p.victim.grab), since: p.victim.since } : null,
    openness: p.openness, bodyEnabled: p.body?.enabled ?? null, standQ: Array.from(p.standQ), standT: Array.from(p.standT),
    pose: p.pose ? { root: p.pose.root, q: Array.from(p.pose.q), t: Array.from(p.pose.t), w: Array.from(p.pose.w) } : null,
    root: p.rig.root.matrix.toArray(), rootWorld: p.rig.root.matrixWorld.toArray(),
    bones: p.rig.bones.map(b => ({ parent: p.rig.bones.indexOf(b.parent), p: xyz(b.position), q: quat(b.quaternion), s: xyz(b.scale), world: b.matrixWorld.toArray() })),
    bind: p.rig.bones.map(b => ({ p: xyz(p.rig.bind.get(b).p), q: quat(p.rig.bind.get(b).q) })), record: p.state() };
}
export function animatedBody(u, p, game) {
  const s = u.soldier, held = u.held;
  return { self: u, id: u.id, generation: u.generation, soldier: Boolean(s), walker: Boolean(u.walker), removed: Boolean(u.removed),
    alive: Boolean(u.alive), dead: Boolean(u.dead), seated: Boolean(u.seat), combat: Boolean(u.combat), mounted: Boolean(u.combat?.mounted),
    position: xyz(u.position), previous: xyz(s?.previous), settled: xyz(s?.settled), velocity: xyz(s?.velocity) ?? [0, 0, 0],
    air: Boolean(s?.air), dive: s?.dive ?? 0, tickCount: u.tickCount ?? null, letGo: typeof u.letGo === 'function', remove: typeof u.remove === 'function',
    held: held ? { prop: game.live.animated.indexOf(held.prop), grab: held.prop.grabs.indexOf(held.grab), since: held.since,
      frame: held.frame?.toArray() ?? null, time: held.time ?? null } : null,
    riderClips: p.grabs.map(g => Boolean(u.animator?.clips?.get(g.clip))) };
}
const opened = [], entered = new WeakSet(), frameBodies = new WeakMap(), prepared = new WeakSet(), last = new Map();
const sound = { emit() {} }, rider = { emit() {} };
globalThis.__bfePropsRider = (...args) => rider.emit(...args);
function current() { return opened.at(-1); }
function prepare(game) {
  if (prepared.has(game)) return;
  prepared.add(game);
  observeSounds((...args) => sound.emit(...args));
  // Prototype shims cover soldiers constructed after capture starts.
  const proto = Object.getPrototypeOf(game.playerUnit);
  for (const method of ['letGo', 'remove']) {
    const base = proto[method];
    proto[method] = function (...args) {
      const p = current();
      if (!p) return base.apply(this, args);
      const before = animatedBody(this, p, game), propBefore = animatedOwn(p, game);
      try { return base.apply(this, args); }
      finally { last.set(method, { before, after: animatedBody(this, p, game), propBefore, propAfter: animatedOwn(p, game) }); }
    };
  }
  // Every soldier's combat uses PlayerCombat's prototype (including ones
  // born later); map the callback back to its unit in the live registry.
  const cp = Object.getPrototypeOf(game.playerUnit.combat), base = cp.damage;
  cp.damage = function (...args) {
    const p = current(), u = p && game.units.all().find(u => u.combat === this);
    if (!u) return base.apply(this, args);
    const before = animatedBody(u, p, game), propBefore = animatedOwn(p, game);
    try { return base.apply(this, args); }
    finally { last.set('damage', { before, after: animatedBody(u, p, game), propBefore, propAfter: animatedOwn(p, game) }); }
  };
}
function boundaries(game) {
  prepare(game);
  const proto = Object.getPrototypeOf(game.playerUnit);
  return [['animated.sound', sound, 'emit'], ['animated.rider', rider, 'emit'],
    ...['letGo', 'remove'].map(method => [`animated.${method}`, proto, method, () => last.get(method)]),
    ['animated.damage', Object.getPrototypeOf(game.playerUnit.combat), 'damage', () => last.get('damage')]];
}
export const animated = {
  frames: [{ method: 'AnimatedProp.step', target: game => game.live.animated.length ? Object.getPrototypeOf(game.live.animated[0]) : null,
    boundaries,
    state(game, args, cap, p) {
      if (!entered.has(args)) {
        entered.add(args); opened.push(p);
        frameBodies.set(args, [...new Set([...(args[2] ?? []), ...(p.victim ? [p.victim.unit] : [])])]);
      }
      else if (opened.pop() !== p) throw Error('animated capture frame order');
      const order = args[2] ?? [], bodies = frameBodies.get(args);
      return { prop: animatedOwn(p, game), settings: animatedSettings(p), bodies: bodies.map(u => animatedBody(u, p, game)),
        order: order.map(u => bodies.indexOf(u)), positions: (args[1] ?? []).map(xyz),
        positionOwners: (args[1] ?? []).map(position => bodies.findIndex(u => u.position === position)) };
    },
  }],
};
