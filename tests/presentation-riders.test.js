import './fixtures/native-anim.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import {Session} from 'node:inspector/promises';
import {UnitFrame as F} from '../src/presentation/layout.js';
import * as THREE from 'three';
import {Clip} from '../src/anim.js';
import {bodyHost} from './helpers/presentation.js';
import {EntitySeats} from '../src/runtime/entity-seats.js';
import {pageProps,fixtureVehicle,drawHull} from './helpers/page-presentation.js';

// A complete body phase, with an authored ground-to-saddle offset. The
// pelvis must meet the published saddle, rather than adding its height twice.
function rider() {
  const root=new THREE.Group(),dummy=new THREE.Bone(),pelvis=new THREE.Bone();
  dummy.name='DummyRoot';pelvis.name='bone_pelvis';dummy.position.y=2.1;pelvis.position.y=.5;
  root.add(dummy);dummy.add(pelvis);
  const data=Float32Array.from([0,0,0,1,0,2.1,0,0,0,0,1,0,2.1,0,0,0,0,1,0,.5,0,0,0,0,1,0,.5,0]);
  const clip=new Clip('human_ride',{frames:2,tracks:[0,1],offset:0,flags:0,travel:[0,0,0]},data,30);
  const host=bodyHost({root,bones:[dummy,pelvis],byName:{DummyRoot:dummy,bone_pelvis:pelvis}},new Map([['human_ride',clip]]));
  const hull=new THREE.Group(),saddle=new THREE.Object3D();hull.add(saddle);
  const vehicle={alive:true,aiTarget:{id:37,generation:1},root:hull,velocity:new THREE.Vector3(),speed:10,yawRate:0,age:0,hoverPhase:0,yaw:0,healthType:'animal',
    seats:[{lease:null,aimYaw:0,pitch:0,yawRate:0,view:{rider:{clip:'human_ride',anim:'ride',node:saddle,ninePose:false},turnRate:2,pitchLimits:[-.5,.5],yawLimits:null}}]};
  fixtureVehicle(vehicle);
  host.unit.seat={vehicle,index:0};
  return {host,vehicle,saddle,pelvis,root};
}
test('lease birth retains its pre-frame clip time and publishes the source body once',()=>{
  const {host,vehicle,saddle,pelvis}=rider();
  // A visible turn between these frames distinguishes the arrival sample
  // from the hull's age after its next display update.
  const clip=host.source.clips.get('human_ride');
  clip.data[21+1]=Math.SQRT1_2;clip.data[21+3]=Math.SQRT1_2;
  // Clip registration has already copied its authored bytes, so build a
  // fresh body owner from the changed resource before admitting the lease.
  const fresh=bodyHost(host.row.actor,host.source.clips),unit=fresh.unit;
  saddle.position.set(4,3,-2);vehicle.root.updateMatrixWorld(true);
  vehicle.seats[0].aimYaw=.4;vehicle.seats[0].pitch=.2;
  drawHull(vehicle);
  const seats=new EntitySeats({units:{byId:()=>unit},driver:{}});
  seats.setLease(null,vehicle,[unit.id,1,'pilot',unit.generation],lease=>{vehicle.seats[0].lease=lease;});
  vehicle.age=1/60;vehicle.seats[0].pitch=.5;
  fresh.begin(1/60);fresh.owner.run();
  assert.ok(pelvis.quaternion.angleTo(new THREE.Quaternion())<1e-8,'sampled the arrival frame');
  assert.deepEqual(unit.soldier.position.toArray(),[4,3,-2]);
  assert.equal(unit.soldier.aimYaw,.4);assert.equal(unit.soldier.aimPitch,.2);
  unit.soldier.position.x=20;
  fresh.begin(1/60);fresh.owner.run();
  assert.equal(unit.soldier.position.x,20,'later authority cannot be overwritten by the birth again');
});
test('the native rider phase keeps a posed pelvis on each animated saddle frame',()=>{
  const {host,vehicle,saddle,pelvis,root}=rider();
  for(let frame=0;frame<60;frame++) {
    if(frame===0)saddle.position.y=2.4;
    vehicle.position.y=Math.sin(frame*.1)*.04;
    host.begin(1/60);host.owner.run();
    const at=saddle.getWorldPosition(new THREE.Vector3()),body=pelvis.getWorldPosition(new THREE.Vector3());
    assert.ok(at.distanceTo(body)<1e-8);
  }
  assert.ok(root.position.y<1,'authored root height is not added above the saddle');
});
test('native enclosure survives corrections and hull death until the next body generation',()=>{
  const {host,vehicle,root}=rider();vehicle.seats[0].view.rider=null;
  const draw=()=>{root.visible=true;host.begin(1/60);host.owner.run();};
  draw();assert.equal(root.visible,false);
  vehicle.alive=false;host.unit.seat=null;draw();assert.equal(root.visible,false);
  draw();assert.equal(root.visible,false,'a correction cannot expose the enclosed corpse');
  host.unit.generation++;draw();assert.equal(root.visible,true,'a new life releases hidden crew state');
});

