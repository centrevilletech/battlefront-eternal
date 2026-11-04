import test from 'node:test';
import { readFileSync } from 'node:fs';
import { gunzipSync } from 'node:zlib';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { loadRayKernel } from '../src/ray-kernel.js';
import { PredictionArsenal } from '../src/runtime/prediction-arsenal.js';
import { createClientPrediction } from '../src/runtime/client-prediction.js';
import { captureCompactSoldier, restoreCompactSoldier } from '../src/runtime/network-entity-soldier-compact.js';
import { captureDroidekaState, restoreDroidekaState } from '../src/runtime/network-droideka-state.js';
import { actorCourse, createPredictionActor } from './fixtures/client-actors.js';
import { createWeaponUnit, weaponDefinition, arsenalObservation } from './fixtures/client-weapons.js';

function fixture(kind, course, scenario) {
  const actor = createPredictionActor({kind, pattern: kind === 'droideka' ? 'droideka' : 'walk'}, course);
  const unit = createWeaponUnit(scenario==='soldier-melee' ? [weaponDefinition('blade',0,{base:'melee',ordnance:undefined,swings:[{duration:.16,hit:.03},{duration:.13,hit:.06},{duration:.22,hit:.1}]}),weaponDefinition('secondary',1)] : [weaponDefinition('burst', 0, {SalvoCount: 3, SalvoDelay: .04, ShotsPerSalvo: 3, RoundsPerClip: 80}),
    weaponDefinition('secondary', 1)]);
  actor.combat.arsenal = unit.combat.arsenal;
  actor.combat.zoomStep = 0;
  unit.soldier = actor.soldier; unit.combat = actor.combat;
  return unit;
}
const capture = unit => unit.soldier.constructor.name === 'Droideka' ? captureDroidekaState(unit.soldier) : captureCompactSoldier(unit.soldier);
const restore = (unit, state) => unit.soldier.constructor.name === 'Droideka' ? restoreDroidekaState(unit.soldier, state) : restoreCompactSoldier(unit.soldier, state);
const shotValue = (s, tick, sequence, id, held, button, level) => ({tick, sequence, id, held, button, level,
  kind: s.repair ? 1 : s.meleeStart ? 2 : s.meleeHit ? 3 : s.meleeEnd ? 4 : 0,
  yaw: s.yaw ?? 0, pitch: s.pitch ?? 0, charge: s.charge ?? 0, target: s.targetId ?? 0, targetGeneration: s.targetGeneration ?? 0, swing: s.swing ?? 0});

// These reference digests and byte streams were recorded from the frozen JS
// implementation before its replay and adapters were removed. Fixtures use only
// synthetic geometry, clips and definitions, so CI needs no game installation.
const encode=value=>typeof value==='number'?Buffer.from(new Float64Array([value]).buffer).toString('hex'):Array.isArray(value)?value.map(encode):value&&typeof value==='object'?Object.fromEntries(Object.keys(value).sort().map(k=>[k,encode(value[k])])):value;
const referenceDigests = {
  'soldier-melee': '44f8f27b509b3850773cd68f8a6b5a897e5df8bc0991487b830db28671eee877',
  'droideka-roll': '2428c0fdc3d49e9e0ebfaffae4ca9303372ce68071d63ced97cd2b7bc1084154',
  soldier: '3bbac210fb877e27c6df289b0fc00a1febbd74e53ef5a92ec3124a274811e684',
  droideka: '0d73e20a08f3015792bf94da61b93a9dea0e2ee8ba22f1a63544041b47904ce8',
  'droideka-visual': 'b049fcf195a3fcc4f696a9c64d1efff6938ec8db347cc8f795c2b0d1aba415de',
  'droideka-grenade': '6ba598469b982466b9259f7747c4e5d443c7b8569e3268de1951a8b1546667a9',
};
for (const [scenario, digest] of Object.entries(referenceDigests)) test(`${scenario} session matches frozen correction, checkpoint and shot identity`, async () => {
  const text=gunzipSync(readFileSync(new URL(`./fixtures/prediction-reference/${scenario}.json.gz`,import.meta.url))).toString('utf8').trimEnd();
  assert.equal(createHash('sha256').update(text).digest('hex'),digest);
  const frames=JSON.parse(text),kind=scenario.startsWith('soldier')?'soldier':'droideka';
  const instance=await loadSim(),course=actorCourse(await loadRayKernel(instance)),a=fixture(kind,course,scenario);
  if(scenario==='droideka-grenade')a.combat.arsenal.weapons[0].def.ordnance.base='grenade';
  const session=createClientPrediction(instance,course.world),wa=new PredictionArsenal(),baseA=capture(a);
  let authority=0,pending=[],sequence=0,epoch=1;
  const edges=[];
  try {
    for(let frame=0;frame<frames.length;frame++) {
      restore(a,baseA);
      if (frame === 25 || frame === 47) {
        // A newer authority invalidates the continuation and replays the same
        // still-pending stream, including commands stamped in the past.
        authority += 4;
      }
      if (frame === 35) { epoch++; a.generation++; pending = []; edges.length = 0; }
      if(scenario==='droideka-roll')a.combat.arsenal.weapons[1].def.base='shield';
      for (let n = 0; n < 2; n++) {
        const tick = frame * 3 + n + 1;
        const actions = [...(tick % 19 === 1 ? ['firePrimary'] : []), ...(tick % 31 === 1 ? ['fireSecondary'] : []),
          ...(tick % 23 === 3 ? ['crouch'] : []), ...(tick % 41 === 7 ? ['prone'] : []), ...(tick % 43 === 9 ? ['jump'] : []), ...(scenario==='droideka-roll'&&[55,151].includes(tick)?['use']:[])];
        const c = {tick: frame === 26 ? authority - 1 : tick, sequence: ++sequence, epoch, generation: a.generation,
          down: tick % 10 < 7 ? ['KeyW', 'Mouse0'] : ['KeyD'], actions,
          look: [[tick % 4 - 2, .3]], mouseScale: frame % 4 ? undefined : 1.3, sent: tick % 13 !== 0};
        pending.push(c);
        for (const button of ['firePrimary', 'fireSecondary']) if (actions.includes(button)) edges.push({kind: 'fire', sequence: c.sequence, button, id: c.sequence * 7, pressed: true});
      }
      wa.begin(a,{epoch,tick:authority});
      const expected=frames[frame],result=session.predict(a,pending,{epoch,tick:authority,held:['KeyW'],fireEdges:edges,cosmeticSlots:expected.cosmeticSlots});
      assert.equal(result.tick,expected.tick,`frame ${frame} tick`);
      assert.equal(result.continued,expected.continued,`frame ${frame} continuation`);
      assert.equal(Buffer.from(session.correction()).toString('hex'),expected.correction,`frame ${frame} correction`);
      assert.equal(Buffer.from(capture(a)).toString('hex'),expected.correction,`frame ${frame} published checkpoint`);
      assert.deepEqual(encode(result.kick),expected.kick,`frame ${frame} kick`);
      assert.deepEqual(encode(arsenalObservation(a.combat.arsenal,a.combat.zoomStep)),expected.arsenal,`frame ${frame} arsenal`);
      const actualShots=[];
      session.present(a,null,(notice,w)=>actualShots.push(shotValue({yaw:notice.yaw,pitch:notice.pitch,charge:notice.charge,targetId:notice.target,targetGeneration:notice.targetGeneration,swing:notice.swing,[notice.kind===1?'repair':notice.kind===2?'meleeStart':notice.kind===3?'meleeHit':notice.kind===4?'meleeEnd':'round']:true},notice.tick,notice.sequence,notice.press,notice.held,notice.button,w.level)));
      assert.deepEqual(encode(actualShots),expected.shots,`frame ${frame} shots`);
      assert.equal(Buffer.from(capture(a)).toString('hex'),expected.correction,`frame ${frame} presentation keeps checkpoint`);
      wa.restore();
    }
    assert.equal(session.counters.calls,frames.length);
  } finally {wa.dispose();session.dispose();course.dispose();}
});

