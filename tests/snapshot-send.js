// Test double for the snapshot host's wire side: turns the semantic
// records a test wants to publish at a tick into the snapshot datagrams
// (entities parts, then own) the real host would send, merging them
// into the full captured state and encoding against the baseline the
// adapter under test last acknowledged.
import { SnapshotSlotTable, snapshotFromRecords, ownSummaryFields } from '../src/runtime/snapshot-records.js';
import { encodeEntities } from '../src/runtime/snapshot-codec.js';
import { SnapshotRing } from '../src/runtime/snapshot-ring.js';
import { encodeOwnDatagram } from '../src/runtime/snapshot-own.js';

export function createSnapshotSender(adapter) {
  const latest = new Map(), world = new Map(), ownSummaries = new Map();
  const slots = new SnapshotSlotTable(), ring = new SnapshotRing(128), ownByTick = new Map();
  let previous = null, lastDatagrams = [];
  const key = r => `${r.kind}:${r.id}:${r.schema ?? ''}`;
  function datagrams(tick, records, { ackInput = 0 } = {}) {
    let correction = null;
    const tombstones = [];
    for (const r of records) {
      if (r.kind === 'correction') { correction = r; continue; }
      if (r.kind === 'summary') {
        if (r.schema === 3 || r.id === 79999) ownSummaries.set(key(r), r); else world.set(key(r), r);
        continue;
      }
      if (r.kind === 'tombstone') { latest.delete(`soldier:${r.id}:`); latest.delete(`vehicle:${r.id}:`); latest.delete(`droideka:${r.id}:`); tombstones.push(r); continue; }
      latest.set(key(r), r);
    }
    const snapshot = snapshotFromRecords({ tick, epoch: 1, records: [...latest.values(), ...tombstones], worldRecords: [...world.values()], slots, previous });
    ring.set(tick, 1, snapshot); previous = snapshot;
    const baselineTick = adapter.receiver.snapshotAck.tick;
    const baseline = baselineTick ? ring.get(baselineTick, 1) : null;
    const baselineOwn = ownByTick.get(baselineTick);
    const summaries = ownSummaryFields([...ownSummaries.values()]);
    ownByTick.set(tick, { payload: correction?.payload ?? baselineOwn?.payload ?? null, summaries });
    const own = encodeOwnDatagram({ epoch: 1, tick, interval: 2, baselineTick: baseline ? baselineTick : 0, ackInput,
      correction, baselinePayload: baselineOwn?.payload ?? null, summaries, baselineSummaries: baselineOwn?.summaries ?? null });
    const parts = encodeEntities(snapshot, baseline ?? null, { interval: 2 });
    lastDatagrams = [...parts, own];
    return lastDatagrams;
  }
  function deliver(bytes) { for (const b of bytes) adapter.receive(b); }
  function send(tick, records, options) {
    const made = datagrams(tick, records, options);
    deliver(made);
    return made;
  }
  function resend() { deliver(lastDatagrams); }
  return { datagrams, send, resend, deliver, latest, world, ownSummaries };
}
