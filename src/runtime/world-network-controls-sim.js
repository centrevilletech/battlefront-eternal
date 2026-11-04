import { blankSeatIntent } from '../seats.js';
import { mountInReach, seatScheme } from '../boarding.js';
import { ONLINE_PLAYOUT_MAX_TICKS } from './hit-history.js';
import { cycleNetworkSeat } from './network-seat.js';
import { networkVehicleInput } from './network-vehicle.js';

const squads = ['squadMoveOut', 'squadHold', 'squadFollow', 'squadAtEase'];
const labels = ['moveout', 'hold', 'follow', 'atease'];
const axes = ['forward', 'right', 'turn'];
const actions = ['jump', 'crouch', 'prone', 'use', 'reload', 'nextPrimary', 'previousPrimary', 'nextSecondary', 'zoom'];
const queues = ['mailbox', 'controls', 'fire', 'actions'];

function iterator(value) {
  const it = value[Symbol.iterator]();
  if (Object(it) !== it) throw new TypeError('Result of the Symbol.iterator method is not an object');
  return { it, next: it.next, body: false };
}
function next(frame) {
  frame.body = false;
  const result = Reflect.apply(frame.next, frame.it, []);
  if (Object(result) !== result) throw new TypeError(`Iterator result ${result} is not an object`);
  if (result.done) return { done: true };
  const value = result.value;
  frame.body = true;
  return { done: false, value };
}

