// One private Rust replay per page frame. Packing and publication happen at
// this boundary; no JavaScript callback participates in a simulated tick.
import { bridgeFor } from '../sim/runtime.js';
import { ClientHeld_INDEX, ClientAction_INDEX, putClientFrame, putClientCommand, putClientLook, putClientCrowd,
  syncClientResult, ClientResultMirror, syncClientShot, ClientShotMirror,
  CLIENT_FRAME_WORDS, CLIENT_COMMAND_WORDS, CLIENT_LOOK_WORDS, CLIENT_CROWD_WORDS, CLIENT_SHOT_WORDS } from '../sim/gen/client.js';
import { putWeapon, syncWeapon, WeaponMirror } from '../sim/gen/combat.js';
import { predictionActorConfig, packPredictionActor, publishPredictionActor } from './actor-prediction-sim.js';
import { packPredictionWeaponDefinition } from './weapon-prediction-sim.js';
import { captureCompactSoldier } from './network-entity-soldier-compact.js';
import { captureDroidekaState } from './network-droideka-state.js';
import { Droideka } from '../droideka.js';
import { CommandControls } from './commands.js';
import { seededRandom } from '../weapons.js';

const FRAME = CLIENT_FRAME_WORDS, COMMAND = CLIENT_COMMAND_WORDS, LOOK = CLIENT_LOOK_WORDS;
const CROWD = CLIENT_CROWD_WORDS, SHOT = CLIENT_SHOT_WORDS;
const mutable = ['deployed','firing','timer','bottomless','perShot','clip','reserve','heat','overheated','floor','kick',
  'spreadScale','sniperScale','yawSpreadOverride','wait','reloadLeft','throwTime','released','hidden','charging','held','charge',
  'limit','pattern','salvosLeft','salvoWait','lockTargetId','lockGeneration','lockElapsed','lockComplete'];
const weaponWords = 42;
const heldFlag = name => 1 << ClientHeld_INDEX.get(name);
const FORWARD = heldFlag('forward'), BACKWARD = heldFlag('backward'), RIGHT = heldFlag('right'), LEFT = heldFlag('left');
const TURN_RIGHT = heldFlag('turnRight'), TURN_LEFT = heldFlag('turnLeft'), PRIMARY = heldFlag('primary'), SECONDARY = heldFlag('secondary');
const shape = posture => posture === 'crouch' ? [0.45, 1.45, 0.65] : posture === 'prone' ? [0.8, 0.45, 0.25] : [0.57, 1.65, 0.9];
function heldBits(controls, keys) {
  controls.down.clear(); for (const key of keys) controls.down.add(key);
  const has = key => controls.down.has(key);
  return Number(has('KeyW') || has('ArrowUp')) * FORWARD | Number(has('KeyS') || has('ArrowDown')) * BACKWARD
    | Number(has('KeyD') || has('ArrowRight')) * RIGHT | Number(has('KeyA') || has('ArrowLeft')) * LEFT
    | Number(has('NumpadMultiply')) * TURN_RIGHT | Number(has('NumpadDivide')) * TURN_LEFT
    | Number(controls.held('firePrimary')) * PRIMARY | Number(controls.held('fireSecondary')) * SECONDARY;
}

