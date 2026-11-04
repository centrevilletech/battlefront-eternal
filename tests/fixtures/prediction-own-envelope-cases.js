import { Quaternion, Vector3 } from 'three';
import { PlayerCombat } from '../../src/combat.js';
import { Arsenal } from '../../src/weapons.js';
import { captureOwnPlayer, decodeOwnPlayer } from '../../src/runtime/network-entity-player.js';
import { correctionDescriptors, correctionGun, framedCorrection } from './prediction-correction-cases.js';

const descriptor = name => correctionDescriptors().find(d => d.name === name);
export const ownEnvelopeKinds = ['infantry', 'droideka', 'ground', 'flight', 'passenger', 'turret', 'recon'];
export function ownEnvelopeView(flags = 2047) {
  const view = { invertMouse: !!(flags & 1), firstPerson: !!(flags & 2) };
  if (flags & 32) view.mouseScale = .003;
  if (flags & 64) view.aspect = 16 / 9;
  if (flags & 128) view.viewTick = 17.625;
  if (flags & (4 | 8 | 256 | 512 | 1024)) {
    view.flightSettings = { invert: !!(flags & 8) };
    if (flags & 256) view.flightSettings.sensitivity = .7;
    if (flags & 512) view.flightSettings.curve = 1.4;
    if (flags & 1024) view.flightSettings.bindings = {};
  }
  if (flags & 16) view.gamepad = { axes: [-0, Number.MIN_VALUE, .75, -1], buttons: Array.from({ length: 16 }, (_, i) => ({ pressed: i % 3 === 0 })) };
  return view;
}

export function createOwnEnvelopeFixture(kind = 'infantry', { seed = 2, view = ownEnvelopeView() } = {}) {
  const definitions = [correctionGun(), { ...correctionGun(), class: 'correction-secondary', channel: 1 }];
  const soldier = descriptor(kind === 'droideka' ? 'droideka' : 'soldier-compact-00').create(seed);
  const combat = Object.create(PlayerCombat.prototype);
  Object.assign(combat, { soldier, arsenal: new Arsenal(definitions, { player: true }), health: seed + 70,
    kick: { rate: -0, hold: .5, offset: -.125 }, zoomStep: 0, firstPerson: true, gunUnderwater: false, dead: false, deathTime: 0 });
  combat.arsenal.weapons[0].clip = seed / 16;
  const unit = { id: 1, generation: 7, team: 1, className: 'trooper', soldier, combat, deadTime: -0, removed: false };
  const registry = { contentEpoch: 42, classes: new Map([[1, 'trooper']]), weapons: new Map(definitions.map((d, i) => [i + 1, d.class])), inputCodes: ['KeyW', 'Mouse0'], bindingProfiles: new Map([[0, {}]]) };
  const options = { unit, registry, tick: 100, epoch: 3, classId: 1, view, held: ['KeyW', 'Mouse0'], discardedInput: 0xffffffff };
  if (kind === 'droideka') unit.walker = true;
  if (kind === 'ground' || kind === 'flight') {
    const vehicle = descriptor(kind === 'ground' ? 'ground' : 'flight-compact-Landing-immelmann').create(seed);
    vehicle.aiTarget = { id: 20, generation: 3 }; vehicle.seats[0].weapons = [];
    unit.seat = { vehicle, index: 0 };
    Object.assign(options, { vehicle, vehicleId: 20, vehicleGeneration: 3 });
  }
  if (kind === 'passenger' || kind === 'turret') unit.seat = descriptor(kind === 'passenger' ? 'mounted-2' : 'mounted-3').create(seed).seat;
  if (kind === 'recon') {
    const arsenal = new Arsenal(definitions), droid = { def: { base: 'droid' }, arsenal, yaw: -0, pitch: .6, health: .8, dead: false, countdown: 37 };
    for (const key of ['position', 'velocity', 'eye', 'aimDir', 'target']) droid[key] = new Vector3(0, 2, -3);
    const remotes = { random: { seed: 0xf0123456 }, piloted: droid, lookX: -0, lookY: .07, zoomStep: 0, firstPerson: true,
      simulationView: { fov: 60, reach: 300, fresh: true }, aimCamera: { position: new Vector3(0, 2, 0), quaternion: new Quaternion(), aspect: 1.6 },
      identityFor: () => ({ id: 0, generation: 5 }) };
    combat.remotes = remotes; registry.combat = { definitionId: () => 23 };
  }
  return { ...options, record: captureOwnPlayer(options) };
}

