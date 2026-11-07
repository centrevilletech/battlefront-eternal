import test from 'node:test';
import assert from 'node:assert/strict';
import {Session} from 'node:inspector/promises';
import {SnapshotClient} from '../src/runtime/snapshot-client.js';
import {NetworkSession} from '../src/runtime/network-session.js';
import {encodeOwnDatagram} from '../src/runtime/snapshot-own.js';
import {RecordMap,PacketLease} from '../src/runtime/network-records.js';
import {encodeEntities} from '../src/runtime/snapshot-codec.js';
import {snapshotFromRecords,SnapshotSlotTable} from '../src/runtime/snapshot-records.js';
const record={kind:'soldier',id:7,generation:1,tick:1,classId:1,team:1,position:[1,2,3],velocity:[0,0,0],yaw:.5,pitch:-.2,health:80,posture:0,state:0,weapon:1,fireCounter:0,charge:0};
function source(){const snapshot=snapshotFromRecords({tick:1,records:[record],slots:new SnapshotSlotTable()});const bytes=encodeEntities(snapshot,null)[0];return{bytes,header:new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength)};}
// Transport owns only the current borrow. Resource prerequisites explicitly
// retain rows; packet correction leases keep their original transport ACK.
test('retained records and deferred packet metadata survive publication rotation',()=>{
 const client=new SnapshotClient(),held=new RecordMap(),saved=new PacketLease(client.views),{bytes,header}=source();
 try{let packet=client.receive(bytes);const first=packet.records.get(0);held.set(first.id,first);saved.capture(packet);const tick=saved.tick,ack=saved.ackInput,x=first.position[0];
 for(let n=2;n<4200;n++){header.setUint32(6,n,true);header.setUint32(16,n,true);packet=client.receive(bytes);assert.equal(packet.records.length,1);}
 assert.equal(first.id,7);assert.equal(first.position[0],x);assert.equal(saved.tick,tick);assert.equal(saved.ackInput,ack);assert.equal(saved.records.get(0),first);
 held.clear();saved.release();header.setUint32(6,5000,true);assert.ok(client.receive(bytes));
 }finally{held.clear();saved.release();client.dispose();}
});
// Warm the optimized stores and the profiler independently. The measured
// receive path includes native decoding, packet metadata and lease routing;
// no diagnostic capture or synthetic replacement decoder participates.
test('snapshot receive and changed publication allocate no packet objects',async()=>{
 const client=new SnapshotClient(),{bytes,header}=source(),profiler=new Session(),transport=new NetworkSession({lifecycle:null});let tick=0;profiler.connect();
 transport.entityMode=true;transport.sendEntityAcknowledgement=()=>false;
 const own=encodeOwnDatagram({epoch:1,tick:1}),ownHeader=new DataView(own.buffer,own.byteOffset,own.byteLength);
 const receive=()=>{header.setUint32(6,++tick,true);client.receive(bytes);ownHeader.setUint32(6,tick,true);transport.setSnapshotAck(client.receive(own).ack);};
 const warm=async()=>{for(let b=0;b<64;b++){for(let n=0;n<128;n++)receive();await new Promise(resolve=>setTimeout(resolve,1));}};
 const options={samplingInterval:1,includeObjectsCollectedByMajorGC:true,includeObjectsCollectedByMinorGC:true};
 try{await profiler.post('HeapProfiler.collectGarbage');await warm();await profiler.post('HeapProfiler.startSampling',options);for(let n=0;n<128;n++)receive();await profiler.post('HeapProfiler.stopSampling');
 await profiler.post('HeapProfiler.collectGarbage');await warm();await profiler.post('HeapProfiler.startSampling',options);for(let n=0;n<512;n++)receive();
 const {profile}=await profiler.post('HeapProfiler.stopSampling'),allocations=[];
 const visit=node=>{if(node.selfSize&&/\/src\/(runtime\/(snapshot-client|network-records|page-network|network-session)|presentation-pkg\/bfe_presentation)\.js$/.test(node.callFrame.url))allocations.push({name:node.callFrame.functionName,line:node.callFrame.lineNumber+1,bytes:node.selfSize});for(const child of node.children)visit(child);};visit(profile.head);assert.deepEqual(allocations,[]);
 }finally{profiler.disconnect();transport.dispose();client.dispose();}
});

// Cold resource admission may grow presentation memory between deliveries.
// A retained lease must rebind its scalar/vector views and release commands
// without copying the packet or retaining a detached backing buffer.
test('retained leases survive presentation memory growth between packets',()=>{
 const client=new SnapshotClient(),held=new RecordMap(),{bytes,header}=source();
 try{
  const packet=client.receive(bytes),row=packet.records.get(0);held.set(row.id,row);
  const position=Array.from(row.position),id=row.id,old=row.position;
  assert.ok(client.network.memory.buffer instanceof ArrayBuffer);
  client.network.memory.grow(1);
  assert.equal(old.length,0);assert.equal(row.id,id);assert.deepEqual(Array.from(row.position),position);
  assert.equal(packet.records.get(0),row);assert.equal(packet.records.length,1);
  held.clear();header.setUint32(6,2,true);assert.ok(client.receive(bytes));
 }finally{held.clear();client.dispose();}
});
