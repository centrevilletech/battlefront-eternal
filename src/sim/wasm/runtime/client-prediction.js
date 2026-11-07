import {BodyAimBatch} from '../../../presentation/body.js';
// game.js replayPrediction and restorePrediction now cross once per frame.
// Rust owns every pending tick; these retained rows only import page facts and
// publish the result. Input clocks and effect callbacks keep their page lifetime.
import { bridgeFor, Heap } from '../../runtime.js';
import { ClientOp_INDEX, ClientActorKind_INDEX, ClientHeld_INDEX, ClientAction_INDEX,
  putClientFrame, ClientFrameMirror, putClientCommand, ClientCommandMirror, putClientLook, ClientLookMirror,
  putClientCrowd, ClientCrowdMirror, putClientMuzzles, putClientMount, ClientMountMirror,
  syncClientResult, ClientResultMirror, syncClientShot, ClientShotMirror,
  CLIENT_MUZZLES_WORDS, CLIENT_FRAME_WORDS, CLIENT_COMMAND_WORDS, CLIENT_LOOK_WORDS,
  CLIENT_CROWD_WORDS, CLIENT_MOUNT_WORDS, CLIENT_SHOT_WORDS, CLIENT_WEAPONS_WORDS, CLIENT_COMMAND_OFFSETS, CLIENT_RESULT_WORDS,
  CLIENT_HULL_WORDS, CLIENT_HULL_STATE_WORDS, CLIENT_HULL_SEAT_WORDS, CLIENT_HULL_MEMBER_WORDS,
  CLIENT_WEAPONS_OFFSETS, CLIENT_ARSENAL_TAIL_WORDS, CLIENT_ARSENAL_TAIL_OFFSETS,
  CLIENT_BARREL_RESULT_WORDS, CLIENT_BARREL_RESULT_OFFSETS } from '../../gen/client.js';
import { putWeapon, syncWeapon, publishWeapon, WeaponMirror, WEAPON_WORDS } from '../../gen/combat.js';
import { predictionActorConfig, packPredictionWeaponDefinition } from '../../../runtime/client-content.js';
import { putSoldier, putDroideka, syncSoldier, syncDroideka, publishSoldier, publishDroideka,
  SoldierMirror, DroidekaMirror, SOLDIER_WORDS, DROIDEKA_WORDS } from '../../gen/units.js';
import { writeCompactSoldier, COMPACT_SOLDIER_MAX_BYTES } from '../../../runtime/network-entity-soldier-compact.js';
import { writeDroidekaState, DROIDEKA_STATE_MAX_BYTES } from '../../../runtime/network-droideka-state.js';
import { Droideka } from '../../../droideka.js';
import { ClientHull } from '../../../runtime/client-hull.js';
import { FLIGHT_BINDINGS } from '../../../flight/controls.js';
import { ClientFlightHeld_INDEX, ClientFlightAction_INDEX, ClientFlightCommandMirror, putClientFlightCommand, CLIENT_FLIGHT_COMMAND_WORDS } from '../../gen/client.js';
import { HOLDS } from '../../../input.js';
import { ClientRecon } from '../../../runtime/client-recon.js';
import {NetLimits} from '../../../runtime/network-layout.js';
import { CLIENT_RECON_STATE_WORDS } from '../../gen/client.js';

