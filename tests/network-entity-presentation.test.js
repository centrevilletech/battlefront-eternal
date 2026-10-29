import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { SoldierPresentation } from '../src/soldier-presentation.js';
import { Vehicles } from '../src/vehicles.js';
import { Unit } from '../src/units.js';
import { CollisionWorld } from '../src/collision.js';
import { EntityPresentation } from '../src/runtime/network-entity-presentation.js';
import { EntityTimeline } from '../src/runtime/network-entity-timeline.js';
import { EntityReceiver } from '../src/runtime/network-entity-stream.js';
import { encodeEntityPacket } from '../src/runtime/network-entity-codec.js';

function stream(options) {
  const receiver = new EntityReceiver(), timeline = new EntityTimeline(options); let packetId = 0;
  return { timeline, view: new EntityPresentation(timeline),
    push(records, tick, epoch = 1) { timeline.push(receiver.apply(encodeEntityPacket({ epoch, tick, packetId: ++packetId, records }))); } };
}
function actor() {
  const soldier = { position: new THREE.Vector3(90, 2, 3), velocity: new THREE.Vector3(), aimYaw: 2, aimPitch: .8, groundAdjustment: 0, posture: 'stand' };
  return { soldier, presentation: new SoldierPresentation(soldier) };
}
const infantry = (tick, x, extra = {}) => ({ kind: 'soldier', id: 1, generation: 1, tick, classId: 1, team: 1,
  position: [x, 0, 0], velocity: [6, 0, 0], yaw: 0, pitch: tick / 100, health: 100, posture: 0, state: 0, weapon: 1, fireCounter: 0, ...extra });

test('sparse remote samples drive actual soldier presentation independently of global alpha', () => {
  const s = stream(), { soldier, presentation } = actor();
  s.push([infantry(10, 1)], 10); s.push([infantry(16, 1.6)], 16);
  s.view.bindSoldier(1, 1, presentation);
  // Another actor's much newer packet cannot shift the first actor's samples.
  s.push([infantry(40, 20, { id: 2 })], 40);
  s.view.update(13);
  for (const alpha of [0, .5, 1]) {
    const state = presentation.update(1 / 60, alpha);
    assert.ok(Math.abs(state.position.x - 1.30078125) < .00001);
    assert.ok(Math.abs(state.aimPitch - .13) < .0001);
  }
  assert.deepEqual(soldier.position.toArray(), [90, 2, 3]);
  assert.equal(soldier.aimPitch, .8, 'remote drawing never mutates authoritative aim');
  let posedPitch;
  const unit = { soldier, presentation, actor: { root: new THREE.Object3D() }, poseClock: 0,
    presentGone: Unit.prototype.presentGone, shadowNear: true, animator: { update(dt, state) { posedPitch = state.aimPitch; } }, updateJetFlame() {} };
  Unit.prototype.present.call(unit, 1 / 60, 0, null, null);
  assert.ok(Math.abs(posedPitch - .13) < .0001, 'Unit.present must not replace sampled pitch with latest authoritative pitch');
  assert.equal(unit.actor.root.position.x, presentation.position.x);
  s.view.update(500); presentation.update(1 / 60, 0);
  assert.ok(Math.abs(presentation.position.x - 2.8015625) < .00001, 'tapered coast has a finite travel bound');
});

test('own player bypasses remote samples and lifecycle changes clear old render bindings', () => {
  const s = stream(), a = actor(), b = actor();
  s.push([infantry(10, 1)], 10); s.view.bindSoldier(1, 1, a.presentation);
  s.view.update(10); assert.ok(a.presentation.networkPose);
  s.view.update(10, { playerId: 1 }); assert.equal(a.presentation.networkPose, null);
  assert.equal(a.presentation.update(0, 1).position.x, 90, 'own presentation remains on the prediction path');
  s.push([{ kind: 'tombstone', id: 1, generation: 1, tick: 11 }], 11);
  s.view.update(11); assert.equal(a.presentation.networkPose, null);
  s.push([infantry(12, 4, { generation: 2 })], 12); s.view.update(12);
  assert.equal(s.view.bindings.size, 0, 'old life is not rebound to a new body automatically');
  s.view.bindSoldier(1, 2, b.presentation); s.view.update(12); assert.ok(b.presentation.networkPose);
  s.push([infantry(1, 9)], 1, 2); s.view.update(1);
  assert.equal(b.presentation.networkPose, null); assert.equal(s.view.bindings.size, 0);
});

