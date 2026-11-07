import { VERSION, OrdnanceFrame as F, OrdnanceRecord as R, OrdnanceStyle as S, OrdnanceHeader as H, OrdnanceCall as C, NetCommand as N, OrdnanceLimits as L } from "./ordnance-layout.js";

// A caller-owned, bounded record of the ordnance phase: every class the page
// registered, every network command block it applied outside a frame, and
// each frame's inputs and outputs, as numbers. The native `ordnance-replay`
// reads it back and requires every output bit to match. Nothing here runs
// unless a capture was begun, and nothing allocates in a frame.
// Entries of the stream, after a one-number version: a class registered, a
// block of network commands applied outside a frame, and a frame.
const TAG = { style: 1, flush: 2, frame: 3 };
// The widths of the kernel's output attributes, to read each handle's region.
const QUAD_SIZES = [3, 4, 4, 3, 3, 4];
const BLASTER_SIZES = [3, 3, 4, 4];

export class OrdnanceCapture {
  constructor(capacity = 8 * 1024 * 1024) {
    this.data = new Float64Array(capacity + 1);
    this.data[0] = VERSION;
    this.used = 1;
    this.frames = 0;
    this.full = false;
  }
  // Past the capacity the capture stops and says so; it never grows.
  room(n) {
    if (this.used + n > this.data.length) this.full = true;
    return !this.full;
  }
  style(index, row) {
    if (!this.room(2 + S.STRIDE)) return;
    this.data[this.used++] = TAG.style;
    this.data[this.used++] = index;
    for (let i = 0; i < S.STRIDE; i++) this.data[this.used++] = row[i];
  }
  flush(count, net) {
    const n = count * N.STRIDE;
    if (!this.room(2 + n)) return;
    this.data[this.used++] = TAG.flush;
    this.data[this.used++] = count;
    for (let i = 0; i < n; i++) this.data[this.used++] = net[i];
  }
  frame(input, netCount, net, header, calls, counts, quads, blasters) {
    const records = input[F.RECORDS], callCount = header[H.CALLS], blasterCount = header[H.BLASTERS];
    let quadTotal = 0;
    for (let h = 0; h < L.HANDLES; h++) for (let k = 0; k < 6; k++) quadTotal += counts[h] * QUAD_SIZES[k];
    let blasterTotal = 0;
    for (let k = 0; k < 4; k++) blasterTotal += blasterCount * BLASTER_SIZES[k];
    const inputLength = F.STRIDE + records * R.STRIDE;
    if (!this.room(5 + inputLength + netCount * N.STRIDE + H.STRIDE + callCount * C.STRIDE + L.HANDLES + quadTotal + blasterTotal)) return;
    const d = this.data;
    d[this.used++] = TAG.frame;
    d[this.used++] = records;
    d[this.used++] = netCount;
    d[this.used++] = callCount;
    d[this.used++] = blasterCount;
    for (let i = 0; i < inputLength; i++) d[this.used++] = input[i];
    for (let i = 0; i < netCount * N.STRIDE; i++) d[this.used++] = net[i];
    for (let i = 0; i < H.STRIDE; i++) d[this.used++] = header[i];
    for (let i = 0; i < callCount * C.STRIDE; i++) d[this.used++] = calls[i];
    for (let h = 0; h < L.HANDLES; h++) d[this.used++] = counts[h];
    for (let h = 0; h < L.HANDLES; h++) {
      const n = counts[h];
      for (let k = 0; k < 6; k++) {
        const size = QUAD_SIZES[k], from = h * L.QUADS_PER_HANDLE * size;
        for (let i = 0; i < n * size; i++) d[this.used++] = quads[k][from + i];
      }
    }
    for (let k = 0; k < 4; k++) for (let i = 0; i < blasterCount * BLASTER_SIZES[k]; i++) d[this.used++] = blasters[k][i];
    this.frames++;
  }
  export() {
    return this.data.slice(0, this.used);
  }
}
