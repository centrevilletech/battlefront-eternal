import assert from 'node:assert/strict';
import test from 'node:test';
import { CONTENT_VERSION, preparedContentCurrent } from '../electron/prepare.js';
import { startupContent } from '../electron/startup-content.js';

const current = { version: CONTENT_VERSION.state, mapImportVersion: CONTENT_VERSION.maps, muzzleVersion: CONTENT_VERSION.sides, commonVersion: CONTENT_VERSION.common, complete: true, failed: [], skipped: [], game: '/steam/GameData/Data/_LVL_PC', addOns: ['tat3'] };
function setup({ chosen, states = {}, install = null, found = null, addOns = ['tat3'] } = {}) {
  return { chosen, managed: '/managed', hasGameData: dir => dir in states, readState: async dir => states[dir], findInstall: async () => install, findContent: async () => found && ({ dir: found }), installedAddOns: async () => addOns };
}

test('a generation-8 import is repaired automatically from Steam before opening the menu', async () => {
  const install = { dir: '/steam' };
  const result = await startupContent(setup({ chosen: '/imported', states: { '/imported': { ...current, mapImportVersion: CONTENT_VERSION.maps - 1, game: '/another-computer/GameData/Data/_LVL_PC' } }, install }));
  assert.deepEqual(result, { mode: 'prepare', content: '/imported', install, selected: true });
});

test('current portable content works offline regardless of its source path', async () => {
  const options = setup({ chosen: '/imported', states: { '/imported': { ...current, game: 'C:\\Other PC\\GameData\\Data\\_LVL_PC' } } });
  options.findInstall = async () => { throw Error('a current import does not need Steam'); };
  assert.deepEqual(await startupContent(options), { mode: 'menu', content: '/imported' });
});

test('old, incomplete and unknown imports require repair or a current export', async () => {
  for (const state of [null, {}, { ...current, version: 0 }, { ...current, mapImportVersion: 0 }, { ...current, muzzleVersion: 0 }, { ...current, commonVersion: 1 }, { ...current, complete: false }, { ...current, failed: ['map:hot1'] }, { ...current, skipped: ['textures'] }]) {
    assert.equal(preparedContentCurrent(state), false);
    assert.equal((await startupContent(setup({ chosen: '/imported', states: { '/imported': state } }))).mode, 'outdated');
    assert.equal((await startupContent(setup({ chosen: '/imported', states: { '/imported': state }, install: { dir: '/steam' } }))).mode, 'prepare');
  }
});

test('managed preparation resumes after interruption and rechecks installed add-ons', async () => {
  const options = setup({ states: { '/managed': current }, install: { dir: '/steam' } });
  assert.equal((await startupContent(options)).mode, 'menu');
  options.installedAddOns = async () => ['tat3', 'new-map'];
  assert.equal((await startupContent(options)).mode, 'prepare');
  options.readState = async () => ({ ...current, complete: false });
  assert.equal((await startupContent(options)).mode, 'prepare');
});

test('managed and discovered folders cannot bypass freshness when Steam is missing', async () => {
  for (const content of ['/managed', '/found']) {
    for (const [state, mode] of [[current, 'menu'], [{ ...current, mapImportVersion: 0 }, 'outdated']]) {
      assert.deepEqual(await startupContent(setup({ states: { [content]: state }, found: content })), { mode, content });
    }
  }
  assert.deepEqual(await startupContent(setup()), { mode: 'missing' });
});

test('a removed selected folder falls back to managed preparation', async () => {
  assert.equal((await startupContent(setup({ chosen: '/removed', states: { '/managed': current }, install: { dir: '/steam' } }))).mode, 'menu');
});
