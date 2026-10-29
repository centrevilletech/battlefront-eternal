import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as THREE from 'three';
import { Ordnance } from '../src/ordnance.js';
import { RemoteDroids } from '../src/remote.js';

const collision = { cast: () => null, gather: () => [] };
function setup() {
  const shots = [], blasts = [];
  const remote = new RemoteDroids({ scene: new THREE.Scene(), camera: new THREE.PerspectiveCamera(), collision,
    lights: { flash() {} }, model: () => () => new THREE.Group(),
    fire: (def, at, dir) => shots.push({ def, at: at.clone(), dir: dir.clone() }), explode: (...args) => blasts.push(args) });
  remote.aimCamera = new THREE.PerspectiveCamera();
  return { remote, shots, blasts };
}
const fixture = { SwitchImmediately: 1, droid: { GeometryName: 'probe', weapons: [], EyePointOffset: [0, .5, 0] } };
const launch = (remote, def = fixture) => remote.launch({ def }, new THREE.Vector3(40, 2, 70), new THREE.Vector3(), .4, 1);

test('use hands control back and the surviving droid can be flown again', () => {
  const { remote } = setup(), d = launch(remote);
  remote.update(0, {}, { take: key => key === 'use', held: () => false });
  assert.equal(remote.active, false); assert.equal(d.dead, false);
  remote.enter(d); assert.equal(remote.active, true);
  remote.kill(d); assert.equal(remote.active, false);
});

test('hover is cosmetic, hides the hull in its eye view, and restores Classic and exit visibility', () => {
  const { remote } = setup(), d = launch(remote), position = d.position.clone();
  remote.present(.5, true, false);
  assert.notEqual(d.mesh.position.y, position.y); assert.deepEqual(d.position, position);
  remote.present(0, true, true); assert.equal(d.mesh.visible, false);
  remote.exit(); assert.equal(d.mesh.visible, true);
  remote.present(0, false, false); assert.deepEqual(d.mesh.position, position); assert.equal(d.mesh.rotation.z, 0);
});

for (const side of ['rep', 'cis', 'all', 'imp']) {
  const path = `public/sides/${side}/units.json`;
  test(`${side}: all imported recon loadouts take over and send four bolts down from the sky`, { skip: !fs.existsSync(path) }, () => {
    const units = JSON.parse(fs.readFileSync(path)).units;
    let checked = 0;
    for (const unit of Object.values(units)) for (const weapon of unit.weapons || []) {
      if (!weapon.droid) continue;
      checked++;
      const { remote, shots, blasts } = setup(), d = launch(remote, weapon);
      assert.equal(remote.piloted, d);
      remote.aim(d, false); remote.updateCamera(1 / 60, false);
      assert.ok(remote.camera.position.distanceTo(d.position) < 5);
      remote.zoomStep = 1;
      assert.equal(remote.zoom, 2.5);
      remote.updateCamera(1 / 60, false);
      assert.ok(remote.camera.position.distanceTo(d.eye.clone().addScaledVector(d.aimDir, -.5)) < 1e-5);
      remote.zoomStep = 0;
      const destruct = d.arsenal.current(1).def;
      remote.shoot(d, 0, { take: key => key === 'fireSecondary', held: () => false });
      remote.shoot(d, 2.99, null); assert.equal(d.salvo, null);
      remote.shoot(d, .011, null); assert.ok(d.salvo);
      remote.updateStrikes(0); assert.equal(shots.length, 1);
      remote.updateStrikes(.749); assert.equal(shots.length, 1);
      remote.updateStrikes(.0011); assert.equal(shots.length, 2);
      remote.updateStrikes(1.5); assert.equal(shots.length, 4); assert.equal(d.dead, true);
      assert.equal(blasts.length, 1); assert.equal(remote.active, false);
      const impacts = [], effects = [];
      const ordnance = new Ordnance({ scene: new THREE.Scene(), texture: () => null,
        lights: { add() {}, flash() {} }, effects: { play: name => effects.push(name) },
        onExplode: (def, at) => impacts.push({ def, at: at.clone() }),
        collision: { cast(at, dir, reach) {
          const distance = -at.y / dir.y;
          return distance >= 0 && distance <= reach ? { distance, point: at.clone().addScaledVector(dir, distance), normal: new THREE.Vector3(0, 1, 0), terrain: true } : null;
        } } });
      for (const shot of shots) {
        ordnance.fire(shot.def, shot.at, shot.dir, 'player');
        assert.ok(shot.at.y > 500); assert.ok(shot.dir.y < -.9);
        assert.ok(shot.def.Velocity * shot.def.LifeSpan > destruct.AimDistance);
        assert.ok(shot.def.ExplosionImpact.Effect); assert.equal(shot.def.ExplosionImpact.Damage, 500);
      }
      for (let tick = 0; tick < 180; tick++) for (const bolt of ordnance.bolts) if (bolt.life > 0) ordnance.stepBolt(bolt, 1 / 60, false);
      assert.equal(impacts.length, 4, 'every sky bolt reaches the ground and explodes');
      assert.ok(impacts.every(hit => Math.abs(hit.at.y - .2) < 1e-5));
      assert.equal(effects.filter(name => name === 'atat_explosion').length, 4);
    }
    assert.ok(checked > 0);
  });
}

 test('concurrent owner contexts isolate controls/cameras and retain strike ownership',()=>{
 const {remote}=setup(),firstOwner={id:11},secondOwner={id:12};const shots=[],blasts=[];remote.fire=(def,at,dir,owner)=>shots.push(owner);remote.explode=(def,at,owner)=>blasts.push(owner);
 const options=()=>({camera:new THREE.PerspectiveCamera(),aimCamera:new THREE.PerspectiveCamera()});const a=remote.createContext(firstOwner,options()),b=remote.createContext(secondOwner,options());const da=launch(a),db=launch(b);
 assert.notEqual(a.camera,b.camera);assert.notEqual(a.aimCamera,b.aimCamera);assert.notEqual(a.view,b.view);assert.equal(remote.active,false);
 a.look(20,10,1,false);a.zoomStep=1;const before=db.yaw;a.fly(da,1/60,{forward:1});assert.equal(db.yaw,before);assert.equal(b.lookX,0);assert.equal(b.zoomStep,0);
 const def={SalvoCount:1,SalvoDelay:.1,AimDistance:10,strike:{},ScatterDistance:0};da.def={...da.def,explosion:{}};db.def={...db.def,explosion:{}};
 a.strike(da,def);b.strike(db,def);a.exit();a.owner={id:99};a.updateStrikes(0);b.updateStrikes(0);assert.deepEqual(shots,[firstOwner,secondOwner]);assert.deepEqual(blasts,[firstOwner,secondOwner]);assert.equal(a.active,false);assert.equal(b.active,false);
});