// Locate mutation sites in a valid writer-produced envelope. This does not
// interpret values or validate malformed input; decodeOwnPlayer is the oracle.
export function ownEnvelopeLayout(record) {
  const bytes = record.payload, view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let at = 0; const fields = {}, ranges = {}, numbers = [];
  const field = (name, type) => {
    const size = { Uint8: 1, Uint16: 2, Uint32: 4, Float64: 8 }[type], offset = at; at += size;
    fields[name] = { offset, size, type }; if (type === 'Float64') numbers.push(name);
    return view['get' + type](offset, true);
  };
  const slice = (name, length, lengthField = null) => { ranges[name] = { offset: at, length, lengthField }; at += length; };
  for (const [name, type] of [['epoch', 'Uint32'], ['contentEpoch', 'Uint32'], ['classId', 'Uint16'], ['team', 'Uint8'], ['bodySchema', 'Uint8'], ['vehicleId', 'Uint32'], ['vehicleGeneration', 'Uint32']]) field(name, type);
  slice('body', field('bodyLength', 'Uint16'), 'bodyLength');
  field('health', 'Float64'); field('dead', 'Uint8'); field('deathTime', 'Float64'); field('unitDeadTime', 'Float64'); field('removed', 'Uint8');
  for (const name of ['kick.rate', 'kick.hold', 'kick.offset']) field(name, 'Float64');
  field('zoomStep', 'Uint8'); field('zoom', 'Float64'); field('weaponViewFlags', 'Uint8');
  field('selected0', 'Uint8'); field('selected1', 'Uint8');
  const weapons = field('weaponCount', 'Uint8');
  for (let i = 0; i < weapons; i++) { field(`weapon${i}.id`, 'Uint16'); slice(`weapon${i}`, field(`weapon${i}.length`, 'Uint16'), `weapon${i}.length`); }
  slice('heldBits', 16);
  const flags = field('viewFlags', 'Uint16');
  for (const [name, bit] of [['mouseScale', 32], ['aspect', 64], ['viewTick', 128]]) if (flags & bit) field(name, 'Float64');
  field('bindingProfileId', 'Uint16');
  if (flags & 256) field('sensitivity', 'Float64'); if (flags & 512) field('curve', 'Float64');
  if (flags & 16) { for (let i = 0; i < 4; i++) field(`axis${i}`, 'Float64'); field('buttons', 'Uint16'); }
  field('discardedInput', 'Uint32'); const mode = field('controlMode', 'Uint8');
  if (mode >= 2) slice('mounted', field('mountedLength', 'Uint16'), 'mountedLength');
  if (mode === 1) {
    field('remoteId', 'Uint16'); field('remoteGeneration', 'Uint32'); field('remoteDefinitionId', 'Uint16');
    slice('remote', field('remoteLength', 'Uint16'), 'remoteLength');
  }
  return { fields, ranges, numbers, length: at };
}

export function patchOwnField(record, layout, name, value) {
  const { offset, type } = layout.fields[name]; new DataView(record.payload.buffer, record.payload.byteOffset, record.payload.byteLength)['set' + type](offset, value, true);
  return record;
}
export function replaceOwnRange(record, name, bytes) {
  const layout = ownEnvelopeLayout(record), range = layout.ranges[name];
  const out = new Uint8Array(record.payload.length - range.length + bytes.length);
  out.set(record.payload.subarray(0, range.offset)); out.set(bytes, range.offset); out.set(record.payload.subarray(range.offset + range.length), range.offset + bytes.length);
  const next = { ...record, payload: out };
  if (range.lengthField) patchOwnField(next, layout, range.lengthField, bytes.length);
  return next;
}
export function observeOwnEnvelope(value, source) {
  if (value instanceof Uint8Array) return { bytes: Array.from(value), buffer: Buffer.isBuffer(value), aliasesSource: value.buffer === source?.buffer,
    sourceOffset: value.buffer === source?.buffer ? value.byteOffset - source.byteOffset : null };
  if (Array.isArray(value)) return value.map(item => observeOwnEnvelope(item, source));
  if (value && typeof value === 'object') return Object.entries(value).map(([name, item]) => [name, observeOwnEnvelope(item, source)]);
  return value;
}
export function runOwnEnvelopeCase(scenario, native = null) {
  const record = scenario.make(), source = record?.payload, before = native?.calls ?? 0;
  let decoded = null, error = null;
  try { decoded = decodeOwnPlayer(record, native); } catch (caught) { error = { name: caught.name, message: caught.message }; }
  return { error, value: observeOwnEnvelope(decoded, source), decoded, record, calls: (native?.calls ?? 0) - before };
}

