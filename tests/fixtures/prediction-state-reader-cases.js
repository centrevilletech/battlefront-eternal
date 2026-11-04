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
  ids.set(weapon.random, 'random'); ids.set(weapon.salvoRandom, 'salvoRandom');
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
  for (const key of ['random', 'salvoRandom']) if ('value' in (descriptors[key] || {})) state[key] = descriptors[key].value === null ? null : ids.get(descriptors[key].value) ?? typeof descriptors[key].value;
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
  return [...cases, ...stateReaderCallbackCases()];
}
export function runStateReaderCase(scenario, native = null) {
  const before = native?.calls ?? 0, result = scenario.run(native);
  return { result, calls: (native?.calls ?? 0) - before };
}

export function stateReaderCallbackCases() {
  const cases = [];
  cases.push(arsenalReaderCase('saved-mask-after-cold-constructor', { payload: () => deltaBytes({ numbers: { heat: .125 } }), setup(f) {
    Object.defineProperty(f.definition, 'ammo', { configurable: true, get() {
      f.events.push('ammo'); f.bytes[0] = 1; f.bytes[1] = 127;
      const view = new DataView(f.bytes.buffer); view.setUint32(2, 4, true); view.setFloat64(6, .75, true); return 4;
    } });
  } }));
  for (const late of [false, true]) cases.push(arsenalReaderCase(`subarray-copy-timing-${late}`, {
    payload: () => deltaBytes({ numbers: { timer: .125, heat: .25 } }), setup(f) {
      const install = () => {
        Object.defineProperty(f.bytes, 'subarray', { configurable: true, get() {
          f.events.push('subarray'); return function (start, end) {
            f.events.push(['copy', start, end]);
            if (start === 0) f.bytes[1] = 64;
            if (start === 6) new DataView(f.bytes.buffer).setFloat64(14, .875, true);
            return Uint8Array.prototype.subarray.call(this, start, end);
          };
        } });
      };
      if (late) Object.defineProperty(f.definition, 'ammo', { configurable: true, get() { f.events.push('ammo'); install(); return 4; } });
      else install();
    },
  }));
  cases.push(arsenalReaderCase('melee-current-three-incoming-one', { kind: 'melee', apply: true,
    payload: () => deltaBytes({ level: 1, melee: { time: .5, index: 2, active: 1 } }) }));
  cases.push(arsenalReaderCase('melee-current-one-incoming-three', { kind: 'melee',
    payload: () => deltaBytes({ level: 0, melee: { time: .5, index: 2, active: 1 } }), setup(f) { f.weapon.def = f.levels[1]; } }));
  cases.push(arsenalReaderCase('melee-limit-after-copied-values', { kind: 'melee', setup(f) {
    Object.defineProperty(f.weapon, 'def', { configurable: true, get() {
      f.events.push('current def'); f.bytes[0] = 255; f.bytes[1] = 255;
      new DataView(f.bytes.buffer).setFloat64(6, NaN, true); f.bytes[f.bytes.length - 1] = 0; return f.definition;
    } });
  } }));
  cases.push(arsenalReaderCase('melee-invalid-time-skips-definition', { kind: 'melee',
    payload: () => deltaBytes({ flags: 255, melee: { time: NaN, index: 2, active: 255 } }), setup(f) {
      Object.defineProperty(f.weapon, 'def', { get() { f.events.push('unexpected def'); throw Error('definition observed'); } });
    } }));
  for (const bound of [0, '0', -1, 2, 3, Infinity, NaN]) cases.push(arsenalReaderCase(`melee-bound-coercion-${String(bound)}-${typeof bound}`, { kind: 'melee', setup(f) {
    f.definition.swings = { get length() {
      f.events.push('length'); return { valueOf() { f.events.push(['bound', bound]); return bound; } };
    } };
  } }));
  cases.push(arsenalReaderCase('late-buffer-getter-melee-view', { kind: 'melee', setup(f) {
    const source = f.bytes.buffer, alternate = source.slice(0); let changed = false;
    new DataView(alternate).setFloat64(f.bytes.length - 10, .875, true);
    Object.defineProperty(f.definition, 'ammo', { get() {
      f.events.push('ammo'); if (!changed) { changed = true; Object.defineProperty(f.bytes, 'buffer', { get() { f.events.push('buffer'); return alternate; } }); } return 4;
    } });
  } }));
  for (const throwing of [false, true]) cases.push(arsenalReaderCase(`publication-replacement-${throwing}`, { kind: 'melee', apply: true, setup(f) {
    const replacement = { time: -1, index: 0, active: false, swing: null, restoreState(state) { f.events.push(['restore replacement', { ...state }, this === replacement]); this.time = state.time; } };
    Object.defineProperty(f.weapon, 'def', { configurable: true, get: () => f.definition, set(value) { f.events.push(['set def', f.ids.get(value)]); f.weapon.melee = replacement; } });
    let timer = f.weapon.timer;
    Object.defineProperty(f.weapon, 'timer', { configurable: true, get: () => timer, set(value) { f.events.push(['set timer', value]); if (throwing) throw Error('timer stopped publication'); timer = value; } });
  } }));

  const evaluate = (weapon, bytes, native) => {
    try { const entry = validateArsenalWeapon(weapon, bytes, native); return { state: entry.state, melee: entry.melee, same: entry.weapon === weapon }; }
    catch (error) { return { error: { name: error.name, message: error.message } }; }
  };
  const script = (name, run) => cases.push({ name: `arsenal:${name}`, run });
  script('warm-shared-definition-cache', native => {
    const f = createReaderWeapon(), events = [], bytes = deltaBytes(); captureArsenalWeapon(f.weapon);
    Object.defineProperty(f.definition, 'ammo', { configurable: true, get() { events.push('cached ammo'); return 10; } });
    const other = new Weapon(f.definition); events.length = 0;
    const first = evaluate(f.weapon, bytes, native), shared = evaluate(other, bytes, native);
    const fresh = createReaderWeapon('gun', { ...readerDefinition(), ammo: 10 });
    const changed = evaluate(fresh.weapon, bytes, native);
    return { first, shared, changed, events };
  });
  script('cold-cache-failure-and-later-parse-failure', native => {
    const f = createReaderWeapon(), events = []; let first = true;
    Object.defineProperty(f.definition, 'ammo', { get() { events.push('ammo'); if (first) { first = false; throw Error('cold constructor failed'); } return 4; } });
    const outcomes = [evaluate(f.weapon, deltaBytes(), native), evaluate(f.weapon, deltaBytes({ mask: 3, numbers: { timer: .25 } }), native), evaluate(f.weapon, deltaBytes(), native)];
    return { outcomes, events };
  });
  script('reentrant-shared-cold-initial', native => {
    const f = createReaderWeapon(), other = new Weapon(f.definition), events = []; let once = true;
    Object.defineProperty(f.definition, 'ammo', { get() {
      events.push('ammo'); if (once) { once = false; events.push(['nested', evaluate(other, deltaBytes({ numbers: { heat: .875 } }), native)]); } return 4;
    } });
    const outcome = evaluate(f.weapon, deltaBytes({ numbers: { timer: -.125 } }), native);
    return { outcome, events };
  });
  script('retained-baseline-slice-partial-copy', native => {
    const f = createReaderWeapon(), events = [], bytes = deltaBytes({ level: 1, flags: 65, mask: 3, numbers: { timer: .25 } });
    const own = Object.getOwnPropertyDescriptor(Uint8Array, Symbol.species); let retained = null, once = true;
    const restore = () => { if (own) Object.defineProperty(Uint8Array, Symbol.species, own); else delete Uint8Array[Symbol.species]; };
    Object.defineProperty(f.definition, 'ammo', { get() {
      events.push('ammo'); if (once) { once = false; Object.defineProperty(Uint8Array, Symbol.species, { configurable: true, get() {
        events.push('slice species'); return function (length) { retained = new Uint8Array(length); restore(); return retained; };
      } }); } return 4;
    } });
    let outcome; try { outcome = evaluate(f.weapon, bytes, native); } finally { restore(); }
    return { outcome, events, partial: retained && Array.from(retained) };
  });
  script('overlapping-baseline-slice-sequential-copy', native => {
    const f = createReaderWeapon(), events = [], storage = new ArrayBuffer(166), bytes = new Uint8Array(storage, 0, 22);
    bytes.set(deltaBytes({ numbers: { clip: .25, reserve: .75 } }));
    const own = Object.getOwnPropertyDescriptor(Uint8Array, Symbol.species); let full = null, once = true;
    const restore = () => { if (own) Object.defineProperty(Uint8Array, Symbol.species, own); else delete Uint8Array[Symbol.species]; };
    Object.defineProperty(f.definition, 'ammo', { get() {
      events.push('ammo'); if (once) { once = false; Object.defineProperty(Uint8Array, Symbol.species, { configurable: true, get() {
        events.push('slice species'); return function () { full = new Uint8Array(storage, 4, 162); restore(); return full; };
      } }); } return 4;
    } });
    let outcome; try { outcome = evaluate(f.weapon, bytes, native); } finally { restore(); }
    return { outcome, events, full: full && Array.from(full), shared: full?.buffer === bytes.buffer };
  });
  return [...cases, ...reconReaderCallbackCases()];
}

