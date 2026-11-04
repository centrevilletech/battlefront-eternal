import { Vector3 } from 'three';
import { Weapon, Arsenal } from '../../src/weapons.js';
import { prepareMelee } from '../../src/heroes.js';
import { RemoteDroids } from '../../src/remote.js';
import { reconRandom } from '../../src/runtime/network-remote-world.js';
import { captureRemoteReplay, restoreRemoteReplay } from '../../src/runtime/network-remote-replay.js';
import { encodeRemoteCorrection, decodeRemoteCorrection } from '../../src/runtime/network-remote-codec.js';
import { captureArsenalWeapon, validateArsenalWeapon, applyArsenalWeapon } from '../../src/runtime/network-arsenal-state.js';
import { WEAPON_STATE_NUMBERS, WEAPON_STATE_FLAGS } from '../../src/runtime/network-weapon-state.js';
import { framedCorrection } from './prediction-correction-cases.js';

export const readerDefinition = (kind = 'gun') => ({ class: `reader-${kind}`, base: kind === 'melee' ? 'melee' : 'cannon', channel: 0,
  RoundsPerClip: 10, ammo: 4, ShotDelay: .125, ReloadTime: .25, SpreadThreshold: .0625, ordnance: {},
  swings: [{ duration: 1, hit: .2 }, { duration: 2, hit: .3 }, { duration: 3, hit: .4 }],
  charges: [{ at: .5, ShotsPerSalvo: 2, swings: [{ duration: 4, hit: .5 }] }] });
export function createReaderWeapon(kind = 'gun', definition = readerDefinition(kind)) {
  const weapon = new Weapon(definition);
  if (kind === 'melee') prepareMelee({ weapons: [weapon] });
  Object.assign(weapon, { salvoSeed: 123, salvoCount: 7, salvoRandom: { retained: true }, random: () => .25 });
  const ids = new Map([[weapon, 'weapon'], [weapon.melee, 'melee']]);
  weapon.levels.forEach((d, i) => { ids.set(d, `level${i}`); d.swings?.forEach((s, n) => ids.set(s, `level${i}.swing${n}`)); });
  ids.delete(null); return { weapon, definition, levels: weapon.levels, timeline: weapon.melee, ids };
}

export function deltaBytes({ level = 0, flags = 0, numbers = {}, melee = null, mask = null } = {}) {
  const indices = WEAPON_STATE_NUMBERS.map((key, i) => Object.hasOwn(numbers, key) ? i : -1).filter(i => i >= 0);
  const bytes = new Uint8Array(6 + indices.length * 8 + (melee ? 10 : 0)), view = new DataView(bytes.buffer);
  bytes[0] = level; bytes[1] = flags; view.setUint32(2, mask ?? indices.reduce((m, i) => m | 1 << i, 0), true);
  let at = 6;
  for (const i of indices) { view.setFloat64(at, numbers[WEAPON_STATE_NUMBERS[i]], true); at += 8; }
  if (melee) { view.setFloat64(at, melee.time, true); bytes[at + 8] = melee.index; bytes[at + 9] = melee.active; }
  return bytes;
}
export function observeStateReader(value, ids = new Map()) {
  if (value && ids.has(value)) return { identity: ids.get(value) };
  if (value instanceof Uint8Array) return { bytes: Array.from(value), buffer: Buffer.isBuffer(value) };
  if (Array.isArray(value)) return value.map(item => observeStateReader(item, ids));
  if (value && typeof value === 'object') return Object.entries(value).map(([key, item]) => [key, observeStateReader(item, ids)]);
  return value;
}
export function readerWeaponSnapshot(fixture) {
  const { weapon, ids } = fixture, descriptors = Object.getOwnPropertyDescriptors(weapon), state = {};
  for (const key of ['level', ...WEAPON_STATE_NUMBERS, ...WEAPON_STATE_FLAGS, 'salvoSeed', 'salvoCount']) if ('value' in (descriptors[key] || {})) state[key] = descriptors[key].value;
  if ('value' in (descriptors.def || {})) state.definition = ids.get(descriptors.def.value) ?? 'other';
  const timeline = descriptors.melee?.value;
  if (timeline) state.melee = { time: timeline.time, index: timeline.index, active: timeline.active,
    swing: timeline.swing == null ? timeline.swing : ids.get(timeline.swing) ?? 'other' };
  return state;
}
export function arsenalReaderCase(name, { kind = 'gun', payload = null, setup = null, apply = false } = {}) {
  return { name: `arsenal:${name}`, run(native) {
    const f = createReaderWeapon(kind), events = [];
    f.bytes = payload ? payload(f) : deltaBytes({ numbers: { timer: -0, heat: .375 }, melee: kind === 'melee' ? { time: .2, index: 2, active: 1 } : null });
    Object.assign(f, { native, events }); const cleanup = setup?.(f);
    const before = readerWeaponSnapshot(f); let entry = null, error = null;
    try { entry = validateArsenalWeapon(f.weapon, f.bytes, native); if (apply) applyArsenalWeapon(entry); }
    catch (caught) { error = { name: caught.name, message: caught.message }; }
    finally { cleanup?.(); }
    return { error, result: entry && { state: observeStateReader(entry.state), melee: observeStateReader(entry.melee), weaponIdentity: entry.weapon === f.weapon },
      events: events.slice(), before, after: readerWeaponSnapshot(f) };
  } };
}

