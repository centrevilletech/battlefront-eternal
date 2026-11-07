import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as T from 'three';
import {createHash} from 'node:crypto';
import {CameraFrame,openCameraWorld} from './helpers/cameras.js';
import {CameraOutput as O,CameraFrame as F,CameraLimits as L,VERSION} from '../src/presentation/layout.js';
const goldens=JSON.parse(fs.readFileSync(new URL('./fixtures/presentation/camera-goldens.json',import.meta.url)));
function digest(camera){
  const values=[...camera.position.toArray(),...camera.quaternion.toArray(),...camera.up.toArray(),camera.fov,camera.near,camera.far,...camera.matrixWorld.toArray(),...camera.matrixWorldInverse.toArray(),...camera.projectionMatrix.toArray(),...camera.projectionMatrixInverse.toArray()];
  return createHash('sha256').update(new Uint8Array(Float64Array.from(values).buffer)).digest('hex');
}
for(const sample of goldens.cases)test(`native camera phase matches frozen ${sample.responsive?'responsive':'eased'} records, motion, shake and death`,()=>{
  const h=new CameraFrame(openCameraWorld(),undefined,goldens.def);h.owner.view.responsive=sample.responsive;h.owner.shake.random=()=>.25;
  for(let i=0;i<sample.rows.length;i++){
    const r=sample.rows[i],s=h.state;s.position.fromArray(r.feet);s.posture=r.posture;s.air=r.air;s.dive=r.dive;s.aimYaw=r.yaw;s.aimPitch=r.pitch;s.velocity.set(2,r.air?-4:0,1);
    h.first=r.first;h.ownEyes=r.first&&!r.dead;h.combat.zoom=r.zoom;h.eye.fromArray(r.eye);h.death.active=r.dead;h.style=r.classic?'classic':'enhanced';
    if(r.dead){h.death.killerUnit=7;h.death.killerLife=1;h.death.killerCharacter=-1;h.death.killerHull=-1;h.killer.actor.root.position.set(4+i*.01,1.3,8);h.chest.position.set(r.feet[0],r.feet[1]+1.2,r.feet[2]);}
    if(i===70)h.owner.shake.add(2,.4);
    if(i===100){h.killer.actor.root.position.set(0,0,20.73);h.owner.shake.explode({Shake:2,ShakeLength:1,ShakeRadiusInner:10,ShakeRadiusOuter:30},[0,0,0],{position:h.killer.actor.root.position,radius:1.46});}
    const cue=h.frame(r.dt);assert.equal(cue,r.cue,`frame ${i}: same-frame arms cue`);
    assert.equal(digest(h.camera),r.pre,`frame ${i}: pre-shake records`);h.finish();
    assert.equal(digest(h.camera),r.bits,`frame ${i}: final records`);
  }
});
test('an oversized paused camera command burst is bounded and the live frame survives',()=>{
  const h=new CameraFrame(),owner=h.owner;
  for(let i=0;i<L.COMMANDS+300;i++)owner.shake.add(1,.5);
  h.frame(0,true);h.finish();assert.equal(owner.output[O.STATUS],0);assert.ok(owner.output[O.REFUSED]>0);
  owner.input[F.COMMANDS]=1e30;owner.input[F.VERSION]=VERSION;
  owner.x.camera_frame(owner.handle);assert.equal(owner.output[O.STATUS],0);assert.ok(owner.output[O.SKIPPED]>0);
  h.frame(1/60);h.finish();assert.ok(h.camera.matrixWorld.elements.every(Number.isFinite));
});
test('unknown camera owner and excess cold owners cannot poison an admitted frame',()=>{
  const h=new CameraFrame(),x=h.owner.x;
  x.camera_frame(0xffffffff);assert.equal(x.camera_input_ptr(0xffffffff),0);
  const bytes=new TextEncoder().encode('{"definitions":[],"motions":[]}'),ptr=x.sim_scratch(bytes.length);new Uint8Array(x.memory.buffer,ptr,bytes.length).set(bytes);
  assert.ok(x.camera_init(ptr,bytes.length));assert.equal(x.camera_init(ptr,bytes.length),0);
  h.frame(1/60);h.finish();assert.equal(h.owner.output[O.STATUS],0);
});

test('a distant death shot retains the frozen two-argument radius rounding',()=>{
  const fixture=JSON.parse(fs.readFileSync(new URL('./fixtures/presentation/death-far-golden.json',import.meta.url))),initial=fixture.initial;
  const camera=new T.PerspectiveCamera(initial.fov,initial.aspect,initial.near,initial.far);
  const h=new CameraFrame(openCameraWorld(),camera);
  camera.position.fromArray(initial.position);camera.quaternion.fromArray(initial.quaternion);
  h.feet.fromArray(fixture.feet);h.death.active=true;h.death.hfov=fixture.hfov;
  // DeathShot.begin's two-component hypot can differ from the same call
  // with a third zero. Check complete phase output, including projection.
  for(let i=0;i<fixture.rows.length;i++){
    const row=fixture.rows[i];h.chest.position.fromArray(row.chest);h.frame(row.dt);h.finish();
    assert.equal(digest(camera),row.sha256,`distant death frame ${i}`);
  }
});
