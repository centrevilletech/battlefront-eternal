// SnapshotClient receives one datagram in the display module. Rust retains the
// multipart assemblies, immutable ACK baselines and display ring. Only changed
// gameplay records and opaque owned payloads cross to existing JS consumers.
import { DEFAULT_SNAPSHOT_BOUNDS } from "./snapshot-codec.js";
import { createPageNetwork } from "./page-network.js";
import { networkRecord } from "./network-records.js";
import { NetHeader as H, NetRecord as R, NetSeat as S, NetKind as K } from "./network-layout.js";
export class SnapshotClient {
  constructor({ bounds = DEFAULT_SNAPSHOT_BOUNDS, onMissingBaseline = () => {}, kernel = createPageNetwork() } = {}) {
    this.bounds = bounds;
    this.onMissingBaseline = onMissingBaseline;
    this.network = kernel;
    kernel.receiveSetup(bounds);
    this.epoch = NaN;
    this.ackTick = NaN;
    this.missingBaselines = NaN;
    this.reset(0);
  }
  // Reset clears native baselines, pending multipart ticks and display state.
  // The three JS fields below are transport metadata still read by callers.
  reset(epoch) {
    this.network.reset(epoch);
    this.epoch = epoch;
    this.ackTick = 0;
    this.missingBaselines = 0;
  }
  // ACKs are packet publications, not frame results. The transport retains
  // their epoch/tick independently from replaceable incoming datagrams.
  get snapshotAck() {
    return { epoch: this.epoch, tick: this.ackTick };
  }
  // Header and result spans remain valid until the next native operation.
  // Construct only the changed records the gameplay adapter still consumes;
  // no baseline entry or presentation sample is copied into a JS object.
  receive(bytes) {
    const k = this.network;
    if (k.disposed) return null;
    const valid = k.receive(bytes),
      h = k.header;
    if (this.epoch !== h[H.EPOCH]) this.missingBaselines = 0;
    this.epoch = h[H.EPOCH];
    this.ackTick = h[H.ACK_TICK];
    const missing = h[H.MISSING_BASELINES];
    // Loss recovery receives the original header fields, even when the
    // native reader refused a missing baseline and emitted no gameplay data.
    if (missing > this.missingBaselines) {
      this.missingBaselines = missing;
      this.onMissingBaseline({
        magic: h[H.MAGIC],
        epoch: this.epoch,
        tick: h[H.TICK],
        interval: h[H.INTERVAL],
        baselineTick: h[H.BASELINE_TICK],
        partIndex: h[H.PART_INDEX],
        partCount: h[H.PART_COUNT],
        ackInput: h[H.ACK_INPUT],
        headerBytes: h[H.HEADER_BYTES],
      });
    }
    if (!valid) return null;
    const records = [],
      seatChanges = [];
    for (let i = 0; i < h[H.RECORDS]; i++) records.push(networkRecord(k.records, i * R.STRIDE, k.payloads));
    // Ownership changes outlive this packet. Generations identify both ends
    // of a lease so a delayed seat cannot attach to a recycled body or hull.
    for (let i = 0; i < h[H.SEATS]; i++) {
      const at = i * S.STRIDE,
        x = k.seats;
      seatChanges.push({
        id: x[at + S.ID],
        generation: x[at + S.GENERATION],
        seat: x[at + S.PRESENT]
          ? {
              vehicleId: x[at + S.VEHICLE_ID],
              vehicleGeneration: x[at + S.VEHICLE_GENERATION],
              seatIndex: x[at + S.SEAT_INDEX],
            }
          : null,
      });
    }
    return {
      tick: h[H.TICK],
      epoch: this.epoch,
      epochChanged: !!h[H.EPOCH_CHANGED],
      interval: h[H.INTERVAL],
      ackInput: h[H.ACK_INPUT],
      ack: { epoch: this.epoch, tick: this.ackTick },
      records,
      seatChanges,
      snapshotAck: this.snapshotAck,
      complete: !!h[H.COMPLETE],
    };
  }
  // Reliable records already have semantic payloads and no snapshot baseline.
  gameplayRecords(records, tick) {
    return {
      tick,
      epoch: this.epoch,
      epochChanged: false,
      interval: 2,
      ackInput: 0,
      ack: { epoch: this.epoch, tick: this.ackTick },
      records,
      seatChanges: [],
      snapshotAck: this.epoch >= 1 ? this.snapshotAck : null,
      complete: false,
    };
  }
  // Deferred prerequisites occasionally need latest gameplay facts. This
  // lookup is separate from the native historical rows used to draw frames.
  get(kind, id) {
    const code = K[kind.toUpperCase()];
    const k = this.network;
    if (code === undefined || !k.lookup(code, id)) return undefined;
    return networkRecord(k.records, 0, k.payloads);
  }
  dispose() {
    this.network.dispose();
  }
}
