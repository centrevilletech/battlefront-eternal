import test from 'node:test';
import assert from 'node:assert/strict';
import { NetworkHost } from '../src/runtime/network-host.js';
import { UdpTransport } from '../src/runtime/udp-transport.js';
import { encodeEntities, emptySnapshot } from '../src/runtime/snapshot-codec.js';
import { decodeSnapshotAckKeepalive } from '../src/runtime/network-entity-input.js';

test('entity profile validates registry before welcome and acknowledges only applied raw packets', async (t) => {
  const received=[],sent=[],registry={contentEpoch:1};
  const host=new NetworkHost({onEntityRegistry:value=>{assert.deepEqual(value,registry);return registry;},onEntities:bytes=>received.push(bytes)});
  host.authenticated=true;
  host.resolveReady=()=>{};
  host.channels.set('state',{readyState:'open',bufferedAmount:0,send:value=>sent.push(value)});
  await assert.rejects(host.receive('events',JSON.stringify({type:'welcome',snapshotMode:'snapshot-delta-v1'})),/did not send its battle description/);
  await host.receive('events',JSON.stringify({type:'entity-registry',registry}));
  await host.receive('events',JSON.stringify({type:'welcome',snapshotMode:'snapshot-delta-v1',inputMode:'split-v1'}));
  const packet=encodeEntities({...emptySnapshot(30,1)},null)[0];
  await host.receive('state',packet.buffer.slice(packet.byteOffset,packet.byteOffset+packet.byteLength));
  assert.equal(received.length,0,'entity packets wait for the frame loop, they are not applied in the message handler');
  host.drainEntities();
  assert.equal(received.length,1);assert.equal(host.snapshotAck,null);assert.equal(sent.length,0,'decoding alone is not application');
  host.setSnapshotAck({epoch:1,tick:30});
  assert.deepEqual(decodeSnapshotAckKeepalive(sent.at(-1)),{epoch:1,tick:30});
  await assert.rejects(host.receive('events',JSON.stringify({type:'entity-registry',registry})),/Unexpected/);
  host.dispose();
});


test('closing a host rejects pending RPC and clears deadlines', async () => {
  const host = new NetworkHost();
  host.authenticated = true;
  host.channels.set('events', { readyState: 'open', bufferedAmount: 0, send() {} });
  const pending = host.call('start');
  host.dispose();
  await assert.rejects(pending, /Disconnected/);
  assert.equal(host.pending.size, 0);
});


// A fake network-process port: what the page posts, and replies on demand.
function fakePort(){const posted=[];return {posted,port:{postMessage:message=>posted.push(message),start(){},close(){posted.push({op:'port-closed'});}}};}

test('JOIN IP and LAN connect straight to the launch address without the master', async () => {
  const requests=[],{posted,port}=fakePort();
  const transport = new UdpTransport({ bridge: { openSession(){}, request: async action => { requests.push(action); throw Error('no master'); } }, openPort: async () => port });
  const connecting=transport.connect({ query: '?server=' + 'a'.repeat(64) + '&address=127.0.0.1&port=18499', content:'c', channels:new Map(), receive(){}, fail(){}, status(){} });
  await new Promise(resolve => setTimeout(resolve, 10));
  assert.equal(transport.address, '127.0.0.1');
  assert.equal(transport.port, 18499);
  assert.equal(transport.serverId, 'a'.repeat(64));
  assert.deepEqual(posted.find(message => message.op === 'connect').candidates, [{ address: '127.0.0.1', port: 18499, kind: 'host' }]);
  assert.deepEqual(requests, [], 'no credentials, consent or signaling for a direct join');
  transport.message({op:'error',for:'connected',message:'refused'});
  await assert.rejects(connecting, /refused/);
  transport.dispose();
});


test('nothing reaches the session before the network process proves the server', async () => {
  const received=[],{port}=fakePort();
  const transport = new UdpTransport({ bridge: { openSession(){}, request: async () => true }, openPort: async () => port });
  const channels=new Map();
  const connecting=transport.connect({ query: '?server=' + 'a'.repeat(64) + '&address=127.0.0.1&port=18451', content:'c', channels, receive:(...args)=>received.push(args), fail(){}, status(){} });
  await new Promise(resolve => setTimeout(resolve, 10));
  transport.message({ op: 'recv', items: [['events', JSON.stringify({ type: 'welcome' }), performance.timeOrigin + performance.now()]] });
  assert.equal(received.length, 0);
  assert.equal(channels.get('events').send('x'), false, 'and nothing leaves it');
  transport.message({ op: 'connected', server: { id: 'a'.repeat(64), hello: { protocol: 3, profile: 'snapshot-delta-v1', simulation: 'bfe-sim-27', content: 'c' } }, path: { kind: 'direct', family: 'IPv4' }, rtt: 12 });
  await connecting;
  assert.equal(channels.get('events').readyState, 'open');
  transport.message({ op: 'recv', items: [['events', JSON.stringify({ type: 'welcome' }), performance.timeOrigin + performance.now()]] });
  assert.equal(received.length, 1);
  transport.dispose();
});


