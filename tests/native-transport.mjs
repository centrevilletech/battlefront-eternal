// Native packets run against the production JS server in this process. The
// addon is only a fixture observer; device clients link the same Rust owner.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createPrivateKey,createPublicKey} from 'node:crypto';
import {identityKeys,RetryTokens} from '../src/net/crypto.js';
import {ServerHandshakes} from '../src/net/handshake.js';
import {Connection,encodeAck} from '../src/net/connection.js';
import {RangeSet,MAX_DATAGRAM} from '../src/net/codec.js';
import {NETWORK_CONTRACT} from '../src/runtime/network-contract.js';
const {NativePeer}=createRequire(import.meta.url)('../.transport-addon/bfe-transport.node');
const seed=Buffer.alloc(32,9),hello={protocol:NETWORK_CONTRACT.protocol,profile:NETWORK_CONTRACT.profile,simulation:NETWORK_CONTRACT.simulation,content:'synthetic-content',name:'Player'};
function identity(value){const key=createPrivateKey({key:Buffer.concat([Buffer.from('302e020100300506032b657004220420','hex'),Buffer.alloc(32,value)]),type:'pkcs8',format:'der'});return identityKeys({privateKey:key.export({type:'pkcs8',format:'pem'}),publicKey:createPublicKey(key).export({type:'spki',format:'der'}).toString('base64')});}
function pair({keyInterval=1<<20,retry=false}={}){
 let now=0;const serverIdentity=identity(17),native=new NativePeer(seed,serverIdentity.id,JSON.stringify(hello));
 const handshakes=new ServerHandshakes({identity:serverIdentity,hello:()=>hello,retry:new RetryTokens(Buffer.alloc(32,31)),now:()=>now});
 const first=native.initiation();let response=handshakes.initiation(first,'client',{requireRetry:retry});
 if(retry){assert.equal(native.handshake(response,now,100),'retry');const next=native.initiation(native.token());assert.deepEqual(next.subarray(0,54),first.subarray(0,54));response=handshakes.initiation(next,'client',{requireRetry:true});}
 assert.equal(native.handshake(response,now,100),'established');const confirm=native.confirmation();const session=handshakes.confirm(confirm,'client');assert.ok(session);
 assert.equal(session.client.id,identity(9).id);assert.equal(native.identity(),session.client.id);assert.deepEqual(JSON.parse(native.serverHello()),hello);
 assert.equal(native.handshake(response,now,100),'duplicate');assert.deepEqual(native.confirmation(),confirm);
 const wire=[],messages=[],closed=[];
 const server=new Connection({role:'server',localSession:session.serverSession,remoteSession:session.clientSession,send:session.send,receive:session.receive,
  transmit:(bytes,path)=>{assert.ok(bytes.length<=MAX_DATAGRAM);wire.push({bytes:Buffer.from(bytes),path});},onMessage:(lane,bytes)=>messages.push({lane,text:typeof bytes==='string',bytes:Buffer.from(bytes)}),onClose:value=>closed.push(value),
  now:()=>now,setTimer:()=>1,clearTimer:()=>{},schedule:()=>{},keyUpdateEvery:keyInterval,path:{key:'client',remote:'client'}});
 native.keyInterval(keyInterval);
 const transfer=(filter=()=>true)=>{
  for(let pass=0;pass<64;pass++){
   let moved=false;
   for(const item of native.packets()){assert.ok(item.bytes.length<=MAX_DATAGRAM);if(filter('up',item.bytes)){server.receive(item.bytes,{key:'client',remote:'client'});moved=true;}}
   for(const item of wire.splice(0)){if(filter('down',item.bytes)){native.feed(item.bytes,now);moved=true;}}
   if(!moved)break;
  }
 };
 const advance=(milliseconds,filter)=>{const end=now+milliseconds;while(now<end){now=Math.min(end,now+10);native.poll(now);server.onTimer();transfer(filter);}};
 server.confirmHandshake();server.flush();transfer();assert.equal(JSON.parse(native.stats()).handshakeConfirmed,true);
 return {native,server,wire,messages,closed,advance,transfer,get now(){return now;},stop(){server.finish({code:0,reason:'fixture finished',remote:false});}};
}
test('Rust client authenticates the JS server, retries and repeats one confirmation',()=>{const p=pair({retry:true});p.stop();});
test('all three message lanes interoperate without a native world',()=>{
 const p=pair();try{
  const large=Buffer.alloc(32768);for(let i=0;i<large.length;i++)large[i]=i%251;
  for(const lane of ['state','events','bulk']){assert.equal(p.native.send(lane,false,large),true);p.server.send(lane,large);}
  p.native.send('events',true,Buffer.from('native text'));p.server.send('bulk','server text');p.native.flush(p.now);p.server.flush();p.transfer();p.advance(2000);
  const down=p.native.messages();for(const lane of ['state','events','bulk']){assert.deepEqual(down.find(m=>m.lane===lane&&!m.text)?.bytes,large);assert.deepEqual(p.messages.find(m=>m.lane===lane&&!m.text)?.bytes,large);}
  assert.equal(down.find(m=>m.text)?.bytes.toString(),'server text');assert.equal(p.messages.find(m=>m.text)?.bytes.toString(),'native text');assert.equal(JSON.parse(p.native.stats()).backlog,0);assert.equal(p.server.streamBacklog,0);
 }finally{p.stop();}
});
test('loss on one ordered stream does not block state or the other stream',()=>{
 const p=pair();try{
  const bulk=Buffer.alloc(180000,73),events=Buffer.from('an independent event');p.native.send('bulk',false,bulk);p.native.flush(p.now);
  const first=p.native.packets();assert.ok(first.length>2);for(const item of first.slice(1))p.server.receive(item.bytes,{key:'client',remote:'client'});
  p.native.send('events',false,events);p.native.send('state',false,Buffer.from('new state'));p.native.flush(p.now);p.transfer();
  assert.ok(p.messages.some(m=>m.lane==='state'));
  // Reliable lanes share congestion control, so first release that window.
  // The missing bulk prefix must not hold the independent event stream.
  p.advance(40);assert.ok(p.messages.some(m=>m.lane==='events'));assert.ok(!p.messages.some(m=>m.lane==='bulk'));
  p.advance(6000);assert.deepEqual(p.messages.filter(m=>m.lane==='bulk').map(m=>m.bytes),[bulk]);assert.ok(JSON.parse(p.native.stats()).retransmitted>0);
 }finally{p.stop();}
});
test('reliable messages converge under deterministic bidirectional packet loss',()=>{
 const p=pair();try{
  const expected=[];for(let i=0;i<60;i++){const bytes=Buffer.alloc(1500+i*3,i);expected.push(bytes);p.native.send('events',false,bytes);p.server.send('bulk',bytes);}
  let up=0,down=0;const filter=(direction)=>direction==='up'?++up%17!==1:++down%19!==1;
  p.native.flush(p.now);p.server.flush();p.transfer(filter);p.advance(10000,filter);
  assert.deepEqual(p.messages.filter(m=>m.lane==='events').map(m=>m.bytes),expected);assert.deepEqual(p.native.messages().filter(m=>m.lane==='bulk').map(m=>m.bytes),expected);
  assert.equal(JSON.parse(p.native.stats()).backlog,0);assert.equal(p.server.streamBacklog,0);
 }finally{p.stop();}
});
test('authentication and replay checks cannot advance a valid packet away',()=>{
 const p=pair();try{
  p.server.send('state',Buffer.from('one message'));p.server.flush();const packet=p.wire.pop().bytes;const bad=Buffer.from(packet);bad[bad.length-1]^=1;
  assert.equal(p.native.feed(bad,p.now),false);assert.equal(p.native.feed(packet,p.now),true);assert.equal(p.native.feed(packet,p.now),false);
  assert.deepEqual(p.native.messages().map(m=>m.bytes.toString()),['one message']);assert.equal(JSON.parse(p.native.stats()).authFailures,1);
 }finally{p.stop();}
});
test('key phases rotate in both directions while ordered messages remain exact',()=>{
 const p=pair({keyInterval:4});try{
  for(let i=0;i<40;i++){const bytes=Buffer.from('message '+i);p.native.send('events',false,bytes);p.server.send('bulk',bytes);p.native.flush(p.now);p.server.flush();p.transfer();p.advance(30);}
  p.advance(1000);assert.deepEqual(p.messages.filter(m=>m.lane==='events').map(m=>m.bytes.toString()),Array.from({length:40},(_,i)=>'message '+i));
  assert.deepEqual(p.native.messages().filter(m=>m.lane==='bulk').map(m=>m.bytes.toString()),Array.from({length:40},(_,i)=>'message '+i));
 }finally{p.stop();}
});
test('authenticated malformed ACKs close cleanly and an ordinary close reaches the peer',()=>{
 const p=pair();try{
  p.native.send('state',false,Buffer.from('sent'));p.native.flush(p.now);p.transfer();const ranges=new RangeSet();ranges.add(0);const ack=encodeAck(ranges,0);
  p.server.emitPacket([ack,ack],{reliable:false,datagrams:0,streams:[]});p.transfer();assert.equal(JSON.parse(p.native.stats()).closed.code,2);assert.equal(p.closed[0]?.code,2);
 }finally{p.stop();}
 const q=pair();try{q.native.close(0,'done',q.now);q.transfer();assert.equal(q.closed[0]?.reason,'done');}finally{q.stop();}
});
test('idle peers exchange keepalives and close after the authenticated idle bound',()=>{
 const p=pair();try{p.advance(5000);assert.ok(JSON.parse(p.native.stats()).sent>=5);assert.equal(JSON.parse(p.native.stats()).closed,null);p.advance(21000,()=>false);assert.equal(JSON.parse(p.native.stats()).closed.code,1);}finally{p.stop();}
});

