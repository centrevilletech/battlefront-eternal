import test from 'node:test';
import assert from 'node:assert/strict';
import {NetworkSession} from '../src/runtime/network-session.js';
import {EntityInputSender} from '../src/runtime/network-entity-sender.js';
import {INPUT_CODES,DEFAULT_BINDING_PROFILES,decodeSnapshotAckKeepalive} from '../src/runtime/network-entity-input.js';

function fixture(){
 const sent=[],session=new NetworkSession({lifecycle:null});
 session.authenticated=session.entityMode=true;
 session.channels.set('state',{readyState:'open',bufferedAmount:0,send:value=>sent.push(value)});
 session.entityInput=new EntityInputSender({registry:{inputCodes:INPUT_CODES,bindingProfiles:DEFAULT_BINDING_PROFILES},send:()=>true});
 session.entityInput.send([{epoch:1,generation:1,sequence:1,tick:1,down:[],look:[],actions:[]}]);
 return{session,sent,channel:session.channels.get('state')};
}
test('production session coalesces remote held state while local simulation keeps every tick',async()=>{
 for(const local of [false,true]){
  const session=new NetworkSession({lifecycle:null,transport:{local},onEntityRegistry:()=>({inputCodes:INPUT_CODES,bindingProfiles:DEFAULT_BINDING_PROFILES})});
  await session.receive('events',JSON.stringify({type:'entity-registry',registry:{}}));
  assert.equal(session.entityInput.stateIntervalTicks,local?1:2);
 }
});
test('edge result ACK stops after admission, and lost ACK repairs on repeated server feedback',async()=>{
 const{session,sent}=fixture(),feedback=JSON.stringify({type:'entity-edge-results',epoch:1,generation:1,received:[1],results:[{id:1,tick:1,status:'consumed'}]});
 await session.receive('state',feedback);assert.equal(sent.length,1);assert.deepEqual(JSON.parse(sent[0]).ids,[1]);
 assert.equal(session.sendEntityEdgeAcknowledgement(),false,'idle timer must not repeat already admitted ACKs forever');
 // Drop that ACK. The server retains and repeats the result until one arrives.
 await session.receive('state',feedback);assert.equal(sent.length,2);assert.deepEqual(JSON.parse(sent[1]).ids,[1]);
});
test('edge result ACK retries after local backpressure',async()=>{
 const{session,sent,channel}=fixture();channel.bufferedAmount=40000;
 await session.receive('state',JSON.stringify({type:'entity-edge-results',epoch:1,generation:1,received:[1],results:[{id:1,tick:1,status:'consumed'}]}));
 assert.equal(sent.length,0);assert.deepEqual(session.entityInput.resultAcks,[1]);
 channel.bufferedAmount=0;assert.equal(session.sendEntityEdgeAcknowledgement(),true);assert.equal(sent.length,1);
});
test('standalone snapshot ACK keepalive resumes when input pauses or cannot be sent',()=>{
 const{session,sent,channel}=fixture();session.snapshotAck={epoch:1,tick:7};
 assert.equal(session.sendEntityInput(new Uint8Array(16)),true);
 assert.equal(session.sendEntityAcknowledgement(),false);assert.equal(sent.length,1);
 session.lastInputSentAt=performance.now()-101;
 channel.bufferedAmount=5000;assert.equal(session.sendEntityInput(new Uint8Array(16)),false);
 assert.equal(session.sendEntityAcknowledgement(),true);assert.deepEqual(decodeSnapshotAckKeepalive(sent[1]),{epoch:1,tick:7});
 session.setSnapshotAck({epoch:1,tick:5});assert.equal(session.snapshotAck.tick,7,'the baseline never moves backward');
});

test('spectator welcome blocks gameplay commands while retaining world acknowledgements',async(t)=>{
 const{session,sent}=fixture();t.after(()=>session.dispose());
 session.entityRegistry={};session.resolveReady=()=>{};
 await session.receive('events',JSON.stringify({type:'welcome',snapshotMode:'snapshot-delta-v1',spectator:true,epoch:1}));
 session.entityInput.send=()=>assert.fail('spectator commands must not enter reliable input retention');
 assert.equal(session.inputs([{human:[{kind:'spawn',team:1,className:'trooper',post:'cp1'}]}]),false);
 assert.equal(session.sendEntityInput(new Uint8Array(16)),false);
 assert.equal(sent.length,0);
 session.setSnapshotAck({epoch:1,tick:7});
 assert.equal(session.closed,false);
 assert.deepEqual(decodeSnapshotAckKeepalive(sent[0]),{epoch:1,tick:7});
});

