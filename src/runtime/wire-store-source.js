// EntityWorldCapture binds immutable protocol identities at attachment and
// cold client admission. capture() publishes no JS entity records; probe()
// materializes owned values only for the byte corpus.
import { WireStoreRecordsMirror, WireWorldFactsLayout, WirePublicationLinkMirror, WirePublicationLinkLayout, putWirePublicationLink, putWireStoreSetup, syncWireStoreRecords } from '../sim/gen/index.js';
import { bindSpawnSelection } from './input-mailbox.js';
import { combatSource } from '../combat-sources.js';

export function createStoreSource(game, registry, shots, { maxEntities = 256, world = null } = {}) {
  const bridge = game.simBridge, x = bridge.x;
  if (!shots) throw Error('Canonical capture requires native shot counters');
  const input = { words: x.wire_store_source_words(), put: putWireStoreSetup };
  const actors = [], links = [], peerIds = new WeakMap(); let nextPeer = 1;
  function content() {
    const classes = [];
    for (const [id, name] of registry.classes) classes.push({ id, name });
    const weaponNames = [], posts = [];
    for (const [id, name] of registry.weapons) weaponNames.push({ id, name });
    for (const [id, name] of registry.posts) posts.push({ id, name });
    const peers = []; actors.length = 0; links.length = 0;
    if (world) for (const actor of world.players.values()) {
      if (!actor.character) continue;
      const owner = actor.spectator ? null : game.unitsSim.tailOwner.entries.get(actor.character);
      if (!actor.spectator && !owner) throw Error('Canonical roster identity missing');
      let id = peerIds.get(actor); if (id === undefined) { id = nextPeer++; peerIds.set(actor, id); }
      const link = new WirePublicationLinkMirror(); link.id = id;
      actors.push(actor); links.push(link); peers.push({ id, character: owner?.handle ?? null, mailbox: actor.mailbox.handle });
      bindSpawnSelection(actor, registry);
    }
    if (links.length > 256 || nextPeer > 0xffffffff) throw Error('Canonical connection capacity exceeded');
    const weapons = [];
    for (let definition = 0; definition < bridge.defs.weapon.list.length; definition++) {
      const def = bridge.defs.weapon.list[definition], id = registry.weaponIds.get(def.class);
      if (id !== undefined) weapons.push({ definition, id });
    }
    const items = [];
    const combatIds = new WeakMap();
    for (const [id, definition] of registry.combat.definitions) combatIds.set(definition, id);
    for (let definition = 0; definition < bridge.defs.item.list.length; definition++) {
      const def = bridge.defs.item.list[definition], id = combatIds.get(def) ?? combatIds.get(combatSource(def));
      // A URL may load a side the mission never fields. Do not give its cold
      // definitions protocol ids; an actual sample of one must still reject.
      if (id !== undefined) items.push({ definition, id });
    }
    const remotes = [];
    for (let definition = 0; definition < game.unitsSim.recon.classes.length; definition++) {
      const def = game.unitsSim.recon.classes[definition], id = combatIds.get(def) ?? combatIds.get(combatSource(def));
      if (id !== undefined) remotes.push({ definition, id });
    }
    const ordnance = [];
    for (let definition = 0; definition < bridge.defs.ordnance.list.length; definition++) {
      const def = bridge.defs.ordnance.list[definition], id = combatIds.get(def) ?? combatIds.get(combatSource(def));
      if (id !== undefined) ordnance.push({ definition, id });
    }
    let seats = 0;
    for (const value of Object.values(game.data.vehicles?.classes ?? {})) seats = Math.max(seats, value.seats?.length ?? 0);
    const pools = game.items?.pools;
    bridge.callArgs(input, {
      unbuilt: 'unbuilt', built: 'built', destroyed: 'destroyed', classes, weaponNames, posts, peers, weapons, items, remotes, ordnance,
      itemCapacity: pools ? (pools.mine ?? 50) + (pools.powerupitem ?? 25) : 0,
      remoteCapacity: 4096, entityCapacity: maxEntities, recordCapacity: 512, seatCapacity: seats,
    });
  }
  content();
  let handle = x.wire_store_source_create();
  if (!handle) throw Error('Invalid canonical capture setup');
  const output = { Mirror: WireStoreRecordsMirror, sync: syncWireStoreRecords };
  const factsAt = (x.wire_store_world_input(handle) >>> 0) / 8;
  return {
    bridge,
    epoch: registry.contentEpoch,
    peerId(actor) { return peerIds.get(actor) ?? 0; },
    writeWorldFacts(tick, { at: clock = 0, unixMs = 0, droppedTicks = 0 } = {}) {
      if (!handle) throw Error('Canonical capture disposed');
      bridge.views(); const f = bridge.f;
      f[factsAt + WireWorldFactsLayout.tick] = tick;
      f[factsAt + WireWorldFactsLayout.at] = clock; f[factsAt + WireWorldFactsLayout.unixMs] = unixMs; f[factsAt + WireWorldFactsLayout.droppedTicks] = droppedTicks;
      f[factsAt + WireWorldFactsLayout.links] = 0;
      f[factsAt + WireWorldFactsLayout.linksLength] = links.length;
      let at = factsAt + WireWorldFactsLayout.words;
      for (let i = 0; i < actors.length; i++) {
        const actor = actors[i], row = links[i], path = actor.link?.path;
        row.discardedActions = actor.actions?.discarded ?? 0;
        row.rtt = typeof actor.link?.rtt === 'number' ? actor.link.rtt : NaN;
        row.hasKind = !!path?.kind; row.relay = path?.kind === 'relay'; row.hasFamily = !!path?.family;
        row.familySix = row.hasFamily && String(path.family).includes('6');
        putWirePublicationLink(bridge, row, f, at, bridge.heap); at += WirePublicationLinkLayout.words;
      }
      if (bridge.boundaryCounters) {
        bridge.boundaryCounters.inputWords += WireWorldFactsLayout.words + links.length * WirePublicationLinkLayout.words;
      }
    },
    get nativeHandle() { if (!handle) throw Error('Canonical capture disposed'); return handle; },
    refreshContent() {
      if (!handle) throw Error('Canonical capture disposed');
      content();
      if (!x.wire_store_source_configure(handle)) throw Error('Invalid canonical content rebind');
    },
    probe() {
      if (!handle) throw Error('Canonical capture disposed');
      const result = bridge.callResult(output, x.wire_store_source_probe(handle, shots));
      if (result.error !== null) throw Error(result.error);
      return result;
    },
    dispose() { if (handle) { x.wire_store_source_drop(handle); handle = 0; } },
  };
}
