import { createWirePresentationBridge } from './wire-presentation.js';
import { createWireBridge } from './wire-sim.js';
import { createWirePublisher } from './wire-publish.js';
import { createWireHostBridge, createWireStaggerBridge } from './wire-host.js';
import { createWireRegistryBridge } from './wire-registry.js';
import { WireEntityCapture } from './wire-capture.js';
import { entityPostNames } from './entity-spawn-posts.js';
import { attachPresentationCapture } from './entity-presentation-events.js';
import { createEntityRegistry, entityRegistryDescriptor } from './network-entity-registry.js';
import { readStateBudget } from './network-entity-budget.js';
import { CombatCapture, attachCombatCapture } from './network-entity-combat.js';
import { EntityShotCounters } from './network-entity-shots.js';
import { INPUT_CODES, DEFAULT_BINDING_PROFILES, decodeEntityInput, isSnapshotAckKeepalive, decodeSnapshotAckKeepalive } from './network-entity-input.js';
import { EntityEdges } from './network-entity-edge-host.js';

// Rust owns snapshot publication (docs/snapshot-delta-design.md): capture,
// cadence, shared baseline choice, slots, retained own state, event queues,
// budgets and byte encoding. Transport copies each shared group once and fans
// it out before its clients' own packets. A second own packet is required
// when the first one's header announces it, including after packet loss.
const STALE_AFTER_MS = 5000;
export const MAX_LOCAL_INTERVAL = 3;
export const CLIENT_BASELINE_TICKS = 128;

export function publicationBuffersFor(publication, client) {
  const buffers = [];
  for (const group of publication.shared) if (group.clients.includes(client)) buffers.push(...group.parts);
  for (const packet of publication.ownPackets) if (packet.client === client) buffers.push(packet.buffer);
  return buffers;
}