// The operation discriminants are shared with client_prediction_run. Resolve
// their names at module load; a frame never interprets an unlabelled number.
const CONFIGURE_ACTOR = ClientOp_INDEX.get('configureActor'), CONFIGURE_WEAPON = ClientOp_INDEX.get('configureWeapon');
const CONFIGURE_HULL = ClientOp_INDEX.get('configureHull'), CONFIGURE_RECON=ClientOp_INDEX.get('configureRecon');
const PREDICT = ClientOp_INDEX.get('predict'), RESET = ClientOp_INDEX.get('reset'), CONFIGURE_MUZZLES = ClientOp_INDEX.get('configureMuzzles');
const SOLDIER = ClientActorKind_INDEX.get('soldier'), DROIDEKA = ClientActorKind_INDEX.get('droideka');
const flightHeldNames = Object.freeze([...ClientFlightHeld_INDEX.keys()]);
const FRAME = CLIENT_FRAME_WORDS, COMMAND = CLIENT_COMMAND_WORDS, LOOK = CLIENT_LOOK_WORDS;
const CROWD = CLIENT_CROWD_WORDS, SHOT = CLIENT_SHOT_WORDS, EMPTY = Object.freeze([]), NO_VIEW = Object.freeze({});
const rowContext = { defs: { unitClass: { id: () => 0, at: () => null }, weapon: { id: () => 0, at: () => null } } };
const seededMarker = () => { throw Error('prediction weapons are a presentation view'); };
// These sizes describe existing rows; diagnostic tests read them on demand.
// The shipped frame path has no counter, transfer tally or diagnostic branch.
const layout = Object.freeze({ frame: FRAME, command: COMMAND, look: LOOK, crowd: CROWD,
  mount: CLIENT_MOUNT_WORDS, shot: SHOT, correctionBytesPerWord: 4, result: CLIENT_RESULT_WORDS,
  commandOffsets: CLIENT_COMMAND_OFFSETS, flightCommand: CLIENT_FLIGHT_COMMAND_WORDS,
  hull: CLIENT_HULL_WORDS, hullState: CLIENT_HULL_STATE_WORDS, hullSeat: CLIENT_HULL_SEAT_WORDS, hullMember: CLIENT_HULL_MEMBER_WORDS });

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
bindHeld('space', ['Space']);
bindHeld('primary', HOLDS.firePrimary); bindHeld('secondary', HOLDS.fireSecondary);
function heldBits(keys) {
  let bits = 0;
  for (let i = 0; i < keys.length; i++) bits |= heldKeys.get(keys[i]) || 0;
  return bits;
}

