// Linear-memory addresses stop at this backend. The host bridge consumes
// generated BlockViews; a world and all of its cold attachments share it.
import { loadSim } from '../module.js';
import { WasmBlocks } from '../block-view.js';
import { BLOCK_HEADER_WORDS } from '../gen/boundary.js';
import { KIND_TABLE, COMMAND_TABLE } from '../gen/index.js';
import { WorldOperation } from './world.js';
import { PublicationOperation } from './publication.js';
import { operations } from './operations.js';

export class WasmBackend {
  constructor(instance, counters = null) {
    this.instance = instance;
    this.setExports(instance.exports);
    this.blocks = new WasmBlocks(this.exports.memory);
    this.bridge = null;
    this.closed = false; this.closing = false; this.active = 0;
    this.attachments = operations; this.owners = new Set(); this.counters = counters;
    if (counters) counters[0]++;
  }
  setExports(exports) {
    this.exports=exports;
    this.commands=COMMAND_TABLE.map(command=>command && (command.bulk
      ? exports[command.bulk] : exports.sim_command.bind(null,command.code)));
  }
  connect(bridge, options) {
    const x = this.exports;
    if (x.sim_abi() !== 1) throw Error('the simulation module has another store ABI');
    if (x.sim_kinds() !== KIND_TABLE.length - 1) throw Error(`the simulation's module has ${x.sim_kinds()} kinds, its JavaScript ${KIND_TABLE.length - 1}: rebuild it (npm run rust:build)`);
    for (const k of KIND_TABLE) if (k && x.sim_kind_hash(k.code) >>> 0 !== k.hash) throw Error(`the simulation's module lays ${k.name} out otherwise than the store's JavaScript: rebuild it (npm run rust:build)`);
    if (this.instance.holder?.bridge && !options.replace) throw Error('this instance of the module has a bridge already: bridgeFor(instance) gives it');
    this.bridge = bridge;
    if (this.instance.holder) this.instance.holder.bridge = bridge;
    bridge.exchange = x.sim_import_args();
    bridge.exchangeWords = x.sim_import_capacity();
  }
  getBridge(options = {}) { return this.bridge.identify(options); }
  arguments(rows, count, heap) {
    const block = this.blocks.readBlock(this.exports.sim_call_room(count, heap.length));
    const at = block.base + BLOCK_HEADER_WORDS, words = block.words;
    for (let i = 0; i < count; i++) words[at + i] = rows[i];
    for (let i = 0; i < heap.length; i++) words[at + count + i] = heap[i];
  }
  command(code) { return this.blocks.readBlock(this.commands[code]()); }
  reserve(bytes) { const pages=this.exports.sim_reserve(bytes); this.blocks.refresh(); return pages; }
  scratch(bytes) { const at=this.exports.sim_scratch(bytes.length); this.blocks.refresh(); this.blocks.bytes.set(bytes,at); }
  exportKind(kind,mode) { return this.exports.sim_export(kind,mode); }
  importRoom(kind,n,stride,heap) { return this.exports.sim_import_room(kind,n,stride,heap); }
  importKind(kind,mode) { return this.exports.sim_import(kind,mode); }
  retire(kind,slot,generation) { return this.exports.sim_retire(kind,slot,generation); }
  events() {return this.exports.sim_events();}
  clearEvents() {this.exports.sim_events_clear();}
  intern(text) {
    const units=new Uint8Array(text.length*2);
    for(let i=0;i<text.length;i++){const c=text.charCodeAt(i);units[2*i]=c&255;units[2*i+1]=c>>8;}
    this.scratch(units);return this.exports.sim_str_intern(text.length);
  }
  string(id) {
    const length=this.exports.sim_str_len(id),at=this.exports.sim_str_ptr(id);this.blocks.refresh();
    const units=new Uint16Array(this.blocks.view.words.buffer,at,length);let text='';
    for(let i=0;i<length;i++)text+=String.fromCharCode(units[i]);return text;
  }
  prepare(category, ...args) {
    const result = this.attachments[category](this, ...args);
    return result?.then ? result.then(owner => this.own(owner)) : this.own(result);
  }
  // Session admission and release are cold. No counter allocates a row, keeps
  // an owner alive after release, or runs in a steady-state entity loop.
  own(owner, method = 'dispose') {
    if (!owner?.[method] || this.owners.has(owner)) return owner;
    const release = owner[method], backend = this;
    this.owners.add(owner); if (this.counters) this.counters[2]++;
    owner[method] = function(...args) {
      if (!backend.owners.delete(owner)) return false;
      try { return release.apply(owner, args); }
      finally { if (backend.counters) backend.counters[3]++; }
    };
    return owner;
  }
  enter() { if(this.closing)throw Error('Simulation closed');this.active++; }
  leave() { if (--this.active === 0 && this.closing) this.release(); }
  world(phase, clock) { return this.own(new WorldOperation(this.bridge, phase, clock), 'close'); }
  publisher(resources) { return this.own(new PublicationOperation(this.bridge, resources), 'close'); }
  // A callback may request teardown during a synchronous tick. Keep its
  // input/output loans alive until Game finishes the enclosing observer span.
  close() {
    if (this.closing) return false;
    this.closing = true;
    if (!this.active) this.release();
    return true;
  }
  release() {
    let failure;
    // Reverse admission order unlinks publisher/sinks before their resources.
    for (const owner of Array.from(this.owners).reverse()) {
      try { if (owner.dispose) owner.dispose(); else owner.close(); }
      catch (error) { failure ??= error; }
    }
    const bridge = this.bridge;
    if (this.instance.holder?.bridge === bridge) this.instance.holder.bridge = null;
    this.bridge = null; this.exports = null; this.commands = null; this.instance = null; this.closed = true;
    if (bridge) { bridge.handlers = {}; bridge.served.length = 0; bridge.beforeImport = null; bridge.memory = null; bridge.closed = true; }
    this.blocks.close(); if (this.counters) this.counters[1]++;
    if (failure) throw failure;
  }
}
export function wasmBackendFor(instance) {
  if (instance.holder?.backend) return instance.holder.backend;
  const backend = new WasmBackend(instance);
  if (instance.holder) instance.holder.backend = backend;
  return backend;
}
export async function loadWasmBackend({ counters = null } = {}) {
  const instance = await loadSim(), backend = new WasmBackend(instance, counters);
  instance.holder.backend = backend;
  // Resolve the cycle only at construction, before any kernel attaches.
  const { bridgeFor } = await import('../runtime.js');
  try {
    bridgeFor(instance);
    backend.attachments = { ...operations, ...(await import('./attachments.js')).attachments };
    return backend;
  } catch (error) { backend.close(); throw error; }
}
