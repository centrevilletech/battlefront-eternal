// Private movement, shield and recoil replay on the battle's shared bridge.
// Collision queries finish in Rust; copied output is published afterward.
import { bridgeFor } from '../sim/runtime.js';
import { Soldier } from '../soldier.js';
import { Droideka } from '../droideka.js';
import { PlayerCombat } from '../combat.js';
import { CollisionWorld } from '../collision.js';
import { MovementKernel } from '../movement-kernel.js';
import { Vector3 } from 'three';
import { putSoldier, putDroideka } from '../sim/gen/units.js';

const soldierPrototype = Object.getOwnPropertyDescriptors(Soldier.prototype);
const droidekaPrototype = Object.getOwnPropertyDescriptors(Droideka.prototype);
const collisionPrototype = Object.getOwnPropertyDescriptors(CollisionWorld.prototype);
const movementTick = MovementKernel.prototype.tick, updateKick = PlayerCombat.prototype.updateKick, vectorSet = Vector3.prototype.set;
const clipNames = ['idle', 'turnl', 'turnr', 'death01', null, 'idle_to_leftfoot', null, 'walk_leftfoot_rightfoot',
  'walk_rightfoot_leftfoot', 'leftfoot_to_idle', 'rightfoot_to_idle', 'fold', 'roll', 'unfold'];
const definitionKeys = ['MaxPitchSpeed', 'BallTurnSpeed', 'BallMaxLean', 'BallAcceleration', 'BallRollingFriction'];
const soldierNumbers = ['maxSpeed', 'maxStrafe', 'maxJump', 'maxTurn', 'maxPitch', 'accel', 'fuel', 'collisionDamage',
  'maxFlyHeight', 'getupTime', 'diveTime', 'aimYaw', 'aimPitch', 'stateTime', 'lock', 'dive', 'lookX', 'lookY', 'tickLength', 'groundAdjustment', 'contactCount'];
const droidekaNumbers = ['maxTurn', 'speed', 'turnThreshold', 'ballRadius', 'maxSpin', 'maxBallSlope', 'eyeHeight',
  'rootIndex', 'maxFlyHeight', 'collisionDamage', 'aimYaw', 'aimPitch', 'stateTime', 'lookX', 'lookY', 'mode', 'time',
  'legs', 'spin', 'roll', 'lean', 'blastClock', 'gun', 'dive', 'lock'];
