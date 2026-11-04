import { Vector3 } from 'three';
import { bridgeFor } from '../sim/runtime.js';
import { putWeapon, syncWeapon, WeaponMirror } from '../sim/gen/combat.js';
import { seededRandom } from '../weapons.js';

const weaponWords = 42;
const vectors = ['eye', 'aimDir', 'target'];
const mutable = ['deployed', 'firing', 'timer', 'bottomless', 'perShot', 'clip', 'reserve', 'heat', 'overheated',
  'floor', 'kick', 'spreadScale', 'sniperScale', 'yawSpreadOverride', 'wait', 'reloadLeft', 'throwTime',
  'released', 'hidden', 'charging', 'held', 'charge', 'limit', 'pattern', 'salvosLeft', 'salvoWait',
  'lockTargetId', 'lockGeneration', 'lockElapsed', 'lockComplete'];
const cameraVectors = ['eye', 'center', 'offset', 'pivot', 'target', 'boom', 'local', 'right', 'up', 'dir'];
const actions = ['use', 'nextPrimary', 'previousPrimary', 'nextSecondary', 'zoom', 'firePrimary', 'fireSecondary'];
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
function listId(table, value) {
  let id = table.indexOf(value);
  if (id < 0) { id = table.length; table.push(value); }
  return id;
}

// Each replay owns its command and callback continuation. Calls into the
// module always return before collision, water, controls or shot observers run.
export function createRemotePredictionSim(instance, options) {
  return createRemotePredictionBridge(bridgeFor(instance), options);
}