export function createClientPrediction(instance, collision) {
  const bridge = bridgeFor(instance), x = bridge.x;
  if (collision.rays?.x !== x) throw Error('prediction session requires shared collision');
  let handle = x.client_prediction_create(), body = null, source = null, kind = 0, sourceId = 0;
  let disposed = false, busy = false, dropAfter = false, active = false;
  let input = new Float64Array(4096), output = new Float64Array(4096), used = 0;
  let inputPtr = 0, inputCapacity = 0;
  const controls = new CommandControls(), config = { signature: null, clips: null };
  const result = new ClientResultMirror(), notice = new ClientShotMirror(), mirrors = [], seeds = [];
  const counters = { frames: 0, calls: 0, ticks: 0, wordsIn: 0, wordsOut: 0, setupCalls: 0, reserves: 0 };
  const transfers = { headerIn: 0, actorIn: 0, correctionIn: 0, weaponsIn: 0, flagsIn: 0, commandsIn: 0, lookIn: 0, crowdIn: 0,
    headerOut: 0, poseOut: 0, beforeOut: 0, correctionOut: 0, weaponsOut: 0, shotsOut: 0 };
  const live = () => { if (disposed) throw Error('prediction session is disposed'); };
  function room(n) {
    if (n <= input.length) return;
    let cap = input.length; while (cap < n) cap *= 2;
    const next = new Float64Array(cap); next.set(input); input = next;
  }
  function reserve(n) { const at = used; used += n; room(used); return at; }
  function run(op, words, count = words.length) {
    live();
    let ptr;
    if (op === 2) {
      if (inputCapacity < count) {
        inputCapacity = input.length;
        inputPtr = x.client_prediction_room(handle, inputCapacity); counters.reserves++;
      }
      ptr = inputPtr;
    } else {
      ptr = x.client_prediction_room(handle, count);
      // A setup command can relocate the exchange allocation.
      inputCapacity = 0;
    }
    bridge.views();
    for (let i = 0; i < count; i++) bridge.f[ptr / 8 + i] = words[i];
    const published = op === 2 ? x.client_predict_session(handle, count) : 0;
    const ok = op === 2 ? published !== 0 : x.client_prediction_run(handle, op);
    bridge.views();
    if (!ok) throw Error(new TextDecoder().decode(new Uint8Array(x.memory.buffer, x.client_prediction_error(handle), x.client_prediction_error_len(handle))));
    let length, at;
    if (op === 2) {
      at = published / 8;
      syncClientResult(null, bridge.f, at, 0, result);
      length = result.shotsAt + result.shotsCount * (SHOT + result.actorWords);
    } else {
      length = x.client_prediction_output_len(handle);
      at = x.client_prediction_output(handle) / 8;
    }
    if (output.length < length) output = new Float64Array(Math.max(length, output.length * 2));
    for (let i = 0; i < length; i++) output[i] = bridge.f[at + i];
    if (op === 2) { counters.calls++; counters.wordsIn += count; counters.wordsOut += length; }
    else counters.setupCalls++;
    return length;
  }
  function configure(unit) {
    const s = unit.soldier, authority = unit.combat.arsenal;
    if (body === s && source === authority) return;
    body = s; source = authority; kind = s instanceof Droideka ? 1 : 0; sourceId++;
    config.signature = null; config.clips = null;
    const setup = predictionActorConfig(s, kind, config);
    if (!setup) throw Error('unsupported prediction actor content');
    run(0, setup.words); config.signature = setup.signature; config.clips = setup.clips;
    for (const weapon of authority.weapons) {
      const words = [bridge.defs.weapon.id(weapon.levels[0])]; packPredictionWeaponDefinition(words, weapon.levels[0]); run(1, words);
    }
  }
  function publishWeapons(unit, at) {
    const arsenal = unit.combat.arsenal, count = output[at++];
    for (let i = 0; i < count; i++, at += weaponWords) {
      const weapon = arsenal.weapons[i];
      const m = syncWeapon(bridge, output, at, 0, mirrors[i] ||= new WeaponMirror());
      weapon.level = m.level; weapon.def = weapon.levels[m.level];
      for (const key of mutable) weapon[key] = m[key];
      if (m.melee) weapon.melee.restoreState(m.melee);
      if (m.salvoSeed === null) { weapon.salvoRandom = null; seeds[i] = null; }
      else {
        // This runs at publication only. A resumed JS twin gets the same
        // seeded cursor without making any per-tick WASM calls.
        let cursor = seeds[i];
        if (!cursor || cursor.seed !== m.salvoSeed || cursor.count !== m.salvoCount) {
          cursor = { seed: m.salvoSeed, count: m.salvoCount, random: null };
          cursor.draw = () => { if (!cursor.random) { cursor.random = seededRandom(cursor.seed); for (let n = 0; n < cursor.count; n++) cursor.random(); } return cursor.random(); };
          seeds[i] = cursor;
        }
        weapon.salvoRandom = cursor.draw;
      }
      weapon.salvoSeed = m.salvoSeed; weapon.salvoCount = m.salvoCount;
    }
    arsenal.selected[0] = output[at]; arsenal.selected[1] = output[at + 1]; unit.combat.zoomStep = output[at + 2];
  }
  function publishActor(unit, at, kick) {
    // The old value publisher's kick/scratch suffix is deliberately absent:
    // the session returns its kick separately and restores completed scratch.
    publishPredictionActor(unit.soldier, kick, output.subarray(at, at + result.actorWords), kind, true);
  }
  function drop() { if (handle) { x.client_prediction_drop(handle); handle = 0; } }
  return {
    bridge, counters, transfers,
    get handle() { return handle; }, get disposed() { return disposed; },
    reset() { live(); if (active) { run(3, []); active = false; } },
    dispose() { if (disposed) return; disposed = true; if (busy) dropAfter = true; else drop(); },
    predict(unit, pending, { epoch, tick, held = [], view = {}, mouseScale = 1, fireEdges = [], crowd = [], checkTick = 0, presentedThrough = tick } = {}) {
      live(); if (busy) throw Error('prediction session is busy'); busy = true;
      try {
        configure(unit); collision.rayBodies();
        const combat = unit.combat, s = unit.soldier, kick = { ...combat.kick };
        used = FRAME;
        const actorInput = packPredictionActor(s, kick, { forward: 0, right: 0, turn: 0, pitch: 0 }, 1 / 60, collision, kind,
          { shieldEquipped: combat.arsenal.current(1)?.def.base === 'shield', gunUnderwater: combat.gunUnderwater }, false);
        const actorWords = kind ? 77 : 64;
        const actorAt = reserve(actorWords), flagsAt = reserve(collision.bodies.length);
        const start = 4 + collision.bodies.length;
        for (let i = 0; i < actorWords; i++) input[actorAt + i] = actorInput[start + i];
        for (let i = 0; i < collision.bodies.length; i++) input[flagsAt + i] = Number(collision.bodies[i].enabled);
        const correction = kind ? captureDroidekaState(s) : captureCompactSoldier(s);
        const correctionAt = reserve(correction.length); input.set(correction, correctionAt);
        const weaponsAt = reserve(1 + combat.arsenal.weapons.length * weaponWords + 4);
        let at = weaponsAt; input[at++] = combat.arsenal.weapons.length;
        for (const weapon of combat.arsenal.weapons) { putWeapon(bridge, weapon, input, at, null); at += weaponWords; }
        input[at++] = combat.arsenal.selected[0]; input[at++] = combat.arsenal.selected[1]; input[at++] = combat.zoomStep; input[at] = 0;
        const commandsAt = reserve(pending.length * COMMAND);
        let lookWords = 0;
        for (let i = 0; i < pending.length; i++) {
          const c = pending[i], look = c.worldLook ?? c.look ?? [], lookAt = reserve(look.length * LOOK);
          lookWords += look.length * LOOK;
          for (let n = 0; n < look.length; n++) putClientLook(null, { x: look[n][0], y: look[n][1] }, input, lookAt + n * LOOK, null);
          let actions = 0, primaryCount = 0, primaryId = 0, secondaryCount = 0, secondaryId = 0;
          for (const name of c.actions) { const bit = ClientAction_INDEX.get(name); if (bit !== undefined) actions |= 1 << bit; }
          for (const edge of fireEdges) {
            if (edge.sequence !== c.sequence || edge.kind !== 'fire' || !edge.pressed) continue;
            if (edge.button === 'firePrimary') { primaryCount++; primaryId = edge.id; }
            if (edge.button === 'fireSecondary') { secondaryCount++; secondaryId = edge.id; }
          }
          putClientCommand(null, { tick: c.tick, generation: c.generation, sequence: c.sequence, sent: c.sent !== false,
            held: heldBits(controls, c.down || []), actions, lookAt, lookCount: look.length,
            mouseScale: c.mouseScale, invert: c.invertMouse, primaryCount, primaryId, secondaryCount, secondaryId }, input, commandsAt + i * COMMAND, null);
        }
        const crowdAt = reserve(crowd.length * CROWD);
        for (let i = 0; i < crowd.length; i++) {
          const other = crowd[i], [radius, height, center] = other.walker ? [1, 2, 1] : shape(other.hitPosture);
          putClientCrowd(null, { position: other.position, radius, height, center }, input, crowdAt + i * CROWD, null);
        }
        putClientFrame(null, { epoch, identity: unit.id, generation: unit.generation, source: sourceId, authorityTick: tick,
          world: collision.rayWorld, ignored: kind ? -1 : s.passThrough?.raySlot ?? -1, movement: Boolean(collision.movement), dead: false,
          underwater: Boolean(combat.gunUnderwater), shield: combat.arsenal.current(1)?.def.base === 'shield',
          actorAt, actorWords, correctionAt, correctionBytes: correction.length, weaponsAt, weaponsWords: 1 + combat.arsenal.weapons.length * weaponWords + 4,
          flagsAt, flagsCount: collision.bodies.length, commandsAt, commandsCount: pending.length, crowdAt, crowdCount: crowd.length,
          held: heldBits(controls, held), presentedThrough, mouseScale: view.mouseScale ?? mouseScale, invert: view.invertMouse || false,
          kick: [kick.rate, kick.hold, kick.offset], checkTick }, input, 0, null);
        run(2, input, used); active = true; syncClientResult(null, output, 0, 0, result);
        counters.frames++; counters.ticks += result.ticks;
        transfers.headerIn += FRAME; transfers.actorIn += actorWords; transfers.correctionIn += correction.length;
        transfers.weaponsIn += 1 + combat.arsenal.weapons.length * weaponWords + 4;
        transfers.flagsIn += collision.bodies.length; transfers.commandsIn += pending.length * COMMAND;
        transfers.lookIn += lookWords; transfers.crowdIn += crowd.length * CROWD;
        transfers.headerOut += result.actorAt; transfers.poseOut += result.actorWords;
        transfers.beforeOut += result.ticks ? result.actorWords : 0;
        transfers.correctionOut += result.correctionBytes; transfers.weaponsOut += result.weaponsWords;
        transfers.shotsOut += result.shotsCount * (SHOT + result.actorWords);
        publishActor(unit, result.actorAt, kick); publishWeapons(unit, result.weaponsAt);
        return result;
      } finally { busy = false; if (dropAfter) drop(); }
    },
    correction() { return Uint8Array.from(output.subarray(result.correctionAt, result.correctionAt + result.correctionBytes)); },
    present(unit, presentation, onShot) {
      live(); if (busy) throw Error('prediction session is busy'); busy = true;
      const kick = {};
      try {
        let before = false;
        const beforeStep = () => { before = true; publishActor(unit, result.beforeAt, kick); presentation?.beforeStep(); };
        let at = result.shotsAt;
        for (let i = 0; i < result.shotsCount; i++) {
          if (result.ticks && i === result.beforeShot) beforeStep();
          syncClientShot(null, output, at, 0, notice);
          publishActor(unit, at + notice.actorAt, kick);
          const w = unit.combat.arsenal.weapons[notice.weapon], level = w.level, def = w.def;
          w.level = notice.level; w.def = w.levels[notice.level];
          try { onShot?.(notice, w); } finally { w.level = level; w.def = def; }
          at += SHOT + result.actorWords;
        }
        if (result.ticks && !before) beforeStep();
        publishActor(unit, result.actorAt, kick);
        if (result.ticks && presentation) { presentation.previousYaw += result.lookYaw; presentation.afterStep(); }
      } finally {
        try { publishActor(unit, result.actorAt, kick); }
        finally { busy = false; if (dropAfter) drop(); }
      }

    },
  };
}