const soldierVectors = ['position', 'previous', 'settled', 'velocity', 'normal', 'avoidance', 'contactMean'];
const droidekaVectors = ['position', 'previous', 'settled', 'velocity', 'avoidance'];
const postures = ['stand', 'crouch', 'prone'], airs = [null, 'jump', 'fall', 'thrown', 'jet'];
const rowContext = { defs: { unitClass: { id: () => 0 }, weapon: { id: () => 0 } } };
let nextSession = 1;
const flag = value => value ? 1 : 0;
const same = (a, b) => a?.length === b.length && b.every((value, i) => Object.is(value, a[i]));
const descriptor = (object, key) => {
  for (let at = object; at; at = Object.getPrototypeOf(at)) {
    const desc = Object.getOwnPropertyDescriptor(at, key); if (desc) return desc;
  }
};
const plain = (object, key, writable = false) => {
  const desc = descriptor(object, key);
  return !desc || 'value' in desc && (!writable || desc.writable);
};
function methods(object, prototype, saved) {
  if (Object.getPrototypeOf(object) !== prototype) return false;
  for (const [key, expected] of Object.entries(saved)) {
    if (key === 'constructor') continue;
    const got = descriptor(object, key);
    if (got?.value !== expected.value || got?.get !== expected.get || got?.set !== expected.set) return false;
  }
  return true;
}
const numeric = value => typeof value === 'number';
function numericFields(object, fields) { return object && fields.every(key => plain(object, key, true) && numeric(object[key])); }
function vector(value) {
  return value instanceof Vector3 && plain(value, 'set') && value.set === vectorSet && numericFields(value, ['x', 'y', 'z']);
}
function numbers(array, length) { return (Array.isArray(array) || ArrayBuffer.isView(array)) && array.length === length && Array.from(array).every(numeric); }
function eligible(s, prediction, input, collision, kind) {
  if (!['updateKick', 'kick', 'shieldEquipped', 'gunUnderwater'].every(key => plain(prediction, key))
    || !['world', 'pitchLimits', 'collisionRule', 'jet', 'air', 'diveFrom', 'posture', 'passThrough'].every(key => {
      if (kind && key === 'posture') return true;
      return plain(s, key);
    })) return false;
  if (prediction.updateKick !== updateKick || s.world !== collision || !methods(collision, CollisionWorld.prototype, collisionPrototype)) return false;
  if (!methods(s, kind ? Droideka.prototype : Soldier.prototype, kind ? droidekaPrototype : soldierPrototype)) return false;
  const vectors = kind ? droidekaVectors : soldierVectors;
  if (!numericFields(s, kind ? droidekaNumbers : soldierNumbers) || vectors.some(key => !plain(s, key) || !vector(s[key]))
    || new Set(vectors.map(key => s[key])).size !== vectors.length || !numbers(s.pitchLimits, 2)
    || !s.collisionRule || !plain(s.collisionRule, 'threshold') || !plain(s.collisionRule, 'scale')
    || !numbers(s.collisionRule.threshold, 3) || !numbers(s.collisionRule.scale, 3)) return false;
  if (!plain(s, 'grounded', true) || typeof s.grounded !== 'boolean') return false;
  if (!prediction.kick || !['rate', 'hold', 'offset'].every(key => plain(prediction.kick, key, true)
    && (prediction.kick[key] === undefined || numeric(prediction.kick[key])))) return false;
  if (!input || !['forward', 'right', 'turn', 'pitch'].every(key => plain(input, key) && (input[key] === undefined || numeric(input[key])))
    || !['jump', 'crouch', 'prone', 'use', 'jetDirection'].every(key => plain(input, key))) return false;
  if (input.jetDirection && (!vector(input.jetDirection) || vectors.some(key => s[key] === input.jetDirection))) return false;
  if (!plain(collision, 'bodies') || !Array.isArray(collision.bodies) || collision.bodies.some(body => !plain(body, 'enabled'))) return false;
  if (kind) {
    if (!Number.isInteger(s.mode) || s.mode < 0 || s.mode > 13 || !['reverse', 'dead'].every(key => plain(s, key, true) && typeof s[key] === 'boolean')
      || !plain(s, 'shield') || !plain(s, 'air') || !plain(s, 'def') || !plain(s, 'clips')
      || !(s.clips instanceof Map) || s.clips.get !== Map.prototype.get) return false;
    if (s.shield && (!numericFields(s.shield, ['value', 'max', 'drain', 'refill']) || !plain(s.shield, 'on', true) || typeof s.shield.on !== 'boolean')) return false;
    if (!definitionKeys.every(key => plain(s.def, key) && (s.def[key] == null || numeric(s.def[key])))) return false;
  } else {
    if (!postures.includes(s.posture) || !postures.includes(s.diveFrom) || !airs.includes(s.air)
      || !numericFields(s.jet, ['jump', 'push', 'recharge', 'cost', 'initial', 'minBorder']) || !plain(s.jet, 'hover') || typeof s.jet.hover !== 'boolean'
      || !plain(s, 'stepBlocked', true) || typeof s.stepBlocked !== 'boolean') return false;
    if (collision.movement && (!(collision.movement instanceof MovementKernel) || collision.movement.tick !== movementTick)) return false;
  }
  return true;
}

