import { Frame, Command, Header, Effect, Draw, Event, VERSION } from './layout.js';
// Captures are caller-owned and bounded. Start explicitly, then export after
// pausing; nothing is downloaded, stringified or allocated in a frame.
// The file contains numeric records and stable handles, never textures/models.
export class PresentationCapture {
  constructor(capacity = 8 * 1024 * 1024) {
    this.data = new Float64Array(capacity); this.used = 0; this.frames = 0; this.full = false;
  }
  record(input, header, effects, draws, events) {
    if (this.full) return;
    const a = Frame.STRIDE + input[Frame.COMMANDS] * Command.STRIDE;
    const b = header[Header.EFFECTS] * Effect.STRIDE;
    const c = header[Header.DRAWS] * Draw.STRIDE;
    const d = header[Header.EVENTS] * Event.STRIDE;
    const size = 5 + a + Header.STRIDE + b + c + d;
    if (this.used + size > this.data.length) { this.full = true; return; }
    let at = this.used; this.data[at++] = VERSION;
    this.data[at++] = a; this.data[at++] = b; this.data[at++] = c; this.data[at++] = d;
    for (let i = 0; i < a; i++) this.data[at++] = input[i];
    for (let i = 0; i < Header.STRIDE; i++) this.data[at++] = header[i];
    for (let i = 0; i < b; i++) this.data[at++] = effects[i];
    for (let i = 0; i < c; i++) this.data[at++] = draws[i];
    for (let i = 0; i < d; i++) this.data[at++] = events[i];
    this.used = at; this.frames++;
  }
  export() { return this.data.slice(0, this.used); }
}
