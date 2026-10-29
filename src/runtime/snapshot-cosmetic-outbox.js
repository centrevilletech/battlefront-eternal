// What one client is owed of the cosmetic combat events (snapshot-host.js):
// bolt launches and impacts, dynamic projectile samples and their ends,
// explosions and presentation events, packed into its own datagrams.
//
// A shot is not lost to a full datagram, a snapshot skipped over the state
// budget, or a dropped datagram. What the client needs to see a shot at all
// (a launch, a projectile's first sample, a terminal, an explosion) stays
// owed until the client acknowledges a snapshot that carried it; when it
// acknowledges a later snapshot instead (or none within RESEND_TICKS), the
// event is sent again, as long as the client could still present it.
// Records keep their identity, so the client draws each one once however
// often it arrives. Refinements (a known projectile's later samples, which
// a newer sample replaces, and presentation events) are sent once and,
// when the space runs out, wait for the next snapshot until they are stale.
// Space goes to the client's own events first, then launches and first
// samples, then terminals and explosions, then presentation, then samples,
// nearest first within each.
import { cosmeticPriority } from './snapshot-cosmetics.js';
import { SnapshotWriter as Writer } from './snapshot-codec.js';
import { LAUNCH_PRESENTATION_TICKS } from './network-entity-combat.js';

const RANK_OWN = 0, RANK_LAUNCH = 1, RANK_END = 2, RANK_PRESENTATION = 3, RANK_SAMPLE = 4, RANK_OWED = RANK_END;
// Unacknowledged sends older than this are presumed lost.
export const RESEND_TICKS = 30;
// Cosmetic events kept for one client at most; the oldest give way.
export const MAX_OWED_EVENTS = 1024;
// Short-lived cosmetics: the client shows an impact, explosion or sample
// up to 18 ticks after its tick.
const COSMETIC_TICKS = 18;

// One drained combat record as every client's outbox sees it: encoded once.
export function cosmeticEvent(record, view, bytes) {
  const schema = record.schema, id = record.id;
  // A launch can start late on the client up to LAUNCH_PRESENTATION_TICKS
  // after the tick it was fired on (CombatPresentation.present).
  if (schema === 1) return { record, view, bytes, key: `l${id}`, kind: 'launch', projectile: id, must: true, expires: Math.ceil(view.executionTick + view.life * 60) + LAUNCH_PRESENTATION_TICKS };
  if (schema === 2 || schema === 7) return { record, view, bytes, key: `t${id}`, kind: 'terminal', projectile: id, must: true, expires: record.tick + COSMETIC_TICKS + LAUNCH_PRESENTATION_TICKS };
  if (schema === 6) return { record, view, bytes, key: `s${id}`, kind: 'sample', projectile: id, must: false, expires: record.tick + COSMETIC_TICKS };
  if (schema === 5) return { record, view, bytes, key: `e${id}`, kind: 'explosion', projectile: null, must: true, expires: record.tick + COSMETIC_TICKS };
  return { record, view, bytes, key: `e${id}`, kind: 'presentation', projectile: null, must: false, expires: record.tick + LAUNCH_PRESENTATION_TICKS };
}

