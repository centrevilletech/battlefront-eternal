import './fixtures/native-anim.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import {Session} from 'node:inspector/promises';
import {SnapshotTimeline,SnapshotPresentation} from '../src/runtime/snapshot-timeline.js';
import {animationKernel,setAnimKernel} from '../src/anim.js';
import {loadAnimKernel} from '../src/anim-kernel.js';

// Exercise the whole snapshot batch and its networkPose publication. Floating
// playout clocks expose boxed-number or temporary-record allocations that a
// paused, integer-only fixture would miss.
test('warm hull snapshot publication retains its JavaScript records',async()=>{
  const timeline=new SnapshotTimeline(),presentation=new SnapshotPresentation(timeline),poses=new Map(),targets=[];
  for(let id=1;id<=64;id++){
    const target={networkPose:null};targets.push(target);presentation.bindVehicle(id,1,target);
    poses.set(id,{kind:'vehicle',id,generation:1,position:[.1,.2,.3],velocity:[.4,.5,.6],quaternion:[0,.2,0,.9797958971132712],health:100});
  }
  timeline.addSnapshot(100,poses);timeline.addSnapshot(110,poses);
  const step=i=>presentation.update(101+i%60*.01);
  for(let batch=0;batch<40;batch++){
    for(let i=0;i<120;i++)step(i);
    await new Promise(resolve=>setImmediate(resolve));
  }
  const records=targets.map(target=>target.networkPose),positions=records.map(record=>record.position);
  const session=new Session();session.connect();
  try{
    const options={samplingInterval:1,includeObjectsCollectedByMajorGC:true,includeObjectsCollectedByMinorGC:true};
    await session.post('HeapProfiler.startSampling',options);
    for(let i=0;i<120;i++)step(i);
    await session.post('HeapProfiler.stopSampling');
    await session.post('HeapProfiler.startSampling',options);
    for(let i=0;i<120;i++)step(i);
    const {profile}=await session.post('HeapProfiler.stopSampling'),allocations=[];
    function visit(node){
      if(node.selfSize&&node.callFrame.url.endsWith('/src/runtime/snapshot-timeline.js'))allocations.push({name:node.callFrame.functionName,bytes:node.selfSize});
      for(const child of node.children)visit(child);
    }
    visit(profile.head);assert.deepEqual(allocations,[]);
    // Publishing advances values in the binding-owned sample, preserving the
    // references held by hulls and their networkPose readers.
    for(let i=0;i<targets.length;i++){
      assert.equal(targets[i].networkPose,records[i]);assert.equal(records[i].position,positions[i]);
      assert.equal(records[i].id,i+1);assert.deepEqual(records[i].position,[.1,.2,.3]);
    }
  }finally{session.disconnect();}
});

// Native histories reserve their full capacity once. Battle disposal must
// recycle that reservation, clear old epochs and reject a stale reader.
test('closed battles reuse one native snapshot reservation without old poses',()=>{
  let timeline=new SnapshotTimeline(),id=timeline.id;
  const poses=new Map([[7,{kind:'vehicle',id:7,generation:1,position:[1,2,3],velocity:[0,0,0],quaternion:[0,0,0,1]}]]);
  timeline.addSnapshot(1,poses,8);timeline.dispose();
  const bytes=timeline.kernel.memory.buffer.byteLength;
  for(let battle=0;battle<128;battle++){
    const closed=timeline;timeline=new SnapshotTimeline();assert.equal(timeline.id,id);
    assert.equal(timeline.sample(7,1),null);assert.equal(closed.sample(7,1),null);
    assert.equal(closed.addSnapshot(2,poses),false);closed.dispose();
    timeline.addSnapshot(2,poses,1);assert.ok(timeline.sample(7,2));timeline.dispose();
    assert.equal(timeline.kernel.memory.buffer.byteLength,bytes);
  }
});

test('snapshot reservations belong to their own native battle instance',async()=>{
  const first=animationKernel(),a=new SnapshotTimeline();a.dispose();
  try{
    const second=await loadAnimKernel();setAnimKernel(second);
    const b=new SnapshotTimeline();assert.equal(b.id,0);b.dispose();
    setAnimKernel(first);const c=new SnapshotTimeline();assert.equal(c.id,a.id);c.dispose();
  }finally{setAnimKernel(first);}
});

test('stale native snapshot handles never index outside their store',()=>{
  const api=animationKernel().x;
  assert.equal(api.timeline_input(0xffffffff),0);assert.equal(api.timeline_output(0xffffffff),0);
  assert.doesNotThrow(()=>{api.timeline_push(0xffffffff);api.timeline_present(0xffffffff);api.timeline_remove(0xffffffff,1);api.timeline_reset(0xffffffff,0);});
});
