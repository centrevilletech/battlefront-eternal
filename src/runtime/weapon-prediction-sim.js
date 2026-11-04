// Private page replay owners on the battle's shared simulation bridge.
// Commands return copied state and shots before any presentation callback.
import { bridgeFor } from '../sim/runtime.js';
import { putWeapon, syncWeapon, WeaponMirror } from '../sim/gen/combat.js';
import { seededRandom } from '../weapons.js';

const WEAPON_WORDS = 42;
const mutable = ['deployed', 'firing', 'timer', 'bottomless', 'perShot', 'clip', 'reserve', 'heat', 'overheated',
  'floor', 'kick', 'spreadScale', 'sniperScale', 'yawSpreadOverride', 'wait', 'reloadLeft', 'throwTime',
  'released', 'hidden', 'charging', 'held', 'charge', 'limit', 'pattern', 'salvosLeft', 'salvoWait',
  'lockTargetId', 'lockGeneration', 'lockElapsed', 'lockComplete'];
const postures = ['stand', 'crouch', 'prone'];
let nextSession = 1;

function text(words, value) {
  words.push(value.length);
  for (let i = 0; i < value.length; i++) words.push(value.charCodeAt(i));
}
function definition(words, value, depth = 0) {
  if (value === undefined) words.push(0);
  else if (value === null) words.push(1);
  else if (value === false) words.push(2);
  else if (value === true) words.push(3);
  else if (typeof value === 'number') words.push(4, value);
  else if (typeof value === 'string') { words.push(5); text(words, value); }
  else if (Array.isArray(value) && depth < 12) {
    words.push(6, value.length);
    for (const item of value) definition(words, item, depth + 1);
  } else if (typeof value === 'object' && depth < 12) {
    const keys = Object.keys(value); words.push(7, keys.length);
    for (const key of keys) { text(words, key); definition(words, value[key], depth + 1); }
  } else words.push(0);
}

export function createWeaponPredictionSim(instance, options) {
  return createWeaponPredictionBridge(bridgeFor(instance), options);
}

