import test from 'node:test';
import assert from 'node:assert/strict';
import { UdpTransport } from '../src/runtime/udp-transport.js';
import { NetworkHost } from '../src/runtime/network-host.js';

const serverId='a'.repeat(64),token='b'.repeat(32),query=`?map=nab2&side=rep&match=${token}`;
function fixture(){
  const listeners=new Set(),requests=[];
  const bridge={onSignal(fn){listeners.add(fn);return()=>listeners.delete(fn);},async request(action,value){requests.push({action,value});return true;}};
  const transport=new UdpTransport({bridge,serverId});transport.closed=true;
  return {transport,listeners,requests,bridge,emit:message=>{for(const fn of listeners)fn({type:'matchReady',...message});}};
}

test('online readiness accepts only the expected identity, launch token and fresh probe nonce',async()=>{
  const f=fixture();
  const waiting=f.transport.matchReady(query);await new Promise(resolve=>setImmediate(resolve));
  const sent=f.requests.find(r=>r.action==='signal'),probe=JSON.parse(sent.value.payload);
  assert.equal(sent.value.type,'matchProbe','readiness must not depend on an expired ICE candidate route');
  assert.equal(probe.type,'matchProbe');
  assert.equal(probe.matchProbe,token);
  for(const message of [
    {from:serverId,type:'candidate',payload:JSON.stringify({type:'matchReady',matchReady:token,nonce:probe.nonce})},
    {from:'c'.repeat(64),payload:JSON.stringify({type:'matchReady',matchReady:token,nonce:probe.nonce})},
    {from:serverId,payload:JSON.stringify({type:'matchReady',matchReady:'d'.repeat(32),nonce:probe.nonce})},
    {from:serverId,payload:JSON.stringify({type:'matchReady',matchReady:token,nonce:'e'.repeat(32)})},
    {from:serverId,direct:true,payload:JSON.stringify({type:'matchReady',matchReady:token,nonce:probe.nonce})},
    {from:serverId,payload:'malformed'},
  ])f.emit(message);
  assert.equal(f.listeners.size,1);
  f.emit({from:serverId,payload:JSON.stringify({type:'matchReady',matchReady:token,nonce:probe.nonce})});
  assert.equal(await waiting,true);
  assert.equal(f.listeners.size,0);
  f.transport.cancelMatchProbe();
});

test('readiness probe timeout and page disposal unsubscribe without claiming readiness',async()=>{
  const f=fixture();
  assert.equal(await f.transport.matchReady(query),false);
  assert.equal(f.listeners.size,0,'a silent server leaves no listener behind');
  const pending=f.transport.matchReady(query);await Promise.resolve();
  assert.equal(f.listeners.size,1);
  f.transport.cancelMatchProbe();
  assert.equal(await pending,false);assert.equal(f.listeners.size,0);
  assert.equal(await f.transport.matchReady(query),false);
});

test('LAN readiness requires the advertised identity and exact launch, even when the map repeats',async()=>{
  let info={id:serverId,query};
  const calls=[],transport=new UdpTransport({serverId,address:'127.0.0.1',port:18451,bridge:{async request(action,value){calls.push({action,value});return info;}}});
  transport.closed=false;
  assert.equal(await transport.matchReady(query),true);
  info={id:serverId,query:query.replace(token,'c'.repeat(32))};
  assert.equal(await transport.matchReady(query),false);
  info={id:'d'.repeat(64),query};assert.equal(await transport.matchReady(query),false);
  info={id:serverId,query:query.replace('nab2','bes2')};assert.equal(await transport.matchReady(query),false);
  assert.ok(calls.every(call=>call.action==='directInfo'&&call.value.port===18451));
});

test('a ready next server advances the session even before the old peer reports closed',async(t)=>{
  const f=fixture(),previous=globalThis.location,navigated=[];
  f.transport.closed=false;
  globalThis.location={search:'?network=1&server='+serverId,assign:url=>navigated.push(url)};
  t.after(()=>{globalThis.location=previous;});
  const request=f.bridge.request;
  f.bridge.request=async(action,value)=>{
    const result=await request(action,value);
    if(action==='signal'){
      const probe=JSON.parse(value.payload);
      queueMicrotask(()=>f.emit({from:serverId,payload:JSON.stringify({type:'matchReady',matchReady:token,nonce:probe.nonce})}));
    }
    return result;
  };
  const session=new NetworkHost({transport:f.transport});t.after(()=>session.dispose());
  await session.receive('events',JSON.stringify({type:'match',query,delayMs:0}));
  const deadline=performance.now()+1000;
  while(!navigated.length){assert.ok(performance.now()<deadline);await new Promise(resolve=>setTimeout(resolve,2));}
  assert.equal(session.closed,false);
  assert.equal(f.transport.closed,false);
  assert.equal(navigated.length,1);
  assert.equal(new URLSearchParams(navigated[0].slice(2)).get('match'),token);
});

test('a late old-peer close preserves an in-flight readiness probe',async()=>{
  const f=fixture();f.transport.closed=false;
  const pending=f.transport.matchReady(query);await new Promise(resolve=>setImmediate(resolve));
  const probe=JSON.parse(f.requests.find(request=>request.action==='signal').value.payload);
  f.transport.dispose();
  assert.equal(f.requests.some(request=>request.action==='close'),false,'old RTC disposal does not close the new signaling connection');
  assert.equal(f.listeners.size,1);
  f.emit({from:serverId,payload:JSON.stringify({type:'matchReady',matchReady:token,nonce:probe.nonce})});
  assert.equal(await pending,true);
  f.transport.cancelMatchProbe();
  assert.equal(f.requests.at(-1).action,'close','actual page departure closes it');
});

test('leaving during signaling connection setup cannot start a late readiness listener',async()=>{
  const f=fixture();let connected;
  f.bridge.request=async action=>{
    f.requests.push({action});
    if(action==='connect')await new Promise(resolve=>{connected=resolve;});
    return true;
  };
  const pending=f.transport.matchReady(query);
  f.transport.cancelMatchProbe();
  connected();
  assert.equal(await pending,false);
  assert.equal(f.listeners.size,0);
  assert.equal(f.requests.some(request=>request.action==='signal'),false);
  assert.equal(f.requests.at(-1).action,'close');
});

test('a closed signaling socket is reconnected before the next readiness attempt',async()=>{
  const f=fixture();let signals=0;
  const request=f.bridge.request;
  f.bridge.request=async(action,value)=>{
    await request(action,value);
    return action!=='signal'||++signals>1;
  };
  assert.equal(await f.transport.matchReady(query),false);
  assert.equal(f.transport.rotationConnect,null);
  assert.equal(f.listeners.size,0);
  const pending=f.transport.matchReady(query);await new Promise(resolve=>setImmediate(resolve));
  assert.equal(f.requests.filter(request=>request.action==='connect').length,2);
  const probe=JSON.parse(f.requests.filter(request=>request.action==='signal').at(-1).value.payload);
  f.emit({from:serverId,payload:JSON.stringify({type:'matchReady',matchReady:token,nonce:probe.nonce})});
  assert.equal(await pending,true);
  f.transport.cancelMatchProbe();
});
