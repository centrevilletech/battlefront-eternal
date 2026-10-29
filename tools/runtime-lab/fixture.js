// Synthetic geometry, real movement/collision code. No retail data or renderer.
import { CollisionWorld } from '../../src/collision.js';
import { Soldier } from '../../src/soldier.js';

export function movementFixture() {
  const world = new CollisionWorld(
    new Float32Array([-100, 0, -100, 100, 0, -100, 100, 0, 100, -100, 0, 100]),
    new Uint32Array([0, 2, 1, 0, 3, 2]), new Uint32Array([0, 1]),
  );
  const soldier = new Soldier(world, {});
  soldier.spawn(0, 0, 0, 0);
  return {
    soldier,
    step(command) {
      if (command.look) soldier.look(...command.look);
      soldier.tick(command, 1 / 60);
    },
    snapshot() {
      return { position: soldier.position.toArray(), velocity: soldier.velocity.toArray(),
        yaw: soldier.aimYaw, pitch: soldier.aimPitch, posture: soldier.posture,
        air: soldier.air, grounded: soldier.grounded };
    },
  };
}

export function replayCommands() {
  return Array.from({ length: 360 }, (_, tick) => ({
    forward: tick < 240 ? 1 : 0, right: tick >= 240 && tick < 300 ? .5 : 0,
    turn: tick >= 120 && tick < 180 ? .25 : 0,
    jump: tick === 75, crouch: tick === 300,
    look: tick === 30 ? [14, -3] : tick === 200 ? [-9, 2] : null,
  }));
}
