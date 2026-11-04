// Network riders and prediction clones use the battle's existing module.
// Contexts contain the actual receivers; no authoritative Store row is synced.
import { personLease, seatControl } from '../boarding.js';
import { flightBindings } from '../flight/controls.js';
import { putNetworkSeatsCommand, NetworkSeatsReplyMirror, syncNetworkSeatsReply } from '../sim/gen/network-seats.js';

const ports = new WeakMap();
const names = ['throttle', 'brake', 'rollRight', 'rollLeft', 'boost', 'rollLock', 'takeoff', 'trick', 'immelmann', 'targetLock', 'forward', 'right', 'turn'];
const lookFields = ['stickX', 'stickY', 'lookYaw', 'lookPitch'];

// A generator retains the source iterator's next method and lets the engine
// perform IteratorClose on destructuring/body errors, with the original throw.
function* looks(command) {
  for (const [dx, dy] of command.look) yield [dx, dy];
}
function context(fields) {
  const objects = [], ids = new Map();
  const id = object => {
    let i = ids.get(object);
    if (i === undefined) { i = objects.length; ids.set(object, i); objects.push(object); }
    return i;
  };
  const pack = value => value === undefined ? [0, 0, 0] : value === null ? [1, 0, 0]
    : typeof value === 'boolean' ? [value ? 3 : 2, 0, Number(value)]
    : typeof value === 'number' ? [4, value, Number(Boolean(value))] : [5, id(value), Number(Boolean(value))];
  const unpack = (values, at = 0) => {
    switch (values[at]) {
      case 0: return undefined;
      case 1: return null;
      case 2: return false;
      case 3: return true;
      case 4: return values[at + 1];
      case 5: return objects[values[at + 1]];
      default: throw Error('invalid network input value');
    }
  };
  return { ...fields, objects, id, pack, unpack, failed: false, error: undefined, iterator: null };
}

function answer(f, op, a) {
  const { vehicle: v, index, controls: c, command, objects: o, id, pack, unpack } = f;
  switch (op) {
    case 'rider': {
      const lease = f.unit?.seat, hull = lease?.vehicle;
      if (!f.unit?.networkHuman || !hull?.alive || lease.released) return [0];
      return [1, id(hull), id(lease), Number(lease.index)];
    }
    case 'seatCount': {
      const hull = o[a[0]], n = hull?.seats?.length ?? 0;
      return [!hull?.alive || n < 2 ? 0 : n];
    }
    case 'direction': return [Number(f.direction)];
    case 'candidate': {
      const hull = o[a[0]], lease = a[1] < 0 ? null : o[a[1]], to = a[2];
      const occupant = hull.seats[to].lease;
      if (occupant && occupant !== lease && personLease(occupant)) return [1, occupant ? id(occupant) : -1];
      return [Number(Boolean(hull.driver && hull.seatIndex === to && !lease?.human)), occupant ? id(occupant) : -1];
    }
    case 'copyAim': {
      const hull = o[a[0]], from = hull.seats[a[1]], to = hull.seats[a[2]];
      to.aimYaw = from.aimYaw; to.pitch = from.pitch; to.lookYaw = from.lookYaw; to.lookPitch = from.lookPitch;
      return [];
    }
    case 'move': f.system.move(o[a[0]], a[1]); return [];
    case 'aim': {
      const hull = o[a[0]], to = a[1];
      if (to === 0) hull.restAim(0); else hull.clampAim(to);
      return [];
    }
    case 'clearSticks': { const hull = o[a[0]]; hull.stickX = hull.stickY = 0; return []; }
    case 'scales': {
      const fs = command.flightSettings || {};
      return [Number(command.mouseScale ?? 1), Number(Boolean(command.invertMouse)), Number(fs.sensitivity ?? 1), Number(Boolean(fs.invert))];
    }
    case 'nextLook': {
      if (!f.iterator) {
        f.iterator = looks(command); f.next = f.iterator.next; f.raise = f.iterator.throw;
      }
      const next = Reflect.apply(f.next, f.iterator, []);
      return next.done ? [0] : [1, Number(next.value[0]), Number(next.value[1])];
    }
    case 'control': return [{ flight: 1, drive: 2, turret: 3 }[seatControl(v, index)] ?? 0];
    case 'lookRead': {
      const field = a[0];
      if (field < 2) return [Number(v[lookFields[field]])];
      const seat = v.seats[index];
      return [Number(field === 2 ? (seat.lookYaw ?? seat.aimYaw) : (seat.lookPitch ?? seat.pitch))];
    }
    case 'lookWrite': {
      const field = a[0];
      (field < 2 ? v : v.seats[index])[lookFields[field]] = a[1]; return [];
    }
    case 'clamp': v.clampAim(index); return [];
    case 'copyLook': {
      const seat = v.seats[index]; seat.aimYaw = seat.lookYaw ?? seat.aimYaw; seat.pitch = seat.lookPitch ?? seat.pitch; return [];
    }
    case 'flight': {
      f.fs = command.flightSettings || {};
      f.bindings = flightBindings(f.fs.bindings);
      f.pad = command.gamepad || {}; f.axes = f.pad.axes || [];
      return [];
    }
    case 'binding': {
      const binding = f.bindings[names[a[0]]] || [];
      return [id(binding), binding.length];
    }
    case 'bindingCode': {
      const binding = o[a[0]], i = a[1];
      return i in binding ? [1, ...pack(binding[i])] : [0];
    }
    case 'down': return [Number(Boolean(c.down.has(unpack(a))))];
    case 'take': return pack(c.take(names[a[0]]));
    case 'axis': return [Number(f.axes[a[0]])];
    case 'button': return [Number(Boolean(f.pad.buttons?.[a[0]]?.pressed))];
    case 'curve': return pack(f.fs.curve);
    case 'driveAxis': return pack(c.axis(names[a[0]]));
    case 'space': return pack(c.down.has('Space'));
    default: throw Error(`unknown network seat operation ${op}`);
  }
}

