// game.js replayPrediction and restorePrediction now cross once per frame.
// Rust owns every pending tick; these retained rows only import page facts and
// publish the result. Clocks, smoothing and effects keep their page lifetime.
import { bridgeFor, Heap } from '../sim/runtime.js';
import { ClientOp_INDEX, ClientActorKind_INDEX, ClientHeld_INDEX, ClientAction_INDEX,
  putClientFrame, ClientFrameMirror, putClientCommand, ClientCommandMirror, putClientLook, ClientLookMirror,
  putClientCrowd, ClientCrowdMirror, putClientMuzzles, putClientMount, ClientMountMirror,
  syncClientResult, ClientResultMirror, syncClientShot, ClientShotMirror,
  CLIENT_MUZZLES_WORDS, CLIENT_FRAME_WORDS, CLIENT_COMMAND_WORDS, CLIENT_LOOK_WORDS,
  CLIENT_CROWD_WORDS, CLIENT_MOUNT_WORDS, CLIENT_SHOT_WORDS, CLIENT_WEAPONS_WORDS,
  CLIENT_WEAPONS_OFFSETS, CLIENT_ARSENAL_TAIL_WORDS, CLIENT_ARSENAL_TAIL_OFFSETS,
  CLIENT_BARREL_RESULT_WORDS, CLIENT_BARREL_RESULT_OFFSETS } from '../sim/gen/client.js';
import { putWeapon, syncWeapon, publishWeapon, WeaponMirror, WEAPON_WORDS } from '../sim/gen/combat.js';
import { predictionActorConfig, packPredictionWeaponDefinition } from './client-content.js';
import { putSoldier, putDroideka, syncSoldier, syncDroideka, publishSoldier, publishDroideka,
  SoldierMirror, DroidekaMirror, SOLDIER_WORDS, DROIDEKA_WORDS } from '../sim/gen/units.js';
import { writeCompactSoldier, COMPACT_SOLDIER_MAX_BYTES } from './network-entity-soldier-compact.js';
import { writeDroidekaState, DROIDEKA_STATE_MAX_BYTES } from './network-droideka-state.js';
import { Droideka } from '../droideka.js';
import { HOLDS } from '../input.js';

// The operation discriminants are shared with client_prediction_run. Resolve
// their names at module load; a frame never interprets an unlabelled number.
const CONFIGURE_ACTOR = ClientOp_INDEX.get('configureActor'), CONFIGURE_WEAPON = ClientOp_INDEX.get('configureWeapon');
const PREDICT = ClientOp_INDEX.get('predict'), RESET = ClientOp_INDEX.get('reset'), CONFIGURE_MUZZLES = ClientOp_INDEX.get('configureMuzzles');
const SOLDIER = ClientActorKind_INDEX.get('soldier'), DROIDEKA = ClientActorKind_INDEX.get('droideka');
const FRAME = CLIENT_FRAME_WORDS, COMMAND = CLIENT_COMMAND_WORDS, LOOK = CLIENT_LOOK_WORDS;
const CROWD = CLIENT_CROWD_WORDS, SHOT = CLIENT_SHOT_WORDS, EMPTY = Object.freeze([]), NO_VIEW = Object.freeze({});
const rowContext = { defs: { unitClass: { id: () => 0, at: () => null }, weapon: { id: () => 0, at: () => null } } };
const seededMarker = () => { throw Error('prediction weapons are a presentation view'); };
// These sizes describe existing rows; diagnostic tests read them on demand.
// The shipped frame path has no counter, transfer tally or diagnostic branch.
const layout = Object.freeze({ frame: FRAME, command: COMMAND, look: LOOK, crowd: CROWD,
  mount: CLIENT_MOUNT_WORDS, shot: SHOT, correctionBytesPerWord: 4 });