export class CosmeticOutbox {
  constructor({ range = 300 } = {}) {
    this.range = range;
    this.entries = new Map(); // key -> { event, d2, rank, sentTick }
    this.inflight = []; // [{ tick, events }], oldest first: owed events sent and not yet acknowledged
    this.projectiles = new Map(); // id -> { expires, confirmed }: projectiles this client was sent
    this.dropped = 0; this.owedDropped = 0; this.resent = 0; this.prunedTick = -Infinity;
  }
  // This snapshot's events, as relevant to a client at `position` whose
  // body is `unitId`.
  offer(events, position, unitId) {
    for (const event of events) {
      const known = event.projectile !== null ? this.projectiles.get(event.projectile) : null;
      let d2 = cosmeticPriority(event.view, position, unitId, this.range);
      if (event.kind === 'terminal') {
        // A terminal ends what this client was sent, wherever it ended; a
        // removal (no position) concerns nobody else.
        if (known) d2 ??= this.range * this.range;
        else if (event.record.schema === 7) continue;
      }
      if (d2 === null) continue;
      if (event.kind === 'terminal') this.entries.delete(`s${event.projectile}`);
      const rank = d2 < 0 ? RANK_OWN : event.kind === 'launch' || event.kind === 'sample' && !known?.confirmed ? RANK_LAUNCH : event.must ? RANK_END : event.kind === 'presentation' ? RANK_PRESENTATION : RANK_SAMPLE;
      this.entries.set(event.key, { event, d2, rank, sentTick: -1 });
    }
    if (this.entries.size > MAX_OWED_EVENTS) for (const key of this.entries.keys()) {
      this.entries.delete(key); this.dropped++;
      if (this.entries.size <= MAX_OWED_EVENTS) break;
    }
  }
  // Pack what fits into the first own datagram's cosmetic space and one
  // second datagram. Owed events go only into a snapshot the client can
  // acknowledge (`confirmable`; an own-only datagram is not). Refinements
  // take at most `spare` bytes, what is left of the client's state budget,
  // so they never push it into skipping the snapshots owed events ride.
  pack(tick, firstBudget, secondBudget, confirmable, spare = Infinity) {
    // Expired: whatever the client never got is counted (one still in
    // flight may well have arrived).
    for (const [key, entry] of this.entries) if (entry.event.expires < tick) { this.entries.delete(key); if (entry.sentTick < 0) { this.dropped++; if (entry.rank <= RANK_OWED) this.owedDropped++; } }
    if (tick >= this.prunedTick + RESEND_TICKS || tick < this.prunedTick) { this.prunedTick = tick; for (const [id, projectile] of this.projectiles) if (projectile.expires < tick) this.projectiles.delete(id); }
    while (this.inflight.length && this.inflight[0].tick <= tick - RESEND_TICKS) this.settle(this.inflight.shift(), false);
    const candidates = [];
    let bytes = 0;
    for (const entry of this.entries.values()) {
      if (entry.sentTick >= 0 || !confirmable && entry.rank <= RANK_OWED) continue;
      candidates.push(entry); bytes += 2 + entry.event.bytes.length;
    }
    // Order only matters when something has to wait.
    if (bytes > firstBudget) candidates.sort((a, b) => a.rank - b.rank || a.d2 - b.d2);
    const first = new Writer(), second = new Writer(), owed = [];
    let refinements = 0;
    for (const entry of candidates) {
      const event = entry.event, size = 2 + event.bytes.length, w = first.length + size <= firstBudget ? first : second.length + size <= secondBudget ? second : null;
      if (!w || entry.rank > RANK_OWED && refinements + size > spare) continue;
      if (entry.rank > RANK_OWED) refinements += size;
      w.u16(event.bytes.length); w.raw(event.bytes);
      if (event.kind === 'launch' || event.kind === 'sample') {
        // Sent: its terminal now concerns this client wherever it lands,
        // for as long as the client may still be drawing it.
        const until = event.kind === 'launch' ? event.expires : tick + Math.ceil(event.view.life * 60) + COSMETIC_TICKS + LAUNCH_PRESENTATION_TICKS;
        const known = this.projectiles.get(event.projectile);
        if (known) known.expires = Math.max(known.expires, until); else this.projectiles.set(event.projectile, { expires: until, confirmed: false });
      }
      if (confirmable && entry.rank <= RANK_OWED) { entry.sentTick = tick; owed.push(event); }
      else this.entries.delete(event.key);
    }
    if (owed.length) this.inflight.push({ tick, events: owed });
    return { first: first.length ? first.finish() : null, second: second.length ? second.finish() : null };
  }
  // The client acknowledged the snapshot at `tick` (its newest complete
  // one): what that snapshot carried arrived; anything sent before it and
  // not acknowledged is presumed lost.
  acknowledge(tick) {
    while (this.inflight.length && this.inflight[0].tick <= tick) { const sent = this.inflight.shift(); this.settle(sent, sent.tick === tick); }
  }
  // Everything in flight is presumed lost (the client asked to resync).
  resend() { for (const sent of this.inflight.splice(0)) this.settle(sent, false); }
  settle({ tick, events }, delivered) {
    for (const event of events) {
      // A newer sample may have replaced this one meanwhile; its projectile
      // is still known to the client once this one arrived.
      if (delivered && event.projectile !== null) { const known = this.projectiles.get(event.projectile); if (known) known.confirmed = true; }
      const entry = this.entries.get(event.key);
      if (entry?.event !== event || entry.sentTick !== tick) continue;
      if (delivered) this.entries.delete(event.key);
      else { entry.sentTick = -1; this.resent++; }
    }
  }
}
