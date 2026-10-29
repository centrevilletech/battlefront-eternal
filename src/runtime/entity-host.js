import {entityPostNames} from './entity-spawn-posts.js';
import {attachPresentationCapture} from './entity-presentation-events.js';
import {captureUnitState} from './entity-unit-state.js';
import {captureWorldPresentation} from './entity-world-state.js';
// The host half of the entity protocol: everything one authoritative
// world owes each connected client - scheduling, budgets, edges, combat
// retention, own corrections, input acknowledgement - with no transport
// in it. The dedicated server's world worker used to open-code all of it
// in its publish loop; it lives in the engine now so a local server (the
// single-client path) and the dedicated server run the same host. The
// caller owns simulation stepping and message transport: it feeds input
// bytes in, calls publish() each tick, and sends what comes back.
import { EntityScheduler } from './network-entity-stream.js';
import { prepareEntityRecord } from './network-entity-codec.js';
import { EntityWorldCapture, capturePlayerCorrection } from './network-entity-capture.js';
import { createEntityRegistry, entityRegistryDescriptor } from './network-entity-registry.js';
import { captureWorldSummaries } from './network-entity-summary.js';
import { CombatCapture, attachCombatCapture, decodeCombat } from './network-entity-combat.js';
import { EntityShotCounters } from './network-entity-shots.js';
import { INPUT_CODES, DEFAULT_BINDING_PROFILES, decodeEntityInput } from './network-entity-input.js';
import { EntityBudget, readStateBudget } from './network-entity-budget.js';
import { entityImportance, entityUpdateInterval } from './network-entity-importance.js';
import { EntityEdges } from './network-entity-edge-host.js';
import { EntityEventStore } from './network-entity-event-store.js';

