import * as THREE from "three";
// Discover separate hanging pieces inside the importer's material bucket.
// Weld only for connectivity: UV/normal seams stay intact in the draw data.
export function fabricGeometry(source) {
  const position = source.attributes.position;
  const parent = Array.from({ length: position.count }, (_, i) => i);
  const root = (i) => { while (parent[i] !== i) { parent[i] = parent[parent[i]]; i = parent[i]; } return i; };
  const join = (a, b) => { parent[root(a)] = root(b); };
  const welded = new Map();
  for (let i = 0; i < position.count; i++) {
    const key = [position.getX(i), position.getY(i), position.getZ(i)].map(v => Math.round(v * 1000)).join(",");
    if (welded.has(key)) join(i, welded.get(key)); else welded.set(key, i);
  }
  let indices = source.index ? Array.from(source.index.array) : Array.from(parent.keys());
  for (let i = 0; i < indices.length; i += 3) { join(indices[i], indices[i + 1]); join(indices[i], indices[i + 2]); }
  const bounds = new Map();
  for (let i = 0; i < position.count; i++) {
    const id = root(i), y = position.getY(i), b = bounds.get(id) || [Infinity, -Infinity];
    b[0] = Math.min(b[0], y); b[1] = Math.max(b[1], y); bounds.set(id, b);
  }
  const attributes = {};
  for (const [name, attribute] of Object.entries(source.attributes)) {
    const values = [];
    for (let i = 0; i < attribute.count; i++) for (let c = 0; c < attribute.itemSize; c++) values.push(attribute.getComponent(i, c));
    attributes[name] = { size: attribute.itemSize, values };
  }
  attributes.fabricAnchor = { size: 2, values: parent.flatMap((_, i) => {
    const b = bounds.get(root(i)); return [b[1], Math.max(0.01, b[1] - b[0])];
  }) };
  // Two subdivisions give the original long quads enough interior vertices
  // to bend. This happens once, only for explicit banner/cloth materials.
  for (let level = 0; level < 2; level++) {
    const cache = new Map(), next = [];
    const midpoint = (a, b) => {
      const key = a < b ? `${a}:${b}` : `${b}:${a}`;
      if (cache.has(key)) return cache.get(key);
      const index = attributes.position.values.length / 3;
      for (const { size, values } of Object.values(attributes)) for (let c = 0; c < size; c++) values.push((values[a * size + c] + values[b * size + c]) * 0.5);
      cache.set(key, index); return index;
    };
    for (let i = 0; i < indices.length; i += 3) {
      const [a, b, c] = indices.slice(i, i + 3), ab = midpoint(a, b), bc = midpoint(b, c), ca = midpoint(c, a);
      next.push(a, ab, ca, ab, b, bc, ca, bc, c, ab, bc, ca);
    }
    indices = next;
  }
  const geometry = new THREE.BufferGeometry();
  for (const [name, { size, values }] of Object.entries(attributes)) geometry.setAttribute(name, new THREE.Float32BufferAttribute(values, size));
  geometry.setIndex(indices);
  geometry.computeBoundingBox(); geometry.boundingBox.expandByScalar(0.18);
  geometry.computeBoundingSphere(); geometry.boundingSphere.radius += 0.18;
  return geometry;
}

export function addFabricWind(mesh) {
  const original = mesh.geometry;
  mesh.geometry = fabricGeometry(original);
  original.dispose();
  const depth = new THREE.Material();
  depth.name = "depth:fabric";
  depth.map = mesh.material.map;
  depth.alphaTest = mesh.material.alphaTest;
  depth.side = mesh.material.side;
  depth.userData.bfeProgram = depth.alphaTest>0 ? "actors/depth-fabric-cutout" : "actors/depth-fabric";
  mesh.customDepthMaterial = depth;
  mesh.userData.fabricWind = true;
  return mesh;
}
