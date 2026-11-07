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

test('native outgoing triggers preserve short clicks, holds and life changes', async () => {
  const {EntityInputSender}=await import('../src/runtime/network-entity-sender.js');
  const {decodeEntityInput,INPUT_CODES,DEFAULT_BINDING_PROFILES}=await import('../src/runtime/network-entity-input.js');
  const registry={inputCodes:INPUT_CODES,bindingProfiles:DEFAULT_BINDING_PROFILES},seen=[],held=[];let sequence=0,generation=1;
  const sender=new EntityInputSender({registry,onFire:event=>{if(event.held)held.push(event.button);},send:bytes=>{
    const packet=decodeEntityInput(bytes,registry);for(const event of packet.edges)if(event.kind==='fire')seen.push([event.button==='fireSecondary'?1:0,event.pressed]);
    sender.edges.receipt({epoch:1,generation,ids:packet.edges.map(edge=>edge.id)});return true;
  }});
  const run=command=>{generation=command.generation??1;sender.send([{epoch:1,generation,sequence:++sequence,tick:sequence,...command}]);};
  run({down:[],actions:['firePrimary']});assert.deepEqual(seen.splice(0),[[0,true],[0,false]]);
  run({down:['Mouse0'],actions:['firePrimary']});assert.deepEqual(seen.splice(0),[[0,true]]);
  run({down:['Mouse0'],actions:[]});run({down:['Mouse0','Mouse2'],actions:['fireSecondary']});
  assert.deepEqual(seen.splice(0),[[1,true]]);assert.deepEqual(held.splice(0),[0,0]);
  run({down:[],actions:[]});assert.deepEqual(seen.splice(0),[[0,false],[1,false]]);
  run({down:['Mouse0'],actions:[],generation:2});assert.deepEqual(seen.splice(0),[[0,true]]);sender.dispose();
});
test('a failed outgoing packet retains its reliable trigger without duplicating the edge', async () => {
  const {EntityInputSender}=await import('../src/runtime/network-entity-sender.js');
  const {decodeEntityInput,INPUT_CODES,DEFAULT_BINDING_PROFILES}=await import('../src/runtime/network-entity-input.js');
  const registry={inputCodes:INPUT_CODES,bindingProfiles:DEFAULT_BINDING_PROFILES};let accept=false,packet;
  const sender=new EntityInputSender({registry,send:bytes=>{packet=decodeEntityInput(bytes,registry);return accept;}});
  const command=sequence=>({epoch:1,generation:1,sequence,tick:sequence,down:['Mouse0'],actions:[],look:[]});
  assert.equal(sender.send([command(1)]),false);assert.equal(sender.edges.pending.size,1);
  accept=true;assert.equal(sender.send([command(2)]),true);assert.equal(packet.edges.length,1);assert.equal(packet.edges[0].pressed,true);
  sender.clearPending();sender.send([command(3)]);assert.equal(packet.edges.length,2);sender.dispose();
});
