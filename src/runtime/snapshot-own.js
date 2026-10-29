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
  SnapshotWriter as Writer, SnapshotReader as Reader, writeSnapshotHeader,
  readSnapshotHeader, encodeWorldSection, decodeWorldSection, encodeWordDelta, decodeWordDelta,
} from './snapshot-codec.js';

export { encodeWordDelta as encodeOwnDelta, decodeWordDelta as decodeOwnDelta } from './snapshot-codec.js';

const SECTION_CORRECTION = 1, SECTION_SUMMARIES = 2, SECTION_COSMETICS = 4, SECTION_SECOND = 8;
// correction: {id, generation, schema, payload} with baselinePayload the
// own payload of the baseline tick (null for absolute). summaries are
// world-field maps (current vs baseline). cosmeticBytes are pre-packed by
// snapshot-cosmetics.js. The caller owns fitting cosmetics into the budget;
// this throws rather than exceed one datagram.
export function encodeOwnDatagram({ epoch, tick, interval = 2, baselineTick = 0, ackInput = 0, correction = null, baselinePayload = null, correctionDelta = null, summaries = null, baselineSummaries = null, summariesSection = null, cosmeticBytes = null, second = false, ownParts = second ? 2 : 1 }) {
  const w = new Writer();
  writeSnapshotHeader(w, { magic: SNAPSHOT_OWN_MAGIC, epoch, tick, interval, baselineTick, partIndex: second ? 1 : 0, partCount: ownParts, ackInput });
  let flags = second ? SECTION_SECOND : 0;
  const sections = [];
  if (correction) {
    flags |= SECTION_CORRECTION;
    const s = new Writer(); s.u32(correction.id); s.u32(correction.generation); s.u16(correction.schema); s.raw(correctionDelta ?? encodeWordDelta(correction.payload, baselinePayload));
    sections.push(s.finish());
  }
  if (summaries || summariesSection) { flags |= SECTION_SUMMARIES; sections.push(summariesSection ?? encodeWorldSection(summaries, baselineSummaries)); }
  if (cosmeticBytes?.length) { flags |= SECTION_COSMETICS; sections.push(cosmeticBytes); }
  w.u8(flags);
  for (const section of sections) { w.u16(section.length); w.raw(section); }
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
export function decodeOwnDatagram(bytes, baselinePayload = null, baselineSummaries = null) {
  const header = readSnapshotHeader(bytes);
  if (header.magic !== SNAPSHOT_OWN_MAGIC) throw Error('not an own datagram');
  const r = new Reader(bytes); r.at = SNAPSHOT_HEADER_BYTES;
  const flags = r.u8(), out = { header, second: !!(flags & SECTION_SECOND), correction: null, summaries: [], cosmeticBytes: null };
  const section = () => r.raw(r.u16());
  if (flags & SECTION_CORRECTION) {
    const body = new Reader(section());
    const id = body.u32(), generation = body.u32(), schema = body.u16();
    try { out.correction = { kind: 'correction', id, generation, tick: header.tick, schema, payload: decodeWordDelta(body.bytes.subarray(body.at), baselinePayload) }; } catch { throw Error('own baseline not held'); }
  }
  if (flags & SECTION_SUMMARIES) out.summaries = decodeWorldSection(section(), baselineSummaries);
  if (flags & SECTION_COSMETICS) out.cosmeticBytes = section();
  if (r.at !== bytes.length) throw Error('own datagram trailing bytes');
  return out;
}
