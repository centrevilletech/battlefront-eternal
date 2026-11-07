// The per-client `own` datagram: own correction first, then the client's
// own summaries, then cosmetic events in the space left. A second own
// datagram carries cosmetic overflow only. The first one's part count says
// whether a second follows (2) or not (1); the second is part 1 of 2. A
// client completes (and acknowledges) a snapshot only with both, so an
// acknowledgement confirms every event the snapshot carried; clients
// released before this ignore own part numbers and complete on the first.
// The correction payload is the existing schema-11 bytes,
// delta-encoded against the baseline tick's own payload at 8-byte word
// granularity: changed words travel verbatim, so every f64 field value
// arrives exactly as the absolute form would carry it.
import {
  SNAPSHOT_OWN_MAGIC, SNAPSHOT_HEADER_BYTES, SNAPSHOT_MAX_DATAGRAM,
  SnapshotWriter as Writer, writeSnapshotHeader,
  encodeWorldSection, encodeWordDelta,
} from './snapshot-codec.js';

export { encodeWordDelta as encodeOwnDelta } from './snapshot-codec.js';

const SECTION_CORRECTION = 1, SECTION_SUMMARIES = 2, SECTION_COSMETICS = 4, SECTION_SECOND = 8;
// The datagram and its correction section are written into scratch
// (encoding is synchronous); only the finished datagram is copied out.
const datagram = new Writer(2048), correctionSection = new Writer(2048);
// correction: {id, generation, schema, payload} with baselinePayload the
// own payload of the baseline tick (null for absolute). summaries are
// world-field maps (current vs baseline). cosmeticBytes are pre-packed by
// snapshot-cosmetics.js. The caller owns fitting cosmetics into the budget;
// this throws rather than exceed one datagram.
export function encodeOwnDatagram({ epoch, tick, interval = 2, baselineTick = 0, ackInput = 0, correction = null, baselinePayload = null, correctionDelta = null, summaries = null, baselineSummaries = null, summariesSection = null, cosmeticBytes = null, second = false, ownParts = second ? 2 : 1 }) {
  const w = datagram.reset();
  writeSnapshotHeader(w, { magic: SNAPSHOT_OWN_MAGIC, epoch, tick, interval, baselineTick, partIndex: second ? 1 : 0, partCount: ownParts, ackInput });
  let flags = second ? SECTION_SECOND : 0, s = null, summary = null;
  if (correction) {
    flags |= SECTION_CORRECTION;
    s = correctionSection.reset(); s.u32(correction.id); s.u32(correction.generation); s.u16(correction.schema); s.raw(correctionDelta ?? encodeWordDelta(correction.payload, baselinePayload));
  }
  if (summaries || summariesSection) { flags |= SECTION_SUMMARIES; summary = summariesSection ?? encodeWorldSection(summaries, baselineSummaries); }
  const cosmetics = cosmeticBytes?.length ? cosmeticBytes : null;
  if (cosmetics) flags |= SECTION_COSMETICS;
  w.u8(flags);
  // The sections in order: correction, summaries, cosmetics.
  if (s) { w.u16(s.length); w.rawFrom(s.buffer, 0, s.length); }
  if (summary) { w.u16(summary.length); w.raw(summary); }
  if (cosmetics) { w.u16(cosmetics.length); w.raw(cosmetics); }
  const bytes = w.finish();
  if (bytes.length > SNAPSHOT_MAX_DATAGRAM) throw Error('own datagram exceeds budget');
  return bytes;
}
export function ownDatagramBudget({ correction = null, baselinePayload = null, summaries = null, baselineSummaries = null } = {}) {
  let used = SNAPSHOT_HEADER_BYTES + 1;
  if (correction) used += 2 + 10 + encodeWordDelta(correction.payload, baselinePayload).length;
  if (summaries) used += 2 + encodeWorldSection(summaries, baselineSummaries).length;
  return Math.max(0, SNAPSHOT_MAX_DATAGRAM - used - 2);
}
