import * as THREE from "three";
import { lighting } from "./materials.js";

const DECLARATIONS = /* glsl */ `
  uniform float fabricTime;
  attribute vec2 fabricAnchor;
  float fabricOffset(vec3 p) {
    float loose = clamp((fabricAnchor.x - p.y) / fabricAnchor.y, 0.0, 1.0);
    float wave = sin(p.y * 1.7 + p.x * 0.23 + p.z * 0.19 - fabricTime * 2.1);
    wave += 0.3 * sin(p.y * 3.9 + p.x * 0.41 - fabricTime * 3.7);
    return 0.13 * loose * loose * wave;
  }
  vec3 fabricGradient(vec3 p) {
    const float e = 0.01;
    return vec3(
      fabricOffset(p + vec3(e,0,0)) - fabricOffset(p - vec3(e,0,0)),
      fabricOffset(p + vec3(0,e,0)) - fabricOffset(p - vec3(0,e,0)),
      fabricOffset(p + vec3(0,0,e)) - fabricOffset(p - vec3(0,0,e))
    ) / (2.0 * e);
  }
`;
const DIRECTION = "vec3(0.91, 0.0, 0.41)";

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

function patch(material, normal) {
  const previous = material.onBeforeCompile;
  const beforeRender = material.onBeforeRender;
  material.onBeforeRender = function(...args) {
    beforeRender.apply(this, args);
    // Three can retain the current program between draws/frames. Update the
    // clock even if this is the only visible material in a reflection/view.
    if (this.isShaderMaterial) this.uniformsNeedUpdate = true;
  };
  material.onBeforeCompile = function(shader, renderer) {
    previous.call(this, shader, renderer);
    shader.uniforms.fabricTime = lighting.time;
    shader.vertexShader = DECLARATIONS + shader.vertexShader;
    shader.vertexShader = shader.vertexShader.replace("#include <begin_vertex>", `#include <begin_vertex>\ntransformed += ${DIRECTION} * fabricOffset(position);`);
    if (normal) shader.vertexShader = shader.vertexShader.replace("#include <beginnormal_vertex>", `#include <beginnormal_vertex>
      vec3 fabricGrad = fabricGradient(position);
      objectNormal = normalize(objectNormal - fabricGrad * dot(objectNormal, ${DIRECTION}) / (1.0 + dot(fabricGrad, ${DIRECTION})));`);
  };
  material.customProgramCacheKey = () => `fabric-v1-${normal}`;
  material.needsUpdate = true;
}

export function addFabricWind(mesh) {
  const original = mesh.geometry;
  mesh.geometry = fabricGeometry(original);
  original.dispose();
  patch(mesh.material, true);
  mesh.customDepthMaterial = new THREE.MeshDepthMaterial({ depthPacking: THREE.RGBADepthPacking, map: mesh.material.map, alphaTest: mesh.material.alphaTest, side: mesh.material.side });
  patch(mesh.customDepthMaterial, false);
  mesh.userData.fabricWind = true;
  return mesh;
}
