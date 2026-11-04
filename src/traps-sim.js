// Live log-trap state machine. Collision/presentation writes and hull
// deaths remain world callbacks; the module decides their order and timing.
import * as THREE from 'three';
import { bridgeFor } from './sim/runtime.js';
import { TrResultMirror, putTrInput, syncTrResult } from './sim/gen/index.js';
import { emitSound } from './audio/events.js';

const xyz = p => [p.x, p.y, p.z];
function state(t) {
  return { built: t.state === 'built', health: t.health, phase: t.phase, started: t.started,
    time: t.time, sprung: t.sprung, tick: t.tick, crushed: t.crushed };
}
function accept(t, s, showing = false) {
  // show() itself changes state. At its entry JavaScript still has the
  // old state, while the native boolean describes the requested one.
  if (!showing) t.state = s.built ? 'built' : 'unbuilt';
  t.health = s.health; t.phase = s.phase; t.started = s.started;
  t.time = s.time; t.sprung = s.sprung; t.tick = s.tick; t.crushed = s.crushed;
}
function settings(t) {
  return { maxHealth: t.maxHealth, reach: t.reach, minRadius: t.minRadius, deathTime: t.deathTime,
    buildFrames: t.buildClip?.frames ?? null, triggerFrames: t.triggerClip?.frames ?? null,
    triggerTeam: t.settings.triggerTeam ?? null, sound: t.settings.sound ?? null, deathEffect: t.settings.deathEffect ?? null,
    point: xyz(t.point), effectAt: xyz(t.effectAt) };
}

export function attachTrapsSim(instance, traps) {
  const bridge = bridgeFor(instance), x = bridge.x, center = new THREE.Vector3();
  const input = { words: x.traps_words(0), put: putTrInput };
  const output = { Mirror: TrResultMirror, sync: syncTrResult };
  bridge.register({
    trapsQuery({ op, index, vehicle }) {
      const out = { flag: false, number: 0, position: [0, 0, 0], name: null };
      if (op === 'alive') out.flag = Boolean(vehicle?.alive);
      else if (op === 'side') out.name = traps[index].sideOf(vehicle.team) ?? null;
      else if (op === 'sphere') {
        if (vehicle.blastSphere) out.number = vehicle.blastSphere(center);
        else { center.copy(vehicle.position); out.number = vehicle.radius || 0; }
        out.position = xyz(center);
      }
      return out;
    },
    trapsAction({ op, index, state: current, vehicle, number, flag, name, position }) {
      const t = traps[index], point = new THREE.Vector3(...position);
      accept(t, current, op === 'show');
      if (op === 'clearAttacker') vehicle.lastAttacker = null;
      else if (op === 'setHealth') vehicle.setHealth(number);
      else if (op === 'setDying') vehicle.dying = number;
      else if (op === 'hideRoot') { if (vehicle.root) vehicle.root.visible = false; }
      else if (op === 'show') t.show(flag ? 'built' : 'unbuilt');
      else if (op === 'sound') emitSound('sound', name, point);
      else if (op === 'effect') t.onEffect(name, point);
      return { state: state(t) };
    },
  });

  traps.forEach((source, index) => {
    function run(op, args = {}) {
      bridge.callArgs(input, { op, index, state: state(source), settings: settings(source), dt: 0, tick: 0,
        value: 0, phase: 'down', started: null, vehicle: null, vehicles: [], ...args });
      const result = bridge.callResult(output, x.traps_run());
      accept(source, result.state);
      return result;
    }
    const methods = {
      setHealth(value) { run('setHealth', { value }); },
      begin(phase, started) { run('begin', { phase, started }); },
      step(dt, tick, vehicles = []) { run('step', { dt, tick, vehicles }); },
      crush(vehicles) { run('crush', { vehicles }); },
      catches(vehicle) { return run('catches', { vehicle: vehicle ?? null }).flag; },
      stateRecord() { return [...run('stateRecord').record]; },
    };
    Object.defineProperties(source, Object.fromEntries(Object.entries(methods).map(([key, value]) => [key, { value, configurable: true }])));
  });
  return { bridge, traps };
}
