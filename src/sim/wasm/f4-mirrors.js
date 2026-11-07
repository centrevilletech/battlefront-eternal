// Compact live-object publications retain one encoded row per facade. Generated
// readers mutate its nested records in place; unchanged objects copy no words.
import { F4DeltaLayout as D, F4HandleLayout as H, F4ObjectKind_INDEX } from '../gen/f4.js';
import { Heap } from '../runtime.js';
import { BlockHeaderLayout as B } from '../gen/boundary.js';

const HEADER = B.words;
const MASK_BITS = 32;
// A pending detailed record always reads the latest complete native cache.
// Intermediate masks need no accumulation; ordinary readers materialize the
// record in place before observing it, preserving every nested identity.
class Observation {
  constructor(entry, context) {
    this.entry=entry;this.context=context;this.pending=false;this.reading=false;this.eagerChanged=true;this.prepared=false;this.expected=null;
    this.eager=new Uint32Array(entry.dirty.length);
  }
  changes(dirty, words) {
    for(let i=0;i<words;i++)if(dirty[i]&this.eager[i])return true;
    return false;
  }
  prepare(f,start,heap,fresh) {
    const e=this.entry;
    this.eagerChanged=!this.prepared||fresh||this.changes(e.dirty,e.dirtyWords);
    this.pending=this.pending||fresh||e.record.changedDeferred(f,start,heap,e.dirty,e.source);
  }
  // Map values and list lengths stay immediate. Rebuild their eager mask on
  // a shape edge; ordinary clock changes do not visit those collections.
  shape(f,start,heap) {
    if(!this.eagerChanged)return;
    const e=this.entry;
    if(this.eager.length<e.dirty.length)this.eager=new Uint32Array(e.dirty.length);
    else this.eager.fill(0);
    e.record.markDeferred(f,start,heap,this.eager,e.source);this.prepared=true;
    this.expected=e.record.identityDeferred(e.view);
  }
  // Held statistics may outlive a Character or battle. Once materialized,
  // they must not keep its native boundary and entire game reachable.
  detach() { this.pending=false;this.entry=null;this.context=null;this.eager=null;this.expected=null; }
  flush() {
    if(!this.pending||this.reading)return;
    const e=this.entry,b=this.context;
    if(this.prepared&&e.record.identityDeferred(e.view)!==this.expected)throw Error('live-object mirror write: detailed record');
    if(b?.views)b.views();
    const f=e.source?(b?.f??e.buffer):e.row,start=e.source+HEADER,heap=e.source+(f[e.source+B.heap]|0);
    this.pending=false;this.reading=true;
    try { e.record.syncDeferred(b,f,start,heap,e.view); }
    finally { this.reading=false; }
  }
}
export class F4Mirrors {
  constructor(context = null) {
    this.context = context;
    this.kinds = [];
    this.heap = new Heap();
    this.rows = this.words = 0;
  }
  // Bind during construction. A replaced slot retains its previous facade
  // until that life's retired publication arrives, even if rebirth came first.
  bind(handle, record, view, publish = null) {
    const kind = F4ObjectKind_INDEX.get(handle.kind);
    if (kind === undefined) throw Error('unknown live-object kind');
    const slots = this.kinds[kind] ??= [];
    const previous = slots[handle.slot];
    if (previous?.handle.generation === handle.generation) throw Error('live-object slot already bound');
    const entry = { handle: { ...handle }, record, view, publish, previous: previous?.retired ? previous.previous : previous, retired: false,
      // Detailed records always lend their native encoding; reserve no second
      // numeric image for a facade that will never copy packed values.
      row: new Float64Array(record.syncDeferred ? 0 : record.capacity ?? record.words + (record.block ? HEADER : 0)),
      dirty: record.syncDelta ? new Uint32Array(Math.ceil((record.capacity ?? record.words + HEADER) / MASK_BITS)) : null, dirtyWords: 0,
      length: 0, source: 0, buffer: null, check: new Float64Array(record.words), published: false };
    entry.observer = record.syncDeferred ? new Observation(entry,this.context) : null;
    slots[handle.slot] = entry;
    return entry;
  }
  // The phase descriptor supplies a counted block. All offsets, including
  // variable mask/value payloads, come from the generated layouts.
  apply(f, base) {
    base |= 0;
    const rows = f[base + B.rows] | 0, stride = f[base + B.stride] | 0, heap = base + (f[base + B.heap] | 0);
    if (stride !== D.words) throw Error('live-object publication stride');
    this.rows += rows; this.words += rows * stride + f[base + B.heapWords];
    for (let i = 0, at = base + HEADER; i < rows; i++, at += stride) {
      const target = at + D.target, kind = f[target + H.kind] | 0, slot = f[target + H.slot] | 0;
      const generation = f[target + H.generation], life = f[target + H.life];
      let entry = this.kinds[kind]?.[slot], newer = null;
      while (entry && entry.handle.generation !== generation) { newer = entry; entry = entry.previous; }
      if (!entry) throw Error('publication has no bound live-object life');
      if (life < entry.handle.life) throw Error('stale live-object publication');
      const length = f[at + D.length] | 0;
      if (entry.record.block ? length < HEADER + entry.record.words : length !== entry.record.words) throw Error('live-object publication width');
      // Shared-memory cursors are integer word indices, as in the unit
      // reader. Keep them unboxed through nested generated record calls.
      const address = f[at + D.source], source = (address / 8) | 0;
      if (address && (address !== source * 8 || source < 0 || source + length > f.length)) throw Error('invalid borrowed live-object row');
      if(entry.observer&&!source)throw Error('detailed readers require a borrowed native row');
      if (!source && length > entry.row.length) {
        // Native variable records retain their own ids. A rare content or
        // notice backlog may exceed the cold estimate; grow this encoding
        // without replacing its facade or freezing the page at capacity.
        const row = new Float64Array(length); row.set(entry.row); entry.row = row;
      }
      const mask = heap + (f[at + D.mask] | 0), masks = f[at + D.maskLength] | 0;
      if (entry.dirty) {
        if (entry.dirty.length < masks) entry.dirty = new Uint32Array(masks);
        if (masks < entry.dirtyWords) entry.dirty.fill(0,masks,entry.dirtyWords);
        entry.dirtyWords = masks;
      }
      let value = heap + (f[at + D.values] | 0);
      const end = value + (f[at + D.valuesLength] | 0);
      for (let m = 0; m < masks; m++) {
        let bits = f[mask + m] >>> 0;
        if (entry.dirty) entry.dirty[m] = bits;
        // Battle rows already retain every unchanged word in native memory.
        // Fixed live records keep their small packed scalar handoff.
        if (source) continue;
        while (bits) {
          const bit = 31 - Math.clz32(bits & -bits), word = m * MASK_BITS + bit;
          if (word >= length || value >= end) throw Error('invalid live-object publication mask');
          entry.row[word] = f[value++]; bits &= bits - 1;
        }
      }
      if (value !== end) throw Error('unused live-object publication values');
      this.words += f[at + D.borrowedWords];
      const fresh = !entry.published || entry.handle.life !== life;
      let observer=entry.observer;
      const start = source + (entry.record.block ? HEADER : 0);
      // The previous native buffer remains valid through this publication.
      // Preserve a removed record before the next barrier can reuse it.
      if(observer&&entry.record.removedDeferred(f,start,entry.view)) {
        observer.flush();observer.detach();
        observer=entry.observer=new Observation(entry,this.context);
      }
      entry.handle.life = life; entry.retired = Boolean(f[at + D.retired]);
      entry.length = length;
      entry.source = source; entry.buffer = source ? f : entry.row;
      const row = entry.buffer;
      const heapStart = source + (entry.record.block ? row[source + B.heap] | 0 : entry.record.words);
      if(observer){observer.prepare(row,start,heapStart,fresh);observer.reading=true;}
      try {
        if (entry.record.syncDelta) entry.record.syncDelta(this.context,row,start,heapStart,entry.view,entry.dirty,fresh,source,observer);
        else entry.record.sync(this.context, row, start, heapStart, entry.view);
      } finally { if(observer)observer.reading=false; }
      if(observer)observer.shape(row,start,heapStart);
      if(entry.retired&&observer){observer.flush();observer.detach();}
      entry.published = true;
      if (entry.publish) entry.publish(entry.view, entry.handle, entry.retired);
      if (entry.retired && newer) newer.previous = entry.previous;
    }
  }
  // Optional debug check. It compares owned scalars against the last encoded
  // publication, materializing any pending detailed reader fields first.
  check() {
    // A cold admission may grow WASM memory between debug observations.
    if (this.context?.views) this.context.views();
    for (let kind = 0; kind < this.kinds.length; kind++) {
      const slots = this.kinds[kind]; if (!slots) continue;
      for (let slot = 0; slot < slots.length; slot++) {
        const entry = slots[slot];
        if (!entry?.published || entry.retired) continue;
        if(entry.observer)entry.observer.flush();
        const source = entry.source, row = source ? this.context?.f ?? entry.buffer : entry.row;
        const start = source + (entry.record.block ? HEADER : 0);
        const heapStart = source + (entry.record.block ? row[source + B.heap] | 0 : entry.record.words);
        if (entry.record.check) {
          const path = entry.record.check(this.context, row, start, heapStart, entry.view);
          if (path) throw Error(`live-object mirror write: ${kind}/${slot}${path}`);
          continue;
        }
        this.heap.clear(); entry.record.put(this.context, entry.view, entry.check, 0, this.heap);
        for (let i = 0; i < entry.record.words; i++) {
          if (!Object.is(row[start + i], entry.check[i])) throw Error(`live-object mirror write: ${kind}/${slot}/${i}`);
        }
        if (this.heap.words.length !== source + entry.length - heapStart) throw Error('live-object mirror heap write');
        for (let i = 0; i < this.heap.words.length; i++) {
          if (!Object.is(row[heapStart + i], this.heap.words[i])) throw Error(`live-object mirror heap write: ${kind}/${slot}/${i}`);
        }
      }
    }
  }
}
