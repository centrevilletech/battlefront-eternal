// Node-only synthetic worlds share a kernel unless a fixture supplies its own.
// The game supplies the kernel on its battle's simulation instance explicitly.
import { CollisionWorld as World } from '../src/collision.js';
import { loadRayKernel } from '../src/ray-kernel.js';
const rays = await loadRayKernel();
export function CollisionWorld(positions, indices, polygons, terrainPositions, terrainIndices, kernel = rays) {
  return new World(positions, indices, polygons, terrainPositions, terrainIndices, kernel ?? rays);
}
CollisionWorld.prototype = World.prototype;