test('spectator notice stays prominent and actionable across periodic connection updates',async(t)=>{
 const previousDocument=globalThis.document;
 const node=()=>({style:{},children:[],setAttribute(){},append(...children){this.children.push(...children);},replaceChildren(...children){this.children=children;},remove(){}});
 globalThis.document={body:node(),createElement:node,createTextNode:text=>({textContent:text})};
 t.after(()=>{if(previousDocument===undefined)delete globalThis.document;else globalThis.document=previousDocument;});
 const{session}=fixture();t.after(()=>session.dispose());
 session.entityRegistry={};session.resolveReady=()=>{};
 await session.receive('events',JSON.stringify({type:'welcome',snapshotMode:'snapshot-delta-v1',spectator:true,epoch:1}));
 const notice=session.statusElement;
 for(const status of [null,'Spectating (content differs) · Direct IPv4 · 18 ms','Spectating (content differs) · Relay IPv4 · 25 ms']){
  if(status)session.status(status);
  assert.equal(session.statusElement,notice);
  assert.match(notice.style.cssText,/left:50%;top:50%/);
  assert.match(notice.children[0].textContent,/Cannot join as a player: game data differs from the server/);
  assert.match(notice.children[0].textContent,/Update the game on both ends; if it persists, re-prepare game data/);
  assert.deepEqual(notice.children.slice(1).map(button=>button.textContent),['Reconnect','Main menu']);
  assert.equal(session.closed,false);
 }
 session.status('Connection lost',true);
 assert.equal(notice.children[0].textContent,'Connection lost','a real connection failure replaces stale mismatch guidance');
 session.rotation={ready:false,warned:false};
 session.status('Direct IPv4 · 18 ms');
 assert.equal(notice.children[0].textContent,'Next battle loading...','late path stats cannot replace rotation status');
 session.rotation.warned=true;
 for(const text of ['Direct IPv4 · 19 ms','Next battle loading...']){
  session.status(text);
  assert.match(notice.children[0].textContent,/taking too long to start/);
  assert.deepEqual(notice.children.slice(1).map(button=>button.textContent),['Reconnect','Main menu']);
 }
});

test('a page that built another battle than the server plays loads the server\'s from its welcome, before any entity lands', async () => {
 const {otherBattle} = await import('../src/runtime/network-session.js');
 const page = '?map=bes2&side=rep&era=cw&ai=classic&seed=1&network=1&server=abc&address=10.0.0.5&port=18452';
 assert.equal(otherBattle(page, '?map=bes2&side=rep&ai=off&seed=1&match=' + 'a'.repeat(32)), true, 'bots off on the server, on in the page');
 assert.equal(otherBattle(page, '?map=bes2&side=rep&ai=classic&seed=1&match=' + 'b'.repeat(32)), false, 'the same battle: the match token and the page\'s own keys do not count');
 assert.equal(otherBattle(page, '?map=bes2&side=imp&ai=classic&seed=1'), true);
 const visited = [], location = globalThis.location;
 globalThis.location = { search: page, assign: url => visited.push(url) };
 try {
  const session = new NetworkSession({ lifecycle: null, onEntityRegistry: () => ({ inputCodes: INPUT_CODES, bindingProfiles: DEFAULT_BINDING_PROFILES }), onEntities: () => {} });
  session.query = page;
  let ready = false; session.resolveReady = () => { ready = true; };
  await session.receive('events', JSON.stringify({ type: 'entity-registry', registry: {} }));
  await session.receive('events', JSON.stringify({ type: 'welcome', snapshotMode: 'snapshot-delta-v1', epoch: 1, query: '?map=bes2&side=rep&ai=off&seed=1&match=' + 'c'.repeat(32) }));
  assert.equal(visited.length, 1);
  const next = new URLSearchParams(visited[0].slice(2));
  assert.deepEqual([next.get('ai'), next.get('map'), next.get('server'), next.get('address'), next.get('port')], ['off', 'bes2', 'abc', '10.0.0.5', '18452'], 'the server\'s battle, over this page\'s connection');
  assert.equal(ready, false, 'this battle never starts');
  assert.equal(session.authenticated, false);
 } finally { globalThis.location = location; }
});

test('a welcome from another battle redirects even before the registry lands; one without a registry names the drop, not a version', async () => {
 const { NetworkSession } = await import('../src/runtime/network-session.js');
 const page = '?map=bes2&side=rep&ai=off&seed=1&network=1&server=abc';
 const visited = [], location = globalThis.location;
 globalThis.location = { search: page, assign: url => visited.push(url) };
 try {
  const rotated = new NetworkSession({ lifecycle: null, onEntityRegistry: () => ({ inputCodes: INPUT_CODES, bindingProfiles: DEFAULT_BINDING_PROFILES }), onEntities: () => {} });
  rotated.query = page; rotated.resolveReady = () => {};
  // The server rotated while this page loaded: it joined as a spectator and
  // its registry message was lost. The server's battle still loads.
  await rotated.receive('events', JSON.stringify({ type: 'welcome', snapshotMode: 'snapshot-delta-v1', epoch: 1, spectator: true, query: '?map=bes2&side=imp&ai=off&seed=1&match=' + 'd'.repeat(32) }));
  assert.equal(visited.length, 1);
  assert.equal(new URLSearchParams(visited[0].slice(2)).get('side'), 'imp');
  const dropped = new NetworkSession({ lifecycle: null, onEntityRegistry: () => ({}), onEntities: () => {} });
  dropped.query = page; dropped.resolveReady = () => {};
  await assert.rejects(dropped.receive('events', JSON.stringify({ type: 'welcome', snapshotMode: 'snapshot-delta-v1', epoch: 1, query: page })), /did not send its battle description/);
  assert.equal(visited.length, 1, 'the same battle does not reload');
  const older = new NetworkSession({ lifecycle: null, onEntityRegistry: () => ({}), onEntities: () => {} });
  older.query = page; older.resolveReady = () => {};
  await assert.rejects(older.receive('events', JSON.stringify({ type: 'welcome', snapshotMode: 'older', epoch: 1, query: page })), /Multiplayer version mismatch/);
 } finally { globalThis.location = location; }
});
