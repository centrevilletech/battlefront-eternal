import assert from 'node:assert/strict';
import {Session} from 'node:inspector/promises';
import {Battle} from '../../src/battle.js';
import {CommandPosts} from '../../src/commandposts.js';
import {F4Live} from '../../src/f4-live.js';
import {F4Tail} from '../../src/f4-tail.js';
import {HumanSpawnMirror} from '../../src/sim/gen/index.js';
import {f3World} from './f3-world.js';

// F4Live.step/F4Tail.step keep their real publications and native counters
// active. Synchronous compiler warmup leaves dead-object sampling strict.
async function census() {
  const w=await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:100,weapons:[]}},
    {debug:false,prepareLive:()=>({buildings:[],animated:[],traps:[]})});
  const posts=new CommandPosts(null,{}, {headless:true,playerTeam:1});
  const battle=new Battle({posts,teams:{1:{units:30,reinforcements:100},2:{units:30,reinforcements:100}},own:1,foe:2});
  battle.populate(1);for(const c of battle.characters)c.className='soldier';
  const live=new F4Live(w.phase,{sound(){}});
  const tail=new F4Tail(w.phase,battle,{posts,human:new HumanSpawnMirror(),gameplay:w.ordnance.random});
  let tick=0;
  const step=()=>{live.step(1/60,++tick);tail.step(1/60,tick,true);};
  for(let batch=0;batch<40;batch++){
    for(let i=0;i<120;i++)step();
    await new Promise(resolve=>setImmediate(resolve));
  }
  const session=new Session();session.connect();
  try{
    const options={samplingInterval:1,includeObjectsCollectedByMajorGC:true,includeObjectsCollectedByMinorGC:true};
    await session.post('HeapProfiler.startSampling',options);
    for(let i=0;i<120;i++)step();
    await session.post('HeapProfiler.stopSampling');
    await session.post('HeapProfiler.startSampling',options);
    live.counters.reset();tail.counters.reset();
    for(let i=0;i<120;i++)step();
    const {profile}=await session.post('HeapProfiler.stopSampling'),allocations=[];
    function visit(node){
      if(node.selfSize&&/\/src\/f4-[^/]+\.js$/.test(node.callFrame.url))allocations.push({name:node.callFrame.functionName,bytes:node.selfSize});
      for(const child of node.children)visit(child);
    }
    visit(profile.head);assert.deepEqual(allocations,[]);
    for(const counters of [live.counters,tail.counters]){
      assert.equal(counters.phaseCalls,120);assert.equal(counters.rustAllocations,0);assert.equal(counters.rustReallocations,0);
    }
  }finally{session.disconnect();}
}
await census();
