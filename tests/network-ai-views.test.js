import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { PerspectiveCamera, Vector3 } from 'three';
import { contentPath } from './helpers/content.js';
import { AiPhaseHeaderLayout, AiPhaseCameraLayout } from '../src/sim/gen/ai-phase.js';

const content = contentPath();
// Read the AI's generated input block after the native players phase. The
// host has no camera list to update or camera objects to retain any more.
test('native AI views follow every living authoritative human and exclude departures and spectators',
  { skip: !existsSync(`${content}/maps/nab2/map.json`) }, async t => {
    t.mock.method(performance, 'now', () => 1000);
    process.env.BFE_CONTENT_DIR ||= content;
    const { headlessGame } = await import('../tools/headless-world.mjs');
    const { createMultiplayerWorld } = await import('../src/runtime/multiplayer-world.js');
    const game = await headlessGame('?map=nab2&side=rep&ai=classic&seed=1');
    const world = createMultiplayerWorld(game);
    for (const id of ['first', 'second']) await world.addPlayer(id, { team: 1 });
    await world.addPlayer('watch', { team: 1, spectator: true });
    const first = world.players.get('first').unit, second = world.players.get('second').unit;
    game.unitsSim.placeBody(first, 0, 0, 0, 0);
    game.unitsSim.placeBody(second, 50, 0, 0, Math.PI);
    function cameras() {
      const { bridge: b, layout } = game.ai.owner; b.views();
      const count = b.f[layout.header / 8 + AiPhaseHeaderLayout.cameras];
      return Array.from({ length: count }, (_, index) => Array.from(b.f.slice(
        layout.cameras / 8 + index * AiPhaseCameraLayout.words,
        layout.cameras / 8 + index * AiPhaseCameraLayout.words + 16)));
    }
    const expected = unit => {
      const camera = new PerspectiveCamera(100, 4, .1, 2000);
      const { aimYaw: yaw, aimPitch: pitch } = unit.soldier;
      camera.position.copy(unit.position); camera.position.y += 1.7;
      camera.lookAt(camera.position.clone().add(new Vector3(
        Math.sin(yaw) * Math.cos(pitch), Math.sin(pitch), Math.cos(yaw) * Math.cos(pitch))));
      camera.updateMatrixWorld(true); return camera.matrixWorld.toArray();
    };
    const before = [expected(first), expected(second)];
    world.step(1000);
    assert.equal(game.ai.worldData.networkViews, true);
    assert.deepEqual(cameras(), before);
    game.unitsSim.placeBody(second, 50, 0, 0, 0);
    const turned = expected(second);
    world.step(1000); assert.deepEqual(cameras()[1], turned);
    world.removePlayer('second'); world.step(1000); assert.equal(cameras().length, 1);
    first.combat.damage(Infinity); world.step(1000); assert.equal(cameras().length, 0);
  });
