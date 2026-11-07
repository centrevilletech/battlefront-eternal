// A recorded map's inputs run through the real addon step boundary. Expected
// fingerprints stay in this comparator and never enter a Context or its owner.
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {readFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {WorldCorpus} from '../../../tools/world-corpus.mjs';
import {WorldTickInputLayout as I,WorldTickOutputLayout as O,WorldTickSetupLayout as S} from '../../../src/sim/gen/world.js';

const [directory, reference] = process.argv.slice(2);
if (!directory || !reference) throw Error('Usage: world-smoke.mjs CORPUS REFERENCE');
const addon = createRequire(import.meta.url)('../../../.host-addon/bfe-node.node');
const hello = addon.handshake();
assert.equal(hello.nativeWorld, true); assert.equal(hello.nativePublication, true);
const corpus = new WorldCorpus({directory, replay: true});
const expected = readFileSync(reference, 'utf8').trim().split('\n');
const assets = Object.keys(corpus.assets).map(url => {
  const bytes = corpus.readAsset(url, () => {throw Error('Replay cannot read installed content');});
  return {url, hex: bytes === null ? null : bytes.toString('hex')};
});
const admission = JSON.stringify({kind: 'admit', setup: corpus.setup, assets});
const hash = value => createHash('sha1').update(value).digest('hex').slice(0, 16);
const before = addon.lifecycleCounts(), host = new addon.NativeHost();
let owner, battle, observed = 0, ticks = 0;
try {
  for (const frame of corpus.frames) {
    if (battle !== frame.battle) {
      owner?.close(); owner = host.create(); battle = frame.battle;
      const setup = new Float64Array(S.words);
      assert.equal(owner.probeWorldAdmit(admission, setup), S.words);
      owner.stepInit(setup);
    }
    // Context's replay adapters receive the same frame envelope as world-twin,
    // including hexadecimal row bits. Production step still takes raw arenas.
    const rows = frame.world.map(encoded => {
      const bytes = Buffer.from(encoded, 'base64'), words = [];
      for (let at = 0; at < bytes.length; at += 8) words.push(bytes.readBigUInt64LE(at).toString(16).padStart(16, '0'));
      return words;
    });
    const source = JSON.stringify({kind: 'frame', ...frame, world: rows});
    owner.probeWorldPrepare(source);
    const outputs = new Float64Array(frame.world.length * O.words);
    for (let i = 0; i < frame.world.length; i++) {
      const encoded = Buffer.from(frame.world[i], 'base64');
      assert.equal(encoded.length, I.words * 8);
      const input = new Float64Array(I.words), out = new Float64Array(O.words);
      new Uint8Array(input.buffer).set(encoded);
      assert.equal(owner.step(input, out), O.words);
      assert.equal(out[O.accepted], 1, `recorded tick ${frame.index} refused`);
      outputs.set(out, i * O.words); ticks++;
    }
    const observation = JSON.parse(owner.probeWorldObserve(source, outputs));
    const packets = createHash('sha1');
    for (const packet of observation.packets) {
      packets.update(packet.channel);
      packets.update(Object.hasOwn(packet, 'bytes') ? Buffer.from(packet.bytes, 'base64') : packet.text);
    }
    const actual = `${observation.prefix || ''}${observation.tick} ${hash(JSON.stringify(JSON.parse(observation.stateJson)))} ${hash(JSON.stringify(JSON.parse(observation.digestJson)))} ${packets.digest('hex').slice(0, 16)}`;
    assert.equal(actual, expected[observed], `first differing observation ${observed + 1}`);
    observed++;
  }
  assert.equal(observed, expected.length);
} finally {
  owner?.close(); assert.equal(owner?.close(), false);
  assert.equal(host.liveOwners(), 0);
  const after = addon.lifecycleCounts();
  assert.equal(after[0] - before[0], after[1] - before[1]);
}
console.log(JSON.stringify({observations: observed, acceptedTicks: ticks, exact: true}));