export function reconReaderCallbackCases() {
  const cases = [], source = captureReaderRemote(createReaderRemote()), layout = remoteReaderLayout(source);
  const add = (name, options = {}) => cases.push(remoteReaderCase(name, { template: source, ...options }));
  for (const truncated of [false, true]) add(`weapon-before-length-${truncated}`, {
    mutate: b => truncated ? b.subarray(0, layout.fields['weapon0.length'].offset) : b,
    setup(f) { Object.defineProperty(f.arsenal.weapons, '0', { get() {
      f.events.push('weapon0'); if (!truncated) patchRemoteField(f.bytes, layout, 'weapon0.length', 65535); return f.weapons[0];
    } }); },
  });
  add('invalid-count-skips-arsenal', { mutate: b => patchRemoteField(b, layout, 'count', 3), setup(f) {
    Object.defineProperty(f.droid, 'arsenal', { get() { f.events.push('unexpected arsenal'); throw Error('arsenal observed'); } });
  } });
  add('channel-zero-rejection-skips-channel-one', { mutate: b => patchRemoteField(b, layout, 'selected0', 255), setup(f) {
    f.arsenal.channels = { 0: { get length() { f.events.push('channel0 length'); return 1; } }, get 1() { f.events.push('unexpected channel1'); throw Error('channel1 observed'); } };
  } });
  add('live-arsenal-replacement', { setup(f) {
    const changed = new Arsenal([{ ...readerDefinition(), ammo: 10 }, f.weapons[1].levels[0]]); let reads = 0;
    changed.weapons[1] = f.weapons[1]; changed.channels[1][0] = f.weapons[1];
    Object.defineProperty(f.droid, 'arsenal', { get() { f.events.push(['arsenal', ++reads]); return reads === 1 ? f.arsenal : changed; } });
  } });
  add('retained-dataview-after-buffer-getter', { setup(f) {
    const alternate = f.bytes.slice(); patchRemoteField(alternate, layout, 'camera.aspect', -1);
    Object.defineProperty(f.arsenal.weapons, '0', { get() {
      f.events.push('weapon0'); Object.defineProperty(f.bytes, 'buffer', { configurable: true, get() { f.events.push('replacement buffer'); return alternate.buffer; } }); return f.weapons[0];
    } });
  } });
  for (const buffer of [false, true]) add(`nested-payload-copy-${buffer}`, { mutate: b => framedCorrection(b, 5, buffer), setup(f) {
    Object.defineProperty(f.weapons[0].levels[0], 'ammo', { get() { f.events.push('ammo'); f.bytes[layout.ranges.weapon0.offset + 1] = 255; return 4; } });
  } });
  add('nested-read-and-outer-tail-mutation', { setup(f) {
    const other = createReaderRemote({ empty: true }), payload = captureReaderRemote(other); let once = true;
    Object.defineProperty(f.weapons[0].levels[0], 'ammo', { get() {
      f.events.push('ammo'); if (once) {
        once = false; const nested = decodeRemoteCorrection(payload, other.droid, f.native); f.events.push(['nested health', nested.health]);
        patchRemoteField(f.bytes, layout, 'selected0', 255); patchRemoteField(f.bytes, layout, 'camera.aspect', 1.25);
      } return 4;
    } });
  } });
  add('retained-salvo-definition-and-count-order', { setup(f) {
    captureArsenalWeapon(f.weapons[1]);
    const def = f.weapons[1].levels[0], replacement = { ...def, SalvoCount: 99 }; f.ids.set(replacement, 'replacement strike');
    Object.defineProperty(def, 'strike', { configurable: true, get() {
      f.events.push('strike'); patchRemoteField(f.bytes, layout, 'salvo.left', 2); patchRemoteField(f.bytes, layout, 'salvo.wait', .875);
      f.weapons[1].levels[0] = replacement; return {};
    } });
    Object.defineProperty(def, 'SalvoCount', { configurable: true, get() {
      f.events.push('old SalvoCount'); patchRemoteField(f.bytes, layout, 'salvo.left', 99); patchRemoteField(f.bytes, layout, 'salvo.at0', 123); return 3;
    } });
  } });
  add('invalid-wait-skips-salvo-count', { mutate: b => patchRemoteField(b, layout, 'salvo.wait', NaN), setup(f) {
    captureArsenalWeapon(f.weapons[1]);
    Object.defineProperty(f.weapons[1].levels[0], 'SalvoCount', { get() { f.events.push('unexpected SalvoCount'); throw Error('count observed'); } });
  } });
  for (const throwing of [false, true]) add(`vector-method-before-read-${throwing}`, { setup(f) {
    const original = Object.getOwnPropertyDescriptor(Vector3.prototype, 'fromArray'), intrinsic = original.value; let gets = 0, calls = 0;
    Object.defineProperty(Vector3.prototype, 'fromArray', { configurable: true, get() {
      f.events.push(['fromArray getter', ++gets]);
      if (gets === 1) patchRemoteField(f.bytes, layout, 'salvo.at0', 321);
      return function (values, offset = 0) {
        f.events.push(['fromArray call', ++calls, Array.from(values)]);
        if (throwing) throw Error('vector callback stopped');
        if (calls === 1) { patchRemoteField(f.bytes, layout, 'salvo.x0', 654); patchRemoteField(f.bytes, layout, 'camera.aspect', 1.75); }
        if (calls === 2) return { vectorResult: values.slice(), marker: 'custom return' };
        return intrinsic.call(this, values, offset);
      };
    } });
    return () => Object.defineProperty(Vector3.prototype, 'fromArray', original);
  } });
  add('resizable-source-retains-original-view-bounds', { mutate(b) {
    const storage = new ArrayBuffer(b.length - 10, { maxByteLength: b.length + 64 }), bytes = new Uint8Array(storage);
    bytes.set(b.subarray(0, bytes.length)); return bytes;
  }, setup(f) {
    Object.defineProperty(f.arsenal.weapons, '0', { get() {
      f.events.push('grow source'); const old = f.bytes.length; f.bytes.buffer.resize(source.length); f.bytes.set(source.subarray(old), old); return f.weapons[0];
    } });
  } });
  const meleeSource = createReaderRemote({ melee: true }); meleeSource.weapons[0].def = meleeSource.weapons[0].levels[1];
  cases.push(remoteReaderCase('restore-keeps-melee-and-numeric-level-omissions', { template: captureReaderRemote(meleeSource), options: { melee: true }, restore: true, setup(f) {
    f.weapons[0].level = 73; const swing = f.weapons[0].melee; swing.time = .987; swing.index = 2; swing.swing = f.weapons[0].def.swings[2]; swing.active = true;
  } }));
  const adjacent = (n, delta) => {
    const buffer = new ArrayBuffer(8), view = new DataView(buffer); view.setFloat64(0, n, true); view.setBigUint64(0, view.getBigUint64(0, true) + BigInt(delta), true); return view.getFloat64(0, true);
  };
  for (const [directionIndex, direction] of [[.1, .2, .3, .4], [1, 1, 1, 1], [2 ** -500, 1, 2 ** -300, 2 ** -200]].entries()) {
    const norm = Math.hypot(...direction);
    for (const target of [.999, 1.001]) for (const ulp of [-2, -1, 0, 1, 2]) {
      const scale = adjacent(target, ulp), q = direction.map(value => value / norm * scale);
      add(`camera-hypot-${directionIndex}-${target}-${ulp}`, { mutate(b) {
        q.forEach((value, i) => patchRemoteField(b, layout, `camera.quaternion${i}`, value)); return b;
      } });
    }
  }
  for (const [name, q] of [['large', [1e308, 1e308, 1e308, 1e308]], ['subnormal', [Number.MIN_VALUE, Number.MIN_VALUE, 0, -0]]]) {
    add(`camera-hypot-${name}`, { mutate(b) { q.forEach((value, i) => patchRemoteField(b, layout, `camera.quaternion${i}`, value)); return b; } });
  }
  return cases;
}
