import { entityPostNames } from './entity-spawn-posts.js';
import { attachPresentationCapture } from './entity-presentation-events.js';
import { captureUnitState, UNIT_STATE_ID } from './entity-unit-state.js';
import { captureWorldPresentation } from './entity-world-state.js';
// The snapshot-delta host (docs/snapshot-delta-design.md): one authoritative
// world owes each client a snapshot every publication interval (30 Hz
// online, every tick locally), delta-encoded against a snapshot that client
// acknowledged. Each snapshot is encoded against at most a few shared
// baselines (MAX_SHARED_BASELINES, plus the empty baseline for clients that
// acknowledged none of them), chosen from the clients' recent
// acknowledgements; each client joins the newest chosen baseline it has
// itself acknowledged, never one only another client holds. The own
// datagram is per client. Publication returns the shared parts once,
// grouped by baseline with their client lists, plus the per-client own
// datagrams: the transport side fans a group out to its clients, so this
// thread neither copies the shared bytes per client nor builds a packet
// object per datagram per client.
//
// No per-actor scheduling or packet receipts. Each client has a state byte
// budget (stateBytesPerSecond over a sliding second): over budget, it skips
// whole snapshots (the next one is still encoded against its acknowledged
// baseline), except that its own datagram alone still goes out every
// OWN_FALLBACK_MS so own corrections never starve. A client heard from
// (input or acknowledgement) less recently than staleAfterMs gets no
// snapshots until it is heard again, so a session whose `leave` was lost
// stops costing encodes and bandwidth; staleClients() lets the server
// evict it.
import { EntityWorldCapture, capturePlayerCorrection } from './network-entity-capture.js';
import { createEntityRegistry, entityRegistryDescriptor } from './network-entity-registry.js';
import { captureWorldSummaries, captureBattleSummary, capturePostSummary, captureRosterSummaries, captureRecordSummaries } from './network-entity-summary.js';
import { readStateBudget } from './network-entity-budget.js';
import { CombatCapture, attachCombatCapture, decodeCombat } from './network-entity-combat.js';
import { feedbackConcerns } from './network-combat-feedback.js';
import { EntityShotCounters } from './network-entity-shots.js';
import { INPUT_CODES, DEFAULT_BINDING_PROFILES, decodeEntityInput, isSnapshotAckKeepalive, decodeSnapshotAckKeepalive } from './network-entity-input.js';
import { EntityEdges } from './network-entity-edge-host.js';
import { encodeEntities } from './snapshot-codec.js';
import { SnapshotRing } from './snapshot-ring.js';
import { SnapshotSlotTable, WorldFieldStagger, snapshotFromRecords, ownSummaryFields } from './snapshot-records.js';
import { encodeOwnDatagram } from './snapshot-own.js';
import { encodeWordDelta, encodeWorldSection, SNAPSHOT_MAX_DATAGRAM, SNAPSHOT_HEADER_BYTES } from './snapshot-codec.js';
import { isGameplayCombat, encodeCosmeticEvent } from './snapshot-cosmetics.js';
import { CosmeticOutbox, cosmeticEvent } from './snapshot-cosmetic-outbox.js';

// The common world fields (battle, posts, roster, records) are captured only
// when one of them is due, and the roster (refreshed every 60 ticks, the
// costly one) only when one of its own chunks is due or its chunk set
// changed; the records (every 300 ticks) likewise when one of theirs is
// due. The presentation schemas are captured every snapshot. A captured
// schema is complete, so a field it no longer produces is removed.
const COMMON_WORLD_SCHEMAS = new Set([1, 2, 4, 5]), BATTLE_COMPLETE_SCHEMAS = [1, 2];
const ROSTER_WORLD_SCHEMAS = new Set([4]);
const RECORD_WORLD_SCHEMAS = new Set([5]);
const PRESENTATION_WORLD_SCHEMAS = [7, 8, 9, 10];
const MAX_SHARED_BASELINES = 3;
// Acknowledged ticks remembered per client (newest first); any of them still
// held in both rings is a baseline that client can decode against.
const ACK_HISTORY = 8;
// Clients released before the snapshot ring held 128 snapshots index it by
// tick % 128, so they keep only baselines under 128 ticks old: one older is
// silently undecodable there. Baselines stay within that age until every
// supported client holds 128 snapshots.
export const CLIENT_BASELINE_TICKS = 128;
// Baseline choice cost: a delta grows with its baseline's age in ticks; an
// empty baseline (a full snapshot) costs more than any held one.
const EMPTY_BASELINE_COST = 1024;
const BUDGET_WINDOW_MS = 1000, OWN_FALLBACK_MS = 100, STALE_AFTER_MS = 5000;
const COSMETIC_RANGE = 300;
// Encoded datagrams come from Writer.finish(), a fresh exact-size buffer,
// so the ArrayBuffer can be handed over (and transferred by the caller)
// without copying. Anything else is sliced to its exact extent.
const asArrayBuffer = bytes => bytes.byteOffset === 0 && bytes.byteLength === bytes.buffer.byteLength
  ? bytes.buffer : bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
