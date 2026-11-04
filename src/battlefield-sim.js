// Battlefield countdowns, boundary tests and the page's warning spring
// run on the shared module. Death paths remain the live world's services.
import { bridgeFor } from './sim/runtime.js';
import { KIND_CHARACTER, putBfConfig, putBfInput, BfResultMirror, syncBfResult } from './sim/gen/index.js';

const xyz = p => [p.x, p.y, p.z];
const flown = unit => { const d = unit?.combat?.remotes?.piloted; return d && !d.dead ? d : null; };

export function attachBattlefieldSim(instance, source, warning = null) {
  const bridge = bridgeFor(instance), x = bridge.x;
  const input = { words: x.battlefield_words(0), put: putBfInput };
  const output = { Mirror: BfResultMirror, sync: syncBfResult };
  const config = { words: x.battlefield_words(2), put: putBfConfig };
  let boundary, index;
  const frames = [];
  const feed = characters => bridge.writeSome(KIND_CHARACTER, characters, true);
  const sync = countdowns => { for (const c of countdowns) c.character.outOfBounds = c.value; };
  bridge.register({
    battlefieldQuery({ op, unit }) {
      const out = { body: null, vehicle: null, flag: false };
      if (op === 'vehicle') out.vehicle = unit.seat?.vehicle ?? null;
      else if (op === 'combatDead') out.flag = Boolean(unit.combat.dead);
      else {
        const hull = unit.seat?.vehicle ?? unit.seat?.turret ?? null, droid = flown(unit);
        out.body = { removed: Boolean(unit.removed), alive: Boolean(unit.alive), combatDead: Boolean(unit.combat?.dead),
          place: xyz(hull ? hull.position : unit.position), vehicle: unit.seat?.vehicle ?? null, droid: droid ? xyz(droid.position) : null };
      }
      return out;
    },
    battlefieldAction({ op, unit, vehicle, countdowns }) {
      const characters = frames.at(-1);
      sync(countdowns);
      try {
        if (op === 'kill') unit.combat.damage(Infinity);
        else if (op === 'killDroid') unit.combat.remotes.kill(unit.combat.remotes.piloted);
        else { vehicle.lastAttacker = null; vehicle.damage(Infinity); }
      } finally { feed(characters); }
      return { characters };
    },
  });
  function run(op, args = {}) {
    if (boundary !== source.boundary) {
      boundary = source.boundary;
      bridge.callArgs(config, { polygons: boundary.polygons.map(edges => ({ edges: edges.map(e => [e.x, e.z, e.nx, e.nz]) })) });
      index = x.battlefield_config();
    }
    bridge.callArgs(input, { op, index, dt: 0, characters: [], present: false, position: [0, 0, 0], droid: null, warning: null, tick: 0, outside: false, ...args });
    return bridge.callResult(output, x.battlefield_run());
  }
  const methods = {
    outside(position) { return run('outside', { position: xyz(position) }).flag; },
    controlsOutside(unit, hull = unit?.seat?.vehicle ?? unit?.seat?.turret ?? null, droid = flown(unit)) {
      return run('controlsOutside', { present: Boolean(unit), position: unit ? xyz(hull ? hull.position : unit.position) : [0, 0, 0], droid: droid ? xyz(droid.position) : null }).flag;
    },
    update(dt, characters) {
      feed(characters);
      frames.push(characters);
      try { sync(run('update', { dt, characters }).countdowns); }
      finally { frames.pop(); }
    },
  };
  Object.defineProperties(source, Object.fromEntries(Object.entries(methods).map(([key, value]) => [key, { value, configurable: true }])));
  if (warning) Object.defineProperties(warning, {
    update: { configurable: true, value(tick, outside) {
      const out = run('warning', { warning, tick, outside: Boolean(outside) });
      Object.assign(warning, out.warning);
      return out.flag;
    } },
    alpha: { configurable: true, get() { return run('alpha', { warning }).alpha; } },
  });
  return { bridge, source };
}