function vehicle() {
  const box = (x, y, z) => {
    const geometry = new THREE.BoxGeometry(x, y, z);
    return { positions: geometry.attributes.position.array, indices: geometry.index.array, polygons: new Uint32Array(geometry.index.count / 3) };
  };
  const floor = box(100, 1, 100), hull = box(2, 2, 4);
  const collision = new CollisionWorld(floor.positions, floor.indices, floor.polygons);
  const definition = { model: 'tank', label: 'Tank', settings: { MaxHealth: 1000, ForwardSpeed: 20, SetAltitude: .5 }, seats: [{ name: 'BODY', settings: {}, weapons: [] }], explosion: null };
  const data = { models: { tank: { bones: [], parts: [] } }, collision: { tank: hull }, classes: { tank: definition },
    spawns: [{ name: 'pad', matrix: new THREE.Matrix4().toArray(), team: 1, post: 'cp', respawnTime: 2, classes: { 1: 'tank', 2: 'tank' } }] };
  const system = new Vehicles({ scene: new THREE.Scene(), data, buffer: new ArrayBuffer(0), indexBuffer: new ArrayBuffer(0), collision,
    texture: () => null, camera: new THREE.PerspectiveCamera(), posts: { posts: [{ desc: { name: 'cp' }, team: 1 }] }, ordnance: { explosion() {} }, effects: { play() {} } });
  return system.vehicles[0];
}

test('actual vehicle hull draws its entity quaternion without changing physics or collision', () => {
  const s = stream(), hull = vehicle(), authority = hull.position.clone(), bodyMatrix = hull.body.matrix.clone();
  const record = tick => ({ kind: 'vehicle', id: 2, generation: 1, tick, classId: 2, team: 1,
    position: [tick, 5, 0], velocity: [60, 0, 0], quaternion: new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 1, 0), tick / 10).toArray(), health: 1000, state: 0, power: 0 });
  s.push([record(10)], 10); s.push([record(16)], 16);
  s.view.bindVehicle(2, 1, hull); s.view.update(13);
  for (const alpha of [0, .7, 1]) {
    assert.equal(hull.present(alpha), true);
    assert.equal(hull.visual.x, 13);
    const expected = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 1, 0), 1.3);
    assert.ok(hull.root.quaternion.angleTo(expected) < .0001);
    assert.ok(hull.position.equals(authority)); assert.ok(hull.body.matrix.equals(bodyMatrix));
  }
  s.view.update(13, { vehicleId: 2 }); assert.equal(hull.networkPose, null, 'own predicted hull bypasses remote timeline');
});

test('presentation bindings have a hard actor limit and release render references', () => {
  const timeline = new EntityTimeline(), view = new EntityPresentation(timeline, { maxEntities: 1 });
  const a = actor(), b = actor(); view.bindSoldier(1, 1, a.presentation);
  assert.throws(() => view.bindSoldier(2, 1, b.presentation), /capacity/);
  view.reset(); assert.equal(a.presentation.networkPose, null); view.bindSoldier(2, 1, b.presentation);
  assert.equal(view.bindings.size, 1);
});

test('a late sample continues from the drawn position and converges instead of snapping', () => {
  const s = stream({extrapolateTicks:6}), { presentation } = actor();
  s.push([infantry(10, 1)], 10); s.view.bindSoldier(1, 1, presentation);
  // Held: extrapolated six ticks past its sample (6 m/s), then standing.
  s.view.update(40); const held = presentation.networkPose.position[0];
  assert.ok(Math.abs(held - 1.6) < 1e-5);
  // Thirty ticks later the next sample lands with the actor 2.4 m further on.
  s.push([infantry(40, 4)], 40);
  s.view.update(41); const continued = presentation.networkPose.position[0];
  assert.ok(Math.abs(continued - held) < .02, `drawn body continues from ${held}, not ${continued}`);
  let previous = continued;
  for (let tick = 42; tick <= 70; tick++) { s.view.update(tick); const x = presentation.networkPose.position[0]; assert.ok(x >= previous - 1e-6 && x - previous < .5, `converges forward in bounded steps (${previous} to ${x})`); previous = x; }
  s.view.update(110); assert.ok(Math.abs(presentation.networkPose.position[0] - 4.6) < .01, 'the offset has decayed onto the authoritative path');
  // A jump beyond the snap distance (a teleport, a respawn) is shown at once.
  s.push([infantry(110, 40)], 110); s.view.update(111);
  assert.ok(Math.abs(presentation.networkPose.position[0] - 40.1) < .01);
  assert.ok(Math.abs(s.timeline.sample(1, 111).position[0] - 40.1) < 1e-9, 'the timeline sample itself is never offset');
});

