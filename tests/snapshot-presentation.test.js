import test from 'node:test';
import assert from 'node:assert/strict';
import {Session} from 'node:inspector/promises';
import {Group,Bone} from 'three';
import {bodyHost} from './helpers/presentation.js';
import {createPageNetwork} from '../src/runtime/page-network.js';
import {SnapshotPresentation} from '../src/runtime/snapshot-timeline.js';
import {pushTimeline,sampleTimeline} from './helpers/network-capture.js';

// The production body phase consumes the native ring directly. There is no
// JavaScript sampled record whose allocation or identity could mask a copy.
test('warm snapshot frames bind directly to the native body batch without allocation',async()=>{
  const root=new Group(),bone=new Bone();bone.name='DummyRoot';root.add(bone);
  const host=bodyHost({root,bones:[bone],byName:{DummyRoot:bone}}),network=createPageNetwork(),view=new SnapshotPresentation(network);
  const poses=new Map([[1,{kind:'soldier',id:1,generation:1,position:[.1,.2,.3],velocity:[0,0,0],yaw:.2,pitch:.1,health:100}]]);
  pushTimeline(network,100,poses);pushTimeline(network,110,poses);view.bindSoldier(1,1,host.p);
  const position=host.p.position,step=i=>{view.update(101+i%60*.01);host.begin(1/60);host.owner.run();};
  const session=new Session();session.connect();
  const warm=async()=>{for(let b=0;b<60;b++){for(let i=0;i<240;i++)step(i);await new Promise(r=>setTimeout(r,10));}};
  try{
    await session.post('HeapProfiler.collectGarbage');await warm();
    const options={samplingInterval:1,includeObjectsCollectedByMajorGC:true,includeObjectsCollectedByMinorGC:true};
    await session.post('HeapProfiler.startSampling',options);for(let i=0;i<240;i++)step(i);await session.post('HeapProfiler.stopSampling');
    await session.post('HeapProfiler.collectGarbage');await warm();
    await session.post('HeapProfiler.startSampling',options);for(let i=0;i<480;i++)step(i);
    const {profile}=await session.post('HeapProfiler.stopSampling'),allocations=[];
    function visit(n){if(n.selfSize&&/src\/(runtime\/(snapshot-timeline|page-network)|presentation\/units)\.js$/.test(n.callFrame.url))allocations.push([n.callFrame.functionName,n.selfSize]);for(const child of n.children)visit(child);}
    visit(profile.head);assert.deepEqual(allocations,[]);assert.equal(host.p.position,position);assert.deepEqual(position.toArray(),[.1,.2,.3]);
  }finally{session.disconnect();view.reset();network.dispose();}
});

// Battle teardown invalidates links before the cold reservation is recycled.
// Old callbacks cannot write into another battle's history through that owner.
test('closed battles reuse their native reservation and reject stale callbacks',()=>{
  let network=createPageNetwork();network.timelineSetup();const kernel=network.kernel;
  const poses=new Map([[7,{kind:'vehicle',id:7,generation:1,position:[1,2,3],velocity:[0,0,0],quaternion:[0,0,0,1]}]]);
  pushTimeline(network,1,poses,8);network.dispose();const bytes=network.memory.buffer.byteLength;
  for(let battle=0;battle<128;battle++){
    const closed=network;network=createPageNetwork();network.timelineSetup();assert.equal(network.kernel,kernel);
    assert.equal(sampleTimeline(network,7,1),null);closed.present(2);closed.remove(7);closed.reset(5);closed.dispose();
    pushTimeline(network,2,poses,1);assert.ok(sampleTimeline(network,7,2));network.dispose();assert.equal(network.memory.buffer.byteLength,bytes);
  }
});

// Reset invalidates native leases even if its first packet had no applicable
// gameplay data. Reusing the same body identity must admit a fresh link.
test('an epoch change clears display links before same-identity rebinding',()=>{
 const root=new Group(),bone=new Bone();bone.name='DummyRoot';root.add(bone);
 const host=bodyHost({root,bones:[bone],byName:{DummyRoot:bone}}),network=createPageNetwork(),view=new SnapshotPresentation(network);
 view.bindSoldier(1,1,host.p);network.reset(2);view.update(2);assert.equal(view.bindings.size,0);assert.equal(host.p.nativeNetwork,false);
 pushTimeline(network,3,new Map([[1,{id:1,generation:1,kind:'soldier',position:[3,2,1],velocity:[0,0,0],health:100,yaw:0,pitch:0}]]),2);
 view.bindSoldier(1,1,host.p);view.update(3);host.begin(1/60);host.owner.run();assert.deepEqual(host.p.position.toArray(),[3,2,1]);view.reset();network.dispose();
});

// The old binding map retired a mismatched sample until another semantic
// bind arrived. A newer bracket alone must not silently reactivate its body.
test('sampled life and class mismatches wait for a later semantic rebind',()=>{
 for(const mismatch of ['generation','kind']){
  const root=new Group(),bone=new Bone();bone.name='DummyRoot';root.add(bone);
  const host=bodyHost({root,bones:[bone],byName:{DummyRoot:bone}}),network=createPageNetwork(),view=new SnapshotPresentation(network);
  const generation=mismatch==='generation'?2:1;host.unit.generation=generation;host.state.position.set(100,0,0);
  const pose=(x,g,kind='soldier')=>({id:1,generation:g,kind,position:[x,0,0],velocity:[0,0,0],health:100,yaw:0,pitch:0});
  pushTimeline(network,10,new Map([[1,pose(10,1,mismatch==='kind'?'droideka':'soldier')]]));
  pushTimeline(network,20,new Map([[1,pose(20,generation)]]));pushTimeline(network,30,new Map([[1,pose(30,generation)]]));view.bindSoldier(1,generation,host.p);
  const draw=tick=>{view.update(tick);host.begin(1/60);host.owner.run();return host.p.position.x;};
  assert.equal(draw(15),100,mismatch);assert.equal(draw(25),100,'a bracket transition cannot revive a retired link');
  view.bindSoldier(1,generation,host.p);assert.equal(draw(25),25,'a semantic packet admits the same resource again');view.reset();network.dispose();
 }
});

// Reusing a physical resource for another receiver changes its native owner.
// Old sampling and teardown must neither retire nor unbind the new link.
test("a previous receiver cannot alter another receiver's resource links",()=>{
 const root=new Group(),bone=new Bone();bone.name='DummyRoot';root.add(bone);
 const host=bodyHost({root,bones:[bone],byName:{DummyRoot:bone}}),a=createPageNetwork(),b=createPageNetwork(),av=new SnapshotPresentation(a),bv=new SnapshotPresentation(b);
 const poses=x=>new Map([[1,{id:1,generation:1,kind:'soldier',position:[x,0,0],velocity:[0,0,0],health:100,yaw:0,pitch:0}]]);
 pushTimeline(a,1,poses(1));pushTimeline(b,1,poses(2));av.bindSoldier(1,1,host.p);bv.bindSoldier(1,1,host.p);
 bv.update(1);av.update(1);av.reset();host.begin(1/60);host.owner.run();assert.equal(host.p.position.x,2);assert.equal(host.p.nativeNetwork,true);
 bv.reset();a.dispose();b.dispose();
});