test('session crowd facts move only the own body and callback failure restores the final pose', async () => {
  const instance=await loadSim(),rays=await loadRayKernel(instance),course=actorCourse(rays),unit=fixture('soldier',course);
  const session=createClientPrediction(instance,course.world),weapons=new PredictionArsenal();
  unit.soldier.position.x=1;unit.soldier.settled.copy(unit.soldier.position);
  const other={position:unit.soldier.position.clone().set(0,0,0),hitPosture:'stand'};
  try {
    weapons.begin(unit,{tick:0});
    const command={tick:1,generation:unit.generation,sequence:1,down:[],actions:[],look:[]};
    const result=session.predict(unit,[command],{epoch:1,tick:0,crowd:[other]});
    const bytes=session.correction();
    const share=((1.14-1)/(Math.PI*.57*.57*1.65+Math.PI*.57*.57*1.65))*(Math.PI*.57*.57*1.65);
    assert.equal(unit.soldier.position.x,1+share);
    assert.deepEqual(other.position.toArray(),[0,0,0]);
    assert.equal(result.ticks,1);
    assert.throws(()=>session.present(unit,{beforeStep(){throw Error('presentation failure');}}),/presentation failure/);
    assert.deepEqual(capture(unit),bytes);
    session.present(unit,{previousYaw:0,beforeStep(){
      assert.throws(()=>session.predict(unit,[],{epoch:1,tick:0}),/busy/);
      instance.exports.memory.grow(1);
    },afterStep(){}});
    assert.deepEqual(capture(unit),bytes);
    session.dispose();session.dispose();assert.equal(session.handle,0);
    assert.throws(()=>session.predict(unit,[],{epoch:1,tick:0}),/disposed/);
  } finally {weapons.dispose();session.dispose();course.dispose();}
});

test('backend resets retain the cosmetic horizon without replaying an already shown shot', async () => {
  const instance = await loadSim(), rays = await loadRayKernel(instance), course = actorCourse(rays), unit = fixture('soldier', course);
  const session = createClientPrediction(instance, course.world), weapons = new PredictionArsenal();
  const base = capture(unit), command = { tick: 1, generation: unit.generation, sequence: 1, down: ['Mouse0'], actions: ['firePrimary'], look: [] };
  try {
    weapons.begin(unit, { tick: 0 });
    let result = session.predict(unit, [command], { epoch: 1, tick: 0 });
    assert.ok(result.shotsCount > 0);
    const expected = session.correction();
    weapons.restore(); restore(unit, base); session.reset();
    weapons.begin(unit, { tick: 0 });
    result = session.predict(unit, [command], { epoch: 1, tick: 0, presentedThrough: 1 });
    assert.equal(result.shotsCount, 0);
    assert.deepEqual(session.correction(), expected);
    const totals = Object.entries(session.transfers).reduce((out, [key, value]) => { out[key.endsWith('In') ? 'input' : 'output'] += value; return out; }, { input: 0, output: 0 });
    assert.equal(totals.input, session.counters.wordsIn);
    assert.equal(totals.output, session.counters.wordsOut);
  } finally { weapons.dispose(); session.dispose(); course.dispose(); }
});