test('late remote aim resumes from the drawn direction and converges without a one-frame turn',()=>{
 const s=stream(),{soldier,presentation}=actor();
 s.push([infantry(10,1,{yaw:0,pitch:0})],10);s.view.bindSoldier(1,1,presentation);s.view.update(30);
 const before=presentation.update(1/60).aimYaw;
 s.push([infantry(30,3,{yaw:Math.PI/2,pitch:.8})],30);s.view.update(30);
 assert.equal(presentation.update(1/60).aimYaw,before,'packet arrival at a held clock must not turn the body');
 assert.equal(presentation.state.aimPitch,0);
 let previous=before;
 for(let tick=31;tick<=42;tick++){
  s.view.update(tick);const state=presentation.update(1/60);
  assert.ok(state.aimYaw>=previous&&state.aimYaw-previous<.4,'90-degree update blends in bounded frame steps');
  previous=state.aimYaw;
 }
 assert.ok(Math.abs(previous-Math.PI/2)<.06,'aim catches up within 200ms instead of retaining an offset');
 assert.ok(Math.abs(presentation.state.aimPitch-.8)<.03);
 assert.equal(soldier.aimYaw,2,'authoritative aim is never modified');
 assert.ok(Math.abs(s.timeline.sample(1,42).yaw-Math.PI/2)<.0001,'retained timeline has no presentation offset');
});

test('angular corrections take the short arc and do not delay a continuous bracketed turn',()=>{
 const s=stream(),{presentation}=actor();
 const turning=tick=>infantry(tick,tick/10,{yaw:(tick-10)*.03,pitch:(tick-10)*.01});
 s.push([turning(10),turning(16)],16);s.view.bindSoldier(1,1,presentation);s.view.update(15);
 s.push([turning(22)],22);s.view.update(17);
 assert.ok(Math.abs(presentation.networkPose.yaw-.21)<.0002);
 assert.ok(Math.abs(presentation.networkPose.pitch-.07)<.0002);
 // A held actor crossing +/-pi must turn 0.1 rad, never nearly a full turn.
 const w=stream(),a=actor();w.push([infantry(10,1,{yaw:Math.PI-.05})],10);w.view.bindSoldier(1,1,a.presentation);w.view.update(30);
 const old=a.presentation.networkPose.yaw;
 w.push([infantry(30,3,{yaw:-Math.PI+.05})],30);w.view.update(31);
 const delta=value=>Math.atan2(Math.sin(value-old),Math.cos(value-old));
 assert.ok(Math.abs(delta(a.presentation.networkPose.yaw))<.001);
 for(let tick=32;tick<=50;tick++){w.view.update(tick);const turn=delta(a.presentation.networkPose.yaw);assert.ok(turn>=-.001&&turn<.101);}
 assert.ok(delta(a.presentation.networkPose.yaw)>.09);
});

test('life and teleport transitions discard angular correction offsets',()=>{
 const s=stream(),{presentation}=actor();s.push([infantry(10,1)],10);s.view.bindSoldier(1,1,presentation);s.view.update(30);
 s.push([infantry(30,3,{yaw:1,pitch:.6})],30);s.view.update(30);
 assert.ok(s.view.bindings.get(1).aimOffset[0]<-.9);
 s.push([infantry(31,40,{yaw:2,pitch:-.4})],31);s.view.update(31);
 assert.ok(Math.abs(presentation.networkPose.yaw-2)<.0001);assert.ok(Math.abs(presentation.networkPose.pitch+.4)<.0001);
 s.push([infantry(32,40,{generation:2,yaw:-1,pitch:.2})],32);s.view.bindSoldier(1,2,presentation);s.view.update(32);
 assert.ok(Math.abs(presentation.networkPose.yaw+1)<.0001);
});

test('new brackets preserve preceding interpolated motion rather than snapping to a new slope',()=>{
  const s=stream(),{presentation}=actor();s.push([infantry(10,0),infantry(16,.6)],16);s.view.bindSoldier(1,1,presentation);
  s.view.update(15);const before=presentation.networkPose.position[0];
  s.push([infantry(28,2.4)],28);s.view.update(17);
  assert.ok(Math.abs(presentation.networkPose.position[0]-(before+.2))<.003,'new bracket preserves the previous interpolation slope');
  const raw=s.timeline.sample(1,17).position[0];assert.ok(raw-presentation.networkPose.position[0]>.04);
  s.view.update(27);assert.ok(Math.abs(presentation.networkPose.position[0]-s.timeline.sample(1,27).position[0])<.025,'correction converges rather than altering samples');
});

