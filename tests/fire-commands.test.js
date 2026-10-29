import test from 'node:test';
import assert from 'node:assert/strict';
import { FireCommands } from '../src/runtime/fire-commands.js';
const edge=(id,pressed,extra={})=>({epoch:1,id,pressed,tick:100,generation:2,button:0,...extra});
test('a short press and release survive a single delivery batch, exactly once',()=>{
 const fire=new FireCommands();assert(fire.push(edge(1,true),100,2));assert(fire.push(edge(2,false),100,2));assert(!fire.push(edge(1,true),100,2));
 assert.deepEqual(fire.take(100,2).buttons,[1,0]);assert.deepEqual(fire.take(101,2).buttons,[0,0]);assert.equal(fire.take(102,2).shots.length,0);
});
test('old generations, stale presses, future claims and invalid aim are rejected',()=>{
 const fire=new FireCommands();assert(!fire.push(edge(1,true,{generation:1}),100,2));assert(!fire.push(edge(1,true,{tick:69}),100,2));assert(!fire.push(edge(1,true,{tick:221}),100,2));assert(!fire.push(edge(1,true,{aimYaw:Infinity}),100,2));assert(!fire.push(edge(1,true,{viewTick:103}),100,2));assert(fire.push(edge(1,true,{viewTick:90,aimYaw:7,aimPitch:0.3}),100,2));assert.equal(fire.take(100,2).shots[0].viewTick,90);fire.clear();assert.deepEqual(fire.take(101,2).buttons,[0,0]);
});
test('edge queues are bounded and respawn clears held fire',()=>{
 const fire=new FireCommands();for(let id=1;id<=32;id++)assert(fire.push(edge(id,Boolean(id%2),{tick:120}),100,2));assert(!fire.push(edge(33,true,{tick:120}),100,2));fire.take(120,2);assert.deepEqual(fire.take(121,3).buttons,[0,0]);assert.equal(fire.queue.length,0);
});

test('future presses retain reliable order ahead of earlier-tick releases',()=>{
 const fire=new FireCommands();
 assert(fire.push({epoch:1,id:1,tick:15,generation:1,button:0,pressed:true},10,1));
 assert(fire.push({epoch:1,id:2,tick:12,generation:1,button:0,pressed:false},10,1));
 assert.equal(fire.take(12,1).buttons[0],0);
 assert.equal(fire.take(15,1).buttons[0],1);
 assert.equal(fire.take(16,1).buttons[0],0);
});

test('newer movement releases a stalled trigger without losing a delayed short click',()=>{
 const fire=new FireCommands();
 assert(fire.push(edge(1,true),100,2));
 assert.equal(fire.take(100,2,{tick:99,buttons:[false,false]}).shots.length,1);
 assert.equal(fire.take(101,2,{tick:99,buttons:[false,false]}).buttons[0],1,'state from before press cannot release it');
 assert.equal(fire.take(102,2,{tick:101,buttons:[true,false]}).buttons[0],1);
 assert.equal(fire.take(103,2,{tick:102,buttons:[false,false]}).buttons[0],0,'fresh movement recovers missing reliable release');
 assert(fire.push(edge(2,false),104,2));
 assert(fire.push(edge(3,true,{tick:103}),104,2));
 fire.take(104,2,{tick:104,buttons:[false,false]});
 const click=fire.take(105,2,{tick:104,buttons:[false,false]});
 assert.equal(click.shots.length,1);assert.equal(click.buttons[0],1,'a late complete click still fires once');
 assert.equal(fire.take(106,2,{tick:104,buttons:[false,false]}).buttons[0],0);
 assert.equal(fire.take(107,2,{tick:107,buttons:[true,false]}).buttons[0],0,'held state cannot invent a new press');
});

test('external shot identity preserves cosmetic dedup without replacing internal consumption ID',()=>{
 const fire=new FireCommands().enableOutcomes();
 assert(!fire.push(edge(1,true,{externalId:-1}),100,2));
 assert(fire.push(edge(1,true,{externalId:9}),100,2));
 assert.equal(fire.take(100,2).shots[0].id,9);
 assert.deepEqual(fire.drainOutcomes(),[{id:1,status:'consumed'}]);
 assert(!fire.push(edge(1,true,{externalId:10}),100,2),'external identity cannot bypass internal ordering');
});

test('trigger edges: a click inside one command, a hold across commands, and a held trigger\'s rounds', async () => {
  const { FireEdges } = await import('../src/runtime/fire-commands.js');
  const edges = new FireEdges(), seen = [], held = [];
  const run = (command) => edges.each({ epoch: 1, generation: 1, ...command }, (button, pressed) => { seen.push([button, pressed]); }, (button) => held.push(button));
  run({ down: [], actions: ['firePrimary'] });
  assert.deepEqual(seen.splice(0), [[0, true], [0, false]], 'a click shorter than a command is a press and a release');
  run({ down: ['Mouse0'], actions: ['firePrimary'] });
  assert.deepEqual(seen.splice(0), [[0, true]]);
  run({ down: ['Mouse0'], actions: [] });
  run({ down: ['Mouse0', 'Mouse2'], actions: ['fireSecondary'] });
  assert.deepEqual(seen.splice(0), [[1, true]]);
  assert.deepEqual(held.splice(0), [0, 0], 'the held primary hears every command without an edge');
  run({ down: [], actions: [] });
  assert.deepEqual(seen.splice(0), [[0, false], [1, false]]);
  run({ down: ['Mouse0'], actions: [], generation: 2 });
  assert.deepEqual(seen.splice(0), [[0, true]], 'a new life starts with the trigger up');
});

test('trigger edges: an edge that could not be sent leaves the button as it was', async () => {
  const { FireEdges } = await import('../src/runtime/fire-commands.js');
  const edges = new FireEdges();
  assert.equal(edges.each({ generation: 1, down: ['Mouse0'], actions: [] }, () => false), false);
  assert.deepEqual(edges.held, [false, false]);
  assert.equal(edges.each({ generation: 1, down: ['Mouse0'], actions: [] }, () => true), true);
  assert.deepEqual(edges.held, [true, false]);
  edges.reset();
  assert.deepEqual(edges.held, [false, false]);
});
