import { BlockHeaderLayout as BlockHeader } from './gen/boundary.js';

// Generated records address their heap relative to a block, never to a native
// pointer. A view is borrowed until the next operation, including reentry.
export class BlockView {
  constructor() {
    this.words = null;
    this.base = 0;
    this.length = 0;
    this.generation = 0;
  }
}

// Keep one view per backend. Resolving a block costs no allocation and happens
// once before a generated loop, so every row uses ordinary typed-array reads.
export class WasmBlocks {
  constructor(memory) {
    this.memory = memory;
    this.view = new BlockView();
    this.bytes = null;
    this.refresh();
  }
  refresh() {
    const view = this.view;
    if (view.words?.buffer.byteLength) return;
    view.words = new Float64Array(this.memory.buffer);
    this.bytes = new Uint8Array(this.memory.buffer);
    view.generation++;
  }
  readBlock(pointer) {
    this.refresh();
    const view = this.view;
    view.base = pointer / Float64Array.BYTES_PER_ELEMENT;
    view.length = view.words.length - view.base;
    return view;
  }
}

const validCount = n => Number.isSafeInteger(n) && n >= 0;

// Native operations return an arena, not an emulated linear address space.
// Validation happens at the operation boundary; repeated decoding reuses it.
export class NativeBlocks {
  constructor() { this.view = new BlockView(); }
  accept(words) {
    if (!(words instanceof Float64Array)) throw Error('Expected a Float64Array block');
    if (words.length < BlockHeader.words) throw Error('Truncated block header');
    const rows = words[BlockHeader.rows], stride = words[BlockHeader.stride];
    const heap = words[BlockHeader.heap], size = words[BlockHeader.heapWords];
    const retired = words[BlockHeader.retired];
    if (!validCount(rows) || !validCount(stride) || !validCount(heap) || !validCount(size) || !validCount(retired)) throw Error('Invalid block extent');
    if (retired > rows || heap !== BlockHeader.words + rows * stride || heap + size !== words.length) throw Error('Invalid block layout');
    const view = this.view;
    view.words = words;
    view.base = 0;
    view.length = words.length;
    view.generation++;
    return view;
  }
  readBlock() { return this.view; }
}
