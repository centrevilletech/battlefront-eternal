// World variants are selected from explicit material and scene features. Source
// discovery and nearest-variant substitution are never part of registration.
import { programLayouts } from '../render-programs.js';
import { ownedGeometryProgram } from './programs.js';

export function ownWorldMaterial(material, geometry) {
  material.userData.bfeFabric = Boolean(geometry?.attributes.fabricAnchor);
  material.userData.bfeTree = Boolean(material.uniforms.treeStates);
  material.userData.bfeFamily = 'world';
  material.isShaderMaterial = false;
  material.type = 'Material';
  delete material.vertexShader;
  delete material.fragmentShader;
  delete material.onBeforeCompile;
}

// Three's loader still compiles unported families, including depth materials.
// Owned material shells supply resource values but never enter that compiler.
export function compileUnowned(renderer, object, camera, scene) {
  const removed = [];
  object.traverse(o => {
    if (o.material?.userData?.bfeFamily === 'world') { removed.push([o, o.material]); o.material = null; }
  });
  try { return renderer.compile(object, camera, scene); }
  finally { for (const [o, material] of removed) o.material = material; }
}

export function ownedWorldProgram(core, material, scene = core.graphics.scene, camera = core.graphics.camera, side = material.side) {
  const entries = core.worldMaterialPrograms ||= new WeakMap();
  const existing = entries.get(material);
  if (existing && existing.version === material.version && existing.scene === scene && existing.side === side) return existing.program;
  const defines = Object.entries(material.defines).map(([k,v]) => [k,String(v)]).sort(([a],[b]) => (a < b ? -1 : a > b ? 1 : 0));
  let points = 0, shadows = 0;
  scene.traverseVisible(o => {
    if (!o.layers.test(camera.layers)) return;
    if (o.isPointLight) points++;
    if (o.isDirectionalLight && o.castShadow && core.renderer.shadowMap.enabled) shadows++;
  });
  const csm = Boolean(core.graphics.csm);
  const lamps = Boolean(core.graphics.settings.get("lampLighting")) && material.defines.DYNAMIC !== undefined && material.defines.NO_POINT === undefined && material.defines.FAR === undefined;
  const signature = JSON.stringify({ defines, side, shadows, points, csm, fabric: Boolean(material.userData.bfeFabric), tree: Boolean(material.userData.bfeTree), map: Boolean(material.map), lamps });
  const matches = Object.entries(programLayouts).filter(([key, p]) => key.startsWith('world/') && p.selector && JSON.stringify(p.selector) === signature);
  if (matches.length !== 1) throw Error(`Unowned world variant (${matches.length} matches): ${signature}`);
  const [key, layout] = matches[0], program = ownedGeometryProgram(core, key);
  program.names = new Set(layout.inputs.map(u => u.name.replace(/\[0\]$/, '')));
  program.inputs = layout.inputs;
  program.owned = true;
  program.lamps = layout.lamps;
  program.aliased = Boolean(material.userData.bfeFabric);
  entries.set(material, { version: material.version, scene, side, program });
  return program;
}
