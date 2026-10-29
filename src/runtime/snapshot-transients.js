import {snapshotEventCalls} from './snapshot-journal.js';

// Only stateless, short-lived presentation belongs on the optional lossy lane.
// Keep effect handles and persistent animator transitions ordered/reliable.
export function transientSnapshotCall([target,method,args]) {
  if(target==='sound')return method==='impact' || ['actorFire','turretFire'].includes(method) || method==='fire' && !args[1]?.repair;
  if(target==='lights')return method==='flash';
  return target.startsWith('anim:') && (method==='fire' || method==='weaponAction' && args[0]==='shoot');
}
export function partitionSnapshotEvents(batch) {
  const reliable=[],transient=[];
  for(const call of batch.calls)(transientSnapshotCall(call)?transient:reliable).push(call);
  return {reliable:{...batch,calls:reliable},transient:{...batch,calls:transient}};
}

// Independent of reliable event sequence numbers: a newer cosmetic must never
// suppress an older cleanup. Reordering is fine inside the short visual window;
// duplication, delayed gunfire and overload are discarded without resyncing.
export class SnapshotTransients {
  constructor({maxAgeTicks=12,maxBatches=32,maxBytes=131072}={}) {
    this.maxAgeTicks=maxAgeTicks;this.maxBatches=maxBatches;this.maxBytes=maxBytes;this.reset();
  }
  reset(){this.epoch=null;this.latestTick=-1;this.appliedTick=-1;this.latestId=0;this.seen=new Set();this.queue=[];this.bytes=0;this.dropped=0;}
  receive(batch) {
    if(!batch || !Number.isSafeInteger(batch.epoch)||batch.epoch<0 || !Number.isSafeInteger(batch.tick)||batch.tick<0 ||
      !Number.isSafeInteger(batch.id)||batch.id<1 || !Array.isArray(batch.calls)||batch.calls.length>2048 ||
      !batch.calls.every(call=>Array.isArray(call)&&typeof call[0]==='string'&&typeof call[1]==='string'&&Array.isArray(call[2])&&transientSnapshotCall(call)) ||
      !Array.isArray(batch.generations||[]) || !(batch.generations||[]).every(pair=>Array.isArray(pair)&&pair.length===2&&pair.every(Number.isSafeInteger)))throw Error('Invalid transient snapshot events');
    if(this.epoch!==null && batch.epoch<this.epoch)return;
    if(batch.epoch!==this.epoch){this.reset();this.epoch=batch.epoch;}
    if(this.seen.has(batch.id)||batch.id<=this.latestId-128)return;
    this.latestId=Math.max(this.latestId,batch.id);this.seen.add(batch.id);
    for(const id of this.seen)if(id<=this.latestId-128)this.seen.delete(id);
    this.latestTick=Math.max(this.latestTick,batch.tick);
    const bytes=JSON.stringify(batch).length*2;
    if(bytes>this.maxBytes){this.dropped+=batch.calls.length;this.prune();return;}
    this.queue.push({batch,bytes});this.bytes+=bytes;
    this.queue.sort((a,b)=>a.batch.tick-b.batch.tick||a.batch.id-b.batch.id);
    this.prune();
  }
  prune() {
    const oldest=Math.max(this.latestTick,this.appliedTick)-this.maxAgeTicks;
    while(this.queue.length && (this.queue[0].batch.tick<oldest||this.queue.length>this.maxBatches||this.bytes>this.maxBytes)) {
      const entry=this.queue.shift();this.bytes-=entry.bytes;this.dropped+=entry.batch.calls.length;
    }
  }
  drain({epoch,tick},generationOf,play) {
    if(epoch!==this.epoch)return;
    this.appliedTick=Math.max(this.appliedTick,tick);this.prune();
    while(this.queue.length&&this.queue[0].batch.tick<=this.appliedTick) {
      const {batch,bytes}=this.queue.shift();this.bytes-=bytes;
      const calls=snapshotEventCalls(batch,generationOf);if(calls.length)play(calls);
    }
  }
}
