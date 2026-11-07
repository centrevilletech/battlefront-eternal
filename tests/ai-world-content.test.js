import test from 'node:test';
import assert from 'node:assert/strict';
import { Vector3 } from 'three';
import { classFacts,hullFacts,postFacts,propFacts } from '../src/sim/wasm/ai-world-content.js';
import { Heap } from '../src/sim/runtime.js';
import { AiCaptureLayout,AiCaptureMirror,putAiCapture,syncAiCapture,
  AiRawCaptureLayout,AiRawCaptureMirror,putAiRawCapture,syncAiRawCapture } from '../src/sim/gen/ai-phase.js';

test('cold AI class facts preserve scalar presence and raw bounds',()=>{
  const def={class:'soldier',MaxSpeed:0,MaxStrafeSpeed:null,MovingTurnOnly:false,HealthType:'person',ai:{role:0},bounds:[[0,-0,1],[2,3,4]]};
  const row=classFacts(def);
  assert.equal(row.def,def);assert.equal(row.name,'soldier');assert.equal(row.maxSpeed,0);assert.equal(row.maxStrafeSpeed,null);
  assert.equal(row.forwardSpeed,undefined);assert.equal(row.movingTurnOnly,false);assert.equal(row.aiRole,0);
  assert.deepEqual(row.bounds,[0,-0,1,2,3,4]);
});

test('cold hull facts keep raw targets distinct from cooked seats and inactive identities',()=>{
  const def={settings:{ForwardSpeed:'12',TurnRate:'2',AISizeType:'HOVER',Label:'vehicle'},seats:[]};
  const hull={def,name:'tank',base:'hover',maxHealth:100,speed:12,turnRate:2,
    bounds:{min:new Vector3(-1,0,-1),max:new Vector3(1,2,1)},
    seats:[{settings:{YawLimits:'-30 30'},weapons:[]},{settings:{TurnRate:'4'},weapons:[{}]}]};
  hull.aiTarget={classDef:def,kind:6,category:3};
  const row=hullFacts(hull);
  assert.equal(row.member,hull.aiTarget);assert.equal(row.policy.forwardSpeed,undefined);assert.equal(row.policy.sizeType,undefined);
  assert.equal(row.traversalSize,'HOVER');assert.equal(row.forwardSpeed,12);assert.ok(Number.isNaN(row.maxSpeed));
  assert.equal(row.seats[0].policy.forwardSpeed,12);assert.equal(row.seats[0].kind,6);assert.equal(row.seats[0].yaw,'-30 30');
  assert.equal(row.seats[1].kind,7);assert.equal(row.seats[1].policy.forwardSpeed,0);
  assert.equal(row.seats[1].policy.turnRate,4);assert.equal(row.seats[1].policy.pitchRate,4);assert.equal(row.seats[1].weapons,1);
  hull.f2Prepared=true;assert.equal(hullFacts(hull).member,null);
});


test('cold post facts keep absent paths distinct from present empty paths',()=>{
  const capture={center:[0,-0,2],half:[3,4,5]},raw={matrix:Array(16).fill(0),size:[6,7,8]};
  const post={name:'post',desc:{spawnPath:'empty',spawnPoints:[],capture:raw},ai:{atk:[1,2,3,4,5,6],def:[6,5,4,3,2,1],
    order:0,bans:34,valueBleed:0,spawnPath:{present:false,points:[]},allyPath:null,allyCount:0,classSpawnPoints:0,capture,radius:0,yaw:-0}};
  const row=postFacts(post);
  assert.equal(row.policy.post,post);assert.equal(row.order,0);assert.equal(row.valueBleed,0);
  assert.equal(row.spawnPresent,false);assert.equal(row.allyPresent,null);assert.equal(row.spawnPath,true);
  assert.equal(row.spawnPoints,0);assert.deepEqual(row.capture,capture);
  assert.deepEqual(row.rawCapture,{center:null,matrix:raw.matrix,size:raw.size,half:null});
  assert.ok(Object.is(row.yaw,-0));assert.equal(row.policy.attack,post.ai.atk);
  // Exercise the generated boundary as well as the cold fact object. These
  // authored arrays must not be serialized as Three vector properties.
  const heap=new Heap(),words=new Float64Array(AiCaptureLayout.words);
  putAiCapture(null,row.capture,words,0,heap);
  const restored=syncAiCapture(null,words,0,0,new AiCaptureMirror());
  assert.deepEqual([...restored.center],capture.center);assert.deepEqual([...restored.half],capture.half);
  const rawWords=new Float64Array(AiRawCaptureLayout.words);
  putAiRawCapture(null,row.rawCapture,rawWords,0,heap);
  const block=new Float64Array(rawWords.length+heap.words.length);block.set(rawWords);block.set(heap.words,rawWords.length);
  const restoredRaw=syncAiRawCapture(null,block,0,rawWords.length,new AiRawCaptureMirror());
  assert.equal(restoredRaw.center,null);assert.deepEqual([...restoredRaw.size],raw.size);
  assert.deepEqual(restoredRaw.matrix,raw.matrix);assert.equal(restoredRaw.half,null);
});

test('cold turret repair policy reads settings while its registry target reads the descriptor',()=>{
  const settings={MaxHealth:120,HealthType:'building',YawLimits:'-30 30'},desc={class:'gun',base:'armedbuilding',settings};
  const building={desc,team:2,weapon:{},aiTarget:{className:'gun',classDef:desc}};
  const handle={kind:'building',slot:2,generation:4,life:3},post={name:'cp'};
  const row=propFacts({object:building,entry:{handle}},post);
  assert.equal(row.target,building);assert.equal(row.member,building.aiTarget);assert.equal(row.owner,handle);assert.equal(row.post,post);
  assert.equal(row.policy.maxHealth,120);assert.equal(row.policy.healthType,'building');assert.equal(row.policy.base,undefined);
  assert.equal(row.targetPolicy.maxHealth,undefined);assert.equal(row.targetPolicy.base,'armedbuilding');
  assert.equal(row.armed,true);assert.equal(row.yaw,'-30 30');assert.equal(row.pitch,undefined);
});


test('cold inactive flyer seats use the same immutable flight configuration as activation',()=>{
  const def={base:'flyer',settings:{MinSpeed:'-5',MidSpeed:'20',MaxSpeed:'70',TurnRate:'1.25',PitchRate:'.75'},seats:[]};
  const hull={def,name:'fighter',base:'flyer',f2Prepared:true,maxHealth:100,
    bounds:{min:new Vector3(-1,0,-1),max:new Vector3(1,2,1)},seats:[{settings:{},weapons:[]}]};
  hull.aiTarget={classDef:def,kind:5,category:3};
  const row=hullFacts(hull),seat=row.seats[0].policy;
  assert.equal(hull.flightConfig,undefined);assert.equal(row.member,null);
  assert.equal(seat.minSpeed,-5);assert.equal(seat.maxSpeed,70);assert.equal(seat.turnRate,1.5);assert.equal(seat.pitchRate,.75);
});
