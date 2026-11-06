import test from 'node:test';
import assert from 'node:assert/strict';
import { F4Mirrors } from '../src/f4-mirrors.js';
import { Heap } from '../src/sim/runtime.js';
import { F4BuildingViewLayout, F4BuildingViewMirror, F4DeltaLayout, putF4BuildingView, syncF4BuildingView, putF4Delta } from '../src/sim/gen/f4.js';
const record = { words: F4BuildingViewLayout.words, put: putF4BuildingView, sync: syncF4BuildingView };
const handle = (generation = 1, life = 1) => ({ kind: 'building', slot: 0, generation, life });
function block(target, values, mask = [1], retired = false) {
  const heap = new Heap(), row = new Float64Array(F4DeltaLayout.words);
  putF4Delta(null, { target, retired, source:0, borrowedWords:0, length: record.words, mask, values }, row, 0, heap);
  return new Float64Array([1, row.length, 5 + row.length, heap.words.length, retired ? 1 : 0, ...row, ...heap.words]);
}
function initial(view) {
  const row = new Float64Array(record.words); putF4BuildingView(null, view, row, 0, new Heap()); return [...row];
}
test('sparse publication preserves facade identity and detects an owned scalar write', () => {
  const mirrors = new F4Mirrors(), view = new F4BuildingViewMirror(), state = view.state, building = state.building;
  const row = mirrors.bind(handle(), record, view);
  mirrors.apply(block(handle(), initial(view), [(1 << record.words) - 1]), 0);
  assert.equal(view.state, state); assert.equal(view.state.building, building); mirrors.check();
  view.state.pulse = 7;
  assert.throws(() => mirrors.check(), /mirror write/);
  assert.equal(row.handle.life, 1);
});
test('retired publication reaches its old facade after a slab slot is rebound', () => {
  const mirrors = new F4Mirrors(), old = new F4BuildingViewMirror(), next = new F4BuildingViewMirror();
  const a = mirrors.bind(handle(), record, old); const b = mirrors.bind(handle(2), record, next);
  mirrors.apply(block(handle(), initial(old), [(1 << record.words) - 1], true), 0);
  assert.equal(a.retired, true); assert.equal(b.published, false);
  mirrors.apply(block(handle(2), initial(next), [(1 << record.words) - 1]), 0);
  assert.equal(b.published, true); mirrors.check();
});
test('rebuild updates the gameplay life and rejects an old publication', () => {
  const mirrors = new F4Mirrors(), view = new F4BuildingViewMirror(), row = mirrors.bind(handle(), record, view);
  mirrors.apply(block(handle(), initial(view), [(1 << record.words) - 1]), 0);
  mirrors.apply(block(handle(1, 2), [], []), 0);
  assert.equal(row.handle.life, 2);
  assert.throws(() => mirrors.apply(block(handle(), [], []), 0), /stale/);
});
test('publication counters count transferred row and heap words', () => {
  const mirrors = new F4Mirrors(), view = new F4BuildingViewMirror(); mirrors.bind(handle(), record, view);
  const f = block(handle(), initial(view), [(1 << record.words) - 1]); mirrors.apply(f, 0);
  assert.equal(mirrors.rows, 1); assert.equal(mirrors.words, f.length - 5);
});

test('animated victim records retain their backing object through release and regrab', async () => {
  const { F4AnimatedViewLayout, F4AnimatedViewMirror, putF4AnimatedView, syncF4AnimatedView } = await import('../src/sim/gen/f4.js');
  const view = new F4AnimatedViewMirror(), victim = view.victim, heap = new Heap();
  const row = new Float64Array(F4AnimatedViewLayout.words);
  for (const held of [true, false, true]) {
    view.hasVictim = held; view.victim.unit = 7;
    putF4AnimatedView(null, view, row, 0, heap);
    syncF4AnimatedView(null, row, 0, row.length, view);
    assert.equal(view.victim, victim); assert.equal(view.hasVictim, held);
  }
});