function tracksSupported(tracks) {
  if (Array.isArray(tracks)) {
    if (Object.getPrototypeOf(tracks) !== Array.prototype || !plain(tracks, 'indexOf') || tracks.indexOf !== Array.prototype.indexOf
      || !plain(tracks, Symbol.iterator) || tracks[Symbol.iterator] !== Array.prototype[Symbol.iterator]) return false;
    for (let i = 0; i < tracks.length; i++) if (!plain(tracks, i)) return false;
  } else if (!(ArrayBuffer.isView(tracks) && Object.getPrototypeOf(tracks) === Int32Array.prototype)
    || Object.hasOwn(tracks, 'length') || !plain(tracks, 'indexOf') || tracks.indexOf !== Int32Array.prototype.indexOf
    || !plain(tracks, Symbol.iterator) || tracks[Symbol.iterator] !== Int32Array.prototype[Symbol.iterator]) return false;
  for (let i = 0; i < tracks.length; i++) if (!Number.isInteger(tracks[i]) || tracks[i] < -1 || tracks[i] > 2147483647) return false;
  return true;
}
function clipEqual(copy, clip) {
  if (!clip) return copy === null;
  if (!copy || copy.tracks.length !== clip.tracks.length || copy.data.length !== clip.data.length) return false;
  for (let i = 0; i < copy.tracks.length; i++) if (!Object.is(copy.tracks[i], clip.tracks[i])) return false;
  for (let i = 0; i < copy.data.length; i++) if (!Object.is(copy.data[i], clip.data[i])) return false;
  return true;
}
function config(s, kind, owner) {
  if (!kind) return same(owner.signature, [0]) ? null : { signature: [0], words: [0], clips: null };
  const clips = clipNames.map(name => name === null ? null : s.clips.get(name));
  const values = definitionKeys.flatMap(key => [flag(s.def[key] != null), s.def[key] ?? 0]);
  const signature = [1, s.def, s.clips, ...values];
  for (const c of clips) {
    if (c && (!['frames', 'fps', 'rootMotion', 'tracks', 'data'].every(key => plain(c, key))
      || !Number.isInteger(c.frames) || c.frames <= 0 || !numeric(c.fps)
      || !(ArrayBuffer.isView(c.data) && Object.getPrototypeOf(c.data) === Float32Array.prototype) || Object.hasOwn(c.data, 'length')
      || !tracksSupported(c.tracks) || c.data.length !== c.tracks.length * c.frames * 7)) return false;
    signature.push(c, c?.frames, c?.fps, c?.tracks, c?.data, c?.rootMotion);
  }
  if (same(owner.signature, signature) && clips.every((c, i) => clipEqual(owner.clips[i], c))) return null;
  // Content normally stays fixed, but a replay must also observe edits made
  // in place between steps. Compare against private copies, not array identity.
  const words = [1, ...values], copies = [];
  for (const c of clips) {
    words.push(flag(c));
    if (c) {
      const tracks = [], data = new Float32Array(c.data.length);
      words.push(c.frames, c.fps, flag(c.rootMotion), c.tracks.length);
      for (let i = 0; i < c.tracks.length; i++) { tracks.push(c.tracks[i]); words.push(c.tracks[i]); }
      words.push(data.length);
      for (let i = 0; i < data.length; i++) { data[i] = c.data[i]; words.push(data[i]); }
      copies.push({ tracks, data });
    } else {
      copies.push(null);
    }
  }
  return { signature, words, clips: copies };
}
function ingress(s, kick, input, dt, collision, kind, prediction, pressed) {
  const flags = collision.bodies.map(body => flag(body.enabled)), words = [collision.rayWorld, kind ? -1 : s.passThrough?.raySlot ?? -1, dt, flags.length, ...flags];
  const body = Object.create(s), row = new Array(kind ? 77 : 64).fill(0);
  if (kind) {
    Object.defineProperties(body, { def: { value: null }, muzzleAnimator: { value: undefined }, waterClock: { value: undefined }, lastGun: { value: undefined } });
    putDroideka(rowContext, body, row, 0, null);
  } else {
    Object.defineProperties(body, { passThrough: { value: null }, mover: { value: -1 }, dead: { value: undefined } });
    putSoldier(rowContext, body, row, 0, null);
  }
  words.push(...row);
  if (!kind) words.push(s.contactCount, s.contactMean.x, s.contactMean.y, s.contactMean.z, flag(s.stepBlocked), flag(collision.movement));
  const jet = input.jetDirection;
  words.push(input.forward, input.right, input.turn, input.pitch, flag(input.jump), flag(input.crouch), flag(input.prone), flag(input.use), flag(jet), jet?.x ?? 0, jet?.y ?? 0, jet?.z ?? 0,
    kick.rate, kick.hold, kick.offset, flag(prediction.shieldEquipped), flag(prediction.gunUnderwater), flag(pressed), flag(s.air && s.air !== 'jet'), flag(s.dive), s.lock);
  return words;
}
function publish(s, kick, output, kind, movement) {
  const vec = (name, at) => s[name].set(output[at], output[at + 1], output[at + 2]);
  if (kind) {
    for (const [name, at] of [['position', 19], ['previous', 22], ['settled', 25], ['velocity', 28], ['avoidance', 31]]) vec(name, at);
    for (const [name, at] of [['collisionDamage', 18], ['aimYaw', 34], ['aimPitch', 35], ['stateTime', 36], ['lookX', 38], ['lookY', 39],
      ['mode', 40], ['time', 41], ['legs', 43], ['spin', 44], ['roll', 45], ['lean', 46], ['blastClock', 48]]) s[name] = output[at];
    s.grounded = Boolean(output[37]); s.reverse = Boolean(output[42]);
    if (s.shield) { s.shield.value = output[56]; s.shield.on = Boolean(output[57]); }
  } else {
    for (const [name, at] of [['position', 26], ['previous', 29], ['settled', 32], ['velocity', 35], ['normal', 38], ['avoidance', 55]]) vec(name, at);
    for (const [name, at] of [['fuel', 15], ['collisionDamage', 22], ['aimYaw', 41], ['aimPitch', 42], ['dive', 46], ['stateTime', 48],
      ['lock', 49], ['lookX', 51], ['lookY', 52], ['tickLength', 53], ['groundAdjustment', 54]]) s[name] = output[at];
    s.posture = postures[output[43]]; s.air = output[44] ? null : airs[output[45] + 1]; s.diveFrom = postures[output[47]]; s.grounded = Boolean(output[50]);
    if (!movement) { s.contactCount = output[64]; vec('contactMean', 65); s.stepBlocked = Boolean(output[68]); }
  }
  const at = kind ? 77 : 69;
  if (output[at]) { kick.rate = output[at + 1]; kick.hold = output[at + 2]; kick.offset = output[at + 3]; }
}

