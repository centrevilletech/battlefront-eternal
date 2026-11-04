import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { loadRayKernel } from '../src/ray-kernel.js';
import { createClientPrediction } from '../src/runtime/client-prediction.js';
import { InfantryPrediction } from '../src/runtime/infantry-prediction.js';
import { DroidekaPrediction } from '../src/runtime/droideka-prediction.js';
import { WeaponPrediction, predictedPress } from '../src/runtime/weapon-prediction.js';
import { PredictionWindow } from '../src/runtime/prediction-window.js';
import { replayTicks } from '../src/runtime/reconcile.js';
import { CommandControls } from '../src/runtime/commands.js';
import { captureCompactSoldier, restoreCompactSoldier } from '../src/runtime/network-entity-soldier-compact.js';
import { captureDroidekaState, restoreDroidekaState } from '../src/runtime/network-droideka-state.js';
import { actorCourse, createPredictionActor } from './fixtures/prediction-actor-cases.js';
import { createWeaponUnit, weaponDefinition, arsenalObservation } from './fixtures/prediction-weapon-cases.js';

function fixture(kind, course) {
  const actor = createPredictionActor({kind, pattern: kind === 'droideka' ? 'droideka' : 'walk'}, course);
  const unit = createWeaponUnit([weaponDefinition('burst', 0, {SalvoCount: 3, SalvoDelay: .04, ShotsPerSalvo: 3, RoundsPerClip: 80}),
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

for (const kind of ['soldier', 'droideka']) test(`whole ${kind} session matches JS bytes, arsenal and seeded notices across continuations and rollback`, async t => {
  const instance = await loadSim(), rays = await loadRayKernel(instance), nativeCourse = actorCourse(rays), jsCourse = actorCourse();
  const a = fixture(kind, nativeCourse), b = fixture(kind, jsCourse);
  const session = createClientPrediction(instance, nativeCourse.world), wa = new WeaponPrediction(), wb = new WeaponPrediction();
  const window = new PredictionWindow(), controls = new CommandControls();
  let baseA = capture(a), baseB = capture(b), authority = 0, pending = [], sequence = 0, epoch = 1;
  let kick = null, shots = 0, ticks = 0;
  const edges = [];
  try {
    for (let frame = 0; frame < 65; frame++) {
      restore(a, baseA); restore(b, baseB);
      if (frame === 25 || frame === 47) {
        // A newer authority invalidates the continuation and replays the same
        // still-pending stream, including commands stamped in the past.
        authority += 4;
      }
      if (frame === 35) { epoch++; a.generation++; b.generation++; pending = []; edges.length = 0; }
      for (let n = 0; n < 2; n++) {
        const tick = frame * 3 + n + 1;
        const actions = [...(tick % 19 === 1 ? ['firePrimary'] : []), ...(tick % 31 === 1 ? ['fireSecondary'] : []),
          ...(tick % 23 === 3 ? ['crouch'] : []), ...(tick % 41 === 7 ? ['prone'] : []), ...(tick % 43 === 9 ? ['jump'] : [])];
        const c = {tick: frame === 26 ? authority - 1 : tick, sequence: ++sequence, epoch, generation: a.generation,
          down: tick % 10 < 7 ? ['KeyW', 'Mouse0'] : ['KeyD'], actions,
          look: [[tick % 4 - 2, .3]], mouseScale: frame % 4 ? undefined : 1.3, sent: tick % 13 !== 0};
        pending.push(c);
        for (const button of ['firePrimary', 'fireSecondary']) if (actions.includes(button)) edges.push({kind: 'fire', sequence: c.sequence, button, id: c.sequence * 7, pressed: true});
      }
      const context = {source: b.soldier, identity: b.id, epoch, generation: b.generation, authorityTick: authority, pending, held: ['KeyW']};
      const continuation = window.select(context);
      if (continuation.checkpoint) restore(b, continuation.checkpoint.state);
      wb.begin(b, {epoch, tick: authority, checkpoint: continuation.checkpoint?.weapons});
      wa.begin(a, {epoch, tick: authority});
      const actor = new (kind === 'soldier' ? InfantryPrediction : DroidekaPrediction)(b.soldier, b.combat, continuation.checkpoint?.kick ?? b.combat.kick);
      const expectedShots = [];
      const last = replayTicks(continuation.commands, continuation.from, b.generation, continuation.held, (tick, c, sequences) => {
        for (const [dx, dy] of c.look) (b.soldier.lookRaw || b.soldier.look).call(b.soldier, Math.max(-10000,Math.min(10000,dx)) / b.combat.zoom,
          Math.max(-10000,Math.min(10000,dy)) / b.combat.zoom, Math.min(c.mouseScale ?? 1, 2), c.invertMouse || false);
        controls.down.clear(); for (const key of c.down) controls.down.add(key);
        actor.step({forward:controls.axis('forward'),right:controls.axis('right'),turn:controls.axis('turn'),
          jump:c.actions.includes('jump'),crouch:c.actions.includes('crouch'),prone:c.actions.includes('prone'),use:c.actions.includes('use')},1/60,c.actions.includes('fireSecondary'));
        const presses = ['firePrimary','fireSecondary'].map(name => c.actions.includes(name) ? predictedPress(edges,sequences,name) : null);
        wb.step(c,tick,s => {
          const button=s.weapon.channel, press=presses[button]; if(press===false)return;
          const source=press?pending.find(p=>p.sequence===press.sequence):pending.find(p=>p.tick===tick);
          expectedShots.push(shotValue(s,tick,source?.sequence??press?.sequence??0,press?.id||0,!press,button,s.weapon.level));
        },presses.map(p=>p?p.id:null));
        const [lo,hi]=b.soldier.pitchRange();b.soldier.aimPitch=Math.max(lo,Math.min(hi,b.soldier.aimPitch));
      },{});
      const state=capture(b);window.save({...context,tick:last,commands:continuation.commands,held:continuation.held,state});
      window.checkpoint.kick=actor.kick;window.checkpoint.weapons=wb.checkpoint();kick=actor.kick;
      const result=session.predict(a,pending,{epoch,tick:authority,held:['KeyW'],fireEdges:edges});
      assert.equal(result.tick,last,`frame ${frame} tick`);
      assert.equal(result.continued,Boolean(continuation.checkpoint),`frame ${frame} continuation`);
      assert.deepEqual(session.correction(),state,`frame ${frame} correction`);
      assert.deepEqual(capture(a),state,`frame ${frame} published body`);
      assert.deepEqual(result.kick,[kick.rate,kick.hold,kick.offset],`frame ${frame} recoil`);
      assert.deepEqual(arsenalObservation(a.combat.arsenal,a.combat.zoomStep),arsenalObservation(b.combat.arsenal,b.combat.zoomStep),`frame ${frame} arsenal`);
      const actualShots=[];
      session.present(a,null,(notice,w)=>actualShots.push(shotValue({yaw:notice.yaw,pitch:notice.pitch,charge:notice.charge,targetId:notice.target,targetGeneration:notice.targetGeneration},
        notice.tick,notice.sequence,notice.press,notice.held,notice.button,w.level)));
      assert.deepEqual(actualShots,expectedShots,`frame ${frame} notices`);
      shots+=actualShots.length;ticks+=result.ticks;
      actor.dispose();wa.restore();wb.restore();
    }
    assert.equal(session.counters.calls,65);assert.ok(ticks>150);assert.ok(shots>10);
    assert.ok(session.counters.wordsIn>0&&session.counters.wordsOut>0);
    t.diagnostic(JSON.stringify({kind,shots,ticks,...session.counters}));
  } finally {wa.dispose();wb.dispose();session.dispose();session.dispose();nativeCourse.dispose();jsCourse.dispose();}
});

test('session crowd facts move only the own body and callback failure restores the final pose', async () => {
  const instance=await loadSim(),rays=await loadRayKernel(instance),course=actorCourse(rays),unit=fixture('soldier',course);
  const session=createClientPrediction(instance,course.world),weapons=new WeaponPrediction();
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
  const session = createClientPrediction(instance, course.world), weapons = new WeaponPrediction();
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
