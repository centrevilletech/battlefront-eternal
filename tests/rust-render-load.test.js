import test from 'node:test';
import assert from 'node:assert/strict';
import { loadRenderer, rendererFailure } from '../src/rust-render/failure.js';

const renderer = (debug = true) => ({ getContext: () => ({
  RENDERER: 1,
  getExtension: () => debug ? { UNMASKED_RENDERER_WEBGL: 2 } : null,
  getParameter: (key) => key === 2 ? 'Fixture GPU' : 'WebGL GPU',
}) });

test('required renderer loading preserves the loaded renderer', async () => {
  const graphics = { renderer: renderer() }, core = {};
  assert.equal(await loadRenderer(graphics, async (g) => { assert.equal(g, graphics); return core; }), core);
});

test('module loading failure rejects with the GPU and original cause', async () => {
  const cause = new Error('module fetch failed');
  await assert.rejects(loadRenderer({ renderer: renderer() }, async () => { throw cause; }), (error) => {
    assert.equal(error.message, 'Battle renderer failed on Fixture GPU: module fetch failed');
    assert.equal(error.cause, cause);
    return true;
  });
});

test('GPU identification works when driver details are unavailable', () => {
  assert.match(rendererFailure(renderer(false), new Error('link failed')).message, /WebGL GPU: link failed/);
  assert.match(rendererFailure({ getContext() { throw Error('unavailable'); } }, 'load failed').message, /unknown GPU: load failed/);
});