test('native UDP sockets authenticate, exchange lanes and reconnect with one identity',async()=>{
 const {UdpEndpoint}=await import('../src/net/endpoint.js'),{NativeSocket}=createRequire(import.meta.url)('../.transport-addon/bfe-transport.node');
 const keys=identity(29),endpoint=new UdpEndpoint({identity:keys,server:{hello:()=>hello,admit:()=>({accept:true})}}),ids=[],received=[];
 let client;
 try{
  const {port}=await endpoint.listen({host:'127.0.0.1',families:['IPv4']});
  endpoint.on('session',session=>{ids.push(session.id);session.on('message',(lane,payload)=>{received.push({lane,bytes:Buffer.from(payload)});session.send(lane,payload);});session.resume();});
  const wait=async(fn)=>{const start=performance.now();while(!fn()){if(performance.now()-start>5000)throw Error('native socket fixture timed out');client.poll();await new Promise(r=>setTimeout(r,2));}};
  for(let connect=0;connect<2;connect++){
   client=new NativeSocket('127.0.0.1:'+port,seed,keys.id,JSON.stringify(hello));await wait(()=>client.poll());
   for(const lane of ['state','events','bulk'])client.send(lane,false,Buffer.from('socket '+lane));
   const messages=[];await wait(()=>{messages.push(...client.messages());return messages.length===3;});
   assert.deepEqual(messages.map(m=>m.lane).sort(),['bulk','events','state']);client.dispose();client=null;await new Promise(r=>setTimeout(r,10));
  }
  assert.deepEqual(ids,[identity(9).id,identity(9).id]);assert.equal(received.length,6);
 }finally{client?.dispose();await endpoint.close();}
});

