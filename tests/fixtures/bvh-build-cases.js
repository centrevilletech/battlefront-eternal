import { BufferAttribute, BufferGeometry } from 'three';
import { MeshBVH } from '../../src/mesh-bvh.js';

export function geometry(positions, indices) {
  const g = new BufferGeometry();
  g.setAttribute('position', new BufferAttribute(positions, 3));
  g.setIndex(new BufferAttribute(indices, 1));
  return g;
}
export function referenceTree(positions, indices) { return new MeshBVH(geometry(positions, indices), { indirect: true, verbose: false }); }
const random = initial => { let seed = initial; return () => ((seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0) / 4294967296); };
function soup(count, seed, wide = false) {
  const u = random(seed), positions = new (wide ? Float64Array : Float32Array)(count * 9), indices = new Uint32Array(count * 3);
  for (let triangle = 0; triangle < count; triangle++) {
    const center = [(u() - 0.5) * 200, (u() - 0.5) * 20, (u() - 0.5) * 200];
    for (let corner = 0; corner < 3; corner++) {
      indices[triangle * 3 + corner] = triangle * 3 + corner;
      for (let axis = 0; axis < 3; axis++) positions[triangle * 9 + corner * 3 + axis] = center[axis] + (u() - 0.5) * 10;
    }
  }
  return { label: `soup-${count}-${seed}-${wide ? 64 : 32}`, positions, indices };
}
function points(label, xyz, wide = false) {
  return { label, positions: new (wide ? Float64Array : Float32Array)(xyz.flat()), indices: Uint32Array.from({ length: xyz.length * 3 }, (_, i) => Math.floor(i / 3)) };
}
export function* builderCases({ large = true } = {}) {
  yield { label: 'empty32', positions: new Float32Array(), indices: new Uint32Array() };
  yield { label: 'empty64-with-unused-corners', positions: new Float64Array([1, 2, 3]), indices: new Uint32Array() };
  yield points('signed-zero', [[-0, 0, -0], [0, -0, 0]]);
  yield points('tiny32', [[1e-45, -1e-45, -0], [-1e-44, 1e-44, 0], [0, 0, 0]]);
  yield points('extreme32', [[3.4028234663852886e38, -3.4028234663852886e38, 1], [-3.4028234663852886e38, 3.4028234663852886e38, -1]]);
  yield points('double-rounding', Array.from({ length: 39 }, (_, i) => [16777216 + i * 0.25, 2 ** -140 * i, -0]), true);
  yield points('axis-ties', Array.from({ length: 45 }, (_, i) => [i % 5, i % 5, i % 5]));
  yield points('partition-plane', Array.from({ length: 45 }, (_, i) => [i % 3 - 1, 0, 0]));
  yield points('max-depth', Array.from({ length: 110 }, (_, i) => [2 ** i, 0, 0]), true);
  for (const count of [1, 2, 9, 10, 11, 19, 20, 21, 255, 256, 257, 800]) yield soup(count, 103 + count, count % 2 === 0);
  for (let seed = 1; seed <= 80; seed++) yield soup((seed * 137) % 1200 + 11, seed, seed % 3 === 0);
  if (large) for (const count of [65535, 65536, 65537]) {
    yield { label: `degenerate-${count}`, positions: new Float64Array([-0, 0, -0]), indices: new Uint32Array(count * 3) };
    yield soup(count, count, false);
  }
}

export function recordFor({ label, positions, indices }, tree = referenceTree(positions, indices)) {
  const hex = array => Buffer.from(array.buffer, array.byteOffset, array.byteLength).toString('hex');
  return { label, wide: positions instanceof Float64Array, positions: hex(positions), indices: hex(indices),
    roots: tree._roots.map(root => hex(new Uint8Array(root))), indirect: hex(Uint32Array.from(tree._indirectBuffer)), width: tree._indirectBuffer.BYTES_PER_ELEMENT };
}