test('network chat uses the authenticated channel and bounds sender display', async () => {
  const messages = [], received = [];
  const host = new NetworkHost({ onChat: message => received.push(message) });
  host.authenticated = true;
  host.channels.set('events', { readyState: 'open', bufferedAmount: 0, send: value => messages.push(JSON.parse(value)) });
  host.chat({ channel: 'team', text: 'Hold the post' });
  assert.deepEqual(messages, [{ type: 'chat', text: 'Hold the post', team: true }]);
  await host.receive('events', JSON.stringify({ type: 'chat', sender: 'Player 82', text: 'Holding', team: true }));
  assert.deepEqual(received, [{ name: 'Player 82', text: 'Holding', channel: 'team' }]);
  await host.receive('events', JSON.stringify({ type: 'chat', sender: 'Server', text: 'Josh joined the game', team: false, admin: true }));
  assert.deepEqual(received.at(-1), { name: '', text: 'Josh joined the game', channel: 'server' });
  await assert.rejects(host.receive('events', JSON.stringify({ type: 'chat', sender: 'Player', text: 'x'.repeat(241), team: false })), /Invalid chat/);
});


test('leaving the battle closes the network port and desktop signaling exactly once', () => {
  const lifecycle = new EventTarget(), calls = [];
  const host = new NetworkHost({ lifecycle, bridge: { openSession(){}, request: async action => calls.push(action) } });
  host.transport.netPort = { postMessage: message => calls.push(message.op), close: () => calls.push('port') };
  lifecycle.dispatchEvent(new Event('pagehide'));
  assert.equal(host.closed, true);
  assert.deepEqual(calls, ['close', 'close'], 'the session close to the network process, then master signaling');
  lifecycle.dispatchEvent(new Event('pagehide'));
  host.dispose();
  assert.deepEqual(calls, ['close', 'close']);
});


test('independent input ACK updates resend feedback without applying a snapshot or replay ACK',async()=>{
 const feedback=[],ticks=[],acks=[],host=new NetworkHost({onInputAck:ack=>feedback.push(ack),onTicks:value=>ticks.push(value)});
 host.authenticated=true;host.entityInput={life:'1:2',ack:sequence=>acks.push(sequence)};
 const message={type:'input-ack',epoch:1,generation:2,serverTick:300,ack:271,ackTick:298,ackActions:12};
 await host.receive('state',JSON.stringify(message));
 assert.deepEqual(acks,[271]);assert.equal(feedback.length,1);assert.ok(Number.isFinite(host.transportAck.receivedAt));
 assert.deepEqual(ticks,[]);assert.equal(host.entityAck,undefined);
 await host.receive('state',JSON.stringify({...message,serverTick:299,ack:270}));assert.equal(feedback.length,1);
 await assert.rejects(host.receive('state',JSON.stringify({...message,ackTick:400})),/Invalid input acknowledgement/);
});


