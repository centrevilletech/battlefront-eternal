// Receipt means durable acceptance into this bounded simulation queue, NOT
// execution. Reconciliation retires predicted actions only on executed ACKs.
// A full resend buffer (a server too far behind to receipt) gives up its
// oldest edge as expired, as age would, rather than failing the session.
export class EntityEdgeSender {
 constructor({capacity=32,maxAge=30}={}){this.capacity=capacity;this.maxAge=maxAge;this.pending=new Map();this.reports=[];this.life=null;this.highest=0;}
 reset(epoch,generation){for(const edge of this.pending.values())this.report({id:edge.id,status:'life-cancelled'});this.pending.clear();this.life={epoch,generation};this.highest=0;}
 add(edge){if(!this.life||edge.epoch!==this.life.epoch||edge.generation!==this.life.generation)throw Error('edge life mismatch');if(!Number.isSafeInteger(edge.id)||edge.id<1||!Number.isSafeInteger(edge.tick)||edge.tick<0)throw Error('invalid edge identity');if(edge.id<=this.highest)return false;if(this.pending.size>=this.capacity){const [oldest]=this.pending.keys();this.pending.delete(oldest);this.report({id:oldest,status:'expired'});}this.pending.set(edge.id,structuredClone(edge));this.highest=edge.id;return true;}
 receipt({epoch,generation,ids}){if(epoch!==this.life?.epoch||generation!==this.life?.generation)return;for(const id of ids)this.pending.delete(id);}
 batch(tick){for(const [id,e]of this.pending)if(e.tick<tick-this.maxAge){this.pending.delete(id);this.report({id,status:'expired'});}return structuredClone([...this.pending.values()]);}
 report(value){this.reports.push(value);if(this.reports.length>this.capacity)this.reports.shift();}
 takeReports(){return this.reports.splice(0);}
}
export class EntityEdgeReceiver {
 constructor({capacity=32,window=256,maxAge=30,maxFuture=30}={}){this.capacity=capacity;this.window=window;this.maxAge=maxAge;this.maxFuture=maxFuture;this.reset(0,0);}
 reset(epoch,generation){this.epoch=epoch;this.generation=generation;this.queue=new Map();this.seen=new Map();this.highest=0;}
 receive(edges,tick){const receipts=[],rejected=[];for(const e of edges){let reason;if(e.epoch!==this.epoch||e.generation!==this.generation)reason='life';else if(!Number.isSafeInteger(e.id)||e.id<1||!Number.isSafeInteger(e.tick))reason='invalid';else if(this.seen.has(e.id)){receipts.push(e.id);continue;}else if(e.id<=this.highest-this.window)reason='outside-window';else if(e.tick<tick-this.maxAge)reason='expired';else if(e.tick>tick+this.maxFuture)reason='future';else if(this.queue.size>=this.capacity)reason='capacity';if(reason){rejected.push({id:e.id,status:reason});continue;}this.queue.set(e.id,structuredClone(e));this.seen.set(e.id,true);this.highest=Math.max(this.highest,e.id);for(const id of this.seen.keys())if(id<=this.highest-this.window)this.seen.delete(id);receipts.push(e.id);}return {receipts,rejected};}
 take(tick){const edges=[];for(const[id,e]of this.queue)if(e.tick<=tick){edges.push(e);this.queue.delete(id);}return edges.sort((a,b)=>a.tick-b.tick||a.id-b.id);}
}