export function createReaderRemote({ melee = false, empty = false } = {}) {
  const primary = readerDefinition(melee ? 'melee' : 'gun');
  const strike = { ...readerDefinition(), class: 'reader-strike', base: 'destruct', channel: 1, strike: {}, SalvoCount: 3,
    SalvoDelay: .25, AimDistance: 10, ScatterDistance: 4, charges: [{ at: .5, SalvoCount: 5 }] };
  const root = new RemoteDroids({ scene: { add() {}, remove() {} }, camera: null, collision: { cast: () => null, gather: () => [] },
    lights: { flash() {} }, model: () => null, fire() {}, explode() {} });
  const context = root.createContext('player', { random: reconRandom(34, 1) });
  const droid = context.launch({ def: { SwitchImmediately: 1, droid: { weapons: empty ? [] : [primary, strike], AddHealth: -1 } } }, new Vector3(0, 2, 0), new Vector3(), 0, 1);
  if (melee) prepareMelee(droid.arsenal);
  context.updateCamera(1 / 60, false, true);
  if (!empty) context.strike(droid, droid.arsenal.current(1).def);
  const arsenal = droid.arsenal, weapons = arsenal.weapons.slice(), ids = new Map();
  weapons.forEach((w, i) => { ids.set(w, `weapon${i}`); w.levels.forEach((d, n) => ids.set(d, `weapon${i}.level${n}`)); });
  return { context, droid, arsenal, weapons, ids, definition: primary };
}
export function captureReaderRemote(fixture) { return encodeRemoteCorrection(captureRemoteReplay(fixture.context, fixture.droid), fixture.droid); }
export function remoteReaderLayout(bytes) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength), fields = {}, ranges = {}, numbers = [];
  let at = 0;
  const field = (key, type) => { const width = { Uint8: 1, Uint16: 2, Uint32: 4, Float64: 8 }[type], offset = at; at += width;
    fields[key] = { offset, width, type }; if (type === 'Float64') numbers.push(key); return view['get' + type](offset, true); };
  field('version', 'Uint8'); field('bits', 'Uint8');
  for (const key of ['yaw', 'pitch', 'health', 'countdown', 'lookX', 'lookY']) field(key, 'Float64');
  field('zoomStep', 'Uint8'); field('randomSeed', 'Uint32');
  for (const key of ['position', 'velocity']) for (let i = 0; i < 3; i++) field(`${key}${i}`, 'Float64');
  field('selected0', 'Uint8'); field('selected1', 'Uint8'); const count = field('count', 'Uint8');
  for (let i = 0; i < count; i++) { const length = field(`weapon${i}.length`, 'Uint16'); ranges[`weapon${i}`] = { offset: at, length }; at += length; }
  if (field('salvo', 'Uint8')) {
    field('salvo.index', 'Uint8'); field('salvo.level', 'Uint8'); field('salvo.left', 'Uint32'); field('salvo.wait', 'Float64');
    for (const key of ['at', 'x', 'y', 'z']) for (let i = 0; i < 3; i++) field(`salvo.${key}${i}`, 'Float64');
  }
  if (field('camera', 'Uint8')) {
    for (let i = 0; i < 3; i++) field(`camera.position${i}`, 'Float64');
    for (let i = 0; i < 4; i++) field(`camera.quaternion${i}`, 'Float64');
    for (const key of ['aspect', 'fov', 'reach']) field(`camera.${key}`, 'Float64'); field('camera.fresh', 'Uint8');
  }
  return { fields, ranges, numbers, length: at };
}
export function patchRemoteField(bytes, layout, key, value) {
  const { offset, type } = layout.fields[key]; new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)['set' + type](offset, value, true); return bytes;
}
export function remoteReaderCase(name, { template = null, options = {}, mutate = null, setup = null, restore = false } = {}) {
  const source = template || captureReaderRemote(createReaderRemote(options));
  return { name: `recon:${name}`, run(native) {
    const f = createReaderRemote(options), events = []; f.bytes = mutate ? mutate(source.slice(), remoteReaderLayout(source)) : source.slice();
    Object.assign(f, { native, events }); const before = captureRemoteReplay(f.context, f.droid), cleanup = setup?.(f);
    let result = null, error = null;
    try { result = decodeRemoteCorrection(f.bytes, f.droid, native); if (restore) restoreRemoteReplay(f.context, f.droid, result); }
    catch (caught) { error = { name: caught.name, message: caught.message }; }
    finally { cleanup?.(); }
    // The observer uses retained ordinary objects rather than re-reading any
    // prerequisite getter installed on the live droid or arsenal.
    const after = { yaw: f.droid.yaw, pitch: f.droid.pitch, health: f.droid.health, position: [f.droid.position.x, f.droid.position.y, f.droid.position.z],
      weapons: f.weapons.map(weapon => readerWeaponSnapshot({ weapon, ids: f.ids })), randomSeed: f.context.random.seed };
    return { error, result: observeStateReader(result, f.ids), events: events.slice(), before: observeStateReader(before, f.ids), after };
  } };
}