// A native invocation retains only scalars. This frame owns every live host
// reference, including iterators and the captured controls/intent objects.
export function networkControlPorts(bridge) {
  const contexts = [];
  bridge.register({ worldControls(args) {
    // The bridge's argument mirror is reused by nested invocations.
    const { op, index, value, other } = args, context = contexts.at(-1);
    const { game, p, u, command, c, intent } = context;
    let number = 0, flag = false;
    switch (op) {
      case 'start': context.c = p.controls; context.intent = u.intent; break;
      case 'squadTake': flag = Boolean(c.take(squads[index])); break;
      case 'squadReady': flag = Boolean(game.ai?.playerCommand); break;
      case 'squad': game.ai.playerCommand(value, p.character); break;
      case 'squadEvent': game.simEvents.emit('presentation', { event: { kind: 'squad', ownerId: u.id, ownerGeneration: u.generation, label: labels[index] } }); break;
      case 'nextSpawn': flag = Boolean(c.take('nextSpawn')); break;
      case 'scheme': c.flightScheme = () => seatScheme(u.seat?.vehicle, command.flightSettings?.bindings); break;
      case 'firstPerson': flag = typeof command.firstPerson === 'boolean'; break;
      case 'combatView': u.combat.firstPerson = command.firstPerson; break;
      case 'remotes': flag = Boolean(u.combat.remotes); break;
      case 'remoteView': u.combat.remotes.firstPerson = command.firstPerson; break;
      case 'finiteAspect': flag = Number.isFinite(command.aspect); break;
      case 'positiveAspect': flag = command.aspect > 0; break;
      case 'aimCamera': flag = Boolean(u.combat.remotes.aimCamera); break;
      case 'aspect': u.combat.remotes.aimCamera.aspect = command.aspect; break;
      case 'axis': intent[axes[index]] = c.axis(axes[index]); break;
      case 'pitchZero': intent.pitch = 0; break;
      case 'action': intent[actions[index]] = c.take(actions[index]); break;
      case 'metadata':
        u.networkShot = null; u.networkShots = [];
        u.networkViewTick = p.inputsStale ? null : command.viewTick;
        u.networkInputLeadTicks = p.inputsStale ? null : (p.inputLeadTicks ?? null);
        u.networkPlayoutMaxTicks = ONLINE_PLAYOUT_MAX_TICKS;
        break;
      case 'reliable': flag = Boolean(p.reliableFire); break;
      case 'fire': context.fire = p.fire.take(value, u.generation, { tick: p.mailbox.stateTick, buttons: [c.down.has('Mouse0'), c.down.has('Mouse2')] }); break;
      case 'fireButtons': intent.buttons[0] = context.fire.buttons[0]; intent.buttons[1] = context.fire.buttons[1]; break;
      case 'shots': context.shots = iterator(context.fire.shots); break;
      case 'nextShot': {
        const result = next(context.shots);
        if (result.done) context.shots = null;
        else { context.shot = result.value; flag = true; }
        break;
      }
      case 'shotTick': number = +context.shot.tick; break;
      case 'shotLate': context.shot.late = value; break;
      case 'publishShots': u.networkShots = context.fire.shots; break;
      case 'held': intent.buttons[index] = c.held(index ? 'fireSecondary' : 'firePrimary') ? 1 : 0; break;
      case 'key':
        if (index === 0) intent.brake = c.down.has('Space');
        else if (index === 1) intent.up = c.down.has('Space') ? 1 : 0;
        else intent.down = c.down.has('ControlLeft') ? 1 : 0;
        break;
      case 'scale': number = +(command.mouseScale ?? game.settings.mouseScale); break;
      case 'looks': context.scale = index; context.looks = iterator(command.look); break;
      case 'nextLook': {
        const result = next(context.looks);
        if (result.done) context.looks = null;
        else {
          const [dx, dy] = result.value;
          context.dx = dx; context.dy = dy; flag = true;
        }
        break;
      }
      case 'dx': number = +context.dx; break;
      case 'dy': number = +context.dy; break;
      case 'remoteActive': flag = Boolean(u.combat.remotes?.active); break;
      case 'remoteLook': u.combat.remotes.look(value, other, index, command.invertMouse || false); break;
      case 'soldierLook': (u.soldier.lookRaw || u.soldier.look).call(u.soldier, value / u.combat.zoom, other / u.combat.zoom, index, command.invertMouse || false); break;
      case 'seat': flag = Boolean(u.seat); break;
      case 'vehicle': flag = Boolean(u.seat.vehicle); break;
      case 'selection': flag = Boolean(intent[index ? 'previousPrimary' : 'nextPrimary']); break;
      case 'cycle': cycleNetworkSeat(game.vehicles, u, value); break;
      case 'clearSelection': intent.nextPrimary = intent.previousPrimary = false; break;
      case 'vehicleInput': context.keys = networkVehicleInput(u.seat.vehicle, u.seat.index, c, command); break;
      case 'vehicleKeys': {
        const keys = context.keys;
        intent.flight = keys; intent.forward = keys.forward || 0; intent.right = keys.right || 0; intent.turn = keys.turn || 0; intent.brake = Boolean(keys.brake);
        break;
      }
      case 'turnStart': context.turn = intent.turn; break;
      case 'reduce': context.reduced = command.look.reduce((n, v) => n + v[index], 0) * 1; number = context.reduced; break;
      case 'turn': intent.turn = context.turn + value; break;
      case 'turnOther': intent.turn = context.turn + context.reduced * context.scale * 60 / 2048; break;
      case 'invert': flag = Boolean(command.invertMouse); break;
      case 'pitch': intent.pitch = value; break;
      case 'pitchOther': intent.pitch = -context.reduced * context.scale * 60 / 2048 * (command.invertMouse ? -1 : 1); break;
      case 'use': flag = Boolean(intent.use); break;
      case 'mount': context.mount = mountInReach(game, u, true); break;
      case 'mountVehicle': flag = Boolean(context.mount?.vehicle); break;
      case 'board': game.vehicles.board(u, context.mount.vehicle); break;
      case 'mountTurret': flag = Boolean(context.mount?.turret); break;
      case 'man': game.turretSeats?.man(u, context.mount.turret); break;
      case 'clearUse': intent.use = false; break;
      case 'clear': p[queues[index]].clear(); break;
      case 'unit': context.u = p.unit; flag = Boolean(context.u); break;
      case 'stopIntent': Object.assign(u.intent, blankSeatIntent(), { flight: {} }); break;
      case 'stopMetadata': u.networkShot = null; u.networkShots = []; u.networkViewTick = null; u.networkInputLeadTicks = null; u.networkPlayoutMaxTicks = null; break;
      case 'driver': flag = Boolean(u.seat?.vehicle && u.seat.index === 0); break;
      case 'stopSticks': u.seat.vehicle.stickX = u.seat.vehicle.stickY = 0; break;
      default: throw Error(`unknown network controls phase ${op}`);
    }
    return { value: number, flag };
  } });
  function run(context, stop, tick) {
    contexts.push(context);
    try { bridge.x.world_controls_run(stop, tick); }
    catch (error) {
      // Only a failure inside a for-of body closes its active iterator.
      // The original error wins even when return() itself throws.
      for (const frame of [context.looks, context.shots]) {
        if (frame?.body) { try { frame.it.return?.(); } catch {} }
      }
      throw error;
    } finally { contexts.pop(); }
  }
  return {
    networkControls(game, p, u, command, tick) { run({ game, p, u, command }, 0, tick); },
    networkStop(p) { run({ p }, 1, 0); },
  };
}