// CommandControls.held used a temporary Set and a predicate for each command.
// Controls.command already sends world keys, so resolve the fixed key table
// once. Keep opposing directions and every secondary-fire alias distinct.
const heldKeys = new Map();
function bindHeld(name, keys) {
  const flag = 1 << ClientHeld_INDEX.get(name);
  for (const key of keys) heldKeys.set(key, (heldKeys.get(key) || 0) | flag);
}
bindHeld('forward', ['KeyW', 'ArrowUp']); bindHeld('backward', ['KeyS', 'ArrowDown']);
bindHeld('right', ['KeyD', 'ArrowRight']); bindHeld('left', ['KeyA', 'ArrowLeft']);
bindHeld('turnRight', ['NumpadMultiply']); bindHeld('turnLeft', ['NumpadDivide']);
bindHeld('primary', HOLDS.firePrimary); bindHeld('secondary', HOLDS.fireSecondary);
function heldBits(keys) {
  let bits = 0;
  for (let i = 0; i < keys.length; i++) bits |= heldKeys.get(keys[i]) || 0;
  return bits;
}

export function createClientPrediction(instance, collision) {
  const bridge = bridgeFor(instance), x = bridge.x;
  if (collision.rays?.x !== x) throw Error('prediction session requires shared collision');
  let handle = x.client_prediction_create(), body = null, source = null, kind = SOLDIER, sourceId = 0;
  let disposed = false, busy = false, dropAfter = false, active = false;
  let input = new Float64Array(4096), output = new Float64Array(4096), used = 0;
  let inputPtr = 0, inputCapacity = 0;
  let actorView, lastGun;
  // These belong to the prediction owner, rather than to a frame or command.
  // Neither an authority correction nor presentation may retain a scratch row.
  // Generated mirrors keep optional solver records separate from live bodies.
  // Reusing them must not publish class configuration back onto the world copy.
  const actorMirror = new SoldierMirror(), droidMirror = new DroidekaMirror();
  const frame = new ClientFrameMirror(), command = new ClientCommandMirror(), lookRow = new ClientLookMirror();
  const crowdRow = new ClientCrowdMirror(), mountRow = new ClientMountMirror();
  const result = new ClientResultMirror(), notice = new ClientShotMirror(), mirrors = [];
  const noticeGun = { index: 0, barrel: 0 };
  const mirrorGun = { index: 0, barrel: 0 };
  // Compact soldier and droideka formats have bounded optional tails. The
  // writer's returned count, not this capacity, determines the imported bytes.
  const correction = new Uint8Array(Math.max(COMPACT_SOLDIER_MAX_BYTES, DROIDEKA_STATE_MAX_BYTES)), correctionWriter = { bytes: correction, view: new DataView(correction.buffer), at: 0 };
  const live = () => { if (disposed) throw Error('prediction session is disposed'); };

  // Grow only at a high-water mark. Setup may move WASM memory; a warm frame
  // reuses both exchange allocations and refreshes views after each crossing.
  function room(n) {
    if (n <= input.length) return;
    let cap = input.length; while (cap < n) cap *= 2;
    const next = new Float64Array(cap); next.set(input); input = next;
  }
  function reserve(n) { const at = used; used += n; room(used); return at; }
  function run(op, words, count = words.length) {
    live();
    let ptr;
    if (op === PREDICT) {
      if (inputCapacity < count) {
        inputCapacity = input.length;
        inputPtr = x.client_prediction_room(handle, inputCapacity);
      }
      ptr = inputPtr;
    } else {
      ptr = x.client_prediction_room(handle, count);
      // The setup loader can relocate the same exchange vector. Its old
      // pointer must not survive until the next predict_session call.
      inputCapacity = 0;
    }
    bridge.views();
    for (let i = 0; i < count; i++) bridge.f[ptr / 8 + i] = words[i];
    // A frame calls the complete session export. All other operations are
    // cold configuration or explicit reset, never a per-method tick adapter.
    const published = op === PREDICT ? x.client_predict_session(handle, count) : 0;
    const ok = op === PREDICT ? published !== 0 : x.client_prediction_run(handle, op);
    bridge.views();
    if (!ok) throw Error(new TextDecoder().decode(new Uint8Array(x.memory.buffer, x.client_prediction_error(handle), x.client_prediction_error_len(handle))));
    let length, at;
    if (op === PREDICT) {
      at = published / 8;
      syncClientResult(null, bridge.f, at, 0, result);
      length = result.shotsAt + result.shotsCount * (SHOT + result.actorWords);
    } else {
      length = x.client_prediction_output_len(handle);
      at = x.client_prediction_output(handle) / 8;
    }
    // Copy before callbacks can grow module memory or start another owner.
    // This is one retained result block, not a view per replayed actor tick.
    if (output.length < length) output = new Float64Array(Math.max(length, output.length * 2));
    for (let i = 0; i < length; i++) output[i] = bridge.f[at + i];
  }

  // restorePrediction used the body's class and its arsenal as prerequisites.
  // A changed body/arsenal invalidates the continuation even if its unit ID is
  // reused. Clip trees and weapon definitions allocate only at this boundary.
  function configure(unit) {
    const s = unit.soldier, authority = unit.combat.arsenal;
    if (body === s && source === authority) return;
    body = s; source = authority; kind = s instanceof Droideka ? DROIDEKA : SOLDIER; sourceId++;
    run(CONFIGURE_ACTOR, predictionActorConfig(s, kind).words);
    actorView = Object.create(s);
    Object.defineProperties(actorView, kind === DROIDEKA ? {
      def: {value:null}, muzzleAnimator: {value:undefined}, waterClock: {value:undefined},
    } : { passThrough: {value:null}, mover: {value:-1}, dead: {value:undefined} });
    if (kind === DROIDEKA) {
      lastGun = { index: 0, barrel: 0 };
      const heap = new Heap(), words = new Array(CLIENT_MUZZLES_WORDS);
      putClientMuzzles(null, { barrels: s.guns.map((_, i) => s.gunBarrels?.[i]?.length || 1) }, words, 0, heap);
      run(CONFIGURE_MUZZLES, words.concat(heap.words));
    }
    for (const weapon of authority.weapons) {
      const words = [bridge.defs.weapon.id(weapon.levels[0])];
      packPredictionWeaponDefinition(words, weapon.levels[0]); run(CONFIGURE_WEAPON, words);
    }
  }

  // replayTicks consumes the command order supplied by InputHistory. Do not
  // sort or drop rolled-back ticks: unsent commands still carry action edges,
  // while Rust alone decides which held keys and look deltas become effective.
  function packCommands(pending, fireEdges) {
    const commandsAt = reserve(pending.length * COMMAND);
    for (let i = 0; i < pending.length; i++) {
      const c = pending[i], look = c.worldLook ?? c.look ?? EMPTY, lookAt = reserve(look.length * LOOK);
      for (let n = 0; n < look.length; n++) {
        lookRow.x = look[n][0]; lookRow.y = look[n][1];
        putClientLook(null, lookRow, input, lookAt + n * LOOK, null);
      }
      let actions = 0, primaryCount = 0, primaryId = 0, secondaryCount = 0, secondaryId = 0;
      for (let n = 0; n < c.actions.length; n++) {
        const bit = ClientAction_INDEX.get(c.actions[n]); if (bit !== undefined) actions |= 1 << bit;
      }
      // Preserve duplicate press counts. A single edge's ID seeds its cycle;
      // collapsing several presses would change shot identity and spread.
      for (let n = 0; n < fireEdges.length; n++) {
        const edge = fireEdges[n];
        if (edge.sequence !== c.sequence || edge.kind !== 'fire' || !edge.pressed) continue;
        if (edge.button === 'firePrimary') { primaryCount++; primaryId = edge.id; }
        if (edge.button === 'fireSecondary') { secondaryCount++; secondaryId = edge.id; }
      }
      // The same row is overwritten for each command. Optional look settings
      // must also be overwritten with undefined, so a previous command cannot
      // accidentally supply its mouse scale or inversion to a later one.
      command.tick = c.tick; command.generation = c.generation; command.sequence = c.sequence;
      command.sent = c.sent !== false; command.held = heldBits(c.down || EMPTY); command.actions = actions;
      command.lookAt = lookAt; command.lookCount = look.length;
      command.mouseScale = c.mouseScale; command.invert = c.invertMouse;
      command.primaryCount = primaryCount; command.primaryId = primaryId;
      command.secondaryCount = secondaryCount; command.secondaryId = secondaryId;
      putClientCommand(null, command, input, commandsAt + i * COMMAND, null);
    }
    return commandsAt;
  }

  // PredictionArsenal publication preserves the existing drawn Weapon objects.
  // Generated assignments list only mutable solver state; callbacks, content,
  // random generators and melee presentation retain their original owners.
  function publishOneWeapon(weapon, at, index) {
    // Mirror allocation is lazy only on first use of a weapon slot. Normal
    // frames reuse its scalar state and any already-present melee record.
    const m = syncWeapon(bridge, output, at, 0, mirrors[index] ||= new WeaponMirror());
    weapon.level = m.level; weapon.def = weapon.levels[m.level];
    publishWeapon(weapon, m);
    if (m.melee) weapon.melee.restoreState(m.melee);
    // Kept quirk: the salvo cursor is state, but this JS weapon must never
    // draw from it. Only Rust may advance the restored generator.
    weapon.salvoRandom = m.salvoSeed === null ? null : seededMarker;
    weapon.salvoSeed = m.salvoSeed; weapon.salvoCount = m.salvoCount;
  }
  function publishWeapons(unit, at) {
    const arsenal = unit.combat.arsenal, count = output[at + CLIENT_WEAPONS_OFFSETS.count];
    at += CLIENT_WEAPONS_WORDS;
    for (let i = 0; i < count; i++, at += WEAPON_WORDS) publishOneWeapon(arsenal.weapons[i], at, i);
    arsenal.selected[0] = output[at + CLIENT_ARSENAL_TAIL_OFFSETS.primary];
    arsenal.selected[1] = output[at + CLIENT_ARSENAL_TAIL_OFFSETS.secondary];
    unit.combat.zoomStep = output[at + CLIENT_ARSENAL_TAIL_OFFSETS.zoomStep];
  }

  // game.js replayPrediction publishes exact before/shot/final poses in turn.
  // Copy into the existing vectors so rigs and smoothing keep their references.
  // The body row also contains class/scratch fields which publication excludes.
  function publishActor(unit, at) {
    const s = unit.soldier;
    if (kind === DROIDEKA) {
      // An absent optional record clears the generated mirror. Prime its
      // storage before decoding so the next visual gun needs no new object.
      droidMirror.lastGun = mirrorGun;
      syncDroideka(rowContext, output, at, 0, droidMirror);
      publishDroideka(s, droidMirror);
      if (droidMirror.lastGun) {
        lastGun.index = droidMirror.lastGun.index; lastGun.barrel = droidMirror.lastGun.barrel;
        s.lastGun = lastGun;
      } else s.lastGun = undefined;
      if (s.shield) { s.shield.value = droidMirror.shield.value; s.shield.on = droidMirror.shield.on; }
    } else {
      syncSoldier(rowContext, output, at, 0, actorMirror);
      publishSoldier(s, actorMirror);
    }
  }
  function drop() { if (handle) { x.client_prediction_drop(handle); handle = 0; } }
  return {
    bridge, layout,
    // Test wiring reads already-retained rows. Merely exposing them performs
    // no work during play; instrumentation lives entirely in test fixtures.
    get frame() { return frame; }, get result() { return result; }, get inputWords() { return used; },
    get handle() { return handle; }, get disposed() { return disposed; },
    reset() { live(); if (active) { run(RESET, EMPTY); active = false; } },
    // An effect callback may dispose its battle during present(). Delay the
    // native drop until the result block's final pose has been republished.
    dispose() { if (disposed) return; disposed = true; if (busy) dropAfter = true; else drop(); },
    // game.js restores authority before calling predict. The session may
    // continue its private checkpoint instead, but page state is always packed
    // as an exact restore source when a newer correction invalidates it.
    predict(unit, pending, options = NO_VIEW) {
      live(); if (busy) throw Error('prediction session is busy'); busy = true;
      try {
        configure(unit); collision.rayBodies();
        const combat = unit.combat, s = unit.soldier, view = options.view ?? NO_VIEW;
        const actorWords = kind === DROIDEKA ? DROIDEKA_WORDS : SOLDIER_WORDS;
        used = FRAME;
        const actorAt = reserve(actorWords), flagsAt = reserve(collision.bodies.length);
        (kind === DROIDEKA ? putDroideka : putSoldier)(rowContext, actorView, input, actorAt, null);
        // Body order is the collision world's order. Publish enabled flags
        // without mutating that shared world during any private replay tick.
        for (let i = 0; i < collision.bodies.length; i++) input[flagsAt + i] = Number(collision.bodies[i].enabled);
        // restorePrediction's wire bytes remain exact, including settled and
        // dive flags. Writers reuse one maximum-size checkpoint and DataView.
        const correctionBytes = kind === DROIDEKA ? writeDroidekaState(s, correctionWriter) : writeCompactSoldier(s, correctionWriter);
        const correctionWords = Math.ceil(correctionBytes / 4), correctionAt = reserve(correctionWords);
        for (let i = 0; i < correctionWords; i++) {
          const at = i * 4;
          // The final word has zero padding even when the retained buffer
          // still contains bytes from a longer checkpoint on the last frame.
          let word = 0;
          for (let n = 0; n < 4 && at + n < correctionBytes; n++) word |= correction[at + n] << (n * 8);
          input[correctionAt + i] = word >>> 0;
        }
        // WeaponPrediction.checkpoint included selections and zoom after the
        // variable weapon rows. The generated tail describes both directions
        // of this existing layout, including its zero barrel count on foot.
        const weaponCount = combat.arsenal.weapons.length;
        const weaponsWords = CLIENT_WEAPONS_WORDS + weaponCount * WEAPON_WORDS + CLIENT_ARSENAL_TAIL_WORDS;
        const weaponsAt = reserve(weaponsWords);
        input[weaponsAt + CLIENT_WEAPONS_OFFSETS.count] = weaponCount;
        let at = weaponsAt + CLIENT_WEAPONS_WORDS;
        for (let i = 0; i < weaponCount; i++, at += WEAPON_WORDS) putWeapon(bridge, combat.arsenal.weapons[i], input, at, null);
        input[at + CLIENT_ARSENAL_TAIL_OFFSETS.primary] = combat.arsenal.selected[0];
        input[at + CLIENT_ARSENAL_TAIL_OFFSETS.secondary] = combat.arsenal.selected[1];
        input[at + CLIENT_ARSENAL_TAIL_OFFSETS.zoomStep] = combat.zoomStep;
        input[at + CLIENT_ARSENAL_TAIL_OFFSETS.barrelsCount] = 0;
        const commandsAt = packCommands(pending, options.fireEdges ?? EMPTY), crowd = options.crowd ?? EMPTY;
        const crowdAt = reserve(crowd.length * CROWD);
        // separatePredictedSoldier reads remote poses once per page frame.
        // Keep its standing, crouching, prone and walker dimensions exactly;
        // the owned body's dive/air shape is selected inside Rust per tick.
        for (let i = 0; i < crowd.length; i++) {
          const other = crowd[i]; crowdRow.position = other.position;
          if (other.walker) { crowdRow.radius = 1; crowdRow.height = 2; crowdRow.center = 1; }
          else if (other.hitPosture === 'crouch') { crowdRow.radius = .45; crowdRow.height = 1.45; crowdRow.center = .65; }
          else if (other.hitPosture === 'prone') { crowdRow.radius = .8; crowdRow.height = .45; crowdRow.center = .25; }
          else { crowdRow.radius = .57; crowdRow.height = 1.65; crowdRow.center = .9; }
          putClientCrowd(null, crowdRow, input, crowdAt + i * CROWD, null);
        }
        // One header covers the whole pending block. Epoch and source are
        // part of continuation identity; tick alone cannot distinguish a new
        // battle, respawn or a changed private arsenal on a reused unit.
        frame.mode = 'actor';
        frame.mountsAt = 0; frame.mountsCount = 0;
        frame.epoch = options.epoch; frame.identity = unit.id; frame.generation = unit.generation; frame.source = sourceId;
        frame.authorityTick = options.tick; frame.world = collision.rayWorld;
        frame.ignored = kind === DROIDEKA ? -1 : s.passThrough?.raySlot ?? -1;
        frame.movement = Boolean(collision.movement); frame.dead = false;
        frame.underwater = Boolean(combat.gunUnderwater); frame.shield = combat.arsenal.current(1)?.def.base === 'shield';
        frame.actorAt = actorAt; frame.actorWords = actorWords; frame.correctionAt = correctionAt; frame.correctionBytes = correctionBytes;
        frame.weaponsAt = weaponsAt; frame.weaponsWords = weaponsWords; frame.flagsAt = flagsAt; frame.flagsCount = collision.bodies.length;
        frame.commandsAt = commandsAt; frame.commandsCount = pending.length; frame.crowdAt = crowdAt; frame.crowdCount = crowd.length;
        frame.held = heldBits(options.held ?? EMPTY); frame.presentedThrough = options.presentedThrough ?? options.tick;
        // Admission and the already-presented horizon come from the page.
        // Rust can produce notices from them without consulting effects while
        // it replays an earlier command or a held trigger's repeat.
        frame.cosmeticSlots = options.cosmeticSlots ?? 0; frame.mouseScale = view.mouseScale ?? options.mouseScale ?? 1;
        frame.invert = view.invertMouse || false; frame.checkTick = options.checkTick ?? 0;
        frame.kick[0] = combat.kick.rate; frame.kick[1] = combat.kick.hold; frame.kick[2] = combat.kick.offset;
        putClientFrame(null, frame, input, 0, null);
        run(PREDICT, input, used); active = true;
        publishActor(unit, result.actorAt); publishWeapons(unit, result.weaponsAt);
        return result;
      } finally { busy = false; if (dropAfter) drop(); }
    },
    mounted(unit, entries, pending, tick, held) {
      live(); if (busy) throw Error('prediction session is busy'); busy = true;
      try {
        const lease = unit.seat;
        // MountedWeaponPrediction had an arsenal per lease. Seat changes must
        // reload definitions, even when the same unit remains in the vehicle.
        if (body !== lease) {
          body = lease; source = null; run(CONFIGURE_ACTOR, [SOLDIER]);
          for (let i = 0; i < entries.length; i++) {
            const w = entries[i].weapon, words = [bridge.defs.weapon.id(w.levels[0])];
            packPredictionWeaponDefinition(words, w.levels[0]); run(CONFIGURE_WEAPON, words);
          }
        }
        used = FRAME;
        const weaponsWords = CLIENT_WEAPONS_WORDS + entries.length * WEAPON_WORDS + CLIENT_ARSENAL_TAIL_WORDS + entries.length;
        const weaponsAt = reserve(weaponsWords);
        input[weaponsAt + CLIENT_WEAPONS_OFFSETS.count] = entries.length;
        let at = weaponsAt + CLIENT_WEAPONS_WORDS;
        for (let i = 0; i < entries.length; i++, at += WEAPON_WORDS) putWeapon(bridge, entries[i].weapon, input, at, null);
        input[at + CLIENT_ARSENAL_TAIL_OFFSETS.primary] = 0; input[at + CLIENT_ARSENAL_TAIL_OFFSETS.secondary] = 0;
        input[at + CLIENT_ARSENAL_TAIL_OFFSETS.zoomStep] = 0; input[at + CLIENT_ARSENAL_TAIL_OFFSETS.barrelsCount] = entries.length;
        at += CLIENT_ARSENAL_TAIL_WORDS;
        for (let i = 0; i < entries.length; i++) input[at + i] = entries[i].barrel;
        const mountsAt = reserve(entries.length * CLIENT_MOUNT_WORDS);
        for (let i = 0; i < entries.length; i++) {
          mountRow.advance = entries[i].advancesBarrel; mountRow.tow = entries[i].towHeld;
          putClientMount(null, mountRow, input, mountsAt + i * CLIENT_MOUNT_WORDS, null);
        }
        const commandsAt = packCommands(pending, EMPTY);
        // The shared header is reused when entering or leaving a seat. Clear
        // actor-only ranges so diagnostics and future validation see no stale
        // collision, crowd or checkpoint words from the preceding actor frame.
        frame.mode = 'mounted'; frame.identity = unit.id; frame.generation = unit.generation; frame.authorityTick = tick;
        frame.epoch = 0; frame.source = 0; frame.world = 0; frame.ignored = -1;
        frame.movement = false; frame.dead = false; frame.underwater = false; frame.shield = false;
        frame.actorAt = 0; frame.actorWords = 0; frame.correctionAt = 0; frame.correctionBytes = 0;
        frame.flagsAt = 0; frame.flagsCount = 0; frame.crowdAt = 0; frame.crowdCount = 0;
        frame.presentedThrough = 0; frame.cosmeticSlots = 0; frame.mouseScale = 0; frame.invert = false; frame.checkTick = 0;
        frame.weaponsAt = weaponsAt; frame.weaponsWords = weaponsWords; frame.mountsAt = mountsAt; frame.mountsCount = entries.length;
        frame.commandsAt = commandsAt; frame.commandsCount = pending.length; frame.held = heldBits(held);
        frame.kick[0] = frame.kick[1] = frame.kick[2] = 0;
        putClientFrame(null, frame, input, 0, null);
        run(PREDICT, input, used); active = false;
        // Publish directly to the mounted entries. They do not have actor
        // selections or zoom, so constructing a pretend combat/arsenal view
        // would allocate a second ownership structure for the same weapons.
        at = result.weaponsAt + CLIENT_WEAPONS_WORDS;
        for (let i = 0; i < entries.length; i++, at += WEAPON_WORDS) publishOneWeapon(entries[i].weapon, at, i);
        at += CLIENT_ARSENAL_TAIL_WORDS;
        // Kept MountedWeaponPrediction quirk: advance-enabled owners receive
        // their final barrel even on a tick with no shot. No temporary arsenal
        // or entries.map view is needed to publish these independent weapons.
        for (let i = 0; i < entries.length; i++, at += CLIENT_BARREL_RESULT_WORDS) {
          if (entries[i].advancesBarrel) entries[i].owner.barrel = output[at + CLIENT_BARREL_RESULT_OFFSETS.barrel];
        }
        return result.tick;
      } finally { busy = false; if (dropAfter) drop(); }
    },
    // Tests request an owned checkpoint copy. Normal frame publication reads
    // the retained output directly and does not allocate this byte array.
    correction() {
      const bytes = new Uint8Array(result.correctionBytes);
      for (let i = 0; i < bytes.length; i++) bytes[i] = output[result.correctionAt + (i >>> 2)] >>> ((i & 3) * 8);
      return bytes;
    },
    // Drawing consumes the retained result after the session has finished.
    // No before-step closure or copied kick object is needed: kick already
    // belongs to the result header and the private arsenal publication.
    present(unit, presentation, onShot) {
      live(); if (busy) throw Error('prediction session is busy'); busy = true;
      try {
        let before = false, at = result.shotsAt;
        // game.js replayPrediction showed each notice at its simulated pose,
        // with the notice's temporary weapon level. Restore that level even
        // when an effect rejects the shot or disposes the battle from a hook.
        for (let i = 0; i < result.shotsCount; i++) {
          if (result.ticks && i === result.beforeShot) {
            before = true; publishActor(unit, result.beforeAt); presentation?.beforeStep();
          }
          notice.visualGun = noticeGun;
          syncClientShot(null, output, at, 0, notice);
          publishActor(unit, at + notice.actorAt);
          const w = unit.combat.arsenal.weapons[notice.weapon], level = w.level, def = w.def;
          w.level = notice.level; w.def = w.levels[notice.level];
          try { onShot?.(notice, w); } finally { w.level = level; w.def = def; }
          at += SHOT + result.actorWords;
        }
        // The old presenter also advanced its interpolation endpoints when
        // no shot occurred. Keep the final look delta out of the movement yaw.
        if (result.ticks && !before) { publishActor(unit, result.beforeAt); presentation?.beforeStep(); }
        publishActor(unit, result.actorAt);
        if (result.ticks && presentation) { presentation.previousYaw += result.lookYaw; presentation.afterStep(); }
      } finally {
        try { publishActor(unit, result.actorAt); }
        finally { busy = false; if (dropAfter) drop(); }
      }
    },
  };
}
