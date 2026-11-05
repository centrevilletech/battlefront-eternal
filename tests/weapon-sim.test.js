import test from 'node:test';
import assert from 'node:assert/strict';
import { Weapon, seededRandom } from '../src/weapons.js';
import { prepareMelee } from '../src/heroes.js';
import { loadSim } from '../src/sim/module.js';
import { attachWeaponSim } from '../src/weapon-sim.js';
import { weaponDefinition } from './fixtures/client-weapons.js';
import { Writer, RECORD } from '../tools/sim-capture/format.mjs';
import { Vector3 } from 'three';
import { updateWeaponLock } from '../src/weapon-targeting.js';

const fields = ['level', 'deployed', 'firing', 'timer', 'bottomless', 'perShot', 'clip', 'reserve', 'heat', 'overheated',
  'floor', 'kick', 'spreadScale', 'sniperScale', 'yawSpreadOverride', 'wait', 'reloadLeft', 'throwTime', 'released', 'hidden',
  'charging', 'held', 'charge', 'limit', 'pattern', 'salvosLeft', 'salvoWait', 'salvoSeed', 'salvoCount',
  'lockTargetId', 'lockGeneration', 'lockElapsed', 'lockComplete'];
const aim = { stick: .25, posture: 'stand', zoomed: false };
const pressed = seed => ({ pressed: true, held: true, seed });
const state = w => ({ values: fields.map(k => w[k]), definition: w.levels.indexOf(w.def), random: Boolean(w.random),
  seeded: Boolean(w.salvoRandom), admits: Boolean(w.admitShot), melee: w.melee ? [w.melee.time, w.melee.index, w.melee.active, Boolean(w.melee.swing)] : null });
function recorder() {
  const bytes = new Writer(); let operations = 0;
  return { add(name, value) { bytes.record(RECORD.frame, ++operations, name, value); },
    result() { return { bytes: Buffer.from(bytes.take()), operations }; } };
}
function shots(trace, w, result) {
  trace.add('shots', result.map(shot => { assert.equal(shot.weapon, w); const { weapon, ...value } = shot; return value; }));
  trace.add('state', state(w));
}

test('ordinary native weapons match finite operation and callback bytes across every weapon state machine', async t => {
  const sim = attachWeaponSim(await loadSim());
  const definitions = [weaponDefinition('single', 0, { TriggerSingle: 1 }), weaponDefinition('auto'),
    weaponDefinition('salvo', 0, { SalvoCount: 3, ShotsPerSalvo: 3, InitialSalvoDelay: .025, SalvoDelay: .04, ShotPatternPitchYaw: [[1, -2], [-3, .5]], ShotPatternCount: 2 }),
    weaponDefinition('charge', 0, { MaxPressedTime: .15, charges: [{ at: .3, ShotsPerSalvo: 2, SpreadPerShot: .5 }, { at: .6, ShotsPerSalvo: 3 }] }),
    weaponDefinition('throw', 1, { base: 'grenade', InitialSalvoDelay: .1, HideOnFire: 1 }),
    weaponDefinition('repair', 1, { base: 'repair', ShotDelay: .02 }),
    weaponDefinition('launcher', 0, { base: 'launcher', LockTime: .12, ordnance: { base: 'missile' } }),
    weaponDefinition('melee', 0, { base: 'melee', swings: [{ duration: .1, hit: .03, sound: 'one' }, { duration: .2, hit: .09, sound: 'two' }] })];
  const run = native => {
    const trace = recorder();
    const prior = Math.random; Math.random = seededRandom(953);
    try { for (let k = 0; k < definitions.length; k++) {
      const w = new Weapon(definitions[k]), rng = seededRandom(k + 13);
      prepareMelee({ weapons: [w] }); if (native) sim.attach(w);
      if (k % 2) w.random = () => { trace.add('random', state(w)); return rng(); };
      for (let i = 0; i < 360; i++) {
        const dt = [0, 1 / 120, 1 / 60, 1 / 30][i % 4];
        if (i % 29 === 0) w.updateLock(dt, { id: i % 3, generation: i % 2, alive: i % 5 !== 0 });
        if (i % 31 === 0) { w.level = 7; w.baseLevel(); }
        if (i % 37 === 0) w.deselect();
        if (i % 41 === 0) trace.add('reload', w.startReload());
        trace.add('spread', w.spread((i % 9) / 8, ['stand', 'prone', 'crouch', undefined][i % 4], i % 7 === 0));
        shots(trace, w, w.update(dt, i % 19 !== 0, { held: i % 16 < 9, pressed: i % 16 === 0,
          seed: i % 3 ? null : i * 17, late: i % 11 / 1000 }, i % 43 === 0,
          { stick: i % 7 / 6, posture: ['stand', 'crouch', 'prone'][i % 3], zoomed: i % 5 === 0, repairTarget: i % 13 < 9 }));
      }
    } } finally { Math.random = prior; }
    return trace.result();
  };
  const js = run(false), native = run(true);
  assert.equal(native.operations, js.operations); assert.deepEqual(native.bytes, js.bytes);
  assert.ok(js.operations > 8500);
  t.diagnostic(`${js.operations} exact operation/callback records`);
});