export function createRemotePredictionBridge(bridge, { trace = null } = {}) {
  const x = bridge.x, sessions = new Set(), seeds = new WeakMap();
  let disposed = false;
  const alive = () => { if (disposed) throw Error('remote prediction bridge is disposed'); };
  function copySeed(weapon, mirror) {
    if (mirror.salvoSeed == null) { weapon.salvoRandom = null; seeds.delete(weapon); }
    else {
      let cursor = seeds.get(weapon);
      if (!cursor || cursor.seed !== mirror.salvoSeed) {
        cursor = { seed: mirror.salvoSeed, count: mirror.salvoCount, generator: null };
        cursor.draw = () => {
          if (!cursor.generator) {
            cursor.generator = seededRandom(cursor.seed);
            for (let i = 0; i < cursor.count; i++) cursor.generator();
          }
          return cursor.generator();
        };
        seeds.set(weapon, cursor);
      } else if (cursor.count !== mirror.salvoCount) { cursor.count = mirror.salvoCount; cursor.generator = null; }
      weapon.salvoRandom = cursor.draw;
    }
    weapon.salvoSeed = mirror.salvoSeed; weapon.salvoCount = mirror.salvoCount;
  }

  function create(context, predicted) {
    alive();
    const serial = nextSession++, definitions = [], classes = new Set(), objects = [predicted], lists = [];
    const mirrors = new WeakMap(), ids = new WeakMap(), weapons = [], streams = [], salvos = [], velocities = [];
    const registered = new Map();
    let handle = x.prediction_remotes_create(), busy = false, steps = 0, view = context.simulationView;
    const cx = Object.create(bridge);
    cx.defs = { weapon: { id: () => 0, at: id => definitions[id]?.[0] } };
    const live = () => { alive(); if (!handle) throw Error('remote prediction session is disposed'); };
    function run(op, words) {
      live();
      const input = Float64Array.from(words), ptr = x.prediction_remotes_room(handle, input.length);
      bridge.views(); bridge.f.set(input, ptr / 8);
      const ok = x.prediction_remotes_run(handle, op);
      bridge.views();
      const ptrOut = x.prediction_remotes_output(handle) / 8;
      const output = Array.from(bridge.f.subarray(ptrOut, ptrOut + x.prediction_remotes_output_len(handle)));
      const error = ok ? null : new TextDecoder().decode(new Uint8Array(x.memory.buffer,
        x.prediction_remotes_error(handle), x.prediction_remotes_error_len(handle)));
      trace?.({ session: serial, op, input: Array.from(input), output: output.slice(), error });
      return { output, error };
    }
    function register(value, kind = 0) {
      let id = ids.get(value);
      if (id === undefined) { id = definitions.length; ids.set(value, id); definitions.push(value); }
      if (kind === 2) classes.add(id);
      const words = [id, kind]; definition(words, value);
      const key = id * 3 + kind, old = registered.get(key);
      if (old && old.length === words.length && old.every((value, i) => Object.is(value, words[i]))) return id;
      registered.set(key, words);
      const { error } = run(0, words);
      if (error) throw Error(error);
      return id;
    }
    function capture() {
      const order = listId(lists, context.droids), strikes = listId(lists, context.strikes);
      for (const list of lists) for (const droid of list) listId(objects, droid);
      const piloted = context.piloted ? listId(objects, context.piloted) : -1;
      const enabled = Boolean(context.simulationView);
      if (enabled) view = context.simulationView;
      const camera = context.aimCamera;
      for (const droid of objects) for (const group of [droid.arsenal.weapons, ...droid.arsenal.channels])
        for (const weapon of group) listId(weapons, weapon);
      for (const droid of objects) { listId(velocities, droid.velocity); if (droid.salvo) listId(salvos, droid.salvo); }
      for (const value of definitions) register(value, classes.has(ids.get(value)) ? 2 : 0);
      const rng = listId(streams, context.random);
      for (const weapon of weapons) if (weapon.random) listId(streams, weapon.random);
      if (streams.some(stream => typeof stream !== 'function' || !('seed' in stream)))
        throw Error('remote prediction requires independent seeded streams');
      const words = [context.lookX, context.lookY, context.zoomStep, context.firstPerson === undefined ? NaN : Number(context.firstPerson),
        piloted, rng, streams.length, ...streams.map(stream => stream.seed), Boolean(context.onFired), enabled];
      words.push(...camera.position.toArray(), ...camera.quaternion.toArray(), camera.aspect,
        view?.fov ?? NaN, view?.reach ?? NaN, Boolean(view?.fresh), view?.hfov ?? NaN, view?.firstFov ?? NaN, camera.fov,
        view?.motion.offset.x ?? 0, view?.motion.offset.y ?? 0, view?.motion.offset.z ?? 0,
        view?.motion.turn.x ?? 0, view?.motion.turn.y ?? 0, view?.motion.turn.z ?? 0);
      words.push(view ? register(view.sections) : -1);
      for (const name of cameraVectors) words.push(view?.[name].x ?? 0, view?.[name].y ?? 0, view?.[name].z ?? 0);
      words.push(...(view?.want.toArray() ?? [0, 0, 0, 1]), ...(view?.inverse.toArray() ?? [0, 0, 0, 1]),
        view?.euler.x ?? 0, view?.euler.y ?? 0, view?.euler.z ?? 0, view?.tilt ?? 0, weapons.length);
      for (const weapon of weapons) {
        const at = words.length; words.length += weaponWords;
        putWeapon(cx, weapon, words, at, null);
        words[at] = register(weapon.levels, 2);
        words[at + 1] = weapon.levels.indexOf(weapon.def);
        words[at + 14] = Boolean(weapon.random);
        words[at + 2] = Boolean(weapon.admitShot);
        if (weapon.admitShot) throw Error('private remote weapons cannot admit authoritative shots');
        words.push(weapon.level, weapon.levels.length);
        for (const def of weapon.levels) words.push(register(def));
        words.push(weapon.random ? streams.indexOf(weapon.random) : -1);
      }
      words.push(velocities.length);for (const velocity of velocities) words.push(...velocity.toArray());
      words.push(salvos.length);
      for (const salvo of salvos) {
        words.push(register(salvo.def));
        for (const name of ['at', 'x', 'y', 'z']) words.push(...salvo[name].toArray());
        words.push(salvo.left, salvo.wait);
      }
      words.push(objects.length);
      for (const droid of objects) {
        words.push(register(droid.def));
        words.push(...droid.position.toArray(), velocities.indexOf(droid.velocity));
        for (const name of vectors) words.push(...droid[name].toArray());
        words.push(droid.yaw, droid.pitch, droid.health, droid.dead, droid.countdown, ...droid.arsenal.selected);
        for (const list of [droid.arsenal.weapons, ...droid.arsenal.channels])
          words.push(list.length, ...list.map(weapon => weapons.indexOf(weapon)));
        words.push(droid.salvo ? salvos.indexOf(droid.salvo) : -1);
      }
      words.push(lists.length);
      for (const list of lists) words.push(list.length, ...list.map(item => objects.indexOf(item)));
      words.push(order, strikes);
      return words;
    }
    function publish(words) {
      let at = 0;
      context.lookX = words[at++]; context.lookY = words[at++]; context.zoomStep = words[at++];
      const first = words[at++]; context.firstPerson = Number.isNaN(first) ? undefined : Boolean(first);
      const piloted = words[at++], rng = words[at++], streamCount = words[at++];
      for (let i = 0; i < streamCount; i++) streams[i].seed = words[at++];
      context.random = streams[rng]; at++;
      const enabled = Boolean(words[at++]), camera = context.aimCamera;
      camera.position.fromArray(words, at); at += 3; camera.quaternion.fromArray(words, at); at += 4;
      camera.aspect = words[at++];
      const fov = words[at++], reach = words[at++], fresh = Boolean(words[at++]), hfov = words[at++], firstFov = words[at++];
      camera.fov = words[at++];
      if (view) {
        view.fov = fov; view.reach = reach; view.fresh = fresh; view.hfov = hfov; view.firstFov = firstFov;
        view.motion.offset.fromArray(words, at); view.motion.turn.set(words[at + 3], words[at + 4], words[at + 5], 'ZYX');
      }
      at += 6;
      const sections = words[at++];
      if (view && sections >= 0) view.sections = definitions[sections];
      for (const name of cameraVectors) { if (view) view[name].fromArray(words, at); at += 3; }
      if (view) view.want.fromArray(words, at); at += 4;
      if (view) view.inverse.fromArray(words, at); at += 4;
      if (view) view.euler.set(words[at], words[at + 1], words[at + 2], 'YXZ'); at += 3;
      if (view) view.tilt = words[at]; at++;
      context.piloted = piloted < 0 ? null : objects[piloted];
      context.simulationView = enabled ? view : null;
      if (!enabled) context.view = null;
      const weaponCount = words[at++];
      for (let i = 0; i < weaponCount; i++) {
        const weapon = weapons[i];
        let mirror = mirrors.get(weapon);
        if (!mirror) { mirror = new WeaponMirror(); mirrors.set(weapon, mirror); }
        syncWeapon(cx, words, at, 0, mirror); at += weaponWords;
        weapon.level = words[at++];
        const levels = words[at++];
        weapon.def = definitions[words[at + mirror.level]]; at += levels;
        for (const name of mutable) weapon[name] = mirror[name];
        if (mirror.melee) weapon.melee.restoreState(mirror.melee);
        const stream = words[at++];
        if (mirror.hasRandom && stream >= 0) weapon.random = streams[stream];
        copySeed(weapon, mirror);
      }
      const velocityCount = words[at++];
      for (let i = 0; i < velocityCount; i++) { velocities[i].fromArray(words, at); at += 3; }
      const salvoCount = words[at++];
      for (let i = 0; i < salvoCount; i++) {
        const salvo = salvos[i] ??= { at: new Vector3(), x: new Vector3(), y: new Vector3(), z: new Vector3() };
        salvo.def = definitions[words[at++]];
        for (const name of ['at', 'x', 'y', 'z']) { salvo[name].fromArray(words, at); at += 3; }
        salvo.left = words[at++]; salvo.wait = words[at++];
      }
      const count = words[at++];
      for (let i = 0; i < count; i++) {
        const droid = objects[i]; droid.def = definitions[words[at++]];
        droid.position.fromArray(words, at); at += 3; droid.velocity = velocities[words[at++]];
        for (const name of vectors) { droid[name].fromArray(words, at); at += 3; }
        droid.yaw = words[at++]; droid.pitch = words[at++]; droid.health = words[at++];
        droid.dead = Boolean(words[at++]); droid.countdown = words[at++];
        droid.arsenal.selected[0] = words[at++]; droid.arsenal.selected[1] = words[at++];
        for (const list of [droid.arsenal.weapons, ...droid.arsenal.channels]) {
          const n = words[at++]; list.length = n;
          for (let j = 0; j < n; j++) list[j] = weapons[words[at++]];
        }
        const salvo = words[at++]; droid.salvo = salvo < 0 ? null : salvos[salvo];
      }
      const listCount = words[at++];
      for (let i = 0; i < listCount; i++) {
        const list = lists[i] ??= [], n = words[at++]; list.length = n;
        for (let j = 0; j < n; j++) list[j] = objects[words[at++]];
      }
      context.droids = lists[words[at++]]; context.strikes = lists[words[at++]];
    }
    function answer(op, args, controls) {
      if (op === 0) {
        const tag = args[7], droid = objects[args[8]];
        const from = tag === 0 ? view.pivot : tag === 4 ? droid.eye : tag === 5 ? context.v[3] : context.v[0];
        const direction = tag === 0 ? view.dir : tag >= 4 ? droid.aimDir : tag === 3 ? context.v[4] : context.v[2];
        from.fromArray(args); direction.fromArray(args, 3);
        const hit = context.collision.cast(from, direction, args[6], tag === 0 ? view.hit : context.hit);
        return [...(hit ? [1, hit.distance, ...hit.point.toArray(), ...hit.normal.toArray()] : [0]), ...from.toArray(), ...direction.toArray()];
      }
      if (op === 1) {
        const center = context.v[0].fromArray(args), polygons = context.collision.gather(center, args[3]), words = [0];
        for (const polygon of polygons) for (const triangle of polygon.triangles) {
          words[0]++; words.push(...triangle.a.toArray(), ...triangle.b.toArray(), ...triangle.c.toArray());
        }
        words.push(...center.toArray());
        return words;
      }
      if (op === 2) return [Boolean(context.water?.over(args[0], args[1]))];
      if (op === 3) return [context.water.level];
      if (op === 4) return [Boolean(controls.take(actions[args[0]]))];
      if (op === 5) return [Boolean(controls.held('firePrimary'))];
      if (op === 6) {
        const point = context.firePoints[objects[args[0]].def.GeometryName];
        return point ? [1, point[0], point[1], point[2]] : [0];
      }
      if (op === 7) {
        const droid = objects[args[0]], weapon = weapons[args[1]], kind = args[2];
        let shot;
        if (kind === 0) shot = { weapon, yaw: args[3], pitch: args[4], charge: args[5], targetId: args[6], targetGeneration: args[7] };
        else if (kind === 1) shot = { weapon, repair: true };
        else if (kind === 2) { const swing = args[8], swings = weapon.def.swings; shot = { weapon, meleeStart: true, swing, sound: swings?.length ? swings[swing - 1].sound : null }; }
        else shot = { weapon, [kind === 3 ? 'meleeHit' : 'meleeEnd']: true };
        const at = context.v[2].fromArray(args, 9);
        context.v[4].fromArray(args, 12);
        context.onFired?.(shot, at, droid);
        return [];
      }
      if (op === 8) return [controls.axis('forward'), controls.axis('right'), controls.axis('turn')];
      if (op === 9) { context.aimCamera.updateProjectionMatrix(); return []; }
      throw Error('unknown remote prediction query');
    }
    const session = {
      get handle() { return handle; }, get disposed() { return !handle; }, get steps() { return steps; },
      step(command, controls, options = {}) {
        live(); if (busy) throw Error('remote prediction session is already running');
        busy = true;
        try {
          const look = command.look;
          const words = [1 / 60, typeof command.firstPerson === 'boolean' ? Number(command.firstPerson) : NaN,
            command.aspect, command.mouseScale ?? options.mouseScale ?? 1, Boolean(command.invertMouse), look.length];
          for (const pair of look) words.push(pair[0], pair[1]);
          let result = run(2, words); steps++;
          for (;;) {
            const { output, error } = result;
            if (output[0] === 0) {
              publish(output.slice(2));
              if (error) throw Error(error);
              break;
            }
            if (error) throw Error(error);
            const op = output[1], n = output[2], args = output.slice(3, 3 + n), state = output.slice(4 + n);
            publish(state);
            const values = answer(op, args, controls);
            const resumed = capture();
            result = run(3, [true, values.length, ...values, resumed.length, ...resumed]);
          }
        } finally { busy = false; }
      },
      dispose() {
        if (!handle) return;
        if (busy) throw Error('remote prediction session is already running');
        x.prediction_remotes_drop(handle); handle = 0; sessions.delete(session);
      },
    };
    sessions.add(session);
    try { const { error } = run(1, capture()); if (error) throw Error(error); }
    catch (error) { session.dispose(); throw error; }
    return session;
  }
  return {
    bridge, create, get activeSessions() { return sessions.size; },
    dispose() { if (disposed) return; for (const session of sessions) session.dispose(); disposed = true; },
  };
}
