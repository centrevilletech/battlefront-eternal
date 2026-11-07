// Owned semantic capture on the battle's shared module. This layer reads
// object fields and content identities; Rust selects layouts and writes bytes.
import { bridgeFor } from '../../runtime.js';
import { checkPayloadRecordFits } from '../../../runtime/network-entity-codec.js';
import { captureArsenalWeapon } from '../../../runtime/network-arsenal-state.js';
import { WEAPON_STATE_FLAGS, WEAPON_STATE_NUMBERS } from '../../../runtime/network-weapon-state.js';

const num = n => typeof n === 'number' ? n : NaN;
const flag = n => typeof n === 'boolean' ? +n : NaN;
const equal = (a, b) => a?.length === b.length && a.every((n, i) => n === b[i]);
const soldierNumbers = ['fuel', 'aimYaw', 'aimPitch', 'dive', 'stateTime', 'lock'];
const droidekaNumbers = ['aimYaw', 'aimPitch', 'stateTime', 'time', 'legs', 'spin', 'roll', 'lean', 'blastClock', 'lookX', 'lookY', 'gun'];
const groundNumbers = ['health', 'team', 'yaw', 'tilt', 'roll', 'yawRate', 'forwardInput', 'rightInput', 'tiltRate', 'rollRate', 'support'];
const flightNumbers = 'health team yaw tilt roll yawRate forwardInput rightInput altitude throttle airspeed tiltRate stateTime floorY bank power stickX stickY flightRollRate trickCooldown trickPending gear landingMessageTime flightClock flightAssistId flightLockId flightLockGeneration flightLockTimer flightLostLOS trickTime trickDirection'.split(' ');
const flightFlags = 'alive sleeping grounded rollLock boosting boostExhausted terrainWarning flightActionHeld trickHeld immelmannHeld'.split(' ');
const seatNumbers = ['aimYaw', 'pitch', 'lookYaw', 'lookPitch', 'yawRate', 'lastYaw'];
const vector = (words, value, quaternion = false) => {
  words.push(+(quaternion ? !!value?.isQuaternion : !!value?.isVector3), num(value?.x), num(value?.y), num(value?.z));
  if (quaternion) words.push(num(value?.w));
};
const seat = (words, s) => { for (const key of seatNumbers) { const n = s?.[key]; words.push(n === undefined ? 2 : n === null ? 1 : 0, num(n ?? 0)); } };

