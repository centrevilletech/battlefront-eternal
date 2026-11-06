// Node-only synthetic worlds share a kernel unless a fixture supplies its own.
// The game supplies the kernel on its battle's simulation instance explicitly.
import { CollisionWorld as World } from '../src/collision.js';
import { RayKernel } from '../src/ray-kernel.js';
import { simModule } from '../src/sim/module.js';
import { IMPORT_SYMBOLS } from '../src/sim/gen/imports.js';
const module = await simModule();
export function CollisionWorld(positions, indices, polygons, terrainPositions, terrainIndices, kernel) {
  // A phase owns a battle's module. Separate fixtures cannot share canonical
  // fleet/map globals merely because their ray worlds have different IDs.
  let instance = null;
  if (!kernel) {
    const holder = { bridge: null }, bfe = {};
    for (const imp of IMPORT_SYMBOLS) if (imp) bfe[imp.symbol] = () => holder.bridge.serve(imp.code);
    instance = new WebAssembly.Instance(module, { bfe }); instance.holder = holder;
  }
  const world = new World(positions, indices, polygons, terrainPositions, terrainIndices, kernel ?? new RayKernel(instance.exports));
  world.fixtureInstance = instance;
  return world;
}
CollisionWorld.prototype = World.prototype;
