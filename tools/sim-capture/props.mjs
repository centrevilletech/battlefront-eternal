// Building and supply methods. Constructor data is copied explicitly;
// callbacks carry their entry state so serving them cannot erase a wrong
// native health write or notification order.
import { observeSounds } from '../../src/audio/events.js';

const xyz = p => [p.x, p.y, p.z];
const num = x => { const n = Number.parseFloat(Array.isArray(x) ? x[0] : x); return Number.isFinite(n) ? n : 0; };
export const buildingState = b => ({ state: b.state, health: b.health, addHealth: b.addHealth,
  lastAttacker: b.lastAttacker, targetGeneration: b.aiTarget?.generation ?? null });
export const stationState = b => ({ ...buildingState(b), mode: b.mode, active: b.active, pulse: b.pulse,
  servedPlayer: b.servedPlayer, servedHealth: b.servedHealth, servedAmmo: b.servedAmmo });
export const targetState = t => ({ position: xyz(t.position), vehicle: t.healthType === 'vehicle',
  alive: Boolean(t.alive), player: Boolean(t.player), radius: t.radius ?? 0, health: t.health,
  maxHealth: t.maxHealth, weapons: t.weapons.map(w => ({ bottomless: Boolean(w.bottomless),
    ammo: w.def.ammo ?? NaN, clip: w.clip, perShot: w.perShot, reserve: w.reserve })) });
export const buildingSettings = b => ({ kind: b.constructor.name === 'Turret' ? 'turret' : b.constructor.name === 'ObjectivePost' ? 'objective' : 'building',
  maxHealth: b.maxHealth, respawnTime: b.respawnTime, builtAddHealth: num(b.desc.settings.AddHealth),
  regenerates: b.regenerates, destroyed: Boolean(b.looks.destroyed), unbuilt: Boolean(b.looks.unbuilt),
  crew: typeof b.crew?.destroyed === 'function', explosion: Boolean(b.explosion), explosionDef: b.explosion, damaged: typeof b.onDamaged === 'function',
  matrix: b.matrix.toArray(), models: ['built', 'destroyed', 'unbuilt'].map(k => b.looks[k]?.model ? {
    sphere: b.looks[k].model.sphere ?? null, bounds: b.looks[k].model.box ?? null } : null) });
export const supplySettings = b => ({ position: xyz(b.position), radius: b.radius, delay: b.delay,
  health: b.giveHealth, ammo: b.giveAmmo, vehicleHealth: b.vehicleHealth, vehicleAmmo: b.vehicleAmmo });
export function propsState(method, game, args, b) {
  const supply = b.constructor.name === 'PowerupStation';
  const soldiers = method === 'update' ? (Array.isArray(args[1]) ? args[1] : args[1] ? [args[1]] : []) : method === 'serve' ? [args[0]] : [];
  const vehicles = method === 'update' ? args[2] ?? [] : [];
  const targets = [...new Set([...soldiers, ...vehicles])];
  return { index: game.live.buildings.indexOf(b), building: buildingState(b), settings: buildingSettings(b),
    station: supply ? stationState(b) : null, supply: supply ? supplySettings(b) : null,
    targets: targets.map(targetState), soldiers: soldiers.map(t => targets.indexOf(t)), vehicles: vehicles.map(t => targets.indexOf(t)) };
}

const sound = { emit() {} }, prepared = new WeakSet(), after = new Map();
function prepare(game) {
  if (prepared.has(game)) return;
  prepared.add(game);
  observeSounds((...args) => sound.emit(...args));
  for (const [index, b] of game.live.buildings.entries()) {
    if (b.constructor.name === 'Trap') continue;
    for (const method of ['onDestroyed', 'onExplode', 'onDamaged', 'onServe']) {
      const base = b[method];
      if (typeof base !== 'function') continue;
      b[method] = function (...args) {
        const supply = method === 'onServe';
        const before = supply ? stationState(this) : buildingState(this);
        const targetBefore = supply ? targetState(args[0]) : null;
        try { return base.apply(this, args); }
        finally { after.set(`props.${method}:${index}`, { before, after: supply ? stationState(this) : buildingState(this),
          targetBefore, targetAfter: supply ? targetState(args[0]) : null }); }
      };
    }
  }
  const crew = game.turretSeats;
  if (crew?.destroyed) {
    const base = crew.destroyed;
    crew.destroyed = function (b, ...args) {
      const before = buildingState(b);
      try { return base.call(this, b, ...args); }
      finally { after.set('props.crew', { before, after: buildingState(b) }); }
    };
  }
}
function boundaries(game) {
  prepare(game);
  const out = [['props.sound', sound, 'emit']];
  if (game.turretSeats?.destroyed) out.push(['props.crew', game.turretSeats, 'destroyed', () => after.get('props.crew')]);
  for (const [i, b] of game.live.buildings.entries()) {
    if (b.constructor.name === 'Trap') continue;
    out.push([`props.show:${i}`, b, 'show', () => ({ after: buildingState(b) })]);
    for (const method of ['onDestroyed', 'onExplode', 'onDamaged', 'onServe']) {
      if (typeof b[method] === 'function') out.push([`props.${method}:${i}`, b, method, () => after.get(`props.${method}:${i}`)]);
    }
  }
  return out;
}
function prototype(game, name) {
  for (const b of game.live?.buildings ?? []) for (let p = Object.getPrototypeOf(b); p; p = Object.getPrototypeOf(p)) {
    if (p.constructor?.name === name) return p;
  }
  return null;
}
export const props = {
  frames: [['Building', 'setHealth'], ['Building', 'damage'], ['Building', 'regenerate'],
    ['Turret', 'damage'], ['PowerupStation', 'update'], ['PowerupStation', 'serve']].map(([kind, method]) => ({
    method: `${kind}.${method}`, target: game => prototype(game, kind), boundaries,
    state: (game, args, cap, self) => propsState(method, game, args, self),
  })),
};
