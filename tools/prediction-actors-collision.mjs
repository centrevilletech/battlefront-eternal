// Owned geometry for private actor replay. Native tests rebuild each packed
// tree from these exact corners and indices, retaining the kernel's IDs.
export const actorWords = values => {
  const bytes = Buffer.alloc(values.length * 8);
  for (let i = 0; i < values.length; i++) bytes.writeDoubleLE(values[i], i * 8);
  return bytes.toString('hex');
};
const hex = values => Buffer.from(values.buffer, values.byteOffset, values.byteLength).toString('hex');
const u32 = values => hex(Uint32Array.from(values));
export function actorCollisionGeometry(world) {
  const rays = world.rays; if (!rays) throw Error('actor corpus requires native collision');
  world.rayBodies();
  const positions = new Map(), trees = new Map();
  const position = data => {
    const id = rays.positionIds.get(data); if (id === undefined) throw Error('unregistered actor collision positions');
    positions.set(id, { id, wide: data instanceof Float64Array, data: hex(data) }); return id;
  };
  const tree = (bvh, polygons = []) => {
    const id = rays.treeIds.get(bvh); if (id === undefined) throw Error('unregistered actor collision tree');
    const existing = trees.get(id);
    if (!existing || polygons.length) trees.set(id, { id, positions: position(bvh.geometry.attributes.position.array),
      indices: u32(bvh.geometry.index.array), polygons: u32(polygons) });
    return id;
  };
  const primary = tree(world.bvh), terrain = world.terrain ? tree(world.terrain) : null;
  const bodies = world.bodies.map(body => ({ id: body.raySlot, tree: tree(body.bvh, body.polygonOf), moving: !!body.moving, offset: body.offset }));
  const fixed = world.staticSet, set = rays.setIds.get(fixed);
  const sets = [{ id: set, tree: primary, bounds: hex(fixed.triBounds), polygons: u32(fixed.polygonOf), offset: fixed.offset }];
  const grid = world.terrainGrid;
  const grids = grid ? [{ id: world.rayGrid, positions: position(grid.positions), indices: u32(grid.indices),
    cells: u32(grid.cells), start: u32(grid.start), numbers: actorWords([grid.minX, grid.minZ, grid.maxX, grid.maxZ, grid.size, grid.nx, grid.nz]) }] : [];
  return { positions: [...positions.values()], trees: [...trees.values()], sets, bodies, grids,
    world: { id: world.rayWorld, tree: primary, terrain, grid: grid ? world.rayGrid : null, statics: set, order: bodies.map(b => b.id) } };
}
export function actorCollisionBodies(world) {
  return world.bodies.map(body => ({ id: body.raySlot, words: actorWords([
    ...body.bounds.min.toArray(), ...body.bounds.max.toArray(),
    ...(body.matrix?.toArray() || new Array(16).fill(0)), ...(body.inverse?.toArray() || new Array(16).fill(0)),
  ]) }));
}
export function actorTraceRecord(event, world, label) {
  return { type: 'operation', label, session: String(event.session), op: event.op, input: actorWords(event.input), output: actorWords(event.output),
    error: event.error ?? null, bodies: actorCollisionBodies(world) };
}
