// Installed by an isolated authority test harness, never by the game. Keep a
// named hull at its ordinary spawn until the human takes it with the use key.
// Its movement, collision, seats and inputs still run through the live game.
export function installMountedSpawnFixture(game, world, name) {
  const acquire = game.factory.acquire, step = world.step, originals = new Map();
  const boarding = game.ai.classic.ai.world.vehicles;
  let target = null;
  const reserve = () => {
    if (target?.alive) return;
    target = game.vehicles.vehicles.filter(v => v.name === name && v.alive)
      .sort((a, b) => a.aiTarget.id - b.aiTarget.id)[0] ?? null;
    if (target) boarding.padLeft.set(target, Infinity);
  };
  game.factory.acquire = function (...args) {
    const unit = acquire.apply(this, args);
    if (!originals.has(unit)) {
      const spawn = unit.spawn;
      originals.set(unit, spawn);
      unit.spawn = function (x, y, z, yaw) {
        if (this.networkHuman && target?.alive && (!target.controllingTeam || target.controllingTeam === this.team)) {
          x = target.position.x + target.radius + 1.5;
          y = target.position.y;
          z = target.position.z;
          yaw = -Math.PI / 2;
        }
        return spawn.call(this, x, y, z, yaw);
      };
    }
    return unit;
  };
  world.step = function (...args) {
    const result = step.apply(this, args);
    reserve();
    return result;
  };
  reserve();
  return {
    get target() { return target; },
    dispose() {
      if (target) boarding.padLeft.delete(target);
      game.factory.acquire = acquire;
      world.step = step;
      for (const [unit, spawn] of originals) unit.spawn = spawn;
    },
  };
}
