import {snapshotEventCalls} from './snapshot-journal.js';
import {transientSnapshotCall} from './snapshot-transients.js';

// Events remain reliable while replaceable snapshots may overtake them.
// Present only once their world tick exists, and never animate a newer life.
export class SnapshotEvents {
  constructor(){this.reset();}
  reset(){this.epoch=null;this.received=0;this.queue=[];this.bytes=0;this.droppedCosmetics=0;}
  receive(batch){
    if(!batch||!Number.isSafeInteger(batch.epoch)||!Number.isSafeInteger(batch.tick)||!Number.isSafeInteger(batch.id)||batch.epoch<0||batch.tick<0||batch.id<1||!Array.isArray(batch.calls)||batch.calls.length>2048||!batch.calls.every(call=>Array.isArray(call)&&typeof call[0]==='string'&&typeof call[1]==='string'&&Array.isArray(call[2]))||!Array.isArray(batch.generations||[])||!(batch.generations||[]).every(pair=>Array.isArray(pair)&&pair.length===2&&pair.every(Number.isSafeInteger)))throw Error(`Invalid snapshot events (epoch=${batch?.epoch}, tick=${batch?.tick}, id=${batch?.id}, calls=${batch?.calls?.length}, invalidTuple=${Array.isArray(batch?.calls) ? batch.calls.findIndex(call => !Array.isArray(call) || typeof call[0] !== 'string' || typeof call[1] !== 'string' || !Array.isArray(call[2])) : -1})`);
    if(this.epoch!==null&&batch.epoch<this.epoch)return;
    if(batch.epoch!==this.epoch){this.reset();this.epoch=batch.epoch;}
    if(batch.id<=this.received)return;
    this.prune(batch.tick - 120);
    const bytes=JSON.stringify(batch).length;
    if(this.bytes+bytes>1048576)this.prune(Infinity);
    if(this.queue.length>=2048||this.bytes+bytes>1048576)throw Error('Snapshot event backlog exceeded');
    this.received=batch.id;this.bytes+=bytes;this.queue.push({batch,bytes});
  }
  prune(beforeTick){
    this.queue=this.queue.filter(entry=>{
      if(entry.batch.tick>=beforeTick)return true;
      // A rendering stall must not erase persistent animator transitions or
      // effect-handle lifecycles. Only proven stateless cosmetics may expire.
      const calls=entry.batch.calls.filter(call=>!transientSnapshotCall(call));
      this.droppedCosmetics+=entry.batch.calls.length-calls.length;
      this.bytes-=entry.bytes;entry.batch={...entry.batch,calls};entry.bytes=calls.length?JSON.stringify(entry.batch).length:0;this.bytes+=entry.bytes;
      return calls.length>0;
    });
  }
  drain({epoch,tick},generationOf,play){
    if(epoch!==this.epoch)return;
    while(this.queue.length&&this.queue[0].batch.tick<=tick){
      const {batch,bytes}=this.queue.shift();this.bytes-=bytes;
      play(snapshotEventCalls(batch,generationOf));
    }
  }
}
