import test from 'node:test';
import assert from 'node:assert/strict';
import { EntityTimeline } from '../src/runtime/network-entity-timeline.js';
import { EntityReceiver } from '../src/runtime/network-entity-stream.js';
import { encodeEntityPacket, ENTITY_SOLDIER_STATES } from '../src/runtime/network-entity-codec.js';

const soldier = (id, tick, x, extra = {}) => ({ kind: 'soldier', id, generation: 1, tick,
  position: [x, 0, 0], velocity: [6, 0, 0], yaw: 0, pitch: 0, health: 100, ...extra });
const push = (timeline, records, epoch = 1) => timeline.push({ epoch, records });

test('remote interpolation uses each entity sample pair, not another entity packet tick', () => {
  const t = new EntityTimeline({extrapolateTicks:6});
  push(t, [soldier(1, 10, 0), soldier(2, 10, 100)]);
  push(t, [soldier(1, 20, 1)]);
  push(t, [soldier(2, 40, 103)]);
  assert.equal(t.sample(1, 15).position[0], .5);
  assert.equal(t.sample(2, 15).position[0], 100.5);
  assert.equal(t.sample(1, 100).position[0], 1.6, 'explicit six-tick limit remains supported');
  assert.equal(t.sample(1, 5).position[0], 0, 'no backwards extrapolation before first sample');
});

test('life changes, removal and epochs cannot blend unrelated bodies or resurrect stale ones', () => {
  const t = new EntityTimeline();
  push(t, [soldier(1, 10, 0)]);
  push(t, [{ kind: 'tombstone', id: 1, generation: 1, tick: 20 }]);
  push(t, [soldier(1, 30, 3)]);
  assert.equal(t.sample(1, 30), null);
  push(t, [soldier(1, 40, 50, { generation: 2 })]);
  assert.equal(t.sample(1, 35).position[0], 50);
  push(t, [soldier(1, 35, -20)]);
  assert.equal(t.sample(1, 40).position[0], 50);
  push(t, [soldier(2, 1, 200)], 2);
  assert.equal(t.sample(1, 50), null);
  assert.equal(push(t, [soldier(1, 60, 70)], 1), false);
});

test('angle and quaternion interpolation take the short arc', () => {
  const t = new EntityTimeline();
  push(t, [soldier(1, 10, 0, { yaw: Math.PI - .1 })]);
  push(t, [soldier(1, 20, 0, { yaw: -Math.PI + .1 })]);
  assert.ok(Math.abs(t.sample(1, 15).yaw - Math.PI) < 1e-12);
  const v = (tick, quaternion) => ({ ...soldier(2, tick, 0), kind: 'vehicle', quaternion });
  push(t, [v(10, [0, 0, 0, 1])]); push(t, [v(20, [0, 0, 0, -1])]);
  assert.deepEqual(t.sample(2, 15).quaternion, [0, 0, 0, 1]);
});

test('timeline bounds storage, isolates records and ignores own corrections', () => {
  const t = new EntityTimeline({ maxEntities: 1, maxSamples: 3 });
  const first = soldier(1, 1, 0); push(t, [first]); first.position[0] = 1000;
  assert.equal(t.sample(1, 1).position[0], 0);
  push(t, [{ kind: 'correction', id: 2, generation: 1, tick: 200, payload: new Uint8Array() }]);
  for (let tick = 2; tick <= 20; tick++) push(t, [soldier(1, tick, tick)]);
  assert.equal(t.entities.get(1).samples.length, 3);
  const out = {}; assert.equal(t.sample(1, 20, out), out); out.position[0] = 1000;
  assert.equal(t.sample(1, 20).position[0], 20);
  assert.throws(() => push(t, [soldier(2, 20, 20)]), /capacity/);
});

test('timeline consumes validated receiver updates directly', () => {
  const receiver = new EntityReceiver(), timeline = new EntityTimeline();
  const record = soldier(1, 10, 4, { classId: 1, team: 1, posture: 0, state: 0, weapon: 0, fireCounter: 0 });
  const applied = receiver.apply(encodeEntityPacket({ epoch: 3, packetId: 1, tick: 10, records: [record] }));
  assert.equal(timeline.push(applied), true);
  assert.equal(timeline.epoch, 3);
  assert.equal(timeline.sample(1, 10).position[0], 4);
  assert.throws(() => timeline.push({ records: [] }), /invalid/);
});

test('LTE sample holes coast with braking and a hard travel bound; settled dead actors never coast',()=>{
  const t=new EntityTimeline();push(t,[soldier(1,10,0)]);
  assert.ok(Math.abs(t.sample(1,16).position[0]-.6)<1e-12);
  assert.ok(Math.abs(t.sample(1,22).position[0]-1.05)<1e-12);
  assert.equal(t.sample(1,22).pathVelocity[0],3);
  assert.ok(Math.abs(t.sample(1,28).position[0]-1.2)<1e-12);
  assert.equal(t.sample(1,28).pathVelocity[0],0);
  assert.ok(Math.abs(t.sample(1,1000).position[0]-1.2)<1e-12);
  push(t,[soldier(1,30,1,{health:0})]);
  assert.equal(t.sample(1,30).position[0],1);assert.equal(t.sample(1,100).position[0],1);
  assert.equal(t.sample(1,100).extrapolatedTicks,0);
});

test('a thrown corpse coasts through a missing sample with the same hard bound, then stops at landing',()=>{
 const t=new EntityTimeline(),thrown=ENTITY_SOLDIER_STATES.indexOf('thrownDead'),dead=ENTITY_SOLDIER_STATES.indexOf('dead');
 const flying=soldier(1,10,0,{health:0,state:thrown,position:[0,4,0],velocity:[6,-3,0]});
 push(t,[flying]);
 assert.ok(t.sample(1,16).position.every((n,i)=>Math.abs(n-[.6,3.7,0][i])<1e-12));
 assert.equal(t.sample(1,16).state,thrown);
 const stopped=t.sample(1,28);
 assert.ok(Math.abs(stopped.position[0]-1.2)<1e-12);
 assert.equal(stopped.position[1],3.4);
 assert.ok(stopped.pathVelocity.every(n=>n===0));
 assert.deepEqual(t.sample(1,1000).position,stopped.position,'missing landing cannot create unlimited flight');
 assert.deepEqual(flying.position,[0,4,0],'rendering never integrates authoritative physics');
 push(t,[soldier(1,30,2,{health:0,state:dead,velocity:[6,-3,0]})]);
 assert.deepEqual(t.sample(1,100).position,[2,0,0]);
 assert.equal(t.sample(1,100).state,dead);
 assert.equal(t.sample(1,100).extrapolatedTicks,0);
});

test('teleports and death do not blend, while speed-consistent long gaps remain continuous',()=>{
  const t=new EntityTimeline();push(t,[soldier(1,10,0),soldier(1,70,6)]);
  assert.equal(t.sample(1,40).position[0],3,'six metres over a second is ordinary running');
  push(t,[soldier(1,76,40)]);
  assert.equal(t.sample(1,73).position[0],6.3,'old segment coasts without interpolating through a teleport');
  assert.equal(t.sample(1,76).position[0],40);
  push(t,[soldier(1,80,40.4,{health:0})]);
  assert.equal(t.sample(1,78).position[0],40.2,'old segment coasts without interpolating the death transition');
  assert.equal(t.sample(1,100).position[0],40.4);
  push(t,[soldier(2,10,0,{velocity:[0,0,0]}),soldier(2,70,6,{velocity:[0,0,0]})]);
  assert.equal(t.sample(2,40).position[0],3,'a runner can start and stop within a long missing interval');
});