test('retained counted blocks decode variable heaps through the shared record ABI', async () => {
  const { F4DeltaMirror, putF4Delta, syncF4Delta } = await import('../src/sim/gen/f4.js');
  const view = new F4DeltaMirror(), values = view.values;
  const record = { block: true, capacity: 128, words: F4DeltaLayout.words, put: putF4Delta, sync: syncF4Delta };
  const mirrors = new F4Mirrors(); mirrors.bind(handle(), record, view);
  function encoded(innerValues) {
    const heap = new Heap(), row = new Float64Array(record.words);
    putF4Delta(null, { target: handle(), retired: false, source:0, borrowedWords:0, length: 3, mask: [7], values: innerValues }, row, 0, heap);
    return new Float64Array([1, row.length, 5 + row.length, heap.words.length, 0, ...row, ...heap.words]);
  }
  function publication(next, old) {
    const mask = Array(Math.ceil(next.length / 32)).fill(0), changed = [];
    for (let i = 0; i < next.length; i++) if (!old || !Object.is(next[i], old[i])) { mask[i >>> 5] |= 1 << (i & 31); changed.push(next[i]); }
    const heap = new Heap(), row = new Float64Array(F4DeltaLayout.words);
    putF4Delta(null, { target: handle(), retired: false, source:0, borrowedWords:0, length: next.length, mask, values: changed }, row, 0, heap);
    return new Float64Array([1, row.length, 5 + row.length, heap.words.length, 0, ...row, ...heap.words]);
  }
  const initial = encoded([1, 2, 3]); mirrors.apply(publication(initial), 0);
  assert.deepEqual(view.values, [1, 2, 3]); assert.equal(view.values, values); mirrors.check();
  const empty = encoded([]); mirrors.apply(publication(empty, initial), 0);
  assert.deepEqual(view.values, []); assert.equal(view.values, values); mirrors.check();
  const grown = encoded([4, 5, 6, 7]); mirrors.apply(publication(grown, empty), 0);
  assert.deepEqual(view.values, [4, 5, 6, 7]); assert.equal(view.values, values); mirrors.check();
  const burstValues=Array.from({length:16384},(_,i)=>i),burst=encoded(burstValues);
  mirrors.apply(publication(burst,grown),0);
  assert.deepEqual(view.values,burstValues);assert.equal(view.values,values);mirrors.check();
  mirrors.apply(publication(empty,burst),0);
  assert.deepEqual(view.values,[]);assert.equal(view.values,values);mirrors.check();
});