test('ordinary callback publication retains state, admissions and per-invocation generators through mutation and reentry', async t => {
  const sim = attachWeaponSim(await loadSim());
  let operations = 0;
  for (const mode of ['writes', 'replaceRandom', 'nestedRandom', 'consumeSeed', 'clearSeed', 'clearSeedLegacy', 'replaceSeed', 'nestedSeed', 'equalSeeds', 'throw']) {
    const run = native => {
      const trace = recorder(), tokens = [true, null, undefined, 0, { token: 7 }, false];
      const w = new Weapon(weaponDefinition('callbacks', 0, { TriggerSingle: 1, RoundsPerClip: 100, HeatPerShot: 0, ShotsPerSalvo: 2,
        charges: [{ at: .5, SpreadPerShot: 3, SpreadLimit: 9 }] }));
      if (native) sim.attach(w);
      let draw = 0, admission = 0, nested = false, previous = null;
      const replacement = () => { trace.add('replacement', state(w)); return .875; };
      const rng = function () {
        assert.equal(this, undefined, 'the retained generator is called without a receiver');
        trace.add('random', state(w)); draw++;
        if (native && draw === 1) sim.x.memory.grow(1);
        if (mode === 'writes') { w.charge = draw / 16; w.def = w.levels[1]; w.level = 9; w.spreadScale = .75; }
        if (mode === 'replaceRandom' && draw === 1) w.random = replacement;
        if ((mode === 'nestedRandom' || mode === 'nestedSeed') && !nested && draw === 1) {
          nested = true; w.wait = 0;
          shots(trace, w, w.update(0, true, pressed(733), false, aim));
          nested = false;
        }
        if (mode === 'throw' && draw === 2) { w.heat = .125; throw 'draw stopped'; }
        return (draw % 7) / 8;
      };
      w.random = rng;
      w.admitShot = value => {
        assert.equal(value, w); trace.add('admit', state(w));
        if (mode === 'consumeSeed' && w.salvoRandom) trace.add('consumed', [w.salvoRandom(), w.salvoRandom()]);
        if (mode === 'clearSeed' || mode === 'clearSeedLegacy') w.salvoRandom = null;
        if (mode === 'clearSeedLegacy') w.random = null;
        if (mode === 'replaceSeed') w.salvoRandom = replacement;
        if (mode === 'nestedSeed' && !nested) w.salvoRandom = rng;
        if (mode === 'equalSeeds') {
          if (w.salvoCount === 0) { assert.notEqual(w.salvoRandom, previous); previous = w.salvoRandom; }
          trace.add('consumed', w.salvoRandom());
        }
        const result = tokens[admission++ % tokens.length];
        return result;
      };
      const prior = Math.random; Math.random = seededRandom(778);
      try { for (let i = 0; i < 5; i++) {
        w.wait = 0; w.clip = 1;
        try {
          const result = w.update(.01, true, pressed(['writes', 'replaceRandom', 'nestedRandom', 'throw'].includes(mode) ? null : 197), false, aim);
          for (const shot of result) if ('launchAdmission' in shot) assert.ok(tokens.includes(shot.launchAdmission));
          shots(trace, w, result);
        } catch (error) { assert.equal(mode, 'throw'); assert.equal(error, 'draw stopped'); trace.add('thrown', { error, state: state(w) }); }
      } } finally { Math.random = prior; }
      return trace.result();
    };
    const js = run(false), native = run(true);
    assert.equal(native.operations, js.operations, mode); assert.deepEqual(native.bytes, js.bytes, mode);
    operations += js.operations;
  }
  t.diagnostic(`${operations} exact callback and reentry records`);
});

test('melee draw callbacks observe prior end writes and resume their changed timeline', async () => {
  const sim = attachWeaponSim(await loadSim());
  const run = native => {
    const trace = recorder(), w = new Weapon(weaponDefinition('melee-callback', 0, { base: 'melee', swings: [{ duration: .1, hit: .03, sound: 'one' }] }));
    prepareMelee({ weapons: [w] }); if (native) sim.attach(w);
    w.melee.restoreState({ time: .2, index: 0, active: true }); w.firing = true;
    w.random = () => { trace.add('draw', state(w)); w.charge = .375; w.melee.time = .7; return .75; };
    shots(trace, w, w.update(.02, true, pressed(null), false, aim));
    return trace.result();
  };
  assert.deepEqual(run(true).bytes, run(false).bytes);
});

test('native lock progress preserves query order, missing targets and callback changes', async () => {
  const sim = attachWeaponSim(await loadSim());
  const run = native => {
    const trace = recorder(), w = new Weapon(weaponDefinition('lock', 0, { base: 'launcher', LockTime: .15,
      TargetEnemy: 1, TargetPerson: 1, ordnance: { base: 'missile' }, charges: [{ at: .5, base: 'cannon' }] }));
    if (native) sim.attach(w);
    const candidate = { id: 3, generation: 7, alive: true, team: 2, category: 0 }, viewer = { team: 1 };
    let mode = 0;
    const units = { byId(id) { trace.add('byId', { id, state: state(w) }); if (mode === 3) w.def = w.levels[1]; return candidate; },
      relation(a, b) { trace.add('relation', { a, b, state: state(w) }); if (mode === 2) w.wait = .2; return -1; } };
    for (mode = 0; mode < 6; mode++) {
      w.baseLevel(); w.wait = 0;
      for (let i = 0; i < 12; i++) {
        candidate.alive = mode !== 4;
        const options = mode === 5 ? {} : { units, viewer, target: candidate, origin: new Vector3(), dir: new Vector3(0, 0, 1) };
        if (native) sim.updateLock(w, 1 / 60, options); else updateWeaponLock(w, 1 / 60, options);
        trace.add('lock', state(w));
      }
    }
    return trace.result();
  };
  assert.deepEqual(run(true).bytes, run(false).bytes);
});
