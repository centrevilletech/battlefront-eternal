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
  readInto(pointer, length, view) {
    this.refresh();
    view.words = this.view.words;
    view.base = pointer / Float64Array.BYTES_PER_ELEMENT;
    view.length = length;
    view.generation = this.view.generation;
    return view;
  }
  close() {
    this.memory = null; this.bytes = null;
    this.view.words = null; this.view.base = 0; this.view.length = 0; this.view.generation++;
  }
  readBlock(pointer) {
    this.refresh();
    const view = this.view;
    view.base = pointer / Float64Array.BYTES_PER_ELEMENT;
    view.length = view.words.length - view.base;
    return view;
  }
}

// Native results use independently owned arenas, never numeric addresses. A
// successful call replaces its borrowed publication without allocation. Separate
// operation, kind and event banks keep observer reentry from overwriting events.
export class NativeBlocks {
  constructor(words = 8192) {
    this.banks = [new BlockView(),new BlockView(),new BlockView()];
    this.view = this.banks[0];
    this.closed = false;
    this.reserve(words);
  }
  reserve(words) {
    if (this.closed) throw Error('Native block bank closed');
    if (!Number.isSafeInteger(words) || words < 0 || words > 4 * 1024 * 1024) throw RangeError('Invalid native arena capacity');
    let grown = false;
    for (const view of this.banks) {
      if (words <= (view.words?.length ?? -1)) continue;
      view.words = new Float64Array(words);
      view.base = view.length = 0; view.generation++; grown = true;
    }
    return grown;
  }
  publish(length, view = this.view) {
    if (this.closed || !Number.isSafeInteger(length) || length < 0 || length > view.words.length) throw Error('Invalid native result extent');
    view.base = 0; view.length = length; view.generation++;
    this.view = view; return view;
  }
  readBlock(block = this.view) {
    if (this.closed) throw Error('Native block bank closed');
    if (!this.banks.includes(block)) throw Error('Result belongs to another native bank');
    return block;
  }
  close() {
    if (this.closed) return false;
    this.closed = true;
    for(const view of this.banks) {
      view.words = null; view.base = view.length = 0; view.generation++;
    }
    return true;
  }
}