test('entity profile sends compact state and numbered edges with separate outcome acknowledgement',async()=>{
 const {INPUT_CODES,DEFAULT_BINDING_PROFILES,decodeEntityInput}=await import('../src/runtime/network-entity-input.js');
 const registry={inputCodes:INPUT_CODES,bindingProfiles:DEFAULT_BINDING_PROFILES,classIds:new Map(),postIds:new Map()};
 const sent=[],events=[],host=new NetworkHost({onEntityRegistry:()=>registry,onEntities:()=>{}});
 host.authenticated=true;host.resolveReady=()=>{};
 host.channels.set('state',{readyState:'open',bufferedAmount:0,send:value=>sent.push(value)});
 host.channels.set('events',{readyState:'open',bufferedAmount:0,send:value=>events.push(value)});
 await host.receive('events',JSON.stringify({type:'entity-registry',registry:{}}));
 await host.receive('events',JSON.stringify({type:'welcome',snapshotMode:'snapshot-delta-v1',inputMode:'split-v1'}));
 host.setSnapshotAck({epoch:1,tick:7});host.setSnapshotAck({epoch:1,tick:5});
 host.input({epoch:1,generation:1,sequence:1,tick:10,down:[],look:[[3,4]],actions:['jump']});
 const packet=decodeEntityInput(sent.at(-1),registry);assert.deepEqual(packet.snapshotAck,{epoch:1,tick:7},'the older ack does not move the baseline backward');assert.equal(packet.edges[0].action,'jump');assert.deepEqual(packet.commands[0].lookTotal,[3,4]);assert.equal(events.length,0);
 await host.receive('state',JSON.stringify({type:'entity-edge-results',epoch:1,generation:1,received:[1],results:[{id:1,status:'consumed',tick:12}]}));
 assert.equal(JSON.parse(sent.at(-1)).type,'entity-edge-ack');assert.deepEqual(host.entityInput.pendingActions(1),['jump']);host.entityInput.applied(12);assert.deepEqual(host.entityInput.pendingActions(1),[]);host.dispose();
});


for (const [name, hello] of [['obsolete protocol', { protocol: 2, profile: 'snapshot-delta-v1', simulation: 'bfe-sim-20' }], ['previous simulation', { protocol: 3, profile: 'snapshot-delta-v1', simulation: 'bfe-sim-6' }]])
test(`a server with the ${name} is rejected with a clear update error`, async () => {
  const {port}=fakePort(), transport=new UdpTransport({ bridge: { openSession(){} }, openPort: async () => port });
  const connecting=transport.connect({ query: '?server=' + 'a'.repeat(64) + '&address=127.0.0.1&port=18451', content:'c', channels:new Map(), receive(){}, fail(){}, status(){} });
  await new Promise(resolve => setTimeout(resolve, 10));
  transport.message({ op: 'connected', server: { id: 'a'.repeat(64), hello }, path: { kind: 'direct' }, rtt: 1 });
  await assert.rejects(connecting, /Update the desktop client and server/);
  assert.notEqual(transport.connected, true);
  transport.dispose();
});

test('mandatory multiplayer rejects legacy streams after authentication',async()=>{
 const host=new NetworkHost();host.authenticated=host.entityRequested=true;
 for(const type of ['ticks','packet','snapshot-events'])await assert.rejects(host.receive('bulk',JSON.stringify({type,buffers:[]})),/Unexpected/);
 host.dispose();
});

test('server version rejection is visible before authentication',async()=>{
 const host=new NetworkHost();await assert.rejects(host.receive('events',JSON.stringify({type:'version-error',message:'Update required: protocol 2 / bfe-sim-9 / entity-state-v1'})),/Update required: protocol 2/);host.dispose();
});

test('authenticated unsupported gameplay reason is shown instead of a silent stream stop',async()=>{
 const host=new NetworkHost();host.authenticated=true;await assert.rejects(host.receive('events',JSON.stringify({type:'gameplay-error',message:'Ground vehicle driving is not supported yet.'})),/Ground vehicle driving/);host.dispose();
});


test('rotation survives the old connection closing and automatically follows actual readiness',async(t)=>{
  const navigated=[],errors=[],previous=globalThis.location;
  globalThis.location={search:'?map=nab2&side=rep&network=1&worker=1&server=x&address=127.0.0.1&port=18461&traceInput=1&profileRuntime=1',assign:url=>navigated.push(url)};
  t.after(()=>{globalThis.location=previous;});
  let ready=false,probes=0;
  const transport={dispose(){},async matchReady(query){probes++;assert.match(query,/map=geo1/);if(probes===1)throw Error('listener not open');return ready;}};
  const host=new NetworkHost({transport,onMatch:()=>true,onError:error=>errors.push(error),rotationPollMs:5});
  t.after(()=>host.dispose());
  await host.receive('events',JSON.stringify({type:'match',query:'?map=geo1&side=rep&ai=classic&seed=1&match='+ 'a'.repeat(32),delayMs:0}));
  assert.equal(host.proceed(),false,'an early Next must not mark the scoreboard as quitting');
  host.fail(Error('Connection to server closed'));
  assert.equal(host.closed,true);assert.deepEqual(errors,[]);
  await until(()=>probes>=2);
  assert.equal(host.rotationReady,false);assert.deepEqual(navigated,[]);
  ready=true;
  await until(()=>host.rotationReady);
  assert.equal(navigated.length,1,'readiness follows immediately without an extra scoreboard dwell');
  assert.equal(host.proceed(),true);
  assert.equal(navigated.length,1);
  const next=new URLSearchParams(navigated[0].slice(2));
  assert.equal(next.get('map'),'geo1');assert.equal(next.get('network'),'1');assert.equal(next.get('address'),'127.0.0.1');assert.equal(next.get('port'),'18461');assert.equal(next.get('traceInput'),'1');assert.equal(next.get('profileRuntime'),'1');
});