export function ownEnvelopeCases() {
  const cases = [];
  const add = (label, base, transform = r => r, native = true) => cases.push({ label, native, make: () => transform({ ...base, payload: base.payload.slice() }) });
  for (const kind of ownEnvelopeKinds) {
    const base = createOwnEnvelopeFixture(kind).record, layout = ownEnvelopeLayout(base);
    add(`${kind}:valid`, base);
    for (const offset of [1, 7]) add(`${kind}:offset${offset}`, base, r => ({ ...r, payload: framedCorrection(r.payload, offset) }));
    add(`${kind}:Buffer`, base, r => ({ ...r, payload: framedCorrection(r.payload, 5, true) }));
    const cuts = new Set([0, 1, base.payload.length - 1]);
    for (const { offset, size } of Object.values(layout.fields)) for (let n = 0; n <= size; n++) cuts.add(offset + n);
    for (const { offset, length } of Object.values(layout.ranges)) for (const n of [0, 1, length >> 1, length - 1, length]) cuts.add(offset + n);
    for (const cut of [...cuts].sort((a, b) => a - b)) if (cut >= 0 && cut < base.payload.length) add(`${kind}:cut${cut}`, base, r => ({ ...r, payload: r.payload.subarray(0, cut) }));
    add(`${kind}:trailing`, base, r => ({ ...r, payload: new Uint8Array([...r.payload, 0]) }));
    for (const name of layout.numbers) for (const [tag, value] of [['nan', NaN], ['positive-infinity', Infinity], ['negative-infinity', -Infinity], ['negative-zero', -0], ['subnormal', Number.MIN_VALUE]]) {
      add(`${kind}:${name}:${tag}`, base, r => patchOwnField(r, layout, name, value));
    }
    for (const [name, values] of Object.entries({ team: [0, 2, 3, 255], dead: [1, 2, 255], removed: [1, 2, 255], weaponViewFlags: [0, 3, 4, 255],
      selected0: [0, 255], selected1: [0, 255], zoomStep: [0, 255], viewFlags: [2048, 65535], controlMode: [5, 255],
      mouseScale: [-1, 0, Number.MIN_VALUE], aspect: [-1, 0, Number.MIN_VALUE], viewTick: [-1, -0, .5, 0xffffffff, 0x100000000],
      sensitivity: [0, 10, 10 + Number.EPSILON * 8], curve: [.1 - Number.EPSILON, .1, 4, 4 + Number.EPSILON * 4], axis0: [-1, 1, -1.00001, 1.00001] })) {
      for (const value of values) add(`${kind}:${name}:${value}`, base, r => patchOwnField(r, layout, name, value));
    }
    for (const [name, range] of Object.entries(layout.ranges)) if (range.lengthField) {
      add(`${kind}:${name}-oversized-length`, base, r => patchOwnField(r, layout, range.lengthField, 65535));
      if (name !== 'mounted') {
        add(`${kind}:${name}-empty`, base, r => replaceOwnRange(r, name, new Uint8Array()));
        add(`${kind}:${name}-opaque`, base, r => replaceOwnRange(r, name, new Uint8Array([255, 0, 128])));
      }
    }
    const pair = (name, first, second) => add(`${kind}:precedence:${name}`, base, r => second(first(r)));
    pair('dead-before-nan', r => patchOwnField(r, layout, 'dead', 255), r => patchOwnField(r, layout, 'deathTime', NaN));
    pair('header-before-team', r => patchOwnField(r, layout, 'team', 0), r => ({ ...r, payload: r.payload.subarray(0, 19) }));
    pair('trailing-before-view', r => patchOwnField(r, layout, 'viewTick', -1), r => ({ ...r, payload: new Uint8Array([...r.payload, 0]) }));
    pair('view-tick-before-axis', r => patchOwnField(r, layout, 'viewTick', -1), r => patchOwnField(r, layout, 'axis0', 2));
    pair('unknown-flags-before-view', r => patchOwnField(r, layout, 'viewFlags', 65535), r => ({ ...r, payload: r.payload.subarray(0, layout.fields.mouseScale.offset) }));
    if (kind === 'recon') {
      for (const id of [0, 4095, 4096, 65535]) add(`recon:identity:${id}`, base, r => patchOwnField(r, layout, 'remoteId', id));
      pair('identity-before-length', r => patchOwnField(r, layout, 'remoteGeneration', 0), r => ({ ...r, payload: r.payload.subarray(0, layout.fields.remoteLength.offset) }));
      pair('definition-read-before-identity', r => patchOwnField(r, layout, 'remoteId', 65535), r => ({ ...r, payload: r.payload.subarray(0, layout.fields.remoteDefinitionId.offset) }));
    }
    if (layout.ranges.mounted) {
      const start = layout.ranges.mounted.offset;
      const mounted = base.payload.slice(start, start + layout.ranges.mounted.length);
      for (let cut = 0; cut < mounted.length; cut++) add(`${kind}:mounted-cut${cut}`, base, r => replaceOwnRange(r, 'mounted', mounted.subarray(0, cut)));
      for (const mode of [0, 1, 2, 3, 4, 5]) add(`${kind}:mounted-mode${mode}`, base, r => { r.payload[start] = mode; return r; });
      for (const [offset, values] of [[1, [0]], [5, [0]], [9, [0, 255]], [10, [0, 3, 4, 255]]]) for (const value of values) {
        add(`${kind}:mounted-header${offset}:${value}`, base, r => {
          if (offset === 1 || offset === 5) new DataView(r.payload.buffer).setUint32(start + offset, value, true);
          else r.payload[start + offset] = value; return r;
        });
      }
      const mode = mounted[0], values = mode === 4 ? 0 : mode === 3 ? 6 : 2 + Number(!!(mounted[10] & 1)) + Number(!!(mounted[10] & 2));
      const countAt = 11 + values * 8;
      for (let i = 0; i < values; i++) add(`${kind}:mounted-nan${i}`, base, r => {
        new DataView(r.payload.buffer).setFloat64(start + 11 + i * 8, NaN, true); return r;
      });
      for (const count of [0, 1, 2, 3, 255]) add(`${kind}:mounted-count${count}`, base, r => { r.payload[start + countAt] = count; return r; });
      if (mounted[countAt]) {
        for (const barrel of [-1, .5, NaN, Infinity, 9007199254740992]) add(`${kind}:mounted-barrel:${barrel}`, base, r => {
          new DataView(r.payload.buffer).setFloat64(start + countAt + 1, barrel, true); return r;
        });
        pair('barrel-before-view', r => {
          new DataView(r.payload.buffer).setFloat64(start + countAt + 1, NaN, true); return r;
        }, r => patchOwnField(r, layout, 'viewTick', -1));
      }
      pair('mounted-before-trailing', r => { r.payload[start] = 255; return r; }, r => ({ ...r, payload: new Uint8Array([...r.payload, 0]) }));
      pair('mounted-before-view', r => { r.payload[start] = 255; return r; }, r => patchOwnField(r, layout, 'viewTick', -1));
    }
  }
  for (let flags = 0; flags < 2048; flags++) {
    const base = createOwnEnvelopeFixture('infantry', { view: ownEnvelopeView(flags) }).record, layout = ownEnvelopeLayout(base);
    add(`view-flags:${flags}`, base, r => patchOwnField(r, layout, 'viewFlags', flags));
    if (!(flags & 4) && (flags & 768)) {
      add(`view-flags:${flags}:context-before-truncation`, base, r => {
        patchOwnField(r, layout, 'viewFlags', flags); return { ...r, payload: r.payload.subarray(0, (layout.fields.sensitivity || layout.fields.curve).offset) };
      });
    }
  }
  const base = createOwnEnvelopeFixture().record;
  add('oversized-opaque-body', base, r => replaceOwnRange(r, 'body', new Uint8Array(65535).fill(255)));
  add('unknown-body-schema', base, r => { r.payload[11] = 255; return r; });
  add('wrong-kind', base, r => ({ ...r, kind: 'delta' }));
  add('wrong-schema', base, r => ({ ...r, schema: 0 }));
  add('wrong-payload-type', base, r => ({ ...r, payload: new DataView(r.payload.buffer) }));
  return cases;
}
