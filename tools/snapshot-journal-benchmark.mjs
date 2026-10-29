// Offline replay of operator-captured common packets; contains no game assets.
// node tools/snapshot-journal-benchmark.mjs /private/capture-directory
import {readFile,readdir} from 'node:fs/promises';
import {join} from 'node:path';
import {deflateSync} from 'node:zlib';
import {SnapshotJournal,SnapshotReceiver} from '../src/runtime/snapshot-journal.js';
const directory=process.argv[2];if(!directory)throw Error('Provide a directory of tick packet captures');
const names=(await readdir(directory)).filter(n=>n.endsWith('.bin')).sort((a,b)=>parseInt(a.split('-')[1])-parseInt(b.split('-')[1]));
const packets=await Promise.all(names.map(async name=>{const b=await readFile(join(directory,name));return b.buffer.slice(b.byteOffset,b.byteOffset+b.byteLength);}));
const journal=new SnapshotJournal({relativeVectors:process.env.BFE_RELATIVE_VECTORS==='1'}),receiver=new SnapshotReceiver(),times={capture:[],encode:[],receive:[]};
let baseTick=null,bytes=0,eventBytes=0,received=0,startTick=null,lastTick=0;
for(let index=0;index<packets.length;index++) {
 const begin=performance.now(),events=journal.push(packets[index]),captured=performance.now();
 const checkpoint=baseTick===null||events.tick%15===0;
 const wire=journal.encode({baseTick,checkpoint}),encoded=performance.now();
 // Keep checkpoints; lose roughly 20% of other messages and deliver a later
 // packet independently. This exercises immutable baseline reconstruction.
 const result=index%5===2&&!checkpoint?null:receiver.receive(wire),finished=performance.now();
 if(result?.checkpoint)baseTick=result.tick;
 if(result)received++;
 if(index>20){times.capture.push(captured-begin);times.encode.push(encoded-captured);if(result)times.receive.push(finished-encoded);bytes+=deflateSync(new Uint8Array(wire),{level:6}).length;eventBytes+=deflateSync(JSON.stringify(events),{level:6}).length;startTick??=events.tick;lastTick=events.tick;}
}
const stats=values=>{const sorted=values.toSorted((a,b)=>a-b);return {mean:values.reduce((a,b)=>a+b,0)/values.length,p95:sorted[Math.floor(sorted.length*.95)],max:sorted.at(-1)};};
console.log(JSON.stringify({packets:packets.length,received,phasesMs:Object.fromEntries(Object.entries(times).map(([k,v])=>[k,stats(v)])),stateBytesPerSecond:bytes/((lastTick-startTick+3)/60),eventBytesPerSecond:eventBytes/((lastTick-startTick+3)/60),retainedSnapshots:journal.snapshots.size,retainedCheckpoints:receiver.checkpoints.size,rssMiB:process.memoryUsage().rss/1048576},null,2));