export function createEntityHost({ game, world, stateBytesPerSecond, epoch = 1, maxPacketsPerPublication = 10, local = false } = {}) {
  if (!game || !world) throw Error('entity host requires a game and a multiplayer world');
  if (local) maxPacketsPerPublication = 128;
  const stateBudget = readStateBudget({ stateBytesPerSecond });
  const combatStore = new EntityEventStore();
  const clients = new Map();
  let registry, capture, shots, combatCapture, combatDispose, presentationDispose;

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
    if (clients.has(id)) throw Error('entity client already joined');
    clients.set(id, {
      started: false, publishPhase: clients.size % 3,
      scheduler: new EntityScheduler({ epoch, maxEntities: 1024, maxPackets: 512 }), ready: false,
      budget: new EntityBudget(stateBudget),
      metrics: { ownOffered: 0, remoteOffered: 0, admitted: 0, budgetDropped: 0, lastOwnTick: null },
      edges: new EntityEdges({ actor: world.players.get(id), world, client: id, registry }),
      combatReady: false,
      inputDiagnostics: { accepted: 0, rejected: 0, reasons: {}, last: {} },
    });
  }
  function removeClient(id) { clients.delete(id); }
  function setStarted(id, started) { const state = clients.get(id); if (state) state.started = Boolean(started); }
  function setReady(id) { const state = clients.get(id); if (state) state.ready = true; }

  function input(id, bytes) {
    const state = clients.get(id);
    if (!state || !state.started) return;
    const packet = decodeEntityInput(new Uint8Array(bytes), registry);
    if (packet.epoch !== epoch || packet.generation !== world.players.get(id)?.unit?.generation) return;
    state.edges.receive(packet, game.worldClock.tick);
    for (const command of packet.commands) {
      const accepted = world.input(id, command), stats = state.inputDiagnostics;
      stats[accepted ? 'accepted' : 'rejected']++;
      stats.last = { sequence: command.sequence, tick: command.tick, generation: command.generation, epoch: command.epoch, serverTick: game.worldClock.tick, accepted, lookTotal: command.lookTotal, down: command.down.slice(0, 32) };
    }
    for (const packetId of packet.receipts) state.scheduler.ack({ epoch: packet.epoch, packetId });
  }
  function edgeAck(id, message) { clients.get(id)?.edges?.acknowledge(message); }
  function ack(id, value) { clients.get(id)?.scheduler?.ack(value); }
  function admitted(id, packetId, { ownCorrection = false, tick = null } = {}) {
    const state = clients.get(id);
    if (state?.scheduler?.admitted(packetId)) {
      state.metrics.admitted++;
      if (ownCorrection) state.metrics.lastOwnTick = tick;
    }
  }
  function dropped(id, packetId) {
    const state = clients.get(id);
    const offer = state?.scheduler.dropped(packetId);
    if (offer?.chargedBytes) state.budget.refund(offer.chargedBytes);
  }

  // Edge delivery around the simulation step, as the worker used to run
  // it: advance due edges into the mailboxes before the step, collect
  // their outcomes after it. Faults come back for the caller to report;
  // the faulting client is stopped either way.
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

  // One publication pass (call every tick). Each client takes its full
  // turn on its own tick in three and still gets a fresh own correction
  // every other tick between turns, so capture cost and datagram bursts
  // never land for every client on the same tick. Returns everything the
  // transport must send, in send order per list.
  function publish({ begun = performance.now(), droppedTicks = 0 } = {}) {
    const capturedAtUnixMs = Date.now();
    const packets = [], inputAcks = [], edgeResults = [], diagnostics = [], faults = [];
    let semanticRecords, semanticMotion, commonSummaries;
    let combatChanges = [];
    const combatFaults = combatCapture?.drainFaults() ?? [];
    if (combatCapture) try { combatChanges = combatStore.update(combatCapture.drain(), game.worldClock.tick, prepareEntityRecord, decodeCombat); } catch (error) { combatFaults.push(String(error.message).slice(0, 160)); }
    const eligibilityCache = new Map();
    for (const [client, state] of clients) if (state.started) {
      const actor0 = world.players.get(client);
      if (actor0) inputAcks.push({ client, epoch, serverTick: game.worldClock.tick, generation: actor0.unit?.generation ?? 0, ack: actor0.mailbox.ack, ackTick: actor0.mailbox.ackTick, dropped: droppedTicks, discardedInput: actor0.mailbox.discarded ?? 0, discardedActions: actor0.actions?.discarded ?? 0, ackActions: 0 });
      const feedback = state.edges.feedback();
      if (feedback.received.length || feedback.results.length) edgeResults.push({ client, ...feedback });
      if (!state.ready) continue;
      const fullTurn = local || (game.worldClock.tick + (state.publishPhase ?? 0)) % 3 === 0;
      try {
        const actor = actor0;
        const vehicle = actor.unit?.seat?.vehicle, soldier = actor.unit?.soldier;
        const position = (vehicle?.position || actor.unit?.position)?.toArray(), velocity = (vehicle?.velocity || soldier?.velocity)?.toArray();
        let forward;
        if (vehicle?.quaternion) { const { x, y, z, w } = vehicle.quaternion; forward = [2 * (x * z + w * y), 2 * (y * z - w * x), 1 - 2 * (x * x + y * y)]; }
        else if (soldier) { const yaw = soldier.aimYaw, pitch = soldier.aimPitch, c = Math.cos(pitch); forward = [Math.sin(yaw) * c, Math.sin(pitch), Math.cos(yaw) * c]; }
        const interest = { position, velocity, forward, flying: !!vehicle?.flying };
        // Drain changes into every recipient before staggering packet publication.
        // Otherwise the two intervening ticks disappear for this client. The
        // existing bounded scheduler retains and retries each event until ACK.
        for (const event of state.combatReady ? combatChanges : combatStore.records.values()) {
          const priority = [event.view.ownerId, event.view.targetId, event.view.attackerId].includes(actor.unit?.id) ? 24 : entityImportance(event.view, interest);
          state.scheduler.updateEvent({ ...event, record: event.prepared }, priority, local ? Infinity : priority === 24 ? 1 : entityUpdateInterval(event.view, interest));
        }
        state.combatReady = true;
        if (fullTurn || game.worldClock.tick % 2 === 0) {
          if (fullTurn) {
            if (!semanticRecords) {
              const tick = game.worldClock.tick;
              for(const [slot,c]of game.battle.characters.entries())c.rosterSlot=slot;
              semanticRecords = [...capture.capture(tick),
                ...[...game.units.all()].filter(u=>u.soldier&&u.combat&&!u.removed).map(u=>captureUnitState(u,registry,tick)),
                ...captureWorldPresentation(game,tick,epoch)].map(prepareEntityRecord);
              semanticMotion = new Map(semanticRecords.filter(p=>p.record.position).map(p=>[p.record.id,p.record]));
            }
            commonSummaries ??= captureWorldSummaries(game, world, registry, { tick: game.worldClock.tick, epoch }).map(prepareEntityRecord);
            if (game.worldClock.tick % 60 === 0) diagnostics.push({ client, actors: capture.unsupported.slice(0, 256).map(e => ({ id: e.entityId, message: String(e.message).slice(0, 160) })) });
            for (const prepared of semanticRecords) {
              const record = prepared.record;
              if (!actor.spectator && record.kind === 'soldier' && record.id === actor.unit?.id) continue;
              const motion=record.kind==='summary'&&record.schema===6?semanticMotion.get(record.id-100000):record;
              const priority=local?1:entityImportance(motion||record,interest);
              const interval=local?Infinity:entityUpdateInterval(motion||record,interest);
              state.scheduler.update(prepared, priority, false, record.kind==='summary'?Math.max(6,interval):interval);
            }
            for (const prepared of commonSummaries) state.scheduler.update(prepared, 4, false, local ? Infinity : 30);
            for (const record of captureWorldSummaries(game, world, registry, { tick: game.worldClock.tick, epoch, playerId: client, includeCommon: false, eligibilityCache })) state.scheduler.update(record, 4, false, local ? Infinity : 6);
            if (combatFaults.length) diagnostics.push({ client, actors: combatFaults.map(message => ({ id: 0, message: String(message).slice(0, 160) })) });
          }
          const own = actor.spectator ? undefined : capturePlayerCorrection(game, world, client, { registry, bindingProfileId: 0, epoch });
          const fields = { tick: game.worldClock.tick, ackInput: actor.mailbox.ack, ackActions: 0, ackFire: 0 };
          const offer = (packet, ownCorrection, force = false) => {
            if (!packet) return false;
            if (!local && !force && !state.budget.take(packet.buffer.byteLength, begun)) { state.scheduler.dropped(packet.packetId); state.metrics.budgetDropped++; return false; }
            state.scheduler.offered.get(packet.packetId).chargedBytes = local || force ? 0 : packet.buffer.byteLength;
            state.metrics[ownCorrection ? 'ownOffered' : 'remoteOffered']++;
            const buffer = packet.buffer.buffer.slice(packet.buffer.byteOffset, packet.buffer.byteOffset + packet.buffer.byteLength);
            packets.push({ client, epoch, packetId: packet.packetId, ownCorrection, capturedAtUnixMs, pacingBytesPerSecond: state.budget.rate, buffer });
            return true;
          };
          // The first packet is clamped to the budget left, like the fill
          // packets below; if the correction alone does not fit what is
          // left it still goes (force): prediction resets from it.
          let packet = null, ownForced = false;
          try { packet = state.scheduler.packet({ ...fields, own, maxBytes: Math.max(30, Math.min(1100, (local ? 1100 : state.budget.available(begun)))) }); } catch { packet = null; }
          if (!packet && own) { packet = state.scheduler.packet({ ...fields, own }); ownForced = true; }
          if (offer(packet, !!own, ownForced) && fullTurn) {
            const offered = [packet.packetId];
            for (let n = 1; n < maxPacketsPerPublication; n++) {
              const maxBytes = Math.min(1100, (local ? 1100 : state.budget.available(begun)));
              if (maxBytes <= 30) break;
              const next = state.scheduler.packet({ ...fields, maxBytes, excludePacketIds: offered });
              if (!next || !offer(next, false)) break;
              offered.push(next.packetId);
            }
          }
        }
      } catch (error) { faults.push({ client, error }); }
    }
    return { packets, inputAcks, edgeResults, diagnostics, faults };
  }

  function inspect(id) {
    const state = clients.get(id);
    if (!state) return undefined;
    return {
      profile: 'entity-v1', payloadBytesPerSecond: state.budget.rate, burstBytes: state.budget.burst,
      maxPacketsPerPublication, ownNominalHz: local ? 60 : 40, ...state.metrics,
      events: { ...state.scheduler.eventMetrics, retained: state.scheduler.events.size, bytes: state.scheduler.eventBytes, globalDropped: combatStore.dropped },
      offeredBytes: state.budget.offeredBytes,
      pendingOffers: state.scheduler.offered.size, pendingVersions: state.scheduler.pending?.size ?? 0,
      admissionAgeTicks: [...state.scheduler.records.values()].map(entry => ({ id: entry.record.id, kind: entry.record.kind, targetIntervalTicks: Number.isFinite(entry.interval) ? entry.interval : null, age: Number.isFinite(entry.last) ? game.worldClock.tick - entry.last : null })),
    };
  }

  return {
    get registry() { return registry; },
    descriptor() { ensureRuntime(); return entityRegistryDescriptor(registry); },
    addClient, removeClient, setStarted, setReady,
    resync(id) { const state = clients.get(id); if(state) { const previous=state.scheduler; state.scheduler = new EntityScheduler({epoch, maxEntities:1024, maxPackets:512}); for(const field of ['serial','offered','pending'])state.scheduler[field]=previous[field]; state.combatReady = false; } },
    has: id => clients.has(id),
    inputDiagnostics: id => clients.get(id)?.inputDiagnostics,
    input, edgeAck, ack, admitted, dropped,
    advanceEdges, consumedEdges, publish, inspect,
    dispose() { presentationDispose?.(); combatDispose?.(); shots?.dispose(); presentationDispose = combatDispose = null; },
  };
}