export function createSnapshotHost({ game, world, epoch = 1, local = false, stateBytesPerSecond, staleAfterMs = local ? Infinity : STALE_AFTER_MS, now = () => performance.now() } = {}) {
  if (!game || !world) throw Error('snapshot host requires a game and a multiplayer world');
  if (!game.simBridge) throw Error('native snapshot host requires the shared simulation bridge');
  const budget = local ? Infinity : readStateBudget({ stateBytesPerSecond }).bytesPerSecond;
  if (!(staleAfterMs > 0)) throw Error('invalid stale client time');
  const native = createWireBridge(game.simBridge); native.start();
  const presentation = createWirePresentationBridge(game.simBridge);
  const policy = createWireHostBridge(game.simBridge, { local, budget, staleAfterMs, epoch });
  const stagger = createWireStaggerBridge(game.simBridge), clients = new Map();
  const history = game.ordnance?.hitHistory?.lease;
  let registry, capture, shots, combat, combatDispose, presentationDispose, publisher;
  const emptyPublication = Object.freeze({ shared: [], ownPackets: [], inputAcks: [], edgeResults: [], reliableEvents: [], diagnostics: [], faults: [] });

  // Protocol names and content handles bind once, then refresh only at cold
  // admission. No publication reads entity facades to resolve a class or post.
  function ensureRuntime() {
    if (registry) return;
    registry = createEntityRegistry(game.data, { contentEpoch: epoch, inputCodes: INPUT_CODES, bindingProfiles: DEFAULT_BINDING_PROFILES,
      posts: entityPostNames(game), native: createWireRegistryBridge(game.simBridge) });
    shots = new EntityShotCounters(game.simEvents, { bridge: game.simBridge });
    combat = new CombatCapture({ tick: () => game.worldClock.tick, definitionId: registry.combat.definitionId, generation: epoch, bridge: game.simBridge });
    combatDispose = attachCombatCapture(game, combat);
    presentationDispose = attachPresentationCapture(game, combat, registry, presentation);
    capture = new WireEntityCapture(game, registry, { bridge: game.simBridge, shots, world });
    native.bindStore(capture);
    publisher = createWirePublisher(game.simBridge, { runtime: native, source: capture, stagger, shots, host: policy, combat });
  }
  function addClient(id) {
    ensureRuntime();
    if (clients.has(id)) throw Error('snapshot client already joined');
    capture.refreshContent();
    const state = policy.add(id, now());
    try {
      state.edges = new EntityEdges({ actor: world.players.get(id), world, client: id, registry, bridge: game.simBridge });
      state.inputDiagnostics = { accepted: 0, rejected: 0, reasons: {}, last: {} };
      publisher.bind(id, state, world.players.get(id)); clients.set(id, state);
    } catch (error) { state.edges?.dispose(); policy.remove(id); throw error; }
  }
  function removeClient(id) {
    const state = clients.get(id); if (!state) return;
    publisher.remove(state); policy.remove(id); state.edges.dispose(); clients.delete(id);
  }
  function setStarted(id, started) { const state = clients.get(id); if (state) { state.started = !!started; if (state.started) state.heardAt = now(); } }
  function setReady(id) { const state = clients.get(id); if (state) state.ready = true; }
  // F5's current mailbox coordinator still consumes input and edge commands.
  // Its ACK/discarded/view facts enter the reserved publication block once;
  // body, arsenal, seat, recon and world records are never constructed here.
  function input(id, bytes) {
    const state = clients.get(id); if (!state) return;
    state.heardAt = now(); if (!state.started) return;
    const view = new Uint8Array(bytes);
    if (isSnapshotAckKeepalive(view)) { policy.acceptAck(state, decodeSnapshotAckKeepalive(view)); return; }
    const packet = decodeEntityInput(view, registry);
    if (packet.epoch !== epoch || packet.generation !== world.players.get(id)?.unit?.generation) return;
    policy.acceptAck(state, packet.snapshotAck); state.edges.receive(packet, game.worldClock.tick);
    for (const command of packet.commands) {
      const accepted = world.input(id, command), stats = state.inputDiagnostics;
      stats[accepted ? 'accepted' : 'rejected']++;
      stats.last = { sequence: command.sequence, tick: command.tick, generation: command.generation, epoch: command.epoch,
        serverTick: game.worldClock.tick, accepted, lookTotal: command.lookTotal, down: command.down.slice(0, 32) };
    }
  }
  function edgeAck(id, message) { const state = clients.get(id); if (state) { state.heardAt = now(); state.edges.acknowledge(message); } }
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
  function publish({ droppedTicks = 0, at = now(), unixMs = Date.now() } = {}) {
    if (!publisher) return emptyPublication;
    return publisher.publish({ tick: game.worldClock.tick, at, unixMs, droppedTicks });
  }
  function staleClients(seconds = staleAfterMs / 1000) { return policy.staleClients(now(), seconds); }
  function inspect(id) {
    const state = clients.get(id); if (!state) return undefined;
    return { profile: 'snapshot-delta-v1', wire: 'rust', interval: policy.interval, baselineTick: state.baselineTick,
      ringNewest: publisher.ringNewest, encodes: publisher.encodes, stateBytes: policy.budgetUsed(state, now()),
      stateBytesPerSecond: budget, heardMsAgo: now() - state.heardAt, ...state.metrics };
  }
  return {
    get registry() { return registry; }, descriptor() { ensureRuntime(); return entityRegistryDescriptor(registry); },
    addClient, removeClient, setStarted, setReady, has: id => clients.has(id), inputDiagnostics: id => clients.get(id)?.inputDiagnostics,
    input, edgeAck, advanceEdges, consumedEdges, publish, inspect, staleClients,
    setLocalInterval(ticks) { policy.setLocalInterval(ticks); },
    resync(id) { const state = clients.get(id); if (state) policy.resync(state); },
    // Adapter ABI 3 send-outcome hooks remain no-ops until the transport's
    // ABI 4 migration. Native cumulative ACKs already settle the event queues.
    ack() {}, admitted() {}, dropped() {},
    dispose() {
      for (const id of clients.keys()) removeClient(id);
      publisher?.dispose(); capture?.dispose(); combat?.dispose(); stagger.dispose(); policy.dispose(); presentation.dispose(); native.dispose();
      history?.dispose(); presentationDispose?.(); combatDispose?.(); shots?.dispose(); presentationDispose = combatDispose = null;
    },
  };
}
