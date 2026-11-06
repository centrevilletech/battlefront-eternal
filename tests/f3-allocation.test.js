import test from 'node:test';
import assert from 'node:assert/strict';
import {Session} from 'node:inspector/promises';
import {f3World} from './fixtures/f3-world.js';
import {weaponDefinition} from './fixtures/client-weapons.js';

// Exercise the owning phase and its real publication/import machinery after
// both V8 and native buffers are warm. Rare structural births have their own
// preparation fixtures; ordinary movement must retain all boundary storage.
test('warm F3 boundary creates no sampled JavaScript objects',async()=>{
  const w=await f3World({soldier:{
    class:'soldier',base:'soldier',MaxHealth:300,weapons:[weaponDefinition('rifle',0)],
  }},{debug:false});
  w.phase.debug=false;
  const unit=w.spawn('soldier',0,0);
  unit.intent.forward=.2;
  // The real aimer reads a weapon during the owning span. Include lazy row
  // observation, not just an empty arsenal, in the warmed allocation check.
  const center=unit.position.clone();center.x=1000;
  const out=[];let queryTick=0;
  // Empty query results must retain their member-array capacity too. The
  // aimer alternates between an empty sphere and one containing this body.
  unit.onMoved=()=>{
    void unit.combat.arsenal.current(0).wait;
    w.units.query(center,queryTick++%2?1:1500,out);
  };
  // Let background optimization finish between bounded movement batches.
  // Remaining on the fixture floor avoids testing an unrelated death birth.
  for(let batch=0;batch<200;batch++) {
    unit.intent.forward=batch%2 ? -.2 : .2;
    for(let i=0;i<120;i++)w.phase.phase(1/60);
    await new Promise(resolve=>setTimeout(resolve,10));
  }
  const session=new Session();session.connect();
  try {
    await session.post('HeapProfiler.startSampling',{samplingInterval:1,
      includeObjectsCollectedByMajorGC:true,includeObjectsCollectedByMinorGC:true});
    for(let i=0;i<120;i++)w.phase.phase(1/60);
    await session.post('HeapProfiler.stopSampling');
    // Sampling can trigger background code preparation. Let that work finish
    // before attributing the next interval to the phase's object allocations.
    await new Promise(resolve=>setTimeout(resolve,100));
    await session.post('HeapProfiler.startSampling',{samplingInterval:1,
      includeObjectsCollectedByMajorGC:true,includeObjectsCollectedByMinorGC:true});
    for(let i=0;i<120;i++)w.phase.phase(1/60);
    const {profile}=await session.post('HeapProfiler.stopSampling');
    const allocations=[];
    function visit(node) {
      if(node.selfSize && /\/src\/(?:f3-[^/]+|f2-delta|sim\/gen\/(?:combat|units))\.js$/.test(node.callFrame.url)) {
        allocations.push({name:node.callFrame.functionName,bytes:node.selfSize});
      }
      for(const child of node.children)visit(child);
    }
    visit(profile.head);assert.deepEqual(allocations,[]);
  } finally {session.disconnect();}
});
