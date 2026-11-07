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
