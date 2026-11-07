// A battle's receive and display owners share one presentation memory. Packet
// bytes enter once; baseline entries and sampled poses never return to JS.
import { presentationModule } from "../presentation/module.js";
import {
  NetHeader as H,
  NetRecord as R,
  NetSeat as S,
  NetView as V,
  NetEdge as E,
  NetOp as OP,
  NetLimits as L,
} from "./network-layout.js";
const owners = new WeakMap(),
  free = [];
let common;
const operations = {
  input: OP.INPUT,
  ack: OP.ACK,
  outboundReset: OP.OUTBOUND_RESET,
  outboundClear: OP.OUTBOUND_CLEAR,
  outboundAck: OP.OUTBOUND_ACK,
  outboundCommand: OP.OUTBOUND_COMMAND,
  outboundSent: OP.OUTBOUND_SENT,
};
// Encoders may share a registry owner; receivers deliberately create their
// own owner so two clients can never acknowledge another client's baseline.
// Weak keys let a completed battle release its registry without a global map.
export function pageNetworkFor(registry) {
  if (!registry) return common && !common.disposed ? common : (common = createPageNetwork());
  let owner = owners.get(registry);
  if (!owner || owner.disposed) {
    owner = createPageNetwork();
    owners.set(registry, owner);
  }
  return owner;
}
export const createPageNetwork = () => new PageNetwork();
class PageNetwork {
  // The pool retains native reservations across battle changes. It contains
  // closed kernels only; each JS wrapper has its own disposed callback guard.
  constructor() {
    const loaded = presentationModule();
    this.memory = loaded.memory;
    this.kernel = free.pop() || new loaded.module.PageNetwork();
    this.disposed = false;
    this.receiver = false;
    this.buffer = null;
    this.count = 0;
    this.calls = 0;
    this.epoch = 0;
    this.refresh();
  }
  // Resource registration elsewhere in the shared module can grow memory.
  // Addresses remain stable; only that cold event recreates typed views.
  refresh() {
    if (this.buffer !== this.memory.buffer) this.rebind();
  }
  // Keep the span factory in this cold function. Capturing `this` in an
  // arrow inside refresh would allocate its lexical context before the hot
  // early return, even while the memory buffer remained unchanged.
  rebind() {
    const b = this.memory.buffer;
    this.buffer = b;
    const k = this.kernel;
    this.input = new Float64Array(b, k.words_ptr(), L.EXCHANGE);
    this.bytes = new Uint8Array(b, k.bytes_ptr(), L.PAYLOAD);
    this.numbers = new Float64Array(b, k.numbers_ptr(), 8);
    this.encoded = new Uint8Array(b, k.encoded_ptr(), L.PAYLOAD);
    this.spans = Array.from({ length: L.PAYLOAD + 1 }, (_, n) => this.encoded.subarray(0, n));
    if (this.receiver) {
      this.header = new Float64Array(b, k.header_ptr(), H.STRIDE);
      this.records = new Float64Array(b, k.records_ptr(), L.RECORDS * R.STRIDE);
      this.seats = new Float64Array(b, k.seats_ptr(), L.SLOTS * 2 * S.STRIDE);
      this.payloads = new Uint8Array(b, k.payloads_ptr(), L.EXCHANGE);
    }
  }
  // Diagnostic/history-only owners reserve no packet baselines. Live receive
  // setup below reserves both stores before the transport delivers its bytes.
  timelineSetup() {
    this.kernel.timeline_setup();
    this.buffer = null;
    this.refresh();
  }
  // Sparse prerequisites are packet-time gameplay lookups. They publish one
  // changed-record row and are never used to sample a displayed entity.
  lookup(kind, id) {
    if (this.disposed) return false;
    this.refresh();
    return this.kernel.lookup(kind, id);
  }
  // Bounds describe the quantized wire space, rather than a drawn camera.
  // They remain constant for this receiver and never cross again per packet.
  receiveSetup(bounds) {
    if (this.disposed) return;
    this.kernel.receive_setup();
    this.receiver = true;
    this.kernel.set_bounds(bounds.xMin, bounds.xExtent, bounds.zMin, bounds.zExtent, bounds.yMin, bounds.yExtent);
    this.buffer = null;
    this.refresh();
  }
  // Copy the datagram once. Native assembly keeps its own immutable versions;
  // neither the packet input nor a JS baseline object becomes retained state.
  // Preserve ordinary Error objects at the existing receive failure boundary.
  receive(bytes) {
    if (this.disposed) return false;
    if (!(bytes instanceof Uint8Array) || bytes.length < 20 || bytes.length > L.PAYLOAD)
      throw Error("invalid snapshot datagram");
    this.refresh();
    this.bytes.set(bytes);
    try {
      this.kernel.receive(bytes.length);
    } catch (error) {
      throw error instanceof Error ? error : Error(String(error));
    }
    this.calls++;
    this.epoch = this.header[H.EPOCH];
    return this.header[H.VALID] !== 0;
  }
  // Epoch and identity commands change history at lifecycle boundaries. They
  // do not read a sampled row or allocate another native ring.
  reset(epoch) {
    if (!this.disposed) {
      this.kernel.reset(epoch);
      this.epoch = epoch;
    }
  }
  remove(id) {
    if (!this.disposed) this.kernel.remove(id);
  }
  // One coarse sample call serves the complete frame. Prediction exclusions
  // stay beside the native output, where both display phases can read them.
  // Resource registration can grow shared memory; refresh only rebinds then.
  present(tick, player = -1, vehicle = -1) {
    if (this.disposed) return;
    this.kernel.present(tick, player ?? -1, vehicle ?? -1);
    this.refresh();
    this.calls++;
  }
  // Input context/edge records have generated layouts. Variable held/look
  // streams are counted, and Rust validates their entire reserved span.
  begin() {
    if (this.disposed) throw Error("page networking is disposed");
    this.refresh();
    this.count = 0;
  }
  put(value) {
    if (this.count === L.EXCHANGE) throw Error("page networking input exceeds bounds");
    this.input[this.count++] = value;
  }
  // The host resolves context names, then writes its generated transport row.
  // Validation, optional-field order and binary scalar encoding remain native.
  view(v) {
    const x = this.input,
      at = this.count;
    if (at + V.STRIDE > x.length) throw Error("page networking input exceeds bounds");
    x[at + V.FLAGS] = v.flags;
    x[at + V.PROFILE] = v.profile;
    x[at + V.PROFILE_FOUND] = v.profileFound ? 1 : 0;
    x[at + V.MOUSE_SCALE] = v.mouseScale;
    x[at + V.ASPECT] = v.aspect;
    x[at + V.VIEW_TICK] = v.viewTick;
    x[at + V.SENSITIVITY] = v.sensitivity;
    x[at + V.CURVE] = v.curve;
    for (let i = 0; i < 4; i++) x[at + V.AXES + i] = v.axes[i];
    x[at + V.BUTTONS] = v.buttons;
    this.count += V.STRIDE;
  }
  // An edge's life and registry facts accompany its numeric row. Rust rejects
  // inconsistent facts before returning any accepted packet bytes.
  edge(e) {
    const x = this.input,
      at = this.count;
    if (at + E.STRIDE > x.length) throw Error("page networking input exceeds bounds");
    x[at + E.LIFE_MATCHES] = e.lifeMatches ? 1 : 0;
    x[at + E.ID] = e.id;
    x[at + E.TICK] = e.tick;
    x[at + E.KIND] = e.kind;
    x[at + E.ACTION] = e.action;
    x[at + E.BUTTON] = e.button;
    x[at + E.PRESSED] = e.pressed;
    x[at + E.VIEW_TICK] = e.viewTick;
    x[at + E.AIM_YAW] = e.aimYaw;
    x[at + E.AIM_PITCH] = e.aimPitch;
    x[at + E.TEAM] = e.team;
    x[at + E.TEAM_RANGE_VALID] = e.teamRangeValid ? 1 : 0;
    x[at + E.CLASS_ID] = e.classId;
    x[at + E.POST_ID] = e.postId;
    this.count += E.STRIDE;
  }
  // Outbound accounting returns a fixed few fields. Encoding returns a byte
  // count and one packed span, never a scalar-by-scalar result stream.
  run(name, handle = 0) {
    const op = operations[name];
    try {
      if (name === "input" || name === "ack") this.length = this.kernel.encode(op, this.count);
      else this.kernel.outbound_run(handle, op, this.count);
    } catch (error) {
      throw error instanceof Error ? error : Error(String(error));
    }
    this.calls++;
  }
  // Synchronous transport sends can borrow a prebuilt span. Retained callers
  // request an owned packet, so a later encode cannot overwrite their bytes.
  result(borrowed = false) {
    return borrowed ? this.spans[this.length] : this.encoded.slice(0, this.length);
  }
  // These are cold connection handles, separate from snapshot ownership.
  // Disposing a sender releases its slot without touching another sender.
  createOutbound() {
    return this.kernel.outbound_new();
  }
  dropOutbound(id) {
    if (!this.disposed) this.kernel.outbound_drop(id);
  }
  // Statistics are explicit cold inspection. Frames increment a numeric
  // counter but never construct this object merely to update presentation.
  stats() {
    return {
      calls: this.calls,
      memoryBytes: this.memory.buffer.byteLength,
      disposed: this.disposed,
    };
  }
  // A closed callback cannot mutate a recycled owner. Native display links
  // also carry the owner's lease, invalidated by close before reuse.
  dispose() {
    if (this.disposed) return;
    this.disposed = true;
    this.kernel.close();
    free.push(this.kernel);
    this.input =
      this.bytes =
      this.records =
      this.seats =
      this.payloads =
      this.numbers =
      this.encoded =
      this.header =
      this.spans =
      this.buffer =
        null;
  }
}
