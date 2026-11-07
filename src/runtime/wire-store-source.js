// EntityWorldCapture binds immutable protocol identities at attachment and
// cold client admission. capture() publishes no JS entity records; probe()
// materializes owned values only for the byte corpus.
import { WIRE_FACT_BATCH_RESERVE, WIRE_FACT_BYTES_RESERVE, WireStoreRecordsMirror, WireDynamicFactMirror, WireDynamicFactLayout, putWireDynamicFact, WireJournalFactMirror, WireJournalFactLayout, putWireJournalFact, WireSpanLayout, WirePublicationLinkMirror, WireWorldFactsLayout, WirePublicationLinkLayout, putWirePublicationLink, putWireStoreSetup, syncWireStoreRecords } from '../sim/gen/index.js';
import { combatSource } from '../combat-sources.js';

export function createStoreSource(game, registry, shots, { maxEntities = 256, world = null } = {}) {
  const bridge = game.simBridge, x = bridge.x;
  if (!shots) throw Error('Canonical capture requires native shot counters');
  const input = { words: x.wire_store_source_words(), put: putWireStoreSetup };
  const actors = [], links = [], peerIds = new WeakMap(); let nextPeer = 1;
  const inputCodes = new Map();
  for (let i = 0; i < registry.inputCodes.length; i++) inputCodes.set(registry.inputCodes[i], i);
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
      actors.push(actor); links.push(link); peers.push({ id, character: owner?.handle ?? null });
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
  let factsAt = (x.wire_store_world_input(handle) >>> 0) / 8, journalRows = WIRE_FACT_BATCH_RESERVE, dynamicRows = WIRE_FACT_BATCH_RESERVE;
  let journalAt = x.wire_store_journal_bytes(handle) >>> 0, journalCapacity = WIRE_FACT_BYTES_RESERVE;
  const journalRow = new WireJournalFactMirror(), dynamicRow = new WireDynamicFactMirror();
  return {
    bridge,
    epoch: registry.contentEpoch,
    peerId(actor) { return peerIds.get(actor) ?? 0; },
    writeWorldFacts(tick, { at: clock = 0, unixMs = 0, droppedTicks = 0, journal = null } = {}) {
      if (!handle) throw Error('Canonical capture disposed');
      // The ordinary journal fits the measured battle envelope. Exceptional
      // paused batches grow once before any input view is borrowed or written.
      const pendingRows = Math.min(journal?.pending.size ?? 0, 4096), pendingDynamics = Math.min(journal?.dynamic?.pending.size ?? 0, 4096);
      if (pendingRows > journalRows || pendingDynamics > dynamicRows) {
        const rows = Math.min(4096, Math.max(journalRows, pendingRows) * 2), dynamics = Math.min(4096, Math.max(dynamicRows, pendingDynamics) * 2);
        const words = WireWorldFactsLayout.words + 256 * WirePublicationLinkLayout.words + rows * WireJournalFactLayout.words + dynamics * WireDynamicFactLayout.words;
        const pointer = x.wire_store_world_room(handle, words) >>> 0;
        if (!pointer) { bridge.views(); bridge.f[factsAt + WireWorldFactsLayout.journalLength] = NaN; return; }
        factsAt = pointer / 8; journalRows = rows; dynamicRows = dynamics;
      }
      const requiredBytes = pendingRows * 900;
      if (requiredBytes > journalCapacity) {
        const capacity = Math.min(4096 * 900, Math.max(requiredBytes, journalCapacity * 2));
        const pointer = x.wire_store_journal_room(handle, capacity) >>> 0;
        if (!pointer) { bridge.views(); bridge.f[factsAt + WireWorldFactsLayout.journalLength] = NaN; return; }
        journalAt = pointer; journalCapacity = capacity;
      }
      bridge.views(); const f = bridge.f;
      f[factsAt + WireWorldFactsLayout.tick] = tick;
      f[factsAt + WireWorldFactsLayout.at] = clock; f[factsAt + WireWorldFactsLayout.unixMs] = unixMs; f[factsAt + WireWorldFactsLayout.droppedTicks] = droppedTicks;
      f[factsAt + WireWorldFactsLayout.links] = 0;
      f[factsAt + WireWorldFactsLayout.linksLength] = links.length;
      let at = factsAt + WireWorldFactsLayout.words;
      for (let i = 0; i < actors.length; i++) {
        const actor = actors[i], row = links[i], path = actor.link?.path;
        row.ack = actor.mailbox.ack; row.ackTick = actor.mailbox.ackTick;
        row.discardedInput = actor.mailbox.discarded ?? 0; row.discardedActions = actor.actions?.discarded ?? 0;
        row.rtt = typeof actor.link?.rtt === 'number' ? actor.link.rtt : NaN;
        row.hasKind = !!path?.kind; row.relay = path?.kind === 'relay'; row.hasFamily = !!path?.family;
        row.familySix = row.hasFamily && String(path.family).includes('6');
        const own = row.own, context = actor.mailbox.view, view = own.view, fs = context.flightSettings, pad = context.gamepad;
        own.held.fill(0); own.heldValid = true;
        const down = actor.mailbox.down;
        for (let j = 0; j < down.length; j++) {
          const bit = inputCodes.get(down[j]);
          if (bit === undefined || bit >= 128) own.heldValid = false;
          else own.held[bit >> 3] |= 1 << (bit & 7);
        }
        view.flags = (context.invertMouse ? 1 : 0) | (context.firstPerson ? 2 : 0) | (fs ? 4 : 0) | (fs?.invert ? 8 : 0) | (pad ? 16 : 0)
          | (context.mouseScale !== undefined ? 32 : 0) | (context.aspect !== undefined ? 64 : 0) | (context.viewTick !== undefined ? 128 : 0)
          | (fs?.sensitivity !== undefined ? 256 : 0) | (fs?.curve !== undefined ? 512 : 0) | (fs?.bindings !== undefined ? 1024 : 0);
        view.mouseScale = typeof context.mouseScale === 'number' ? context.mouseScale : NaN;
        view.aspect = typeof context.aspect === 'number' ? context.aspect : NaN;
        view.viewTick = typeof context.viewTick === 'number' ? context.viewTick : NaN;
        view.sensitivity = typeof fs?.sensitivity === 'number' ? fs.sensitivity : NaN;
        view.curve = typeof fs?.curve === 'number' ? fs.curve : NaN;
        view.profile = 0; view.profileFound = true;
        for (const key in fs?.bindings) if (Object.prototype.propertyIsEnumerable.call(fs.bindings, key)) { view.profileFound = false; break; }
        view.buttons = 0;
        for (let j = 0; j < 4; j++) view.axes[j] = typeof (pad?.axes?.[j] ?? 0) === 'number' ? (pad?.axes?.[j] ?? 0) : NaN;
        for (let j = 0; j < 16; j++) if (pad?.buttons?.[j]?.pressed) view.buttons |= 1 << j;
        own.discarded = actor.mailbox.discarded || 0;
        // F5's mailbox owner will retain camera aspect beside decoded view
        // state. The recon pose, camera orientation and weapons are native.
        own.reconAspect = actor.unit?.combat?.remotes?.aimCamera?.aspect ?? 1;
        own.combatFirstPerson = !!actor.unit?.combat?.firstPerson;
        const spawn = own.spawn, pending = actor.spawn.pending, auto = actor.spawn.auto;
        spawn.pending = !!pending; spawn.auto = !!auto;
        spawn.pendingClass = pending ? registry.classIds.get(pending.className) : 0;
        spawn.pendingPost = pending?.post == null ? 0 : registry.postIds.get(pending.post);
        spawn.autoTeam = auto ? auto.team : 0;
        spawn.autoClass = auto ? registry.classIds.get(auto.className) : 0;
        spawn.autoPost = auto?.post == null ? 0 : registry.postIds.get(auto.post);
        spawn.valid = spawn.pendingClass !== undefined && spawn.pendingPost !== undefined && spawn.autoClass !== undefined && spawn.autoPost !== undefined;
        putWirePublicationLink(bridge, row, f, at, bridge.heap); at += WirePublicationLinkLayout.words;
      }
      const start = 256 * WirePublicationLinkLayout.words;
      let count = 0, byteCount = 0;
      if (journal) for (const { record, view } of journal.pending.values()) {
        if (count >= 4096 || record.payload.length > 900) continue;
        journalRow.id = record.id; journalRow.generation = record.generation; journalRow.tick = record.tick; journalRow.schema = record.schema;
        journalRow.payload.offset = byteCount; journalRow.payload.length = record.payload.length;
        journalRow.hasPosition = !!view.position;
        journalRow.position.set(view.position?.[0] ?? 0, view.position?.[1] ?? 0, view.position?.[2] ?? 0);
        journalRow.ownerTag = view.ownerId === undefined ? 0 : view.ownerId === null ? 1 : 2; journalRow.owner = typeof view.ownerId === 'number' ? view.ownerId : NaN;
        journalRow.speed = typeof view.speed === 'number' ? view.speed : NaN; journalRow.life = typeof view.life === 'number' ? view.life : NaN;
        putWireJournalFact(bridge, journalRow, f, factsAt + WireWorldFactsLayout.words + start + count * WireJournalFactLayout.words, bridge.heap);
        bridge.u8.set(record.payload, journalAt + byteCount); byteCount += record.payload.length; count++;
      }
      f[factsAt + WireWorldFactsLayout.journal] = start; f[factsAt + WireWorldFactsLayout.journalLength] = count;
      f[factsAt + WireWorldFactsLayout.journalBytes + WireSpanLayout.offset] = 0;
      f[factsAt + WireWorldFactsLayout.journalBytes + WireSpanLayout.length] = byteCount;
      // Identity callbacks publish changed attribution once. Rust retains the
      // live table and reads every projectile body from its canonical pool.
      const dynamicStart = start + journalRows * WireJournalFactLayout.words;
      let dynamicCount = 0;
      if (journal?.dynamic) for (const entry of journal.dynamic.pending.values()) {
        if (dynamicCount >= 4096) break;
        dynamicRow.slot = entry.slot; dynamicRow.refGeneration = entry.refGeneration; dynamicRow.type = entry.nativeType;
        dynamicRow.id = entry.id; dynamicRow.generation = entry.generation; dynamicRow.launchTick = entry.launchTick; dynamicRow.definitionId = entry.definitionId;
        dynamicRow.ownerId = entry.ownerId; dynamicRow.ownerGeneration = entry.ownerGeneration; dynamicRow.fireId = entry.fireId; dynamicRow.channel = entry.channel; dynamicRow.terminal = !!entry.terminal;
        putWireDynamicFact(bridge, dynamicRow, f, factsAt + WireWorldFactsLayout.words + dynamicStart + dynamicCount * WireDynamicFactLayout.words, bridge.heap); dynamicCount++;
      }
      f[factsAt + WireWorldFactsLayout.dynamics] = dynamicStart; f[factsAt + WireWorldFactsLayout.dynamicsLength] = dynamicCount;
      if (bridge.boundaryCounters) {
        bridge.boundaryCounters.inputWords += WireWorldFactsLayout.words + links.length * WirePublicationLinkLayout.words + count * WireJournalFactLayout.words + dynamicCount * WireDynamicFactLayout.words;
        bridge.boundaryCounters.inputBytes = (bridge.boundaryCounters.inputBytes ?? 0) + byteCount;
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