function inputResult(f, values) {
  let at = 1;
  const scalar = () => { const result = f.unpack(values, at); at += 3; return result; };
  if (values[0] === 0) return { forward: scalar(), right: scalar(), turn: scalar(), brake: scalar() };
  const result = { forward: values[at++], right: values[at++] };
  if (values[at++]) { result.stickX = values[at++]; result.stickY = values[at++]; }
  result.boost = Boolean(values[at++]); result.rollLock = Boolean(values[at++]);
  for (const name of ['takeoff', 'trick', 'immelmann', 'targetLock', 'responseCurve']) result[name] = scalar();
  result.steeringAssist = false;
  return result;
}

export function networkSeatsPort(bridge) {
  let port = ports.get(bridge);
  if (port) return port;
  const x = bridge.x, frames = new Map(); let serial = 0;
  const input = { words: x.network_seats_words(0), put: putNetworkSeatsCommand };
  const output = { Mirror: NetworkSeatsReplyMirror, sync: syncNetworkSeatsReply };
  bridge.register({ networkSeatsWorld(record) {
    // SimBridge reuses this record and its list during nested imports.
    const frame = frames.get(record.context), op = record.op, args = [...record.values];
    if (!frame || frame.failed) return { ok: false, values: [] };
    try { return { ok: true, values: answer(frame, op, args) }; }
    catch (error) { frame.failed = true; frame.error = error; return { ok: false, values: [] }; }
  } });
  function run(cycle, fields) {
    const f = context(fields), token = ++serial; frames.set(token, f);
    try {
      bridge.callArgs(input, { context: token, cycle });
      const reply = bridge.callResult(output, x.network_seats_run());
      if (!reply.ok) throw f.error;
      return cycle ? Boolean(reply.values[0]) : inputResult(f, reply.values);
    } catch (error) {
      if (f.iterator) try { Reflect.apply(f.raise, f.iterator, [error]); } catch {}
      throw error;
    } finally { frames.delete(token); }
  }
  port = {
    bridge,
    cycle(system, unit, direction = 1) { return run(true, { system, unit, direction }); },
    input(vehicle, index, controls, command) { return run(false, { vehicle, index, controls, command }); },
  };
  ports.set(bridge, port);
  return port;
}

export function attachNetworkSeatsSim(system, bridge) {
  return system.networkSeatsSim = networkSeatsPort(bridge);
}

export function networkSeatsFor(vehicle) {
  const bridge = vehicle?.nativeSession?.bridge;
  return bridge ? networkSeatsPort(bridge) : vehicle?.system?.networkSeatsSim;
}