// One client's datagrams from a publication, in send order: its baseline
// group's shared parts first, then its own datagrams (a snapshot only
// completes once both have been applied). Used by the local transport and
// tests; the dedicated server fans groups out on its main thread instead.
export function publicationBuffersFor(publication, client) {
  const buffers = [];
  for (const group of publication.shared ?? []) if (group.clients.includes(client)) buffers.push(...group.parts);
  for (const packet of publication.ownPackets ?? []) if (packet.client === client) buffers.push(packet.buffer);
  return buffers;
}
// The players attached, by Character, with the link the session reports
// (network-entity-summary.js captureWorldSummaries builds the same map).
function attachedPlayers(world) {
  const players = new Map();
  for (const p of world.players?.values?.() || []) if (p.character) players.set(p.character, { link: p.link || null });
  return players;
}
// Up to `max` baseline ticks for one snapshot at `tick`, chosen greedily to
// minimise the summed baseline age over the clients, each client counted at
// the newest chosen tick it acknowledged (or the empty baseline). `acks` is
// one newest-first list of held acknowledged ticks per client. Ties go to
// the newer tick.
export function chooseSharedBaselines(tick, acks, max = MAX_SHARED_BASELINES) {
  const holders = new Map(); // tick -> indices of the clients that acknowledged it
  for (let i = 0; i < acks.length; i++) for (const t of acks[i]) { const h = holders.get(t); if (h) h.push(i); else holders.set(t, [i]); }
  const candidates = [...holders.keys()].sort((a, b) => b - a);
  if (candidates.length <= max) return candidates;
  const costs = new Array(acks.length).fill(EMPTY_BASELINE_COST), chosen = [];
  while (chosen.length < max) {
    let best = -1, bestGain = 0;
    for (const candidate of candidates) {
      const age = tick - candidate;
      let gain = 0;
      for (const i of holders.get(candidate)) if (costs[i] > age) gain += costs[i] - age;
      if (gain > bestGain) { best = candidate; bestGain = gain; }
    }
    if (best < 0) break;
    chosen.push(best);
    for (const i of holders.get(best)) costs[i] = Math.min(costs[i], tick - best);
  }
  return chosen.sort((a, b) => b - a);
}
export function createSnapshotHost({ game, world, epoch = 1, local = false, stateBytesPerSecond, staleAfterMs = local ? Infinity : STALE_AFTER_MS, now = () => performance.now() } = {}) {
  if (!game || !world) throw Error('snapshot host requires a game and a multiplayer world');
  const interval = local ? 1 : 2;
  // A local client shares the process: no link to protect.
  const budget = local ? Infinity : readStateBudget({ stateBytesPerSecond }).bytesPerSecond;
  if (!(staleAfterMs > 0)) throw Error('invalid stale client time');
  const clients = new Map();
  const ring = new SnapshotRing(128);
  const slots = new SnapshotSlotTable();
  const stagger = new WorldFieldStagger();
  let registry, capture, shots, combatCapture, combatDispose, presentationDispose;
  let lastSnapshot = null, capturedTick = -1, diagnosedTick = -1, rosterLength = -1;
  const encodeCache = { tick: -1, parts: new Map() };

  function ensureRuntime() {
    if (registry) return;
    registry = createEntityRegistry(game.data, { contentEpoch: epoch, inputCodes: INPUT_CODES, bindingProfiles: DEFAULT_BINDING_PROFILES, posts: entityPostNames(game) });
    shots = new EntityShotCounters(game.simEvents);
    combatCapture = new CombatCapture({ tick: () => game.worldClock.tick, definitionId: registry.combat.definitionId, generation: epoch });
    combatDispose = attachCombatCapture(game, combatCapture);
    presentationDispose = attachPresentationCapture(game, combatCapture, registry);
    capture = new EntityWorldCapture(game, registry, { fireCounter: unit => shots.counter(unit) });
  }
  function addClient(id) {
    ensureRuntime();
    if (clients.has(id)) throw Error('snapshot client already joined');
    clients.set(id, {
      started: false, ready: false,
      // Acknowledged ticks, newest first; baselineTick is the newest.
      acked: [], baselineTick: 0,
      ownRing: new SnapshotRing(128),
      edges: new EntityEdges({ actor: world.players.get(id), world, client: id, registry }),
      // State bytes sent inside the budget window: [time, bytes] pairs.
      sent: [], sentBytes: 0, ownSentAt: -Infinity, heardAt: now(),
      // The cosmetic combat events this client is owed (snapshot-cosmetic-outbox.js).
      outbox: new CosmeticOutbox({ range: COSMETIC_RANGE }),
      metrics: { snapshots: 0, datagrams: 0, bytes: 0, cosmeticsDropped: 0, cosmeticsOwedDropped: 0, cosmeticsResent: 0, emptyBaselines: 0, budgetSkips: 0, ownOnly: 0, staleSkips: 0 },
      ownSummariesCache: null, ownSummariesDue: 0,
      inputDiagnostics: { accepted: 0, rejected: 0, reasons: {}, last: {} },
    });
  }
  function removeClient(id) { clients.delete(id); }
  function setStarted(id, started) { const s = clients.get(id); if (s) { s.started = Boolean(started); if (s.started) s.heardAt = now(); } }
  function setReady(id) { const s = clients.get(id); if (s) s.ready = true; }
  // A client's acknowledgement is cumulative (its newest complete snapshot),
  // so each new one is a snapshot it holds. Only ticks this host published
  // to this client, still in both rings, are kept as its baselines.
  function acceptAck(state, ack) {
    if (!ack || ack.epoch !== epoch || !Number.isSafeInteger(ack.tick)) return;
    if (ack.tick > state.baselineTick && ring.has(ack.tick, epoch) && state.ownRing.has(ack.tick, epoch)) {
      state.acked.unshift(ack.tick);
      if (state.acked.length > ACK_HISTORY) state.acked.length = ACK_HISTORY;
      state.baselineTick = ack.tick;
      state.outbox.acknowledge(ack.tick);
    }
  }
  // The client's acknowledged ticks still held (and young enough for the
  // client to hold) at `tick`, newest first.
  function heldAcks(state, tick) {
    const acked = state.acked;
    for (let i = acked.length - 1; i >= 0; i--) if (tick - acked[i] >= CLIENT_BASELINE_TICKS || !ring.has(acked[i], epoch) || !state.ownRing.has(acked[i], epoch)) acked.splice(i, 1);
    return acked;
  }
  function chargeBytes(state, at, bytes) { state.sent.push(at, bytes); state.sentBytes += bytes; }
  function budgetUsed(state, at) {
    const sent = state.sent;
    let drop = 0;
    while (drop < sent.length && sent[drop] <= at - BUDGET_WINDOW_MS) { state.sentBytes -= sent[drop + 1]; drop += 2; }
    if (drop) sent.splice(0, drop);
    return state.sentBytes;
  }
  function input(id, bytes) {
    const state = clients.get(id);
    if (!state) return;
    state.heardAt = now();
    if (!state.started) return;
    const view = new Uint8Array(bytes);
    if (isSnapshotAckKeepalive(view)) { acceptAck(state, decodeSnapshotAckKeepalive(view)); return; }
    const packet = decodeEntityInput(view, registry);
    if (packet.epoch !== epoch || packet.generation !== world.players.get(id)?.unit?.generation) return;
    acceptAck(state, packet.snapshotAck);
    state.edges.receive(packet, game.worldClock.tick);
    for (const command of packet.commands) {
      const accepted = world.input(id, command), stats = state.inputDiagnostics;
      stats[accepted ? 'accepted' : 'rejected']++;
      stats.last = { sequence: command.sequence, tick: command.tick, generation: command.generation, epoch: command.epoch, serverTick: game.worldClock.tick, accepted, lookTotal: command.lookTotal, down: command.down.slice(0, 32) };
    }
  }
  function edgeAck(id, message) { const state = clients.get(id); if (!state) return; state.heardAt = now(); state.edges?.acknowledge(message); }
  function advanceEdges() {
    const faults = [];
    for (const [client, state] of clients) if (state.started) {
      try {
        const actor = world.players.get(client);
        if ((actor.unit?.generation ?? 0) !== state.edges.generation) state.edges.reset(epoch, actor.unit?.generation ?? 0);
        state.edges.advance(game.worldClock.tick + 1);
      } catch (error) { state.started = false; faults.push({ client, error }); }
    }
    return faults;
  }
  function consumedEdges(tick) {
    const faults = [];
    for (const [client, state] of clients) {
      try { state.edges.consumed(tick); } catch (error) { state.started = false; faults.push({ client, error }); }
    }
    return faults;
  }
  function captureSnapshot(tick) {
    const characters = game.battle.characters;
    for (const [slot, c] of characters.entries()) c.rosterSlot = slot;
    const records = capture.capture(tick);
    // The fields captureWorldSummaries produces for everyone. Skipping the
    // roster when none of its chunks is due and its chunk count (set by the
    // character count) is unchanged publishes exactly what capturing it would.
    const captureBattle = stagger.anyDue(tick, COMMON_WORLD_SCHEMAS);
    const captureRoster = captureBattle && (characters.length !== rosterLength || stagger.schemaDue(tick, ROSTER_WORLD_SCHEMAS));
    const captureRecords = captureBattle && stagger.schemaDue(tick, RECORD_WORLD_SCHEMAS);
    const complete = new Set(PRESENTATION_WORLD_SCHEMAS), common = [];
    if (captureBattle) {
      common.push(captureBattleSummary(game.battle, { tick, epoch }), ...game.posts.posts.map(post => capturePostSummary(post, { tick, epoch, registry })));
      for (const schema of BATTLE_COMPLETE_SCHEMAS) complete.add(schema);
    }
    if (captureRoster) {
      common.push(...captureRosterSummaries(game.battle, attachedPlayers(world), { tick, epoch }));
      complete.add(4); rosterLength = characters.length;
    }
    // Everyone's records (battle-stats.js), for DETAILS from Tab's table in
    // whole seconds, and exact once the scores stop (they no longer
    // change) for the end table's DETAILS and AWARDS.
    if (captureRecords) {
      common.push(...captureRecordSummaries(game.battle, registry, { tick, epoch, quantum: game.battle.gameOver ? 1 : 1000 }));
      complete.add(5);
    }
    const worldRecords = stagger.update(tick, [
      ...common,
      ...[...game.units.all()].filter(u => u.soldier && u.combat && !u.removed && stagger.isDue(tick, `6:${UNIT_STATE_ID + u.id}`)).map(u => captureUnitState(u, registry, tick)),
      ...captureWorldPresentation(game, tick, epoch),
    ], complete);
    const snapshot = snapshotFromRecords({ tick, epoch, records, worldRecords, slots, previous: lastSnapshot });
    ring.set(tick, epoch, snapshot);
    lastSnapshot = snapshot; capturedTick = tick;
    encodeCache.tick = tick; encodeCache.parts.clear();
    return snapshot;
  }
  // The entity parts of this snapshot against one chosen baseline (0: the
  // empty baseline), encoded once and shared by every client mapped to it.
  function partsFor(snapshot, baselineTick) {
    let parts = encodeCache.parts.get(baselineTick);
    if (parts) return parts;
    const baseline = baselineTick === 0 ? null : ring.get(baselineTick, epoch);
    if (baselineTick !== 0 && !baseline) throw Error('chosen snapshot baseline not held');
    parts = encodeEntities(snapshot, baseline, { interval });
    encodeCache.parts.set(baselineTick, parts);
    return parts;
  }

  function publish({ droppedTicks = 0 } = {}) {
    const capturedAtUnixMs = Date.now(), at = now();
    const shared = [], groupsByBaseline = new Map(), ownPackets = [], inputAcks = [], edgeResults = [], diagnostics = [], faults = [], reliableEvents = [];
    const tick = game.worldClock.tick;
    const eligibilityCache = new Map();
    // Due by distance from the last snapshot, not by tick parity: a server
    // that publishes once per catch-up batch can land on any tick. Until
    // then the first snapshot aligns to the interval, so an unhurried host
    // keeps the even-tick cadence.
    const snapshotDue = capturedTick < 0 ? tick % interval === 0 : tick - capturedTick >= interval;
    let snapshot = null, cosmetics = [], gameplay = [];
    if (snapshotDue) {
      try { snapshot = captureSnapshot(tick); } catch (error) { faults.push({ client: null, error }); }
      if (combatCapture) {
        for (const event of combatCapture.drain()) {
          try {
            const view = decodeCombat(event.record);
            if (isGameplayCombat(event.record)) gameplay.push({ record: event.record, view });
            else cosmetics.push(cosmeticEvent(event.record, view, encodeCosmeticEvent(event.record)));
          } catch { /* malformed cosmetic content is dropped, never fatal */ }
        }
      }
      if (snapshot && Math.floor(tick / 60) !== Math.floor(diagnosedTick / 60)) {
        diagnosedTick = tick;
        if (capture.unsupported.length) for (const client of clients.keys()) diagnostics.push({ client, actors: capture.unsupported.slice(0, 256).map(e => ({ id: e.entityId, message: String(e.message).slice(0, 160) })) });
      }
    }
    // Recipients of this snapshot: full (entity parts and own) or own only.
    const recipients = [];
    for (const [client, state] of clients) if (state.started) {
      const actor = world.players.get(client);
      if (actor) inputAcks.push({ client, epoch, serverTick: tick, generation: actor.unit?.generation ?? 0, ack: actor.mailbox.ack, ackTick: actor.mailbox.ackTick, dropped: droppedTicks, discardedInput: actor.mailbox.discarded ?? 0, discardedActions: actor.actions?.discarded ?? 0, ackActions: 0 });
      const feedback = state.edges.feedback();
      if (feedback.received.length || feedback.results.length) edgeResults.push({ client, ...feedback });
      if (!snapshot || !state.ready) continue;
      if (at - state.heardAt > staleAfterMs) { state.metrics.staleSkips++; continue; }
      // Owed even when the budget skips this snapshot: they go in the next.
      if (cosmetics.length) state.outbox.offer(cosmetics, actor?.unit?.seat?.vehicle?.position || actor?.unit?.position, actor?.unit?.id);
      if (gameplay.length) {
        // Damage feedback goes only to the attacker's and target's pages.
        const records = [];
        for (const e of gameplay) if (feedbackConcerns(e.view, actor?.unit?.id)) records.push(e.record);
        if (records.length) reliableEvents.push({ client, tick, records });
      }
      if (budgetUsed(state, at) < budget) recipients.push({ client, state, actor, full: true });
      else {
        state.metrics.budgetSkips++;
        if (at - state.ownSentAt >= OWN_FALLBACK_MS) recipients.push({ client, state, actor, full: false });
      }
    }
    const chosen = chooseSharedBaselines(tick, recipients.filter(r => r.full).map(r => heldAcks(r.state, tick)));
    for (const { client, state, actor, full } of recipients) {
      try {
        // The newest chosen baseline this client acknowledged; an own-only
        // datagram uses its newest acknowledgement.
        const held = heldAcks(state, tick);
        const baselineTick = (full ? chosen.find(t => held.includes(t)) : held[0]) ?? 0;
        const parts = full ? partsFor(snapshot, baselineTick) : null;
        if (full && baselineTick === 0) state.metrics.emptyBaselines++;
        // Own summaries are delta-encoded state; capturing them at 10 Hz
        // staggered per client keeps 64-client publication bounded. The
        // spawn summary's deadTime lags at most 100 ms as a result.
        if (!state.ownSummariesCache || tick >= state.ownSummariesDue) {
          state.ownSummariesCache = ownSummaryFields(actor.spectator ? [] : captureWorldSummaries(game, world, registry, { tick, epoch, playerId: client, includeCommon: false, eligibilityCache }));
          state.ownSummariesDue = tick + 6;
        }
        const ownSummaries = state.ownSummariesCache;
        const correction = actor.spectator ? null : capturePlayerCorrection(game, world, client, { registry, bindingProfileId: 0, epoch });
        const baselineOwn = state.ownRing.get(baselineTick, epoch);
        state.ownRing.set(tick, epoch, { payload: correction?.payload ?? baselineOwn?.payload ?? null, summaries: ownSummaries });
        const correctionDelta = correction ? encodeWordDelta(correction.payload, baselineOwn?.payload ?? null) : null;
        const summariesSection = encodeWorldSection(ownSummaries, baselineOwn?.summaries ?? null);
        const header = { epoch, tick, interval, baselineTick, ackInput: actor.mailbox.ack, correction, correctionDelta, summariesSection };
        const cosmeticBudget = Math.max(0, SNAPSHOT_MAX_DATAGRAM - SNAPSHOT_HEADER_BYTES - 1 - (correction ? 12 + correctionDelta.length : 0) - (2 + summariesSection.length) - 2);
        const spare = budget - budgetUsed(state, at) - (full ? parts.reduce((n, part) => n + part.byteLength, 0) : 0) - (SNAPSHOT_MAX_DATAGRAM - cosmeticBudget);
        const cosmeticBytes = state.outbox.pack(tick, cosmeticBudget, SNAPSHOT_MAX_DATAGRAM - SNAPSHOT_HEADER_BYTES - 3, full, spare);
        // Join this snapshot's shared group for the baseline its parts
        // were encoded against; the transport fans the group's parts out
        // to every listed client. Entities come before own in every
        // consumer's send order: a snapshot only completes (and can be
        // acknowledged) once both have been applied by the client.
        if (full) {
          let group = groupsByBaseline.get(baselineTick);
          if (!group) {
            group = { epoch, tick, baselineTick, capturedAtUnixMs, clients: [], parts: parts.map(asArrayBuffer) };
            group.bytes = group.parts.reduce((n, part) => n + part.byteLength, 0);
            groupsByBaseline.set(baselineTick, group); shared.push(group);
          }
          group.clients.push(client);
          state.metrics.datagrams += parts.length; state.metrics.bytes += group.bytes;
          chargeBytes(state, at, group.bytes);
        } else state.metrics.ownOnly++;
        const sendOwn = buffer => { ownPackets.push({ client, epoch, tick, own: true, capturedAtUnixMs, buffer: asArrayBuffer(buffer) }); state.metrics.datagrams++; state.metrics.bytes += buffer.byteLength; chargeBytes(state, at, buffer.byteLength); };
        sendOwn(encodeOwnDatagram({ ...header, cosmeticBytes: cosmeticBytes.first, ownParts: cosmeticBytes.second ? 2 : 1 }));
        state.ownSentAt = at;
        if (cosmeticBytes.second) sendOwn(encodeOwnDatagram({ epoch, tick, interval, baselineTick, ackInput: actor.mailbox.ack, cosmeticBytes: cosmeticBytes.second, second: true }));
        state.metrics.cosmeticsDropped = state.outbox.dropped; state.metrics.cosmeticsOwedDropped = state.outbox.owedDropped; state.metrics.cosmeticsResent = state.outbox.resent;
        if (full) state.metrics.snapshots++;
      } catch (error) { faults.push({ client, error }); }
    }
    return { shared, ownPackets, inputAcks, edgeResults, reliableEvents, diagnostics, faults };
  }
  // Clients started and not heard from (no input, acknowledgement or edge
  // acknowledgement) for at least `seconds`: sessions the server can evict.
  // A suspended client is not expected to send and is never listed.
  function staleClients(seconds = staleAfterMs / 1000) {
    const at = now(), stale = [];
    for (const [client, state] of clients) if (state.started && at - state.heardAt >= seconds * 1000) stale.push(client);
    return stale;
  }
  function inspect(id) {
    const state = clients.get(id);
    if (!state) return undefined;
    return { profile: 'snapshot-delta-v1', interval, baselineTick: state.baselineTick, ringNewest: ring.newestTick, encodes: encodeCache.parts.size, stateBytes: budgetUsed(state, now()), stateBytesPerSecond: budget, heardMsAgo: now() - state.heardAt, ...state.metrics };
  }
  return {
    get registry() { return registry; },
    descriptor() { ensureRuntime(); return entityRegistryDescriptor(registry); },
    addClient, removeClient, setStarted, setReady,
    resync(id) { const s = clients.get(id); if (s) { s.acked.length = 0; s.baselineTick = 0; s.outbox.resend(); } },
    has: id => clients.has(id),
    inputDiagnostics: id => clients.get(id)?.inputDiagnostics,
    input, edgeAck,
    // Adapter ABI 3 send-outcome hooks; the snapshot host needs none of
    // them. They stay as no-ops until the server moves to ABI 4.
    ack() {}, admitted() {}, dropped() {},
    advanceEdges, consumedEdges, publish, inspect, staleClients,
    dispose() { presentationDispose?.(); combatDispose?.(); shots?.dispose(); presentationDispose = combatDispose = null; },
  };
}
