// The optional HUD atlas index is content. Its absence keeps native art, but
// a present index that cannot be read stops loading with a renderer error.
import test from 'node:test';
import assert from 'node:assert/strict';
import { RasterHudAssets } from '../src/raster-hud.js';

const reply = (status, type, body) => async () => ({ ok: status >= 200 && status < 300, status, headers: { get: () => type }, json: async () => JSON.parse(body) });
const renderer = () => ({ resources: { failure: null }, getContext: () => { throw Error('no context'); } });

test('an absent HUD atlas index keeps native art; a broken one is a failure', async () => {
  const previous = globalThis.fetch;
  try {
    for (const [status, type] of [[404, 'text/plain'], [200, 'text/html; charset=utf-8']]) {
      globalThis.fetch = reply(status, type, '<!doctype html>');
      const r = renderer(), assets = new RasterHudAssets(r);
      assert.equal(await assets.ready, null, `status ${status} ${type} means no index`);
      assert.equal(r.resources.failure, null);
    }
    globalThis.fetch = reply(500, 'text/plain', '');
    let r = renderer(), assets = new RasterHudAssets(r);
    await assert.rejects(assets.ready, /HUD atlas index: 500/);
    assert.match(String(r.resources.failure?.message), /HUD atlas index: 500/);
    globalThis.fetch = reply(200, 'application/json', '{"scale":4,"textures":{}}');
    r = renderer(); assets = new RasterHudAssets(r);
    assert.deepEqual(await assets.ready, { scale: 4, textures: {} });
  } finally { globalThis.fetch = previous; }
});
