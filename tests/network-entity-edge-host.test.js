import {test} from 'node:test';
import assert from 'node:assert/strict';
import {EntityEdges} from '../src/runtime/network-entity-edge-host.js';
class Receiver {
 reset(){this.queue=new Map();this.seen=new Set();}
 receive(edges){for(const edge of edges)if(!this.seen.has(edge.id)){this.seen.add(edge.id);this.queue.set(edge.id,edge);}return {receipts:edges.map(e=>e.id),rejected:[]};}
 take(){const items=[...this.queue.values()];this.queue.clear();return items;}
}
function setup(){
 const mailbox=()=>({received:0,outcomes:[],enableOutcomes(){},drainOutcomes(){return this.outcomes.splice(0);}});
 const actor={unit:{generation:2},actions:mailbox(),fire:mailbox()},calls=[];
 const world=Object.fromEntries(['actions','fire'].map(lane=>[lane,(client,message)=>{assert(message.id>actor[lane].received);actor[lane].received=message.id;calls.push({lane,message});return true;}]));
 const edges=new EntityEdges({Receiver,actor,world,client:'one',registry:{classes:new Map([[1,'soldier']])}});
 return {actor,calls,edges};
}
test('reordered external IDs translate to monotonic internal IDs and acceptance is not consumption',()=>{
 const {actor,calls,edges}=setup();
 for(const id of [9,8]){edges.receive({epoch:1,generation:2,edges:[{epoch:1,generation:2,id,tick:10,kind:'action',action:'jump'}]},10);edges.advance(11);}
 assert.deepEqual(calls.map(c=>c.message.id),[1,2]);assert.deepEqual(edges.feedback().received,[9,8]);assert.deepEqual(edges.feedback().results,[]);
 actor.actions.outcomes.push({id:1,status:'consumed'});edges.consumed();assert.deepEqual(edges.feedback().results,[{id:9,status:'consumed',tick:11}]);
 edges.acknowledge({epoch:1,generation:3,ids:[9]});assert.equal(edges.feedback().results.length,1);
 edges.acknowledge({epoch:1,generation:2,ids:[9]});assert.equal(edges.feedback().results.length,0);assert.deepEqual(edges.feedback().received,[8]);
 actor.actions.outcomes.push({id:2,status:'cleared'});edges.consumed();assert.deepEqual(edges.feedback().results,[{id:8,status:'cleared',tick:11}]);
});
test('duplicate accepted clicks do not fire twice and unknown spawn prerequisites are explicit',()=>{
 const {actor,calls,edges}=setup(),edge={epoch:1,generation:2,id:9,tick:10,kind:'fire',button:'firePrimary',pressed:true};
 for(let i=0;i<2;i++){edges.receive({epoch:1,generation:2,edges:[edge]},10);edges.advance(11);}
 assert.equal(calls.length,1);assert.equal(calls[0].message.button,0);assert.equal(calls[0].message.externalId,9);assert.equal(edges.feedback().results.length,0);
 actor.fire.outcomes.push({id:1,status:'expired',tick:11});edges.consumed();assert.deepEqual(edges.feedback().results,[{id:9,status:'expired',tick:11}]);
 edges.receive({epoch:1,generation:2,edges:[{epoch:1,generation:2,id:2,tick:11,kind:'spawn',classId:99}]},11);edges.advance(12);
 assert.equal(calls.length,1);assert.deepEqual(edges.feedback().results.at(-1),{id:2,status:'rejected',tick:12});
 edges.reset(1,3);assert.equal(edges.pending.size,0);assert.deepEqual(edges.feedback().results,[]);
});
test('edge and result backlogs drop and count instead of failing the client',()=>{
 const {calls,edges}=setup();edges.capacity=4;
 const edge=(id,tick=10)=>({epoch:1,generation:2,id,tick,kind:'action',action:'jump'});
 edges.receive({epoch:1,generation:2,edges:[1,2,3].map(id=>edge(id))},10);
 // Two more would exceed the four it holds: neither is receipted.
 assert.doesNotThrow(()=>edges.receive({epoch:1,generation:2,edges:[edge(4),edge(5)]},10));
 assert.deepEqual(edges.dropped,{edges:2,results:0});assert.deepEqual(edges.feedback().received,[1,2,3]);
 edges.advance(11);assert.equal(calls.length,3);
 // Results the client never acknowledges: the oldest gives way.
 for(let id=10;id<16;id++)assert.doesNotThrow(()=>edges.result(id,'rejected'));
 assert.equal(edges.results.size,4);assert.equal(edges.dropped.results,2);
 assert.deepEqual(edges.feedback().results.map(r=>r.id),[12,13,14,15]);
});
test('a spun mouse wheel refuses weapon changes past the queue instead of ending the session',async()=>{
 // A player's report: scrolling fast through weapons disconnected them;
 // a 1 ms autoclicker did not. Edges outran the one-a-tick action queue.
 const {InputActions}=await import('../src/runtime/input-actions.js');
 const {EntityEdgeReceiver}=await import('../src/runtime/network-entity-edges.js');
 const actions=new InputActions(1),fire={received:0,enableOutcomes(){},drainOutcomes(){return [];}};
 const actor={unit:{generation:2},actions,fire};
 const world={actions:(client,message)=>actions.push(message,tick,2),fire:()=>true};
 const edges=new EntityEdges({Receiver:EntityEdgeReceiver,actor,world,client:'one',registry:{classes:new Map()}});
 let tick=10,id=0;
 for(;tick<40;tick++){
  const batch=Array.from({length:32},()=>({epoch:1,generation:2,id:++id,tick,kind:'action',action:'nextPrimary'}));
  assert.doesNotThrow(()=>{edges.receive({epoch:1,generation:2,edges:batch},tick);edges.advance(tick);actions.take(tick,2);edges.consumed(tick);});
 }
 assert(actions.queue.length<=actions.capacity&&actions.queue.length>=actions.capacity-1,'the queue fills and stays bounded');
 assert(edges.feedback().results.some(r=>r.status==='rejected'),'the refused presses are reported to the client');
});