export function createClientPrediction(instance, collision) {
  const bridge = bridgeFor(instance), x = bridge.x;
  const hull = new ClientHull(bridge, collision), recon=new ClientRecon(bridge);
  if (collision.rays?.x !== x) throw Error('prediction session requires shared collision');
  let handle = x.client_prediction_create(), body = null, source = null, kind = SOLDIER, sourceId = 0;
  let disposed = false, busy = false, dropAfter = false, active = false;
  let input = new Float64Array(NetLimits.EXCHANGE), output = new Float64Array(4096), used = 0;
  let inputPtr = 0, inputCapacity = 0;
  let outputBuffer = null, outputPointer = 0, outputViews = [];
  let actorView, lastGun;
  // These belong to the prediction owner, rather than to a frame or command.
  // Neither an authority correction nor presentation may retain a scratch row.
  // Generated mirrors keep optional solver records separate from live bodies.
  // Reusing them must not publish class configuration back onto the world copy.
  const actorMirror = new SoldierMirror(), droidMirror = new DroidekaMirror();
  const frame = new ClientFrameMirror(), command = new ClientCommandMirror(), lookRow = new ClientLookMirror();
  const crowdRow = new ClientCrowdMirror(), mountRow = new ClientMountMirror();
  const flightRow = new ClientFlightCommandMirror();
  const result = new ClientResultMirror(), notice = new ClientShotMirror(), mirrors = [];
  const mountedWeapons = [];
  const shotAims=new BodyAimBatch(collision);
  notice.aimYaw=NaN;notice.aimPitch=NaN;
  let replayActions=null,replayOutcomes=null,replayHeld=EMPTY,packedCount=0,packedAt=0,packingFlight=false,packedFireEdges=EMPTY,packedCapacity=0,outcomeCount=0;
  let flightSettings=NO_VIEW,gamepad=NO_VIEW;
  const replayRow=new ClientCommandMirror();
  // Reserve the occupied span, not the session's 512-command capacity. This
  // keeps the coarse copy proportional to pending input after an ordinary ACK.
  const countOutcome=item=>{if(item.edge.kind==='action'||item.edge.kind==='fire'&&item.edge.pressed)outcomeCount++;};
  // Reliable outcomes are event-owned Map rows. forEach reuses this callback;
  // no iterator, temporary action Map or command clone belongs to a frame.
  const packOutcome=item=>{
    const edge=item.edge,fire=edge.kind==='fire'&&edge.pressed;
    if(edge.kind!=='action'&&!fire||packedCount===packedCapacity)return;
    replayRow.source=fire?'fire':'outcome';replayRow.tick=edge.tick;replayRow.generation=edge.generation;replayRow.sequence=item.sequence;
    replayRow.sent=true;replayRow.held=0;replayRow.lookAt=0;replayRow.lookCount=0;replayRow.flightAt=0;
    replayRow.mouseScale=replayRow.aspect=0;replayRow.hasMouseScale=replayRow.hasAspect=false;
    replayRow.invert=replayRow.firstPerson=undefined;
    const name=fire?edge.button:edge.action,bit=ClientAction_INDEX.get(name),flightBit=ClientFlightAction_INDEX.get(name);
    replayRow.actions=bit===undefined?0:1<<bit;replayRow.flightActions=flightBit===undefined?0:1<<flightBit;
    if(packingFlight)replayRow.flightAt=packFlight(NO_VIEW,replayHeld);
    replayRow.primaryCount=replayRow.secondaryCount=0;replayRow.primaryId=replayRow.secondaryId=0;
    for(let i=0;i<packedFireEdges.length;i++){const press=packedFireEdges[i];
      if(press.sequence!==item.sequence||press.kind!=='fire'||!press.pressed)continue;
      if(press.button==='firePrimary'){replayRow.primaryCount++;replayRow.primaryId=press.id;}
      if(press.button==='fireSecondary'){replayRow.secondaryCount++;replayRow.secondaryId=press.id;}
    }
    putClientCommand(null,replayRow,input,packedAt+packedCount++*COMMAND,null);
  };
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
    const destination = bridge.f, start = ptr / Float64Array.BYTES_PER_ELEMENT;
    for (let i = 0; i < count; i++) destination[start + i] = words[i];
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
    // Cache each exact output span at its high-water mark. Copying through
    // TypedArray.set avoids boxing every float in a generic indexed loop.
    if (outputBuffer !== x.memory.buffer || outputPointer !== at) {
      outputBuffer = x.memory.buffer; outputPointer = at; outputViews = [];
    }
    let view = outputViews[length];
    if (!view) view = outputViews[length] = new Float64Array(outputBuffer, at * Float64Array.BYTES_PER_ELEMENT, length);
    output.set(view);
    // Configuration reserves the complete exchange before a mutable frame.
    // A changed class can relocate it; ordinary replay never asks for room.
    if(op!==PREDICT){inputPtr=x.client_prediction_room(handle,input.length);inputCapacity=input.length;}
  }

  // restorePrediction used the body's class and its arsenal as prerequisites.
  // A changed body/arsenal invalidates the continuation even if its unit ID is
  // reused. Clip trees and weapon definitions allocate only at this boundary.
  function configure(unit) {
    const s = unit.soldier, authority = unit.combat.arsenal;
    if (body === s && source === authority) return;
    hull.invalidate();
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

  // ClientPrediction consumes the command order supplied by InputHistory. Do not
  // sort or drop rolled-back ticks: unsent commands still carry action edges,
  // while Rust alone decides which held keys and look deltas become effective.
  function packFlight(c,down){
        // Decode key names once at the boundary. Rust consumes bit groups and
        // fixed pad values without allocating Sets or visiting binding strings.
        if (c.sent !== false) {
          if (c.flightSettings !== undefined) flightSettings = c.flightSettings || NO_VIEW;
          if (c.gamepad !== undefined) gamepad = c.gamepad || NO_VIEW;
        }
        flightRow.held = 0; flightRow.defaultHeld = 0; flightRow.actions = 0;
        for (let n = 0; n < flightHeldNames.length; n++) {
          const name = flightHeldNames[n], defaults = FLIGHT_BINDINGS[name], codes = flightSettings.bindings?.[name] || defaults;
          for (let k = 0; k < defaults.length; k++) if (down?.includes(defaults[k])) { flightRow.defaultHeld |= 1 << n; break; }
          for (let k = 0; k < codes.length; k++) if (down?.includes(codes[k])) { flightRow.held |= 1 << n; break; }
        }
        for (let n = 0; n < (c.actions||EMPTY).length; n++) {
          const bit = ClientFlightAction_INDEX.get(c.actions[n]); if (bit !== undefined) flightRow.actions |= 1 << bit;
        }
        flightRow.scale = flightSettings.sensitivity ?? 1; flightRow.invert = Boolean(flightSettings.invert);
        flightRow.curve = flightSettings.curve ?? NaN;
        for (let n = 0; n < 4; n++) flightRow.padAxes[n] = gamepad.axes?.[n] ?? 0;
        flightRow.padButtons = Number(Boolean(gamepad.buttons?.[0]?.pressed)) | (Number(Boolean(gamepad.buttons?.[11]?.pressed)) << 1);
    const at=reserve(CLIENT_FLIGHT_COMMAND_WORDS);putClientFlightCommand(null,flightRow,input,at,null);return at;
  }

  function packCommands(pending, fireEdges, flight = false) {
    flightSettings=NO_VIEW;gamepad=NO_VIEW;packingFlight=flight;packedFireEdges=fireEdges;
    const merging=replayOutcomes!==null,extra=merging?replayActions.length:0;
    const ordinary=Math.min(NetLimits.PREDICTION_COMMANDS,pending.length+extra);
    outcomeCount=0;if(merging)replayOutcomes.forEach(countOutcome);
    const capacity=Math.min(NetLimits.PREDICTION_COMMANDS,ordinary+outcomeCount);packedCapacity=capacity;
    const commandsAt = reserve(capacity * COMMAND);packedAt=commandsAt;packedCount=ordinary;
    frame.mergeCommands=merging;
    let lookWords=0;
    for (let i = 0; i < ordinary; i++) {
      const c = i<pending.length?pending[i]:replayActions[i-pending.length], look = i<pending.length?(c.worldLook??c.look??EMPTY):EMPTY;
      const lookCount=Math.min(look.length,NetLimits.PREDICTION_LOOKS-lookWords);lookWords+=lookCount;
      const lookAt=reserve(lookCount*LOOK);
      for (let n = 0; n < lookCount; n++) {
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
      command.source=i<pending.length?'movement':'action';command.flightActions=0;
      command.tick = c.tick; command.generation = c.generation; command.sequence = c.sequence;
      command.sent = c.sent !== false; command.held = heldBits(c.down || EMPTY); command.actions = actions;
      command.lookAt = lookAt; command.lookCount = lookCount;
      command.mouseScale=c.mouseScale??0;command.hasMouseScale=c.mouseScale!=null;command.invert = c.invertMouse;
      command.flightAt = 0;command.firstPerson=c.firstPerson;command.aspect=c.aspect??0;command.hasAspect=c.aspect!=null;
      if(flight){command.flightAt=packFlight(c,i<pending.length?c.down:replayHeld);command.flightActions=flightRow.actions;}
      command.primaryCount = primaryCount; command.primaryId = primaryId;
      command.secondaryCount = secondaryCount; command.secondaryId = secondaryId;
      putClientCommand(null, command, input, commandsAt + i * COMMAND, null);
    }
    if(merging)replayOutcomes.forEach(packOutcome);
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
    restoreRecon(){recon.restore();},
    // predictRemote now submits its entire command window to F3's native
    // core. The source and its independent RNG remain a retained checkpoint.
    replaySources(actions,outcomes,held){replayActions=actions;replayOutcomes=outcomes;replayHeld=held;},
    predictRecon(context,droid,pending,tick,generation,held=EMPTY,view=NO_VIEW){
      live();if(busy)throw Error('prediction session is busy');busy=true;
      try{
        const configuration=recon.configure(context,droid);if(configuration)run(CONFIGURE_RECON,configuration);
        collision.rayBodies();used=FRAME;
        frame.reconAt=reserve(CLIENT_RECON_STATE_WORDS);recon.capture(input,frame.reconAt,view);
        frame.weaponsWords=CLIENT_WEAPONS_WORDS+Math.min(32,droid.arsenal.weapons.length)*WEAPON_WORDS+CLIENT_ARSENAL_TAIL_WORDS;
        frame.weaponsAt=reserve(frame.weaponsWords);recon.weapons(input,frame.weaponsAt);
        frame.commandsAt=packCommands(pending,EMPTY);frame.commandsCount=packedCount;
        frame.flagsAt=reserve(collision.bodies.length);frame.flagsCount=collision.bodies.length;
        for(let i=0;i<collision.bodies.length;i++)input[frame.flagsAt+i]=Number(collision.bodies[i].enabled);
        frame.mode='recon';frame.generation=generation;frame.authorityTick=tick;frame.world=collision.rayWorld;
        frame.held=heldBits(held);frame.mouseScale=view.mouseScale??1;frame.invert=view.invertMouse??false;
        putClientFrame(null,frame,input,0,null);run(PREDICT,input,used);
        recon.acceptAuthority(output,result.reconSourceAt);
        recon.publish(output,result.reconAt,result.weaponsAt);recon.active=true;
        return result.tick;
      }finally{busy=false;if(dropAfter)drop();}
    },
    // Test wiring reads already-retained rows. Merely exposing them performs
    // no work during play; instrumentation lives entirely in test fixtures.
    get vehicle() { return hull.view; }, get vehicleOldPosition() { return hull.oldPosition; },
    get frame() { return frame; }, get result() { return result; }, get inputWords() { return used; },
    get input() { return input; },
    get hullFrame() { return hull.frame; },
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
        frame.mode = 'actor'; frame.hullAt = 0;
        frame.mountsAt = 0; frame.mountsCount = 0;
        frame.epoch = options.epoch; frame.identity = unit.id; frame.generation = unit.generation; frame.source = sourceId;
        frame.authorityTick = options.tick; frame.world = collision.rayWorld;
        frame.ignored = kind === DROIDEKA ? -1 : s.passThrough?.raySlot ?? -1;
        frame.movement = Boolean(collision.movement); frame.dead = false;
        frame.underwater = Boolean(combat.gunUnderwater); frame.shield = combat.arsenal.current(1)?.def.base === 'shield';
        frame.actorAt = actorAt; frame.actorWords = actorWords; frame.correctionAt = correctionAt; frame.correctionBytes = correctionBytes;
        frame.weaponsAt = weaponsAt; frame.weaponsWords = weaponsWords; frame.flagsAt = flagsAt; frame.flagsCount = collision.bodies.length;
        frame.commandsAt = commandsAt; frame.commandsCount = packedCount; frame.crowdAt = crowdAt; frame.crowdCount = crowd.length;
        frame.held = heldBits(options.held ?? EMPTY); frame.flightHeld = 0; frame.presentedThrough = options.presentedThrough ?? options.tick;
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
    mounted(unit, entries, pending, tick, held, vehicle = null, epoch = 0, correction = null, correctionSchema = 0) {
      live(); if (busy) throw Error('prediction session is busy'); busy = true;
      try {
        const lease = unit.seat;
        // MountedWeaponPrediction had an arsenal per lease. Seat changes must
        // reload definitions, even when the same unit remains in the vehicle.
        if (body !== lease) {
          body = lease; source = null; hull.invalidate(); run(CONFIGURE_ACTOR, [SOLDIER]);
          mountedWeapons.length = 0;
        }
        // A flyer can start replay while takeoff freezes all weapon timers.
        // Resolve its weapons on first admission without resetting hull motion
        // when that same lease later reaches Flying.
        for (let i = 0; i < entries.length; i++) {
          const w = entries[i].weapon;
          if (mountedWeapons[i] === w) continue;
          const words = [bridge.defs.weapon.id(w.levels[0])];
          packPredictionWeaponDefinition(words, w.levels[0]); run(CONFIGURE_WEAPON, words);
          mountedWeapons[i] = w;
        }
        // Configure only at a hull/life transition. There is no eligibility
        // crossing in the pending-tick loop; the frame always owns the decision.
        if (vehicle && hull.configure(vehicle)) {
          // Late replicas/variants must register their collider before the
          // private owner copies immutable collider and gather-cache handles.
          run(CONFIGURE_HULL, hull.setup);
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
        const commandsAt = packCommands(pending, EMPTY, Boolean(vehicle?.flying));
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
        frame.commandsAt = commandsAt; frame.commandsCount = packedCount; frame.held = heldBits(held);
        frame.flightHeld = 0;
        if (vehicle?.flying) for (let i = 0; i < flightHeldNames.length; i++) {
          const codes = FLIGHT_BINDINGS[flightHeldNames[i]];
          for (let j = 0; j < codes.length; j++) if (held.includes(codes[j])) { frame.flightHeld |= 1 << i; break; }
        }
        frame.kick[0] = frame.kick[1] = frame.kick[2] = 0;
        frame.hullAt = 0;
        if (vehicle) {
          if (!(correction instanceof Uint8Array)) throw Error('hull prediction requires correction bytes');
          hull.frame.correctionSchema = correctionSchema;
          const count = hull.prepare(), at = reserve(count);
          hull.pack(input, at);
          frame.correctionBytes = correction.length; frame.correctionAt = reserve(Math.ceil(correction.length / 4));
          for (let i = 0; i < correction.length; i += 4) {
            input[frame.correctionAt + (i >>> 2)] = (correction[i] | ((correction[i + 1] || 0) << 8)
              | ((correction[i + 2] || 0) << 16) | ((correction[i + 3] || 0) << 24)) >>> 0;
          } frame.hullAt = at; frame.mode = 'hull'; frame.epoch = epoch;
          frame.source = vehicle.aiTarget.generation; frame.world = collision.rayWorld;
          frame.flagsAt = reserve(collision.bodies.length); frame.flagsCount = collision.bodies.length;
          for (let i = 0; i < collision.bodies.length; i++) input[frame.flagsAt + i] = Number(collision.bodies[i].enabled);
        }
        putClientFrame(null, frame, input, 0, null);
        run(PREDICT, input, used); active = Boolean(vehicle);
        if (vehicle) hull.publish(output, result);
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
        // The notice actors already contain every firing pose. Prepare the
        // complete aim block before callbacks can mutate a shell or grow WASM.
        const aimed=Math.min(result.shotsCount,shotAims.capacity);
        let aimAt=result.shotsAt;
        for(let i=0;i<aimed;i++){
          syncClientShot(null,output,aimAt,0,notice);
          const w=unit.combat.arsenal.weapons[notice.weapon],level=w.level,def=w.def;
          w.level=notice.level;w.def=w.levels[notice.level];
          try{
          if(kind===SOLDIER){syncSoldier(null,output,aimAt+notice.actorAt,0,actorMirror);shotAims.write(i,actorMirror,unit.combat);}
          else{syncDroideka(rowContext,output,aimAt+notice.actorAt,0,droidMirror);shotAims.write(i,droidMirror,unit.combat);}
          }finally{w.level=level;w.def=def;}
          aimAt+=SHOT+result.actorWords;
        }
        if(aimed){shotAims.run(result.shotsCount);for(let i=0;i<aimed;i++)shotAims.answer(i,unit.combat);shotAims.run(aimed,true);}
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
          if(i<aimed)shotAims.publish(i,unit.combat,notice);
          try { if(i<aimed)onShot?.(notice, w); } finally { w.level = level; w.def = def; }
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