test('host skip automatically follows readiness after old peer closes; page disposal cancels in-flight readiness',async(t)=>{
  const navigated=[],previous=globalThis.location;
  globalThis.location={search:'?network=1&server=x',assign:url=>navigated.push(url)};
  t.after(()=>{globalThis.location=previous;});
  let answer;
  const transport={dispose(){},matchReady:()=>new Promise(resolve=>{answer=resolve;})};
  const host=new NetworkHost({transport,onMatch:()=>false});t.after(()=>host.dispose());
  await host.receive('events',JSON.stringify({type:'match',query:'?map=hot1&side=all&match='+ 'b'.repeat(32),delayMs:0}));
  await until(()=>answer);
  host.fail(Error('old peer closed'));
  answer(true);
  await until(()=>navigated.length);
  assert.equal(navigated.length,1,'no manual rejoin and no arbitrary extra delay');
  let pending;
  const cancelled=new NetworkHost({transport:{dispose(){},matchReady:()=>new Promise(resolve=>{pending=resolve;})},onMatch:()=>false});
  t.after(()=>cancelled.dispose());
  await cancelled.receive('events',JSON.stringify({type:'match',query:'?map=geo1&match='+ 'c'.repeat(32),delayMs:0}));
  await until(()=>pending);
  cancelled.fail(Error('old peer closed'));
  cancelled.onPageHide();
  pending(true);
  await new Promise(resolve=>setTimeout(resolve,5));
  assert.equal(navigated.length,1,'a late probe cannot navigate a page the player already left');
  assert.equal(cancelled.rotationReady,false);
});

async function until(predicate){
  const deadline=performance.now()+1000;
  while(!predicate()){
    assert.ok(performance.now()<deadline,'timed out waiting for rotation');
    await new Promise(resolve=>setTimeout(resolve,2));
  }
}

test('old rotation announcements fail clearly and a stalled next launch offers recovery',async(t)=>{
  const notices=[];
  const host=new NetworkHost({transport:{dispose(){},async matchReady(){return false;}},rotationPollMs:5});
  t.after(()=>host.dispose());
  await assert.rejects(host.receive('events',JSON.stringify({type:'match',query:'?map=geo1',delayMs:0})),/Update the server for automatic map rotation/);
  assert.equal(host.rotation,undefined,'incompatible announcements must not enter an endless loading state');
  host.status=(text,failed)=>notices.push({text,failed});
  await host.receive('events',JSON.stringify({type:'match',query:'?map=geo1&match='+'a'.repeat(32),delayMs:0}));
  host.rotation.deadline=performance.now()-1;
  await until(()=>host.rotation.warned);
  assert.match(notices.at(-1).text,/reconnect or return to the main menu/);
  assert.equal(notices.at(-1).failed,true,'existing recovery actions accompany the timeout');
  assert.equal(host.rotationReady,false);
});

test('readiness failures back off instead of creating a signaling-ticket retry storm',async(t)=>{
  let calls=0;
  const host=new NetworkHost({transport:{dispose(){},async matchReady(){calls++;throw Error('rate limit');}},rotationPollMs:5});
  t.after(()=>host.dispose());
  await host.receive('events',JSON.stringify({type:'match',query:'?map=geo1&match='+'a'.repeat(32),delayMs:0}));
  for(let failures=1;failures<=4;failures++){
    await until(()=>host.rotation.failures>=failures);
    assert.equal(host.rotation.retryMs,5*2**(failures-1));
    assert.equal(host.rotation.lastError,'rate limit');
  }
  assert.equal(calls,4);
  host.rotation.failures=100;
  host.rotationPollMs=1000;
  host.rotation.readyAt=0;
  host.scheduleRotation();
  await until(()=>calls===5);
  assert.equal(host.rotation.retryMs,30000,'a persistent failure is limited to two attempts per minute');
});