// A remote gunner follows every published drum frame, even without a new
// lease. Its source interpolation is reset alongside the drawn body.
test('remote turret replicas follow the current drum without another lease',async()=>{
  const {host}=rider(),root=new THREE.Group(),drum=new THREE.Object3D(),seat=new THREE.Object3D();
  root.add(drum);drum.add(seat);seat.position.z=-.5;root.updateMatrixWorld(true);
  const turret={rig:{root},drum,gun:null,seat,drumParentTurn:new THREE.Quaternion(),gunBind:null,up:new THREE.Vector3(0,1,0),yawFactor:1,pitchFactor:0,
    rider:{clip:'human_ride',ninePose:false},yaw:.3,pitch:.2,yawVel:.4,turnRate:2,gunYaw:0,gunPitch:0,alive:true,lease:null,humanGunner:null,aiTarget:{id:71,generation:1}};
  const phase=pageProps({turrets:[turret]});
  const reader=new EntitySeats({units:{byId:()=>host.unit}});
  reader.setLease(turret,null,[host.unit.id,0,'gunner',host.unit.generation],lease=>{turret.lease=lease;});
  turret.yaw=.8;turret.pitch=.6;
  phase.present(1/60,1);host.begin(1/60,null,false);host.owner.run();
  const born=host.row.actor.root.position.clone(),turn=host.row.actor.root.quaternion.clone();
  assert.ok(born.distanceTo(new THREE.Vector3(-Math.sin(.8)*.5,0,-Math.cos(.8)*.5))<1e-12);
  assert.ok(turn.angleTo(new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0,1,0),.8))<1e-7);
  turret.yaw=1.2;turret.pitch=-.3;turret.yawVel=-.8;
  phase.present(1/60,2);host.begin(1/60,null,false);host.owner.run();
  assert.ok(host.row.actor.root.position.distanceTo(born)>.01);
  assert.ok(host.unit.soldier.position.distanceTo(host.row.actor.root.position)<1e-12);
  assert.ok(host.row.actor.root.quaternion.angleTo(turn)>.1);
  assert.ok(Math.abs(seat.matrixWorld.elements[12]+Math.sin(1.2)*.5)<1e-12,'the live drum keeps turning');
  // Distant bodies keep their previous turn between animation samples,
  // although placeReplica still refreshes their source position each frame.
  const prior=host.row.actor.root.quaternion.clone();turret.yaw=1.4;
  phase.present(1/60,3);host.begin(1/60,null,false);
  host.owner.input[F.HAS_VIEW]=1;host.owner.input[F.CAMERA_POSITION]=1000;
  host.owner.run();
  assert.ok(host.row.actor.root.quaternion.angleTo(prior)<1e-7,'LOD holds the displayed turn');
  assert.ok(Math.abs(host.unit.soldier.position.x+Math.sin(1.4)*.5)<1e-12,'source still follows the drum');
  // Fractional angles exercise the full publication path after V8 has had
  // time to optimize it. Retained matrices and source records must allocate
  // nothing, including the interpolation reset for a remote gunner.
  const step=i=>{turret.yaw=.3+i%60*.001;phase.present(1/60,i);host.begin(1/60,null,false);host.owner.run();};
  const warm=async()=>{for(let batch=0;batch<80;batch++){for(let i=0;i<240;i++)step(i);await new Promise(resolve=>setTimeout(resolve,10));}};
  const session=new Session();session.connect();
  try{
    const options={samplingInterval:1,includeObjectsCollectedByMajorGC:true,includeObjectsCollectedByMinorGC:true};
    // Sampling can collect earlier fixtures' weak feedback maps and deopt
    // otherwise warm publication code. Clear those maps before warming this
    // retained body, and repeat after the profiler's own first window.
    await session.post('HeapProfiler.collectGarbage');await warm();
    await session.post('HeapProfiler.startSampling',options);
    for(let i=0;i<120;i++)step(i);
    await session.post('HeapProfiler.stopSampling');
    await session.post('HeapProfiler.collectGarbage');await warm();
    await session.post('HeapProfiler.startSampling',options);
    for(let i=0;i<480;i++)step(i);
    const {profile}=await session.post('HeapProfiler.stopSampling'),allocations=[];
    function visit(node){
      if(node.selfSize&&/\/src\/(presentation\/(units|props|tree)|soldier-presentation)\.js$/.test(node.callFrame.url))allocations.push({name:node.callFrame.functionName,line:node.callFrame.lineNumber+1,bytes:node.selfSize});
      for(const child of node.children)visit(child);
    }
    visit(profile.head);assert.deepEqual(allocations,[]);
  }finally{session.disconnect();}
  // The own Gunner moved only its drawn root. Its predicted position still
  // supplies regional lighting, which differs above the barge from the dock.
  turret.humanGunner={previewYaw:0,previewPitch:0,previewState:{yawVel:0}};
  host.unit.soldier.position.set(12,3,1);phase.present(1/60,4);
  host.begin(1/60,null,true);host.owner.run();
  assert.deepEqual(host.p.position.toArray(),[12,3,1]);
  assert.ok(Math.abs(host.row.actor.root.position.x+Math.sin(turret.yaw)*.5)<1e-12);
  phase.kernel.free();
});