export function stateReaderCases() {
  const cases = [];
  for (const kind of ['gun', 'melee']) {
    const melee = kind === 'melee' ? { time: .2, index: 2, active: 1 } : null;
    const full = deltaBytes({ numbers: Object.fromEntries(WEAPON_STATE_NUMBERS.map(key => [key, key === 'reserve' ? Infinity : key === 'yawSpreadOverride' ? NaN : .125])), melee });
    cases.push(arsenalReaderCase(`${kind}:valid`, { kind }), arsenalReaderCase(`${kind}:apply`, { kind, apply: true }));
    for (let cut = 0; cut < full.length; cut++) cases.push(arsenalReaderCase(`${kind}:cut${cut}`, { kind, payload: () => full.subarray(0, cut) }));
    for (const frame of [1, 7, 'Buffer']) cases.push(arsenalReaderCase(`${kind}:frame${frame}`, { kind, payload: () => framedCorrection(full, frame === 'Buffer' ? 5 : frame, frame === 'Buffer') }));
    for (const [tag, value] of [['nan', NaN], ['infinity', Infinity], ['negative-infinity', -Infinity], ['negative-zero', -0], ['subnormal', Number.MIN_VALUE], ['fraction', .375]]) {
      for (const key of WEAPON_STATE_NUMBERS) cases.push(arsenalReaderCase(`${kind}:${key}:${tag}`, { kind, payload: () => deltaBytes({ numbers: { [key]: value }, melee }) }));
    }
    for (const level of [0, 1, 2, 255]) for (const flags of [0, 1, 63, 127, 128, 255]) cases.push(arsenalReaderCase(`${kind}:level${level}:flags${flags}`, { kind, payload: () => deltaBytes({ level, flags, melee }) }));
    for (let bit = 20; bit < 32; bit++) cases.push(arsenalReaderCase(`${kind}:mask${bit}`, { kind, payload: () => deltaBytes({ mask: 1 << bit, melee }) }));
    cases.push(arsenalReaderCase(`${kind}:trailing`, { kind, payload: () => new Uint8Array([...full, 0]) }));
    if (melee) for (const time of [-2, -1, -.5, -0, .5, NaN, Infinity]) for (const index of [0, 2, 3, 255]) for (const active of [0, 1, 2]) {
      cases.push(arsenalReaderCase(`melee:tail:${time}:${index}:${active}`, { kind, payload: () => deltaBytes({ flags: 255, numbers: { heat: NaN }, melee: { time, index, active } }) }));
    }
  }
  for (const options of [{}, { melee: true }, { empty: true }]) {
    const label = options.melee ? 'melee' : options.empty ? 'empty' : 'standard', source = captureReaderRemote(createReaderRemote(options)), layout = remoteReaderLayout(source);
    const add = (name, extra = {}) => cases.push(remoteReaderCase(`${label}:${name}`, { template: source, options, ...extra }));
    add('valid'); add('restore', { restore: true });
    for (let cut = 0; cut < source.length; cut++) add(`cut${cut}`, { mutate: b => b.subarray(0, cut) });
    for (const offset of [1, 7, 'Buffer']) add(`frame${offset}`, { mutate: b => framedCorrection(b, offset === 'Buffer' ? 5 : offset, offset === 'Buffer') });
    add('trailing', { mutate: b => new Uint8Array([...b, 0]) });
    for (const name of layout.numbers) for (const [tag, value] of [['nan', NaN], ['infinity', Infinity], ['negative-infinity', -Infinity], ['negative-zero', -0], ['subnormal', Number.MIN_VALUE]]) {
      add(`${name}:${tag}`, { mutate: b => patchRemoteField(b, layout, name, value) });
    }
    for (const [key, values] of Object.entries({ version: [0, 1, 2, 3, 255], bits: [0, 1, 2, 3, 4, 5, 6, 7, 8, 255], zoomStep: [0, 1, 2, 255],
      selected0: [0, 1, 2, 255], selected1: [0, 1, 2, 255], count: [0, 1, 2, 3, 255], salvo: [2, 255], camera: [2, 255], 'camera.fresh': [0, 1, 2, 255],
      'camera.aspect': [-1, -0, Number.MIN_VALUE], 'salvo.index': [0, 1, 2, 255], 'salvo.level': [0, 1, 2, 255], 'salvo.left': [0, 3, 4, 0xffffffff] })) {
      if (!layout.fields[key]) continue;
      for (const value of values) add(`${key}:${value}`, { mutate: b => patchRemoteField(b, layout, key, value) });
    }
  }
  return cases;
}
export function runStateReaderCase(scenario, native = null) {
  const before = native?.calls ?? 0, result = scenario.run(native);
  return { result, calls: (native?.calls ?? 0) - before };
}