test('masked Character readers retain nested tables through value changes and heap relocation', async()=>{
  const { SimContext } = await import('../src/sim/runtime.js');
  const { Character } = await import('../src/battle.js');
  const G = await import('../src/sim/gen/index.js');
  const cx = new SimContext({classes:{Character}}), source = new Character(0,1), view = new Character(0,1), victim = new Character(1,2);
  const strings=[],ids=new Map();
  cx.strings={id:s=>{if(!ids.has(s)){ids.set(s,strings.length);strings.push(s);}return ids.get(s);},at:i=>strings[i]};
  cx.refs[G.KIND_CHARACTER].slotOf(view);cx.refs[G.KIND_CHARACTER].slotOf(victim);
  const stats = new G.RecordMirror(), first = new G.StatSlotMirror(), second = new G.StatSlotMirror();
  first.victims.set(victim,1);first.classes.push({key:'first',label:null,time:1});
  second.classes.push({key:'other',label:'Other',time:7});
  stats.slots.push(first,second);source.record=stats;
  const target={kind:'character',slot:0,generation:1,life:1};
  const mirrors=new F4Mirrors(cx);
  mirrors.bind(target,{block:true,capacity:1024,words:G.CHARACTER_EXPORT_STRIDE,put:G.putCharacter,
    sync:G.syncCharacter,syncDelta:G.syncCharacterDelta,syncDeferred:G.syncCharacterDeferred,
    removedDeferred:G.removedCharacterDeferred,changedDeferred:G.changedCharacterDeferred,markDeferred:G.markCharacterDeferred,identityDeferred:G.identityCharacterDeferred,check:G.checkCharacter},view);
  let previous=null;
  const publish=(retired=false)=>{
    const heap=new Heap(),row=new Float64Array(G.CHARACTER_EXPORT_STRIDE);
    G.putCharacter(cx,source,row,0,heap);
    const next=new Float64Array([1,row.length,5+row.length,heap.words.length,0,...row,...heap.words]);
    const mask=Array(Math.ceil(next.length/32)).fill(0),values=[];
    for(let i=0;i<next.length;i++)if(!previous||!Object.is(previous[i],next[i])){mask[i>>>5]|=1<<(i&31);values.push(next[i]);}
    const output=new Float64Array(F4DeltaLayout.words),payload=new Heap();
    const sourceAt=5+output.length+mask.length;
    putF4Delta(cx,{target,retired,length:next.length,source:sourceAt*8,borrowedWords:values.length,mask,values:[]},output,0,payload);
    const shared=new Float64Array([1,output.length,5+output.length,payload.words.length,0,...output,...payload.words,...next]);
    mirrors.apply(shared,0);previous=next;return shared;
  };
  publish();
  const kept=view.record,slots=kept.slots,victims=slots[0].victims,classes=slots[0].classes,entry=classes[0];
  const binding=mirrors.kinds[G.F4ObjectKind_INDEX.get('character')][0];
  assert.equal(binding.observer.pending,true);
  for(let i=2;i<5;i++){first.classes[0].time=i;publish();assert.equal(binding.observer.eagerChanged,false);}
  assert.equal(binding.observer.pending,true);assert.equal(entry.time,4);assert.equal(binding.observer.pending,false);
  assert.deepEqual(Object.keys(slots[0]),Object.keys(first));

  // Neither edit changes a list header or a Character's inline scalar. The
  // enclosing mask must still reach the map value and the nested time entry.
  first.victims.set(victim,2);first.classes[0].time=3;publish();
  assert.equal(victims.get(victim),2);assert.equal(entry.time,3);
  first.classes.push({key:'second',label:'Second',time:9});publish();
  assert.equal(classes.length,2);assert.equal(binding.observer.pending,true);
  assert.equal(slots[1].classes[0].key,'other');assert.equal(slots[1].classes[0].time,7);
  assert.equal(classes[1].key,'second');assert.equal(classes[1].time,9);
  first.victims.clear();publish();
  assert.equal(victims.size,0);assert.equal(slots[1].classes[0].time,7);
  assert.ok(view.record===kept&&view.record.slots===slots&&slots[0].classes===classes&&classes[0]===entry);
  first.classes[0].time=11;publish();assert.equal(binding.observer.pending,true);
  source.record=null;publish();assert.equal(view.record,null);assert.equal(entry.time,11);
  source.record=stats;publish();assert.equal(view.record.slots[0].classes[1].time,9);
  const currentRecord=view.record;
  first.classes[0].time=11.5;publish();view.record=null;
  assert.throws(()=>mirrors.check(),/mirror write/);view.record=currentRecord;
  const current=view.record.slots[0].classes[0];assert.equal(current.time,11.5);current.time=99;
  assert.throws(()=>mirrors.check(),/mirror write/);current.time=11.5;mirrors.check();
  for(let i=0;i<4096;i++)first.classes.push({key:'burst'+i,label:null,time:i});
  publish();assert.equal(view.record.slots[0].classes.length,4098);
  assert.equal(view.record.slots[0].classes[4097].time,4095);
  first.classes[0].time=12;publish();const shared=publish(true);
  shared.fill(0);assert.equal(current.time,12);assert.equal(binding.observer.entry,null);
});

test('borrowed publications reject a source outside the shared memory',()=>{
  const view=new F4BuildingViewMirror(),mirrors=new F4Mirrors();mirrors.bind(handle(),record,view);
  const heap=new Heap(),row=new Float64Array(F4DeltaLayout.words);
  putF4Delta(null,{target:handle(),retired:false,length:record.words,source:8000000,borrowedWords:0,mask:[],values:[]},row,0,heap);
  const f=new Float64Array([1,row.length,5+row.length,heap.words.length,0,...row,...heap.words]);
  assert.throws(()=>mirrors.apply(f,0),/invalid borrowed/);
});
