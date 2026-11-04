import test from 'node:test';
import assert from 'node:assert/strict';
import { Graphics } from '../src/graphics.js';

test('renderer preparation waits for the bridge and settles both adoption draws', async () => {
  const events = [];
  let ready;
  const graphics = {
    rustReady: new Promise(resolve => { ready = resolve; }),
    render(time) { events.push(['scene', time]); },
  };
  let settle;
  const prepared = Graphics.prototype.prepareRenderer.call(graphics,
    () => { events.push('settle'); return new Promise(resolve => { settle = resolve; }); },
    () => events.push('hud'));
  assert.deepEqual(events, []);
  ready();
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(events, [['scene', 0], 'hud', 'settle']);
  settle();
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(events, [['scene', 0], 'hud', 'settle', ['scene', 0], 'hud', 'settle']);
  let finished = false;
  prepared.then(() => { finished = true; });
  await Promise.resolve();
  assert.equal(finished, false, 'the session cannot start while GPU preparation is outstanding');
  settle();
  await prepared;
  assert.equal(finished, true);
});

test('renderer preparation propagates a draw failure rather than admitting a session', async () => {
  await assert.rejects(Graphics.prototype.prepareRenderer.call({
    rustReady: Promise.resolve(), render() { throw Error('draw failed'); },
  }, () => {}, () => {}), /draw failed/);
});
