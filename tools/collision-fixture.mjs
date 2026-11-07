// Synthetic worlds retain their own native collision and camera owners.
// The game supplies the kernel on its battle's simulation instance explicitly.
import { CollisionWorld as World } from '../src/collision.js';
import { RayKernel } from '../src/sim/wasm/ray-kernel.js';
import { simModule, simImports } from '../src/sim/module.js';
const module = await simModule();
export function CollisionWorld(positions, indices, polygons, terrainPositions, terrainIndices, kernel) {
  // A phase owns a battle's module. Separate fixtures cannot share canonical
  // fleet/map globals merely because their ray worlds have different IDs.
  let instance = null;
  if (!kernel) {
    const holder = { bridge: null };
    instance = new WebAssembly.Instance(module, simImports(holder)); instance.holder = holder;
  }
  const world = new World(positions, indices, polygons, terrainPositions, terrainIndices, kernel ?? new RayKernel(instance.exports));
  world.fixtureInstance = instance;
  return world;
}
CollisionWorld.prototype = World.prototype;
