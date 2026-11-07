import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { putWireStoreSetup, WireStoreSetupLayout as L, WireWorldFactsLayout as F, WirePublicationLinkLayout as P } from '../src/sim/gen/wire-store.js';
import { WireDynamicFactLayout as D, WireJournalFactLayout as J, WirePublicationOutputMirror, syncWirePublicationOutput } from '../src/sim/gen/wire-publish.js';
import { BlockHeaderLayout as H } from '../src/sim/gen/boundary.js';

// Raw descriptor corruption must fail before a generated decoder can use its
// count as a Rust slice length. None of these cold failures may trap the module.
test('canonical capture rejects malformed counts and offsets before decoding', async () => {
  const b = bridgeFor(await loadSim()), x = b.x;
  const setup = { unbuilt: 'unbuilt', built: 'built', destroyed: 'destroyed', classes: [], weaponNames: [], posts: [], peers: [], weapons: [], items: [], remotes: [], ordnance: [],
    itemCapacity: 0, remoteCapacity: 0, entityCapacity: 0, recordCapacity: 0, seatCapacity: 0 };
  const input = { words: x.wire_store_source_words(), put: putWireStoreSetup };
  b.callArgs(input, setup); // Intern the three cold names before raw writes.
  const write = () => {
    const ptr = x.sim_call_room(L.words, 0);
    b.callArgs(input, setup); b.views();
    return ptr / 8 + H.words;
  };
  for (const field of [L.classes, L.weaponNames, L.posts, L.peers, L.weapons, L.items, L.remotes, L.ordnance]) for (const value of [NaN, Infinity, -1, 0xffffffff, 0.5]) {
    const at = write(); b.f[at + field + 1] = value;
    assert.equal(x.wire_store_source_create(), 0, `count field ${field} value ${value}`);
    const offset = write(); b.f[offset + field] = value;
    assert.equal(x.wire_store_source_create(), 0, `offset field ${field} value ${value}`);
  }
  for (const field of [L.itemCapacity, L.remoteCapacity, L.entityCapacity, L.recordCapacity, L.seatCapacity]) {
    const at = write(); b.f[at + field] = 0xffffffff;
    assert.equal(x.wire_store_source_create(), 0, `capacity field ${field}`);
  }
  // A failed admission leaves the exchange and subsequent owners usable.
  b.callArgs(input, setup);
  const handle = x.wire_store_source_create(); assert.notEqual(handle, 0);
  const stagger = x.wire_stagger_create(), ptr = x.wire_store_world_input(handle) >>> 0;
  const runtime = x.wire_create(), host = x.wire_host_create(1, 500000, 1000, 1), shots = x.wire_shots_create(256), combat = x.wire_combat_create();
  const publication = x.wire_publish_create(runtime, handle, stagger, shots, host, combat);
  assert.notEqual(publication, 0);
  const output = { Mirror: WirePublicationOutputMirror, sync: syncWirePublicationOutput };
  const rejected = () => b.callResult(output, x.wire_publish(publication)).faults.length;
  const reset = () => {
    b.views(); const at = ptr / 8;
    b.f.fill(0, at, at + F.words + P.words);
    b.f[at + F.tick] = 1; return at;
  };
  for (const field of [F.tick, F.links, F.linksLength]) for (const value of [NaN, Infinity, -Infinity, -1, 0.5, 1e50]) {
    if (field === F.tick && value === -1) continue;
    const at = reset(); b.f[at + field] = value;
    assert.notEqual(rejected(), 0, `world descriptor ${field} value ${value}`);
  }
  for (const field of [P.id, P.hasKind, P.relay, P.hasFamily, P.familySix]) for (const value of [NaN, Infinity, -1, 0.5, 1e50]) {
    const at = reset(); b.f[at + F.linksLength] = 1;
    b.f[at + F.words + field] = value;
    assert.notEqual(rejected(), 0, `link descriptor ${field} value ${value}`);
  }
  // Exceptional batches can request the full protocol window without making
  // the ordinary battle reserve it. Invalid requests retain the old block.
  assert.equal(x.wire_store_world_room(handle, 0xffffffff), 0);
  assert.equal(x.wire_store_journal_room(handle, 0xffffffff), 0);
  const grown = x.wire_store_world_room(handle, F.words + 256 * P.words + 4096 * (J.words + D.words)) >>> 0;
  assert.notEqual(grown, 0); b.views(); assert.equal(b.f[grown / 8 + F.tick], 1);
  const bytes = x.wire_store_journal_room(handle, 4096 * 900) >>> 0;
  assert.notEqual(bytes, 0); b.views(); assert.equal(b.u8[bytes + 4096 * 900 - 1], 0);
  x.wire_publish_drop(publication); x.wire_host_drop(host); x.wire_drop(runtime); x.wire_combat_drop(combat); x.wire_shots_drop(shots);
  x.wire_stagger_drop(stagger); x.wire_store_source_drop(handle);
});