export function createWireSemantic(instance) { return createWireSemanticBridge(bridgeFor(instance)); }
export function createWireSemanticBridge(bridge) {
  const x = bridge.x; let handle = x.wire_semantic_create();
  const weapons = new WeakMap(), ownPayloads = new WeakMap(), summaries = new Map();
  const list = (words, values) => { words.push(values.length); for (const n of values) words.push(n); };
  const sid = value => typeof value === 'string' ? bridge.strings.id(value) : value === null ? -1 : -2;
  function run(op, words, raw = null) {
    if (!handle) throw Error('Semantic encoder disposed');
    const ptr = x.wire_semantic_room(handle, words.length); bridge.views(); bridge.f.set(words, ptr / 8);
    if (raw) new Uint8Array(bridge.buffer, ptr + raw.at * 8, raw.bytes.length).set(raw.bytes);
    const error = x.wire_semantic_run(handle, op); if (error) throw Error(bridge.strings.at(error - 1));
    const at = x.wire_semantic_ptr(handle), length = x.wire_semantic_len(handle); bridge.views();
    const view = new DataView(bridge.buffer, at, length), count = view.getUint32(0, true), parts = []; let offset = 4;
    for (let i = 0; i < count; i++) { const size = view.getUint32(offset, true); offset += 4; parts.push(new Uint8Array(bridge.buffer, at + offset, size).slice()); offset += size; }
    return parts;
  }
  function weaponWords(w) {
    const words = [w.levels.indexOf(w.def)];
    for (const key of WEAPON_STATE_FLAGS) words.push(flag(w[key]));
    words.push(+(w.yawSpreadOverride === null));
    for (const key of WEAPON_STATE_NUMBERS) words.push(num(w[key]));
    return words;
  }
  function weapon(w) { return run(0, weaponWords(w))[0]; }
  function arsenal(w, base) {
    const words = weaponWords(w), melee = w.melee ? w.melee.captureState() : null;
    words.push(...base(), +!!melee);
    if (melee) words.push(+melee.time, +melee.index, Number(melee.active));
    const bytes = run(1, words)[0], cached = weapons.get(w);
    if (equal(cached, bytes)) return cached;
    weapons.set(w, bytes); return bytes;
  }
  function soldier(s, into = null) {
    const words = [num(s.contactCount), s.contactMean.lengthSq(), sid(s.posture), sid(s.air), flag(s.grounded), sid(s.diveFrom)];
    for (const key of soldierNumbers) words.push(num(s[key]));
    for (const key of ['position', 'velocity', 'normal', 'settled']) vector(words, s[key]);
    const bytes = run(3, words)[0]; if (!into) return bytes;
    into.bytes.set(bytes); return into.bytes.subarray(0, bytes.length);
  }
  function droideka(d) {
    const prerequisite = d.clips instanceof Map && typeof d.clip === 'function'
      && ['position', 'velocity', 'avoidance', 'previous', 'settled'].every(key => d[key]?.isVector3);
    const words = [+prerequisite, num(d.mode), num(d.gun), flag(d.grounded), flag(d.reverse), flag(d.dead), +!!d.shield];
    if (d.shield) words.push(flag(d.shield.on), num(d.shield.value), Number(d.shield.max));
    words.push(+(d.lastGun !== undefined));
    if (d.lastGun !== undefined) words.push(num(d.lastGun?.index), num(d.lastGun?.barrel), d.gunBarrels?.[d.lastGun?.index]?.length || 1, d.guns?.length ?? 0);
    for (const key of droidekaNumbers) words.push(num(d[key]));
    for (const key of ['position', 'velocity', 'avoidance']) words.push(num(d[key]?.x), num(d[key]?.y), num(d[key]?.z));
    return run(2, words)[0];
  }
  function ground(v) {
    const words = [+(!v.flying && !!v.seats?.[0] && !!v.position?.isVector3 && !!v.velocity?.isVector3 && !!v.quaternion?.isQuaternion)];
    for (const key of groundNumbers) words.push(num(v[key]));
    for (const key of ['position', 'velocity', 'quaternion']) { words.push(num(v[key]?.x), num(v[key]?.y), num(v[key]?.z)); if (key === 'quaternion') words.push(num(v[key]?.w)); }
    words.push(flag(v.alive), flag(v.sleeping)); seat(words, v.seats?.[0]); return run(4, words)[0];
  }
  function flight(v) {
    const words = [+(!!v.flying && !!v.flightConfig && !!v.seats?.[0])];
    for (const key of ['flightState', 'trick', 'flightLockState', 'flightWarning', 'landingMessage', 'landingReason']) words.push(sid(v[key]));
    const floor = new DataView(new ArrayBuffer(8)); let floorAt = 0;
    for (const key of flightNumbers) {
      const value = key === 'floorY' && typeof v[key] !== 'number' ? Infinity : num(v[key]);
      // Number arrays can quiet signaling NaNs. Save this permitted NaN field
      // before boxing, retaining the sampled value across later source reads.
      if (key === 'floorY') { floorAt = words.length; floor.setFloat64(0, value, true); }
      words.push(value);
    }
    for (const key of ['position', 'velocity', 'flightStart', 'flightEnd', 'flightTrickRight']) vector(words, v[key]);
    for (const key of ['quaternion', 'flightStartQ', 'flightTrickLoopQ', 'flightTrickEndQ']) vector(words, v[key], true);
    for (const key of flightFlags) words.push(v[key] === undefined ? 2 : flag(v[key]));
    seat(words, v.seats?.[0]); return run(5, words, { at: floorAt, bytes: new Uint8Array(floor.buffer) })[0];
  }

  function mounted({ mode, id, generation, index, target }) {
    const fields = mode === 4 ? [] : mode === 2 ? ['aimYaw', 'pitch', 'lookYaw', 'lookPitch'] : ['yaw', 'pitch', 'yawVel', 'pitchVel', 'gunYaw', 'gunPitch'];
    const weapons = mode !== 3 ? (target.weapons || []).map(entry => ({ weapon: entry.state, barrel: entry.barrel })) : target.weapon ? [{ weapon: target.weapon, barrel: target.barrel }] : [];
    // Shape guards precede foreign weapon/default reads, as in the caller's
    // serializer. The native writer validates the same bounds independently.
    if (weapons.length > 2) throw Error('mounted weapon count exceeds correction budget');
    const words = [mode, id, generation, index]; list(words, fields.map(key => num(target[key])));
    words.push(weapons.length, +(target.lookYaw !== null), +(target.lookPitch !== null));
    for (const entry of weapons) { const bytes = captureArsenalWeapon(entry.weapon, api); words.push(num(entry.barrel)); list(words, bytes); }
    return run(6, words)[0];
  }
  function own({ unit, body, bodySchema, epoch, registry, classId, vehicleId, vehicleGeneration, bindingProfileId, held, view, discardedInput, mounted, weaponId, remote }) {
    const c = unit.combat, a = c.arsenal, words = [epoch, registry.contentEpoch, classId, unit.team, bodySchema, vehicleId, vehicleGeneration];
    list(words, body); words.push(num(c.health), +!!c.dead, num(c.deathTime ?? 0), num(unit.deadTime ?? 0), +!!unit.removed,
      num(c.kick.rate), num(c.kick.hold), num(c.kick.offset), num(c.zoomStep), num(c.zoom), +!!c.firstPerson, +!!c.gunUnderwater);
    for (let channel = 0; channel < 2; channel++) words.push(a.current(channel) ? num(a.selected[channel]) : 255);
    words.push(a.weapons.length);
    for (const weapon of a.weapons) { const bytes = captureArsenalWeapon(weapon, api); words.push(weaponId(weapon)); list(words, bytes); }
    list(words, [...held].map(code => registry.inputCodes.indexOf(code)));
    const fs = view.flightSettings;
    words.push(+!!view.invertMouse, +!!view.firstPerson, +!!fs, +!!fs?.invert, +!!view.gamepad);
    for (const key of ['mouseScale', 'aspect', 'viewTick']) words.push(+(view[key] !== undefined), num(view[key]));
    for (const key of ['sensitivity', 'curve']) words.push(+(fs?.[key] !== undefined), num(fs?.[key]));
    words.push(+(fs?.bindings !== undefined), num(bindingProfileId));
    for (let i = 0; i < 4; i++) words.push(num(view.gamepad?.axes?.[i] ?? 0));
    for (let i = 0; i < 16; i++) words.push(+!!view.gamepad?.buttons?.[i]?.pressed);
    words.push(num(discardedInput), remote ? 1 : mounted ? mounted[0] : 0); list(words, mounted ?? []);
    if (remote) { words.push(remote.id, remote.generation, remote.definitionId); list(words, remote.payload); }
    const bytes = run(7, words)[0], cached = ownPayloads.get(unit); if (equal(cached, bytes)) return cached;
    ownPayloads.set(unit, bytes); return bytes;
  }
  function recon(state, droid) {
    if (state.weapons.length > 2 || state.weapons.length !== droid.arsenal.weapons.length || state.selected.length !== 2) throw Error('unsupported recon arsenal');
    const words = [state.weapons.length, droid.arsenal.weapons.length]; list(words, Array.from({ length: state.selected.length }, (_, i) => num(state.selected[i])));
    words.push(num(state.zoomStep), droid.arsenal.channels[0].length, droid.arsenal.channels[1].length, flag(state.dead), flag(state.piloted), +!!state.firstPerson);
    for (const key of ['yaw', 'pitch', 'health', 'countdown', 'lookX', 'lookY']) words.push(num(state[key]));
    words.push(num(state.randomSeed)); for (const key of ['position', 'velocity']) list(words, Array.isArray(state[key]) ? state[key].map(num) : []);
    for (const [index, captured] of state.weapons.entries()) {
      const weapon = droid.arsenal.weapons[index]; if (!weapon?.levels[captured.level]) throw Error('invalid recon weapon level');
      list(words, captureArsenalWeapon({ ...weapon, ...captured, def: weapon.levels[captured.level] }, api));
    }
    words.push(+!!state.salvo);
    if (state.salvo) {
      const s = state.salvo, index = droid.arsenal.weapons.findIndex(w => w.levels.includes(s.def));
      if (index < 0 || !s.def.strike) throw Error('recon salvo definition missing');
      words.push(index, droid.arsenal.weapons[index].levels.indexOf(s.def), num(s.left), Math.max(1, Math.floor(s.def.SalvoCount || 1)), num(s.wait));
      for (const key of ['at', 'x', 'y', 'z']) list(words, s[key].toArray().map(num));
    }
    words.push(+!!state.camera);
    if (state.camera) { const c = state.camera; list(words, Array.isArray(c.position) ? c.position.map(num) : []); list(words, Array.isArray(c.quaternion) ? c.quaternion.map(num) : []); words.push(num(c.aspect), num(c.fov), num(c.reach), flag(c.fresh)); }
    return run(8, words)[0];
  }


  function summary(bytes, id, schema, { epoch, tick }) {
    const key = schema * 4294967296 + id, old = summaries.get(key);
    let payload = equal(old, bytes) ? old : bytes;
    if (payload !== old) { if (summaries.size >= 4096) summaries.clear(); summaries.set(key, payload); }
    const record = { kind: 'summary', id, generation: epoch, tick, schema, payload }; checkPayloadRecordFits(record); return record;
  }
  function lookup(map, name, nullable = true) {
    if (name == null && nullable) return 0;
    for (const [id, value] of map) if (value === name) return id;
    throw Error(`summary registry missing ${name}`);
  }
  function battleSummary(battle, options) {
    const teams = Object.keys(battle.reinforcements).map(Number), words = [teams.length];
    for (const team of teams) { words.push(team); for (const key of ['reinforcements', 'start', 'noPost', 'bleed']) words.push(num(battle[key]?.[team] ?? 0)); }
    for (const key of ['over', 'gameOver', 'finished', 'losing']) words.push(+!!battle[key]);
    words.push(num(battle.loser || 0), num(battle.overTime));
    for (const key of ['victoryClock', 'countdown']) words.push(+(battle[key] !== null), num(battle[key]));
    return summary(run(9, words)[0], 0, 1, options);
  }
  function postSummary(post, options) {
    const id = lookup(options.registry.posts, post.name, false), words = [id, num(post.team), num(post.toward), num(post.progress), num(post.neutralize), flag(post.contested), +(post.active !== false), post.counts.size];
    for (const [team, count] of post.counts) words.push(num(team), num(count));
    return summary(run(10, words)[0], id, 2, options);
  }
  function spawnSummary(player, options) {
    const { registry, eligiblePosts, eligibleClasses, slot = 255 } = options;
    if (!Array.isArray(eligiblePosts) || !Array.isArray(eligibleClasses) || eligiblePosts.length > 64 || eligibleClasses.length > 64) throw Error('spawn eligibility limit');
    const s = player.spawn, words = [num(player.unit?.id || 0), num(player.unit?.generation || 0), num(player.team), sid(s.phase), lookup(registry.classes, s.className), lookup(registry.posts, s.post), +!!s.spawnedOnce, +!!s.ended, num(s.deadTime), +!!s.pending];
    if (s.pending) words.push(lookup(registry.classes, s.pending.className, false), lookup(registry.posts, s.pending.post));
    words.push(+!!s.auto); if (s.auto) words.push(num(s.auto.team), lookup(registry.classes, s.auto.className, false), lookup(registry.posts, s.auto.post));
    list(words, eligiblePosts.map(name => lookup(registry.posts, name, false))); list(words, eligibleClasses.map(name => lookup(registry.classes, name, false))); words.push(num(slot));
    return summary(run(11, words)[0], 65536, 3, options);
  }
  function rosterSummaries(battle, players, options) {
    const characters = battle?.characters || [], words = [characters.length];
    for (const c of characters) {
      const link = players.get(c)?.link, path = link?.path;
      words.push(+players.has(c), +!!c.human, c.team - 0);
      for (const key of ['kills', 'teamKills', 'deaths', 'captures']) words.push(c[key] - 0);
      words.push(num(link?.rtt), path?.kind && path?.family ? sid(typeof path.kind === 'string' ? path.kind : 'other') : -1, path?.kind && path?.family ? sid(String(path.family)) : -1, sid(String(c.name ?? '')));
    }
    return run(12, words).map((bytes, i) => summary(bytes, 70000 + i, 4, options));
  }
  function recordSummaries(battle, registry, options) {
    const { only = null, id = 80000, quantum = 1 } = options, characters = battle?.characters || [];
    const rows = []; for (let slot = 0; slot < characters.length && slot < 65535; slot++) { const c = characters[slot]; if ((!only || c === only) && c?.record) rows.push([slot, c.record]); }
    const words = [quantum - 0, rows.length];
    for (const [slot, record] of rows) {
      words.push(slot, record.slots.length);
      for (const s of record.slots) {
        const tallies = ['teamKills', 'vehiclesDestroyed', 'spree', 'survivalist', 'lifeKillsBest', 'headshots', 'deaths', 'escapes', 'lifeBest', 'campBest'].map(key => s[key]);
        words.push(+tallies.some(Boolean)); for (const value of tallies) words.push(value - 0);
        words.push(s.victims.size); for (const [victim, count] of s.victims) words.push(characters.indexOf(victim), count - 0);
        for (const table of ['classes', 'weapons', 'vehicles']) { words.push(s[table].length); const ids = table === 'weapons' ? registry.weaponIds : registry.classIds; for (const entry of s[table]) words.push(ids?.get(entry.key) ?? 0, num(entry.time)); }
      }
    }
    return run(13, words).map((bytes, i) => summary(bytes, id + i, 5, options));
  }

  const api = { weapon, arsenal, soldier, droideka, ground, flight, mounted, own, recon, battleSummary, postSummary, spawnSummary, rosterSummaries, recordSummaries,
    dispose() { if (handle) { x.wire_semantic_drop(handle); handle = 0; } } };
  return api;
}