// The same public synthetic field exercises the receiver on both targets.
// Native captures are explicit observers, never the executable's frame path.
test('native snapshot receiver matches WASM for maximum datagram fields and baseline deltas',async()=>{
 const {NativeSnapshot}=createRequire(import.meta.url)('../.transport-addon/bfe-transport.node');
 const {SnapshotClient}=await import('./helpers/network-capture.js');
 const {emptySnapshot,encodeEntities,DEFAULT_SNAPSHOT_BOUNDS}=await import('../src/runtime/snapshot-codec.js');
 const {NetHeader:H}=await import('../src/runtime/network-layout.js');
 const bounds=new Float64Array(['xMin','xExtent','zMin','zExtent','yMin','yExtent'].map(k=>DEFAULT_SNAPSHOT_BOUNDS[k]));
 const native=new NativeSnapshot(),page=new SnapshotClient();let base=null;
 for(let tick=2;tick<=10;tick+=2){const current=emptySnapshot(tick,1),payload=Uint8Array.from({length:1063},(_,i)=>(i+tick)%251);
  current.world.set('99:37',{schema:99,id:37,generation:1,payload});
  for(const bytes of encodeEntities(current,base)){const expected=page.receive(bytes),header=native.receive(Buffer.from(bytes),bounds);assert.equal(!!header[H.VALID],!!expected);assert.deepEqual(JSON.parse(native.capture(NaN)),page.capture());assert.deepEqual(JSON.parse(native.publication()),JSON.parse(page.network.kernel.capture_publication()));}
  base=current;
 }
 assert.equal(page.current.world.get('99:37').payload.length,1063);page.dispose();
});
