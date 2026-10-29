import test from 'node:test';
import assert from 'node:assert/strict';
import {partitionSnapshotEvents,SnapshotTransients} from '../src/runtime/snapshot-transients.js';
import {SnapshotEvents} from '../src/runtime/snapshot-events.js';
const call=(target='sound',method='impact',args=[])=>[target,method,args];
const batch=(id,tick,calls=[call()],epoch=1)=>({epoch,tick,id,calls,generations:[]});

test('partition keeps all effect handles and persistent animation reliable without mutating fallback',()=>{
  const reliable=[call('fx','play',[1,'smoke']),...['move','stop','kill','tint'].map(name=>call('fxh',name,[1])),
    ...['reset','setBank','gunner','die','start','endWeaponAction'].map(name=>call('anim:1',name)),
    call('anim:1','weaponAction',['reload']),call('anim:1','weaponAction',['charge']),call('sound','fire',[{$c:1},{repair:true}]),call('hud','killMessage')];
  const transient=[call(),call('sound','fire',[{$c:1},{}]),call('sound','actorFire'),call('sound','turretFire'),call('lights','flash'),call('anim:1','fire'),call('anim:1','weaponAction',['shoot'])];
  const original=batch(1,10,[...reliable,...transient]),before=JSON.stringify(original),parts=partitionSnapshotEvents(original);
  assert.deepEqual(parts.reliable.calls,reliable);assert.deepEqual(parts.transient.calls,transient);assert.equal(JSON.stringify(original),before);
  // Without the optional capability, the original reliable batch still plays all.
  const fallback=new SnapshotEvents(),played=[];fallback.receive(original);fallback.drain({epoch:1,tick:10},()=>1,calls=>played.push(...calls));assert.deepEqual(played,original.calls);
});

test('transient reordering and duplicates never suppress reliable cleanup',()=>{
  const queue=new SnapshotTransients(),reliable=new SnapshotEvents(),played=[];
  queue.receive(batch(5,12));queue.receive(batch(3,10));queue.receive(batch(5,12));
  reliable.receive(batch(1,11,[call('fxh','stop',[8])]));
  queue.drain({epoch:1,tick:9},()=>1,calls=>played.push(...calls));assert.equal(played.length,0);
  queue.drain({epoch:1,tick:12},()=>1,calls=>played.push(...calls));assert.equal(played.length,2);
  reliable.drain({epoch:1,tick:12},()=>1,calls=>played.push(...calls));assert.deepEqual(played.at(-1),call('fxh','stop',[8]));
});

test('stalled, excessive and stale cosmetics drop within bounded memory without reset',()=>{
  const queue=new SnapshotTransients({maxBatches:3,maxBytes:2000});
  for(let id=1;id<=300;id++)queue.receive(batch(id,id));
  assert.ok(queue.queue.length<=3);assert.ok(queue.seen.size<=128);assert.ok(queue.bytes<=2000);
  queue.drain({epoch:1,tick:400},()=>1,()=>assert.fail('expired effects played'));
  assert.equal(queue.bytes,0);assert.equal(queue.queue.length,0);assert.ok(queue.dropped>0);
  queue.receive(batch(301,401,[call('sound','impact',['x'.repeat(2000)])]));assert.equal(queue.bytes,0);
});

test('transients respect generation, epoch and applied frontier before presentation',()=>{
  const queue=new SnapshotTransients(),played=[];
  queue.receive({...batch(1,10,[call('anim:1','fire')]),generations:[[1,2]]});
  queue.drain({epoch:1,tick:10},()=>3,calls=>played.push(...calls));assert.equal(played.length,0);
  queue.receive(batch(2,12));queue.receive(batch(1,1,[call()],2));queue.receive(batch(3,13));
  queue.drain({epoch:1,tick:100},()=>1,calls=>played.push(...calls));assert.equal(played.length,0);
  queue.drain({epoch:2,tick:1},()=>1,calls=>played.push(...calls));assert.equal(played.length,1);
  assert.throws(()=>queue.receive(batch(2,2,[call('fxh','kill',[1])],2)),/Invalid/);
  queue.reset();assert.equal(queue.epoch,null);assert.equal(queue.seen.size,0);
});