test('static wall bounds extended coasting without changing authoritative soldier or timeline',()=>{
  const s=stream(),{soldier,presentation}=actor(),wall=new THREE.BoxGeometry(.1,3,10);wall.translate(.85,1,0);
  soldier.world=new CollisionWorld(wall.attributes.position.array,wall.index.array,new Uint32Array(wall.index.count/3));
  const authority=soldier.position.clone();s.push([infantry(10,0)],10);s.view.bindSoldier(1,1,presentation);
  for(let tick=11;tick<=34;tick++){s.view.update(tick);assert.ok(presentation.networkPose.position[0]<=.801,'visual centre cannot coast through wall');}
  assert.ok(presentation.networkPose.position[0]>.7);
  assert.ok(s.timeline.sample(1,34).position[0]>1,'collision clamp does not mutate retained samples');
  assert.ok(soldier.position.equals(authority));
});

test('same-tick arrivals smooth and death or teleport immediately clears prior visual offsets',()=>{
  const s=stream(),{presentation}=actor();s.push([infantry(10,0)],10);s.view.bindSoldier(1,1,presentation);
  s.view.update(25);const before=presentation.networkPose.position[0];
  s.push([infantry(25,2)],25);s.view.update(25);
  assert.equal(presentation.networkPose.position[0],before,'new packet cannot jump when playout clock is unchanged');
  s.push([infantry(26,2.1,{health:0})],26);s.view.update(26);
  assert.equal(presentation.networkPose.position[0],s.timeline.sample(1,26).position[0]);
  s.push([infantry(30,40)],30);s.view.update(30);
  assert.equal(presentation.networkPose.position[0],40,'teleport never inherits smoothing offset');
});

test('alternating 12/24-tick arrivals avoid mid-run freezes with a six-tick render delay',()=>{
  function replay(extrapolateTicks){
    const s=stream({extrapolateTicks}),{presentation}=actor();let next=1,count=0,previous,freezes=0,largestStep=0;
    for(let wall=1;wall<=241;wall++){
      if(wall===next){s.push([infantry(wall,wall/10)],wall);s.view.bindSoldier(1,1,presentation);next+=count++%2?24:12;}
      s.view.update(wall-6);const x=presentation.networkPose.position[0];
      if(wall>7){const step=Math.abs(x-previous);if(step<1e-6)freezes++;largestStep=Math.max(largestStep,step);}
      previous=x;
    }
    return {freezes,largestStep};
  }
  const previous=replay(6),bounded=replay(18);
  assert.ok(previous.freezes>=10,'fixture reproduces hard holds with the former six-tick bound');
  assert.equal(bounded.freezes,0,'bounded braking spans the measured sample holes');
  assert.ok(bounded.largestStep<.2&&bounded.largestStep<previous.largestStep,'late arrivals also make smaller frame steps');
});

for(const transition of [{name:'death',x:2,health:0},{name:'teleport',x:40,health:100}])test(`future ${transition.name} cannot rewind a coasting body before its render tick`,()=>{
  const s=stream(),{presentation}=actor();s.push([infantry(10,0)],10);s.view.bindSoldier(1,1,presentation);
  s.view.update(25);const before=presentation.networkPose.position[0];
  const expected=s.timeline.sample(1,26);
  s.push([infantry(30,transition.x,{health:transition.health})],30);
  const retained=s.timeline.sample(1,26);
  assert.deepEqual(retained.position,expected.position,'future sample leaves the preceding segment position intact');
  assert.deepEqual(retained.pathVelocity,expected.pathVelocity,'future sample leaves its braking velocity intact');
  s.view.update(26);assert.ok(presentation.networkPose.position[0]>=before,'body cannot snap backwards on packet arrival');
  assert.deepEqual(presentation.networkPose.position,expected.position);
  s.view.update(29);assert.ok(presentation.networkPose.position[0]<=1.201,'old segment still stops at its coast bound');
  s.view.update(30);assert.equal(presentation.networkPose.position[0],transition.x,'transition is shown at its actual render tick without smoothing across it');
});
