// Node-only synthetic worlds share a kernel unless a fixture supplies its own.
// The game supplies the kernel on its battle's simulation instance explicitly.
import { CollisionWorld as World } from '../src/collision.js';
import { loadRayKernel } from '../src/ray-kernel.js';
const rays = await loadRayKernel();
export class CollisionWorld extends World {
  constructor(positions, indices, polygons, terrainPositions, terrainIndices, kernel = rays) {
    super(positions, indices, polygons, terrainPositions, terrainIndices, kernel ?? rays);
  }
}
