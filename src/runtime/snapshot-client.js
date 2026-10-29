// Client receive for snapshot-delta datagrams. Every `entities` part
// applies at once against the acknowledged baseline it names; only a
// snapshot with all its parts and its `own` datagram (both, when the first
// announces a second) enters the ring and is acknowledged. Late snapshots still fill the ring (and the
// interpolation timeline), but gameplay state is only emitted for ticks
// newer than the newest already emitted. Decoded state is handed upward
// as the semantic records the game adapter already consumes.
import {
  SNAPSHOT_ENTITIES_MAGIC, SNAPSHOT_OWN_MAGIC, DEFAULT_SNAPSHOT_BOUNDS,
  readSnapshotHeader, decodeEntities, applyEntities, emptySnapshot, worldFieldKey,
} from './snapshot-codec.js';
import { decodeOwnDatagram } from './snapshot-own.js';
import { unpackCosmeticEvents } from './snapshot-cosmetics.js';
import { recordFromEntry, mountRecordFromEntry, worldRecordFromField } from './snapshot-records.js';
import { SnapshotRing } from './snapshot-ring.js';

export class SnapshotClient {
  constructor({ bounds = DEFAULT_SNAPSHOT_BOUNDS, onMissingBaseline = () => {} } = {}) {
    this.bounds = bounds; this.onMissingBaseline = onMissingBaseline;
    this.reset(0);
  }
  reset(epoch) {
    this.epoch = epoch;
    this.ring = new SnapshotRing(128);
    this.ownRing = new SnapshotRing(128);
    this.assemblies = new Map();
    this.ackTick = 0;
    this.newestEntitiesTick = -1;
    this.newestOwnTick = -1;
    this.current = emptySnapshot(0, epoch); // newest snapshot state seen, for lookups
    this.seats = new Map(); // entityId -> {vehicleId, vehicleGeneration, seatIndex, generation}
    this.missingBaselines = 0;
  }
  get snapshotAck() { return { epoch: this.epoch, tick: this.ackTick }; }
  // Latest record-shaped state by kind and entity id (adapter lookups).
  get(kind, id) {
    for (const entry of this.current.slots.values()) {
      if (kind === 'mount' && entry.cls === 'vehicle' && entry.entityId === id) return mountRecordFromEntry(entry, this.current.tick);
      if (entry.cls === kind && entry.entityId === id) return recordFromEntry(entry, this.current.tick, this.bounds);
      if (kind === 'vehicle' && entry.cls === 'vehicle' && entry.entityId === id) return recordFromEntry(entry, this.current.tick, this.bounds);
    }
    if (kind === 'remote') for (const entry of this.current.slots.values()) if (entry.cls === 'remote' && entry.entityId === id) return recordFromEntry(entry, this.current.tick, this.bounds);
    return undefined;
  }
  assemblyFor(tick, epoch, baselineTick, baseline) {
    let assembly = this.assemblies.get(tick);
    if (!assembly || assembly.baselineTick !== baselineTick || assembly.epoch !== epoch) {
      // Entries and world fields are immutable once decoded, so the working
      // snapshot shares them with its baseline instead of deep-cloning.
      const snapshot = { tick: baseline.tick, epoch: baseline.epoch, slots: new Map(baseline.slots), world: new Map(baseline.world) };
      assembly = { tick, epoch, baselineTick, snapshot, parts: new Set(), partCount: 0, own: null, emitted: new Set() };
      this.assemblies.set(tick, assembly);
      while (this.assemblies.size > 32) this.assemblies.delete(this.assemblies.keys().next().value);
    }
    return assembly;
  }
  // Only the entries a part actually changed become timeline samples at
  // this tick; labelling the baseline's stale poses with the new tick
  // would make interpolation hold, then jump when their part arrives.
  samplePoses(entries, tick) {
    const poses = new Map();
    for (const entry of entries) {
      if (!['soldier', 'droideka', 'vehicle'].includes(entry.cls)) continue;
      const record = recordFromEntry(entry, tick, this.bounds);
      record.disc = entry.disc;
      poses.set(entry.entityId, record);
    }
    return poses;
  }
  seatChangesFor(assembly, emittedSlots) {
    const changes = [];
    for (const slot of emittedSlots) {
      const entry = assembly.snapshot.slots.get(slot);
      if (!entry || (entry.cls !== 'soldier' && entry.cls !== 'droideka')) continue;
      const vehicle = entry.seatSlot >= 0 ? assembly.snapshot.slots.get(entry.seatSlot) : null;
      const seat = vehicle ? { vehicleId: vehicle.entityId, vehicleGeneration: vehicle.generation, seatIndex: entry.seatIndex } : null;
      const previous = this.seats.get(entry.entityId);
      const same = JSON.stringify(previous ?? null) === JSON.stringify(seat) && previous?.generation === entry.generation;
      if (same) continue;
      if (seat) this.seats.set(entry.entityId, { ...seat, generation: entry.generation }); else this.seats.delete(entry.entityId);
      changes.push({ id: entry.entityId, generation: entry.generation, seat });
    }
    return changes;
  }
  complete(assembly) {
    if (!assembly.own || assembly.parts.size < assembly.partCount || assembly.partCount === 0) return false;
    // Cosmetics that overflowed into a second own datagram count too: the
    // host takes an acknowledgement as delivery of everything it carried.
    if (assembly.own.parts > 1 && !assembly.secondOwn) return false;
    this.ring.set(assembly.tick, assembly.epoch, assembly.snapshot);
    const summaries = new Map(this.ownRing.get(assembly.baselineTick, assembly.epoch)?.summaries ?? []);
    for (const field of assembly.own.summaries) {
      if (field.removed) summaries.delete(worldFieldKey(field));
      else summaries.set(worldFieldKey(field), { id: field.id, schema: field.schema, generation: field.generation, payload: field.payload });
    }
    this.ownRing.set(assembly.tick, assembly.epoch, { payload: assembly.own.correction?.payload ?? this.ownRing.get(assembly.baselineTick, assembly.epoch)?.payload ?? null, summaries });
    if (assembly.tick > this.ackTick) this.ackTick = assembly.tick;
    this.assemblies.delete(assembly.tick);
    return true;
  }
  // Returns a packet-like object for the game adapter, or null when the
  // datagram carried nothing to apply (a duplicate part, a baseline we do
  // not hold, a second own datagram with only cosmetics already seen...).
  receive(bytes) {
    const header = readSnapshotHeader(bytes);
    if (header.epoch < this.epoch) return null;
    let epochChanged = false;
    if (header.epoch > this.epoch) { this.reset(header.epoch); epochChanged = true; }
    if (header.magic === SNAPSHOT_ENTITIES_MAGIC) return this.receiveEntities(bytes, header, epochChanged);
    if (header.magic === SNAPSHOT_OWN_MAGIC) return this.receiveOwn(bytes, header, epochChanged);
    throw Error('unknown snapshot datagram');
  }
  baselineFor(header) {
    if (header.baselineTick === 0) return emptySnapshot(0, header.epoch);
    return this.ring.get(header.baselineTick, header.epoch);
  }
  receiveEntities(bytes, header, epochChanged) {
    if (this.ring.has(header.tick, header.epoch)) return null; // already complete: a duplicate
    const baseline = this.baselineFor(header);
    if (!baseline) { this.missingBaselines++; this.onMissingBaseline(header); return null; }
    const assembly = this.assemblyFor(header.tick, header.epoch, header.baselineTick, baseline);
    if (assembly.partCount && assembly.partCount !== header.partCount) throw Error('snapshot part count changed');
    if (assembly.parts.has(header.partIndex)) return null;
    const decoded = decodeEntities(bytes, baseline);
    const emittedSlots = new Set([...decoded.set.keys()]);
    const removedEntries = decoded.removed.map(slot => baseline.slots.get(slot)).filter(Boolean);
    applyEntities(assembly.snapshot, decoded);
    assembly.parts.add(header.partIndex); assembly.partCount = header.partCount;
    if (header.tick >= this.current.tick) this.current = assembly.snapshot;
    const gameplay = header.tick >= this.newestEntitiesTick;
    const records = [], seatChanges = gameplay ? this.seatChangesFor(assembly, emittedSlots) : [];
    if (gameplay) {
      for (const entry of decoded.set.values()) {
        records.push(recordFromEntry(entry, header.tick, this.bounds));
        if (entry.cls === 'vehicle') records.push(mountRecordFromEntry(entry, header.tick));
      }
      for (const entry of removedEntries) {
        records.push({ kind: 'tombstone', id: entry.entityId, generation: entry.generation, tick: header.tick });
        if (this.seats.delete(entry.entityId)) seatChanges.push({ id: entry.entityId, generation: entry.generation, seat: null });
      }
      for (const field of decoded.world) if (!field.removed) records.push(worldRecordFromField(field, header.tick));
    }
    const sample = { tick: header.tick, epoch: header.epoch, poses: this.samplePoses(decoded.set.values(), header.tick) };
    const completed = this.complete(assembly);
    if (gameplay) this.newestEntitiesTick = header.tick;
    return { tick: header.tick, epoch: header.epoch, epochChanged, interval: header.interval, ackInput: assembly.own?.ackInput ?? 0,
      ack: { epoch: header.epoch, tick: this.ackTick }, records, seatChanges, sample, snapshotAck: this.snapshotAck, complete: completed };
  }
  receiveOwn(bytes, header, epochChanged) {
    const baseline = this.baselineFor(header);
    if (!baseline && header.baselineTick !== 0) { this.missingBaselines++; this.onMissingBaseline(header); return null; }
    const baselineOwn = header.baselineTick === 0 ? null : this.ownRing.get(header.baselineTick, header.epoch);
    let decoded;
    try { decoded = decodeOwnDatagram(bytes, baselineOwn?.payload ?? null, baselineOwn?.summaries ?? null); }
    catch { this.missingBaselines++; this.onMissingBaseline(header); return null; }
    const records = [];
    if (decoded.cosmeticBytes) records.push(...unpackCosmeticEvents(decoded.cosmeticBytes, this.bounds));
    if (decoded.second) {
      let completed = false;
      if (!this.ownRing.has(header.tick, header.epoch)) {
        const assembly = this.assemblyFor(header.tick, header.epoch, header.baselineTick, baseline ?? emptySnapshot(0, header.epoch));
        assembly.secondOwn = true; completed = this.complete(assembly);
      }
      return { tick: header.tick, epoch: header.epoch, epochChanged, interval: header.interval, ackInput: header.ackInput,
        ack: { epoch: header.epoch, tick: this.ackTick }, records, seatChanges: [], sample: null, snapshotAck: this.snapshotAck, complete: completed };
    }
    if (this.ownRing.has(header.tick, header.epoch)) return null; // already complete: a duplicate
    const assembly = this.assemblyFor(header.tick, header.epoch, header.baselineTick, baseline ?? emptySnapshot(0, header.epoch));
    if (assembly.own) return null;
    assembly.own = { correction: decoded.correction, summaries: decoded.summaries, ackInput: header.ackInput, parts: header.partCount };
    const gameplay = header.tick >= this.newestOwnTick;
    if (gameplay) {
      this.newestOwnTick = header.tick;
      if (decoded.correction) records.push(decoded.correction);
      for (const field of decoded.summaries) if (!field.removed) records.push(worldRecordFromField(field, header.tick));
    }
    const completed = this.complete(assembly);
    return { tick: header.tick, epoch: header.epoch, epochChanged, interval: header.interval, ackInput: header.ackInput,
      ack: { epoch: header.epoch, tick: this.ackTick }, records, seatChanges: [], sample: null, snapshotAck: this.snapshotAck, complete: completed };
  }
  // Gameplay combat records from the reliable events channel.
  // Reliable gameplay records can arrive before the first snapshot datagram
  // (the lanes are independent); there is no baseline to acknowledge yet.
  gameplayRecords(records, tick) {
    return { tick, epoch: this.epoch, epochChanged: false, interval: 2, ackInput: 0, ack: { epoch: this.epoch, tick: this.ackTick },
      records, seatChanges: [], sample: null, snapshotAck: this.epoch >= 1 ? this.snapshotAck : null, complete: false };
  }
}