export function createActorPredictionSim(instance, collision, options) {
  return createActorPredictionBridge(bridgeFor(instance), collision, options);
}
// The bridge owns one reusable native session per body until bridge.dispose().
// Wrapper disposal releases its lease; disposing the world must dispose its bridge.
export function createActorPredictionBridge(bridge, collision, { trace = null } = {}) {
  const x = bridge.x;
  if (!collision?.rays || collision.rays.x !== x) throw Error('actor prediction requires the collision instance shared by its bridge');
  const bodies = new WeakMap(), owners = new Set(); let disposed = false;
  const live = () => { if (disposed) throw Error('actor prediction bridge is disposed'); };
  function drop(owner) {
    if (owner.busy) { owner.dropAfter = true; return; }
    if (owner.handle) { x.prediction_actors_drop(owner.handle); owner.handle = 0; }
    owners.delete(owner);
  }
  function run(owner, op, values) {
    live();
    const input = Float64Array.from(values);
    live();
    const ptr = x.prediction_actors_room(owner.handle, input.length);
    bridge.views(); bridge.f.set(input, ptr / 8);
    const ok = x.prediction_actors_run(owner.handle, op);
    bridge.views();
    const at = x.prediction_actors_output(owner.handle) / 8;
    const output = Array.from(bridge.f.subarray(at, at + x.prediction_actors_output_len(owner.handle)));
    const error = ok ? null : new TextDecoder().decode(new Uint8Array(x.memory.buffer, x.prediction_actors_error(owner.handle), x.prediction_actors_error_len(owner.handle)));
    owner.events.push({ session: owner.serial, op, input: Array.from(input), output: output.slice(), error });
    if (!ok) throw Error(error);
    return output;
  }
  return {
    bridge, collision,
    bind(soldier) {
      live();
      let owner = bodies.get(soldier);
      if (!owner) { owner = { serial: nextSession++, handle: 0, signature: null, clips: null, busy: false, dropAfter: false, events: [] }; bodies.set(soldier, owner); }
      let released = false, steps = 0;
      const active = () => { live(); if (released) throw Error('actor prediction session is disposed'); };
      return {
        get handle() { return released ? 0 : owner.handle; }, get steps() { return steps; },
        dispose() { released = true; },
        step(prediction, input, dt, pressed = false) {
          active();
          if (owner.busy) throw Error('actor prediction session is busy');
          owner.busy = true; owner.events = [];
          try {
            const kind = Object.getPrototypeOf(soldier) === Droideka.prototype ? 1 : 0;
            if (collision.rays?.x !== x || !numeric(dt) || !eligible(soldier, prediction, input, collision, kind)) return false;
            collision.rayBodies();
            active();
            const setup = config(soldier, kind, owner);
            active();
            if (setup === false) return false;
            const values = ingress(soldier, prediction.kick, input, dt, collision, kind, prediction, pressed);
            active();
            if (!owner.handle) { owner.handle = x.prediction_actors_create(); owners.add(owner); }
            if (setup) {
              run(owner, 0, setup.words); owner.signature = setup.signature; owner.clips = setup.clips;
            }
            const output = run(owner, 1, values);
            publish(soldier, prediction.kick, output, kind, Boolean(collision.movement)); steps++;
            return true;
          } finally {
            // A trace can grow memory, run a different owner, or dispose the
            // bridge. Its records and the already published output are owned.
            try { if (trace) for (const event of owner.events) trace(event); }
            finally { owner.events = []; owner.busy = false; if (owner.dropAfter) drop(owner); }
          }
        },
      };
    },
    dispose() { if (disposed) return; disposed = true; for (const owner of owners) drop(owner); },
  };
}
