import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { headlessGame } from '../tools/headless-world.mjs';
import { createMultiplayerWorld } from '../src/runtime/multiplayer-world.js';
import { createSnapshotHost } from '../src/runtime/snapshot-host.js';
import { SnapshotSlotTable, snapshotFromRecords } from '../src/runtime/snapshot-records.js';
import { encodeEntities } from '../src/runtime/snapshot-codec.js';
import { createWireBridge } from '../src/runtime/wire-sim.js';
import { createStoreSource } from '../src/runtime/wire-store-source.js';

const content = process.env.BFE_CONTENT_DIR || new URL('../public', import.meta.url).pathname;
for (const map of ['nab2', 'tat1', 'bes1', 'hot1', 'kam1']) {
  test(`canonical store records and retained codec agree on entity bytes on ${map}`, { skip: !existsSync(`${content}/maps/${map}/map.json`) }, async () => {
    const game = await headlessGame(`?map=${map}&side=rep&ai=classic&seed=1`);
    const world = createMultiplayerWorld(game, { armySize: 32 });
    await world.addPlayer('probe', { reliableFire: true, separateActions: true });
    world.players.get('probe').autoRespawn = true;
    const host = createSnapshotHost({ game, world }); host.addClient('probe');
    const registry = host.registry, x = game.simBridge.x;
    const shots = x.wire_shots_create(256);
    const unlisten = game.simEvents.listen(event => {
      const source = event.source;
      if (event.kind === 'shot' && source?.unit) x.wire_shots_observe(shots, source.unit.id, source.generation, source.unit.generation);
    });
    const source = createStoreSource(game, registry, shots);
    const native = createWireBridge(game.simBridge); native.start();
    const slots = new SnapshotSlotTable(); let previous = null;
    try {
      for (let frame = 1; frame <= 900; frame++) {
        const unit = world.players.get('probe').unit;
        if (frame === 150 && unit.alive) {
          const turret = game.turretSeats?.turrets.find(t => game.turretSeats.canMan(unit, t));
          if (turret) game.turretSeats.man(unit, turret);
        }
        if (frame === 300 && unit.seat?.turret) game.turretSeats.leave(unit, { placement: true });
        if (frame === 400 && unit.alive && !unit.seat) {
          for (const vehicle of game.vehicles?.vehicles ?? []) {
            if (!vehicle.alive) continue;
            const seat = vehicle.seats.findIndex((s, index) => vehicle.seatFree(index));
            if (seat >= 0 && game.vehicles.board(unit, vehicle, seat)) break;
          }
        }
        if (frame === 650 && unit.alive) unit.combat.damage(Infinity);
        if (frame === 450) x.memory.grow(1);
        world.step(frame * 1000 / 60);
        if (frame % 2) continue;
        const tick = game.worldClock.tick;
        const result = source.probe();
        const snapshot = snapshotFromRecords({ tick, epoch: 1, records: result.records, slots, previous });
        assert.equal(result.allocations, 0, `source allocations ${tick}`);
        assert.equal(result.reallocations, 0, `source reallocations ${tick}`);
        const captured = native.capture({ tick, epoch: 1, records: result.records, world: new Map() });
        for (const baseline of [null, previous]) {
          assert.deepEqual(native.encodeHeld(captured, baseline), encodeEntities(snapshot, baseline), `entity bytes ${tick}, base ${baseline?.tick ?? 0}`);
        }
        previous = snapshot;
      }
    } finally { native.dispose(); source.dispose(); unlisten(); x.wire_shots_drop(shots); host.dispose(); }
  });
}