test('isolated context retains shared movement parity with single-player path',()=>{
 const {remote}=setup(),context=remote.createContext({id:12}),a=launch(remote),b=launch(context);const controls={take:()=>false,held:()=>false};
 for(let tick=0;tick<120;tick++){const input={forward:tick<60?1:-1,right:.3,turn:.1};remote.look(2,-1,.2,false);context.look(2,-1,.2,false);remote.update(1/60,input,controls);context.update(1/60,input,controls);assert.deepEqual(a.position.toArray(),b.position.toArray());assert.deepEqual(a.velocity.toArray(),b.velocity.toArray());assert.equal(a.yaw,b.yaw);assert.equal(a.pitch,b.pitch);}
});

test('headless owners keep separate random streams and finish outstanding strikes after death',()=>{
 const {remote}=setup(),shots=[],blasts=[];remote.fire=(def,at,dir,owner)=>shots.push({at:at.clone(),owner});remote.explode=(def,at,owner)=>blasts.push(owner);
 let drawsA=0,drawsB=0;const ownerA={id:1},ownerB={id:2},a=remote.createContext(ownerA,{random:()=>{drawsA++;return .1;}}),b=remote.createContext(ownerB,{random:()=>{drawsB++;return .8;}});
 const da=launch(a),db=launch(b);assert.equal(a.view,null);assert.ok(a.simulationView);assert.equal(a.camera,null);assert.equal(b.camera,null);
 da.def={...da.def,explosion:{}};db.def={...db.def,explosion:{}};const def={SalvoCount:2,SalvoDelay:.5,AimDistance:10,ScatterDistance:2,strike:{}};a.strike(da,def);b.strike(db,def);a.kill(da);assert.equal(a.active,false);assert.ok(da.salvo);assert.equal(b.active,true);
 const controls={take:()=>false,held:()=>false};a.update(0,null,controls);assert.equal(drawsA,4);assert.equal(drawsB,0);assert.equal(a.droids.length,1);b.update(0,null,controls);assert.equal(drawsB,4);
 a.update(.5,null,controls);b.update(.5,null,controls);assert.equal(shots.filter(s=>s.owner===ownerA).length,2);assert.equal(shots.filter(s=>s.owner===ownerB).length,2);assert.deepEqual(blasts,[ownerA,ownerB]);assert.equal(a.strikes.length,0);assert.equal(b.strikes.length,0);a.update(0,null,controls);assert.equal(a.droids.length,0);
});