export function createWeaponPredictionBridge(bridge, { trace = null } = {}) {
  const x = bridge.x, sessions = new Set(), seeded = new WeakMap();
  let disposed = false;
  const alive = () => { if (disposed) throw Error('weapon prediction bridge is disposed'); };

  // A mirror can later resume JavaScript replay. Keep its callable seeded
  // generator at the native cursor, without drawing its numbers each tick.
  function copySeed(weapon, mirror) {
    if (mirror.salvoSeed === null) { weapon.salvoRandom = null; seeded.delete(weapon); }
    else {
      let cursor = seeded.get(weapon);
      if (!cursor || cursor.seed !== mirror.salvoSeed) {
        cursor = { seed: mirror.salvoSeed, count: mirror.salvoCount, generator: null };
        cursor.draw = () => {
          if (!cursor.generator) {
            cursor.generator = seededRandom(cursor.seed);
            for (let i = 0; i < cursor.count; i++) cursor.generator();
          }
          return cursor.generator();
        };
        seeded.set(weapon, cursor);
      } else if (cursor.count !== mirror.salvoCount) { cursor.count = mirror.salvoCount; cursor.generator = null; }
      weapon.salvoRandom = cursor.draw;
    }
    weapon.salvoSeed = mirror.salvoSeed; weapon.salvoCount = mirror.salvoCount;
  }

  function create(weapons, owners = [], arsenal = null, barrels = []) {
    alive();
    const serial = nextSession++, mirrors = weapons.map(() => new WeaponMirror());
    let handle = x.prediction_weapons_create(), cached = null;
    const live = () => { alive(); if (!handle) throw Error('weapon prediction session is disposed'); return handle; };
    function run(op, values, read = null) {
      live();
      const input = Float64Array.from(values), ptr = x.prediction_weapons_room(handle, input.length);
      bridge.views(); bridge.f.set(input, ptr / 8);
      const ok = x.prediction_weapons_run(handle, op);
      bridge.views();
      const output = Array.from(bridge.f.subarray(x.prediction_weapons_output(handle) / 8,
        x.prediction_weapons_output(handle) / 8 + x.prediction_weapons_output_len(handle)));
      const error = ok ? null : new TextDecoder().decode(new Uint8Array(x.memory.buffer,
        x.prediction_weapons_error(handle), x.prediction_weapons_error_len(handle)));
      const value = ok && read ? read(output) : output;
      trace?.({ session: serial, op, input: Array.from(input), output, error });
      if (!ok) throw Error(error);
      return value;
    }
    function state(combat = null) {
      const words = [weapons.length];
      for (const weapon of weapons) {
        const at = words.length; words.length += WEAPON_WORDS;
        putWeapon(bridge, weapon, words, at, null);
      }
      words.push(arsenal?.selected[0] ?? 0, arsenal?.selected[1] ?? 0, combat?.zoomStep ?? 0, owners.length);
      for (const barrel of barrels) words.push(barrel);
      return Float64Array.from(words);
    }
    function read(output, combat) {
      const count = output[0]; let at = 1;
      const barrelAt = 1 + count * WEAPON_WORDS + 3, snapshot = output.slice(0, barrelAt + 1);
      for (let i = 0; i < output[barrelAt]; i++) snapshot.push(output[barrelAt + 1 + i * 2]);
      // Remember the native values before setters can reenter or mutate
      // mirrors. The next command will feed any such changes back in.
      cached = Float64Array.from(snapshot);
      for (let i = 0; i < count; i++, at += WEAPON_WORDS) {
        const weapon = weapons[i], mirror = syncWeapon(bridge, output, at, 0, mirrors[i]);
        weapon.level = mirror.level; weapon.def = weapon.levels[mirror.level];
        for (const name of mutable) weapon[name] = mirror[name];
        if (mirror.melee) weapon.melee.restoreState(mirror.melee);
        copySeed(weapon, mirror);
      }
      if (arsenal) { arsenal.selected[0] = output[at]; arsenal.selected[1] = output[at + 1]; }
      if (combat) combat.zoomStep = output[at + 2];
      at += 3;
      const barrelCount = output[at++];
      for (let i = 0; i < barrelCount; i++, at += 2) {
        barrels[i] = output[at];
        if (output[at + 1]) owners[i].barrel = output[at];
      }
      const shots = [], shotCount = output[at++];
      for (let i = 0; i < shotCount; i++, at += 8) {
        const weapon = weapons[output[at]], kind = output[at + 1];
        if (kind === 0) shots.push({ weapon, yaw: output[at + 2], pitch: output[at + 3], charge: output[at + 4], targetId: output[at + 5], targetGeneration: output[at + 6] });
        else if (kind === 1) shots.push({ weapon, repair: true });
        else if (kind === 2) {
          const swing = output[at + 7], swings = weapon.def.swings;
          shots.push({ weapon, meleeStart: true, swing, sound: swings?.length ? swings[swing - 1].sound : null });
        } else shots.push({ weapon, [kind === 3 ? 'meleeHit' : 'meleeEnd']: true });
      }
      return shots;
    }
    const session = {
      get handle() { return handle; },
      sync(combat = null) {
        live();
        const values = state(combat);
        if (!cached || values.length !== cached.length || values.some((value, i) => !Object.is(value, cached[i]))) {
          run(1, values, () => { cached = values; });
        }
      },
      infantry(unit, combat, controls, presses) {
        this.sync(combat);
        const primary = controls.take('firePrimary'), secondary = controls.take('fireSecondary');
        const heldPrimary = controls.held('firePrimary') || primary, heldSecondary = controls.held('fireSecondary') || secondary;
        const reload = controls.take('reload'), id = unit?.id ?? 0, generation = unit?.generation ?? 0;
        const next = controls.take('nextPrimary'), previous = controls.take('previousPrimary'), nextSecondary = controls.take('nextSecondary'), zoom = controls.take('zoom');
        const forward = controls.axis('forward'), right = controls.axis('right'), soldier = combat.soldier;
        const air = !soldier.air ? -1 : soldier.air === 'jet' ? 3 : 0;
        return run(2, [id, generation, primary, heldPrimary, secondary, heldSecondary, reload, next, previous, nextSecondary, zoom,
          forward, right, postures.indexOf(soldier.posture), air, Boolean(soldier.dive), soldier.lock, Boolean(soldier.rolled), Boolean(combat.gunUnderwater),
          Boolean(presses?.[0]), presses?.[0] ?? 0, Boolean(presses?.[1]), presses?.[1] ?? 0], output => read(output, combat));
      },
      mounted(entry, pressed, held, reload) {
        this.sync();
        const owner = owners.indexOf(entry.owner);
        const words = [1, weapons.indexOf(entry.weapon), owner, false, entry.towHeld, pressed, held, reload];
        const shots = run(3, words, output => read(output, null));
        // Read this owner's counter only after its weapon update, as +=
        // does in the source. A getter or setter can change another entry.
        if (entry.advancesBarrel) run(4, [owner, entry.owner.barrel, shots.length], output => {
          barrels[owner] = output[0];
          cached[1 + weapons.length * WEAPON_WORDS + 4 + owner] = output[0];
          entry.owner.barrel = output[0];
        });
        return shots;
      },
      dispose() {
        if (!handle) return;
        x.prediction_weapons_drop(handle); handle = 0; sessions.delete(session);
      },
    };
    sessions.add(session);
    try {
      const definitions = new Set();
      for (const weapon of weapons) {
        const def = weapon.levels[0];
        if (definitions.has(def)) continue;
        definitions.add(def);
        const words = [bridge.defs.weapon.id(def)]; definition(words, def); run(0, words);
      }
    } catch (error) { session.dispose(); throw error; }
    return session;
  }

  return {
    bridge,
    createArsenal(arsenal) { return create(arsenal.weapons, [], arsenal); },
    createMounted(entries) {
      const owners = [...new Set(entries.map(entry => entry.owner))];
      return create([...new Set(entries.map(entry => entry.weapon))], owners, null, owners.map(owner => entries.find(entry => entry.owner === owner).barrel));
    },
    dispose() { if (disposed) return; for (const session of sessions) session.dispose(); disposed = true; },
  };
}
